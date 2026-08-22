use crate::compare_versions::compare_versions;
use crate::diagnostics::{Diagnostic, LogLevel};
use crate::helpers::context_value;
use crate::types::{AttributeValue, Condition, Context, Operator, Segment};
use chrono::{DateTime, FixedOffset};
use regex::Regex;
use serde_json::Value as JsonValue;
use std::cmp::Ordering;

pub(crate) type RegexGetter<'a> = dyn Fn(&str, &str) -> Result<Regex, String> + 'a;

fn parse_date(value: &str) -> Option<DateTime<FixedOffset>> {
    let explicit_timezone = regex::Regex::new(r"T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+\-]\d{2}:\d{2})$").ok()?;
    if !explicit_timezone.is_match(value) { return None; }
    DateTime::parse_from_rfc3339(value).ok()
}

fn primitive(value: &AttributeValue) -> Option<JsonValue> {
    match value {
        AttributeValue::String(value) => Some(JsonValue::String(value.clone())),
        AttributeValue::Integer(value) => Some(JsonValue::Number((*value).into())),
        AttributeValue::Double(value) => serde_json::Number::from_f64(*value).map(JsonValue::Number),
        AttributeValue::Boolean(value) => Some(JsonValue::Bool(*value)),
        AttributeValue::Null => Some(JsonValue::Null),
        _ => None,
    }
}

fn js_equals(left: &AttributeValue, right: &JsonValue) -> bool {
    match (left, right) {
        (AttributeValue::Integer(left), JsonValue::Number(right)) => right.as_i64() == Some(*left) || right.as_f64() == Some(*left as f64),
        (AttributeValue::Double(left), JsonValue::Number(right)) => right.as_f64() == Some(*left),
        _ => primitive(left).as_ref() == Some(right),
    }
}

fn json_string(value: &JsonValue) -> Option<&str> { value.as_str() }

fn condition_is_matched(
    condition: &crate::types::PlainCondition,
    context: &Context,
    get_regex: &RegexGetter<'_>,
) -> Result<bool, String> {
    let context_value = context_value(context, &condition.attribute);
    let value = &condition.value;
    let operator = &condition.operator;

    let result = match operator {
        Operator::Equals => context_value.map(|v| js_equals(v, value)).unwrap_or(false),
        Operator::NotEquals => context_value.map(|v| !js_equals(v, value)).unwrap_or(true),
        Operator::Before | Operator::After => {
            let left = context_value.and_then(|v| match v {
                AttributeValue::Date(date) => Some(date.clone()),
                AttributeValue::String(value) => parse_date(value),
                _ => None,
            });
            let right = json_string(value).and_then(parse_date);
            match (left, right) {
                (Some(left), Some(right)) => if matches!(*operator, Operator::Before) { left < right } else { left > right },
                _ => false,
            }
        }
        Operator::In | Operator::NotIn => {
            let Some(context_value) = context_value else { return Ok(false); };
            if !matches!(context_value, AttributeValue::String(_) | AttributeValue::Integer(_) | AttributeValue::Double(_) | AttributeValue::Null) { return Ok(false); }
            let matches = value.as_array().map(|values| values.iter().any(|item| js_equals(context_value, item))).unwrap_or(false);
            if matches!(*operator, Operator::In) { matches } else { !matches }
        }
        Operator::Contains | Operator::NotContains | Operator::StartsWith | Operator::EndsWith
        | Operator::Matches | Operator::NotMatches | Operator::SemverEquals
        | Operator::SemverNotEquals | Operator::SemverGreaterThan
        | Operator::SemverGreaterThanOrEquals | Operator::SemverLessThan
        | Operator::SemverLessThanOrEquals => {
            let (left, right) = match (context_value, value.as_str()) {
                (Some(AttributeValue::String(left)), Some(right)) => (left, right),
                _ => return Ok(false),
            };
            match operator {
                Operator::Contains => left.contains(right),
                Operator::NotContains => !left.contains(right),
                Operator::StartsWith => left.starts_with(right),
                Operator::EndsWith => left.ends_with(right),
                Operator::Matches | Operator::NotMatches => {
                    let regex = get_regex(right, condition.regex_flags.as_deref().unwrap_or(""))?;
                    let matches = regex.is_match(left);
                    if matches!(*operator, Operator::Matches) { matches } else { !matches }
                }
                Operator::SemverEquals | Operator::SemverNotEquals | Operator::SemverGreaterThan
                | Operator::SemverGreaterThanOrEquals | Operator::SemverLessThan
                | Operator::SemverLessThanOrEquals => {
                    let ordering = compare_versions(left, right)?;
                    match operator {
                        Operator::SemverEquals => ordering == Ordering::Equal,
                        Operator::SemverNotEquals => ordering != Ordering::Equal,
                        Operator::SemverGreaterThan => ordering == Ordering::Greater,
                        Operator::SemverGreaterThanOrEquals => ordering != Ordering::Less,
                        Operator::SemverLessThan => ordering == Ordering::Less,
                        Operator::SemverLessThanOrEquals => ordering != Ordering::Greater,
                        _ => false,
                    }
                }
                _ => false,
            }
        }
        Operator::GreaterThan | Operator::GreaterThanOrEquals | Operator::LessThan | Operator::LessThanOrEquals => {
            let (left, right) = match (context_value, value.as_f64()) {
                (Some(AttributeValue::Integer(left)), Some(right)) => (*left as f64, right),
                (Some(AttributeValue::Double(left)), Some(right)) => (*left, right),
                _ => return Ok(false),
            };
            match operator {
                Operator::GreaterThan => left > right,
                Operator::GreaterThanOrEquals => left >= right,
                Operator::LessThan => left < right,
                Operator::LessThanOrEquals => left <= right,
                _ => false,
            }
        }
        Operator::Exists => context_value.is_some(),
        Operator::NotExists => context_value.is_none(),
        Operator::Includes | Operator::NotIncludes => {
            let Some(AttributeValue::Array(values)) = context_value else { return Ok(false); };
            let matches = values.iter().any(|item| js_equals(item, value));
            if matches!(*operator, Operator::Includes) { matches } else { !matches }
        }
    };
    Ok(result)
}

