use featurevisor::{
    create_featurevisor, DatafileInput, EventDetails, EventName, FeaturevisorOptions,
};
use serde_json::json;
use std::sync::{Arc, Mutex};

fn empty_datafile() -> featurevisor::DatafileContent {
    serde_json::from_value(json!({
        "schemaVersion": "2",
        "revision": "one",
        "featurevisorVersion": "3.4.0",
        "segments": {},
        "features": {}
    }))
    .unwrap()
}

#[test]
fn featurevisor_version_is_preserved_and_invalid_datafiles_do_not_emit_datafile_events() {
    let f = create_featurevisor(FeaturevisorOptions {
        datafile: Some(DatafileInput::Content(empty_datafile())),
        ..Default::default()
    });
    assert_eq!(f.get_feature("missing"), None);

    let events = Arc::new(Mutex::new(0));
    let observed = Arc::clone(&events);
    let _unsubscribe = f.on(
        EventName::DatafileSet,
        Arc::new(move |details| {
            if matches!(details, EventDetails::DatafileSet(_)) {
                *observed.lock().unwrap() += 1;
            }
        }),
    );
    f.set_datafile(DatafileInput::Json("{}".to_string()), false);
    assert_eq!(*events.lock().unwrap(), 0);
    assert_eq!(f.get_revision(), "one");
}
