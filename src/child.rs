use crate::events::{ContextSetDetails, EventDetails, EventHandler, EventName, StickySetDetails};
use crate::instance::{Featurevisor, OverrideOptions};
use crate::evaluate::{Evaluation, EvaluationType};
use crate::types::{Context, EvaluatedFeatures, StickyFeatures, VariableValue};
use crate::emitter::Emitter;
use crate::Unsubscribe;
use std::sync::{Arc, Mutex};

struct ChildInner {
    context: Context,
    sticky: StickyFeatures,
    emitter: Emitter,
    parent_unsubscribers: Vec<Unsubscribe>,
    closed: bool,
}

#[derive(Clone)]
pub struct FeaturevisorChild {
    parent: Featurevisor,
    inner: Arc<Mutex<ChildInner>>,
}

impl FeaturevisorChild {
    pub(crate) fn new(parent: Featurevisor, context: Context, sticky: StickyFeatures) -> Self {
        Self { parent, inner: Arc::new(Mutex::new(ChildInner { context, sticky, emitter: Emitter::default(), parent_unsubscribers: Vec::new(), closed: false })) }
    }

    fn options(&self) -> (Context, StickyFeatures) { self.inner.lock().map(|inner| (inner.context.clone(), inner.sticky.clone())).unwrap_or_default() }

    pub fn set_context(&self, context: Context, replace: bool) { let (context, emitter) = { let mut inner = match self.inner.lock() { Ok(inner) => inner, Err(_) => return }; if inner.closed { return; } if replace { inner.context = context; } else { inner.context.extend(context); } (inner.context.clone(), inner.emitter.clone()) }; emitter.emit(EventName::ContextSet, EventDetails::ContextSet(ContextSetDetails { context, replaced: replace })); }
    pub fn get_context(&self, context: Option<&Context>) -> Context { let (stored, _) = self.options(); let mut merged = stored; if let Some(context) = context { merged.extend(context.clone()); } self.parent.get_context(Some(&merged)) }
    pub fn set_sticky(&self, sticky: StickyFeatures, replace: bool) { let (features, emitter) = { let mut inner = match self.inner.lock() { Ok(inner) => inner, Err(_) => return }; if inner.closed { return; } if replace { inner.sticky = sticky; } else { inner.sticky.extend(sticky); } (inner.sticky.keys().cloned().collect(), inner.emitter.clone()) }; emitter.emit(EventName::StickySet, EventDetails::StickySet(StickySetDetails { features, replaced: replace })); }

    pub fn on(&self, event: EventName, callback: EventHandler) -> Unsubscribe {
        if matches!(event, EventName::ContextSet | EventName::StickySet) { return self.inner.lock().map(|inner| inner.emitter.on(event, callback)).unwrap_or_else(|_| Box::new(|| {})); }
        let unsubscribe = self.parent.parent_on(event, callback);
        let shared = Arc::new(Mutex::new(Some(unsubscribe)));
        let shared_for_child = Arc::clone(&shared);
        let shared_for_user = Arc::clone(&shared);
        let child_cleanup: Unsubscribe = Box::new(move || { if let Ok(mut unsubscribe) = shared_for_child.lock() { if let Some(unsubscribe) = unsubscribe.take() { unsubscribe(); } } });
        if let Ok(mut inner) = self.inner.lock() { if inner.closed { drop(inner); child_cleanup(); return Box::new(|| {}); } inner.parent_unsubscribers.push(child_cleanup); }
        Box::new(move || { if let Ok(mut unsubscribe) = shared_for_user.lock() { if let Some(unsubscribe) = unsubscribe.take() { unsubscribe(); } } })
    }

    pub fn close(&self) { let unsubscribers = { let mut inner = match self.inner.lock() { Ok(inner) => inner, Err(_) => return }; if inner.closed { return; } inner.closed = true; inner.emitter.clear(); std::mem::take(&mut inner.parent_unsubscribers) }; for unsubscribe in unsubscribers { unsubscribe(); } }

