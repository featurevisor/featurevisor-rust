use super::options::AssessDistributionOptions;
use super::project::{build_datafile, input, list_targets, project_path};
use crate::FeaturevisorOptions;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use uuid::Uuid;

pub fn run(options: AssessDistributionOptions) -> Result<(), String> {
    let project = project_path(&options.common.project_directory_path);
    let targets = if options.common.target.is_empty() {
        vec![None]
    } else {
        let available = list_targets(&project)?;
        for target in &options.common.target {
            if !available.contains(target) {
                return Err(format!("Unknown target \"{target}\""));
            }
        }
        options
            .common
            .target
            .iter()
            .map(|target| Some(target.as_str()))
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
            target,
            options.common.inflate,
        )?;
        let f = crate::create_featurevisor(FeaturevisorOptions {
            datafile: Some(input(datafile)),
            ..Default::default()
        });
        let mut flags = HashMap::<String, u64>::new();
        let mut variations = HashMap::<String, u64>::new();
        for _ in 0..options.n.max(1) {
            let mut iteration_context = context.clone();
            for key in &options.populate_uuid {
                iteration_context.insert(
                    key.clone(),
                    crate::AttributeValue::String(Uuid::new_v4().to_string()),
                );
            }
            let enabled = f.is_enabled(&options.feature, Some(&iteration_context));
            *flags.entry(enabled.to_string()).or_default() += 1;
            if let Some(value) = f.get_variation(&options.feature, Some(&iteration_context), None) {
                *variations.entry(value).or_default() += 1;
            }
        }
        println!(
            "Assess distribution{}",
            target
                .map(|target| format!(" target={target}"))
                .unwrap_or_default()
        );
        println!("  Flags: {flags:?}");
        if !variations.is_empty() {
            println!("  Variations: {variations:?}");
        }
    }
    Ok(())
}
