use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "featurevisor", disable_help_flag = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Test(TestOptions),
    Benchmark(BenchmarkOptions),
    AssessDistribution(AssessDistributionOptions),
}

#[derive(Args, Debug, Clone)]
pub struct CommonOptions {
    #[arg(long = "projectDirectoryPath", default_value = ".")]
    pub project_directory_path: String,
    #[arg(long, hide = true)]
    pub with_scopes: bool,
    #[arg(long, hide = true)]
    pub with_tags: bool,
    #[arg(long = "schema-version", alias = "schemaVersion", hide = true)]
    pub schema_version: Option<String>,
    #[arg(long, action = clap::ArgAction::SetTrue)]
    pub quiet: bool,
    #[arg(long, action = clap::ArgAction::SetTrue)]
    pub verbose: bool,
    #[arg(long = "showDatafile", action = clap::ArgAction::SetTrue)]
    pub show_datafile: bool,
    #[arg(long, value_delimiter = ',')]
    pub target: Vec<String>,
    #[arg(long, default_value_t = 1)]
    pub inflate: u32,
    #[arg(long = "environment")]
    pub environment: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct TestOptions {
    #[command(flatten)]
    pub common: CommonOptions,
    #[arg(long = "keyPattern")]
    pub key_pattern: Option<String>,
    #[arg(long = "assertionPattern")]
    pub assertion_pattern: Option<String>,
    #[arg(long = "onlyFailures", action = clap::ArgAction::SetTrue)]
    pub only_failures: bool,
}

#[derive(Args, Debug, Clone)]
pub struct BenchmarkOptions {
    #[command(flatten)]
    pub common: CommonOptions,
    #[arg(long, required_unless_present = "variable")]
    pub feature: Option<String>,
    #[arg(long, action = clap::ArgAction::SetTrue, conflicts_with = "variable")]
    pub variation: bool,
    #[arg(long, conflicts_with = "variation")]
    pub variable: Option<String>,
    #[arg(long, default_value = "{}")]
    pub context: String,
    #[arg(long, default_value_t = 1000)]
    pub n: u64,
}

#[derive(Args, Debug, Clone)]
pub struct AssessDistributionOptions {
    #[command(flatten)]
    pub common: CommonOptions,
    #[arg(long)]
    pub feature: String,
    #[arg(long, default_value = "{}")]
    pub context: String,
    #[arg(long, default_value_t = 1000)]
    pub n: u64,
    #[arg(long = "populateUuid", action = clap::ArgAction::Append)]
    pub populate_uuid: Vec<String>,
}
