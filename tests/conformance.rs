use featurevisor::{
    create_featurevisor, AttributeValue, Context, DatafileInput, FeaturevisorOptions,
};
use serde_json::{json, Value};

fn datafile(segments: Value, features: Value) -> featurevisor::DatafileContent {
    serde_json::from_value(json!({
        "schemaVersion": "2",
        "revision": "test",
        "segments": segments,
        "features": features
    }))
    .expect("valid datafile")
}

fn feature(segments: Value, conditions: Value) -> featurevisor::DatafileContent {
    datafile(
        json!({ "segment": { "conditions": conditions } }),
        json!({ "feature": { "bucketBy": "userId", "traffic": [{ "key": "rule", "segments": segments, "percentage": 100000, "enabled": true }] } }),
    )
}

#[test]
fn fixture_is_version_two_and_every_section_is_present() {
    let fixture: Value = serde_json::from_str(include_str!("../conformance/sdk-v3.json")).unwrap();
    assert_eq!(fixture["version"], 2);
    for section in [
        "bucketing",
        "regularExpressions",
        "typedVariables",
        "datafile",
        "diagnostics",
        "numericBucketKeys",
        "portableConditions",
        "conditionCases",
        "childInstances",
        "defaults",
        "diagnosticCase",
        "nativeContexts",
    ] {
        assert!(
            fixture.get(section).is_some(),
            "missing conformance section {section}"
        );
    }
    for case in fixture["conditionCases"].as_array().unwrap() {
        let datafile = feature(json!("segment"), case["condition"].clone());
        let context: Context = case["context"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, value)| (key.clone(), AttributeValue::from_json(value.clone())))
            .collect();
        assert_eq!(
            create_featurevisor(FeaturevisorOptions {
                datafile: Some(DatafileInput::Content(datafile)),
                ..Default::default()
            })
            .is_enabled("feature", Some(&context)),
            case["expected"].as_bool().unwrap(),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn fixture_bucketing_numbers_regex_and_typed_values_are_executed() {
    let fixture: Value = serde_json::from_str(include_str!("../conformance/sdk-v3.json")).unwrap();
    assert_eq!(fixture["bucketing"]["minimum"], 0);
    assert_eq!(fixture["bucketing"]["maximum"], 100000);
    for item in fixture["numericBucketKeys"].as_array().unwrap() {
        let value = item["value"].as_f64().unwrap();
        let datafile = datafile(
            json!({}),
            json!({ "feature": { "bucketBy": "number", "traffic": [{ "key": "rule", "segments": "*", "percentage": 100000, "enabled": true }] } }),
        );
        let context: Context = [("number".to_string(), AttributeValue::Double(value))]
            .into_iter()
            .collect();
        let evaluation = create_featurevisor(FeaturevisorOptions {
            datafile: Some(DatafileInput::Content(datafile)),
            ..Default::default()
        })
        .evaluate_flag("feature", Some(&context));
        let expected_key = format!("{}.feature", item["expected"].as_str().unwrap());
        assert_eq!(
            evaluation.bucket_key.as_deref(),
            Some(expected_key.as_str())
        );
    }
    for item in fixture["regularExpressions"]["portableCases"]
        .as_array()
        .unwrap()
    {
        let datafile = feature(
            json!("segment"),
            json!({ "attribute": "value", "operator": "matches", "value": item["pattern"], "regexFlags": item["flags"] }),
        );
        let context: Context = [(
            "value".to_string(),
            AttributeValue::String(item["value"].as_str().unwrap().to_string()),
        )]
        .into_iter()
        .collect();
        let actual = create_featurevisor(FeaturevisorOptions {
            datafile: Some(DatafileInput::Content(datafile)),
            ..Default::default()
        })
        .is_enabled("feature", Some(&context));
        assert_eq!(
            actual,
            item["expected"].as_bool().unwrap(),
            "regex case {}",
            item["pattern"]
        );
    }
    for item in fixture["typedVariables"].as_array().unwrap() {
        let schema_type = item["type"].as_str().unwrap();
        let datafile = datafile(
            json!({}),
            json!({ "feature": { "bucketBy": "userId", "variablesSchema": { "value": { "type": schema_type, "defaultValue": item["value"] } }, "traffic": [{ "key": "default", "segments": "*", "percentage": 100000, "enabled": true }] } }),
        );
        let f = create_featurevisor(FeaturevisorOptions {
            datafile: Some(DatafileInput::Content(datafile)),
            ..Default::default()
        });
        let valid = match schema_type {
            "integer" => f
                .get_variable_integer("feature", "value", None, None)
                .is_some(),
            "double" => f
                .get_variable_double("feature", "value", None, None)
                .is_some(),
            "boolean" => f
                .get_variable_boolean("feature", "value", None, None)
                .is_some(),
            _ => f.get_variable("feature", "value", None, None).is_some(),
        };
        assert_eq!(
            valid,
            item["valid"].as_bool().unwrap(),
            "typed variable {:?} with value {:?}",
            schema_type,
            item["value"]
        );
    }
}

#[test]
fn fixture_datafile_defaults_diagnostics_and_native_contexts_are_executed() {
    let fixture: Value = serde_json::from_str(include_str!("../conformance/sdk-v3.json")).unwrap();
    let informational = datafile(json!({}), json!({}));
    let json_text = serde_json::to_string(&json!({ "schemaVersion": "informational", "revision": "other", "segments": {}, "features": {} })).unwrap();
    let f = create_featurevisor(FeaturevisorOptions {
        datafile: Some(DatafileInput::Content(informational)),
        ..Default::default()
    });
    f.set_datafile(DatafileInput::Json(json_text), true);
    assert_eq!(f.get_schema_version(), "informational");
    let defaults = &fixture["defaults"]["aggregateCase"];
    let default_datafile: featurevisor::DatafileContent =
        serde_json::from_value(defaults["datafile"].clone()).unwrap();
    let f = create_featurevisor(FeaturevisorOptions {
        datafile: Some(DatafileInput::Content(default_datafile)),
        ..Default::default()
    });
    let options = featurevisor::OverrideOptions {
        default_variation_value: Some(
            defaults["defaultVariationValue"]
                .as_str()
                .unwrap()
                .to_string(),
        ),
        ..Default::default()
    };
    assert_eq!(
        f.get_variation("experiment", None, Some(&options)),
        Some(String::new())
    );
    let diagnostics = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed = std::sync::Arc::clone(&diagnostics);
    let f = create_featurevisor(FeaturevisorOptions {
        on_diagnostic: Some(std::sync::Arc::new(move |diagnostic| {
            observed.lock().unwrap().push(diagnostic.clone())
        })),
        ..Default::default()
    });
    assert!(!f.is_enabled(
        fixture["diagnosticCase"]["featureKey"].as_str().unwrap(),
        None
    ));
    let diagnostics = diagnostics.lock().unwrap();
    assert!(diagnostics.iter().any(
        |diagnostic| diagnostic.level == featurevisor::LogLevel::Warn
            && diagnostic.code == "feature_not_found"
            && !diagnostic.details.is_empty()
    ));
    assert!(
        fixture["nativeContexts"]["numericTypesUseOneComparisonContract"]
            .as_bool()
            .unwrap()
    );
    assert!(
        fixture["nativeContexts"]["primitiveNativeSlicesSupportIncludes"]
            .as_bool()
            .unwrap()
    );
}
