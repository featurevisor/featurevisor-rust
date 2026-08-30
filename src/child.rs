use crate::emitter::Emitter;
use crate::evaluate::{Evaluation, EvaluationType};
use crate::events::{
    ContextSetDetails, EventDetails, EventHandler, EventName, StickyFeaturesSetDetails,
};
use crate::instance::{Featurevisor, OverrideOptions};
use crate::types::{
    Context, EvaluatedFeatures, EvaluatedVariables, StickyFeatures, StickyVariables, VariableValue,
};
use crate::Unsubscribe;
use std::sync::{Arc, Mutex};

struct ChildInner {
    context: Context,
    sticky_features: StickyFeatures,
    sticky_variables: StickyVariables,
    emitter: Emitter,
    parent_unsubscribers: Vec<Unsubscribe>,
    closed: bool,
}

#[derive(Clone)]
/// A child evaluator that inherits datafile and modules from a parent instance.
pub struct FeaturevisorChild {
    parent: Featurevisor,
    inner: Arc<Mutex<ChildInner>>,
}

impl FeaturevisorChild {
    pub(crate) fn new(
        parent: Featurevisor,
        context: Context,
        sticky_features: StickyFeatures,
        sticky_variables: StickyVariables,
    ) -> Self {
        Self {
            parent,
            inner: Arc::new(Mutex::new(ChildInner {
                context,
                sticky_features,
                sticky_variables,
                emitter: Emitter::default(),
                parent_unsubscribers: Vec::new(),
                closed: false,
            })),
        }
    }

    fn options(&self) -> (Context, StickyFeatures, StickyVariables) {
        self.inner
            .lock()
            .map(|inner| {
                (
                    inner.context.clone(),
                    inner.sticky_features.clone(),
                    inner.sticky_variables.clone(),
                )
            })
            .unwrap_or_default()
    }

    /// Updates the child context, either merging with or replacing its stored context.
    pub fn set_context(&self, context: Context, replace: bool) {
        let (context, emitter) = {
            let mut inner = match self.inner.lock() {
                Ok(inner) => inner,
                Err(_) => return,
            };
            if inner.closed {
                return;
            }
            if replace {
                inner.context = context;
            } else {
                inner.context.extend(context);
            }
            (inner.context.clone(), inner.emitter.clone())
        };
        emitter.emit(
            EventName::ContextSet,
            EventDetails::ContextSet(ContextSetDetails {
                context,
                replaced: replace,
            }),
        );
    }
    /// Returns the child context merged with an optional per evaluation context.
    pub fn get_context(&self, context: Option<&Context>) -> Context {
        let (stored, _, _) = self.options();
        let mut merged = stored;
        if let Some(context) = context {
            merged.extend(context.clone());
        }
        self.parent.get_context(Some(&merged))
    }
    /// Updates sticky feature evaluations used by this child.
    pub fn set_sticky_features(&self, sticky: StickyFeatures, replace: bool) {
        let (features, emitter) = {
            let mut inner = match self.inner.lock() {
                Ok(inner) => inner,
                Err(_) => return,
            };
            if inner.closed {
                return;
            }
            if replace {
                inner.sticky_features = sticky;
            } else {
                inner.sticky_features.extend(sticky);
            }
            (
                inner.sticky_features.keys().cloned().collect(),
                inner.emitter.clone(),
            )
        };
        emitter.emit(
            EventName::StickyFeaturesSet,
            EventDetails::StickyFeaturesSet(StickyFeaturesSetDetails {
                features,
                replaced: replace,
            }),
        );
    }

    /// Updates sticky global variable values used by this child.
    pub fn set_sticky_variables(&self, sticky: StickyVariables, replace: bool) {
        let (variables, emitter) = {
            let mut inner = match self.inner.lock() {
                Ok(inner) => inner,
                Err(_) => return,
            };
            if inner.closed {
                return;
            }
            if replace {
                inner.sticky_variables = sticky;
            } else {
                inner.sticky_variables.extend(sticky);
            }
            (
                inner.sticky_variables.keys().cloned().collect(),
                inner.emitter.clone(),
            )
        };
        emitter.emit(
            EventName::StickyVariablesSet,
            EventDetails::StickyVariablesSet(crate::events::StickyVariablesSetDetails {
                variables,
                replaced: replace,
            }),
        );
    }

