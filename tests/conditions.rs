use featurevisor::{
    create_featurevisor, AttributeValue, Context, DatafileInput, Diagnostic, FeaturevisorOptions,
};
use serde_json::json;
use std::sync::{Arc, Mutex};

fn condition_feature(
    segments: serde_json::Value,
    conditions: serde_json::Value,
) -> featurevisor::DatafileContent {
    serde_json::from_value(json!({
        "schemaVersion": "2",
        "revision": "conditions",
        "segments": { "segment": { "conditions": conditions } },
        "features": {
            "feature": {
                "bucketBy": "userId",
                "traffic": [{ "key": "rule", "segments": segments, "percentage": 100000, "enabled": true }]
            }
        }
    }))
    .unwrap()
}

fn context(values: &[(&str, AttributeValue)]) -> Context {
    values
        .iter()
        .map(|(key, value)| ((*key).to_string(), value.clone()))
        .collect()
}

#[test]
fn unknown_operator_is_false_without_a_parse_diagnostic() {
    let diagnostics = Arc::new(Mutex::new(Vec::<Diagnostic>::new()));
    let observed = Arc::clone(&diagnostics);
    let f = create_featurevisor(FeaturevisorOptions {
        datafile: Some(DatafileInput::Content(condition_feature(
            json!("segment"),
            json!({
                "attribute": "country",
                "operator": "futureOperator",
                "value": "nl"
            }),
        ))),
        on_diagnostic: Some(Arc::new(move |diagnostic| {
            observed.lock().unwrap().push(diagnostic.clone());
        })),
        ..Default::default()
    });
    let context = context(&[("country", "nl".into())]);

    assert!(!f.is_enabled("feature", Some(&context)));
    assert!(diagnostics
        .lock()
        .unwrap()
        .iter()
        .all(|diagnostic| diagnostic.code != "conditions_parse_error"));
}

#[test]
fn runtime_accepts_host_regex_syntax_and_reports_real_compile_errors() {
    for (pattern, value) in [
        ("(?:chrome|firefox)", "chrome"),
        ("[(?]", "?"),
        (r"a\++", "a++"),
    ] {
        let f = create_featurevisor(FeaturevisorOptions {
            datafile: Some(DatafileInput::Content(condition_feature(
                json!("segment"),
                json!({
                    "attribute": "browser",
                    "operator": "matches",
                    "value": pattern
                }),
            ))),
            ..Default::default()
        });
        let context = context(&[("browser", value.into())]);
        assert!(f.is_enabled("feature", Some(&context)), "pattern {pattern}");
    }

    let diagnostics = Arc::new(Mutex::new(Vec::<Diagnostic>::new()));
    let observed = Arc::clone(&diagnostics);
    let f = create_featurevisor(FeaturevisorOptions {
        datafile: Some(DatafileInput::Content(condition_feature(
            json!("segment"),
            json!({
                "attribute": "browser",
                "operator": "matches",
                "value": "(?=chrome)"
            }),
        ))),
        on_diagnostic: Some(Arc::new(move |diagnostic| {
            observed.lock().unwrap().push(diagnostic.clone());
        })),
        ..Default::default()
    });
    let context = context(&[("browser", "chrome".into())]);

    assert!(!f.is_enabled("feature", Some(&context)));
    assert!(diagnostics
        .lock()
        .unwrap()
        .iter()
        .any(|diagnostic| diagnostic.code == "condition_match_error"));
}

#[test]
fn not_uses_implicit_and_and_stringified_segments_work() {
    let datafile = serde_json::from_value(json!({
        "schemaVersion": "2", "revision": "conditions",
        "segments": {
            "notBoth": { "conditions": { "not": [{ "attribute": "country", "operator": "equals", "value": "nl" }, { "attribute": "device", "operator": "equals", "value": "mobile" }] } },
            "none": { "conditions": { "not": [{ "or": [{ "attribute": "country", "operator": "equals", "value": "nl" }, { "attribute": "country", "operator": "equals", "value": "de" }] }] } }
        },
        "features": {
            "feature": { "bucketBy": "userId", "traffic": [{ "key": "rule", "segments": { "and": ["notBoth", "none"] }, "percentage": 100000, "enabled": true }] }
        }
    })).unwrap();
    let f = create_featurevisor(FeaturevisorOptions {
        datafile: Some(DatafileInput::Content(datafile)),
        ..Default::default()
    });
    let both = context(&[("country", "nl".into()), ("device", "mobile".into())]);
    let other = context(&[("country", "fr".into()), ("device", "desktop".into())]);
    assert!(!f.is_enabled("feature", Some(&both)));
    assert!(f.is_enabled("feature", Some(&other)));
}

#[test]
fn nested_stringified_segments_match_mobile_eu_context() {
    let datafile = serde_json::from_value(json!({
        "schemaVersion": "2",
        "revision": "conditions",
        "segments": {
            "mobile": { "conditions": "{\"and\":[{\"attribute\":\"device\",\"operator\":\"equals\",\"value\":\"mobile\"},{\"attribute\":\"phone\",\"operator\":\"notExists\"}]}" },
            "eu": { "conditions": "[{\"attribute\":\"continent\",\"operator\":\"equals\",\"value\":\"europe\"},{\"attribute\":\"country\",\"operator\":\"notIn\",\"value\":[\"gb\"]}]" }
        },
        "features": {
            "feature": {
                "bucketBy": "userId",
                "traffic": [{ "key": "mobile_eu", "segments": "{\"and\":[\"mobile\",\"eu\"]}", "percentage": 100000, "enabled": true }]
            }
        }
    })).unwrap();
    let f = create_featurevisor(FeaturevisorOptions {
        datafile: Some(DatafileInput::Content(datafile)),
        ..Default::default()
    });
    let context = context(&[
        ("device", "mobile".into()),
        ("continent", "europe".into()),
        ("country", "it".into()),
    ]);
    assert!(f.is_enabled("feature", Some(&context)));
}

#[test]
fn equals_null_matches_a_null_context_attribute() {
    let datafile = serde_json::from_value(json!({
        "schemaVersion": "2",
        "revision": "conditions",
        "segments": {
            "unknown": {
                "conditions": [{ "attribute": "device", "operator": "equals", "value": null }]
            }
        },
        "features": {
            "feature": {
                "bucketBy": "userId",
                "traffic": [{ "key": "unknown", "segments": "unknown", "percentage": 100000, "enabled": true }]
            }
        }
    })).unwrap();
    let f = create_featurevisor(FeaturevisorOptions {
        datafile: Some(DatafileInput::Content(datafile)),
        ..Default::default()
    });
    let context = context(&[("device", featurevisor::AttributeValue::Null)]);
    assert!(f.is_enabled("feature", Some(&context)));
}

#[test]
fn semver_comparison_zero_pads_missing_segments_and_prerelease_parts() {
    for (context_version, condition_version) in [
        ("1", "1.0.0"),
        ("1.2", "1.2.0"),
        ("1.2.3-alpha.0", "1.2.3-alpha"),
    ] {
        let datafile = condition_feature(
            json!("segment"),
            json!({
                "attribute": "version",
                "operator": "semverEquals",
                "value": condition_version
            }),
        );
        let f = create_featurevisor(FeaturevisorOptions {
            datafile: Some(DatafileInput::Content(datafile)),
            ..Default::default()
        });
        let context = context(&[("version", context_version.into())]);
        assert!(
            f.is_enabled("feature", Some(&context)),
            "{context_version} should equal {condition_version}"
        );
    }
}
