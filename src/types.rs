#![allow(missing_docs)]

use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value as JsonValue;
use std::collections::HashMap;

/// Evaluation attributes keyed by name.
pub type Context = HashMap<String, AttributeValue>;
/// Sticky evaluation results keyed by feature key.
pub type StickyFeatures = HashMap<String, EvaluatedFeature>;
/// Sticky global variable values keyed by variable key.
pub type StickyVariables = HashMap<String, VariableValue>;
/// Global variable evaluation results keyed by variable key.
pub type EvaluatedVariables = HashMap<String, VariableValue>;
/// Evaluation results keyed by feature key.
pub type EvaluatedFeatures = HashMap<String, EvaluatedFeature>;
/// A variation value.
pub type VariationValue = String;
/// A feature key.
pub type FeatureKey = String;
/// A segment key.
pub type SegmentKey = String;
/// A rule key.
pub type RuleKey = String;

fn deserialize_condition_value<'de, D>(deserializer: D) -> Result<Option<JsonValue>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Some(JsonValue::deserialize(deserializer)?))
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
/// An operator used to compare a context attribute.
pub enum Operator {
    Equals,
    NotEquals,
    Before,
    After,
    In,
    NotIn,
    Contains,
    NotContains,
    StartsWith,
    EndsWith,
    SemverEquals,
    SemverNotEquals,
    SemverGreaterThan,
    SemverGreaterThanOrEquals,
    SemverLessThan,
    SemverLessThanOrEquals,
    Matches,
    NotMatches,
    GreaterThan,
    GreaterThanOrEquals,
    LessThan,
    LessThanOrEquals,
    Exists,
    NotExists,
    Includes,
    NotIncludes,
    Unknown(String),
}

impl Serialize for Operator {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let value = match self {
            Self::Equals => "equals",
            Self::NotEquals => "notEquals",
            Self::Before => "before",
            Self::After => "after",
            Self::In => "in",
            Self::NotIn => "notIn",
            Self::Contains => "contains",
            Self::NotContains => "notContains",
            Self::StartsWith => "startsWith",
            Self::EndsWith => "endsWith",
            Self::SemverEquals => "semverEquals",
            Self::SemverNotEquals => "semverNotEquals",
            Self::SemverGreaterThan => "semverGreaterThan",
            Self::SemverGreaterThanOrEquals => "semverGreaterThanOrEquals",
            Self::SemverLessThan => "semverLessThan",
            Self::SemverLessThanOrEquals => "semverLessThanOrEquals",
            Self::Matches => "matches",
            Self::NotMatches => "notMatches",
            Self::GreaterThan => "greaterThan",
            Self::GreaterThanOrEquals => "greaterThanOrEquals",
            Self::LessThan => "lessThan",
            Self::LessThanOrEquals => "lessThanOrEquals",
            Self::Exists => "exists",
            Self::NotExists => "notExists",
            Self::Includes => "includes",
            Self::NotIncludes => "notIncludes",
            Self::Unknown(value) => value,
        };
        serializer.serialize_str(value)
    }
}

