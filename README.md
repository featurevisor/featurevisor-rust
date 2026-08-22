# Featurevisor Rust SDK <!-- omit in toc -->

This is a port of the Featurevisor [JavaScript SDK](https://featurevisor.com/docs/sdks/javascript/) v3.x to Rust. It evaluates feature flags, variations, and variables in Rust applications.

The SDK supports Featurevisor v3 projects and schema version 2 datafiles. The library is synchronous, thread safe, and compatible with Rust 1.74 or newer.

## Table of contents <!-- omit in toc -->

- [Installation](#installation)
- [Public API](#public-api)
- [Initialization](#initialization)
- [Evaluation types](#evaluation-types)
- [Context](#context)
  - [Setting initial context](#setting-initial-context)
  - [Setting after initialization](#setting-after-initialization)
  - [Replacing existing context](#replacing-existing-context)
  - [Manually passing context](#manually-passing-context)
- [Check if enabled](#check-if-enabled)
- [Getting variation](#getting-variation)
- [Getting variables](#getting-variables)
  - [Type specific methods](#type-specific-methods)
- [Getting all evaluations](#getting-all-evaluations)
- [Sticky](#sticky)
  - [Initialize with sticky](#initialize-with-sticky)
  - [Set sticky afterwards](#set-sticky-afterwards)
- [Setting datafile](#setting-datafile)
  - [Merging by default](#merging-by-default)
  - [Replacing](#replacing)
  - [Loading datafiles on demand](#loading-datafiles-on-demand)
  - [Updating datafile](#updating-datafile)
  - [Interval based update](#interval-based-update)
- [Diagnostics](#diagnostics)
  - [Levels](#levels)
  - [Handler](#handler)
- [Events](#events)
  - [`datafile_set`](#datafile_set)
  - [`context_set`](#context_set)
  - [`sticky_set`](#sticky_set)
  - [`error`](#error)
- [Evaluation details](#evaluation-details)
- [Modules](#modules)
  - [Defining a module](#defining-a-module)
  - [Registering modules](#registering-modules)
- [Child instance](#child-instance)
- [Close](#close)
- [CLI usage](#cli-usage)
  - [Test](#test)
  - [Benchmark](#benchmark)
  - [Assess distribution](#assess-distribution)
- [Development of this package](#development-of-this-package)
- [License](#license)

<!-- FEATUREVISOR_DOCS_BEGIN -->

## Installation

Add the SDK to `Cargo.toml`:

```toml
[dependencies]
featurevisor = "0.1"
```

The optional command line runner is available with the `cli` feature:

```toml
featurevisor = { version = "0.1", features = ["cli"] }
```

## Public API

The main API is `create_featurevisor`. Most applications need that function, the `Featurevisor` instance, `FeaturevisorOptions`, and the datafile types.

```rust
use featurevisor::{create_featurevisor, FeaturevisorOptions};

let f = create_featurevisor(FeaturevisorOptions::default());
```

`Featurevisor` is cheaply cloneable and can be shared between threads. Evaluation takes a snapshot of the instance state before invoking module, diagnostic, or event callbacks.

## Initialization

Initialize with a decoded datafile:

```rust
use featurevisor::{create_featurevisor, DatafileInput, FeaturevisorOptions};

let datafile = std::fs::read_to_string("datafile.json")?;
let f = create_featurevisor(FeaturevisorOptions {
    datafile: Some(DatafileInput::Json(datafile)),
    ..Default::default()
});
```

Initialization also accepts `DatafileInput::Content(datafile_content)`. Invalid datafiles are rejected without replacing the previous datafile and report a diagnostic with the message `Could not parse datafile`.

## Evaluation types

Featurevisor evaluates three kinds of values:

* a flag, which answers whether a feature is enabled
* a variation, which returns a variation value
* a variable, which returns remote configuration for a feature

All evaluations use a context and the feature rules in the active datafile.

## Context

Contexts are maps of attributes used by conditions and bucketing. `AttributeValue` supports strings, integers, doubles, booleans, dates, arrays, objects, and null.

```rust
use featurevisor::{context, AttributeValue, Context};

let context: Context = context! {
    "userId" => "123",
    "country" => "nl",
    "isEmployee" => false,
};
```

### Setting initial context

```rust
let f = featurevisor::create_featurevisor(featurevisor::FeaturevisorOptions {
    context: Some(context.clone()),
    ..Default::default()
});
```

### Setting after initialization

```rust
f.set_context(context.clone(), false);
```

The default is to merge the supplied attributes with the stored context.

### Replacing existing context

```rust
f.set_context(context.clone(), true);
```

### Manually passing context

```rust
let is_enabled = f.is_enabled("my_feature", Some(&context));
let variation = f.get_variation("my_feature", Some(&context), None);
let value = f.get_variable("my_feature", "my_variable", Some(&context), None);
```

Context passed for one evaluation wins over stored context for matching keys.

## Check if enabled

```rust
let is_enabled = f.is_enabled("my_feature", None);
if is_enabled {
    // show the enabled experience
}
```

## Getting variation

```rust
if let Some(variation) = f.get_variation("my_feature", None, None) {
    println!("variation: {variation}");
}
```

To provide a fallback when no variation is selected:

```rust
use featurevisor::OverrideOptions;

let variation = f.get_variation(
    "my_feature",
    None,
    Some(&OverrideOptions {
        default_variation_value: Some("control".to_string()),
        ..Default::default()
    }),
);
```

## Getting variables

```rust
let value = f.get_variable("my_feature", "backgroundColour", None, None);
```

### Type specific methods

The SDK also provides typed getters:

```rust
let enabled = f.get_variable_boolean("my_feature", "enabled", None, None);
let title = f.get_variable_string("my_feature", "title", None, None);
let count = f.get_variable_integer("my_feature", "count", None, None);
let ratio = f.get_variable_double("my_feature", "ratio", None, None);
let items = f.get_variable_array("my_feature", "items", None, None);
let object = f.get_variable_object("my_feature", "object", None, None);
let json = f.get_variable_json("my_feature", "json", None, None);
```

Typed getters do not coerce strings, booleans, or unrelated collections. Integer getters accept finite whole doubles because JavaScript has one number type.

## Getting all evaluations

```rust
let evaluations = f.get_all_evaluations(None, &[], None);
```

Pass a list of feature keys to limit the result. An empty list evaluates every feature in the datafile.

## Sticky

Sticky values keep selected evaluations stable for the lifetime of an instance or child instance.

### Initialize with sticky

```rust
use std::collections::HashMap;
use featurevisor::{EvaluatedFeature, FeaturevisorOptions};

let mut sticky = HashMap::new();
sticky.insert("my_feature".to_string(), EvaluatedFeature {
    enabled: true,
    variation: Some("treatment".to_string()),
    variables: None,
});

let f = featurevisor::create_featurevisor(FeaturevisorOptions {
    sticky: Some(sticky),
    ..Default::default()
});
```

### Set sticky afterwards

```rust
f.set_sticky(HashMap::new(), false);
f.set_sticky(HashMap::new(), true); // replace all sticky values
```

## Setting datafile

### Merging by default

`set_datafile` merges incoming features and segments into the stored datafile. Incoming entries replace entries with the same key.

```rust
f.set_datafile(featurevisor::DatafileInput::Json(datafile_json), false);
```

### Replacing

Pass `true` to replace the complete stored datafile:

```rust
f.set_datafile(featurevisor::DatafileInput::Json(datafile_json), true);
```

### Loading datafiles on demand

Fetch a datafile using the HTTP client used by your application, then pass its body as `DatafileInput::Json`.

### Updating datafile

Call `set_datafile` whenever your application receives a newer datafile. A `datafile_set` event contains the changed feature keys and revision information.

### Interval based update

The SDK does not start a background timer. Use your application's scheduler to fetch and set datafiles at the interval that suits your service.

## Diagnostics

Diagnostics are the only SDK observability API. The SDK does not expose a separate logger handler.

### Levels

The levels are `fatal`, `error`, `warn`, `info`, and `debug`. Set the threshold with `set_log_level` or `FeaturevisorOptions::log_level`.

### Handler

```rust
use std::sync::Arc;
use featurevisor::{create_featurevisor, Diagnostic, FeaturevisorOptions, LogLevel};

let f = create_featurevisor(FeaturevisorOptions {
    log_level: Some(LogLevel::Warn),
    on_diagnostic: Some(Arc::new(|diagnostic: &Diagnostic| {
        eprintln!("{}: {}", diagnostic.code, diagnostic.message);
    })),
    ..Default::default()
});
```

Regular expression conditions use Rust's `regex` crate and therefore support
Featurevisor's portable regular expression subset. Lookaround and
backreferences cannot be evaluated by this engine. They produce a
`condition_match_error` diagnostic instead of being evaluated, so lint the
project before shipping its datafiles.

Module diagnostics can be subscribed to through `ModuleApi`. Diagnostic details are always an object, and error diagnostics also emit the `error` event.

## Events

Register an event callback with `f.on(event_name, callback)`. The returned unsubscribe function is safe to call more than once.

### `datafile_set`

Emitted after a valid datafile is stored. Details include `revision`, `previousRevision`, `revisionChanged`, `features`, and `replaced`.

### `context_set`

Emitted after context is merged or replaced. Details include `context` and `replaced`.

### `sticky_set`

Emitted after sticky features are merged or replaced. Details include `features` and `replaced`.

### `error`

Emitted for error diagnostics. The details include the complete `diagnostic`.

## Evaluation details

Use the detailed methods when you need the reason and rule information:

```rust
let evaluation = f.evaluate_flag("my_feature", None);
println!("{evaluation:?}");

let variation_evaluation = f.evaluate_variation("my_feature", None, None);
let variable_evaluation = f.evaluate_variable("my_feature", "my_variable", None, None);
```

An evaluation includes the type, feature key, reason, and when applicable the bucket key, bucket value, rule, traffic, variation, variable, or diagnostic error.

## Modules

Modules extend evaluation and lifecycle behaviour without changing the public evaluation methods.

### Defining a module

```rust
use featurevisor::{ConfigureBucketValueOptions, FeaturevisorModule, ModuleApi};

struct AuditModule;

impl FeaturevisorModule for AuditModule {
    fn name(&self) -> Option<&str> { Some("audit") }

    fn setup(&self, api: &ModuleApi) {
        println!("datafile revision: {}", api.get_revision());
    }

    fn bucket_value(&self, options: ConfigureBucketValueOptions) -> u32 {
        options.bucket_value
    }

    fn close(&self) {
        println!("module closed");
    }
}
```

### Registering modules

```rust
use std::sync::Arc;

let f = featurevisor::create_featurevisor(featurevisor::FeaturevisorOptions {
    modules: vec![Arc::new(AuditModule)],
    ..Default::default()
});

let unsubscribe = f.add_module(Arc::new(AuditModule));
f.remove_module("audit");
drop(unsubscribe);
```

Modules run `before` callbacks in registration order, then bucket key and bucket value callbacks during bucketing, and finally `after` callbacks in registration order. Duplicate names are reported and ignored.

## Child instance

A child keeps its own context, sticky values, and listeners while evaluating through the parent's datafile and modules:

```rust
let child = f.spawn(context! { "userId" => "child-1" }, Default::default());
let enabled = child.is_enabled("my_feature", None);
child.set_context(context! { "country" => "nl" }, false);
child.close();
```

The child snapshots parent context keys available at creation. Parent keys added later are inherited, while child keys win. Close the child when it is no longer needed.

## Close

```rust
f.close();
```

Close is idempotent. It closes modules, clears diagnostic subscriptions, clears event listeners, and makes later state changes no ops.

## CLI usage

The optional CLI delegates project discovery and datafile generation to the Node.js Featurevisor CLI, then evaluates through this Rust SDK. Install Rust and use the `cli` feature to build it:

```bash
cargo install featurevisor --features cli
```

### Test

```bash
featurevisor test --projectDirectoryPath=../featurevisor/examples/example-1 --onlyFailures
```

`--keyPattern` filters test keys with a case insensitive regular expression.
`--assertionPattern` does the same for assertion descriptions. A successful run
prints totals for test specs and assertions. If filters select no specs, the
command fails instead of reporting a silent success.

### Benchmark

```bash
featurevisor benchmark --projectDirectoryPath=../featurevisor/examples/example-1 --feature=foo --n=1000000
```

Use either `--variation` or `--variable=<key>` when benchmarking. They cannot be
used together. The command reports minimum, average, maximum, and total
durations in fractional milliseconds for the individual SDK evaluations.

### Assess distribution

```bash
featurevisor assess-distribution --projectDirectoryPath=../featurevisor/examples/example-1 --feature=foo --n=100000
```

The output includes separate flag and variation sections with counts and
percentages. Pass `--target` more than once to assess multiple target
datafiles.

The legacy `--with-scopes`, `--with-tags`, `--schemaVersion`, and `--schema-version` options are accepted and ignored. Targets can be passed more than once.

<!-- FEATUREVISOR_DOCS_END -->

## Development of this package

Install Rust 1.74 or newer, then run:

```bash
make check
make test-example-1
```

The package uses `cargo fmt`, `cargo clippy`, and `cargo test`. `Cargo.lock` is committed so library and CLI dependency resolution stays reproducible.

To release, update the version in `Cargo.toml`, run the checks, merge the change, and tag the matching version. Publishing to crates.io is performed by `cargo publish` or the release workflow.

## License

MIT © [Fahad Heylaal](https://fahad19.com)