    /// Subscribes to child events and returns an idempotent cleanup function.
    pub fn on(&self, event: EventName, callback: EventHandler) -> Unsubscribe {
        if matches!(
            event,
            EventName::ContextSet | EventName::StickyFeaturesSet | EventName::StickyVariablesSet
        ) {
            return self
                .inner
                .lock()
                .map(|inner| inner.emitter.on(event, callback))
                .unwrap_or_else(|_| Box::new(|| {}));
        }
        let unsubscribe = self.parent.parent_on(event, callback);
        let shared = Arc::new(Mutex::new(Some(unsubscribe)));
        let shared_for_child = Arc::clone(&shared);
        let shared_for_user = Arc::clone(&shared);
        let child_cleanup: Unsubscribe = Box::new(move || {
            if let Ok(mut unsubscribe) = shared_for_child.lock() {
                if let Some(unsubscribe) = unsubscribe.take() {
                    unsubscribe();
                }
            }
        });
        if let Ok(mut inner) = self.inner.lock() {
            if inner.closed {
                drop(inner);
                child_cleanup();
                return Box::new(|| {});
            }
            inner.parent_unsubscribers.push(child_cleanup);
        }
        Box::new(move || {
            if let Ok(mut unsubscribe) = shared_for_user.lock() {
                if let Some(unsubscribe) = unsubscribe.take() {
                    unsubscribe();
                }
            }
        })
    }

    /// Closes the child and removes its event subscriptions.
    pub fn close(&self) {
        let unsubscribers = {
            let mut inner = match self.inner.lock() {
                Ok(inner) => inner,
                Err(_) => return,
            };
            if inner.closed {
                return;
            }
            inner.closed = true;
            inner.emitter.clear();
            std::mem::take(&mut inner.parent_unsubscribers)
        };
        for unsubscribe in unsubscribers {
            unsubscribe();
        }
    }

