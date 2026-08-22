use regex::Regex;
use std::cmp::Ordering;
use std::sync::OnceLock;

static SEMVER_REGEX: OnceLock<Regex> = OnceLock::new();

fn semver_regex() -> &'static Regex {
    SEMVER_REGEX.get_or_init(|| {
        Regex::new(r"(?i)^[v\^~<>=]*?(\d+)(?:\.([x*]|\d+)(?:\.([x*]|\d+)(?:\.([x*]|\d+))?(?:-([\da-z\-]+(?:\.[\da-z\-]+)*))?(?:\+[\da-z\-]+(?:\.[\da-z\-]+)*)?)?)?$")
            .expect("valid version regex")
    })
}

fn parts(version: &str) -> Result<Vec<String>, String> {
    let captures = semver_regex()
        .captures(version)
        .ok_or_else(|| format!("Invalid argument not valid semver ('{version}' received)"))?;
    Ok((1..=5)
        .map(|index| {
            captures
                .get(index)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default()
        })
        .collect())
}

fn compare_segment(left: &str, right: &str) -> Ordering {
    let left = if left.is_empty() { "0" } else { left };
    let right = if right.is_empty() { "0" } else { right };
    if left == "*"
        || left.eq_ignore_ascii_case("x")
        || right == "*"
        || right.eq_ignore_ascii_case("x")
    {
        return Ordering::Equal;
    }
    match (left.parse::<i64>(), right.parse::<i64>()) {
        (Ok(a), Ok(b)) => a.cmp(&b),
        (Ok(a), Err(_)) => a.to_string().cmp(&right.to_string()),
        (Err(_), Ok(b)) => left.to_string().cmp(&b.to_string()),
        (Err(_), Err(_)) => left.cmp(right),
    }
}

pub(crate) fn compare_versions(left: &str, right: &str) -> Result<Ordering, String> {
    let mut a = parts(left)?;
    let mut b = parts(right)?;
    let pre_a = a.pop().unwrap_or_default();
    let pre_b = b.pop().unwrap_or_default();
    for index in 0..3 {
        let ordering = compare_segment(
            a.get(index).map(String::as_str).unwrap_or("0"),
            b.get(index).map(String::as_str).unwrap_or("0"),
        );
        if ordering != Ordering::Equal {
            return Ok(ordering);
        }
    }
    if !pre_a.is_empty() && !pre_b.is_empty() {
        let left_parts = pre_a.split('.').collect::<Vec<_>>();
        let right_parts = pre_b.split('.').collect::<Vec<_>>();
        for index in 0..left_parts.len().max(right_parts.len()) {
            let left_part = left_parts.get(index).copied().unwrap_or("0");
            let right_part = right_parts.get(index).copied().unwrap_or("0");
            let ordering = compare_segment(left_part, right_part);
            if ordering != Ordering::Equal {
                return Ok(ordering);
            }
        }
        return Ok(Ordering::Equal);
    }
    Ok(match (pre_a.is_empty(), pre_b.is_empty()) {
        (true, true) => Ordering::Equal,
        (false, true) => Ordering::Less,
        (true, false) => Ordering::Greater,
        (false, false) => Ordering::Equal,
    })
}
