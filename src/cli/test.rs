use super::options::TestOptions;
use super::project::{
    build_datafile, datafile_key, input, json_command, list_targets, project_path, unique_targets,
};
use crate::modules::ConfigureBucketValueOptions;
use crate::{
    Featurevisor, FeaturevisorChild, FeaturevisorModule, FeaturevisorOptions, LogLevel,
    OverrideOptions, Segment, SpawnOptions,
};
use regex::{Regex, RegexBuilder};
use serde_json::{Map, Value as JsonValue};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

struct AtModule {
    at: Option<f64>,
}

impl FeaturevisorModule for AtModule {
    fn name(&self) -> Option<&str> {
        Some("tester")
    }

    fn bucket_value(&self, options: ConfigureBucketValueOptions) -> u32 {
        self.at
            .map(|at| (at * (crate::MAX_BUCKETED_NUMBER as f64 / 100.0)) as u32)
            .unwrap_or(options.bucket_value)
    }
}

fn context_from_json(value: Option<&JsonValue>) -> crate::Context {
    value
        .and_then(crate::helpers::json_to_context)
        .unwrap_or_default()
}

fn log_level(options: &TestOptions) -> LogLevel {
    if options.common.verbose {
        LogLevel::Debug
    } else if options.common.quiet {
        LogLevel::Error
    } else {
        LogLevel::Warn
    }
}

fn compile_pattern(option: &str, pattern: Option<&str>) -> Result<Option<Regex>, String> {
    pattern
        .map(|pattern| {
            RegexBuilder::new(pattern)
                .case_insensitive(true)
                .build()
                .map_err(|error| format!("Invalid {option}: {error}"))
        })
        .transpose()
}

fn json_equal(
    actual: Option<&crate::VariableValue>,
    expected: &JsonValue,
    variable_type: &str,
) -> bool {
    let mut expected = expected.clone();
    if variable_type == "json" {
        if let Some(value) = expected.as_str() {
            if let Ok(parsed) = serde_json::from_str(value) {
                expected = parsed;
            }
        }
    }
    actual
        .map(crate::VariableValue::to_json)
        .unwrap_or(JsonValue::Null)
        == expected
}

fn compare_evaluation(
    errors: &mut Vec<String>,
    feature: &str,
    kind: &str,
    expected: &Map<String, JsonValue>,
    actual: &JsonValue,
) {
    for (key, expected_value) in expected {
        if actual.get(key) != Some(expected_value) {
            errors.push(format!(
                "{feature}: {kind} evaluation {key} expected {expected_value}, got {}",
                actual.get(key).cloned().unwrap_or(JsonValue::Null)
            ));
        }
    }
}

fn compare_evaluations(
    errors: &mut Vec<String>,
    feature_key: &str,
    assertion: &JsonValue,
    sdk: &Featurevisor,
) {
    let Some(expected_evaluations) = assertion
        .get("expectedEvaluations")
        .and_then(JsonValue::as_object)
    else {
        return;
    };

    if let Some(expected) = expected_evaluations
        .get("flag")
        .and_then(JsonValue::as_object)
    {
        let actual =
            serde_json::to_value(sdk.evaluate_flag(feature_key, None)).unwrap_or(JsonValue::Null);
        compare_evaluation(errors, feature_key, "flag", expected, &actual);
    }
    if let Some(expected) = expected_evaluations
        .get("variation")
        .and_then(JsonValue::as_object)
    {
        let actual = serde_json::to_value(sdk.evaluate_variation(feature_key, None, None))
            .unwrap_or(JsonValue::Null);
        compare_evaluation(errors, feature_key, "variation", expected, &actual);
    }
    if let Some(expected_variables) = expected_evaluations
        .get("variables")
        .and_then(JsonValue::as_object)
    {
        for (variable_key, expected) in expected_variables {
            if let Some(expected) = expected.as_object() {
                let actual = serde_json::to_value(sdk.evaluate_variable(
                    feature_key,
                    variable_key,
                    None,
                    None,
                ))
                .unwrap_or(JsonValue::Null);
                compare_evaluation(
                    errors,
                    feature_key,
                    &format!("variable {variable_key}"),
                    expected,
                    &actual,
                );
            }
        }
    }
}

