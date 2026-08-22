use crate::types::{AttributeValue, Context};
#[cfg(feature = "cli")]
use serde_json::Value as JsonValue;

pub fn format_js_number(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }

    let negative = value.is_sign_negative();
    let absolute = value.abs();
    let mut result = absolute.to_string();

    if let Some(index) = result.find('e').or_else(|| result.find('E')) {
        let mantissa = result[..index].to_string();
        let exponent: i32 = result[index + 1..].parse().unwrap_or(0);
        if (1e-6..1e21).contains(&absolute) {
            result = expand_scientific(&mantissa, exponent);
        } else {
            result = format_scientific(&mantissa, exponent);
        }
    } else if absolute < 1e-6 || absolute >= 1e21 {
        result = to_scientific(&result);
    }

    if negative {
        format!("-{result}")
    } else {
        result
    }
}

fn expand_scientific(mantissa: &str, exponent: i32) -> String {
    let mut digits = mantissa.replace('.', "");
    let decimal_position = mantissa.find('.').unwrap_or(mantissa.len()) as i32 + exponent;

    if decimal_position <= 0 {
        return format!("0.{}{}", "0".repeat((-decimal_position) as usize), digits);
    }

    if decimal_position as usize >= digits.len() {
        digits.push_str(&"0".repeat(decimal_position as usize - digits.len()));
        return digits;
    }

    let index = decimal_position as usize;
    format!("{}.{}", &digits[..index], &digits[index..])
}

fn format_scientific(mantissa: &str, exponent: i32) -> String {
    let exponent = if exponent >= 0 {
        format!("+{exponent}")
    } else {
        exponent.to_string()
    };
    format!("{mantissa}e{exponent}")
}

fn to_scientific(value: &str) -> String {
    let (integer, fraction) = value.split_once('.').unwrap_or((value, ""));
    let digits = format!("{integer}{fraction}");
    let leading_zeroes = digits
        .chars()
        .take_while(|character| *character == '0')
        .count();
    let significant = &digits[leading_zeroes..];
    if significant.is_empty() {
        return "0".to_string();
    }
    let significant = significant.trim_end_matches('0');
    let first = significant.chars().next().unwrap_or('0');
    let rest = significant.get(first.len_utf8()..).unwrap_or("");
    let exponent = integer.len() as i32 - leading_zeroes as i32 - 1;
    let exponent = if exponent >= 0 {
        format!("+{exponent}")
    } else {
        exponent.to_string()
    };
    if rest.is_empty() {
        format!("{first}e{exponent}")
    } else {
        format!("{first}.{rest}e{exponent}")
    }
}

pub fn attribute_to_bucket_string(value: &AttributeValue) -> String {
    match value {
        AttributeValue::String(value) => value.clone(),
        AttributeValue::Integer(value) => value.to_string(),
        AttributeValue::Double(value) => format_js_number(*value),
        AttributeValue::Boolean(value) => value.to_string(),
        AttributeValue::Date(value) => value.to_rfc3339(),
        AttributeValue::Array(values) => values
            .iter()
            .map(attribute_to_bucket_string)
            .collect::<Vec<_>>()
            .join(","),
        AttributeValue::Object(_) => "[object Object]".to_string(),
        AttributeValue::Null => String::new(),
    }
}

pub fn context_value<'a>(context: &'a Context, path: &str) -> Option<&'a AttributeValue> {
    let mut current = context.get(path.split('.').next().unwrap_or(path))?;
    for part in path.split('.').skip(1) {
        current = match current {
            AttributeValue::Object(values) => values.get(part)?,
            _ => return None,
        };
    }
    Some(current)
}

#[cfg(feature = "cli")]
pub fn json_to_context(value: &JsonValue) -> Option<Context> {
    match value {
        JsonValue::Object(values) => Some(
            values
                .iter()
                .map(|(key, value)| (key.clone(), AttributeValue::from_json(value.clone())))
                .collect(),
        ),
        _ => None,
    }
}
