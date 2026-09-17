//! Async worker pool handles.

use std::sync::Arc;
use std::sync::mpsc::SyncSender;

use crate::error::VmError;
use crate::latency_metrics::LatencyMetrics;
use crate::queue_metrics::{QueueMetrics, QueueMetricsSnapshot, QueuedCommand};
use crate::types::VmLatencyStats;

use super::{AsyncWorkerCommand, send_command};

pub(super) struct AsyncWorkerHandle {
    pub(super) control: Arc<crate::runner::execution::ExecutionControl>,
    pub(super) tx: SyncSender<QueuedCommand<AsyncWorkerCommand>>,
    pub(super) queue_metrics: Arc<QueueMetrics>,
    pub(super) latency_metrics: Arc<LatencyMetrics>,
}

impl AsyncWorkerHandle {
    pub(super) fn send(&self, command: AsyncWorkerCommand) -> Result<(), VmError> {
        if self.control.is_stopping() {
            return Err(VmError::WorkerOffline);
        }
        send_command(&self.tx, &self.queue_metrics, command)
    }

    pub(super) fn queue_snapshot(&self) -> QueueMetricsSnapshot {
        self.queue_metrics.snapshot()
    }

    pub(super) fn latency_snapshot(&self) -> VmLatencyStats {
        self.latency_metrics.snapshot()
    }
}
