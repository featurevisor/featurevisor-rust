use crate::bucketer::{get_bucket_key, get_bucketed_number};
use crate::conditions::{all_conditions_are_matched, all_segments_are_matched, RegexGetter};
use crate::diagnostics::{Diagnostic, LogLevel};
use crate::modules::{ConfigureBucketKeyOptions, ConfigureBucketValueOptions, FeaturevisorModule};
use crate::types::{
    Allocation, Context, DatafileContent, EvaluatedFeature, Feature, Force, Required,
    ResolvedVariableSchema, StickyFeatures, Traffic, VariableOverride, VariableValue, Variation,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationType {
    Flag,
    Variation,
    Variable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationReason {
    FeatureNotFound,
    Disabled,
    Required,
    OutOfRange,
    NoVariations,
    VariationDisabled,
    VariableNotFound,
    VariableDefault,
    VariableDisabled,
    VariableOverrideVariation,
    VariableOverrideRule,
    NoMatch,
    Forced,
    Sticky,
    Rule,
    Allocated,
    Error,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Evaluation {
    #[serde(rename = "type")]
    pub evaluation_type: EvaluationType,
    pub feature_key: String,
    pub reason: EvaluationReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bucket_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bucket_value: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub traffic: Option<Traffic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force: Option<Force>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<Required>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sticky: Option<EvaluatedFeature>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variation: Option<Variation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variation_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variable_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variable_value: Option<VariableValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variable_schema: Option<ResolvedVariableSchema>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variable_override_index: Option<usize>,
}

#[derive(Clone)]
pub struct EvaluateOptions {
    pub evaluation_type: EvaluationType,
    pub feature_key: String,
    pub variable_key: Option<String>,
    pub context: Context,
    pub default_variation_value: Option<String>,
    pub default_variable_value: Option<VariableValue>,
    pub sticky: Option<StickyFeatures>,
    pub(crate) data: Arc<EvaluationData>,
    pub(crate) modules: Arc<Vec<Arc<dyn FeaturevisorModule>>>,
    pub(crate) report: Arc<dyn Fn(Diagnostic) + Send + Sync>,
}

#[derive(Clone)]
pub(crate) struct EvaluationData {
    pub datafile: Arc<DatafileContent>,
    pub regex_cache: Arc<Mutex<HashMap<String, regex::Regex>>>,
}

impl EvaluationData {
    fn regex(&self, pattern: &str, flags: &str) -> Result<regex::Regex, String> {
        let key = format!("{pattern}\u{0}{flags}");
        if let Ok(cache) = self.regex_cache.lock() {
            if let Some(regex) = cache.get(&key) {
                return Ok(regex.clone());
            }
        }
        let mut prefix = String::new();
        for flag in flags.chars() {
            match flag {
                'g' => {}
                'i' | 'm' | 's' => {
                    if !prefix.contains(flag) {
                        prefix.push(flag);
                    }
                }
                other => return Err(format!("invalid regular expression flag '{other}'")),
            }
        }
        let source = if prefix.is_empty() {
            pattern.to_string()
        } else {
            format!("(?{prefix}){pattern}")
        };
        let regex = regex::Regex::new(&source).map_err(|error| error.to_string())?;
        if let Ok(mut cache) = self.regex_cache.lock() {
            cache.insert(key, regex.clone());
        }
        Ok(regex)
    }

    fn get_feature(&self, key: &str) -> Option<Feature> {
        self.datafile.features.get(key).cloned()
    }

    fn all_conditions(
        &self,
        value: &JsonValue,
        context: &Context,
        report: &dyn Fn(Diagnostic),
    ) -> bool {
        let get_regex: &RegexGetter<'_> = &|pattern, flags| self.regex(pattern, flags);
        all_conditions_are_matched(value, context, get_regex, report)
    }

    pub(crate) fn all_segments(
        &self,
        value: &JsonValue,
        context: &Context,
        report: &dyn Fn(Diagnostic),
    ) -> bool {
        let get_regex: &RegexGetter<'_> = &|pattern, flags| self.regex(pattern, flags);
        let get_segment = |key: &str| self.datafile.segments.get(key).cloned();
        all_segments_are_matched(value, context, &get_segment, get_regex, report)
    }

    fn matched_traffic(
        &self,
        feature: &Feature,
        context: &Context,
        report: &dyn Fn(Diagnostic),
    ) -> Option<Traffic> {
        feature
            .traffic
            .iter()
            .find(|traffic| self.all_segments(&traffic.segments, context, report))
            .cloned()
    }

    fn matched_allocation(&self, traffic: &Traffic, bucket_value: u32) -> Option<Allocation> {
        traffic
            .allocation
            .as_ref()?
            .iter()
            .find(|allocation| {
                allocation.range[0] <= bucket_value as f64
                    && allocation.range[1] >= bucket_value as f64
            })
            .cloned()
    }

    fn matched_force(
        &self,
        feature: &Feature,
        context: &Context,
        report: &dyn Fn(Diagnostic),
    ) -> (Option<Force>, Option<usize>) {
        let Some(forces) = feature.force.as_ref() else {
            return (None, None);
        };
        for (index, force) in forces.iter().enumerate() {
            if let Some(conditions) = &force.conditions {
                if self.all_conditions(conditions, context, report) {
                    return (Some(force.clone()), Some(index));
                }
            }
            if let Some(segments) = &force.segments {
                if self.all_segments(segments, context, report) {
                    return (Some(force.clone()), Some(index));
                }
            }
        }
        (None, None)
    }
}

fn diag_for(evaluation: &Evaluation, level: LogLevel, code: &str, message: &str) -> Diagnostic {
    let mut diagnostic = Diagnostic::new(level, code, message);
    diagnostic.details.insert(
        "featureKey".to_string(),
        JsonValue::String(evaluation.feature_key.clone()),
    );
    diagnostic.details.insert(
        "reason".to_string(),
        serde_json::to_value(&evaluation.reason).unwrap_or(JsonValue::Null),
    );
    if let Some(variable_key) = &evaluation.variable_key {
        diagnostic.details.insert(
            "variableKey".to_string(),
            JsonValue::String(variable_key.clone()),
        );
    }
    diagnostic.details.insert(
        "evaluation".to_string(),
        serde_json::to_value(evaluation).unwrap_or(JsonValue::Null),
    );
    diagnostic
}

fn apply_diagnostic(
    report: &dyn Fn(Diagnostic),
    evaluation: &Evaluation,
    level: LogLevel,
    code: &str,
    message: &str,
) {
    report(diag_for(evaluation, level, code, message));
}

pub(crate) fn evaluate_with_modules(mut options: EvaluateOptions) -> Evaluation {
    let original = options.clone();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let modules = Arc::clone(&options.modules);
        for module in modules.iter() {
            options = module.before(options.clone());
        }
        let mut evaluation = evaluate(&options);
        if let Some(default) = options.default_variation_value.clone() {
            if evaluation.evaluation_type == EvaluationType::Variation
                && evaluation.variation_value.is_none()
                && evaluation.variation.is_none()
            {
                evaluation.variation_value = Some(default);
            }
        }
        if let Some(default) = options.default_variable_value.clone() {
            if evaluation.evaluation_type == EvaluationType::Variable
                && evaluation.variable_value.is_none()
            {
                evaluation.variable_value = Some(default);
            }
        }
        for module in options.modules.iter() {
            evaluation = module.after(evaluation, &options);
        }
        evaluation
    }));
    match result {
        Ok(evaluation) => evaluation,
        Err(_) => {
            let evaluation = Evaluation {
                evaluation_type: original.evaluation_type,
                feature_key: original.feature_key.clone(),
                reason: EvaluationReason::Error,
                bucket_key: None,
                bucket_value: None,
                rule_key: None,
                error: Some("module callback panicked".to_string()),
                enabled: None,
                traffic: None,
                force_index: None,
                force: None,
                required: None,
                sticky: None,
                variation: None,
                variation_value: None,
                variable_key: original.variable_key.clone(),
                variable_value: None,
                variable_schema: None,
                variable_override_index: None,
            };
            apply_diagnostic(
                original.report.as_ref(),
                &evaluation,
                LogLevel::Error,
                "evaluation_error",
                "Error during evaluation",
            );
            evaluation
        }
    }
}

