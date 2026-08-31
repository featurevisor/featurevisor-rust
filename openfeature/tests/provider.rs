use featurevisor::{
    create_featurevisor, DatafileInput, FeaturevisorModule, FeaturevisorOptions, LogLevel,
};
use featurevisor_openfeature::{FeaturevisorProvider, FeaturevisorProviderOptions};
use open_feature::{
    provider::FeatureProvider, EvaluationContext, EvaluationErrorCode, EvaluationReason,
    FlagMetadataValue, OpenFeature,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const DATAFILE: &str = r#"{
  "schemaVersion":"2",
  "revision":"openfeature-test",
  "segments":{},
  "features":{
    "checkout":{
      "bucketBy":"userId",
      "variations":[{"value":"on","variables":{"title":"Hello","count":3,"ratio":1.5,"visible":true,"config":{"colour":"blue"},"json":"{\"nested\":true}","invalidJson":"not-json"}}],
      "variablesSchema":{
        "title":{"type":"string","defaultValue":"Default"},
        "count":{"type":"integer","defaultValue":0},
        "ratio":{"type":"double","defaultValue":0},
        "visible":{"type":"boolean","defaultValue":false},
        "config":{"type":"object","defaultValue":{}},
        "json":{"type":"json","defaultValue":"{}"},
        "invalidJson":{"type":"json","defaultValue":"{}"}
      },
      "force":[
        {"conditions":{"attribute":"userId","operator":"equals","value":"forced-user"},"enabled":true,"variation":"on"},
        {"conditions":{"attribute":"userId","operator":"equals","value":""},"enabled":true,"variation":"on"}
      ],
      "traffic":[{"key":"all","segments":"*","percentage":100000,"variation":"on"}]
    },
    "empty":{"bucketBy":"userId","variations":[],"traffic":[{"key":"all","segments":"*","percentage":100000,"allocation":[]}]},
    "disabled":{
      "bucketBy":"userId",
      "disabledVariationValue":"off",
      "variations":[{"value":"on"}],
      "force":[{"conditions":{"attribute":"blocked","operator":"equals","value":true},"enabled":false}],
      "traffic":[{"key":"all","segments":"*","percentage":100000,"variation":"on"}]
    },
    "allocated":{
      "bucketBy":"userId",
      "variations":[{"value":"on"}],
      "traffic":[{"key":"all","segments":"*","percentage":100000,"allocation":[{"variation":"on","range":[0,100000]}]}]
    }
  },
  "variables":{
    "supportEmail":{"type":"string","defaultValue":"support@example.com","overrides":[{"key":"nl","conditions":{"attribute":"country","operator":"equals","value":"nl"},"value":"nl@example.com"}]},
    "settings":{"type":"object","defaultValue":{"enabled":true,"limits":[1,2]}},
    "globalJson":{"type":"json","defaultValue":"{\"source\":\"global\"}"}
  }
}"#;

fn provider() -> FeaturevisorProvider {
    FeaturevisorProvider::new(FeaturevisorProviderOptions {
        featurevisor_options: FeaturevisorOptions {
            datafile: Some(DatafileInput::Json(DATAFILE.to_string())),
            log_level: Some(LogLevel::Fatal),
            ..Default::default()
        },
        ..Default::default()
    })
    .expect("valid provider")
}

#[tokio::test]
async fn resolves_flags_variations_variables_and_global_variables() {
    let provider = provider();
    let context = EvaluationContext::default().with_targeting_key("forced-user");

    let flag = provider
        .resolve_bool_value("checkout", &context)
        .await
        .expect("flag");
    assert!(flag.value);
    assert_eq!(flag.reason, Some(EvaluationReason::TargetingMatch));

    let variation = provider
        .resolve_string_value("checkout:variation", &context)
        .await
        .expect("variation");
    assert_eq!(variation.value, "on");
    assert_eq!(variation.variant.as_deref(), Some("on"));

    assert_eq!(
        provider
            .resolve_string_value("checkout:title", &context)
            .await
            .expect("string")
            .value,
        "Hello"
    );
    assert_eq!(
        provider
            .resolve_int_value("checkout:count", &context)
            .await
            .expect("integer")
            .value,
        3
    );
    assert_eq!(
        provider
            .resolve_float_value("checkout:ratio", &context)
            .await
            .expect("float")
            .value,
        1.5
    );
    assert!(
        provider
            .resolve_bool_value("checkout:visible", &context)
            .await
            .expect("boolean variable")
            .value
    );

    let object = provider
        .resolve_struct_value("checkout:config", &context)
        .await
        .expect("object");
    assert_eq!(
        object.value.fields.get("colour"),
        Some(&open_feature::Value::String("blue".to_string()))
    );
    let json = provider
        .resolve_struct_value("checkout:json", &context)
        .await
        .expect("json object");
    assert_eq!(
        json.value.fields.get("nested"),
        Some(&open_feature::Value::Bool(true))
    );

    assert_eq!(
        provider
            .resolve_string_value("variable:supportEmail", &context)
            .await
            .expect("global string")
            .value,
        "support@example.com"
    );
    assert!(provider
        .resolve_struct_value("variable:settings", &context)
        .await
        .expect("global object")
        .value
        .fields
        .contains_key("limits"));
    assert_eq!(
        provider
            .resolve_struct_value("variable:globalJson", &context)
            .await
            .expect("global json")
            .value
            .fields
            .get("source"),
        Some(&open_feature::Value::String("global".to_string()))
    );
}

