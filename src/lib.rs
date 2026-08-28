#![forbid(unsafe_code)]
#![warn(missing_docs)]
//! Feature flag evaluation for Featurevisor v3 datafiles.
//!
//! Create a [`Featurevisor`] instance with [`create_featurevisor`], provide a
//! v2 datafile, and evaluate flags, variations, and variables:
//!
//! ```
//! use featurevisor::{context, create_featurevisor, DatafileInput, FeaturevisorOptions};
//! use serde_json::json;
//!
//! let datafile = serde_json::from_value(json!({
//!     "schemaVersion": "2",
//!     "revision": "example",
//!     "segments": {},
//!     "features": {
//!         "welcome": {
//!             "bucketBy": "userId",
//!             "traffic": [{
//!                 "key": "everyone",
//!                 "segments": "*",
//!                 "percentage": 100000,
//!                 "enabled": true
//!             }]
//!         }
//!     }
//! })).expect("valid datafile");
//! let f = create_featurevisor(FeaturevisorOptions {
//!     datafile: Some(DatafileInput::Content(datafile)),
//!     ..Default::default()
//! });
//! assert!(f.is_enabled("welcome", Some(&context!("userId" => "123"))));
//! ```

mod bucketer;
mod child;
mod compare_versions;
mod conditions;
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
#[allow(missing_docs)]
pub mod cli;

pub use bucketer::MAX_BUCKETED_NUMBER;
pub use child::FeaturevisorChild;
pub use chrono;
pub use diagnostics::{Diagnostic, DiagnosticHandler, LogLevel};
pub use evaluate::{EvaluateOptions, Evaluation, EvaluationReason, EvaluationType};
pub use events::{
    ContextSetDetails, DatafileSetDetails, EventDetails, EventHandler, EventName,
    StickyFeaturesSetDetails, StickyVariablesSetDetails,
};
pub use instance::{
    create_featurevisor, Featurevisor, FeaturevisorOptions, OverrideOptions, SpawnOptions,
};
pub use modules::{
    ConfigureBucketKeyOptions, ConfigureBucketValueOptions, FeaturevisorModule, ModuleApi,
};
pub use types::{
    Allocation, AndCondition, AndGroupSegment, AttributeValue, BucketBy, Condition, Context,
    DatafileContent, DatafileInput, EvaluatedFeature, EvaluatedFeatures, EvaluatedVariables,
    Feature, FeatureKey, Force, GlobalVariable, GroupSegment, NotCondition, NotGroupSegment,
    Operator, OrCondition, OrGroupSegment, PlainCondition, Required, ResolvedVariableSchema,
    RuleKey, Segment, SegmentKey, StickyFeatures, StickyVariables, Traffic, VariableOverride,
    VariableValue, Variation, VariationValue,
};

/// A one shot cleanup callback returned by subscriptions and module registration.
pub type Unsubscribe = Box<dyn FnOnce() + Send + Sync>;

/// Builds a [`Context`] from key and value pairs.
#[macro_export]
macro_rules! context {
    ($($key:expr => $value:expr),* $(,)?) => {{
        let mut context = ::std::collections::HashMap::new();
        $(context.insert($key.to_string(), $crate::AttributeValue::from($value));)*
        context
    }};
}
