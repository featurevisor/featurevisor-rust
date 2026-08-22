use crate::child::FeaturevisorChild;
use crate::diagnostics::{Diagnostic, DiagnosticHandler, LogLevel};
use crate::emitter::Emitter;
use crate::evaluate::{
    evaluate_all, evaluate_with_modules, EvaluateOptions, Evaluation, EvaluationData,
    EvaluationType,
};
use crate::events::{
    ContextSetDetails, DatafileSetDetails, EventDetails, EventHandler, EventName, StickySetDetails,
};
use crate::helpers::panic_message;
use crate::modules::{FeaturevisorModule, ModuleApi, ModuleSubscription};
use crate::types::{
    Context, DatafileContent, DatafileInput, EvaluatedFeatures, Feature, Segment, StickyFeatures,
    VariableValue,
};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

const EMPTY_REVISION: &str = "unknown";

#[derive(Clone, Debug, Default)]
/// Options that override defaults for a variation or variable evaluation.
#[allow(missing_docs)]
pub struct OverrideOptions {
    pub default_variation_value: Option<String>,
    pub default_variable_value: Option<VariableValue>,
}

#[derive(Clone, Debug, Default)]
/// Options used when creating a child evaluator.
#[allow(missing_docs)]
pub struct SpawnOptions {
    pub sticky: Option<StickyFeatures>,
}

#[derive(Default)]
/// Configuration for a [`Featurevisor`] instance.
#[allow(missing_docs)]
pub struct FeaturevisorOptions {
    pub datafile: Option<DatafileInput>,
    pub context: Option<Context>,
    pub log_level: Option<LogLevel>,
    pub on_diagnostic: Option<DiagnosticHandler>,
    pub sticky: Option<StickyFeatures>,
    pub modules: Vec<Arc<dyn FeaturevisorModule>>,
}

struct ModuleRecord {
    id: u64,
    name: Option<String>,
    module: Arc<dyn FeaturevisorModule>,
}

struct Inner {
    datafile: Arc<DatafileContent>,
    context: Context,
    sticky: StickyFeatures,
    log_level: LogLevel,
    on_diagnostic: Option<DiagnosticHandler>,
    modules: Vec<ModuleRecord>,
    subscriptions: Vec<ModuleSubscription>,
    emitter: Emitter,
    regex_cache: Arc<RwLock<HashMap<String, regex::Regex>>>,
    closed: bool,
    next_module_id: u64,
}

type Snapshot = (
    Arc<DatafileContent>,
    Context,
    StickyFeatures,
    LogLevel,
    Vec<Arc<dyn FeaturevisorModule>>,
    Arc<RwLock<HashMap<String, regex::Regex>>>,
);

#[derive(Clone)]
/// A thread safe Featurevisor v3 datafile evaluator.
pub struct Featurevisor {
    inner: Arc<Mutex<Inner>>,
    next_subscription_id: Arc<AtomicU64>,
}

/// Creates a Featurevisor evaluator from the supplied options.
pub fn create_featurevisor(options: FeaturevisorOptions) -> Featurevisor {
    let instance = Featurevisor {
        inner: Arc::new(Mutex::new(Inner {
            datafile: Arc::new(DatafileContent::default()),
            context: options.context.unwrap_or_default(),
            sticky: options.sticky.unwrap_or_default(),
            log_level: options.log_level.unwrap_or_default(),
            on_diagnostic: options.on_diagnostic,
            modules: Vec::new(),
            subscriptions: Vec::new(),
            emitter: Emitter::default(),
            regex_cache: Arc::new(RwLock::new(HashMap::new())),
            closed: false,
            next_module_id: 1,
        })),
        next_subscription_id: Arc::new(AtomicU64::new(1)),
    };
    for module in options.modules {
        instance.add_module(module);
    }
    if let Some(datafile) = options.datafile {
        instance.set_datafile(datafile, true);
    }
    instance.report_diagnostic(
        Diagnostic::new(LogLevel::Info, "sdk_initialized", "SDK initialized"),
        None,
    );
    instance
}

