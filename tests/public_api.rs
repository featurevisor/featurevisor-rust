use featurevisor::{context, create_featurevisor, AttributeValue, DatafileInput, Featurevisor, FeaturevisorOptions};
use serde_json::json;

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn primary_public_api_is_usable_by_an_external_consumer() {
    assert_send_sync::<Featurevisor>();
    let datafile = serde_json::from_value(json!({
        "schemaVersion": "2",
        "revision": "public",
        "segments": {},
        "features": {}
    }))
    .unwrap();
    let context = context! {
        "userId" => "user-1",
        "count" => 2_i64,
        "enabled" => true,
    };
    let f = create_featurevisor(FeaturevisorOptions {
        datafile: Some(DatafileInput::Content(datafile)),
        context: Some(context),
        ..Default::default()
    });
    let _clone = f.clone();
    let _attribute = AttributeValue::from(vec!["one", "two"]);
    let _ = f.get_revision();
    let _ = f.get_schema_version();
    let _ = f.get_all_evaluations(None, &[], None);
}
