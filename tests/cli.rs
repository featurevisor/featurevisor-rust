#![cfg(feature = "cli")]

use clap::Parser;
use featurevisor::cli::{Cli, Command};

#[test]
fn legacy_flags_are_accepted_and_targets_are_repeatable() {
    let cli = Cli::try_parse_from([
        "featurevisor",
        "test",
        "--with-scopes",
        "--with-tags",
        "--schemaVersion",
        "1",
        "--target",
        "web",
        "--target",
        "mobile",
    ])
    .unwrap();
    match cli.command.unwrap() {
        Command::Test(options) => {
            assert!(options.common.with_scopes);
            assert!(options.common.with_tags);
            assert_eq!(options.common.schema_version.as_deref(), Some("1"));
            assert_eq!(
                options.common.target,
                vec!["web".to_string(), "mobile".to_string()]
            );
        }
        _ => panic!("expected test command"),
    }
}

#[test]
fn benchmark_rejects_variation_and_variable_together() {
    let result = Cli::try_parse_from([
        "featurevisor",
        "benchmark",
        "--feature",
        "checkout",
        "--variation",
        "--variable",
        "title",
    ]);
    assert!(result.is_err());
}
