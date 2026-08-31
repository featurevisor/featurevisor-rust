# Featurevisor OpenFeature provider for Rust

This crate adapts the Featurevisor Rust SDK to the official OpenFeature Rust SDK.

## Installation

```toml
[dependencies]
featurevisor = "1.2"
featurevisor-openfeature = "1.2"
open-feature = "0.3"
```

The provider is a separate crate, so applications that only use `featurevisor`
do not compile or link OpenFeature.

## Usage

Create a provider that owns its Featurevisor instance:

```rust
use featurevisor::{DatafileInput, FeaturevisorOptions};
use featurevisor_openfeature::{FeaturevisorProvider, FeaturevisorProviderOptions};

let provider = FeaturevisorProvider::new(FeaturevisorProviderOptions {
    featurevisor_options: FeaturevisorOptions {
        datafile: Some(DatafileInput::Json(datafile)),
        ..Default::default()
    },
    ..Default::default()
})?;
```

You can also pass an existing Featurevisor instance. The provider borrows it
and does not close it:

```rust
let provider = FeaturevisorProvider::from_featurevisor(f)?;
```

Featurevisor supports several evaluation types through one OpenFeature key:

| OpenFeature key | Featurevisor evaluation |
| --- | --- |
| `checkout` | Flag for feature `checkout` |
| `checkout:variation` | Variation for feature `checkout` |
| `checkout:title` | Variable `title` inside feature `checkout` |
| `variable:supportEmail` | Global variable `supportEmail` |

`targeting_key_field`, `key_separator`, `variation_key`, and
`global_variable_prefix` customize this mapping. The targeting key maps to
`userId` by default.

The provider implements boolean, integer, float, string, and structure
resolution. OpenFeature represents top level object values with `StructValue`.
Arrays can be nested in an object, but the provider contract does not expose a
separate top level array resolver.

Featurevisor reasons and evaluation metadata are mapped to OpenFeature
resolution details. Missing definitions, type mismatches, invalid contexts,
and invalid datafiles use standard OpenFeature errors. Replacing an invalid
datafile with a valid one recovers the provider.

Calling `close` releases provider subscriptions. It also closes a Featurevisor
instance created by the provider, but never closes a borrowed instance.

The current OpenFeature Rust SDK does not expose provider tracking or provider
event callbacks. Featurevisor modules and diagnostics continue to run inside
the Featurevisor instance.

See the [Featurevisor Rust SDK documentation](https://featurevisor.com/docs/sdks/rust/#openfeature)
and the [shared OpenFeature provider guide](https://featurevisor.com/docs/sdks/openfeature/)
for more details.
