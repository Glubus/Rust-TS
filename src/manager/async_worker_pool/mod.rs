//! Manager-owned async QuickJS worker pool.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::thread::JoinHandle;
use std::time::Instant;

use serde_json::Value;

use crate::compiler::CompiledModule;
use crate::config::VmOptions;
use crate::error::VmError;
use crate::latency_metrics::LatencyMetrics;
use crate::queue_metrics::{QueueMetrics, QueueMetricsSnapshot, QueuedCommand};
use crate::registry::InMemoryHostContractRegistry;
use crate::runner::WorkerRuntimeStats;
use crate::types::{ScriptId, VmLatencyStats, WorkerId};

use self::command::{
    AsyncCallFunctionCommand, AsyncEmitEventCommand, AsyncEmitEventReply, AsyncLoadScriptCommand,
    AsyncShutdownCommand, AsyncStatsCommand, AsyncUnloadScriptCommand, AsyncWorkerCommand,
};
use self::handle::AsyncWorkerHandle;
use self::thread::spawn_async_worker;

mod command;
mod handle;
mod state;
mod thread;

pub(super) struct AsyncWorkerPool {
    workers: Vec<AsyncWorkerHandle>,
    next_worker: AtomicUsize,
    placements: tokio::sync::Mutex<std::collections::HashMap<ScriptId, usize>>,
}

pub(super) struct AsyncWorkerLoad {
    pub(super) worker_id: WorkerId,
    pub(super) module_ids: Vec<String>,
    pub(super) subscriptions: Vec<String>,
}

pub(super) struct AsyncWorkerScriptRequest {
    pub(super) script_id: ScriptId,
    pub(super) cache_key: String,
    pub(super) transpiled_js: String,
    pub(super) entry_module_id: Option<String>,
    pub(super) modules: Vec<CompiledModule>,
}

impl AsyncWorkerPool {
    pub(super) fn start(
        worker_count: usize,
        options: &VmOptions,
        host_registry: std::sync::Arc<InMemoryHostContractRegistry>,
    ) -> Result<(Self, Vec<JoinHandle<()>>), VmError> {
        let mut workers = Vec::with_capacity(worker_count);
        let mut joins = Vec::with_capacity(worker_count);

        for worker_id in 0..worker_count {
            let (tx, rx) = sync_channel(options.queue_capacity);
            let queue_metrics = Arc::new(QueueMetrics::default());
            let latency_metrics = Arc::new(LatencyMetrics::new(options.latency_histograms));
            let control = Arc::new(crate::runner::execution::ExecutionControl::default());
            let join = spawn_async_worker(
                worker_id,
                options.clone(),
                host_registry.clone(),
                rx,
                control.clone(),
            )?;
            workers.push(AsyncWorkerHandle {
                control,
                tx,
                queue_metrics,
                latency_metrics,
            });
            joins.push(join);
        }

        Ok((
            Self {
                workers,
                next_worker: AtomicUsize::new(0),
                placements: tokio::sync::Mutex::new(std::collections::HashMap::new()),
            },
            joins,
        ))
    }

    pub(super) async fn load_script(
        &self,
        request: AsyncWorkerScriptRequest,
    ) -> Result<AsyncWorkerLoad, VmError> {
        let mut placements = self.placements.lock().await;
        let id = request.script_id.clone();
        let index = placements.get(&id).copied().unwrap_or_else(|| {
            self.next_worker.fetch_add(1, Ordering::Relaxed) % self.workers.len()
        });
        let worker = self.worker(index)?;
        let (reply, receiver) = tokio::sync::oneshot::channel();
        let started_at = Instant::now();
        worker.send(AsyncWorkerCommand::LoadScript(AsyncLoadScriptCommand {
            script_id: request.script_id,
            cache_key: request.cache_key,
            transpiled_js: request.transpiled_js,
            entry_module_id: request.entry_module_id,
            modules: request.modules,
            reply,
        }))?;
        let result = receiver.await.map_err(|_| VmError::WorkerOffline)?;
        worker.latency_metrics.observe_load(started_at.elapsed());
        if result.is_ok() {
            placements.insert(id, index);
        }
        result
    }

