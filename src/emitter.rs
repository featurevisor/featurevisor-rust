use crate::events::{EventDetails, EventHandler, EventName};
use crate::Unsubscribe;
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

struct Listener {
    id: u64,
    callback: EventHandler,
}

#[derive(Clone, Default)]
pub(crate) struct Emitter {
    listeners: Arc<Mutex<HashMap<EventName, Vec<Listener>>>>,
    next_id: Arc<AtomicU64>,
}

impl Emitter {
    pub fn on(&self, event: EventName, callback: EventHandler) -> Unsubscribe {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.listeners.lock().unwrap().entry(event).or_default().push(Listener {
            id,
            callback,
        });
        let listeners = Arc::clone(&self.listeners);
        let active = Arc::new(AtomicBool::new(true));
        let active_for_closure = Arc::clone(&active);
        Box::new(move || {
            if !active_for_closure.swap(false, Ordering::AcqRel) {
                return;
            }
            if let Ok(mut listeners) = listeners.lock() {
                for values in listeners.values_mut() {
                    values.retain(|listener| listener.id != id);
                }
            }
        })
    }

    pub fn emit(&self, event: EventName, details: EventDetails) {
        let callbacks: Vec<EventHandler> = self
            .listeners
            .lock()
            .ok()
            .and_then(|listeners| listeners.get(&event).map(|values| {
                values.iter().map(|listener| Arc::clone(&listener.callback)).collect()
            }))
            .unwrap_or_default();
        for callback in callbacks {
            let _ = catch_unwind(AssertUnwindSafe(|| callback(&details)));
        }
    }

    pub fn clear(&self) {
        if let Ok(mut listeners) = self.listeners.lock() {
            listeners.clear();
        }
    }
}