fn compare_child_evaluations(
    errors: &mut Vec<String>,
    feature_key: &str,
    assertion: &JsonValue,
    child: &FeaturevisorChild,
) {
    let Some(expected_evaluations) = assertion
        .get("expectedEvaluations")
        .and_then(JsonValue::as_object)
    else {
        return;
    };

    if let Some(expected) = expected_evaluations
        .get("flag")
        .and_then(JsonValue::as_object)
    {
        let actual =
            serde_json::to_value(child.evaluate_flag(feature_key, None)).unwrap_or(JsonValue::Null);
        compare_evaluation(errors, feature_key, "child flag", expected, &actual);
    }
    if let Some(expected) = expected_evaluations
        .get("variation")
        .and_then(JsonValue::as_object)
    {
        let actual = serde_json::to_value(child.evaluate_variation(feature_key, None, None))
            .unwrap_or(JsonValue::Null);
        compare_evaluation(errors, feature_key, "child variation", expected, &actual);
    }
    if let Some(expected_variables) = expected_evaluations
        .get("variables")
        .and_then(JsonValue::as_object)
    {
        for (variable_key, expected) in expected_variables {
            if let Some(expected) = expected.as_object() {
                let actual = serde_json::to_value(child.evaluate_variable(
                    feature_key,
                    variable_key,
                    None,
                    None,
                ))
                .unwrap_or(JsonValue::Null);
                compare_evaluation(
                    errors,
                    feature_key,
                    &format!("child variable {variable_key}"),
                    expected,
                    &actual,
                );
            }
        }
    }
}

fn compare_variables(
    errors: &mut Vec<String>,
    feature_key: &str,
    assertion: &JsonValue,
    sdk: &Featurevisor,
) {
    let Some(expected_variables) = assertion
        .get("expectedVariables")
        .and_then(JsonValue::as_object)
    else {
        return;
    };
    let feature = sdk.get_feature(feature_key);

    for (variable_key, expected) in expected_variables {
        let Some(schema) = feature
            .as_ref()
            .and_then(|feature| feature.variables_schema.as_ref())
            .and_then(|schemas| schemas.get(variable_key))
        else {
            errors.push(format!(
                "{feature_key}.{variable_key}: variable schema not found"
            ));
            continue;
        };
        let default_value = assertion
            .get("defaultVariableValues")
            .and_then(|values| values.get(variable_key))
            .map(|value| crate::VariableValue::from_json(value.clone()));
        let actual = sdk.get_variable(
            feature_key,
            variable_key,
            None,
            Some(&OverrideOptions {
                default_variation_value: None,
                default_variable_value: default_value,
            }),
        );
        if !json_equal(actual.as_ref(), expected, &schema.variable_type) {
            errors.push(format!(
                "{feature_key}.{variable_key}: expected {expected}, got {}",
                actual
                    .map(|value| value.to_json())
                    .unwrap_or(JsonValue::Null)
            ));
        }
    }
}

fn compare_child_variables(
    errors: &mut Vec<String>,
    feature_key: &str,
    assertion: &JsonValue,
    child: &FeaturevisorChild,
) {
    let Some(expected_variables) = assertion
        .get("expectedVariables")
        .and_then(JsonValue::as_object)
    else {
        return;
    };
    for (variable_key, expected) in expected_variables {
        let default_value = assertion
            .get("defaultVariableValues")
            .and_then(|values| values.get(variable_key))
            .map(|value| crate::VariableValue::from_json(value.clone()));
        let actual = child.get_variable(
            feature_key,
            variable_key,
            None,
            Some(&OverrideOptions {
                default_variation_value: None,
                default_variable_value: default_value,
            }),
        );
        if actual
            .as_ref()
            .map(crate::VariableValue::to_json)
            .unwrap_or(JsonValue::Null)
            != *expected
        {
            errors.push(format!(
                "{feature_key}.{variable_key}: child expected {expected}, got {}",
                actual
                    .map(|value| value.to_json())
                    .unwrap_or(JsonValue::Null)
            ));
        }
    }
}

