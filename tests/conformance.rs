use featurevisor::{
    create_featurevisor, AttributeValue, ConfigureBucketValueOptions, Context, DatafileInput,
    Diagnostic, EventDetails, EventName, FeaturevisorModule, FeaturevisorOptions, LogLevel,
    ModuleApi,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

struct FixedBucket(u32);

impl FeaturevisorModule for FixedBucket {
    fn bucket_value(&self, _options: ConfigureBucketValueOptions) -> u32 {
        self.0
    }
}

struct ReportingModule;

impl FeaturevisorModule for ReportingModule {
    fn name(&self) -> Option<&str> {
        Some("conformance")
    }

    fn setup(&self, api: &ModuleApi) {
        api.report_diagnostic(Diagnostic::new(
            LogLevel::Info,
            "module_ready",
            "Module ready",
        ));
    }
}

struct PanickingModule;

impl FeaturevisorModule for PanickingModule {
    fn name(&self) -> Option<&str> {
        Some("broken")
    }

    fn setup(&self, _api: &ModuleApi) {
        panic!("setup failed");
    }
}

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

fn condition_feature(condition: Value) -> featurevisor::DatafileContent {
    feature(json!("segment"), condition)
}

#[test]
fn fixture_version_and_every_section_are_present() {
    let fixture: Value = serde_json::from_str(include_str!("../conformance/sdk-v3.json")).unwrap();
    assert_eq!(fixture["version"], 6);
    for section in [
        "bucketing",
        "regularExpressions",
        "typedVariables",
        "datafile",
        "globalVariables",
        "requiredFeatures",
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
fn global_variables_and_required_features_match_the_canonical_fixture() {
    let fixture: Value = serde_json::from_str(include_str!("../conformance/sdk-v3.json")).unwrap();
    let global = &fixture["globalVariables"];
    for case in global["cases"].as_array().unwrap() {
        let mut options = FeaturevisorOptions {
            datafile: Some(DatafileInput::Content(
                serde_json::from_value(global["datafile"].clone()).unwrap(),
            )),
            ..Default::default()
        };
        if let Some(sticky) = case.get("stickyVariables") {
            options.sticky_variables = Some(serde_json::from_value(sticky.clone()).unwrap());
        }
        let f = create_featurevisor(options);
        let context: Context = case
            .get("context")
            .and_then(Value::as_object)
            .map(|values| {
                values
                    .iter()
                    .map(|(key, value)| (key.clone(), AttributeValue::from_json(value.clone())))
                    .collect()
            })
            .unwrap_or_default();
        let override_options = featurevisor::OverrideOptions {
            default_variable_value: case
                .get("defaultVariableValue")
                .cloned()
                .map(featurevisor::VariableValue::from_json),
            ..Default::default()
        };
        let evaluation = f.evaluate_global_variable(
            case["key"].as_str().unwrap(),
            Some(&context),
            Some(&override_options),
        );
        assert_eq!(
            serde_json::to_value(&evaluation.reason).unwrap(),
            case["expectedReason"],
            "{}",
            case["name"]
        );
        if let Some(expected) = case.get("expectedValue") {
            assert_eq!(
                evaluation
                    .variable_value
                    .as_ref()
                    .map(|value| value.to_json()),
                Some(expected.clone()),
                "{}",
                case["name"]
            );
        } else {
            assert!(evaluation.variable_value.is_none(), "{}", case["name"]);
        }
        assert_eq!(
            evaluation
                .variable_override_index
                .map(|value| Value::from(value as u64))
                .as_ref(),
            case.get("expectedOverrideIndex"),
            "{}",
            case["name"]
        );
        assert_eq!(
            evaluation.variable_override_key.as_deref(),
            case.get("expectedOverrideKey").and_then(Value::as_str),
            "{}",
            case["name"]
        );
        if let Some(expected) = case.get("expectedOverridePath") {
            assert_eq!(
                serde_json::to_value(&evaluation.variable_override_path).unwrap(),
                *expected,
                "{}",
                case["name"]
            );
        }
    }

    let required = &fixture["requiredFeatures"];
    let f = create_featurevisor(FeaturevisorOptions {
        datafile: Some(DatafileInput::Content(
            serde_json::from_value(required["datafile"].clone()).unwrap(),
        )),
        ..Default::default()
    });
    for case in required["cases"].as_array().unwrap() {
        assert_eq!(
            f.is_enabled(case["feature"].as_str().unwrap(), None),
            case["expectedEnabled"].as_bool().unwrap(),
            "{}",
            case["name"]
        );
    }
    let variable_case = &required["featureVariableCase"];
    assert!(f.is_enabled("enabledFeature", None));
    let evaluation = f.evaluate_variable(
        variable_case["feature"].as_str().unwrap(),
        variable_case["variable"].as_str().unwrap(),
        None,
        None,
    );
    assert_eq!(
        evaluation.variable_value.map(|value| value.to_json()),
        Some(variable_case["expectedValue"].clone())
    );
    assert_eq!(
        evaluation.variable_override_key.as_deref(),
        variable_case["expectedOverrideKey"].as_str()
    );
}

#[test]
fn global_variable_datafile_events_include_direct_and_dependency_changes() {
    let fixture: Value = serde_json::from_str(include_str!("../conformance/sdk-v3.json")).unwrap();
    for (case_key, expected_key, replace) in [
        ("merge", "expectedAfterMerge", false),
        ("replacement", "expectedAfterReplacement", true),
    ] {
        let update = &fixture["globalVariables"]["datafileUpdateCase"];
        let f = create_featurevisor(FeaturevisorOptions {
            datafile: Some(DatafileInput::Content(
                serde_json::from_value(update["initial"].clone()).unwrap(),
            )),
            ..Default::default()
        });
        if replace {
            f.set_datafile(
                DatafileInput::Content(serde_json::from_value(update["merge"].clone()).unwrap()),
                false,
            );
        }
        let observed = Arc::new(Mutex::new(None));
        let copy = Arc::clone(&observed);
        let _unsubscribe = f.on(
            EventName::DatafileSet,
            Arc::new(move |event| {
                if let EventDetails::DatafileSet(details) = event {
                    *copy.lock().unwrap() = Some(details.clone());
                }
            }),
        );
        f.set_datafile(
            DatafileInput::Content(serde_json::from_value(update[case_key].clone()).unwrap()),
            replace,
        );
        let details = observed.lock().unwrap().clone().unwrap();
        let mut features = details.features;
        let mut variables = details.variables;
        features.sort();
        variables.sort();
        let mut expected_features: Vec<String> =
            serde_json::from_value(update[expected_key]["changedFeatures"].clone()).unwrap();
        let mut expected_variables: Vec<String> =
            serde_json::from_value(update[expected_key]["changedVariables"].clone()).unwrap();
        expected_features.sort();
        expected_variables.sort();
        assert_eq!(features, expected_features, "{case_key}");
        assert_eq!(variables, expected_variables, "{case_key}");
    }

    let dependency = &fixture["globalVariables"]["dependencyUpdateCase"];
    let f = create_featurevisor(FeaturevisorOptions {
        datafile: Some(DatafileInput::Content(
            serde_json::from_value(dependency["initial"].clone()).unwrap(),
        )),
        ..Default::default()
    });
    let observed = Arc::new(Mutex::new(None));
    let copy = Arc::clone(&observed);
    let _unsubscribe = f.on(
        EventName::DatafileSet,
        Arc::new(move |event| {
            if let EventDetails::DatafileSet(details) = event {
                *copy.lock().unwrap() = Some(details.clone());
            }
        }),
    );
    f.set_datafile(
        DatafileInput::Content(serde_json::from_value(dependency["updated"].clone()).unwrap()),
        true,
    );
    let details = observed.lock().unwrap().clone().unwrap();
    let mut features = details.features;
    features.sort();
    let mut variables = details.variables;
    variables.sort();
    let mut expected_features: Vec<String> =
        serde_json::from_value(dependency["expectedChangedFeatures"].clone()).unwrap();
    expected_features.sort();
    let mut expected_variables: Vec<String> =
        serde_json::from_value(dependency["expectedChangedVariables"].clone()).unwrap();
    expected_variables.sort();
    assert_eq!(features, expected_features);
    assert_eq!(variables, expected_variables);
}

#[test]
fn fixture_bucketing_numbers_regex_and_typed_values_are_executed() {
    let fixture: Value = serde_json::from_str(include_str!("../conformance/sdk-v3.json")).unwrap();
    assert_eq!(fixture["bucketing"]["minimum"], 0);
    assert_eq!(fixture["bucketing"]["maximum"], 100000);
    for (bucket_value, expected) in fixture["bucketing"]["percentage"]["enabledAt"]
        .as_array()
        .unwrap()
        .iter()
        .zip([true, true])
    {
        let f = create_featurevisor(FeaturevisorOptions {
            datafile: Some(DatafileInput::Content(datafile(
                json!({}),
                json!({
                    "feature": {
                        "bucketBy": "userId",
                        "traffic": [{ "key": "rule", "segments": "*", "percentage": 50000 }]
                    }
                }),
            ))),
            modules: vec![Arc::new(FixedBucket(bucket_value.as_u64().unwrap() as u32))],
            ..Default::default()
        });
        assert_eq!(f.is_enabled("feature", None), expected);
    }
    for bucket_value in fixture["bucketing"]["percentage"]["disabledAt"]
        .as_array()
        .unwrap()
    {
        let f = create_featurevisor(FeaturevisorOptions {
            datafile: Some(DatafileInput::Content(datafile(
                json!({}),
                json!({
                    "feature": {
                        "bucketBy": "userId",
                        "traffic": [{ "key": "rule", "segments": "*", "percentage": 50000 }]
                    }
                }),
            ))),
            modules: vec![Arc::new(FixedBucket(bucket_value.as_u64().unwrap() as u32))],
            ..Default::default()
        });
        assert!(!f.is_enabled("feature", None));
    }
    for (bucket_value, expected) in fixture["bucketing"]["allocationExpectations"]
        .as_object()
        .unwrap()
    {
        let f = create_featurevisor(FeaturevisorOptions {
            datafile: Some(DatafileInput::Content(datafile(
                json!({}),
                json!({
                    "feature": {
                        "bucketBy": "userId",
                        "variations": [{ "value": "control" }, { "value": "treatment" }],
                        "traffic": [{ "key": "rule", "segments": "*", "percentage": 100000, "allocation": [
                            { "variation": "control", "range": [0, 50000] },
                            { "variation": "treatment", "range": [50000, 100000] }
                        ] }]
                    }
                }),
            ))),
            modules: vec![Arc::new(FixedBucket(bucket_value.parse::<u32>().unwrap()))],
            ..Default::default()
        });
        assert_eq!(
            f.get_variation("feature", None, None).as_deref(),
            Some(expected.as_str().unwrap())
        );
    }
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
    let repeated_condition = json!({
        "attribute": "value",
        "operator": "matches",
        "value": fixture["regularExpressions"]["pattern"],
        "regexFlags": fixture["regularExpressions"]["flags"]
    });
    for (value, expected) in fixture["regularExpressions"]["values"]
        .as_array()
        .unwrap()
        .iter()
        .zip(fixture["regularExpressions"]["matches"].as_array().unwrap())
    {
        let f = create_featurevisor(FeaturevisorOptions {
            datafile: Some(DatafileInput::Content(feature(
                json!("segment"),
                repeated_condition.clone(),
            ))),
            ..Default::default()
        });
        let context = [(
            "value".to_string(),
            AttributeValue::from(value.as_str().unwrap()),
        )]
        .into_iter()
        .collect();
        assert_eq!(
            f.is_enabled("feature", Some(&context)),
            expected.as_bool().unwrap()
        );
    }
    for flag in fixture["portableConditions"]["rejectedRegexFlags"]
        .as_array()
        .unwrap()
    {
        let diagnostics = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&diagnostics);
        let f = create_featurevisor(FeaturevisorOptions {
            datafile: Some(DatafileInput::Content(condition_feature(json!({
                "attribute": "value",
                "operator": "matches",
                "value": "chrome",
                "regexFlags": flag
            })))),
            on_diagnostic: Some(Arc::new(move |diagnostic| {
                observed.lock().unwrap().push(diagnostic.clone())
            })),
            ..Default::default()
        });
        let context = [("value".to_string(), AttributeValue::from("chrome"))]
            .into_iter()
            .collect();
        assert!(!f.is_enabled("feature", Some(&context)), "flag {flag}");
        assert!(diagnostics
            .lock()
            .unwrap()
            .iter()
            .any(|diagnostic| diagnostic.code == "condition_match_error"));
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
fn fixture_portable_conditions_dates_semver_and_invalid_inputs_are_executed() {
    let fixture: Value = serde_json::from_str(include_str!("../conformance/sdk-v3.json")).unwrap();
    let dates = &fixture["portableConditions"]["dates"];
    let before = create_featurevisor(FeaturevisorOptions {
        datafile: Some(DatafileInput::Content(feature(
            json!("segment"),
            json!({
                "attribute": "date",
                "operator": "before",
                "value": dates[2]
            }),
        ))),
        ..Default::default()
    });
    let context = [(
        "date".to_string(),
        AttributeValue::from(dates[0].as_str().unwrap()),
    )]
    .into_iter()
    .collect();
    assert!(before.is_enabled("feature", Some(&context)));

    for (context_version, operator, expected_version, expected) in [
        ("1.2.3", "semverEquals", "1.2.3+build.5", true),
        ("1.2.3-beta.1", "semverLessThan", "1.2.3", true),
        ("1.2.3+build.5", "semverEquals", "1.2.3", true),
    ] {
        let f = create_featurevisor(FeaturevisorOptions {
            datafile: Some(DatafileInput::Content(feature(
                json!("segment"),
                json!({
                    "attribute": "version",
                    "operator": operator,
                    "value": expected_version
                }),
            ))),
            ..Default::default()
        });
        let context = [("version".to_string(), AttributeValue::from(context_version))]
            .into_iter()
            .collect();
        assert_eq!(f.is_enabled("feature", Some(&context)), expected);
    }

    let diagnostics = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&diagnostics);
    let f = create_featurevisor(FeaturevisorOptions {
        datafile: Some(DatafileInput::Content(feature(
            json!("segment"),
            json!({
                "attribute": "version",
                "operator": "semverEquals",
                "value": fixture["portableConditions"]["semanticVersions"][0]
            }),
        ))),
        on_diagnostic: Some(Arc::new(move |diagnostic| {
            observed.lock().unwrap().push(diagnostic.clone())
        })),
        ..Default::default()
    });
    let context = [("version".to_string(), AttributeValue::from("invalid"))]
        .into_iter()
        .collect();
    assert!(!f.is_enabled("feature", Some(&context)));
    assert!(diagnostics.lock().unwrap().iter().any(|diagnostic| {
        diagnostic.code
            == fixture["portableConditions"]["invalidSemanticVersionDiagnosticCode"]
                .as_str()
                .unwrap()
    }));
}

#[test]
fn fixture_child_context_case_is_executed() {
    let fixture: Value = serde_json::from_str(include_str!("../conformance/sdk-v3.json")).unwrap();
    let datafile = datafile(
        json!({}),
        json!({
            "country": { "bucketBy": "userId", "traffic": [{ "key": "rule", "segments": "*", "conditions": { "attribute": "country", "operator": "equals", "value": "de" }, "percentage": 100000, "enabled": true }] },
            "plan": { "bucketBy": "userId", "traffic": [{ "key": "rule", "segments": "*", "conditions": { "attribute": "plan", "operator": "equals", "value": "free" }, "percentage": 100000, "enabled": true }] },
            "region": { "bucketBy": "userId", "traffic": [{ "key": "rule", "segments": "*", "conditions": { "attribute": "region", "operator": "equals", "value": "eu" }, "percentage": 100000, "enabled": true }] },
            "experiment": { "bucketBy": "userId", "variations": [{ "value": "control" }], "traffic": [{ "key": "rule", "segments": "*", "percentage": 100000, "allocation": [{ "variation": "control", "range": [0, 100000] }] }] },
            "config": { "bucketBy": "userId", "variablesSchema": { "value": { "type": "string", "defaultValue": "default" } }, "traffic": [{ "key": "rule", "segments": "*", "percentage": 100000, "variables": { "value": "child" } }] }
        }),
    );
    let parent_context: Context = fixture["childInstances"]["contextCase"]["parentAtSpawn"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, value)| (key.clone(), AttributeValue::from_json(value.clone())))
        .collect();
    let child_context: Context = fixture["childInstances"]["contextCase"]["child"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, value)| (key.clone(), AttributeValue::from_json(value.clone())))
        .collect();
    let parent_after_spawn: Context = fixture["childInstances"]["contextCase"]["parentAfterSpawn"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, value)| (key.clone(), AttributeValue::from_json(value.clone())))
        .collect();
    let f = create_featurevisor(FeaturevisorOptions {
        datafile: Some(DatafileInput::Content(datafile)),
        context: Some(parent_context),
        ..Default::default()
    });
    let child = f.spawn(child_context, Default::default());
    f.set_context(parent_after_spawn, false);
    let expected = &fixture["childInstances"]["contextCase"]["expected"];
    assert_eq!(
        child.is_enabled("country", None),
        expected["country"] == "de"
    );
    assert_eq!(child.is_enabled("plan", None), expected["plan"] == "free");
    assert_eq!(child.is_enabled("region", None), expected["region"] == "eu");
    assert_eq!(child.evaluate_flag("country", None).enabled, Some(true));
    assert_eq!(
        child.get_variation("experiment", None, None).as_deref(),
        Some("control")
    );
    assert_eq!(
        child.get_variable_string("config", "value", None, None),
        Some("child".to_string())
    );
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
        datafile: Some(DatafileInput::Content(default_datafile.clone())),
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
    for value in [
        featurevisor::VariableValue::String(String::new()),
        featurevisor::VariableValue::Integer(0),
        featurevisor::VariableValue::Boolean(false),
        featurevisor::VariableValue::Null,
    ] {
        assert_eq!(
            f.get_variable(
                "experiment",
                "missing",
                None,
                Some(&featurevisor::OverrideOptions {
                    default_variable_value: Some(value.clone()),
                    ..Default::default()
                })
            ),
            Some(value)
        );
    }

    let diagnostics = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed = std::sync::Arc::clone(&diagnostics);
    let f = create_featurevisor(FeaturevisorOptions {
        datafile: Some(DatafileInput::Content(default_datafile.clone())),
        modules: vec![std::sync::Arc::new(ReportingModule)],
        on_diagnostic: Some(std::sync::Arc::new(move |diagnostic| {
            observed.lock().unwrap().push(diagnostic.clone())
        })),
        ..Default::default()
    });
    let errors = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed_errors = std::sync::Arc::clone(&errors);
    let _unsubscribe = f.on(
        EventName::Error,
        std::sync::Arc::new(move |details| {
            if let EventDetails::Error { diagnostic } = details {
                observed_errors.lock().unwrap().push(diagnostic.clone());
            }
        }),
    );
    assert!(!f.is_enabled(
        fixture["diagnosticCase"]["featureKey"].as_str().unwrap(),
        None
    ));
    let _ = f.get_variable("experiment", "missing", None, None);
    f.set_datafile(DatafileInput::Json("not-json".to_string()), false);
    let diagnostics = diagnostics.lock().unwrap();
    let feature_not_found = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "feature_not_found")
        .expect("feature_not_found diagnostic");
    assert_eq!(feature_not_found.level, featurevisor::LogLevel::Warn);
    let serialized = serde_json::to_value(feature_not_found).unwrap();
    for field in fixture["diagnostics"]["requiredFields"].as_array().unwrap() {
        assert!(serialized.get(field.as_str().unwrap()).is_some(), "{field}");
    }
    assert!(serialized["details"].is_object());
    let variable_not_found = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "variable_not_found")
        .expect("variable_not_found diagnostic");
    for field in fixture["diagnostics"]["evaluationDetailFields"]
        .as_array()
        .unwrap()
    {
        assert!(
            variable_not_found
                .details
                .contains_key(field.as_str().unwrap()),
            "{field}"
        );
    }
    let module_ready = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "module_ready")
        .expect("module diagnostic");
    assert_eq!(module_ready.module.as_deref(), Some("conformance"));
    assert!(module_ready.details.is_empty());
    let errors = errors.lock().unwrap();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].level, featurevisor::LogLevel::Error);

    let setup_diagnostics = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed_setup = std::sync::Arc::clone(&setup_diagnostics);
    let _ = create_featurevisor(FeaturevisorOptions {
        on_diagnostic: Some(std::sync::Arc::new(move |diagnostic| {
            observed_setup.lock().unwrap().push(diagnostic.clone())
        })),
        modules: vec![std::sync::Arc::new(PanickingModule)],
        ..Default::default()
    });
    let setup_diagnostics = setup_diagnostics.lock().unwrap();
    let setup_error = setup_diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "module_setup_error")
        .expect("module setup diagnostic");
    assert_eq!(setup_error.module_name.as_deref(), Some("broken"));
    assert!(setup_error.original_error.is_some());
    let module_ready_json = serde_json::to_value(module_ready).unwrap();
    let setup_error_json = serde_json::to_value(setup_error).unwrap();
    for field in fixture["diagnostics"]["moduleEnvelopeFields"]
        .as_array()
        .unwrap()
    {
        let field = field.as_str().unwrap();
        let present =
            module_ready_json.get(field).is_some() || setup_error_json.get(field).is_some();
        assert!(present, "{field}");
    }
    assert!(fixture["diagnostics"]["errorEventLevels"]
        .as_array()
        .unwrap()
        .iter()
        .any(|level| level == "error"));

    let numeric = create_featurevisor(FeaturevisorOptions {
        datafile: Some(DatafileInput::Content(condition_feature(json!({
            "attribute": "score",
            "operator": "greaterThan",
            "value": 1.5
        })))),
        ..Default::default()
    });
    for value in [AttributeValue::Integer(2), AttributeValue::Double(2.0)] {
        let context = [("score".to_string(), value)].into_iter().collect();
        assert!(numeric.is_enabled("feature", Some(&context)));
    }
    let includes = create_featurevisor(FeaturevisorOptions {
        datafile: Some(DatafileInput::Content(condition_feature(json!({
            "attribute": "roles",
            "operator": "includes",
            "value": "admin"
        })))),
        ..Default::default()
    });
    let context = [(
        "roles".to_string(),
        AttributeValue::Array(vec![
            AttributeValue::from("user"),
            AttributeValue::from("admin"),
        ]),
    )]
    .into_iter()
    .collect();
    assert!(includes.is_enabled("feature", Some(&context)));
}