fn evaluate(options: &EvaluateOptions) -> Evaluation {
    let type_ = options.evaluation_type;
    let key = options.feature_key.clone();
    let variable_key = options.variable_key.clone();
    let data = &options.data;
    let report = options.report.as_ref();

    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if type_ != EvaluationType::Flag {
            let flag = evaluate(&EvaluateOptions {
                evaluation_type: EvaluationType::Flag,
                ..options.clone()
            });
            if flag.enabled == Some(false) {
                let mut evaluation = Evaluation {
                    evaluation_type: type_,
                    feature_key: key.clone(),
                    reason: EvaluationReason::Disabled,
                    bucket_key: None,
                    bucket_value: None,
                    rule_key: None,
                    error: None,
                    enabled: Some(false),
                    traffic: None,
                    force_index: None,
                    force: None,
                    required: None,
                    sticky: None,
                    variation: None,
                    variation_value: None,
                    variable_key: variable_key.clone(),
                    variable_value: None,
                    variable_schema: None,
                    variable_override_index: None,
                };
                if let (EvaluationType::Variable, Some(variable_key)) =
                    (type_, variable_key.as_ref())
                {
                    if let Some(feature) = data.get_feature(&key) {
                        if let Some(schema) = feature
                            .variables_schema
                            .as_ref()
                            .and_then(|schemas| schemas.get(variable_key))
                        {
                            if let Some(value) = &schema.disabled_value {
                                evaluation.reason = EvaluationReason::VariableDisabled;
                                evaluation.variable_value = Some(value.clone());
                                evaluation.variable_schema = Some(schema.clone());
                            } else if schema.use_default_when_disabled == Some(true) {
                                evaluation.reason = EvaluationReason::VariableDefault;
                                evaluation.variable_value = Some(schema.default_value.clone());
                                evaluation.variable_schema = Some(schema.clone());
                            }
                        }
                    }
                }
                if type_ == EvaluationType::Variation {
                    if let Some(value) = data
                        .get_feature(&key)
                        .and_then(|feature| feature.disabled_variation_value)
                    {
                        evaluation.reason = EvaluationReason::VariationDisabled;
                        evaluation.variation_value = Some(value);
                    }
                }
                apply_diagnostic(
                    report,
                    &evaluation,
                    LogLevel::Debug,
                    &format_reason(&evaluation.reason),
                    "feature is disabled",
                );
                return evaluation;
            }
        }

        if let Some(sticky) = &options.sticky {
            if let Some(sticky_feature) = sticky.get(&key) {
                if type_ == EvaluationType::Flag {
                    let evaluation = Evaluation {
                        evaluation_type: type_,
                        feature_key: key.clone(),
                        reason: EvaluationReason::Sticky,
                        bucket_key: None,
                        bucket_value: None,
                        rule_key: None,
                        error: None,
                        enabled: Some(sticky_feature.enabled),
                        traffic: None,
                        force_index: None,
                        force: None,
                        required: None,
                        sticky: Some(sticky_feature.clone()),
                        variation: None,
                        variation_value: None,
                        variable_key: None,
                        variable_value: None,
                        variable_schema: None,
                        variable_override_index: None,
                    };
                    apply_diagnostic(
                        report,
                        &evaluation,
                        LogLevel::Debug,
                        "sticky",
                        "using sticky enabled",
                    );
                    return evaluation;
                }
                if type_ == EvaluationType::Variation {
                    if let Some(value) = &sticky_feature.variation {
                        let evaluation = basic_variation(
                            key.clone(),
                            EvaluationReason::Sticky,
                            Some(value.clone()),
                        );
                        apply_diagnostic(
                            report,
                            &evaluation,
                            LogLevel::Debug,
                            "sticky",
                            "using sticky variation",
                        );
                        return evaluation;
                    }
                }
                if let (EvaluationType::Variable, Some(variable_key)) =
                    (type_, variable_key.as_ref())
                {
                    if let Some(value) = sticky_feature
                        .variables
                        .as_ref()
                        .and_then(|values| values.get(variable_key))
                    {
                        let evaluation = variable_evaluation(
                            key.clone(),
                            EvaluationReason::Sticky,
                            variable_key.clone(),
                            value.clone(),
                            None,
                        );
                        apply_diagnostic(
                            report,
                            &evaluation,
                            LogLevel::Debug,
                            "sticky",
                            "using sticky variable",
                        );
                        return evaluation;
                    }
                }
            }
        }

        let Some(feature) = data.get_feature(&key) else {
            let evaluation = basic(type_, key.clone(), EvaluationReason::FeatureNotFound);
            apply_diagnostic(
                report,
                &evaluation,
                LogLevel::Warn,
                "feature_not_found",
                "Feature not found",
            );
            return evaluation;
        };

        if type_ == EvaluationType::Flag && feature.deprecated == Some(true) {
            let mut diagnostic = Diagnostic::new(
                LogLevel::Warn,
                "deprecated_feature",
                "Feature is deprecated",
            );
            diagnostic
                .details
                .insert("featureKey".to_string(), JsonValue::String(key.clone()));
            report(diagnostic);
        }
        let schema = variable_key.as_ref().and_then(|variable_key| {
            feature
                .variables_schema
                .as_ref()
                .and_then(|schemas| schemas.get(variable_key).cloned())
        });
        if let Some(variable_key) = &variable_key {
            if schema.is_none() {
                let evaluation = variable_evaluation(
                    key.clone(),
                    EvaluationReason::VariableNotFound,
                    variable_key.clone(),
                    VariableValue::Null,
                    None,
                );
                apply_diagnostic(
                    report,
                    &evaluation,
                    LogLevel::Warn,
                    "variable_not_found",
                    "Variable schema not found",
                );
                return evaluation;
            }
            if schema
                .as_ref()
                .and_then(|schema| schema.deprecated)
                .unwrap_or(false)
            {
                let mut diagnostic = Diagnostic::new(
                    LogLevel::Warn,
                    "deprecated_variable",
                    "Variable is deprecated",
                );
                diagnostic
                    .details
                    .insert("featureKey".to_string(), JsonValue::String(key.clone()));
                diagnostic.details.insert(
                    "variableKey".to_string(),
                    JsonValue::String(variable_key.clone()),
                );
                report(diagnostic);
            }
        }
        if type_ == EvaluationType::Variation
            && feature
                .variations
                .as_ref()
                .map(Vec::is_empty)
                .unwrap_or(true)
        {
            let evaluation = basic(type_, key.clone(), EvaluationReason::NoVariations);
            apply_diagnostic(
                report,
                &evaluation,
                LogLevel::Warn,
                "no_variations",
                "No variations",
            );
            return evaluation;
        }

        let (force, force_index) = data.matched_force(&feature, &options.context, report);
        if let Some(force_value) = &force {
            if type_ == EvaluationType::Flag {
                if let Some(enabled) = force_value.enabled {
                    let mut evaluation = basic(type_, key.clone(), EvaluationReason::Forced);
                    evaluation.enabled = Some(enabled);
                    evaluation.force = Some(force_value.clone());
                    evaluation.force_index = force_index;
                    apply_diagnostic(
                        report,
                        &evaluation,
                        LogLevel::Debug,
                        "forced",
                        "forced enabled found",
                    );
                    return evaluation;
                }
            }
            if type_ == EvaluationType::Variation {
                if let (Some(value), Some(variations)) =
                    (&force_value.variation, &feature.variations)
                {
                    if let Some(variation) = variations
                        .iter()
                        .find(|variation| &variation.value == value)
                    {
                        let mut evaluation = basic(type_, key.clone(), EvaluationReason::Forced);
                        evaluation.force = Some(force_value.clone());
                        evaluation.force_index = force_index;
                        evaluation.variation = Some(variation.clone());
                        apply_diagnostic(
                            report,
                            &evaluation,
                            LogLevel::Debug,
                            "forced",
                            "forced variation found",
                        );
                        return evaluation;
                    }
                }
            }
            if let (Some(variable_key), Some(variables)) = (&variable_key, &force_value.variables) {
                if let Some(value) = variables.get(variable_key) {
                    let mut evaluation = variable_evaluation(
                        key.clone(),
                        EvaluationReason::Forced,
                        variable_key.clone(),
                        value.clone(),
                        schema.clone(),
                    );
                    evaluation.force = Some(force_value.clone());
                    evaluation.force_index = force_index;
                    apply_diagnostic(
                        report,
                        &evaluation,
                        LogLevel::Debug,
                        "forced",
                        "forced variable",
                    );
                    return evaluation;
                }
            }
        }

        if type_ == EvaluationType::Flag {
            if let Some(required) = &feature.required {
                if !required.is_empty()
                    && !required
                        .iter()
                        .all(|required| required_is_met(required, options, data, report))
                {
                    let mut evaluation = basic(type_, key.clone(), EvaluationReason::Required);
                    evaluation.required = Some(required.clone());
                    evaluation.enabled = Some(false);
                    apply_diagnostic(
                        report,
                        &evaluation,
                        LogLevel::Debug,
                        "required",
                        "required features not enabled",
                    );
                    return evaluation;
                }
            }
        }

        let mut bucket_key =
            match get_bucket_key(&key, &feature.bucket_by, &options.context, report) {
                Ok(bucket_key) => bucket_key,
                Err(error) => {
                    let mut evaluation = basic(type_, key.clone(), EvaluationReason::Error);
                    evaluation.error = Some(error);
                    apply_diagnostic(
                        report,
                        &evaluation,
                        LogLevel::Error,
                        "evaluation_error",
                        "Error during evaluation",
                    );
                    return evaluation;
                }
            };
        for module in options.modules.iter() {
            bucket_key = module.bucket_key(ConfigureBucketKeyOptions {
                feature_key: key.clone(),
                context: options.context.clone(),
                bucket_by: feature.bucket_by.clone(),
                bucket_key,
            });
        }
        let mut bucket_value = get_bucketed_number(&bucket_key);
        for module in options.modules.iter() {
            bucket_value = module.bucket_value(ConfigureBucketValueOptions {
                feature_key: key.clone(),
                bucket_key: bucket_key.clone(),
                context: options.context.clone(),
                bucket_value,
            });
        }
        let matched_traffic = data.matched_traffic(&feature, &options.context, report);
        let matched_allocation = matched_traffic
            .as_ref()
            .and_then(|traffic| data.matched_allocation(traffic, bucket_value));

        if let Some(traffic) = &matched_traffic {
            if traffic.percentage == 0.0 {
                let mut evaluation = basic(type_, key.clone(), EvaluationReason::Rule);
                evaluation.bucket_key = Some(bucket_key);
                evaluation.bucket_value = Some(bucket_value);
                evaluation.rule_key = Some(traffic.key.clone());
                evaluation.traffic = Some(traffic.clone());
                evaluation.enabled = Some(false);
                apply_diagnostic(
                    report,
                    &evaluation,
                    LogLevel::Debug,
                    "rule",
                    "matched rule with 0 percentage",
                );
                return evaluation;
            }
            if type_ == EvaluationType::Flag {
                if let Some(ranges) = &feature.ranges {
                    if !ranges.is_empty() {
                        if ranges.iter().any(|range| {
                            bucket_value as f64 >= range[0] && (bucket_value as f64) < range[1]
                        }) {
                            let mut evaluation =
                                basic(type_, key.clone(), EvaluationReason::Allocated);
                            evaluation.bucket_key = Some(bucket_key);
                            evaluation.bucket_value = Some(bucket_value);
                            evaluation.rule_key = Some(traffic.key.clone());
                            evaluation.traffic = Some(traffic.clone());
                            evaluation.enabled = Some(traffic.enabled.unwrap_or(true));
                            apply_diagnostic(
                                report,
                                &evaluation,
                                LogLevel::Debug,
                                "allocated",
                                "matched",
                            );
                            return evaluation;
                        }
                        let mut evaluation =
                            basic(type_, key.clone(), EvaluationReason::OutOfRange);
                        evaluation.bucket_key = Some(bucket_key);
                        evaluation.bucket_value = Some(bucket_value);
                        evaluation.enabled = Some(false);
                        apply_diagnostic(
                            report,
                            &evaluation,
                            LogLevel::Debug,
                            "out_of_range",
                            "not matched",
                        );
                        return evaluation;
                    }
                }
                if let Some(enabled) = traffic.enabled {
                    let mut evaluation = basic(type_, key.clone(), EvaluationReason::Rule);
                    evaluation.bucket_key = Some(bucket_key);
                    evaluation.bucket_value = Some(bucket_value);
                    evaluation.rule_key = Some(traffic.key.clone());
                    evaluation.traffic = Some(traffic.clone());
                    evaluation.enabled = Some(enabled);
                    apply_diagnostic(
                        report,
                        &evaluation,
                        LogLevel::Debug,
                        "rule",
                        "override from rule",
                    );
                    return evaluation;
                }
                if bucket_value as f64 <= traffic.percentage {
                    let mut evaluation = basic(type_, key.clone(), EvaluationReason::Rule);
                    evaluation.bucket_key = Some(bucket_key);
                    evaluation.bucket_value = Some(bucket_value);
                    evaluation.rule_key = Some(traffic.key.clone());
                    evaluation.traffic = Some(traffic.clone());
                    evaluation.enabled = Some(true);
                    apply_diagnostic(
                        report,
                        &evaluation,
                        LogLevel::Debug,
                        "rule",
                        "matched traffic",
                    );
                    return evaluation;
                }
            }
            if type_ == EvaluationType::Variation {
                if let (Some(value), Some(variations)) = (&traffic.variation, &feature.variations) {
                    if let Some(variation) = variations
                        .iter()
                        .find(|variation| &variation.value == value)
                    {
                        let mut evaluation = basic(type_, key.clone(), EvaluationReason::Rule);
                        evaluation.bucket_key = Some(bucket_key);
                        evaluation.bucket_value = Some(bucket_value);
                        evaluation.rule_key = Some(traffic.key.clone());
                        evaluation.traffic = Some(traffic.clone());
                        evaluation.variation = Some(variation.clone());
                        apply_diagnostic(
                            report,
                            &evaluation,
                            LogLevel::Debug,
                            "rule",
                            "override from rule",
                        );
                        return evaluation;
                    }
                }
                if let (Some(allocation), Some(variations)) =
                    (matched_allocation.as_ref(), &feature.variations)
                {
                    if let Some(variation) = variations
                        .iter()
                        .find(|variation| variation.value == allocation.variation)
                    {
                        let mut evaluation = basic(type_, key.clone(), EvaluationReason::Allocated);
                        evaluation.bucket_key = Some(bucket_key);
                        evaluation.bucket_value = Some(bucket_value);
                        evaluation.rule_key = Some(traffic.key.clone());
                        evaluation.traffic = Some(traffic.clone());
                        evaluation.variation = Some(variation.clone());
                        apply_diagnostic(
                            report,
                            &evaluation,
                            LogLevel::Debug,
                            "allocated",
                            "allocated variation",
                        );
                        return evaluation;
                    }
                }
            }
        }

        if type_ == EvaluationType::Variable {
            if let Some(variable_key) = &variable_key {
                if let Some(traffic) = &matched_traffic {
                    if let Some(overrides) = traffic
                        .variable_overrides
                        .as_ref()
                        .and_then(|values| values.get(variable_key))
                    {
                        if let Some((index, override_value)) = overrides
                            .iter()
                            .enumerate()
                            .find(|(_, item)| override_matches(item, options, data, report))
                        {
                            let mut evaluation = variable_evaluation(
                                key.clone(),
                                EvaluationReason::VariableOverrideRule,
                                variable_key.clone(),
                                override_value.value.clone(),
                                schema.clone(),
                            );
                            evaluation.bucket_key = Some(bucket_key.clone());
                            evaluation.bucket_value = Some(bucket_value);
                            evaluation.rule_key = Some(traffic.key.clone());
                            evaluation.traffic = Some(traffic.clone());
                            evaluation.variable_override_index = Some(index);
                            apply_diagnostic(
                                report,
                                &evaluation,
                                LogLevel::Debug,
                                "variable_override_rule",
                                "variable override from rule",
                            );
                            return evaluation;
                        }
                    }
                    if let Some(value) = traffic
                        .variables
                        .as_ref()
                        .and_then(|values| values.get(variable_key))
                    {
                        let mut evaluation = variable_evaluation(
                            key.clone(),
                            EvaluationReason::Rule,
                            variable_key.clone(),
                            value.clone(),
                            schema.clone(),
                        );
                        evaluation.bucket_key = Some(bucket_key.clone());
                        evaluation.bucket_value = Some(bucket_value);
                        evaluation.rule_key = Some(traffic.key.clone());
                        evaluation.traffic = Some(traffic.clone());
                        apply_diagnostic(
                            report,
                            &evaluation,
                            LogLevel::Debug,
                            "rule",
                            "override from rule",
                        );
                        return evaluation;
                    }
                }
                let variation_value = force
                    .as_ref()
                    .and_then(|force| force.variation.clone())
                    .or_else(|| {
                        matched_traffic
                            .as_ref()
                            .and_then(|traffic| traffic.variation.clone())
                    })
                    .or_else(|| {
                        matched_allocation
                            .as_ref()
                            .map(|allocation| allocation.variation.clone())
                    });
                if let (Some(variation_value), Some(variations)) =
                    (variation_value, &feature.variations)
                {
                    if let Some(variation) = variations
                        .iter()
                        .find(|variation| variation.value == variation_value)
                    {
                        if let Some(overrides) = variation
                            .variable_overrides
                            .as_ref()
                            .and_then(|values| values.get(variable_key))
                        {
                            if let Some((index, override_value)) = overrides
                                .iter()
                                .enumerate()
                                .find(|(_, item)| override_matches(item, options, data, report))
                            {
                                let mut evaluation = variable_evaluation(
                                    key.clone(),
                                    EvaluationReason::VariableOverrideVariation,
                                    variable_key.clone(),
                                    override_value.value.clone(),
                                    schema.clone(),
                                );
                                evaluation.bucket_key = Some(bucket_key.clone());
                                evaluation.bucket_value = Some(bucket_value);
                                evaluation.rule_key =
                                    matched_traffic.as_ref().map(|traffic| traffic.key.clone());
                                evaluation.traffic = matched_traffic.clone();
                                evaluation.variable_override_index = Some(index);
                                apply_diagnostic(
                                    report,
                                    &evaluation,
                                    LogLevel::Debug,
                                    "variable_override_variation",
                                    "variable override from variation",
                                );
                                return evaluation;
                            }
                        }
                        if let Some(value) = variation
                            .variables
                            .as_ref()
                            .and_then(|values| values.get(variable_key))
                        {
                            let mut evaluation = variable_evaluation(
                                key.clone(),
                                EvaluationReason::Allocated,
                                variable_key.clone(),
                                value.clone(),
                                schema.clone(),
                            );
                            evaluation.bucket_key = Some(bucket_key.clone());
                            evaluation.bucket_value = Some(bucket_value);
                            evaluation.rule_key =
                                matched_traffic.as_ref().map(|traffic| traffic.key.clone());
                            evaluation.traffic = matched_traffic.clone();
                            apply_diagnostic(
                                report,
                                &evaluation,
                                LogLevel::Debug,
                                "allocated",
                                "allocated variable",
                            );
                            return evaluation;
                        }
                    }
                }
            }
        }
        if type_ == EvaluationType::Variation {
            let mut evaluation = basic(type_, key.clone(), EvaluationReason::NoMatch);
            evaluation.bucket_key = Some(bucket_key);
            evaluation.bucket_value = Some(bucket_value);
            apply_diagnostic(
                report,
                &evaluation,
                LogLevel::Debug,
                "no_match",
                "no matched variation",
            );
            return evaluation;
        }
        if type_ == EvaluationType::Variable {
            if let (Some(variable_key), Some(schema)) = (variable_key, schema) {
                let mut evaluation = variable_evaluation(
                    key.clone(),
                    EvaluationReason::VariableDefault,
                    variable_key,
                    schema.default_value.clone(),
                    Some(schema),
                );
                evaluation.bucket_key = Some(bucket_key);
                evaluation.bucket_value = Some(bucket_value);
                apply_diagnostic(
                    report,
                    &evaluation,
                    LogLevel::Debug,
                    "variable_default",
                    "using default value",
                );
                return evaluation;
            }
        }
        let mut evaluation = basic(type_, key.clone(), EvaluationReason::NoMatch);
        evaluation.bucket_key = Some(bucket_key);
        evaluation.bucket_value = Some(bucket_value);
        evaluation.enabled = Some(false);
        apply_diagnostic(
            report,
            &evaluation,
            LogLevel::Debug,
            "no_match",
            "nothing matched",
        );
        evaluation
    }));
    run.unwrap_or_else(|_| {
        let evaluation = basic(type_, key.clone(), EvaluationReason::Error);
        apply_diagnostic(
            report,
            &evaluation,
            LogLevel::Error,
            "evaluation_error",
            "Error during evaluation",
        );
        evaluation
    })
}

