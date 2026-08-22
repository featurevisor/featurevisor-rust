use serde::Serialize;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
/// Diagnostic severity used by the SDK and its modules.
pub enum LogLevel {
    /// A fatal error that prevents normal operation.
    Fatal,
    /// An error that affects an operation or evaluation.
    Error,
    /// A recoverable problem or unexpected condition.
    Warn,
    #[default]
    /// Informational diagnostic messages.
    Info,
    /// Detailed diagnostic messages for debugging.
    Debug,
}

impl LogLevel {
    /// Returns whether this configured level accepts a diagnostic level.
    pub fn allows(self, diagnostic_level: LogLevel) -> bool {
        self >= diagnostic_level
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
/// A diagnostic emitted by the SDK or one of its modules.
#[allow(missing_docs)]
pub struct Diagnostic {
    pub level: LogLevel,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_error: Option<String>,
    pub details: HashMap<String, JsonValue>,
}

impl Diagnostic {
    /// Creates a diagnostic with empty details and no module metadata.
    pub fn new(level: LogLevel, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            level,
            code: code.into(),
            message: message.into(),
            module: None,
            module_name: None,
            original_error: None,
            details: HashMap::new(),
        }
    }
}

/// A thread safe callback that receives SDK diagnostics.
pub type DiagnosticHandler = Arc<dyn Fn(&Diagnostic) + Send + Sync>;