    fn evaluate(
        &self,
        evaluation_type: EvaluationType,
        feature_key: &str,
        variable_key: Option<&str>,
        context: Option<&Context>,
        options: Option<&OverrideOptions>,
    ) -> Evaluation {
        let (stored, sticky, _) = self.options();
        let mut merged = stored;
        if let Some(context) = context {
            merged.extend(context.clone());
        }
        self.parent.evaluate_child(
            evaluation_type,
            feature_key,
            variable_key,
            Some(&merged),
            options,
            sticky,
        )
    }
    /// Evaluates a feature as a flag and returns evaluation details.
    pub fn evaluate_flag(&self, feature_key: &str, context: Option<&Context>) -> Evaluation {
        self.evaluate(EvaluationType::Flag, feature_key, None, context, None)
    }
    /// Returns whether a feature is enabled for the supplied context.
    pub fn is_enabled(&self, feature_key: &str, context: Option<&Context>) -> bool {
        self.evaluate_flag(feature_key, context).enabled == Some(true)
    }
    /// Evaluates a feature variation and returns evaluation details.
    pub fn evaluate_variation(
        &self,
        feature_key: &str,
        context: Option<&Context>,
        options: Option<&OverrideOptions>,
    ) -> Evaluation {
        self.evaluate(
            EvaluationType::Variation,
            feature_key,
            None,
            context,
            options,
        )
    }
    /// Returns the selected variation value, if one is available.
    pub fn get_variation(
        &self,
        feature_key: &str,
        context: Option<&Context>,
        options: Option<&OverrideOptions>,
    ) -> Option<String> {
        let evaluation = self.evaluate_variation(feature_key, context, options);
        evaluation
            .variation_value
            .or_else(|| evaluation.variation.map(|variation| variation.value))
    }
    /// Evaluates a feature variable and returns evaluation details.
    pub fn evaluate_variable(
        &self,
        feature_key: &str,
        variable_key: &str,
        context: Option<&Context>,
        options: Option<&OverrideOptions>,
    ) -> Evaluation {
        self.evaluate(
            EvaluationType::Variable,
            feature_key,
            Some(variable_key),
            context,
            options,
        )
    }
    /// Returns a feature variable value, if one is available.
    pub fn get_variable(
        &self,
        feature_key: &str,
        variable_key: &str,
        context: Option<&Context>,
        options: Option<&OverrideOptions>,
    ) -> Option<VariableValue> {
        self.parent.get_variable_with_sticky(
            feature_key,
            variable_key,
            self.get_context(context),
            options,
            self.options().1,
        )
    }
    /// Returns a variable as a boolean when its value has that type.
    pub fn get_variable_boolean(
        &self,
        feature_key: &str,
        variable_key: &str,
        context: Option<&Context>,
        options: Option<&OverrideOptions>,
    ) -> Option<bool> {
        match self.get_variable(feature_key, variable_key, context, options)? {
            VariableValue::Boolean(value) => Some(value),
            _ => None,
        }
    }
    /// Returns a variable as a string when its value has that type.
    pub fn get_variable_string(
        &self,
        feature_key: &str,
        variable_key: &str,
        context: Option<&Context>,
        options: Option<&OverrideOptions>,
    ) -> Option<String> {
        match self.get_variable(feature_key, variable_key, context, options)? {
            VariableValue::String(value) => Some(value),
            _ => None,
        }
    }
    /// Returns a variable as an integer when its value has that type.
    pub fn get_variable_integer(
        &self,
        feature_key: &str,
        variable_key: &str,
        context: Option<&Context>,
        options: Option<&OverrideOptions>,
    ) -> Option<i64> {
        match self.get_variable(feature_key, variable_key, context, options)? {
            VariableValue::Integer(value) => Some(value),
            VariableValue::Double(value) if value.is_finite() && value.fract() == 0.0 => {
                Some(value as i64)
            }
            _ => None,
        }
    }
    /// Returns a variable as a double when its value has that type.
    pub fn get_variable_double(
        &self,
        feature_key: &str,
        variable_key: &str,
        context: Option<&Context>,
        options: Option<&OverrideOptions>,
    ) -> Option<f64> {
        match self.get_variable(feature_key, variable_key, context, options)? {
            VariableValue::Integer(value) => Some(value as f64),
            VariableValue::Double(value) if value.is_finite() => Some(value),
            _ => None,
        }
    }
    /// Returns a variable as an array when its value has that type.
    pub fn get_variable_array(
        &self,
        feature_key: &str,
        variable_key: &str,
        context: Option<&Context>,
        options: Option<&OverrideOptions>,
    ) -> Option<Vec<VariableValue>> {
        match self.get_variable(feature_key, variable_key, context, options)? {
            VariableValue::Array(value) => Some(value),
            _ => None,
        }
    }
    /// Returns a variable as an object when its value has that type.
    pub fn get_variable_object(
        &self,
        feature_key: &str,
        variable_key: &str,
        context: Option<&Context>,
        options: Option<&OverrideOptions>,
    ) -> Option<std::collections::HashMap<String, VariableValue>> {
        match self.get_variable(feature_key, variable_key, context, options)? {
            VariableValue::Object(value) => Some(value),
            _ => None,
        }
    }
    /// Returns a variable value without imposing a more specific Rust type.
    pub fn get_variable_json(
        &self,
        feature_key: &str,
        variable_key: &str,
        context: Option<&Context>,
        options: Option<&OverrideOptions>,
    ) -> Option<VariableValue> {
        self.get_variable(feature_key, variable_key, context, options)
    }
    /// Evaluates all requested features, or every feature when no keys are supplied.
    pub fn get_feature_evaluations(
        &self,
        context: Option<&Context>,
        feature_keys: &[String],
        options: Option<&OverrideOptions>,
    ) -> EvaluatedFeatures {
        let (stored, sticky, _) = self.options();
        let mut merged = stored;
        if let Some(context) = context {
            merged.extend(context.clone());
        }
        self.parent.get_feature_evaluations_with_sticky(
            Some(&merged),
            feature_keys,
            options,
            sticky,
        )
    }

    /// Evaluates a global variable and returns evaluation details.
    pub fn evaluate_global_variable(
        &self,
        variable_key: &str,
        context: Option<&Context>,
        options: Option<&OverrideOptions>,
    ) -> Evaluation {
        let (stored, sticky_features, sticky_variables) = self.options();
        let mut merged = stored;
        if let Some(context) = context {
            merged.extend(context.clone());
        }
        self.parent.evaluate_global_variable_with_sticky(
            variable_key,
            Some(&merged),
            options,
            sticky_features,
            sticky_variables,
        )
    }