pub(crate) fn parse_condition(value: &JsonValue) -> Result<Condition, String> {
    if let JsonValue::String(value) = value {
        if value == "*" { return Ok(Condition::String(value.clone())); }
        let decoded: JsonValue = serde_json::from_str(value).map_err(|e| e.to_string())?;
        return serde_json::from_value(decoded).map_err(|e| e.to_string());
    }
    serde_json::from_value(value.clone()).map_err(|e| e.to_string())
}

pub(crate) fn all_conditions_are_matched(
    value: &JsonValue,
    context: &Context,
    get_regex: &RegexGetter<'_>,
    report: &dyn Fn(Diagnostic),
) -> bool {
    let condition = match parse_condition(value) {
        Ok(value) => value,
        Err(error) => {
            let mut diagnostic = Diagnostic::new(LogLevel::Error, "conditions_parse_error", "Error parsing conditions");
            diagnostic.original_error = Some(error);
            report(diagnostic);
            return false;
        }
    };
    match condition {
        Condition::String(value) => value == "*",
        Condition::Plain(condition) => match condition_is_matched(&condition, context, get_regex) {
            Ok(value) => value,
            Err(error) => {
                let mut diagnostic = Diagnostic::new(LogLevel::Warn, "condition_match_error", error.clone());
                diagnostic.original_error = Some(error);
                report(diagnostic);
                false
            }
        },
        Condition::And(value) => value.and.iter().all(|item| all_conditions_are_matched(&serde_json::to_value(item).unwrap_or(JsonValue::Null), context, get_regex, report)),
        Condition::Or(value) => value.or.iter().any(|item| all_conditions_are_matched(&serde_json::to_value(item).unwrap_or(JsonValue::Null), context, get_regex, report)),
        Condition::Not(value) => !value.not.is_empty() && !value.not.iter().all(|item| all_conditions_are_matched(&serde_json::to_value(item).unwrap_or(JsonValue::Null), context, get_regex, report)),
        Condition::List(values) => values.iter().all(|item| all_conditions_are_matched(&serde_json::to_value(item).unwrap_or(JsonValue::Null), context, get_regex, report)),
    }
}

pub(crate) fn parse_segments(value: &JsonValue) -> Result<JsonValue, String> {
    if let JsonValue::String(value) = value {
        if value == "*" { return Ok(JsonValue::String(value.clone())); }
        if value.starts_with('{') || value.starts_with('[') {
            return serde_json::from_str(value).map_err(|e| e.to_string());
        }
    }
    Ok(value.clone())
}

pub(crate) fn all_segments_are_matched(
    value: &JsonValue,
    context: &Context,
    get_segment: &dyn Fn(&str) -> Option<Segment>,
    get_regex: &RegexGetter<'_>,
    report: &dyn Fn(Diagnostic),
) -> bool {
    let value = match parse_segments(value) {
        Ok(value) => value,
        Err(_) => return false,
    };
    if value == JsonValue::String("*") { return true; }
    if let Some(key) = value.as_str() {
        return get_segment(key).map(|segment| all_conditions_are_matched(&segment.conditions, context, get_regex, report)).unwrap_or(false);
    }
    if let Some(values) = value.as_array() {
        return values.iter().all(|item| all_segments_are_matched(item, context, get_segment, get_regex, report));
    }
    if let Some(object) = value.as_object() {
        if let Some(values) = object.get("and").and_then(JsonValue::as_array) {
            return values.iter().all(|item| all_segments_are_matched(item, context, get_segment, get_regex, report));
        }
        if let Some(values) = object.get("or").and_then(JsonValue::as_array) {
            return values.iter().any(|item| all_segments_are_matched(item, context, get_segment, get_regex, report));
        }
        if let Some(values) = object.get("not").and_then(JsonValue::as_array) {
            return !values.is_empty() && !values.iter().all(|item| all_segments_are_matched(item, context, get_segment, get_regex, report));
        }
    }
    false
}
