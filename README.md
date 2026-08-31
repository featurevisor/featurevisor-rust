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
  - [`sticky_features_set` and `sticky_variables_set`](#sticky_features_set-and-sticky_variables_set)
  - [`error`](#error)
- [Evaluation details](#evaluation-details)
- [Modules](#modules)
  - [Defining a module](#defining-a-module)
  - [Registering modules](#registering-modules)
- [Child instance](#child-instance)
- [Close](#close)
- [OpenFeature](#openfeature)
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
featurevisor = "1"
```

The optional command line runner is available with the `cli` feature:

```toml
featurevisor = { version = "1", features = ["cli"] }
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
* a feature variable, which returns remote configuration owned by a feature
* a global variable, which returns remote configuration independently of a feature

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

Global variables use explicit method names because Rust does not overload functions:

```rust
let email = f.get_global_variable("supportEmail", None, None);
let evaluation = f.evaluate_global_variable("supportEmail", None, None);
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
let features = f.get_feature_evaluations(None, &[], None);
let variables = f.get_global_variable_evaluations(None, &[], None);
```

Pass a list of keys to limit either result. An empty list evaluates every entity of that kind in the datafile.

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
    sticky_features: Some(sticky),
    sticky_variables: Some(HashMap::from([
        ("supportEmail".to_string(), "sticky@example.com".into()),
    ])),
    ..Default::default()
});
```

### Set sticky afterwards

```rust
f.set_sticky_features(HashMap::new(), false);
f.set_sticky_variables(HashMap::new(), false);
f.set_sticky_features(HashMap::new(), true); // replace all sticky feature values
```

## Setting datafile

### Merging by default

`set_datafile` merges incoming features, global variables, and segments into the stored datafile. Incoming entries replace entries with the same key.

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

Call `set_datafile` whenever your application receives a newer datafile. A `datafile_set` event contains changed feature and global variable keys, including dependants affected by changed requirements or segments, plus revision information.

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

Emitted after a valid datafile is stored. Details include `revision`, `previousRevision`, `revisionChanged`, `features`, `variables`, and `replaced`.

### `context_set`

Emitted after context is merged or replaced. Details include `context` and `replaced`.

### `sticky_features_set` and `sticky_variables_set`

`sticky_features_set` is emitted after sticky features are merged or replaced. Details include `features` and `replaced`.

`sticky_variables_set` is emitted after sticky global variables are merged or replaced. Its details include `variables` and `replaced`.

### `error`

Emitted for error diagnostics. The details include the complete `diagnostic`.

## Evaluation details

Use the detailed methods when you need the reason and rule information:

```rust
let evaluation = f.evaluate_flag("my_feature", None);
println!("{evaluation:?}");

let variation_evaluation = f.evaluate_variation("my_feature", None, None);
let variable_evaluation = f.evaluate_variable("my_feature", "my_variable", None, None);
let global_evaluation = f.evaluate_global_variable("supportEmail", None, None);
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

For feature evaluations, all `before` callbacks run in registration order, followed by all `before_evaluation` callbacks. After evaluation and caller defaults, all `after_evaluation` callbacks run, followed by all `after` callbacks. Global variable evaluations use only `before_evaluation` and `after_evaluation`. Required feature checks run through the complete module pipeline, and transformed defaults are preserved. Bucket key and bucket value callbacks run during feature bucketing. Duplicate names are reported and ignored.

`before` and `after` remain available as deprecated feature-only compatibility callbacks. Use `before_evaluation` and `after_evaluation` for new modules so the same callbacks can handle feature and global variable evaluations.

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

## OpenFeature

The OpenFeature provider is published as a separate crate. Applications that
only use the Featurevisor SDK do not compile or link OpenFeature, Tokio, or the
provider code.

The official OpenFeature Rust SDK currently requires Rust 1.80.1 or newer. The
base Featurevisor crate continues to support Rust 1.74 or newer.

```toml
[dependencies]
featurevisor = "1.2"
featurevisor-openfeature = "1.2"
open-feature = "0.3"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Create a provider that owns its Featurevisor instance:

```rust
use featurevisor::{DatafileInput, FeaturevisorOptions};
use featurevisor_openfeature::{FeaturevisorProvider, FeaturevisorProviderOptions};
use open_feature::{EvaluationContext, OpenFeature};

async fn configure(datafile: String) -> Result<(), Box<dyn std::error::Error>> {
    let provider = FeaturevisorProvider::new(FeaturevisorProviderOptions {
        featurevisor_options: FeaturevisorOptions {
            datafile: Some(DatafileInput::Json(datafile)),
            ..Default::default()
        },
        ..Default::default()
    })?;

    let mut api = OpenFeature::singleton_mut().await;
    api.set_provider(provider).await;
    let client = api.create_client();
    drop(api);

    let enabled = client
        .get_bool_value(
            "checkout",
            Some(&EvaluationContext::default().with_targeting_key("user-123")),
            None,
        )
        .await?;

    println!("Checkout enabled: {enabled}");
    Ok(())
}
```

You can also pass an existing Featurevisor instance. The provider borrows it
and does not close it:

```rust
use featurevisor_openfeature::FeaturevisorProvider;

let provider = FeaturevisorProvider::from_featurevisor(f.clone())?;
```

OpenFeature uses one flag key while Featurevisor supports flags, variations,
feature variables, and global variables:

| OpenFeature key | Featurevisor evaluation |
| --- | --- |
| `checkout` | Flag for feature `checkout` |
| `checkout:variation` | Variation for feature `checkout` |
| `checkout:title` | Variable `title` inside feature `checkout` |
| `variable:supportEmail` | Global variable `supportEmail` |

`targeting_key_field`, `key_separator`, `variation_key`, and
`global_variable_prefix` customize this mapping. The targeting key maps to
`userId` by default. The global variable prefix defaults to `variable` and
cannot contain the separator.

The provider implements boolean, integer, float, string, and structure
resolution. The OpenFeature Rust SDK represents top level object values with
`StructValue`. Arrays can be nested in those objects, but its provider contract
does not expose a separate top level array resolver.

Featurevisor evaluation reasons, variation values, revision, schema version,
rule keys, bucket information, and override information are mapped to
OpenFeature resolution details and flag metadata. Missing definitions, type
mismatches, invalid contexts, and invalid datafiles use standard OpenFeature
errors. Replacing an invalid datafile with a valid one recovers the provider.

The OpenFeature Rust SDK does not currently expose provider tracking or
provider event callbacks. Featurevisor modules and diagnostics continue to
work inside the Featurevisor instance.

See the [OpenFeature provider guide](https://featurevisor.com/docs/sdks/openfeature/) for the shared key convention and providers for other languages.

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
used together. Pass `--variable=<key>` without `--feature` to benchmark a global variable. The command reports minimum, average, maximum, and total
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

The base SDK supports Rust 1.74 or newer. The complete workspace requires Rust
1.80.1 or newer because it includes the OpenFeature provider. With a current
stable toolchain, run:

```bash
make check
make test-example-1
```

The package uses `cargo fmt`, `cargo clippy`, and `cargo test`. `Cargo.lock` is committed so library, CLI, and provider dependency resolution stays reproducible.

The repository publishes two crates with the same version:

- `featurevisor`
- `featurevisor-openfeature`

To release, update both versions, run the checks, merge the change, and tag the matching version. The release workflow publishes the base crate first and the provider second so the provider's exact Featurevisor dependency is available on crates.io.

Cargo can only package the provider after that exact base crate version is
visible on crates.io. Pull request checks therefore build, test, lint, and
document the provider. The tagged release performs the final provider package
inspection after publishing the base crate.

## License

MIT © [Fahad Heylaal](https://fahad19.com)
