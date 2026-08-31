#![forbid(unsafe_code)]
#![warn(missing_docs)]
//! OpenFeature provider for the Featurevisor Rust SDK.
//!
//! The provider is intentionally published separately from the base
//! `featurevisor` crate. Applications that do not use OpenFeature therefore do
//! not compile or link the OpenFeature SDK and its asynchronous runtime.
//!
//! ```
//! use featurevisor_openfeature::{FeaturevisorProvider, FeaturevisorProviderOptions};
//! use open_feature::{provider::FeatureProvider, EvaluationContext};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let provider = FeaturevisorProvider::new(FeaturevisorProviderOptions::default())?;
//! let result = provider
//!     .resolve_bool_value("checkout", &EvaluationContext::default())
//!     .await;
//! assert!(result.is_err());
//! # Ok(())
//! # }
//! ```

use featurevisor::{
    create_featurevisor, AttributeValue, Context, DatafileContent, DatafileInput, Evaluation,
    EvaluationReason as FeaturevisorReason, EventDetails, EventName, Featurevisor,
    FeaturevisorOptions, Unsubscribe, VariableValue,
};
use open_feature::provider::{FeatureProvider, ProviderMetadata, ResolutionDetails};
use open_feature::{
    async_trait, EvaluationContext, EvaluationContextFieldValue, EvaluationError,
    EvaluationErrorCode, EvaluationReason, EvaluationResult, FlagMetadata, StructValue, Value,
};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::sync::{Arc, Mutex};
use time::format_description::well_known::Rfc3339;

const PROVIDER_NAME: &str = "Featurevisor";

/// Configuration for [`FeaturevisorProvider`].
pub struct FeaturevisorProviderOptions {
    /// An existing Featurevisor instance. The provider borrows this instance
    /// and does not close it.
    pub featurevisor: Option<Featurevisor>,
    /// Options used when the provider creates and owns a Featurevisor instance.
    pub featurevisor_options: FeaturevisorOptions,
    /// Featurevisor context field that receives the OpenFeature targeting key.
    pub targeting_key_field: String,
    /// Separator between a feature key and a variation or variable selector.
    pub key_separator: String,
    /// Selector reserved for feature variation evaluation.
    pub variation_key: String,
    /// Prefix reserved for global variable evaluation.
    pub global_variable_prefix: String,
}

impl Default for FeaturevisorProviderOptions {
    fn default() -> Self {
        Self {
            featurevisor: None,
            featurevisor_options: FeaturevisorOptions::default(),
            targeting_key_field: "userId".to_string(),
            key_separator: ":".to_string(),
            variation_key: "variation".to_string(),
            global_variable_prefix: "variable".to_string(),
        }
    }
}

/// An invalid provider configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderConfigurationError {
    message: String,
}

impl ProviderConfigurationError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for ProviderConfigurationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ProviderConfigurationError {}

/// OpenFeature provider backed by a Featurevisor SDK instance.
pub struct FeaturevisorProvider {
    featurevisor: Featurevisor,
    metadata: ProviderMetadata,
    targeting_key_field: String,
    key_separator: String,
    variation_key: String,
    global_variable_prefix: String,
    datafile_error: Arc<Mutex<Option<String>>>,
    subscriptions: Mutex<Vec<Unsubscribe>>,
    owns_featurevisor: bool,
}

