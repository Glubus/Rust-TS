//! Internal event bus for manager-level subscriptions.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::channel;

use crate::types::{VmEvent, VmSubscription};

pub(super) struct EventBus {
    next_subscriber_id: AtomicUsize,
    subscribers: Mutex<HashMap<usize, std::sync::mpsc::Sender<VmEvent>>>,
}

impl EventBus {
    pub(super) fn new() -> Self {
        Self {
            next_subscriber_id: AtomicUsize::new(1),
            subscribers: Mutex::new(HashMap::new()),
        }
    }

    pub(super) fn subscribe(&self) -> VmSubscription {
        let (tx, rx) = channel();
        let subscriber_id = self.next_subscriber_id.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut subscribers) = self.subscribers.lock() {
            subscribers.insert(subscriber_id, tx);
        }
        VmSubscription { rx }
    }

    pub(super) fn publish(&self, event: VmEvent) {
        let Ok(mut subscribers) = self.subscribers.lock() else {
            return;
        };
        subscribers.retain(|_, sender| sender.send(event.clone()).is_ok());
    }
}
