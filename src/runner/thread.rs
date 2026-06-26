//! Worker thread lifecycle.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, sync_channel};
use std::thread::JoinHandle;

use crate::config::VmOptions;
use crate::error::VmError;
use crate::latency_metrics::LatencyMetrics;
use crate::queue_metrics::{QueueMetrics, QueuedCommand};
use crate::registry::InMemoryHostContractRegistry;
use crate::types::WorkerId;

use super::command::{WorkerCommand, WorkerFlow};
use super::handle::WorkerHandle;
use super::jobs::drain_pending_jobs;
use super::render::worker_thread_name;
use super::state::WorkerState;

pub(crate) fn spawn_worker(
    worker_id: WorkerId,
    options: &VmOptions,
    host_registry: Arc<InMemoryHostContractRegistry>,
) -> Result<(WorkerHandle, JoinHandle<()>), VmError> {
    let (tx, rx) = sync_channel(options.queue_capacity);
    let queue_metrics = Arc::new(QueueMetrics::default());
    let latency_metrics = Arc::new(LatencyMetrics::new(options.latency_histograms));
    let worker_options = options.clone();
    let join_handle = spawn_worker_thread(worker_id, worker_options, host_registry, rx)?;
    Ok((
        WorkerHandle {
            id: worker_id,
            tx,
            queue_metrics,
            latency_metrics,
        },
        join_handle,
    ))
}

fn spawn_worker_thread(
    worker_id: WorkerId,
    worker_options: VmOptions,
    host_registry: Arc<InMemoryHostContractRegistry>,
    rx: Receiver<QueuedCommand<WorkerCommand>>,
) -> Result<JoinHandle<()>, VmError> {
    std::thread::Builder::new()
        .name(worker_thread_name(worker_id))
        .spawn(move || {
            let _ = run_worker_loop(worker_id, worker_options, host_registry, rx);
        })
        .map_err(thread_spawn_error)
}

fn run_worker_loop(
    worker_id: WorkerId,
    options: VmOptions,
    host_registry: Arc<InMemoryHostContractRegistry>,
    inbox: Receiver<QueuedCommand<WorkerCommand>>,
) -> Result<(), VmError> {
    let mut state = WorkerState::new(worker_id, options, host_registry)?;

    loop {
        match inbox.recv_timeout(state.options.idle_sleep) {
            Ok(queued_command) => {
                let command = queued_command.into_received_command();
                if should_stop(command.execute(&mut state)?) {
                    break;
                }
            }
            Err(RecvTimeoutError::Timeout) => drain_pending_jobs(&state),
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    Ok(())
}

fn should_stop(flow: WorkerFlow) -> bool {
    matches!(flow, WorkerFlow::Stop)
}

fn thread_spawn_error(error: std::io::Error) -> VmError {
    VmError::Execution {
        details: error.to_string(),
    }
}