impl FeaturevisorProvider {
    /// Creates a provider from provider and Featurevisor options.
    pub fn new(options: FeaturevisorProviderOptions) -> Result<Self, ProviderConfigurationError> {
        if options.key_separator.is_empty() {
            return Err(ProviderConfigurationError::new(
                "keySeparator cannot be empty",
            ));
        }
        if options.global_variable_prefix.is_empty() {
            return Err(ProviderConfigurationError::new(
                "globalVariablePrefix cannot be empty",
            ));
        }
        if options
            .global_variable_prefix
            .contains(&options.key_separator)
        {
            return Err(ProviderConfigurationError::new(
                "globalVariablePrefix cannot contain keySeparator",
            ));
        }

        let owns_featurevisor = options.featurevisor.is_none();
        let initial_error = if owns_featurevisor {
            initial_datafile_error(&options.featurevisor_options)
        } else {
            None
        };
        let featurevisor = options
            .featurevisor
            .unwrap_or_else(|| create_featurevisor(options.featurevisor_options));
        let datafile_error = Arc::new(Mutex::new(initial_error));
        let subscriptions = subscribe_to_datafile_state(&featurevisor, &datafile_error);

        Ok(Self {
            featurevisor,
            metadata: ProviderMetadata::new(PROVIDER_NAME),
            targeting_key_field: options.targeting_key_field,
            key_separator: options.key_separator,
            variation_key: options.variation_key,
            global_variable_prefix: options.global_variable_prefix,
            datafile_error,
            subscriptions: Mutex::new(subscriptions),
            owns_featurevisor,
        })
    }

    /// Creates a provider that borrows an existing Featurevisor instance.
    pub fn from_featurevisor(
        featurevisor: Featurevisor,
    ) -> Result<Self, ProviderConfigurationError> {
        Self::new(FeaturevisorProviderOptions {
            featurevisor: Some(featurevisor),
            ..Default::default()
        })
    }

    /// Returns the Featurevisor instance used by the provider.
    pub fn featurevisor(&self) -> &Featurevisor {
        &self.featurevisor
    }

    /// Releases provider subscriptions and closes an owned Featurevisor instance.
    ///
    /// The operation is idempotent. A borrowed Featurevisor instance is never
    /// closed by the provider.
    pub fn close(&self) {
        if let Ok(mut subscriptions) = self.subscriptions.lock() {
            for unsubscribe in subscriptions.drain(..) {
                unsubscribe();
            }
        }
        if self.owns_featurevisor {
            self.featurevisor.close();
        }
    }

    fn resolve(&self, flag_key: &str, context: &EvaluationContext) -> EvaluationResult<Resolved> {
        if let Some(message) = self
            .datafile_error
            .lock()
            .ok()
            .and_then(|error| error.clone())
        {
            return Err(evaluation_error(EvaluationErrorCode::ParseError, message));
        }

        let context = featurevisor_context(context, &self.targeting_key_field)?;
        let (feature_key, selector) = split_key(flag_key, &self.key_separator);
        let evaluation = if feature_key == self.global_variable_prefix && selector.is_some() {
            self.featurevisor.evaluate_global_variable(
                selector.unwrap_or_default(),
                Some(&context),
                None,
            )
        } else if selector.is_none() {
            self.featurevisor.evaluate_flag(feature_key, Some(&context))
        } else if selector == Some(self.variation_key.as_str()) {
            self.featurevisor
                .evaluate_variation(feature_key, Some(&context), None)
        } else {
            self.featurevisor.evaluate_variable(
                feature_key,
                selector.unwrap_or_default(),
                Some(&context),
                None,
            )
        };

        if let Some(error) = error_for_evaluation(&evaluation) {
            return Err(error);
        }

        let value = if selector.is_none() {
            evaluation.enabled.map(ResolvedValue::Bool)
        } else if selector == Some(self.variation_key.as_str()) {
            evaluation
                .variation_value
                .clone()
                .or_else(|| {
                    evaluation
                        .variation
                        .as_ref()
                        .map(|value| value.value.clone())
                })
                .map(ResolvedValue::String)
        } else {
            evaluation
                .variable_value
                .clone()
                .map(|value| normalize_variable(value, variable_type(&evaluation)))
                .map(ResolvedValue::Variable)
        };

        Ok(Resolved { evaluation, value })
    }

