//! Async worker command types.

use serde_json::Value;
use tokio::sync::oneshot;

use crate::compiler::CompiledModule;
use crate::error::VmError;
use crate::runner::WorkerRuntimeStats;
use crate::types::ScriptId;

use super::AsyncWorkerLoad;

pub(super) type AsyncWorkerReply<T> = oneshot::Sender<Result<T, VmError>>;

pub(super) enum AsyncWorkerCommand {
    LoadScript(AsyncLoadScriptCommand),
    CallFunction(AsyncCallFunctionCommand),
    EmitEvent(AsyncEmitEventCommand),
    UnloadScript(AsyncUnloadScriptCommand),
    Stats(AsyncStatsCommand),
    Shutdown(AsyncShutdownCommand),
}

pub(super) struct AsyncLoadScriptCommand {
    pub(super) script_id: ScriptId,
    pub(super) cache_key: String,
    pub(super) transpiled_js: String,
    pub(super) entry_module_id: Option<String>,
    pub(super) modules: Vec<CompiledModule>,
    pub(super) reply: AsyncWorkerReply<AsyncWorkerLoad>,
}

pub(super) struct AsyncCallFunctionCommand {
    pub(super) script_id: ScriptId,
    pub(super) cache_key: String,
    pub(super) function_name: String,
    pub(super) args: Vec<Value>,
    pub(super) reply: AsyncWorkerReply<Value>,
}

pub(super) struct AsyncEmitEventCommand {
    pub(super) target_script_ids: Vec<ScriptId>,
    pub(super) event_name: String,
    pub(super) payload: Value,
    pub(super) reply: AsyncEmitEventReply,
}

pub(super) enum AsyncEmitEventReply {
    Blocking(std::sync::mpsc::Sender<Result<usize, VmError>>),
    Async(AsyncWorkerReply<usize>),
}

impl AsyncEmitEventReply {
    pub(super) fn send(self, result: Result<usize, VmError>) {
        match self {
            Self::Blocking(reply) => {
                let _ = reply.send(result);
            }
            Self::Async(reply) => {
                let _ = reply.send(result);
            }
        }
    }
}

pub(super) struct AsyncUnloadScriptCommand {
    pub(super) script_id: ScriptId,
    pub(super) cache_key: Option<String>,
}

pub(super) struct AsyncStatsCommand {
    pub(super) reply: std::sync::mpsc::Sender<Result<WorkerRuntimeStats, VmError>>,
}

pub(super) struct AsyncShutdownCommand;