#[tokio::test]
async fn maps_context_targeting_key_and_custom_configuration() {
    let provider = FeaturevisorProvider::new(FeaturevisorProviderOptions {
        featurevisor_options: FeaturevisorOptions {
            datafile: Some(DatafileInput::Json(DATAFILE.to_string())),
            log_level: Some(LogLevel::Fatal),
            ..Default::default()
        },
        targeting_key_field: "accountId".to_string(),
        key_separator: "/".to_string(),
        variation_key: "$variation".to_string(),
        global_variable_prefix: "$variable".to_string(),
        ..Default::default()
    })
    .expect("valid provider");
    let context = EvaluationContext::default()
        .with_targeting_key("forced-user")
        .with_custom_field("country", "nl");

    assert_eq!(
        provider
            .resolve_string_value("checkout/$variation", &context)
            .await
            .expect("custom variation")
            .value,
        "on"
    );
    assert_eq!(
        provider
            .resolve_string_value("$variable/supportEmail", &context)
            .await
            .expect("custom global")
            .value,
        "nl@example.com"
    );

    let empty = provider
        .resolve_bool_value(
            "checkout",
            &EvaluationContext::default().with_targeting_key(""),
        )
        .await
        .expect("empty targeting key remains valid");
    assert!(empty.value);
}

#[tokio::test]
async fn reports_standard_errors_and_type_mismatches() {
    let provider = provider();
    let context = EvaluationContext::default();

    let missing = provider
        .resolve_bool_value("missing", &context)
        .await
        .expect_err("missing feature");
    assert_eq!(missing.code, EvaluationErrorCode::FlagNotFound);
    assert_eq!(
        missing.message.as_deref(),
        Some("Feature \"missing\" was not found")
    );

    assert_eq!(
        provider
            .resolve_string_value("checkout", &context)
            .await
            .expect_err("flag is not string")
            .code,
        EvaluationErrorCode::TypeMismatch
    );
    assert_eq!(
        provider
            .resolve_bool_value("checkout:title", &context)
            .await
            .expect_err("string is not boolean")
            .code,
        EvaluationErrorCode::TypeMismatch
    );
    assert_eq!(
        provider
            .resolve_int_value("checkout:ratio", &context)
            .await
            .expect_err("double is not integer")
            .code,
        EvaluationErrorCode::TypeMismatch
    );
    assert_eq!(
        provider
            .resolve_struct_value("checkout:invalidJson", &context)
            .await
            .expect_err("invalid json is not structure")
            .code,
        EvaluationErrorCode::TypeMismatch
    );
    assert_eq!(
        provider
            .resolve_string_value("empty:variation", &context)
            .await
            .expect_err("no variations")
            .code,
        EvaluationErrorCode::FlagNotFound
    );
    assert_eq!(
        provider
            .resolve_string_value("checkout:missing", &context)
            .await
            .expect_err("missing variable")
            .code,
        EvaluationErrorCode::FlagNotFound
    );
    let empty_global = provider
        .resolve_string_value("variable:", &context)
        .await
        .expect_err("empty global variable key");
    assert_eq!(empty_global.code, EvaluationErrorCode::FlagNotFound);
    assert_eq!(
        empty_global.message.as_deref(),
        Some("Global variable \"\" was not found")
    );
}

#[tokio::test]
async fn maps_targeting_split_and_disabled_reasons() {
    let provider = provider();

    let allocated = provider
        .resolve_string_value(
            "allocated:variation",
            &EvaluationContext::default().with_targeting_key("allocated-user"),
        )
        .await
        .expect("allocated variation");
    assert_eq!(allocated.value, "on");
    assert_eq!(allocated.reason, Some(EvaluationReason::Split));

    let overridden = provider
        .resolve_string_value(
            "variable:supportEmail",
            &EvaluationContext::default().with_custom_field("country", "nl"),
        )
        .await
        .expect("overridden global variable");
    assert_eq!(overridden.value, "nl@example.com");
    assert_eq!(overridden.reason, Some(EvaluationReason::TargetingMatch));

    let blocked_context = EvaluationContext::default().with_custom_field("blocked", true);
    let disabled = provider
        .resolve_bool_value("disabled", &blocked_context)
        .await
        .expect("forced disabled flag");
    assert!(!disabled.value);
    assert_eq!(disabled.reason, Some(EvaluationReason::TargetingMatch));

    let disabled_variation = provider
        .resolve_string_value("disabled:variation", &blocked_context)
        .await
        .expect("disabled variation");
    assert_eq!(disabled_variation.value, "off");
    assert_eq!(disabled_variation.reason, Some(EvaluationReason::Disabled));
}

