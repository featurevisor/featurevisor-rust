use super::options::BenchmarkOptions;
use super::project::{build_datafile, input, list_targets, project_path, unique_targets};
use crate::FeaturevisorOptions;
use serde_json::Value as JsonValue;
use std::path::Path;
use std::time::Instant;

fn context(value: &str) -> Result<crate::Context, String> {
    let value: JsonValue = serde_json::from_str(value).map_err(|error| error.to_string())?;
    crate::helpers::json_to_context(&value)
        .ok_or_else(|| "Context must be a JSON object".to_string())
}

fn one(options: &BenchmarkOptions, project: &Path, target: Option<&str>) -> Result<(), String> {
    let datafile = build_datafile(
        project,
        options.common.environment.as_deref(),
        target,
        options.common.inflate,
    )?;
    let context = context(&options.context)?;
    let f = crate::create_featurevisor(FeaturevisorOptions {
        datafile: Some(input(datafile)),
        ..Default::default()
    });
    let n = options.n.max(1);
    let mut durations = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let start = Instant::now();
        if let Some(variable) = &options.variable {
            let _ = f.get_variable(&options.feature, variable, Some(&context), None);
        } else if options.variation {
            let _ = f.get_variation(&options.feature, Some(&context), None);
        } else {
            let _ = f.is_enabled(&options.feature, Some(&context));
        }
        durations.push(start.elapsed().as_nanos());
    }
    let min = *durations.iter().min().unwrap_or(&0);
    let max = *durations.iter().max().unwrap_or(&0);
    let total: u128 = durations.iter().sum();
    let average = total / durations.len() as u128;
    println!(
        "Benchmark{}",
        target
            .map(|target| format!(" target={target}"))
            .unwrap_or_default()
    );
    println!("  Evaluations: {n}");
    println!("  Minimum duration: {min}ns");
    println!("  Average duration: {average}ns");
    println!("  Maximum duration: {max}ns");
    println!("  Total duration: {total}ns");
    Ok(())
}

pub fn run(options: BenchmarkOptions) -> Result<(), String> {
    let project = project_path(&options.common.project_directory_path);
    let targets: Vec<Option<String>> = if options.common.target.is_empty() {
        vec![None]
    } else {
        let available = list_targets(&project)?;
        for target in &options.common.target {
            if !available.contains(target) {
                return Err(format!("Unknown target \"{target}\""));
            }
        }
        unique_targets(options.common.target.clone())
            .into_iter()
            .map(Some)
            .collect()
    };
    for target in targets {
        one(&options, &project, target.as_deref())?;
    }
    Ok(())
}