    fn details<T>(&self, evaluation: &Evaluation, value: T) -> ResolutionDetails<T> {
        let mut details = ResolutionDetails::new(value);
        details.reason = Some(reason_for(evaluation.reason.clone()));
        details.flag_metadata = Some(metadata_for(evaluation, &self.featurevisor));
        details.variant = evaluation.variation_value.clone().or_else(|| {
            evaluation
                .variation
                .as_ref()
                .map(|value| value.value.clone())
        });
        details
    }

    fn type_mismatch(&self, flag_key: &str, expected: &str) -> EvaluationError {
        evaluation_error(
            EvaluationErrorCode::TypeMismatch,
            format!("Flag \"{flag_key}\" did not resolve to a {expected} value"),
        )
    }
}

impl Drop for FeaturevisorProvider {
    fn drop(&mut self) {
        self.close();
    }
}

#[async_trait]
impl FeatureProvider for FeaturevisorProvider {
    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn resolve_bool_value(
        &self,
        flag_key: &str,
        evaluation_context: &EvaluationContext,
    ) -> EvaluationResult<ResolutionDetails<bool>> {
        let resolved = self.resolve(flag_key, evaluation_context)?;
        match resolved.value {
            Some(ResolvedValue::Bool(value)) => Ok(self.details(&resolved.evaluation, value)),
            Some(ResolvedValue::Variable(VariableValue::Boolean(value))) => {
                Ok(self.details(&resolved.evaluation, value))
            }
            _ => Err(self.type_mismatch(flag_key, "boolean")),
        }
    }

    async fn resolve_int_value(
        &self,
        flag_key: &str,
        evaluation_context: &EvaluationContext,
    ) -> EvaluationResult<ResolutionDetails<i64>> {
        let resolved = self.resolve(flag_key, evaluation_context)?;
        match resolved.value {
            Some(ResolvedValue::Variable(VariableValue::Integer(value))) => {
                Ok(self.details(&resolved.evaluation, value))
            }
            _ => Err(self.type_mismatch(flag_key, "integer")),
        }
    }

    async fn resolve_float_value(
        &self,
        flag_key: &str,
        evaluation_context: &EvaluationContext,
    ) -> EvaluationResult<ResolutionDetails<f64>> {
        let resolved = self.resolve(flag_key, evaluation_context)?;
        let value = match resolved.value {
            Some(ResolvedValue::Variable(VariableValue::Integer(value))) => Some(value as f64),
            Some(ResolvedValue::Variable(VariableValue::Double(value))) if value.is_finite() => {
                Some(value)
            }
            _ => None,
        };
        match value {
            Some(value) => Ok(self.details(&resolved.evaluation, value)),
            None => Err(self.type_mismatch(flag_key, "number")),
        }
    }

    async fn resolve_string_value(
        &self,
        flag_key: &str,
        evaluation_context: &EvaluationContext,
    ) -> EvaluationResult<ResolutionDetails<String>> {
        let resolved = self.resolve(flag_key, evaluation_context)?;
        let value = match resolved.value {
            Some(ResolvedValue::String(value)) => Some(value),
            Some(ResolvedValue::Variable(VariableValue::String(value))) => Some(value),
            _ => None,
        };
        match value {
            Some(value) => Ok(self.details(&resolved.evaluation, value)),
            None => Err(self.type_mismatch(flag_key, "string")),
        }
    }

    async fn resolve_struct_value(
        &self,
        flag_key: &str,
        evaluation_context: &EvaluationContext,
    ) -> EvaluationResult<ResolutionDetails<StructValue>> {
        let resolved = self.resolve(flag_key, evaluation_context)?;
        let value = match resolved.value {
            Some(ResolvedValue::Variable(VariableValue::Object(value))) => {
                Some(variable_object_to_struct(value))
            }
            _ => None,
        };
        match value {
            Some(value) => Ok(self.details(&resolved.evaluation, value)),
            None => Err(self.type_mismatch(flag_key, "structure")),
        }
    }
}

struct Resolved {
    evaluation: Evaluation,
    value: Option<ResolvedValue>,
}

