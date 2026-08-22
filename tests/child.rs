use featurevisor::{create_featurevisor, AttributeValue, DatafileInput, FeaturevisorOptions};
use serde_json::json;

#[test]
fn child_keeps_a_context_snapshot_and_inherits_new_parent_keys() {
    let datafile = serde_json::from_value(json!({ "schemaVersion": "2", "revision": "child", "segments": {}, "features": {} })).unwrap();
    let f = create_featurevisor(FeaturevisorOptions { context: Some([(String::from("userId"), AttributeValue::from("one"))].into_iter().collect()), datafile: Some(DatafileInput::Content(datafile)), ..Default::default() });
    let child = f.spawn(Default::default(), Default::default());
    f.set_context([(String::from("userId"), AttributeValue::from("two")), (String::from("country"), AttributeValue::from("nl"))].into_iter().collect(), false);
    assert_eq!(child.get_context(None).get("userId"), Some(&AttributeValue::from("one")));
    assert_eq!(child.get_context(None).get("country"), Some(&AttributeValue::from("nl")));
    child.close();
    child.close();
}
