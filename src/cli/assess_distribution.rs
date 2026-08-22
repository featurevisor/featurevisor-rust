use super::options::AssessDistributionOptions;
use super::project::{build_datafile, input, list_targets, project_path, unique_targets};
use crate::FeaturevisorOptions;
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use uuid::Uuid;

fn print_count(label: &str, count: u64, total: u64) {
    let percentage = (count as f64 / total as f64) * 100.0;
    println!("  • {label}: {count} {percentage:.2}%");
}

pub fn run(options: AssessDistributionOptions) -> Result<(), String> {
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
    let context_value: JsonValue =
        serde_json::from_str(&options.context).map_err(|error| error.to_string())?;
    let context = crate::helpers::json_to_context(&context_value)
        .ok_or_else(|| "Context must be a JSON object".to_string())?;
    for target in targets {
        let datafile = build_datafile(
            &project,
            options.common.environment.as_deref(),
            target.as_deref(),
            options.common.inflate,
        )?;
        let f = crate::create_featurevisor(FeaturevisorOptions {
            datafile: Some(input(datafile)),
            ..Default::default()
        });
        let iterations = options.n.max(1);
        let has_variations = f.has_variations(&options.feature);
        let mut flags = BTreeMap::<bool, u64>::new();
        let mut variations = BTreeMap::<String, u64>::new();
        for _ in 0..iterations {
            let mut iteration_context = context.clone();
            for key in &options.populate_uuid {
                iteration_context.insert(
                    key.clone(),
                    crate::AttributeValue::String(Uuid::new_v4().to_string()),
                );
            }
            let enabled = f.is_enabled(&options.feature, Some(&iteration_context));
            *flags.entry(enabled).or_default() += 1;
            if has_variations {
                if let Some(value) =
                    f.get_variation(&options.feature, Some(&iteration_context), None)
                {
                    *variations.entry(value).or_default() += 1;
                }
            }
        }
        println!();
        println!("Assess Featurevisor distribution");
        println!("  Feature: {}", options.feature);
        println!(
            "  Environment: {}",
            options.common.environment.as_deref().unwrap_or("false")
        );
        if let Some(target) = target {
            println!("  Target: {target}");
        }
        println!("  Iterations: {iterations}");
        println!("  Context: {}", options.context);
        println!();
        println!("Flag evaluations");
        println!();
        print_count(
            "disabled",
            flags.get(&false).copied().unwrap_or_default(),
            iterations,
        );
        print_count(
            "enabled",
            flags.get(&true).copied().unwrap_or_default(),
            iterations,
        );
        if has_variations {
            println!();
            println!("Variation evaluations");
            println!();
            for (value, count) in variations {
                print_count(&value, count, iterations);
            }
        }
    }
    Ok(())
}
