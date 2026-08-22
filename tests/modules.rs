use featurevisor::{
    create_featurevisor, ConfigureBucketKeyOptions, ConfigureBucketValueOptions, DatafileInput,
    Diagnostic, EvaluateOptions, Evaluation, FeaturevisorModule, FeaturevisorOptions, ModuleApi,
};
use serde_json::json;
use std::sync::{Arc, Mutex};

struct TestModule {
    events: Arc<Mutex<Vec<String>>>,
}

struct FailingCloseModule;

impl FeaturevisorModule for FailingCloseModule {
    fn name(&self) -> Option<&str> {
        Some("failing-close")
    }

    fn close(&self) {
        panic!("close failed");
    }
}
impl FeaturevisorModule for TestModule {
    fn name(&self) -> Option<&str> {
        Some("test")
    }
    fn setup(&self, api: &ModuleApi) {
        self.events
            .lock()
            .unwrap()
            .push(format!("setup:{}", api.get_revision()));
    }
    fn before(&self, mut options: EvaluateOptions) -> EvaluateOptions {
        options.context.insert(
            "module".to_string(),
            featurevisor::AttributeValue::from("yes"),
        );
        options
    }
    fn bucket_key(&self, options: ConfigureBucketKeyOptions) -> String {
        format!("{}.module", options.bucket_key)
    }
    fn bucket_value(&self, options: ConfigureBucketValueOptions) -> u32 {
        options.bucket_value
    }
    fn after(&self, evaluation: Evaluation, _options: &EvaluateOptions) -> Evaluation {
        self.events.lock().unwrap().push("after".to_string());
        evaluation
    }
    fn close(&self) {
        self.events.lock().unwrap().push("close".to_string());
    }
}

fn datafile() -> featurevisor::DatafileContent {
    serde_json::from_value(json!({ "schemaVersion": "2", "revision": "ready", "segments": {}, "features": { "flag": { "bucketBy": "userId", "traffic": [{ "key": "rule", "segments": "*", "percentage": 100000, "enabled": true }] } } })).unwrap()
}

#[test]
fn module_lifecycle_and_duplicate_names_are_handled() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let module = Arc::new(TestModule {
        events: Arc::clone(&events),
    });
    let f = create_featurevisor(FeaturevisorOptions {
        modules: vec![module],
        datafile: Some(DatafileInput::Content(datafile())),
        ..Default::default()
    });
    assert_eq!(events.lock().unwrap()[0], "setup:unknown");
    assert!(f.is_enabled("flag", None));
    let duplicate = f.add_module(Arc::new(TestModule {
        events: Arc::clone(&events),
    }));
    assert!(duplicate.is_none());
    f.close();
    assert!(events.lock().unwrap().iter().any(|value| value == "close"));
    f.close();
}

#[test]
fn diagnostics_and_error_events_are_available() {
    let diagnostics = Arc::new(Mutex::new(Vec::<Diagnostic>::new()));
    let observed = Arc::clone(&diagnostics);
    let f = create_featurevisor(FeaturevisorOptions {
        on_diagnostic: Some(Arc::new(move |diagnostic| {
            observed.lock().unwrap().push(diagnostic.clone())
        })),
        ..Default::default()
    });
    f.set_datafile(DatafileInput::Json("bad".to_string()), false);
    assert!(diagnostics
        .lock()
        .unwrap()
        .iter()
        .any(|diagnostic| diagnostic.code == "invalid_datafile"));
}

#[test]
fn module_close_failures_include_module_metadata() {
    let diagnostics = Arc::new(Mutex::new(Vec::<Diagnostic>::new()));
    let observed = Arc::clone(&diagnostics);
    let f = create_featurevisor(FeaturevisorOptions {
        on_diagnostic: Some(Arc::new(move |diagnostic| {
            observed.lock().unwrap().push(diagnostic.clone())
        })),
        modules: vec![Arc::new(FailingCloseModule)],
        ..Default::default()
    });

    f.close();

    let diagnostics = diagnostics.lock().unwrap();
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "module_close_error")
        .expect("module close diagnostic");
    assert_eq!(diagnostic.module_name.as_deref(), Some("failing-close"));
    assert!(diagnostic.original_error.is_some());
}
