//! Async worker thread lifecycle.

use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::thread::{self, JoinHandle};

use crate::config::VmOptions;
use crate::error::VmError;
use crate::queue_metrics::QueuedCommand;
use crate::registry::InMemoryHostContractRegistry;
use crate::types::WorkerId;

use super::command::AsyncWorkerCommand;
use super::state::AsyncWorkerState;

pub(super) fn spawn_async_worker(
    worker_id: WorkerId,
    options: VmOptions,
    host_registry: Arc<InMemoryHostContractRegistry>,
    rx: Receiver<QueuedCommand<AsyncWorkerCommand>>,
) -> Result<JoinHandle<()>, VmError> {
    thread::Builder::new()
        .name(format!("rustts-async-worker-{worker_id}"))
        .spawn(move || run_async_worker(worker_id, options, host_registry, rx))
        .map_err(VmError::from)
}

fn run_async_worker(
    worker_id: WorkerId,
    options: VmOptions,
    host_registry: Arc<InMemoryHostContractRegistry>,
    rx: Receiver<QueuedCommand<AsyncWorkerCommand>>,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => return,
    };

    runtime.block_on(async move {
        let Ok(mut state) = AsyncWorkerState::new(worker_id, &options, host_registry).await else {
            return;
        };

        while let Ok(queued_command) = rx.recv() {
            let command = queued_command.into_received_command();
            if state.execute(command).await {
                break;
            }
        }
    });
}
