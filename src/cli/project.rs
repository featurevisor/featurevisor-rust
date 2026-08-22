use crate::{DatafileContent, DatafileInput};
use serde_json::Value as JsonValue;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) fn run_node(project: &Path, command: &str, args: &[String]) -> Result<String, String> {
    let mut process = Command::new("npx");
    process
        .arg("featurevisor")
        .arg(command)
        .args(args)
        .current_dir(project);
    let output = process.output().map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub(crate) fn json_command(
    project: &Path,
    command: &str,
    mut args: Vec<String>,
) -> Result<JsonValue, String> {
    args.push("--json".to_string());
    let output = run_node(project, command, &args)?;
    serde_json::from_str(&output)
        .map_err(|error| format!("Could not parse {command} output: {error}"))
}

pub(crate) fn project_path(value: &str) -> PathBuf {
    PathBuf::from(value)
}

pub(crate) fn list_targets(project: &Path) -> Result<Vec<String>, String> {
    let value = json_command(project, "list", vec!["--targets".to_string()])?;
    Ok(unique_targets(
        value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| {
                item.get("key")
                    .and_then(JsonValue::as_str)
                    .map(str::to_string)
            })
            .collect(),
    ))
}

pub(crate) fn unique_targets(targets: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    targets
        .into_iter()
        .filter(|target| seen.insert(target.clone()))
        .collect()
}

pub(crate) fn build_datafile(
    project: &Path,
    environment: Option<&str>,
    target: Option<&str>,
    inflate: u32,
) -> Result<DatafileContent, String> {
    let mut args = Vec::new();
    if let Some(environment) = environment {
        args.push(format!("--environment={environment}"));
    }
    if let Some(target) = target {
        args.push(format!("--target={target}"));
    }
    if inflate > 1 {
        args.push(format!("--inflate={inflate}"));
    }
    let value = json_command(project, "build", args)?;
    serde_json::from_value(value)
        .map_err(|error| format!("Could not parse generated datafile: {error}"))
}

pub(crate) fn datafile_key(environment: Option<&str>, target: Option<&str>) -> String {
    // Keep the no-environment key compatible with the other SDK runners.
    match target {
        Some(target) => format!("{}-target-{target}", environment.unwrap_or("false")),
        None => environment.unwrap_or("false").to_string(),
    }
}

pub(crate) fn input(datafile: DatafileContent) -> DatafileInput {
    DatafileInput::Content(datafile)
}

#[cfg(test)]
mod tests {
    use super::{datafile_key, unique_targets};

    #[test]
    fn datafile_keys_match_the_javascript_runner_shape() {
        assert_eq!(datafile_key(None, None), "false");
        assert_eq!(datafile_key(Some("production"), None), "production");
        assert_eq!(
            datafile_key(None, Some("checkout")),
            "false-target-checkout"
        );
        assert_eq!(
            datafile_key(Some("production"), Some("checkout")),
            "production-target-checkout"
        );
    }

    #[test]
    fn target_values_are_deduplicated_without_reordering() {
        assert_eq!(
            unique_targets(vec![
                "web".to_string(),
                "mobile".to_string(),
                "web".to_string(),
            ]),
            vec!["web".to_string(), "mobile".to_string()]
        );
    }
}
