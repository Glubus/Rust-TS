//! Commands sent to one worker runtime.

use std::sync::mpsc::Sender;

use serde_json::Value;

use crate::compiler::CompiledModule;
use crate::error::VmError;
use crate::types::ScriptId;

use super::event_dispatch::emit_event_to_scripts;
use super::invocation::call_script_function;
use super::jobs::{WorkerRuntimeStats, collect_worker_stats};
use super::load::{load_script_into_runtime, unload_loaded_script};
use super::state::WorkerState;

pub(crate) type WorkerReply<T> = Sender<Result<T, VmError>>;

pub(crate) enum WorkerCommand {
    LoadScript(LoadScriptCommand),
    CallFunction(CallFunctionCommand),
    EmitEvent(EmitEventCommand),
    UnloadScript(UnloadScriptCommand),
    Stats(StatsCommand),
    Shutdown(ShutdownCommand),
}

impl WorkerCommand {
    pub(crate) fn execute(self, state: &mut WorkerState) -> Result<WorkerFlow, VmError> {
        match self {
            Self::LoadScript(command) => command.execute(state),
            Self::CallFunction(command) => command.execute(state),
            Self::EmitEvent(command) => command.execute(state),
            Self::UnloadScript(command) => command.execute(state),
            Self::Stats(command) => command.execute(state),
            Self::Shutdown(command) => command.execute(state),
        }
    }
}

pub(crate) enum WorkerFlow {
    Continue,
    Stop,
}

pub(crate) struct LoadScriptCommand {
    pub(crate) id: ScriptId,
    pub(crate) transpiled_js: String,
    pub(crate) entry_module_id: Option<String>,
    pub(crate) modules: Vec<CompiledModule>,
    pub(crate) reply: WorkerReply<Vec<String>>,
}

impl LoadScriptCommand {
    pub(crate) fn execute(self, state: &mut WorkerState) -> Result<WorkerFlow, VmError> {
        let result = load_script_into_runtime(
            state,
            self.id,
            self.transpiled_js,
            self.entry_module_id,
            self.modules,
        );
        let _ = self.reply.send(result);
        Ok(WorkerFlow::Continue)
    }
}

pub(crate) struct CallFunctionCommand {
    pub(crate) script_id: ScriptId,
    pub(crate) function_name: String,
    pub(crate) args: Vec<Value>,
    pub(crate) reply: WorkerReply<Value>,
}

impl CallFunctionCommand {
    pub(crate) fn execute(self, state: &mut WorkerState) -> Result<WorkerFlow, VmError> {
        let result = call_script_function(
            &state.scripts,
            &self.script_id,
            &self.function_name,
            &self.args,
        );
        let _ = self.reply.send(result);
        Ok(WorkerFlow::Continue)
    }
}

pub(crate) struct EmitEventCommand {
    pub(crate) target_script_ids: Vec<ScriptId>,
    pub(crate) event_name: String,
    pub(crate) payload: Value,
    pub(crate) reply: WorkerReply<usize>,
}

impl EmitEventCommand {
    pub(crate) fn execute(self, state: &mut WorkerState) -> Result<WorkerFlow, VmError> {
        let result = emit_event_to_scripts(
            &state.scripts,
            &self.target_script_ids,
            &self.event_name,
            &self.payload,
        );
        let _ = self.reply.send(result);
        Ok(WorkerFlow::Continue)
    }
}

pub(crate) struct UnloadScriptCommand {
    pub(crate) script_id: ScriptId,
    pub(crate) reply: WorkerReply<()>,
}

impl UnloadScriptCommand {
    pub(crate) fn execute(self, state: &mut WorkerState) -> Result<WorkerFlow, VmError> {
        let result = unload_loaded_script(state, self.script_id);
        let _ = self.reply.send(result);
        Ok(WorkerFlow::Continue)
    }
}

pub(crate) struct StatsCommand {
    pub(crate) reply: WorkerReply<WorkerRuntimeStats>,
}

impl StatsCommand {
    pub(crate) fn execute(self, state: &mut WorkerState) -> Result<WorkerFlow, VmError> {
        let result = collect_worker_stats(state);
        let _ = self.reply.send(result);
        Ok(WorkerFlow::Continue)
    }
}

pub(crate) struct ShutdownCommand {
    pub(crate) reply: WorkerReply<()>,
}

impl ShutdownCommand {
    pub(crate) fn execute(self, state: &mut WorkerState) -> Result<WorkerFlow, VmError> {
        state.scripts.clear();
        state.runtime.run_gc();
        let _ = self.reply.send(Ok(()));
        Ok(WorkerFlow::Stop)
    }
}