#[tokio::test]
async fn exposes_metadata_and_works_through_openfeature_client() {
    let provider = provider();
    assert_eq!(provider.metadata().name, "Featurevisor");

    let mut api = OpenFeature::default();
    api.set_provider(provider).await;
    let client = api.create_client();
    let context = EvaluationContext::default().with_targeting_key("forced-user");
    let details = client
        .get_bool_details("checkout", Some(&context), None)
        .await
        .expect("client evaluation");
    assert!(details.value);
    assert_eq!(details.reason, Some(EvaluationReason::TargetingMatch));
    assert_eq!(
        details.flag_metadata.values.get("revision"),
        Some(&FlagMetadataValue::String("openfeature-test".to_string()))
    );
    assert_eq!(
        details.flag_metadata.values.get("featurevisorReason"),
        Some(&FlagMetadataValue::String("forced".to_string()))
    );
    api.shutdown().await;
}

#[tokio::test]
async fn reports_parse_errors_and_recovers_after_valid_datafile() {
    let provider = FeaturevisorProvider::new(FeaturevisorProviderOptions {
        featurevisor_options: FeaturevisorOptions {
            datafile: Some(DatafileInput::Json("{".to_string())),
            log_level: Some(LogLevel::Fatal),
            ..Default::default()
        },
        ..Default::default()
    })
    .expect("provider configuration");

    assert_eq!(
        provider
            .resolve_bool_value("checkout", &EvaluationContext::default())
            .await
            .expect_err("parse error")
            .code,
        EvaluationErrorCode::ParseError
    );
    provider
        .featurevisor()
        .set_datafile(DatafileInput::Json(DATAFILE.to_string()), true);
    assert!(
        provider
            .resolve_bool_value(
                "checkout",
                &EvaluationContext::default().with_targeting_key("forced-user")
            )
            .await
            .expect("recovered")
            .value
    );

    provider
        .featurevisor()
        .set_datafile(DatafileInput::Json("{".to_string()), true);
    assert_eq!(
        provider
            .resolve_bool_value("checkout", &EvaluationContext::default())
            .await
            .expect_err("later parse error")
            .code,
        EvaluationErrorCode::ParseError
    );
}

struct CloseModule(Arc<AtomicUsize>);

impl FeaturevisorModule for CloseModule {
    fn close(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn closes_owned_instances_but_not_borrowed_instances() {
    let owned_closed = Arc::new(AtomicUsize::new(0));
    {
        let provider = FeaturevisorProvider::new(FeaturevisorProviderOptions {
            featurevisor_options: FeaturevisorOptions {
                modules: vec![Arc::new(CloseModule(Arc::clone(&owned_closed)))],
                ..Default::default()
            },
            ..Default::default()
        })
        .expect("owned provider");
        provider.close();
        provider.close();
    }
    assert_eq!(owned_closed.load(Ordering::SeqCst), 1);

    let borrowed_closed = Arc::new(AtomicUsize::new(0));
    let featurevisor = create_featurevisor(FeaturevisorOptions {
        datafile: Some(DatafileInput::Json(DATAFILE.to_string())),
        modules: vec![Arc::new(CloseModule(Arc::clone(&borrowed_closed)))],
        ..Default::default()
    });
    {
        let provider = FeaturevisorProvider::from_featurevisor(featurevisor.clone())
            .expect("borrowed provider");
        provider.close();
    }
    assert_eq!(borrowed_closed.load(Ordering::SeqCst), 0);
    assert!(featurevisor.is_enabled("checkout", None));
    featurevisor.close();
    assert_eq!(borrowed_closed.load(Ordering::SeqCst), 1);
}

#[test]
fn rejects_ambiguous_or_empty_key_grammar() {
    let error = FeaturevisorProvider::new(FeaturevisorProviderOptions {
        global_variable_prefix: "global:variable".to_string(),
        ..Default::default()
    })
    .err()
    .expect("invalid prefix");
    assert_eq!(
        error.to_string(),
        "globalVariablePrefix cannot contain keySeparator"
    );

    assert_eq!(
        FeaturevisorProvider::new(FeaturevisorProviderOptions {
            key_separator: String::new(),
            ..Default::default()
        })
        .err()
        .expect("empty separator")
        .to_string(),
        "keySeparator cannot be empty"
    );
}