    /// Returns a global variable value, if one is available.
    pub fn get_global_variable(
        &self,
        variable_key: &str,
        context: Option<&Context>,
        options: Option<&OverrideOptions>,
    ) -> Option<VariableValue> {
        let evaluation = self.evaluate_global_variable(variable_key, context, options);
        let is_json = evaluation
            .variable
            .as_ref()
            .map(|variable| variable.variable_type == "json")
            .unwrap_or(false);
        let value = evaluation.variable_value?;
        if is_json {
            if let VariableValue::String(string) = &value {
                return serde_json::from_str::<serde_json::Value>(string)
                    .ok()
                    .map(VariableValue::from_json);
            }
        }
        Some(value)
    }

    /// Returns a global variable as a boolean when its value has that type.
    pub fn get_global_variable_boolean(
        &self,
        key: &str,
        context: Option<&Context>,
        options: Option<&OverrideOptions>,
    ) -> Option<bool> {
        match self.get_global_variable(key, context, options)? {
            VariableValue::Boolean(value) => Some(value),
            _ => None,
        }
    }
    /// Returns a global variable as a string when its value has that type.
    pub fn get_global_variable_string(
        &self,
        key: &str,
        context: Option<&Context>,
        options: Option<&OverrideOptions>,
    ) -> Option<String> {
        match self.get_global_variable(key, context, options)? {
            VariableValue::String(value) => Some(value),
            _ => None,
        }
    }
    /// Returns a global variable as an integer when its value has that type.
    pub fn get_global_variable_integer(
        &self,
        key: &str,
        context: Option<&Context>,
        options: Option<&OverrideOptions>,
    ) -> Option<i64> {
        match self.get_global_variable(key, context, options)? {
            VariableValue::Integer(value) => Some(value),
            VariableValue::Double(value) if value.is_finite() && value.fract() == 0.0 => {
                Some(value as i64)
            }
            _ => None,
        }
    }
    /// Returns a global variable as a double when its value has that type.
    pub fn get_global_variable_double(
        &self,
        key: &str,
        context: Option<&Context>,
        options: Option<&OverrideOptions>,
    ) -> Option<f64> {
        match self.get_global_variable(key, context, options)? {
            VariableValue::Integer(value) => Some(value as f64),
            VariableValue::Double(value) if value.is_finite() => Some(value),
            _ => None,
        }
    }
    /// Returns a global variable as an array when its value has that type.
    pub fn get_global_variable_array(
        &self,
        key: &str,
        context: Option<&Context>,
        options: Option<&OverrideOptions>,
    ) -> Option<Vec<VariableValue>> {
        match self.get_global_variable(key, context, options)? {
            VariableValue::Array(value) => Some(value),
            _ => None,
        }
    }
    /// Returns a global variable as an object when its value has that type.
    pub fn get_global_variable_object(
        &self,
        key: &str,
        context: Option<&Context>,
        options: Option<&OverrideOptions>,
    ) -> Option<std::collections::HashMap<String, VariableValue>> {
        match self.get_global_variable(key, context, options)? {
            VariableValue::Object(value) => Some(value),
            _ => None,
        }
    }
    /// Returns a global variable value without imposing a more specific Rust type.
    pub fn get_global_variable_json(
        &self,
        key: &str,
        context: Option<&Context>,
        options: Option<&OverrideOptions>,
    ) -> Option<VariableValue> {
        self.get_global_variable(key, context, options)
    }

    /// Evaluates requested global variables, or every global variable when no keys are supplied.
    pub fn get_global_variable_evaluations(
        &self,
        context: Option<&Context>,
        variable_keys: &[String],
        options: Option<&OverrideOptions>,
    ) -> EvaluatedVariables {
        let keys = if variable_keys.is_empty() {
            self.parent.get_global_variable_keys()
        } else {
            variable_keys.to_vec()
        };
        keys.into_iter()
            .filter_map(|key| {
                self.get_global_variable(&key, context, options)
                    .map(|value| (key, value))
            })
            .collect()
    }
}