fn basic(
    evaluation_type: EvaluationType,
    feature_key: String,
    reason: EvaluationReason,
) -> Evaluation {
    Evaluation {
        evaluation_type,
        feature_key,
        reason,
        bucket_key: None,
        bucket_value: None,
        rule_key: None,
        error: None,
        enabled: None,
        traffic: None,
        force_index: None,
        force: None,
        required: None,
        sticky: None,
        variation: None,
        variation_value: None,
        variable_key: None,
        variable_value: None,
        variable_schema: None,
        variable_override_index: None,
    }
}
fn basic_variation(
    feature_key: String,
    reason: EvaluationReason,
    value: Option<String>,
) -> Evaluation {
    let mut evaluation = basic(EvaluationType::Variation, feature_key, reason);
    evaluation.variation_value = value;
    evaluation
}
fn variable_evaluation(
    feature_key: String,
    reason: EvaluationReason,
    variable_key: String,
    value: VariableValue,
    schema: Option<ResolvedVariableSchema>,
) -> Evaluation {
    let mut evaluation = basic(EvaluationType::Variable, feature_key, reason);
    evaluation.variable_key = Some(variable_key);
    evaluation.variable_value = Some(value);
    evaluation.variable_schema = schema;
    evaluation
}
fn format_reason(reason: &EvaluationReason) -> String {
    serde_json::to_string(reason)
        .unwrap_or_else(|_| "evaluation".to_string())
        .trim_matches('"')
        .to_string()
}

