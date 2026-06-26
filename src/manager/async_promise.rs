//! Experimental manager entry points for Promise-aware script execution.

use std::path::Path;

use serde_json::Value;

use crate::compiler::CompiledScript;
use crate::error::VmError;
use crate::registry::{
    ActiveRuntimeRegistry, ScriptMaterializationState, ScriptRegistry, ScriptRegistryEntry,
};
use crate::types::{
    RuntimeExecutionLane, ScriptId, ScriptRetentionPolicy, ScriptSourceKind, VmEvent, WorkerId,
};

use super::async_worker_pool::AsyncWorkerScriptRequest;
use super::operations::module_dependency_edges;
use super::script_manager::ScriptManager;

/// Async script loaded through the manager compile/cache pipeline.
///
/// This reuses the manager compile/cache pipeline and runs inside the manager-owned
/// async worker lane for Promise-aware execution.
pub struct AsyncManagedScript {
    manager: ScriptManager,
    script_id: ScriptId,
    worker_id: WorkerId,
    cache_key: String,
    subscriptions: Vec<String>,
    module_ids: Vec<String>,
    module_dependencies: Vec<(String, String)>,
}

impl ScriptManager {
    /// Compiles, caches, and loads one inline TypeScript script into the async worker lane.
    ///
    /// This path supports `HostFunctionExecution::AsyncPromise` contracts because it uses
    /// `rquickjs::AsyncRuntime` instead of the default synchronous worker lane.
    pub async fn load_async_script(
        &self,
        id: impl Into<ScriptId>,
        source: impl Into<String>,
    ) -> Result<AsyncManagedScript, VmError> {
        let script_id = id.into();
        let source = source.into();
        let compiled = self.prepare_script(&script_id, &source)?;
        self.load_compiled_async_script(script_id, ScriptSourceKind::Inline, compiled)
            .await
    }

    /// Compiles, caches, and loads one TypeScript project into the async worker lane.
    ///
    /// Version 0 supports the same static ESM graph as the synchronous manager path,
    /// but executes it through `rquickjs::AsyncRuntime`.
    pub async fn load_async_script_project(
        &self,
        id: impl Into<ScriptId>,
        entry_path: impl AsRef<Path>,
    ) -> Result<AsyncManagedScript, VmError> {
        let script_id = id.into();
        let compiled = self.prepare_project_script(entry_path.as_ref())?;
        self.load_compiled_async_script(script_id, ScriptSourceKind::Project, compiled)
            .await
    }

    async fn load_compiled_async_script(
        &self,
        script_id: ScriptId,
        source_kind: ScriptSourceKind,
        compiled: CompiledScript,
    ) -> Result<AsyncManagedScript, VmError> {
        let cache_key = compiled.cache_key.clone();
        let transpiled_path = compiled.transpiled_path.clone();
        let entry_path = compiled.entry_path.clone();
        let module_dependencies = module_dependency_edges(&compiled);
        let loaded = self
            .inner
            .async_workers
            .load_script(AsyncWorkerScriptRequest {
                script_id: script_id.clone(),
                cache_key: cache_key.clone(),
                transpiled_js: compiled.transpiled_js,
                entry_module_id: compiled.entry_path,
                modules: compiled.modules,
            })
            .await?;
        self.record_async_script_registration(
            &script_id,
            source_kind,
            &cache_key,
            &transpiled_path,
            entry_path,
            loaded.worker_id,
        )?;
        self.record_async_runtime_binding(
            &script_id,
            loaded.worker_id,
            &loaded.subscriptions,
            &module_dependencies,
        )?;
        self.inner.event_bus.publish(VmEvent::AsyncScriptLoaded {
            script_id: script_id.clone(),
            source_kind,
            cache_key: cache_key.clone(),
        });

        Ok(AsyncManagedScript {
            manager: self.clone(),
            script_id,
            worker_id: loaded.worker_id,
            cache_key,
            subscriptions: loaded.subscriptions,
            module_ids: loaded.module_ids,
            module_dependencies,
        })
    }

