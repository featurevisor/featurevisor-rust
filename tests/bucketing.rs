use featurevisor::{
    create_featurevisor, AttributeValue, Context, DatafileInput, FeaturevisorOptions,
    MAX_BUCKETED_NUMBER,
};
use serde_json::json;

#[test]
fn bucket_values_include_the_configured_upper_bound() {
    assert_eq!(MAX_BUCKETED_NUMBER, 100_000);
    let datafile = serde_json::from_value(json!({
        "schemaVersion": "2",
        "revision": "bucket",
        "segments": {},
        "features": {
            "flag": {
                "bucketBy": "userId",
                "traffic": [{ "key": "all", "segments": "*", "percentage": 100000, "enabled": true }]
            }
        }
    }))
    .unwrap();
    let f = create_featurevisor(FeaturevisorOptions {
        datafile: Some(DatafileInput::Content(datafile)),
        ..Default::default()
    });
    let context: Context = [("userId".to_string(), AttributeValue::from("user-1"))]
        .into_iter()
        .collect();
    let evaluation = f.evaluate_flag("flag", Some(&context));
    assert!(evaluation.bucket_value.unwrap_or(0) <= MAX_BUCKETED_NUMBER);
    assert!(f.is_enabled("flag", Some(&context)));
}
