//! Lightweight handle to one worker thread.

use std::sync::Arc;
use std::sync::mpsc::SyncSender;

use crate::latency_metrics::LatencyMetrics;
use crate::queue_metrics::{QueueMetrics, QueueMetricsSnapshot, QueuedCommand};
use crate::types::{VmLatencyStats, WorkerId};

use super::WorkerCommand;

#[derive(Debug)]
pub(crate) struct WorkerHandle {
    pub(crate) id: WorkerId,
    pub(crate) tx: SyncSender<QueuedCommand<WorkerCommand>>,
    pub(crate) queue_metrics: Arc<QueueMetrics>,
    pub(crate) latency_metrics: Arc<LatencyMetrics>,
}

impl WorkerHandle {
    pub(crate) fn queue_snapshot(&self) -> QueueMetricsSnapshot {
        self.queue_metrics.snapshot()
    }

    pub(crate) fn latency_snapshot(&self) -> VmLatencyStats {
        self.latency_metrics.snapshot()
    }
}