    fn record_async_script_registration(
        &self,
        script_id: &str,
        source_kind: ScriptSourceKind,
        cache_key: &str,
        transpiled_path: &Path,
        entry_path: Option<String>,
        worker_id: WorkerId,
    ) -> Result<(), VmError> {
        self.inner.script_registry.upsert(ScriptRegistryEntry {
            script_id: script_id.to_owned(),
            source_kind,
            source_hash: cache_key.to_owned(),
            compiled_path: transpiled_path.display().to_string(),
            entry_path,
            state: ScriptMaterializationState::Mounted,
            preferred_runner: Some(worker_id),
        })
    }

    fn record_async_runtime_binding(
        &self,
        script_id: &str,
        worker_id: WorkerId,
        subscriptions: &[String],
        module_dependencies: &[(String, String)],
    ) -> Result<(), VmError> {
        self.inner.active_runtime_registry.bind_script_with_lane(
            script_id.to_owned(),
            worker_id,
            RuntimeExecutionLane::Async,
            ScriptRetentionPolicy::KeepMounted,
        )?;
        self.inner
            .active_runtime_registry
            .set_subscriptions(script_id, subscriptions)?;
        self.inner
            .active_runtime_registry
            .set_module_dependencies(script_id, module_dependencies)
    }
}

impl AsyncManagedScript {
    /// Calls one exported function and awaits the JavaScript result.
    pub async fn call_function(
        &self,
        function_name: impl AsRef<str>,
        args: &[Value],
    ) -> Result<Value, VmError> {
        let function_name = function_name.as_ref().to_owned();
        let result = self
            .manager
            .inner
            .async_workers
            .call_function(
                self.worker_id,
                self.script_id.clone(),
                self.cache_key.clone(),
                function_name.clone(),
                args.to_vec(),
            )
            .await?;
        self.manager
            .inner
            .event_bus
            .publish(VmEvent::AsyncFunctionCalled {
                script_id: self.script_id.clone(),
                function_name,
                result: result.clone(),
            });
        Ok(result)
    }

    /// Returns the manager cache key used for this compiled script.
    #[must_use]
    pub fn cache_key(&self) -> &str {
        &self.cache_key
    }

    /// Returns the event subscriptions collected during script bootstrap.
    #[must_use]
    pub fn subscriptions(&self) -> &[String] {
        &self.subscriptions
    }

    /// Returns runtime-scoped module IDs mounted for this script.
    #[must_use]
    pub fn module_ids(&self) -> &[String] {
        &self.module_ids
    }

    /// Returns module dependency edges from the compiled project graph.
    #[must_use]
    pub fn module_dependencies(&self) -> &[(String, String)] {
        &self.module_dependencies
    }

    /// Returns the async worker lane currently hosting this script.
    #[must_use]
    pub fn worker_id(&self) -> WorkerId {
        self.worker_id
    }
}

impl Drop for AsyncManagedScript {
    fn drop(&mut self) {
        let _ = self.manager.inner.async_workers.unload_script(
            self.worker_id,
            self.script_id.clone(),
            Some(self.cache_key.clone()),
        );
        if self.is_current_registry_entry() {
            let _ = self.manager.inner.script_registry.set_state_if_source_hash(
                &self.script_id,
                &self.cache_key,
                ScriptMaterializationState::Compiled,
            );
            let _ = self
                .manager
                .inner
                .active_runtime_registry
                .unbind_script(&self.script_id);
            self.manager
                .inner
                .event_bus
                .publish(VmEvent::AsyncScriptUnloaded {
                    script_id: self.script_id.clone(),
                });
        }
    }
}

impl AsyncManagedScript {
    fn is_current_registry_entry(&self) -> bool {
        self.manager
            .inner
            .script_registry
            .get(&self.script_id)
            .ok()
            .flatten()
            .is_some_and(|entry| entry.source_hash == self.cache_key)
    }
}
