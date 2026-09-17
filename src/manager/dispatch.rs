//! Worker command dispatch helpers for the manager.

use std::sync::mpsc::{Receiver, TrySendError, channel};
use std::time::Instant;

use serde_json::Value;

use crate::compiler::CompiledScript;
use crate::error::VmError;
use crate::runner::{
    CallFunctionCommand, EmitEventCommand, LoadScriptCommand, ShutdownCommand, StatsCommand,
    UnloadScriptCommand, WorkerCommand, WorkerRuntimeStats,
};
use crate::types::{ScriptId, WorkerId};

use super::script_manager::ScriptManager;
use super::worker_pool::recv_reply;

impl ScriptManager {
    pub(crate) fn dispatch_load_script(
        &self,
        worker_id: WorkerId,
        id: ScriptId,
        compiled: CompiledScript,
    ) -> Result<Vec<String>, VmError> {
        let (reply, rx) = channel();
        let command = WorkerCommand::LoadScript(LoadScriptCommand {
            id,
            transpiled_js: compiled.transpiled_js,
            entry_module_id: compiled.entry_path,
            modules: compiled.modules,
            reply,
        });
        let started_at = Instant::now();
        let result = self.send_command(worker_id, command, rx);
        self.observe_sync_worker_load(worker_id, started_at);
        result
    }

    pub(crate) fn dispatch_call_function(
        &self,
        worker_id: WorkerId,
        script_id: ScriptId,
        function_name: String,
        args: Vec<Value>,
    ) -> Result<Value, VmError> {
        let (reply, rx) = channel();
        let command = WorkerCommand::CallFunction(CallFunctionCommand {
            script_id,
            function_name,
            args,
            reply,
        });
        let started_at = Instant::now();
        let result = self.send_command(worker_id, command, rx);
        self.observe_sync_worker_call(worker_id, started_at);
        result
    }

    pub(crate) fn dispatch_unload_script(
        &self,
        worker_id: WorkerId,
        script_id: ScriptId,
    ) -> Result<(), VmError> {
        let (reply, rx) = channel();
        let command = WorkerCommand::UnloadScript(UnloadScriptCommand { script_id, reply });
        self.send_command(worker_id, command, rx)
    }

    pub(crate) fn dispatch_emit_event(
        &self,
        worker_id: WorkerId,
        target_script_ids: Vec<ScriptId>,
        event_name: String,
        payload: Value,
    ) -> Result<usize, VmError> {
        let (reply, rx) = channel();
        let command = WorkerCommand::EmitEvent(EmitEventCommand {
            target_script_ids,
            event_name,
            payload,
            reply,
        });
        let started_at = Instant::now();
        let result = self.send_command(worker_id, command, rx);
        self.observe_sync_worker_emit(worker_id, started_at);
        result
    }

    pub(crate) fn dispatch_stats(
        &self,
        worker_id: WorkerId,
    ) -> Result<WorkerRuntimeStats, VmError> {
        let (reply, rx) = channel();
        let command = WorkerCommand::Stats(StatsCommand { reply });
        self.send_command(worker_id, command, rx)
    }

    pub(super) fn dispatch_shutdown(&self, worker_id: WorkerId) -> Result<(), VmError> {
        let (reply, _rx) = channel();
        let command = WorkerCommand::Shutdown(ShutdownCommand { reply });
        let worker = &self.inner.workers[worker_id];
        let _ = worker.tx.try_send(worker.queue_metrics.track(command));
        Ok(())
    }

    pub(super) fn join_workers(&self) -> Result<(), VmError> {
        let mut join_slot = self
            .inner
            .worker_joins
            .lock()
            .map_err(|_| VmError::WorkerPanicked)?;

        super::worker_pool::join_with_timeout(&mut join_slot, self.inner.options.shutdown_timeout)
    }

    fn send_command<T>(
        &self,
        worker_id: WorkerId,
        command: WorkerCommand,
        rx: Receiver<Result<T, VmError>>,
    ) -> Result<T, VmError> {
        if self
            .inner
            .is_shutdown
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(VmError::WorkerOffline);
        }
        let worker = self
            .inner
            .workers
            .get(worker_id)
            .ok_or(VmError::InvalidWorkerCount)?;

        let queued_command = worker.queue_metrics.track(command);
        match worker.tx.try_send(queued_command) {
            Ok(()) => recv_reply(rx),
            Err(TrySendError::Full(_)) => {
                worker.queue_metrics.record_rejected_send();
                Err(VmError::QueueFull)
            }
            Err(TrySendError::Disconnected(_)) => {
                worker.queue_metrics.record_rejected_send();
                Err(VmError::WorkerOffline)
            }
        }
    }

    fn observe_sync_worker_load(&self, worker_id: WorkerId, started_at: Instant) {
        if let Some(worker) = self.inner.workers.get(worker_id) {
            worker.latency_metrics.observe_load(started_at.elapsed());
        }
    }

    fn observe_sync_worker_call(&self, worker_id: WorkerId, started_at: Instant) {
        if let Some(worker) = self.inner.workers.get(worker_id) {
            worker.latency_metrics.observe_call(started_at.elapsed());
        }
    }

    fn observe_sync_worker_emit(&self, worker_id: WorkerId, started_at: Instant) {
        if let Some(worker) = self.inner.workers.get(worker_id) {
            worker.latency_metrics.observe_emit(started_at.elapsed());
        }
    }
}
