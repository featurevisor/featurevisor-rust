use featurevisor::{create_featurevisor, AttributeValue, DatafileInput, FeaturevisorOptions};
use serde_json::json;
use std::sync::Arc;
use std::thread;

#[test]
fn one_instance_can_evaluate_concurrently() {
    let datafile = serde_json::from_value(json!({
        "schemaVersion": "2",
        "revision": "concurrent",
        "segments": {},
        "features": {
            "flag": {
                "bucketBy": "userId",
                "traffic": [{ "key": "all", "segments": "*", "percentage": 100000, "enabled": true }]
            }
        }
    }))
    .unwrap();
    let f = Arc::new(create_featurevisor(FeaturevisorOptions {
        datafile: Some(DatafileInput::Content(datafile)),
        ..Default::default()
    }));
    let handles = (0..8)
        .map(|index| {
            let f = Arc::clone(&f);
            thread::spawn(move || {
                let context = [("userId".to_string(), AttributeValue::from(index as i64))]
                    .into_iter()
                    .collect();
                for _ in 0..100 {
                    assert!(f.is_enabled("flag", Some(&context)));
                }
            })
        })
        .collect::<Vec<_>>();
    for handle in handles {
        handle.join().unwrap();
    }
}
