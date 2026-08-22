use featurevisor::{
    create_featurevisor, DatafileInput, EventDetails, EventName, FeaturevisorOptions,
};
use serde_json::json;
use std::sync::{Arc, Mutex};

fn empty_datafile(revision: &str) -> featurevisor::DatafileContent {
    serde_json::from_value(
        json!({ "schemaVersion": "2", "revision": revision, "segments": {}, "features": {} }),
    )
    .unwrap()
}

#[test]
fn events_are_emitted_in_the_documented_order_and_unsubscribe_stops_events() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&events);
    let f = create_featurevisor(FeaturevisorOptions::default());
    let unsubscribe = f.on(
        EventName::DatafileSet,
        Arc::new(move |_| observed.lock().unwrap().push("datafile")),
    );
    f.set_datafile(DatafileInput::Content(empty_datafile("one")), true);
    unsubscribe();
    f.set_datafile(DatafileInput::Content(empty_datafile("two")), true);
    assert_eq!(events.lock().unwrap().as_slice(), ["datafile"]);
}

#[test]
fn error_diagnostics_emit_error_event() {
    let errors = Arc::new(Mutex::new(0));
    let observed = Arc::clone(&errors);
    let f = create_featurevisor(FeaturevisorOptions::default());
    let _unsubscribe = f.on(
        EventName::Error,
        Arc::new(move |details| {
            if matches!(details, EventDetails::Error { .. }) {
                *observed.lock().unwrap() += 1;
            }
        }),
    );
    f.set_datafile(DatafileInput::Json("not-json".to_string()), false);
    assert_eq!(*errors.lock().unwrap(), 1);
}
