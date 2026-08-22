use serde::Serialize;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Fatal,
    Error,
    Warn,
    #[default]
    Info,
    Debug,
}

impl LogLevel {
    pub fn allows(self, diagnostic_level: LogLevel) -> bool {
        self >= diagnostic_level
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
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

pub type DiagnosticHandler = Arc<dyn Fn(&Diagnostic) + Send + Sync>;
