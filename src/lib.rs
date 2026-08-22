mod bucketer;
mod child;
mod compare_versions;
mod conditions;
mod datafile;
mod diagnostics;
mod emitter;
mod evaluate;
mod events;
mod helpers;
mod instance;
mod modules;
mod murmurhash;
mod types;

#[cfg(feature = "cli")]
pub mod cli;

pub use bucketer::MAX_BUCKETED_NUMBER;
pub use child::FeaturevisorChild;
pub use chrono;
pub use diagnostics::{Diagnostic, DiagnosticHandler, LogLevel};
pub use evaluate::{EvaluateOptions, Evaluation, EvaluationReason, EvaluationType};
pub use events::{
    ContextSetDetails, DatafileSetDetails, EventDetails, EventHandler, EventName, StickySetDetails,
};
pub use instance::{
    create_featurevisor, Featurevisor, FeaturevisorOptions, OverrideOptions, SpawnOptions,
};
pub use modules::{
    ConfigureBucketKeyOptions, ConfigureBucketValueOptions, FeaturevisorModule, ModuleApi,
};
pub use types::{
    Allocation, AndCondition, AndGroupSegment, AttributeValue, BucketBy, Condition, Context,
    DatafileContent, DatafileInput, EvaluatedFeature, EvaluatedFeatures, Feature, FeatureKey,
    Force, GroupSegment, NotCondition, NotGroupSegment, Operator, OrCondition, OrGroupSegment,
    PlainCondition, Required, ResolvedVariableSchema, RuleKey, Segment, SegmentKey, StickyFeatures,
    Traffic, VariableOverride, VariableValue, Variation, VariationValue,
};

pub type Unsubscribe = Box<dyn FnOnce() + Send + Sync>;

#[macro_export]
macro_rules! context {
    ($($key:expr => $value:expr),* $(,)?) => {{
        let mut context = ::std::collections::HashMap::new();
        $(context.insert($key.to_string(), $crate::AttributeValue::from($value));)*
        context
    }};
}