fn compare_children(
    errors: &mut Vec<String>,
    feature_key: &str,
    assertion: &JsonValue,
    sdk: &Featurevisor,
) {
    let Some(children) = assertion.get("children").and_then(JsonValue::as_array) else {
        return;
    };

    for (index, child) in children.iter().enumerate() {
        let child_context = context_from_json(child.get("context"));
        let child_sdk = sdk.spawn(
            child_context,
            SpawnOptions {
                sticky_features: child
                    .get("sticky")
                    .and_then(|value| serde_json::from_value(value.clone()).ok()),
                sticky_variables: child
                    .get("stickyVariables")
                    .and_then(|value| serde_json::from_value(value.clone()).ok()),
            },
        );
        if let Some(expected) = child
            .get("expectedToBeEnabled")
            .and_then(JsonValue::as_bool)
        {
            let actual = child_sdk.is_enabled(feature_key, None);
            if actual != expected {
                errors.push(format!(
                    "{feature_key}: child {index} expected enabled {expected}, got {actual}"
                ));
            }
        }
        if let Some(expected) = child.get("expectedVariation").and_then(JsonValue::as_str) {
            let actual = child_sdk.get_variation(feature_key, None, None);
            if actual.as_deref() != Some(expected) {
                errors.push(format!(
                    "{feature_key}: child {index} expected variation {expected}, got {actual:?}"
                ));
            }
        }
        compare_child_variables(errors, feature_key, child, &child_sdk);
        compare_child_evaluations(errors, feature_key, child, &child_sdk);
        child_sdk.close();
    }
}

fn run_assertion(
    test: &JsonValue,
    assertion: &JsonValue,
    options: &TestOptions,
    target_keys: &[String],
    datafiles: &HashMap<String, crate::DatafileContent>,
    segments: &HashMap<String, Segment>,
) -> Result<Vec<String>, String> {
    let feature_key = test.get("feature").and_then(JsonValue::as_str);
    let segment_key = test.get("segment").and_then(JsonValue::as_str);
    let variable_key = test.get("variable").and_then(JsonValue::as_str);
    let environment = assertion.get("environment").and_then(JsonValue::as_str);
    let target = assertion.get("target").and_then(JsonValue::as_str);

    if let Some(target) = target {
        if !target_keys.is_empty() && !target_keys.iter().any(|key| key == target) {
            return Ok(Vec::new());
        }
    }

    if let Some(segment_key) = segment_key {
        let datafile = base_datafile(datafiles, environment);
        let Some(datafile) = datafile else {
            return Err(format!(
                "No datafile available for segment assertion {segment_key}"
            ));
        };
        let context = context_from_json(assertion.get("context"));
        let mut segment_datafile = datafile.clone();
        segment_datafile.segments = segments.clone();
        let f = crate::create_featurevisor(FeaturevisorOptions {
            datafile: Some(input(segment_datafile)),
            context: Some(context.clone()),
            log_level: Some(log_level(options)),
            ..Default::default()
        });
        if f.get_segment(segment_key).is_none() {
            return Ok(vec![format!("{segment_key}: segment not found")]);
        }
        let actual = f.segment_matches(segment_key, &context);
        let expected = assertion
            .get("expectedToMatch")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false);
        return if actual == expected {
            Ok(Vec::new())
        } else {
            Ok(vec![format!(
                "{segment_key}: expected segment match {expected}, got {actual}"
            )])
        };
    }

    if let Some(variable_key) = variable_key {
        let selected_key = datafile_key(environment, target);
        let datafile = datafiles
            .get(&selected_key)
            .or_else(|| base_datafile(datafiles, environment));
        let Some(datafile) = datafile else {
            return Err(format!(
                "No datafile available for variable assertion {variable_key}"
            ));
        };
        let f = crate::create_featurevisor(FeaturevisorOptions {
            datafile: Some(input(datafile.clone())),
            context: Some(context_from_json(assertion.get("context"))),
            sticky_variables: assertion
                .get("stickyVariables")
                .and_then(|value| serde_json::from_value(value.clone()).ok()),
            log_level: Some(log_level(options)),
            ..Default::default()
        });
        let evaluation_options = OverrideOptions {
            default_variable_value: assertion
                .get("defaultVariableValue")
                .cloned()
                .map(crate::VariableValue::from_json),
            ..Default::default()
        };
        let evaluation = f.evaluate_global_variable(variable_key, None, Some(&evaluation_options));
        let mut errors = Vec::new();
        if let Some(expected) = assertion.get("expectedValue") {
            if evaluation
                .variable_value
                .as_ref()
                .map(crate::VariableValue::to_json)
                != Some(expected.clone())
            {
                errors.push(format!(
                    "{variable_key}: expected value {expected}, got {:?}",
                    evaluation.variable_value
                ));
            }
        }
        if let Some(expected) = assertion
            .get("expectedEvaluation")
            .and_then(JsonValue::as_object)
        {
            let actual = serde_json::to_value(&evaluation).unwrap_or(JsonValue::Null);
            compare_evaluation(&mut errors, variable_key, "variable", expected, &actual);
        }
        return Ok(errors);
    }

    let Some(feature_key) = feature_key else {
        return Ok(vec!["test has no feature, segment, or variable".to_string()]);
    };
    let selected_key = datafile_key(environment, target);
    let datafile = datafiles
        .get(&selected_key)
        .or_else(|| base_datafile(datafiles, environment));
    let Some(datafile) = datafile else {
        return Err(format!(
            "No datafile available for feature assertion {feature_key}"
        ));
    };
    if options.common.show_datafile {
        println!(
            "{}",
            serde_json::to_string_pretty(datafile).unwrap_or_default()
        );
    }

    let context = context_from_json(assertion.get("context"));
    let sticky = assertion
        .get("sticky")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_default();
    let module = Arc::new(AtModule {
        at: assertion.get("at").and_then(JsonValue::as_f64),
    });
    let f = crate::create_featurevisor(FeaturevisorOptions {
        datafile: Some(input(datafile.clone())),
        context: Some(context),
        sticky_features: Some(sticky),
        sticky_variables: assertion
            .get("stickyVariables")
            .and_then(|value| serde_json::from_value(value.clone()).ok()),
        log_level: Some(log_level(options)),
        modules: vec![module],
        ..Default::default()
    });
    let mut errors = Vec::new();

    if let Some(expected) = assertion
        .get("expectedToBeEnabled")
        .and_then(JsonValue::as_bool)
    {
        let actual = f.is_enabled(feature_key, None);
        if actual != expected {
            errors.push(format!(
                "{feature_key}: expected enabled {expected}, got {actual}"
            ));
        }
    }
    if let Some(expected) = assertion
        .get("expectedVariation")
        .and_then(JsonValue::as_str)
    {
        let options = OverrideOptions {
            default_variation_value: assertion
                .get("defaultVariationValue")
                .and_then(JsonValue::as_str)
                .map(str::to_string),
            default_variable_value: None,
        };
        let actual = f.get_variation(feature_key, None, Some(&options));
        if actual.as_deref() != Some(expected) {
            errors.push(format!(
                "{feature_key}: expected variation {expected}, got {:?}",
                actual
            ));
        }
    }
    compare_variables(&mut errors, feature_key, assertion, &f);
    compare_evaluations(&mut errors, feature_key, assertion, &f);
    compare_children(&mut errors, feature_key, assertion, &f);
    Ok(errors)
}

