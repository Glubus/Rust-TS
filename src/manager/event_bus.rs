//! Internal event bus for manager-level subscriptions.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};

use crate::types::{VmEvent, VmSubscription};

pub(super) struct EventBus {
    next_subscriber_id: AtomicUsize,
    subscribers: Mutex<HashMap<usize, Subscriber>>,
    capacity: usize,
}

struct Subscriber {
    sender: SyncSender<VmEvent>,
    dropped: Arc<AtomicU64>,
}

impl EventBus {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            next_subscriber_id: AtomicUsize::new(1),
            subscribers: Mutex::new(HashMap::new()),
            capacity,
        }
    }

    pub(super) fn subscribe(&self) -> VmSubscription {
        let (tx, rx) = sync_channel(self.capacity);
        let dropped = Arc::new(AtomicU64::new(0));
        let subscriber_id = self.next_subscriber_id.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut subscribers) = self.subscribers.lock() {
            subscribers.insert(
                subscriber_id,
                Subscriber {
                    sender: tx,
                    dropped: dropped.clone(),
                },
            );
        }
        VmSubscription { rx, dropped }
    }

    pub(super) fn publish(&self, event: VmEvent) {
        let Ok(mut subscribers) = self.subscribers.lock() else {
            return;
        };
        subscribers.retain(
            |_, subscriber| match subscriber.sender.try_send(event.clone()) {
                Ok(()) => true,
                Err(TrySendError::Full(_)) => {
                    subscriber.dropped.fetch_add(1, Ordering::Relaxed);
                    true
                }
                Err(TrySendError::Disconnected(_)) => false,
            },
        );
    }
}
