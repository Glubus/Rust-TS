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
    /// Loads or replaces one TypeScript script. A reload keeps the retention policy of
    /// the script it replaces; a new script stays mounted until unloaded.
    pub fn load_script(
        &self,
        id: impl Into<ScriptId>,
        source: impl Into<String>,
    ) -> Result<ScriptSnapshot, VmError> {
        self.load_inline_script(id.into(), source.into(), None)
    }

    /// Loads or replaces one TypeScript script with an explicit retention policy.
    pub fn load_script_with_policy(
        &self,
        id: impl Into<ScriptId>,
        source: impl Into<String>,
        policy: ScriptRetentionPolicy,
    ) -> Result<ScriptSnapshot, VmError> {
        self.load_inline_script(id.into(), source.into(), Some(policy))
    }

    /// Loads one multi-file TypeScript project from a filesystem entry point. A reload
    /// keeps the retention policy of the script it replaces.
    ///
    /// Version 0 supports static ESM graphs with local imports, tsconfig aliases, and
    /// package imports resolved from project-local `node_modules`.
    pub fn load_script_project(
        &self,
        id: impl Into<ScriptId>,
        entry_path: impl AsRef<Path>,
    ) -> Result<ScriptSnapshot, VmError> {
        self.load_script_project_with_policy(id, entry_path, None)
    }

    /// Unloads one script from its current worker.
    pub fn unload_script(&self, script_id: impl Into<ScriptId>) -> Result<(), VmError> {
        let script_id = script_id.into();
        let _lifecycle = self.lock_script_lifecycle()?;
        let worker_id = self.lookup_worker_for_script(&script_id)?;
        self.demount_locked(worker_id, &script_id)
    }

    /// `None` keeps the retention policy of the script being replaced.
    pub(crate) fn load_script_project_with_policy(
        &self,
        id: impl Into<ScriptId>,
        entry_path: impl AsRef<Path>,
        policy: Option<ScriptRetentionPolicy>,
    ) -> Result<ScriptSnapshot, VmError> {
        self.load_prepared(id.into(), ScriptSourceKind::Project, policy, |_| {
            self.prepare_project_script(entry_path.as_ref())
        })
    }

    fn load_inline_script(
        &self,
        script_id: ScriptId,
        source: String,
        policy: Option<ScriptRetentionPolicy>,
    ) -> Result<ScriptSnapshot, VmError> {
        self.load_prepared(script_id, ScriptSourceKind::Inline, policy, |script_id| {
            self.prepare_script(script_id, &source)
        })
    }

    /// Compiles and mounts one script under the lifecycle lock, recording load latency.
    fn load_prepared(
        &self,
        script_id: ScriptId,
        source_kind: ScriptSourceKind,
        policy: Option<ScriptRetentionPolicy>,
        prepare: impl FnOnce(&str) -> Result<CompiledScript, VmError>,
    ) -> Result<ScriptSnapshot, VmError> {
        let started_at = Instant::now();
        let _lifecycle = self.lock_script_lifecycle()?;
        let result = prepare(&script_id).and_then(|compiled| {
            self.mount_compiled_script(script_id, source_kind, compiled, policy)
        });
        self.inner.metrics.observe_load(started_at.elapsed());
        result
    }

    fn mount_compiled_script(
        &self,
        script_id: ScriptId,
        source_kind: ScriptSourceKind,
        compiled: CompiledScript,
        policy: Option<ScriptRetentionPolicy>,
    ) -> Result<ScriptSnapshot, VmError> {
        let policy = self.retention_policy_for_load(&script_id, policy)?;
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

    /// An explicit policy wins; otherwise a reload keeps the mounted script's policy
    /// and a new script stays mounted.
    fn retention_policy_for_load(
        &self,
        script_id: &str,
        requested: Option<ScriptRetentionPolicy>,
    ) -> Result<ScriptRetentionPolicy, VmError> {
        if let Some(policy) = requested {
            return Ok(policy);
        }
        Ok(self
            .inner
            .active_runtime_registry
            .retention_policy(script_id)?
            .unwrap_or(ScriptRetentionPolicy::KeepMounted))
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
