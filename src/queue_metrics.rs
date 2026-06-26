//! Low-overhead queue depth counters for worker command channels.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Atomic queue counters shared by one sender and one worker receiver.
#[derive(Debug, Default)]
pub(crate) struct QueueMetrics {
    current_depth: AtomicUsize,
    peak_depth: AtomicUsize,
    rejected_sends: AtomicUsize,
}

impl QueueMetrics {
    pub(crate) fn track<T>(self: &Arc<Self>, command: T) -> QueuedCommand<T> {
        self.increment_depth();
        QueuedCommand {
            command: Some(command),
            ticket: Some(QueueTicket {
                metrics: Arc::clone(self),
                received: false,
            }),
        }
    }

    pub(crate) fn record_rejected_send(&self) {
        self.rejected_sends.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn snapshot(&self) -> QueueMetricsSnapshot {
        QueueMetricsSnapshot {
            current_depth: self.current_depth.load(Ordering::Relaxed),
            peak_depth: self.peak_depth.load(Ordering::Relaxed),
            rejected_sends: self.rejected_sends.load(Ordering::Relaxed),
        }
    }

    fn increment_depth(&self) {
        let depth = self.current_depth.fetch_add(1, Ordering::Relaxed) + 1;
        self.update_peak(depth);
    }

    fn update_peak(&self, depth: usize) {
        let mut peak = self.peak_depth.load(Ordering::Relaxed);
        while depth > peak {
            match self.peak_depth.compare_exchange_weak(
                peak,
                depth,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(observed) => peak = observed,
            }
        }
    }

    fn decrement_depth(&self) {
        self.current_depth.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Snapshot of one worker command queue.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct QueueMetricsSnapshot {
    pub(crate) current_depth: usize,
    pub(crate) peak_depth: usize,
    pub(crate) rejected_sends: usize,
}

/// Command wrapper that owns one queue-depth ticket until the worker receives it.
#[derive(Debug)]
pub(crate) struct QueuedCommand<T> {
    command: Option<T>,
    ticket: Option<QueueTicket>,
}

impl<T> QueuedCommand<T> {
    pub(crate) fn into_received_command(mut self) -> T {
        if let Some(mut ticket) = self.ticket.take() {
            ticket.mark_received();
        }
        self.command.take().expect("queued command is present")
    }
}

impl<T> Drop for QueuedCommand<T> {
    fn drop(&mut self) {
        let _ = self.ticket.take();
    }
}

#[derive(Debug)]
struct QueueTicket {
    metrics: Arc<QueueMetrics>,
    received: bool,
}

impl QueueTicket {
    fn mark_received(&mut self) {
        if !self.received {
            self.metrics.decrement_depth();
            self.received = true;
        }
    }
}

impl Drop for QueueTicket {
    fn drop(&mut self) {
        if !self.received {
            self.metrics.decrement_depth();
        }
    }
}
