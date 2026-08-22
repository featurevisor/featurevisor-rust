use featurevisor::{
    create_featurevisor, AttributeValue, Context, DatafileInput, FeaturevisorOptions,
    OverrideOptions,
};
use serde_json::json;

fn sdk() -> featurevisor::Featurevisor {
    let datafile = serde_json::from_value(json!({
        "schemaVersion": "2",
        "revision": "one",
        "segments": {},
        "features": {
            "flag": { "bucketBy": "userId", "traffic": [{ "key": "rule", "segments": "*", "percentage": 100000, "enabled": true }] },
            "experiment": { "bucketBy": "userId", "variations": [{ "value": "control" }, { "value": "treatment" }], "traffic": [{ "key": "rule", "segments": "*", "percentage": 100000, "allocation": [{ "variation": "control", "range": [0, 50000] }, { "variation": "treatment", "range": [50001, 100000] }] }] },
            "config": { "bucketBy": "userId", "variablesSchema": { "enabled": { "type": "boolean", "defaultValue": false }, "count": { "type": "integer", "defaultValue": 0 }, "json": { "type": "json", "defaultValue": { "a": 1 } } }, "traffic": [{ "key": "rule", "segments": "*", "percentage": 100000, "variables": { "enabled": true, "count": 3 } }] }
        }
    })).unwrap();
    create_featurevisor(FeaturevisorOptions {
        datafile: Some(DatafileInput::Content(datafile)),
        ..Default::default()
    })
}

#[test]
fn evaluates_flag_variation_and_typed_variables() {
    let f = sdk();
    let context: Context = [("userId".to_string(), AttributeValue::from("one"))]
        .into_iter()
        .collect();
    assert!(f.is_enabled("flag", Some(&context)));
    assert!(matches!(
        f.get_variation("experiment", Some(&context), None)
            .as_deref(),
        Some("control") | Some("treatment")
    ));
    assert_eq!(
        f.get_variable_boolean("config", "enabled", Some(&context), None),
        Some(true)
    );
    assert_eq!(
        f.get_variable_integer("config", "count", Some(&context), None),
        Some(3)
    );
    assert_eq!(
        f.get_variable_string("config", "count", Some(&context), None),
        None
    );
}

#[test]
fn defaults_are_presence_based_and_datafiles_merge_or_replace() {
    let f = sdk();
    let value = f.get_variation(
        "missing",
        None,
        Some(&OverrideOptions {
            default_variation_value: Some(String::new()),
            ..Default::default()
        }),
    );
    assert_eq!(value, Some(String::new()));
    let partial = serde_json::to_string(&serde_json::json!({ "schemaVersion": "2", "revision": "two", "segments": {}, "features": { "new": { "bucketBy": "userId", "traffic": [] } } })).unwrap();
    f.set_datafile(DatafileInput::Json(partial), false);
    assert!(f.get_feature("flag").is_some());
    assert!(f.get_feature("new").is_some());
    let replacement = serde_json::to_string(&serde_json::json!({ "schemaVersion": "2", "revision": "three", "segments": {}, "features": {} })).unwrap();
    f.set_datafile(DatafileInput::Json(replacement), true);
    assert!(f.get_feature("flag").is_none());
    assert_eq!(f.get_revision(), "three");
}

#[test]
fn malformed_datafile_preserves_existing_state() {
    let diagnostics = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed = std::sync::Arc::clone(&diagnostics);
    let f = create_featurevisor(FeaturevisorOptions { on_diagnostic: Some(std::sync::Arc::new(move |diagnostic| observed.lock().unwrap().push((diagnostic.code.clone(), diagnostic.message.clone())))), datafile: Some(DatafileInput::Content(serde_json::from_value(json!({ "schemaVersion": "2", "revision": "original", "segments": {}, "features": {} })).unwrap())), ..Default::default() });
    f.set_datafile(DatafileInput::Json("{not-json".to_string()), false);
    assert_eq!(f.get_revision(), "original");
    assert!(
        diagnostics
            .lock()
            .unwrap()
            .iter()
            .any(|(code, message)| code == "invalid_datafile"
                && message == "Could not parse datafile")
    );
}