impl<'de> Deserialize<'de> for Operator {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(match value.as_str() {
            "equals" => Self::Equals,
            "notEquals" => Self::NotEquals,
            "before" => Self::Before,
            "after" => Self::After,
            "in" => Self::In,
            "notIn" => Self::NotIn,
            "contains" => Self::Contains,
            "notContains" => Self::NotContains,
            "startsWith" => Self::StartsWith,
            "endsWith" => Self::EndsWith,
            "semverEquals" => Self::SemverEquals,
            "semverNotEquals" => Self::SemverNotEquals,
            "semverGreaterThan" => Self::SemverGreaterThan,
            "semverGreaterThanOrEquals" => Self::SemverGreaterThanOrEquals,
            "semverLessThan" => Self::SemverLessThan,
            "semverLessThanOrEquals" => Self::SemverLessThanOrEquals,
            "matches" => Self::Matches,
            "notMatches" => Self::NotMatches,
            "greaterThan" => Self::GreaterThan,
            "greaterThanOrEquals" => Self::GreaterThanOrEquals,
            "lessThan" => Self::LessThan,
            "lessThanOrEquals" => Self::LessThanOrEquals,
            "exists" => Self::Exists,
            "notExists" => Self::NotExists,
            "includes" => Self::Includes,
            "notIncludes" => Self::NotIncludes,
            _ => Self::Unknown(value),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// A plain attribute condition.
pub struct PlainCondition {
    pub attribute: String,
    pub operator: Operator,
    #[serde(
        default,
        deserialize_with = "deserialize_condition_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub value: Option<JsonValue>,
    #[serde(rename = "regexFlags", skip_serializing_if = "Option::is_none")]
    pub regex_flags: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// An implicit AND group of conditions.
pub struct AndCondition {
    pub and: Vec<Condition>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// An OR group of conditions.
pub struct OrCondition {
    pub or: Vec<Condition>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// A negated implicit AND group of conditions.
pub struct NotCondition {
    pub not: Vec<Condition>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
/// A condition expression accepted by Featurevisor datafiles.
pub enum Condition {
    Plain(PlainCondition),
    And(AndCondition),
    Or(OrCondition),
    Not(NotCondition),
    List(Vec<Condition>),
    String(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// An implicit AND group of segment references.
pub struct AndGroupSegment {
    pub and: Vec<GroupSegment>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// An OR group of segment references.
pub struct OrGroupSegment {
    pub or: Vec<GroupSegment>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// A negated implicit AND group of segment references.
pub struct NotGroupSegment {
    pub not: Vec<GroupSegment>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
/// A segment expression used by a rule, force, or override.
pub enum GroupSegment {
    Plain(String),
    And(AndGroupSegment),
    Or(OrGroupSegment),
    Not(NotGroupSegment),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
/// A JSON compatible Featurevisor variable value.
pub enum VariableValue {
    String(String),
    Integer(i64),
    Double(f64),
    Boolean(bool),
    Array(Vec<VariableValue>),
    Object(HashMap<String, VariableValue>),
    Null,
}

impl VariableValue {
    /// Converts the value to JSON.
    pub fn to_json(&self) -> JsonValue {
        match self {
            Self::String(value) => JsonValue::String(value.clone()),
            Self::Integer(value) => JsonValue::Number((*value).into()),
            Self::Double(value) => serde_json::Number::from_f64(*value)
                .map(JsonValue::Number)
                .unwrap_or(JsonValue::Null),
            Self::Boolean(value) => JsonValue::Bool(*value),
            Self::Array(values) => JsonValue::Array(values.iter().map(Self::to_json).collect()),
            Self::Object(values) => JsonValue::Object(
                values
                    .iter()
                    .map(|(key, value)| (key.clone(), value.to_json()))
                    .collect(),
            ),
            Self::Null => JsonValue::Null,
        }
    }

    /// Converts JSON to a variable value, using null for unsupported input.
    pub fn from_json(value: JsonValue) -> Self {
        serde_json::from_value(value).unwrap_or(Self::Null)
    }
}

#[derive(Clone, Debug, PartialEq)]
/// A context attribute value, including dates and nested values.
pub enum AttributeValue {
    String(String),
    Integer(i64),
    Double(f64),
    Boolean(bool),
    Date(DateTime<FixedOffset>),
    Array(Vec<AttributeValue>),
    Object(HashMap<String, AttributeValue>),
    Null,
}

impl AttributeValue {
    /// Converts the attribute to JSON.
    pub fn to_json(&self) -> JsonValue {
        match self {
            Self::String(value) => JsonValue::String(value.clone()),
            Self::Integer(value) => JsonValue::Number((*value).into()),
            Self::Double(value) => serde_json::Number::from_f64(*value)
                .map(JsonValue::Number)
                .unwrap_or(JsonValue::Null),
            Self::Boolean(value) => JsonValue::Bool(*value),
            Self::Date(value) => JsonValue::String(value.to_rfc3339()),
            Self::Array(values) => JsonValue::Array(values.iter().map(Self::to_json).collect()),
            Self::Object(values) => JsonValue::Object(
                values
                    .iter()
                    .map(|(key, value)| (key.clone(), value.to_json()))
                    .collect(),
            ),
            Self::Null => JsonValue::Null,
        }
    }

    /// Converts JSON to an attribute value.
    pub fn from_json(value: JsonValue) -> Self {
        match value {
            JsonValue::Null => Self::Null,
            JsonValue::Bool(value) => Self::Boolean(value),
            JsonValue::Number(value) => value
                .as_i64()
                .map(Self::Integer)
                .or_else(|| value.as_f64().map(Self::Double))
                .unwrap_or(Self::Null),
            JsonValue::String(value) => Self::String(value),
            JsonValue::Array(values) => {
                Self::Array(values.into_iter().map(Self::from_json).collect())
            }
            JsonValue::Object(values) => Self::Object(
                values
                    .into_iter()
                    .map(|(key, value)| (key, Self::from_json(value)))
                    .collect(),
            ),
        }
    }
}

impl Serialize for AttributeValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.to_json().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for AttributeValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self::from_json(JsonValue::deserialize(deserializer)?))
    }
}

impl From<String> for AttributeValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for AttributeValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

macro_rules! impl_attribute_integer {
    ($($type:ty),* $(,)?) => { $(
        impl From<$type> for AttributeValue {
            fn from(value: $type) -> Self { Self::Integer(value as i64) }
        }
    )* };
}

impl_attribute_integer!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

impl From<f32> for AttributeValue {
    fn from(value: f32) -> Self {
        Self::Double(value as f64)
    }
}

impl From<f64> for AttributeValue {
    fn from(value: f64) -> Self {
        Self::Double(value)
    }
}

impl From<bool> for AttributeValue {
    fn from(value: bool) -> Self {
        Self::Boolean(value)
    }
}

impl From<DateTime<FixedOffset>> for AttributeValue {
    fn from(value: DateTime<FixedOffset>) -> Self {
        Self::Date(value)
    }
}

impl From<HashMap<String, AttributeValue>> for AttributeValue {
    fn from(value: HashMap<String, AttributeValue>) -> Self {
        Self::Object(value)
    }
}

impl<T: Into<AttributeValue>> From<Vec<T>> for AttributeValue {
    fn from(value: Vec<T>) -> Self {
        Self::Array(value.into_iter().map(Into::into).collect())
    }
}

impl<T: Into<AttributeValue>> From<Option<T>> for AttributeValue {
    fn from(value: Option<T>) -> Self {
        value.map(Into::into).unwrap_or(Self::Null)
    }
}

impl From<String> for VariableValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for VariableValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<bool> for VariableValue {
    fn from(value: bool) -> Self {
        Self::Boolean(value)
    }
}

impl From<i64> for VariableValue {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}

impl From<f64> for VariableValue {
    fn from(value: f64) -> Self {
        Self::Double(value)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// The v2 datafile consumed by the SDK.
pub struct DatafileContent {
    #[serde(rename = "schemaVersion")]
    pub schema_version: String,
    pub revision: String,
    #[serde(
        rename = "featurevisorVersion",
        skip_serializing_if = "Option::is_none"
    )]
    pub featurevisor_version: Option<String>,
    pub segments: HashMap<String, Segment>,
    pub features: HashMap<String, Feature>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub variables: HashMap<String, GlobalVariable>,
}

impl Default for DatafileContent {
    fn default() -> Self {
        Self {
            schema_version: "2".to_string(),
            revision: "unknown".to_string(),
            featurevisor_version: None,
            segments: HashMap::new(),
            features: HashMap::new(),
            variables: HashMap::new(),
        }
    }
}

#[derive(Clone, Debug)]
/// A datafile supplied as decoded content or a JSON string.
pub enum DatafileInput {
    Content(DatafileContent),
    Json(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
/// A feature bucketing key definition.
pub enum BucketBy {
    Plain(String),
    And(Vec<String>),
    Or { or: Vec<String> },
    Invalid(JsonValue),
}

impl Default for BucketBy {
    fn default() -> Self {
        Self::Plain("userId".to_string())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
/// A feature dependency requirement.
pub enum Required {
    Feature(String),
    Details {
        feature: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        enabled: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        variation: Option<String>,
    },
    LegacyVariation {
        key: String,
        variation: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// A resolved variable schema from a feature definition.
pub struct ResolvedVariableSchema {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(rename = "type")]
    pub variable_type: String,
    #[serde(rename = "defaultValue")]
    pub default_value: VariableValue,
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "useDefaultWhenDisabled"
    )]
    pub use_default_when_disabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "disabledValue")]
    pub disabled_value: Option<VariableValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl Default for ResolvedVariableSchema {
    fn default() -> Self {
        Self {
            deprecated: None,
            key: None,
            variable_type: "string".to_string(),
            default_value: VariableValue::Null,
            use_default_when_disabled: None,
            disabled_value: None,
            description: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// A conditional variable override.
pub struct VariableOverride {
    pub value: VariableValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "keyPath")]
    pub key_path: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conditions: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segments: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "requiredFeatures")]
    pub required_features: Option<Vec<Required>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// A global variable definition from a datafile.
pub struct GlobalVariable {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    #[serde(rename = "type")]
    pub variable_type: String,
    #[serde(rename = "defaultValue", skip_serializing_if = "Option::is_none")]
    pub default_value: Option<VariableValue>,
    #[serde(rename = "disabledValue", skip_serializing_if = "Option::is_none")]
    pub disabled_value: Option<VariableValue>,
    #[serde(
        rename = "useDefaultWhenDisabled",
        skip_serializing_if = "Option::is_none"
    )]
    pub use_default_when_disabled: Option<bool>,
    #[serde(rename = "requiredFeatures", skip_serializing_if = "Option::is_none")]
    pub required_features: Option<Vec<Required>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<VariableOverride>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// A feature variation and its optional variable values.
pub struct Variation {
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variables: Option<HashMap<String, VariableValue>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "variableOverrides")]
    pub variable_overrides: Option<HashMap<String, Vec<VariableOverride>>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// A variation allocation range.
pub struct Allocation {
    pub variation: String,
    pub range: [f64; 2],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// A feature traffic rule.
pub struct Traffic {
    pub key: String,
    pub segments: JsonValue,
    pub percentage: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variables: Option<HashMap<String, VariableValue>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "variationWeights")]
    pub variation_weights: Option<HashMap<String, f64>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "variableOverrides")]
    pub variable_overrides: Option<HashMap<String, Vec<VariableOverride>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allocation: Option<Vec<Allocation>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// A force rule that overrides normal traffic evaluation.
pub struct Force {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conditions: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segments: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variables: Option<HashMap<String, VariableValue>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
/// A feature definition from a datafile.
pub struct Feature {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<Required>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "requiredFeatures")]
    pub required_features: Option<Vec<Required>>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "variablesSchema")]
    pub variables_schema: Option<HashMap<String, ResolvedVariableSchema>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "disabledVariationValue"
    )]
    pub disabled_variation_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variations: Option<Vec<Variation>>,
    #[serde(rename = "bucketBy")]
    pub bucket_by: BucketBy,
    pub traffic: Vec<Traffic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force: Option<Vec<Force>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ranges: Option<Vec<[f64; 2]>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// A segment definition from a datafile.
pub struct Segment {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub conditions: JsonValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// The compact result stored for sticky evaluations.
pub struct EvaluatedFeature {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variables: Option<HashMap<String, VariableValue>>,
}
