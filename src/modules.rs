use crate::diagnostics::{Diagnostic, DiagnosticHandler, LogLevel};
use crate::evaluate::{EvaluateOptions, Evaluation};
use crate::types::{BucketBy, Context};
use crate::Unsubscribe;
use std::sync::Arc;

#[derive(Clone, Debug)]
/// Input and current value supplied to a module's bucket key callback.
#[allow(missing_docs)]
pub struct ConfigureBucketKeyOptions {
    pub feature_key: String,
    pub context: Context,
    pub bucket_by: BucketBy,
    pub bucket_key: String,
}

#[derive(Clone, Debug)]
/// Input and current value supplied to a module's bucket value callback.
#[allow(missing_docs)]
pub struct ConfigureBucketValueOptions {
    pub feature_key: String,
    pub bucket_key: String,
    pub context: Context,
    pub bucket_value: u32,
}

/// Extension point for setup, diagnostics, bucketing, and evaluation lifecycle hooks.
pub trait FeaturevisorModule: Send + Sync {
    /// Returns the optional unique module name.
    fn name(&self) -> Option<&str> {
        None
    }

    /// Sets up the module and optionally registers diagnostic subscriptions.
    fn setup(&self, _api: &ModuleApi) {}

    /// Transforms evaluation options before the feature is evaluated.
    fn before(&self, options: EvaluateOptions) -> EvaluateOptions {
        options
    }

    /// Transforms options before any feature or global variable evaluation.
    fn before_evaluation(&self, options: EvaluateOptions) -> EvaluateOptions {
        options
    }

    /// Transforms the bucket key used for an evaluation.
    fn bucket_key(&self, options: ConfigureBucketKeyOptions) -> String {
        options.bucket_key
    }

    /// Transforms the bucket value used for an evaluation.
    fn bucket_value(&self, options: ConfigureBucketValueOptions) -> u32 {
        options.bucket_value
    }

    /// Transforms an evaluation after defaults and evaluation details are applied.
    fn after(&self, evaluation: Evaluation, _options: &EvaluateOptions) -> Evaluation {
        evaluation
    }

    /// Transforms a result after any feature or global variable evaluation.
    fn after_evaluation(&self, evaluation: Evaluation, options: &EvaluateOptions) -> Evaluation {
        let _ = options;
        evaluation
    }

    /// Releases module resources when the module is removed or the instance closes.
    fn close(&self) {}
}

/// API exposed to a module during setup.
pub struct ModuleApi {
    pub(crate) get_revision: Arc<dyn Fn() -> String + Send + Sync>,
    pub(crate) on_diagnostic: Arc<dyn Fn(DiagnosticHandler, LogLevel) -> Unsubscribe + Send + Sync>,
    pub(crate) report_diagnostic: Arc<dyn Fn(Diagnostic) + Send + Sync>,
}

impl ModuleApi {
    /// Returns the current datafile revision.
    pub fn get_revision(&self) -> String {
        (self.get_revision)()
    }

    /// Subscribes the module to diagnostics at the requested level.
    pub fn on_diagnostic(
        &self,
        handler: DiagnosticHandler,
        log_level: Option<LogLevel>,
    ) -> Unsubscribe {
        (self.on_diagnostic)(handler, log_level.unwrap_or(LogLevel::Info))
    }

    /// Reports a diagnostic through the instance pipeline.
    pub fn report_diagnostic(&self, diagnostic: Diagnostic) {
        (self.report_diagnostic)(diagnostic)
    }
}

pub(crate) struct ModuleSubscription {
    pub id: u64,
    pub module_id: u64,
    pub handler: DiagnosticHandler,
    pub log_level: LogLevel,
}
