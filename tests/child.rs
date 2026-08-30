use featurevisor::{
    create_featurevisor, AttributeValue, DatafileInput, EvaluatedFeature, FeaturevisorOptions,
    SpawnOptions, StickyFeatures, StickyVariables,
};
use serde_json::json;

#[test]
fn child_keeps_a_context_snapshot_and_inherits_new_parent_keys() {
    let datafile = serde_json::from_value(
        json!({ "schemaVersion": "2", "revision": "child", "segments": {}, "features": {} }),
    )
    .unwrap();
    let f = create_featurevisor(FeaturevisorOptions {
        context: Some(
            [(String::from("userId"), AttributeValue::from("one"))]
                .into_iter()
                .collect(),
        ),
        datafile: Some(DatafileInput::Content(datafile)),
        ..Default::default()
    });
    let child = f.spawn(Default::default(), Default::default());
    f.set_context(
        [
            (String::from("userId"), AttributeValue::from("two")),
            (String::from("country"), AttributeValue::from("nl")),
        ]
        .into_iter()
        .collect(),
        false,
    );
    assert_eq!(
        child.get_context(None).get("userId"),
        Some(&AttributeValue::from("one"))
    );
    assert_eq!(
        child.get_context(None).get("country"),
        Some(&AttributeValue::from("nl"))
    );
    child.close();
    child.close();
}

#[test]
fn child_global_variable_required_features_use_child_sticky_state() {
    let datafile = serde_json::from_value(json!({
        "schemaVersion": "2",
        "revision": "child-required-feature",
        "segments": {},
        "features": {
            "dependency": {
                "bucketBy": ["userId"],
                "traffic": [{ "key": "all", "segments": "*", "percentage": 100000 }]
            }
        },
        "variables": {
            "message": {
                "type": "string",
                "defaultValue": "available",
                "disabledValue": "unavailable",
                "requiredFeatures": ["dependency"]
            }
        }
    }))
    .unwrap();
    let f = create_featurevisor(FeaturevisorOptions {
        datafile: Some(DatafileInput::Content(datafile)),
        context: Some(
            [(String::from("userId"), AttributeValue::from("one"))]
                .into_iter()
                .collect(),
        ),
        sticky_features: Some(StickyFeatures::from([(
            "dependency".to_string(),
            EvaluatedFeature {
                enabled: false,
                variation: None,
                variables: None,
            },
        )])),
        ..Default::default()
    });
    let child = f.spawn(Default::default(), Default::default());

    assert_eq!(
        f.get_global_variable_string("message", None, None)
            .as_deref(),
        Some("unavailable")
    );
    assert_eq!(
        child
            .get_global_variable_string("message", None, None)
            .as_deref(),
        Some("available")
    );
}

#[test]
fn child_global_variable_sticky_state_is_isolated() {
    let datafile = serde_json::from_value(json!({
        "schemaVersion": "2", "revision": "child-variables", "segments": {}, "features": {},
        "variables": { "message": { "type": "string", "defaultValue": "default" } }
    }))
    .unwrap();
    let f = create_featurevisor(FeaturevisorOptions {
        datafile: Some(DatafileInput::Content(datafile)),
        sticky_variables: Some(StickyVariables::from([(
            "message".to_string(),
            "parent".into(),
        )])),
        ..Default::default()
    });
    let child = f.spawn(
        Default::default(),
        SpawnOptions {
            sticky_variables: Some(StickyVariables::from([(
                "message".to_string(),
                "child".into(),
            )])),
            ..Default::default()
        },
    );
    let plain_child = f.spawn(Default::default(), Default::default());
    assert_eq!(
        f.get_global_variable_string("message", None, None)
            .as_deref(),
        Some("parent")
    );
    assert_eq!(
        child
            .get_global_variable_string("message", None, None)
            .as_deref(),
        Some("child")
    );
    assert_eq!(
        plain_child
            .get_global_variable_string("message", None, None)
            .as_deref(),
        Some("default")
    );
}
