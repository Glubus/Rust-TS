//! Worker pool startup and reply helpers.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvError};
use std::thread::JoinHandle;

use crate::config::VmOptions;
use crate::error::VmError;
use crate::registry::InMemoryHostContractRegistry;
use crate::runner::{WorkerHandle, spawn_worker};

pub(super) fn resolve_worker_count(configured: usize) -> Result<usize, VmError> {
    let count = if configured == 0 {
        std::thread::available_parallelism()
            .map(|parallelism| parallelism.get())
            .unwrap_or(1)
    } else {
        configured
    };
    if count == 0 {
        return Err(VmError::InvalidWorkerCount);
    }
    Ok(count)
}

pub(super) fn start_workers(
    worker_count: usize,
    options: &VmOptions,
    host_registry: Arc<InMemoryHostContractRegistry>,
) -> Result<(Vec<WorkerHandle>, Vec<JoinHandle<()>>), VmError> {
    let mut workers = Vec::with_capacity(worker_count);
    let mut worker_joins = Vec::with_capacity(worker_count);

    for worker_id in 0..worker_count {
        let (worker, join_handle) = spawn_worker(worker_id, options, host_registry.clone())?;
        workers.push(worker);
        worker_joins.push(join_handle);
    }

    Ok((workers, worker_joins))
}

pub(super) fn recv_reply<T>(reply_rx: Receiver<Result<T, VmError>>) -> Result<T, VmError> {
    reply_rx.recv().map_err(map_recv_error)?
}

fn map_recv_error(_: RecvError) -> VmError {
    VmError::WorkerOffline
}
