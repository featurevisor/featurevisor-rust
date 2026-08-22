use clap::Parser;
use featurevisor::cli::{run, Cli};

fn main() {
    let cli = Cli::try_parse().unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(2);
    });
    if let Err(error) = run(cli) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
