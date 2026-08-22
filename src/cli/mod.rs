mod assess_distribution;
mod benchmark;
mod options;
mod project;
mod test;

pub use options::{
    AssessDistributionOptions, BenchmarkOptions, Cli, Command, CommonOptions, TestOptions,
};

pub fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Some(options::Command::Test(options)) => test::run(options),
        Some(options::Command::Benchmark(options)) => benchmark::run(options),
        Some(options::Command::AssessDistribution(options)) => assess_distribution::run(options),
        None => {
            println!("Learn more at https://featurevisor.com/docs/sdks/rust/");
            Ok(())
        }
    }
}
