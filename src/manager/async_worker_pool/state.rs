//! Async worker runtime state.

use std::collections::HashMap;
use std::sync::Arc;

use crate::config::VmOptions;
use crate::error::VmError;
use crate::registry::InMemoryHostContractRegistry;
use crate::runner::WorkerRuntimeStats;
use crate::runner::async_script_runtime::{AsyncLoadedScript, AsyncScriptRuntime};
use crate::types::{ScriptId, WorkerId};

use super::AsyncWorkerLoad;
use super::command::{
    AsyncCallFunctionCommand, AsyncEmitEventCommand, AsyncLoadScriptCommand, AsyncShutdownCommand,
    AsyncStatsCommand, AsyncUnloadScriptCommand, AsyncWorkerCommand,
};

pub(super) struct AsyncWorkerState {
    worker_id: WorkerId,
    runtime: AsyncScriptRuntime,
    scripts: HashMap<ScriptId, LoadedAsyncScript>,
}

struct LoadedAsyncScript {
    /// Load identity: a stale handle of the same script id never reaches a newer load.
    instance: u64,
    script: AsyncLoadedScript,
}

impl AsyncWorkerState {
    pub(super) async fn new(
        worker_id: WorkerId,
        options: &VmOptions,
        host_registry: Arc<InMemoryHostContractRegistry>,
        control: Arc<crate::runner::execution::ExecutionControl>,
    ) -> Result<Self, VmError> {
        Ok(Self {
            worker_id,
            runtime: AsyncScriptRuntime::new_with_control(options, host_registry, control).await?,
            scripts: HashMap::new(),
        })
    }

    pub(super) async fn execute(&mut self, command: AsyncWorkerCommand) -> bool {
        match command {
            AsyncWorkerCommand::LoadScript(command) => self.execute_load(command).await,
            AsyncWorkerCommand::CallFunction(command) => self.execute_call(command).await,
            AsyncWorkerCommand::EmitEvent(command) => self.execute_emit(command).await,
            AsyncWorkerCommand::UnloadScript(command) => self.execute_unload(command),
            AsyncWorkerCommand::Stats(command) => self.execute_stats(command).await,
            AsyncWorkerCommand::Shutdown(command) => self.execute_shutdown(command),
        }
    }

    async fn execute_load(&mut self, command: AsyncLoadScriptCommand) -> bool {
        let result = self.load_script(command).await;
        let _ = result.reply.send(result.value);
        false
    }

    async fn load_script(
        &mut self,
        command: AsyncLoadScriptCommand,
    ) -> CommandResult<AsyncWorkerLoad> {
        let script_id = command.script_id.clone();
        let value = self
            .runtime
            .load_script(
                command.script_id.clone(),
                command.transpiled_js,
                command.entry_module_id,
                command.modules,
            )
            .await
            .map(|(script, subscriptions)| {
                let module_ids = script.module_ids().to_vec();
                self.scripts.insert(
                    script_id,
                    LoadedAsyncScript {
                        instance: command.instance,
                        script,
                    },
                );
                AsyncWorkerLoad {
                    worker_id: self.worker_id,
                    instance: command.instance,
                    module_ids,
                    subscriptions,
                }
            });

        CommandResult {
            reply: command.reply,
            value,
        }
    }

    async fn execute_call(&self, command: AsyncCallFunctionCommand) -> bool {
        let result = self.call_function(&command).await;
        let _ = command.reply.send(result);
        false
    }

    async fn call_function(
        &self,
        command: &AsyncCallFunctionCommand,
    ) -> Result<serde_json::Value, VmError> {
        let loaded = self.script_for_call(&command.script_id, command.instance)?;
        loaded
            .script
            .call_function(&command.script_id, &command.function_name, &command.args)
            .await
    }

    async fn execute_emit(&self, command: AsyncEmitEventCommand) -> bool {
        let result = self.emit_event(&command).await;
        command.reply.send(result);
        false
    }

    /// Targets unloaded after routing no longer listen and are skipped.
    async fn emit_event(&self, command: &AsyncEmitEventCommand) -> Result<usize, VmError> {
        let mut delivered_count = 0usize;
        for loaded in command
            .target_script_ids
            .iter()
            .filter_map(|script_id| self.scripts.get(script_id))
        {
            delivered_count += loaded
                .script
                .emit_event(&command.event_name, &command.payload)
                .await?;
        }
        Ok(delivered_count)
    }

    fn script_for_call(
        &self,
        script_id: &str,
        instance: u64,
    ) -> Result<&LoadedAsyncScript, VmError> {
        self.scripts
            .get(script_id)
            .filter(|loaded| loaded.instance == instance)
            .ok_or_else(|| VmError::ScriptNotFound {
                script_id: script_id.to_owned(),
            })
    }

    fn execute_unload(&mut self, command: AsyncUnloadScriptCommand) -> bool {
        self.unload_script(&command.script_id, command.instance);
        false
    }

    async fn execute_stats(&self, command: AsyncStatsCommand) -> bool {
        let result = Ok(WorkerRuntimeStats {
            loaded_scripts: self.scripts.len(),
            quickjs_memory: self.runtime.quickjs_memory_stats().await,
        });
        let _ = command.reply.send(result);
        false
    }

    fn unload_script(&mut self, script_id: &str, instance: u64) {
        if self
            .scripts
            .get(script_id)
            .is_some_and(|loaded| loaded.instance == instance)
        {
            self.scripts.remove(script_id);
        }
    }

    fn execute_shutdown(&mut self, _command: AsyncShutdownCommand) -> bool {
        self.scripts.clear();
        true
    }
}

struct CommandResult<T> {
    reply: super::command::AsyncWorkerReply<T>,
    value: Result<T, VmError>,
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::super::command::{
        AsyncEmitEventCommand, AsyncEmitEventReply, AsyncLoadScriptCommand, AsyncWorkerCommand,
    };
    use super::AsyncWorkerState;
    use crate::config::VmOptions;

    #[test]
    fn emit_skips_targets_unloaded_after_routing() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        runtime.block_on(async {
            let mut state =
                AsyncWorkerState::new(0, &VmOptions::default(), Arc::default(), Arc::default())
                    .await
                    .unwrap();
            let (reply, loaded) = tokio::sync::oneshot::channel();
            state
                .execute(AsyncWorkerCommand::LoadScript(AsyncLoadScriptCommand {
                    script_id: "listener".into(),
                    instance: 1,
                    transpiled_js: "ctx.on(\"tick\", () => {});".into(),
                    entry_module_id: None,
                    modules: Vec::new(),
                    reply,
                }))
                .await;
            loaded.await.unwrap().unwrap();
            let (reply, delivered) = std::sync::mpsc::channel();

            state
                .execute(AsyncWorkerCommand::EmitEvent(AsyncEmitEventCommand {
                    target_script_ids: vec!["unloaded".into(), "listener".into()],
                    event_name: "tick".into(),
                    payload: json!(null),
                    reply: AsyncEmitEventReply::Blocking(reply),
                }))
                .await;

            assert_eq!(delivered.recv().unwrap().unwrap(), 1);
        });
    }
}
