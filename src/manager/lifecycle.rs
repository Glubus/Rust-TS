//! Script lifecycle bookkeeping owned by the manager.

use std::path::Path;
use std::sync::MutexGuard;

use crate::error::VmError;
use crate::registry::ScriptRegistryEntry;
use crate::registry::{ActiveRuntimeRegistry, ScriptMaterializationState, ScriptRegistry};
use crate::types::{ScriptId, ScriptSnapshot, ScriptSourceKind, VmEvent, WorkerId};

use super::script_manager::ScriptManager;

impl ScriptManager {
    pub(crate) fn record_script_registration(
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

    pub(crate) fn next_oneshot_script_id(&self) -> ScriptId {
        let next = self
            .inner
            .next_oneshot_script
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        format!("__oneshot_{}__", next)
    }

    pub(crate) fn build_script_snapshot(
        &self,
        script_id: &str,
        source_kind: ScriptSourceKind,
        worker_id: WorkerId,
        cache_key: String,
        entry_path: Option<String>,
    ) -> ScriptSnapshot {
        let transpiled_path = self.inner.cache.artifact_path(&cache_key);
        ScriptSnapshot {
            id: script_id.to_owned(),
            source_kind,
            worker_id,
            cache_key,
            transpiled_path: transpiled_path.display().to_string(),
            entry_path,
        }
    }

    pub(crate) fn resolve_load_worker(&self, script_id: &str) -> Result<WorkerId, VmError> {
        if let Some(worker_id) = self.inner.active_runtime_registry.get_worker(script_id)? {
            return Ok(worker_id);
        }

        if let Some(entry) = self.inner.script_registry.get(script_id)?
            && let Some(worker_id) = entry.preferred_runner
        {
            return Ok(worker_id);
        }

        let next = self
            .inner
            .next_worker
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(next % self.inner.workers.len())
    }

    pub(crate) fn lookup_worker_for_script(&self, script_id: &str) -> Result<WorkerId, VmError> {
        self.inner
            .active_runtime_registry
            .get_worker(script_id)?
            .ok_or_else(|| VmError::ScriptNotFound {
                script_id: script_id.to_owned(),
            })
    }

    /// Serializes loads, reloads, unloads and demounts, so the registries and the
    /// workers change together.
    pub(crate) fn lock_script_lifecycle(&self) -> Result<MutexGuard<'_, ()>, VmError> {
        self.inner
            .script_lifecycle_lock
            .lock()
            .map_err(|_| VmError::WorkerPanicked)
    }

    /// Retains one dependency reference for a mounted script.
    pub fn retain_script_dependency(&self, script_id: impl Into<ScriptId>) -> Result<(), VmError> {
        let script_id = script_id.into();
        self.inner
            .active_runtime_registry
            .retain_dependency(&script_id)
    }

    /// Releases one dependency reference and demounts the script if it became idle.
    pub fn release_script_dependency(&self, script_id: impl Into<ScriptId>) -> Result<(), VmError> {
        let script_id = script_id.into();
        let _lifecycle = self.lock_script_lifecycle()?;
        if self
            .inner
            .active_runtime_registry
            .release_dependency(&script_id)?
        {
            self.demount_idle_locked(&script_id)?;
        }
        Ok(())
    }

    /// Records that one mounted script depends on another mounted script.
    pub fn retain_script_dependency_edge(
        &self,
        dependent_script_id: impl Into<ScriptId>,
        dependency_script_id: impl Into<ScriptId>,
    ) -> Result<(), VmError> {
        let dependent_script_id = dependent_script_id.into();
        let dependency_script_id = dependency_script_id.into();
        self.inner
            .active_runtime_registry
            .retain_script_dependency(&dependent_script_id, &dependency_script_id)
    }

    /// Removes one dependency edge and demounts scripts that become idle.
    pub fn release_script_dependency_edge(
        &self,
        dependent_script_id: impl Into<ScriptId>,
        dependency_script_id: impl Into<ScriptId>,
    ) -> Result<(), VmError> {
        let dependent_script_id = dependent_script_id.into();
        let dependency_script_id = dependency_script_id.into();
        let _lifecycle = self.lock_script_lifecycle()?;
        let demount_candidates = self
            .inner
            .active_runtime_registry
            .release_script_dependency(&dependent_script_id, &dependency_script_id)?;
        self.demount_idle_candidates_locked(demount_candidates)
    }

    /// Demounts one script if it is still idle once no load or unload can race it.
    pub(crate) fn demount_if_idle(&self, script_id: &str) -> Result<(), VmError> {
        let _lifecycle = self.lock_script_lifecycle()?;
        self.demount_idle_locked(script_id)
    }

    /// Unloads one script from its worker and the registries. The caller holds the
    /// lifecycle lock.
    pub(crate) fn demount_locked(
        &self,
        worker_id: WorkerId,
        script_id: &str,
    ) -> Result<(), VmError> {
        self.dispatch_unload_script(worker_id, script_id.to_owned())?;
        self.inner
            .script_registry
            .set_state(script_id, ScriptMaterializationState::Compiled)?;
        let demount_candidates = self
            .inner
            .active_runtime_registry
            .unbind_script(script_id)?;
        self.inner.event_bus.publish(VmEvent::ScriptUnloaded {
            worker_id,
            script_id: script_id.to_owned(),
        });
        self.demount_idle_candidates_locked(demount_candidates)
    }

    fn demount_idle_locked(&self, script_id: &str) -> Result<(), VmError> {
        let registry = &self.inner.active_runtime_registry;
        match registry.get_worker(script_id)? {
            Some(worker_id) if registry.should_demount(script_id)? => {
                self.demount_locked(worker_id, script_id)
            }
            _ => Ok(()),
        }
    }

    fn demount_idle_candidates_locked(
        &self,
        candidates: Vec<(ScriptId, WorkerId)>,
    ) -> Result<(), VmError> {
        for (script_id, _) in candidates {
            self.demount_idle_locked(&script_id)?;
        }
        Ok(())
    }
}
