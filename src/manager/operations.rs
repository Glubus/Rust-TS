//! Manager script operations.

use std::path::Path;
use std::time::Instant;

use super::script_manager::ScriptManager;
use crate::compiler::CompiledScript;
use crate::error::VmError;
use crate::registry::ActiveRuntimeRegistry;
use crate::types::ScriptRetentionPolicy;
use crate::types::{ScriptId, ScriptSnapshot, ScriptSourceKind, VmEvent};

impl ScriptManager {
    /// Loads or replaces one TypeScript script.
    pub fn load_script(
        &self,
        id: impl Into<ScriptId>,
        source: impl Into<String>,
    ) -> Result<ScriptSnapshot, VmError> {
        self.load_script_with_policy(id, source, ScriptRetentionPolicy::KeepMounted)
    }

    /// Loads or replaces one TypeScript script with an explicit retention policy.
    pub fn load_script_with_policy(
        &self,
        id: impl Into<ScriptId>,
        source: impl Into<String>,
        policy: ScriptRetentionPolicy,
    ) -> Result<ScriptSnapshot, VmError> {
        let started_at = Instant::now();
        let script_id = id.into();
        let source = source.into();
        let _load_guard = self
            .inner
            .script_load_lock
            .lock()
            .map_err(|_| VmError::WorkerPanicked)?;
        let result = self
            .prepare_script(&script_id, &source)
            .and_then(|compiled| {
                self.mount_compiled_script(script_id, ScriptSourceKind::Inline, compiled, policy)
            });
        self.inner.metrics.observe_load(started_at.elapsed());
        result
    }

    /// Loads one multi-file TypeScript project from a filesystem entry point.
    ///
    /// Version 0 supports static ESM graphs with local imports, tsconfig aliases, and
    /// package imports resolved from project-local `node_modules`.
    pub fn load_script_project(
        &self,
        id: impl Into<ScriptId>,
        entry_path: impl AsRef<Path>,
    ) -> Result<ScriptSnapshot, VmError> {
        self.load_script_project_with_policy(id, entry_path, ScriptRetentionPolicy::KeepMounted)
    }

    /// Unloads one script from its current worker.
    pub fn unload_script(&self, script_id: impl Into<ScriptId>) -> Result<(), VmError> {
        let script_id = script_id.into();
        let worker_id = self.lookup_worker_for_script(&script_id)?;
        self.demount_script(worker_id, &script_id)
    }

    pub(crate) fn load_script_project_with_policy(
        &self,
        id: impl Into<ScriptId>,
        entry_path: impl AsRef<Path>,
        policy: ScriptRetentionPolicy,
    ) -> Result<ScriptSnapshot, VmError> {
        let started_at = Instant::now();
        let script_id = id.into();
        let _load_guard = self
            .inner
            .script_load_lock
            .lock()
            .map_err(|_| VmError::WorkerPanicked)?;
        let result = self
            .prepare_project_script(entry_path.as_ref())
            .and_then(|compiled| {
                self.mount_compiled_script(script_id, ScriptSourceKind::Project, compiled, policy)
            });
        self.inner.metrics.observe_load(started_at.elapsed());
        result
    }

    fn mount_compiled_script(
        &self,
        script_id: ScriptId,
        source_kind: ScriptSourceKind,
        compiled: CompiledScript,
        policy: ScriptRetentionPolicy,
    ) -> Result<ScriptSnapshot, VmError> {
        let cache_key = compiled.cache_key.clone();
        let transpiled_path = compiled.transpiled_path.clone();
        let entry_path = compiled.entry_path.clone();
        let module_dependencies = module_dependency_edges(&compiled);
        let worker_id = self.resolve_load_worker(&script_id)?;
        let subscriptions = self.dispatch_load_script(worker_id, script_id.clone(), compiled)?;
        let snapshot = self.build_script_snapshot(
            &script_id,
            source_kind,
            worker_id,
            cache_key.clone(),
            entry_path.clone(),
        );

        self.inner
            .active_runtime_registry
            .bind_script(script_id.clone(), worker_id, policy)?;
        self.inner
            .active_runtime_registry
            .set_subscriptions(&script_id, &subscriptions)?;
        self.inner
            .active_runtime_registry
            .set_module_dependencies(&script_id, &module_dependencies)?;
        self.record_script_registration(
            &script_id,
            source_kind,
            &cache_key,
            &transpiled_path,
            entry_path,
            worker_id,
        )?;
        self.inner.event_bus.publish(VmEvent::ScriptLoaded {
            snapshot: snapshot.clone(),
        });
        Ok(snapshot)
    }
}

pub(crate) fn module_dependency_edges(compiled: &CompiledScript) -> Vec<(String, String)> {
    let module_ids = compiled
        .modules
        .iter()
        .map(|module| module.module_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();

    compiled
        .modules
        .iter()
        .flat_map(|module| {
            module
                .resolved_requests
                .values()
                .filter(|dependency| module_ids.contains(dependency.as_str()))
                .map(|dependency| (module.module_id.clone(), dependency.clone()))
        })
        .collect()
}