    fn evaluate(&self, evaluation_type: EvaluationType, feature_key: &str, variable_key: Option<&str>, context: Option<&Context>, options: Option<&OverrideOptions>) -> Evaluation { let (stored, sticky) = self.options(); let mut merged = stored; if let Some(context) = context { merged.extend(context.clone()); } self.parent.evaluate_child(evaluation_type, feature_key, variable_key, Some(&merged), options, sticky) }
    pub fn evaluate_flag(&self, feature_key: &str, context: Option<&Context>) -> Evaluation { self.evaluate(EvaluationType::Flag, feature_key, None, context, None) }
    pub fn is_enabled(&self, feature_key: &str, context: Option<&Context>) -> bool { self.evaluate_flag(feature_key, context).enabled == Some(true) }
    pub fn evaluate_variation(&self, feature_key: &str, context: Option<&Context>, options: Option<&OverrideOptions>) -> Evaluation { self.evaluate(EvaluationType::Variation, feature_key, None, context, options) }
    pub fn get_variation(&self, feature_key: &str, context: Option<&Context>, options: Option<&OverrideOptions>) -> Option<String> { let evaluation = self.evaluate_variation(feature_key, context, options); evaluation.variation_value.or_else(|| evaluation.variation.map(|variation| variation.value)) }
    pub fn evaluate_variable(&self, feature_key: &str, variable_key: &str, context: Option<&Context>, options: Option<&OverrideOptions>) -> Evaluation { self.evaluate(EvaluationType::Variable, feature_key, Some(variable_key), context, options) }
    pub fn get_variable(&self, feature_key: &str, variable_key: &str, context: Option<&Context>, options: Option<&OverrideOptions>) -> Option<VariableValue> { self.parent.get_variable_with_sticky(feature_key, variable_key, self.get_context(context), options, self.options().1) }
    pub fn get_variable_boolean(&self, feature_key: &str, variable_key: &str, context: Option<&Context>, options: Option<&OverrideOptions>) -> Option<bool> { match self.get_variable(feature_key, variable_key, context, options)? { VariableValue::Boolean(value) => Some(value), _ => None } }
    pub fn get_variable_string(&self, feature_key: &str, variable_key: &str, context: Option<&Context>, options: Option<&OverrideOptions>) -> Option<String> { match self.get_variable(feature_key, variable_key, context, options)? { VariableValue::String(value) => Some(value), _ => None } }
    pub fn get_variable_integer(&self, feature_key: &str, variable_key: &str, context: Option<&Context>, options: Option<&OverrideOptions>) -> Option<i64> { match self.get_variable(feature_key, variable_key, context, options)? { VariableValue::Integer(value) => Some(value), VariableValue::Double(value) if value.is_finite() && value.fract() == 0.0 => Some(value as i64), _ => None } }
    pub fn get_variable_double(&self, feature_key: &str, variable_key: &str, context: Option<&Context>, options: Option<&OverrideOptions>) -> Option<f64> { match self.get_variable(feature_key, variable_key, context, options)? { VariableValue::Integer(value) => Some(value as f64), VariableValue::Double(value) if value.is_finite() => Some(value), _ => None } }
    pub fn get_variable_array(&self, feature_key: &str, variable_key: &str, context: Option<&Context>, options: Option<&OverrideOptions>) -> Option<Vec<VariableValue>> { match self.get_variable(feature_key, variable_key, context, options)? { VariableValue::Array(value) => Some(value), _ => None } }
    pub fn get_variable_object(&self, feature_key: &str, variable_key: &str, context: Option<&Context>, options: Option<&OverrideOptions>) -> Option<std::collections::HashMap<String, VariableValue>> { match self.get_variable(feature_key, variable_key, context, options)? { VariableValue::Object(value) => Some(value), _ => None } }
    pub fn get_variable_json(&self, feature_key: &str, variable_key: &str, context: Option<&Context>, options: Option<&OverrideOptions>) -> Option<VariableValue> { self.get_variable(feature_key, variable_key, context, options) }
    pub fn get_all_evaluations(&self, context: Option<&Context>, feature_keys: &[String], options: Option<&OverrideOptions>) -> EvaluatedFeatures { let (stored, sticky) = self.options(); let mut merged = stored; if let Some(context) = context { merged.extend(context.clone()); } self.parent.get_all_evaluations_with_sticky(Some(&merged), feature_keys, options, sticky) }
}
