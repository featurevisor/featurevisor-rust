use featurevisor::{create_featurevisor, DatafileInput, Diagnostic, FeaturevisorModule, FeaturevisorOptions, ModuleApi, LogLevel};
use serde_json::json;
use std::sync::{Arc, Mutex};

struct DiagnosticModule {
    received: Arc<Mutex<Vec<String>>>,
}

impl FeaturevisorModule for DiagnosticModule {
    fn name(&self) -> Option<&str> {
        Some("diagnostics")
    }

    fn setup(&self, api: &ModuleApi) {
        let received = Arc::clone(&self.received);
        let _ = api.on_diagnostic(
            Arc::new(move |diagnostic| received.lock().unwrap().push(diagnostic.code.clone())),
            Some(LogLevel::Debug),
        );
        api.report_diagnostic(Diagnostic::new(LogLevel::Info, "module_ready", "ready"));
    }
}

#[test]
fn module_diagnostics_are_filtered_and_removed_with_the_module() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let f = create_featurevisor(FeaturevisorOptions {
        modules: vec![Arc::new(DiagnosticModule {
            received: Arc::clone(&received),
        })],
        ..Default::default()
    });
    assert!(received.lock().unwrap().is_empty());

    f.set_datafile(DatafileInput::Json("bad".to_string()), false);
    assert!(received.lock().unwrap().iter().any(|code| code == "invalid_datafile"));

    f.remove_module("diagnostics");
    let before = received.lock().unwrap().len();
    f.set_datafile(DatafileInput::Json("bad".to_string()), false);
    assert_eq!(received.lock().unwrap().len(), before);
}

#[test]
fn empty_details_are_serialized_as_an_object() {
    let diagnostic = featurevisor::Diagnostic::new(LogLevel::Info, "code", "message");
    let value = serde_json::to_value(diagnostic).unwrap();
    assert_eq!(value["details"], json!({}));
}