enum ResolvedValue {
    Bool(bool),
    String(String),
    Variable(VariableValue),
}

fn initial_datafile_error(options: &FeaturevisorOptions) -> Option<String> {
    match options.datafile.as_ref() {
        Some(DatafileInput::Json(json)) => serde_json::from_str::<DatafileContent>(json)
            .ok()
            .filter(|datafile| !datafile.revision.is_empty())
            .map(|_| None)
            .unwrap_or_else(|| Some("Could not parse datafile".to_string())),
        Some(DatafileInput::Content(datafile)) if datafile.revision.is_empty() => {
            Some("Could not parse datafile".to_string())
        }
        _ => None,
    }
}

fn subscribe_to_datafile_state(
    featurevisor: &Featurevisor,
    error: &Arc<Mutex<Option<String>>>,
) -> Vec<Unsubscribe> {
    let error_for_diagnostics = Arc::clone(error);
    let diagnostic_subscription = featurevisor.on(
        EventName::Error,
        Arc::new(move |details| {
            if let EventDetails::Error { diagnostic } = details {
                if diagnostic.code == "invalid_datafile" {
                    if let Ok(mut current) = error_for_diagnostics.lock() {
                        *current = Some(diagnostic.message.clone());
                    }
                }
            }
        }),
    );
    let error_for_datafile = Arc::clone(error);
    let datafile_subscription = featurevisor.on(
        EventName::DatafileSet,
        Arc::new(move |_| {
            if let Ok(mut current) = error_for_datafile.lock() {
                *current = None;
            }
        }),
    );
    vec![diagnostic_subscription, datafile_subscription]
}

fn split_key<'a>(key: &'a str, separator: &str) -> (&'a str, Option<&'a str>) {
    match key.find(separator) {
        Some(index) => (&key[..index], Some(&key[index + separator.len()..])),
        None => (key, None),
    }
}

fn variable_type(evaluation: &Evaluation) -> Option<&str> {
    evaluation
        .variable_schema
        .as_ref()
        .map(|schema| schema.variable_type.as_str())
        .or_else(|| {
            evaluation
                .variable
                .as_ref()
                .map(|variable| variable.variable_type.as_str())
        })
}

fn normalize_variable(value: VariableValue, variable_type: Option<&str>) -> VariableValue {
    if variable_type == Some("json") {
        if let VariableValue::String(raw) = &value {
            if let Ok(parsed) = serde_json::from_str(raw) {
                return VariableValue::from_json(parsed);
            }
        }
    }
    value
}

fn error_for_evaluation(evaluation: &Evaluation) -> Option<EvaluationError> {
    match evaluation.reason {
        FeaturevisorReason::FeatureNotFound => Some(evaluation_error(
            EvaluationErrorCode::FlagNotFound,
            format!("Feature \"{}\" was not found", evaluation.feature_key),
        )),
        FeaturevisorReason::VariableNotFound => Some(evaluation_error(
            EvaluationErrorCode::FlagNotFound,
            match evaluation.variable_key.as_deref() {
                Some(variable_key) if evaluation.feature_key.is_empty() => {
                    format!("Global variable \"{variable_key}\" was not found")
                }
                Some(variable_key) => format!(
                    "Variable \"{variable_key}\" was not found for feature \"{}\"",
                    evaluation.feature_key
                ),
                None => "Variable was not found".to_string(),
            },
        )),
        FeaturevisorReason::NoVariations => Some(evaluation_error(
            EvaluationErrorCode::FlagNotFound,
            format!("Feature \"{}\" has no variations", evaluation.feature_key),
        )),
        FeaturevisorReason::Error => Some(evaluation_error(
            EvaluationErrorCode::General("GENERAL".to_string()),
            evaluation
                .error
                .clone()
                .unwrap_or_else(|| "Featurevisor evaluation failed".to_string()),
        )),
        _ => None,
    }
}