fn build_test_datafiles(
    project: &Path,
    environments: &[Option<String>],
    target_keys: &[String],
    inflate: u32,
) -> Result<HashMap<String, crate::DatafileContent>, String> {
    let mut datafiles = HashMap::new();
    for environment in environments {
        let base = build_datafile(project, environment.as_deref(), None, inflate)?;
        datafiles.insert(datafile_key(environment.as_deref(), None), base);
        for target in target_keys {
            let datafile = build_datafile(project, environment.as_deref(), Some(target), inflate)?;
            datafiles.insert(datafile_key(environment.as_deref(), Some(target)), datafile);
        }
    }
    Ok(datafiles)
}

fn project_environments(project: &Path) -> Result<Vec<Option<String>>, String> {
    let config = json_command(project, "config", Vec::new())?;
    let configured = config
        .get("environments")
        .and_then(JsonValue::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(JsonValue::as_str)
                .map(str::to_string)
                .map(Some)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if configured.is_empty() {
        Ok(vec![None])
    } else {
        Ok(configured)
    }
}

fn project_segments(project: &Path) -> Result<HashMap<String, Segment>, String> {
    let value = json_command(project, "list", vec!["--segments".to_string()])?;
    let items = value
        .as_array()
        .ok_or_else(|| "Expected segments JSON array".to_string())?;
    let mut segments = HashMap::new();
    for item in items {
        let Some(key) = item.get("key").and_then(JsonValue::as_str) else {
            continue;
        };
        let segment = serde_json::from_value::<Segment>(item.clone())
            .map_err(|error| format!("Could not parse segment {key}: {error}"))?;
        segments.insert(key.to_string(), segment);
    }
    Ok(segments)
}

fn base_datafile<'a>(
    datafiles: &'a HashMap<String, crate::DatafileContent>,
    environment: Option<&str>,
) -> Option<&'a crate::DatafileContent> {
    datafiles.get(&datafile_key(environment, None)).or_else(|| {
        datafiles
            .iter()
            .find(|(key, _)| !key.contains("-target-"))
            .map(|(_, value)| value)
    })
}