fn required_is_met(
    required: &Required,
    options: &EvaluateOptions,
    data: &EvaluationData,
    report: &dyn Fn(Diagnostic),
) -> bool {
    let (key, expected) = match required {
        Required::Feature(key) => (key, None),
        Required::Variation { key, variation } => (key, Some(variation.as_str())),
    };
    let flag = evaluate(&EvaluateOptions {
        evaluation_type: EvaluationType::Flag,
        feature_key: key.clone(),
        ..options.clone()
    });
    if flag.enabled != Some(true) {
        return false;
    }
    if let Some(expected) = expected {
        let variation = evaluate(&EvaluateOptions {
            evaluation_type: EvaluationType::Variation,
            feature_key: key.clone(),
            ..options.clone()
        });
        variation.variation_value.as_deref().or_else(|| {
            variation
                .variation
                .as_ref()
                .map(|value| value.value.as_str())
        }) == Some(expected)
    } else {
        let _ = data;
        let _ = report;
        true
    }
}

fn override_matches(
    item: &VariableOverride,
    options: &EvaluateOptions,
    data: &EvaluationData,
    report: &dyn Fn(Diagnostic),
) -> bool {
    if let Some(conditions) = &item.conditions {
        return data.all_conditions(conditions, &options.context, report);
    }
    if let Some(segments) = &item.segments {
        return data.all_segments(segments, &options.context, report);
    }
    false
}