fn evaluation_error(code: EvaluationErrorCode, message: impl Into<String>) -> EvaluationError {
    EvaluationError {
        code,
        message: Some(message.into()),
    }
}

fn reason_for(reason: FeaturevisorReason) -> EvaluationReason {
    match reason {
        FeaturevisorReason::FeatureNotFound
        | FeaturevisorReason::VariableNotFound
        | FeaturevisorReason::NoVariations
        | FeaturevisorReason::Error => EvaluationReason::Error,
        FeaturevisorReason::Required
        | FeaturevisorReason::Forced
        | FeaturevisorReason::Sticky
        | FeaturevisorReason::Rule
        | FeaturevisorReason::VariableOverrideVariation
        | FeaturevisorReason::VariableOverrideRule => EvaluationReason::TargetingMatch,
        FeaturevisorReason::Allocated => EvaluationReason::Split,
        FeaturevisorReason::Disabled
        | FeaturevisorReason::VariationDisabled
        | FeaturevisorReason::VariableDisabled
        | FeaturevisorReason::RequiredFeaturesUnmet => EvaluationReason::Disabled,
        _ => EvaluationReason::Default,
    }
}

fn metadata_for(evaluation: &Evaluation, featurevisor: &Featurevisor) -> FlagMetadata {
    let mut metadata = FlagMetadata::default();
    metadata.add_value(
        "featurevisorReason",
        serde_json::to_value(&evaluation.reason)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_else(|| "error".to_string()),
    );
    metadata.add_value("schemaVersion", featurevisor.get_schema_version());
    let revision = featurevisor.get_revision();
    if !revision.is_empty() {
        metadata.add_value("revision", revision);
    }
    if !evaluation.feature_key.is_empty() {
        metadata.add_value("featureKey", evaluation.feature_key.clone());
    }
    if let Some(value) = &evaluation.variable_key {
        metadata.add_value("variableKey", value.clone());
    }
    if let Some(value) = &evaluation.rule_key {
        metadata.add_value("ruleKey", value.clone());
    }
    if let Some(value) = &evaluation.bucket_key {
        metadata.add_value("bucketKey", value.clone());
    }
    if let Some(value) = evaluation.bucket_value {
        metadata.add_value("bucketValue", i64::from(value));
    }
    if let Some(value) = evaluation
        .force_index
        .and_then(|value| i64::try_from(value).ok())
    {
        metadata.add_value("forceIndex", value);
    }
    if let Some(value) = evaluation
        .variable_override_index
        .and_then(|value| i64::try_from(value).ok())
    {
        metadata.add_value("variableOverrideIndex", value);
    }
    if let Some(value) = &evaluation.variable_override_key {
        metadata.add_value("variableOverrideKey", value.clone());
    }
    metadata
}

fn featurevisor_context(
    context: &EvaluationContext,
    targeting_key_field: &str,
) -> EvaluationResult<Context> {
    let mut result = Context::new();
    for (key, value) in &context.custom_fields {
        result.insert(key.clone(), context_value(value)?);
    }
    if let Some(targeting_key) = &context.targeting_key {
        result.insert(
            "targetingKey".to_string(),
            AttributeValue::String(targeting_key.clone()),
        );
        result.insert(
            targeting_key_field.to_string(),
            AttributeValue::String(targeting_key.clone()),
        );
    }
    Ok(result)
}