    pub(super) async fn call_function(
        &self,
        worker_id: WorkerId,
        script_id: ScriptId,
        cache_key: String,
        function_name: String,
        args: Vec<Value>,
    ) -> Result<Value, VmError> {
        let worker = self.worker(worker_id)?;
        let (reply, receiver) = tokio::sync::oneshot::channel();
        let started_at = Instant::now();
        worker.send(AsyncWorkerCommand::CallFunction(AsyncCallFunctionCommand {
            script_id,
            cache_key,
            function_name,
            args,
            reply,
        }))?;
        let result = receiver.await.map_err(|_| VmError::WorkerOffline)?;
        worker.latency_metrics.observe_call(started_at.elapsed());
        result
    }

    pub(super) fn emit_event(
        &self,
        worker_id: WorkerId,
        target_script_ids: Vec<ScriptId>,
        event_name: String,
        payload: Value,
    ) -> Result<usize, VmError> {
        let worker = self.worker(worker_id)?;
        let (reply, receiver) = std::sync::mpsc::channel();
        let started_at = Instant::now();
        worker.send(AsyncWorkerCommand::EmitEvent(AsyncEmitEventCommand {
            target_script_ids,
            event_name,
            payload,
            reply: AsyncEmitEventReply::Blocking(reply),
        }))?;
        let result = receiver.recv().map_err(|_| VmError::WorkerOffline)?;
        worker.latency_metrics.observe_emit(started_at.elapsed());
        result
    }

    pub(super) async fn emit_event_async(
        &self,
        worker_id: WorkerId,
        target_script_ids: Vec<ScriptId>,
        event_name: String,
        payload: Value,
    ) -> Result<usize, VmError> {
        let worker = self.worker(worker_id)?;
        let (reply, receiver) = tokio::sync::oneshot::channel();
        let started_at = Instant::now();
        worker.send(AsyncWorkerCommand::EmitEvent(AsyncEmitEventCommand {
            target_script_ids,
            event_name,
            payload,
            reply: AsyncEmitEventReply::Async(reply),
        }))?;
        let result = receiver.await.map_err(|_| VmError::WorkerOffline)?;
        worker.latency_metrics.observe_emit(started_at.elapsed());
        result
    }

    pub(super) fn unload_script(
        &self,
        worker_id: WorkerId,
        script_id: ScriptId,
        cache_key: Option<String>,
    ) -> Result<(), VmError> {
        let worker = self.worker(worker_id)?;
        worker.send(AsyncWorkerCommand::UnloadScript(AsyncUnloadScriptCommand {
            script_id,
            cache_key,
        }))
    }

    pub(super) fn shutdown(&self) -> Result<(), VmError> {
        for worker in &self.workers {
            worker.control.stop();
            let _ = worker.tx.try_send(
                worker
                    .queue_metrics
                    .track(AsyncWorkerCommand::Shutdown(AsyncShutdownCommand)),
            );
        }
        Ok(())
    }

    pub(super) fn queue_snapshots(&self) -> Vec<QueueMetricsSnapshot> {
        self.workers
            .iter()
            .map(AsyncWorkerHandle::queue_snapshot)
            .collect()
    }

    pub(super) fn latency_snapshots(&self) -> Vec<VmLatencyStats> {
        self.workers
            .iter()
            .map(AsyncWorkerHandle::latency_snapshot)
            .collect()
    }

    pub(super) fn runtime_stats(&self) -> Result<Vec<WorkerRuntimeStats>, VmError> {
        let mut stats = Vec::with_capacity(self.workers.len());
        for worker in &self.workers {
            let (reply, receiver) = std::sync::mpsc::channel();
            worker.send(AsyncWorkerCommand::Stats(AsyncStatsCommand { reply }))?;
            stats.push(receiver.recv().map_err(|_| VmError::WorkerOffline)??);
        }
        Ok(stats)
    }

    fn worker(&self, worker_id: WorkerId) -> Result<&AsyncWorkerHandle, VmError> {
        self.workers
            .get(worker_id)
            .ok_or(VmError::InvalidWorkerCount)
    }
}

fn send_command(
    tx: &SyncSender<QueuedCommand<AsyncWorkerCommand>>,
    queue_metrics: &Arc<QueueMetrics>,
    command: AsyncWorkerCommand,
) -> Result<(), VmError> {
    let queued_command = queue_metrics.track(command);
    tx.try_send(queued_command)
        .map_err(|error| map_send_error(queue_metrics, error))
}

fn map_send_error(
    queue_metrics: &QueueMetrics,
    error: TrySendError<QueuedCommand<AsyncWorkerCommand>>,
) -> VmError {
    queue_metrics.record_rejected_send();
    match error {
        TrySendError::Full(_) => VmError::QueueFull,
        TrySendError::Disconnected(_) => VmError::WorkerOffline,
    }
}