pub(crate) fn evaluate_all(
    options: &EvaluateOptions,
    feature_keys: &[String],
) -> HashMap<String, EvaluatedFeature> {
    feature_keys
        .iter()
        .map(|key| {
            let flag = evaluate_with_modules(EvaluateOptions {
                evaluation_type: EvaluationType::Flag,
                feature_key: key.clone(),
                ..options.clone()
            });
            let variation = evaluate_with_modules(EvaluateOptions {
                evaluation_type: EvaluationType::Variation,
                feature_key: key.clone(),
                ..options.clone()
            });
            let variables = options
                .data
                .datafile
                .features
                .get(key)
                .and_then(|feature| feature.variables_schema.as_ref())
                .map(|schemas| {
                    schemas
                        .keys()
                        .filter_map(|variable_key| {
                            let evaluation = evaluate_with_modules(EvaluateOptions {
                                evaluation_type: EvaluationType::Variable,
                                feature_key: key.clone(),
                                variable_key: Some(variable_key.clone()),
                                ..options.clone()
                            });
                            evaluation
                                .variable_value
                                .map(|value| (variable_key.clone(), value))
                        })
                        .collect()
                });
            (
                key.clone(),
                EvaluatedFeature {
                    enabled: flag.enabled.unwrap_or(false),
                    variation: variation
                        .variation_value
                        .or_else(|| variation.variation.map(|value| value.value)),
                    variables,
                },
            )
        })
        .collect()
}
