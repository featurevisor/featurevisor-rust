use crate::diagnostics::{Diagnostic, DiagnosticHandler, LogLevel};
use crate::evaluate::{Evaluation, EvaluateOptions};
use crate::types::{BucketBy, Context};
use crate::Unsubscribe;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct ConfigureBucketKeyOptions {
    pub feature_key: String,
    pub context: Context,
    pub bucket_by: BucketBy,
    pub bucket_key: String,
}

#[derive(Clone, Debug)]
pub struct ConfigureBucketValueOptions {
    pub feature_key: String,
    pub bucket_key: String,
    pub context: Context,
    pub bucket_value: u32,
}

pub trait FeaturevisorModule: Send + Sync {
    fn name(&self) -> Option<&str> {
        None
    }

    fn setup(&self, _api: &ModuleApi) {}

    fn before(&self, options: EvaluateOptions) -> EvaluateOptions {
        options
    }

    fn bucket_key(&self, options: ConfigureBucketKeyOptions) -> String {
        options.bucket_key
    }

    fn bucket_value(&self, options: ConfigureBucketValueOptions) -> u32 {
        options.bucket_value
    }

    fn after(&self, evaluation: Evaluation, _options: &EvaluateOptions) -> Evaluation {
        evaluation
    }

    fn close(&self) {}
}

pub struct ModuleApi {
    get_revision: Arc<dyn Fn() -> String + Send + Sync>,
    on_diagnostic: Arc<dyn Fn(DiagnosticHandler, LogLevel) -> Unsubscribe + Send + Sync>,
    report_diagnostic: Arc<dyn Fn(Diagnostic) + Send + Sync>,
}

impl ModuleApi {
    pub fn get_revision(&self) -> String {
        (self.get_revision)()
    }

    pub fn on_diagnostic(
        &self,
        handler: DiagnosticHandler,
        log_level: Option<LogLevel>,
    ) -> Unsubscribe {
        (self.on_diagnostic)(handler, log_level.unwrap_or(LogLevel::Info))
    }

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