pub fn run(options: TestOptions) -> Result<(), String> {
    let project = project_path(&options.common.project_directory_path);
    let key_pattern = compile_pattern("--keyPattern", options.key_pattern.as_deref())?;
    let assertion_pattern =
        compile_pattern("--assertionPattern", options.assertion_pattern.as_deref())?;
    let target_keys = if options.common.target.is_empty() {
        list_targets(&project)?
    } else {
        let available = list_targets(&project)?;
        let selected = unique_targets(options.common.target.clone());
        for target in &selected {
            if !available.contains(target) {
                return Err(format!("Unknown target \"{target}\""));
            }
        }
        selected
    };
    let value = json_command(
        &project,
        "list",
        vec!["--tests".to_string(), "--apply-matrix".to_string()],
    )?;
    let tests = value
        .as_array()
        .ok_or_else(|| "Expected tests JSON array".to_string())?;
    let environments = project_environments(&project)?;
    let segments = project_segments(&project)?;
    let datafiles = build_test_datafiles(
        &project,
        &environments,
        &target_keys,
        options.common.inflate,
    )?;
    let mut passed_specs = 0usize;
    let mut failed_specs = 0usize;
    let mut passed_assertions = 0usize;
    let mut failed_assertions = 0usize;

    for test in tests {
        let key = test
            .get("key")
            .and_then(JsonValue::as_str)
            .unwrap_or("unknown");
        if key_pattern
            .as_ref()
            .is_some_and(|pattern| !pattern.is_match(key))
        {
            continue;
        }
        let mut test_failed = false;
        let mut assertions_run = 0usize;
        if let Some(assertions) = test.get("assertions").and_then(JsonValue::as_array) {
            for assertion in assertions {
                if assertion
                    .get("target")
                    .and_then(JsonValue::as_str)
                    .is_some_and(|target| !target_keys.iter().any(|key| key == target))
                {
                    continue;
                }
                if assertion_pattern.as_ref().is_some_and(|pattern| {
                    !pattern.is_match(
                        assertion
                            .get("description")
                            .and_then(JsonValue::as_str)
                            .unwrap_or(""),
                    )
                }) {
                    continue;
                }
                let errors = run_assertion(
                    test,
                    assertion,
                    &options,
                    &target_keys,
                    &datafiles,
                    &segments,
                )?;
                assertions_run += 1;
                if !errors.is_empty() {
                    test_failed = true;
                    failed_assertions += 1;
                    if !options.common.quiet {
                        let description = assertion
                            .get("description")
                            .and_then(JsonValue::as_str)
                            .unwrap_or("assertion");
                        for error in errors {
                            eprintln!("  {key} ({description}): {error}");
                        }
                    }
                } else {
                    passed_assertions += 1;
                }
            }
        }
        if assertions_run == 0 {
            continue;
        }
        if test_failed {
            failed_specs += 1;
            if !options.only_failures && !options.common.quiet {
                println!("FAIL {key}");
            }
        } else if !options.only_failures && !options.common.quiet {
            println!("PASS {key}");
            passed_specs += 1;
        } else {
            passed_specs += 1;
        }
    }
    println!("\n---");
    println!("Test specs: {passed_specs} passed, {failed_specs} failed");
    println!("Assertions: {passed_assertions} passed, {failed_assertions} failed");
    if passed_specs + failed_specs == 0 {
        return Err("No test specs matched the requested filters".to_string());
    }
    if failed_specs > 0 {
        return Err(format!("{failed_specs} test specs failed"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::compile_pattern;

    #[test]
    fn test_filters_are_case_insensitive_regular_expressions() {
        let pattern = compile_pattern("--keyPattern", Some(r"^features\.pricing$")).unwrap();
        assert!(pattern.unwrap().is_match("FEATURES.PRICING"));
    }

    #[test]
    fn invalid_test_filter_is_reported_before_running_the_command() {
        let error = compile_pattern("--keyPattern", Some("[")).unwrap_err();
        assert!(error.starts_with("Invalid --keyPattern:"));
    }
}
