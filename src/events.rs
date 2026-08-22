use crate::diagnostics::Diagnostic;
use crate::types::Context;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventName {
    DatafileSet,
    ContextSet,
    StickySet,
    Error,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatafileSetDetails {
    pub revision: String,
    pub previous_revision: String,
    pub revision_changed: bool,
    pub features: Vec<String>,
    pub replaced: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSetDetails {
    pub context: Context,
    pub replaced: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StickySetDetails {
    pub features: Vec<String>,
    pub replaced: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EventDetails {
    DatafileSet(DatafileSetDetails),
    ContextSet(ContextSetDetails),
    StickySet(StickySetDetails),
    Error { diagnostic: Diagnostic },
}

pub type EventHandler = std::sync::Arc<dyn Fn(&EventDetails) + Send + Sync>;
