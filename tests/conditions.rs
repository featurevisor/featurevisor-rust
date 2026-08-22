use featurevisor::{
    create_featurevisor, AttributeValue, Context, DatafileInput, FeaturevisorOptions,
};
use serde_json::json;

fn context(values: &[(&str, AttributeValue)]) -> Context {
    values
        .iter()
        .map(|(key, value)| ((*key).to_string(), value.clone()))
        .collect()
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
