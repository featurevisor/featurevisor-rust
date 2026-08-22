use crate::diagnostics::{Diagnostic, LogLevel};
use crate::helpers::{attribute_to_bucket_string, context_value};
use crate::murmurhash::murmur_hash_v3;
use crate::types::{BucketBy, Context};

pub const MAX_BUCKETED_NUMBER: u32 = 100_000;

pub(crate) fn get_bucketed_number(bucket_key: &str) -> u32 {
    let hash = murmur_hash_v3(bucket_key, 1);
    ((hash as f64 / 4_294_967_296.0) * MAX_BUCKETED_NUMBER as f64).floor() as u32
}

pub(crate) fn get_bucket_key(
    feature_key: &str,
    bucket_by: &BucketBy,
    context: &Context,
    report: &dyn Fn(Diagnostic),
) -> Result<String, String> {
    let (kind, attributes): (&str, Vec<&str>) = match bucket_by {
        BucketBy::Plain(value) => ("plain", vec![value]),
        BucketBy::And(values) => ("and", values.iter().map(String::as_str).collect()),
        BucketBy::Or { or } => ("or", or.iter().map(String::as_str).collect()),
        BucketBy::Invalid(_) => {
            report(Diagnostic::new(LogLevel::Error, "invalid_bucket_by", "Invalid bucketBy"));
            return Err("invalid bucketBy".to_string());
        }
    };

    let mut parts = Vec::new();
    for attribute in attributes {
        if let Some(value) = context_value(context, attribute) {
            if kind != "or" || parts.is_empty() {
                parts.push(attribute_to_bucket_string(value));
            }
        }
    }
    parts.push(feature_key.to_string());

    if parts.is_empty() {
        report(Diagnostic::new(LogLevel::Error, "invalid_bucket_by", "Invalid bucketBy"));
        return Err("invalid bucketBy".to_string());
    }

    Ok(parts.join("."))
}