impl Featurevisor {
    fn snapshot(&self) -> Snapshot {
        let inner = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        (
            Arc::clone(&inner.datafile),
            inner.context.clone(),
            inner.sticky.clone(),
            inner.log_level,
            inner
                .modules
                .iter()
                .map(|record| Arc::clone(&record.module))
                .collect(),
            Arc::clone(&inner.regex_cache),
        )
    }

    fn module_api(&self, id: u64, name: Option<String>) -> ModuleApi {
        let instance_for_revision = self.clone();
        let instance_for_subscribe = self.clone();
        let instance_for_report = self.clone();
        let module_name = name.clone();
        ModuleApi {
            get_revision: Arc::new(move || instance_for_revision.get_revision()),
            on_diagnostic: Arc::new(move |handler, log_level| {
                instance_for_subscribe.subscribe_module(id, handler, log_level)
            }),
            report_diagnostic: Arc::new(move |mut diagnostic| {
                diagnostic.module = module_name.clone();
                instance_for_report.report_diagnostic(diagnostic, Some(id));
            }),
        }
    }

    fn subscribe_module(
        &self,
        module_id: u64,
        handler: DiagnosticHandler,
        log_level: LogLevel,
    ) -> crate::Unsubscribe {
        let subscription_id = self.next_subscription_id.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut inner) = self.inner.lock() {
            if !inner.closed {
                inner.subscriptions.push(ModuleSubscription {
                    id: subscription_id,
                    module_id,
                    handler,
                    log_level,
                });
            }
        }
        let instance = self.clone();
        let active = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let active_for_closure = Arc::clone(&active);
        Box::new(move || {
            if !active_for_closure.swap(false, Ordering::AcqRel) {
                return;
            }
            if let Ok(mut inner) = instance.inner.lock() {
                inner
                    .subscriptions
                    .retain(|subscription| subscription.id != subscription_id);
            }
        })
    }

    fn clear_module_subscriptions(&self, module_id: u64) {
        if let Ok(mut inner) = self.inner.lock() {
            inner
                .subscriptions
                .retain(|subscription| subscription.module_id != module_id);
        }
    }

    fn report_diagnostic(&self, diagnostic: Diagnostic, source_module: Option<u64>) {
        let (subscriptions, handler, log_level, emitter) = match self.inner.lock() {
            Ok(inner) => (
                inner
                    .subscriptions
                    .iter()
                    .filter(|subscription| Some(subscription.module_id) != source_module)
                    .map(|subscription| {
                        (
                            subscription.module_id,
                            Arc::clone(&subscription.handler),
                            subscription.log_level,
                        )
                    })
                    .collect::<Vec<_>>(),
                inner.on_diagnostic.clone(),
                inner.log_level,
                inner.emitter.clone(),
            ),
            Err(_) => return,
        };
        for (_, handler, subscription_level) in subscriptions {
            if subscription_level.allows(diagnostic.level) {
                let _ = catch_unwind(AssertUnwindSafe(|| handler(&diagnostic)));
            }
        }
        if log_level.allows(diagnostic.level) {
            if let Some(handler) = handler {
                let _ = catch_unwind(AssertUnwindSafe(|| handler(&diagnostic)));
            }
        }
        if diagnostic.level == LogLevel::Error {
            emitter.emit(EventName::Error, EventDetails::Error { diagnostic });
        }
    }

    /// Sets the minimum diagnostic level delivered to the instance handler.
    pub fn set_log_level(&self, level: LogLevel) {
        if let Ok(mut inner) = self.inner.lock() {
            if !inner.closed {
                inner.log_level = level;
            }
        }
    }

    /// Merges a datafile into the current one, or replaces it when `replace` is true.
    pub fn set_datafile(&self, input: DatafileInput, replace: bool) {
        let parsed = match input {
            DatafileInput::Content(content) => Some(content),
            DatafileInput::Json(json) => match serde_json::from_str::<DatafileContent>(&json) {
                Ok(content) => Some(content),
                Err(error) => {
                    let mut diagnostic = Diagnostic::new(
                        LogLevel::Error,
                        "invalid_datafile",
                        "Could not parse datafile",
                    );
                    diagnostic.original_error = Some(error.to_string());
                    self.report_diagnostic(diagnostic, None);
                    return;
                }
            },
        };
        let Some(incoming) = parsed.filter(|datafile| !datafile.revision.is_empty()) else {
            let diagnostic = Diagnostic::new(
                LogLevel::Error,
                "invalid_datafile",
                "Could not parse datafile",
            );
            self.report_diagnostic(diagnostic, None);
            return;
        };
        let (previous, next, emitter) = {
            let mut inner = match self.inner.lock() {
                Ok(inner) => inner,
                Err(_) => return,
            };
            if inner.closed {
                return;
            }
            let previous = Arc::clone(&inner.datafile);
            let next = if replace {
                incoming
            } else {
                merge_datafile(&previous, incoming)
            };
            inner.datafile = Arc::new(next.clone());
            if let Ok(mut cache) = inner.regex_cache.write() {
                cache.clear();
            }
            (previous, next, inner.emitter.clone())
        };
        let details = datafile_details(&previous, &next, replace);
        let mut diagnostic = Diagnostic::new(LogLevel::Info, "datafile_set", "Datafile set");
        diagnostic.details = serde_json::to_value(&details)
            .ok()
            .and_then(|value| value.as_object().cloned())
            .map(|value| value.into_iter().collect())
            .unwrap_or_default();
        self.report_diagnostic(diagnostic, None);
        emitter.emit(EventName::DatafileSet, EventDetails::DatafileSet(details));
    }

    /// Returns the current datafile revision.
    pub fn get_revision(&self) -> String {
        self.inner
            .lock()
            .map(|inner| inner.datafile.revision.clone())
            .unwrap_or_else(|_| EMPTY_REVISION.to_string())
    }
    /// Returns the current datafile schema version.
    pub fn get_schema_version(&self) -> String {
        self.inner
            .lock()
            .map(|inner| inner.datafile.schema_version.clone())
            .unwrap_or_else(|_| "2".to_string())
    }
    /// Returns a feature definition by key.
    pub fn get_feature(&self, feature_key: &str) -> Option<Feature> {
        self.inner
            .lock()
            .ok()
            .and_then(|inner| inner.datafile.features.get(feature_key).cloned())
    }
    /// Returns a segment definition by key.
    pub fn get_segment(&self, segment_key: &str) -> Option<Segment> {
        let mut segment = self
            .inner
            .lock()
            .ok()
            .and_then(|inner| inner.datafile.segments.get(segment_key).cloned())?;
        if let JsonValue::String(value) = &segment.conditions {
            if value == "*" {
                segment.conditions = JsonValue::String(value.clone());
            } else if let Ok(parsed) = serde_json::from_str(value) {
                segment.conditions = parsed;
            }
        }
        Some(segment)
    }
    /// Returns the keys of all features in the current datafile.
    pub fn get_feature_keys(&self) -> Vec<String> {
        self.inner
            .lock()
            .map(|inner| inner.datafile.features.keys().cloned().collect())
            .unwrap_or_default()
    }
    /// Returns the variable keys defined for a feature.
    pub fn get_variable_keys(&self, feature_key: &str) -> Vec<String> {
        self.get_feature(feature_key)
            .and_then(|feature| {
                feature
                    .variables_schema
                    .map(|schema| schema.keys().cloned().collect())
            })
            .unwrap_or_default()
    }
    /// Returns whether a feature defines at least one variation.
    pub fn has_variations(&self, feature_key: &str) -> bool {
        self.get_feature(feature_key)
            .and_then(|feature| feature.variations.map(|variations| !variations.is_empty()))
            .unwrap_or(false)
    }

    #[cfg(feature = "cli")]
    pub(crate) fn segment_matches(&self, segment_key: &str, context: &Context) -> bool {
        let (datafile, _, _, _, _, regex_cache) = self.snapshot();
        let data = EvaluationData {
            datafile,
            regex_cache,
        };
        let report = |_diagnostic: Diagnostic| {};
        data.all_segments(
            &JsonValue::String(segment_key.to_string()),
            context,
            &report,
        )
    }

    /// Updates the stored context, either merging with or replacing it.
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
                context: context.clone(),
                replaced: replace,
            }),
        );
        let mut diagnostic = Diagnostic::new(
            LogLevel::Debug,
            "context_set",
            if replace {
                "Context replaced"
            } else {
                "Context updated"
            },
        );
        diagnostic.details.insert(
            "context".to_string(),
            serde_json::to_value(&context).unwrap_or(JsonValue::Null),
        );
        diagnostic
            .details
            .insert("replaced".to_string(), JsonValue::Bool(replace));
        self.report_diagnostic(diagnostic, None);
    }

    /// Returns the stored context merged with an optional per evaluation context.
    pub fn get_context(&self, context: Option<&Context>) -> Context {
        let (_, stored, _, _, _, _) = self.snapshot();
        let mut result = stored;
        if let Some(context) = context {
            result.extend(context.clone());
        }
        result
    }
    /// Updates sticky evaluations, either merging with or replacing them.
    pub fn set_sticky(&self, sticky: StickyFeatures, replace: bool) {
        let (features, emitter) = {
            let mut inner = match self.inner.lock() {
                Ok(inner) => inner,
                Err(_) => return,
            };
            if inner.closed {
                return;
            }
            let mut features: Vec<String> = inner.sticky.keys().cloned().collect();
            if replace {
                inner.sticky = sticky;
            } else {
                inner.sticky.extend(sticky);
            }
            for key in inner.sticky.keys() {
                if !features.contains(key) {
                    features.push(key.clone());
                }
            }
            (features, inner.emitter.clone())
        };
        let details = StickySetDetails {
            features,
            replaced: replace,
        };
        let mut diagnostic = Diagnostic::new(LogLevel::Info, "sticky_set", "Sticky features set");
        diagnostic.details = serde_json::to_value(&details)
            .ok()
            .and_then(|value| value.as_object().cloned())
            .map(|value| value.into_iter().collect())
            .unwrap_or_default();
        self.report_diagnostic(diagnostic, None);
        emitter.emit(EventName::StickySet, EventDetails::StickySet(details));
    }

    /// Registers a module and returns an idempotent cleanup function.
    pub fn add_module(&self, module: Arc<dyn FeaturevisorModule>) -> Option<crate::Unsubscribe> {
        let (id, name) = {
            let mut inner = self.inner.lock().ok()?;
            if inner.closed {
                return None;
            }
            let name = module.name().map(str::to_string);
            if name.as_ref().is_some_and(|name| {
                inner
                    .modules
                    .iter()
                    .any(|existing| existing.name.as_ref() == Some(name))
            }) {
                let mut diagnostic =
                    Diagnostic::new(LogLevel::Error, "duplicate_module", "Duplicate module name");
                diagnostic.module_name = name;
                drop(inner);
                self.report_diagnostic(diagnostic, None);
                return None;
            }
            let id = inner.next_module_id;
            inner.next_module_id += 1;
            (id, name)
        };
        let api = self.module_api(id, name.clone());
        let setup = catch_unwind(AssertUnwindSafe(|| module.setup(&api)));
        if let Err(error) = setup {
            self.clear_module_subscriptions(id);
            let mut diagnostic =
                Diagnostic::new(LogLevel::Error, "module_setup_error", "Module setup failed");
            diagnostic.module_name = name.clone();
            diagnostic.original_error = Some(panic_message(error.as_ref()));
            self.report_diagnostic(diagnostic, None);
            self.close_module(module, name);
            return None;
        }
        if let Ok(mut inner) = self.inner.lock() {
            if inner.closed {
                drop(inner);
                self.clear_module_subscriptions(id);
                self.close_module(module, name);
                return None;
            }
            inner.modules.push(ModuleRecord {
                id,
                name,
                module: Arc::clone(&module),
            });
        }
        let instance = self.clone();
        let active = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let active_for_closure = Arc::clone(&active);
        Some(Box::new(move || {
            if !active_for_closure.swap(false, std::sync::atomic::Ordering::AcqRel) {
                return;
            }
            instance.remove_module_by_id(id);
        }))
    }

    fn remove_module_by_id(&self, id: u64) {
        let module = self.inner.lock().ok().and_then(|mut inner| {
            let index = inner.modules.iter().position(|module| module.id == id)?;
            let record = inner.modules.remove(index);
            Some((record.module, record.name))
        });
        self.clear_module_subscriptions(id);
        if let Some((module, name)) = module {
            self.close_module(module, name);
        }
    }
    /// Removes and closes all modules with the supplied name.
    pub fn remove_module(&self, name: &str) {
        let modules = self
            .inner
            .lock()
            .ok()
            .map(|mut inner| {
                let mut removed = Vec::new();
                let mut remaining = Vec::new();
                for record in inner.modules.drain(..) {
                    if record.name.as_deref() == Some(name) {
                        removed.push(record);
                    } else {
                        remaining.push(record);
                    }
                }
                inner.modules = remaining;
                removed
            })
            .unwrap_or_default();
        for record in modules {
            self.clear_module_subscriptions(record.id);
            self.close_module(record.module, record.name);
        }
    }
    fn close_module(&self, module: Arc<dyn FeaturevisorModule>, name: Option<String>) {
        if let Err(error) = catch_unwind(AssertUnwindSafe(|| module.close())) {
            let mut diagnostic =
                Diagnostic::new(LogLevel::Error, "module_close_error", "Module close failed");
            diagnostic.module_name = name;
            diagnostic.original_error = Some(panic_message(error.as_ref()));
            self.report_diagnostic(diagnostic, None);
        }
    }

    /// Subscribes to an instance event and returns an idempotent cleanup function.
    pub fn on(&self, event: EventName, callback: EventHandler) -> crate::Unsubscribe {
        self.inner
            .lock()
            .map(|inner| inner.emitter.on(event, callback))
            .unwrap_or_else(|_| Box::new(|| {}))
    }

    fn evaluate_options(
        &self,
        evaluation_type: EvaluationType,
        feature_key: &str,
        context: Option<&Context>,
        options: Option<&OverrideOptions>,
    ) -> EvaluateOptions {
        let (datafile, stored_context, sticky, _, modules, regex_cache) = self.snapshot();
        let mut evaluation_context = stored_context;
        if let Some(context) = context {
            evaluation_context.extend(context.clone());
        }
        let options = options.cloned().unwrap_or_default();
        EvaluateOptions {
            evaluation_type,
            feature_key: feature_key.to_string(),
            variable_key: None,
            context: evaluation_context,
            default_variation_value: options.default_variation_value,
            default_variable_value: options.default_variable_value,
            sticky: Some(sticky),
            data: Arc::new(EvaluationData {
                datafile,
                regex_cache,
            }),
            modules: Arc::new(modules),
            report: Arc::new({
                let instance = self.clone();
                move |diagnostic| instance.report_diagnostic(diagnostic, None)
            }),
        }
    }
    fn evaluate_options_variable(
        &self,
        feature_key: &str,
        variable_key: &str,
        context: Option<&Context>,
        options: Option<&OverrideOptions>,
    ) -> EvaluateOptions {
        let mut options =
            self.evaluate_options(EvaluationType::Variable, feature_key, context, options);
        options.variable_key = Some(variable_key.to_string());
        options
    }
    /// Evaluates a feature as a flag and returns evaluation details.
    pub fn evaluate_flag(&self, feature_key: &str, context: Option<&Context>) -> Evaluation {
        evaluate_with_modules(self.evaluate_options(
            EvaluationType::Flag,
            feature_key,
            context,
            None,
        ))
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
        evaluate_with_modules(self.evaluate_options(
            EvaluationType::Variation,
            feature_key,
            context,
            options,
        ))
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
        evaluate_with_modules(self.evaluate_options_variable(
            feature_key,
            variable_key,
            context,
            options,
        ))
    }
    /// Returns a feature variable value, if one is available.
    pub fn get_variable(
        &self,
        feature_key: &str,
        variable_key: &str,
        context: Option<&Context>,
        options: Option<&OverrideOptions>,
    ) -> Option<VariableValue> {
        let evaluation = self.evaluate_variable(feature_key, variable_key, context, options);
        let value = evaluation.variable_value?;
        if evaluation
            .variable_schema
            .as_ref()
            .map(|schema| schema.variable_type == "json")
            .unwrap_or(false)
        {
            if let VariableValue::String(value) = value {
                return serde_json::from_str::<JsonValue>(&value)
                    .ok()
                    .map(VariableValue::from_json);
            }
        }
        Some(value)
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
    ) -> Option<HashMap<String, VariableValue>> {
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
    pub fn get_all_evaluations(
        &self,
        context: Option<&Context>,
        feature_keys: &[String],
        options: Option<&OverrideOptions>,
    ) -> EvaluatedFeatures {
        let eval_options = self.evaluate_options(EvaluationType::Flag, "", context, options);
        let keys = if feature_keys.is_empty() {
            self.get_feature_keys()
        } else {
            feature_keys.to_vec()
        };
        evaluate_all(&eval_options, &keys)
    }
    /// Creates a child evaluator with its own context and sticky state.
    pub fn spawn(&self, context: Context, options: SpawnOptions) -> FeaturevisorChild {
        FeaturevisorChild::new(
            self.clone(),
            self.get_context(Some(&context)),
            options.sticky.unwrap_or_default(),
        )
    }
    /// Closes the instance, modules, subscriptions, and event listeners.
    pub fn close(&self) {
        let modules = {
            let mut inner = match self.inner.lock() {
                Ok(inner) => inner,
                Err(_) => return,
            };
            if inner.closed {
                return;
            }
            inner.closed = true;
            inner.subscriptions.clear();
            inner.emitter.clear();
            std::mem::take(&mut inner.modules)
        };
        for record in modules {
            self.close_module(record.module, record.name);
        }
    }

    pub(crate) fn parent_on(&self, event: EventName, callback: EventHandler) -> crate::Unsubscribe {
        self.on(event, callback)
    }

    pub(crate) fn evaluate_child(
        &self,
        evaluation_type: EvaluationType,
        feature_key: &str,
        variable_key: Option<&str>,
        context: Option<&Context>,
        options: Option<&OverrideOptions>,
        sticky: StickyFeatures,
    ) -> Evaluation {
        let mut evaluate_options =
            self.evaluate_options(evaluation_type, feature_key, context, options);
        evaluate_options.variable_key = variable_key.map(str::to_string);
        evaluate_options.sticky = Some(sticky);
        evaluate_with_modules(evaluate_options)
    }

    pub(crate) fn get_variable_with_sticky(
        &self,
        feature_key: &str,
        variable_key: &str,
        context: Context,
        options: Option<&OverrideOptions>,
        sticky: StickyFeatures,
    ) -> Option<VariableValue> {
        let evaluation = self.evaluate_child(
            EvaluationType::Variable,
            feature_key,
            Some(variable_key),
            Some(&context),
            options,
            sticky,
        );
        evaluation.variable_value
    }

    pub(crate) fn get_all_evaluations_with_sticky(
        &self,
        context: Option<&Context>,
        feature_keys: &[String],
        options: Option<&OverrideOptions>,
        sticky: StickyFeatures,
    ) -> EvaluatedFeatures {
        let mut evaluate_options =
            self.evaluate_options(EvaluationType::Flag, "", context, options);
        evaluate_options.sticky = Some(sticky);
        let keys = if feature_keys.is_empty() {
            self.get_feature_keys()
        } else {
            feature_keys.to_vec()
        };
        evaluate_all(&evaluate_options, &keys)
    }
}

fn merge_datafile(previous: &DatafileContent, incoming: DatafileContent) -> DatafileContent {
    let mut segments = previous.segments.clone();
    segments.extend(incoming.segments);
    let mut features = previous.features.clone();
    features.extend(incoming.features);
    DatafileContent {
        schema_version: incoming.schema_version,
        revision: incoming.revision,
        featurevisor_version: incoming.featurevisor_version,
        segments,
        features,
    }
}
fn datafile_details(
    previous: &DatafileContent,
    next: &DatafileContent,
    replaced: bool,
) -> DatafileSetDetails {
    let mut features = Vec::new();
    for (key, old) in &previous.features {
        match next.features.get(key) {
            None => features.push(key.clone()),
            Some(new) if old.hash != new.hash => features.push(key.clone()),
            _ => {}
        }
    }
    for key in next.features.keys() {
        if !previous.features.contains_key(key) {
            features.push(key.clone());
        }
    }
    DatafileSetDetails {
        revision: next.revision.clone(),
        previous_revision: previous.revision.clone(),
        revision_changed: previous.revision != next.revision,
        features,
        replaced,
    }
}
