use crate::diagnostics::Diagnostic;
use crate::types::Context;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
/// Events emitted by a Featurevisor instance.
pub enum EventName {
    /// A datafile was merged or replaced.
    DatafileSet,
    /// Stored context was merged or replaced.
    ContextSet,
    /// Sticky feature evaluations were merged or replaced.
    StickyFeaturesSet,
    /// Sticky global variables were merged or replaced.
    StickyVariablesSet,
    /// An error diagnostic was emitted.
    Error,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
/// Details emitted with a datafile update event.
#[allow(missing_docs)]
pub struct DatafileSetDetails {
    pub revision: String,
    pub previous_revision: String,
    pub revision_changed: bool,
    pub features: Vec<String>,
    pub variables: Vec<String>,
    pub replaced: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
/// Details emitted with a context update event.
#[allow(missing_docs)]
pub struct ContextSetDetails {
    pub context: Context,
    pub replaced: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
/// Details emitted with a sticky evaluation update event.
#[allow(missing_docs)]
pub struct StickyFeaturesSetDetails {
    pub features: Vec<String>,
    pub replaced: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
/// Details emitted with a sticky global variable update event.
pub struct StickyVariablesSetDetails {
    /// Global variable keys affected by the update.
    pub variables: Vec<String>,
    /// Whether the previous sticky variable map was replaced.
    pub replaced: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
/// Event payloads emitted by the SDK.
pub enum EventDetails {
    /// Details for a datafile update.
    DatafileSet(DatafileSetDetails),
    /// Details for a context update.
    ContextSet(ContextSetDetails),
    /// Details for a sticky evaluation update.
    StickyFeaturesSet(StickyFeaturesSetDetails),
    /// Details for a sticky global variable update.
    StickyVariablesSet(StickyVariablesSetDetails),
    /// Details for an error diagnostic.
    Error {
        /// The diagnostic that caused the event.
        diagnostic: Diagnostic,
    },
}

/// A thread safe callback that receives instance events.
pub type EventHandler = std::sync::Arc<dyn Fn(&EventDetails) + Send + Sync>;