fn context_value(value: &EvaluationContextFieldValue) -> EvaluationResult<AttributeValue> {
    match value {
        EvaluationContextFieldValue::Bool(value) => Ok(AttributeValue::Boolean(*value)),
        EvaluationContextFieldValue::Int(value) => Ok(AttributeValue::Integer(*value)),
        EvaluationContextFieldValue::Float(value) => Ok(AttributeValue::Double(*value)),
        EvaluationContextFieldValue::String(value) => Ok(AttributeValue::String(value.clone())),
        EvaluationContextFieldValue::DateTime(value) => value
            .format(&Rfc3339)
            .map(AttributeValue::String)
            .map_err(|error| {
                evaluation_error(EvaluationErrorCode::InvalidContext, error.to_string())
            }),
        EvaluationContextFieldValue::Struct(value) => {
            if let Ok(value) = Arc::clone(value).downcast::<serde_json::Value>() {
                return Ok(AttributeValue::from_json((*value).clone()));
            }
            if let Ok(value) = Arc::clone(value).downcast::<StructValue>() {
                return Ok(AttributeValue::from_json(struct_to_json(&value)));
            }
            Err(evaluation_error(
                EvaluationErrorCode::InvalidContext,
                "OpenFeature structure context values must use StructValue or serde_json::Value",
            ))
        }
    }
}

fn struct_to_json(value: &StructValue) -> serde_json::Value {
    serde_json::Value::Object(
        value
            .fields
            .iter()
            .map(|(key, value)| (key.clone(), openfeature_value_to_json(value)))
            .collect(),
    )
}

fn openfeature_value_to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Bool(value) => serde_json::Value::Bool(*value),
        Value::Int(value) => serde_json::Value::Number((*value).into()),
        Value::Float(value) => serde_json::Number::from_f64(*value)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Value::String(value) => serde_json::Value::String(value.clone()),
        Value::Array(value) => {
            serde_json::Value::Array(value.iter().map(openfeature_value_to_json).collect())
        }
        Value::Struct(value) => struct_to_json(value),
    }
}

fn variable_object_to_struct(
    values: std::collections::HashMap<String, VariableValue>,
) -> StructValue {
    StructValue {
        fields: values
            .into_iter()
            .filter_map(|(key, value)| variable_to_openfeature(value).map(|value| (key, value)))
            .collect(),
    }
}

fn variable_to_openfeature(value: VariableValue) -> Option<Value> {
    match value {
        VariableValue::String(value) => Some(Value::String(value)),
        VariableValue::Integer(value) => Some(Value::Int(value)),
        VariableValue::Double(value) if value.is_finite() => Some(Value::Float(value)),
        VariableValue::Double(_) | VariableValue::Null => None,
        VariableValue::Boolean(value) => Some(Value::Bool(value)),
        VariableValue::Array(values) => Some(Value::Array(
            values
                .into_iter()
                .filter_map(variable_to_openfeature)
                .collect(),
        )),
        VariableValue::Object(values) => Some(Value::Struct(variable_object_to_struct(values))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_every_current_featurevisor_reason() {
        for reason in [
            FeaturevisorReason::Required,
            FeaturevisorReason::Forced,
            FeaturevisorReason::Sticky,
            FeaturevisorReason::Rule,
            FeaturevisorReason::VariableOverrideVariation,
            FeaturevisorReason::VariableOverrideRule,
        ] {
            assert_eq!(reason_for(reason), EvaluationReason::TargetingMatch);
        }

        assert_eq!(
            reason_for(FeaturevisorReason::Allocated),
            EvaluationReason::Split
        );

        for reason in [
            FeaturevisorReason::Disabled,
            FeaturevisorReason::VariationDisabled,
            FeaturevisorReason::VariableDisabled,
            FeaturevisorReason::RequiredFeaturesUnmet,
        ] {
            assert_eq!(reason_for(reason), EvaluationReason::Disabled);
        }

        for reason in [
            FeaturevisorReason::FeatureNotFound,
            FeaturevisorReason::VariableNotFound,
            FeaturevisorReason::NoVariations,
            FeaturevisorReason::Error,
        ] {
            assert_eq!(reason_for(reason), EvaluationReason::Error);
        }

        for reason in [
            FeaturevisorReason::OutOfRange,
            FeaturevisorReason::NoMatch,
            FeaturevisorReason::VariableDefault,
        ] {
            assert_eq!(reason_for(reason), EvaluationReason::Default);
        }
    }
}
