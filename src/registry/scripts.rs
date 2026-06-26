//! Script registry storing source-related materialization metadata.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::VmError;
use crate::types::{ScriptId, ScriptSourceKind, WorkerId};

/// Materialization state for one script.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ScriptMaterializationState {
    /// Script is registered.
    Registered,
    /// Script has a compiled artifact.
    Compiled,
    /// Script has been mounted.
    Mounted,
}

/// Entry stored by the script registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScriptRegistryEntry {
    /// Stable script identity.
    pub script_id: ScriptId,
    /// Source kind used to produce the current artifact.
    pub source_kind: ScriptSourceKind,
    /// Current source hash.
    pub source_hash: String,
    /// Path to the compiled artifact, if known.
    pub compiled_path: String,
    /// Original filesystem entry path when applicable.
    pub entry_path: Option<String>,
    /// Materialization state.
    pub state: ScriptMaterializationState,
    /// Preferred or last known runner affinity.
    pub preferred_runner: Option<WorkerId>,
}

/// Script materialization metadata access.
pub trait ScriptRegistry: Send + Sync {
    /// Returns one entry by script id.
    fn get(&self, script_id: &str) -> Result<Option<ScriptRegistryEntry>, VmError>;

    /// Returns every stored entry.
    fn list(&self) -> Result<Vec<ScriptRegistryEntry>, VmError>;

    /// Stores or replaces one entry.
    fn upsert(&self, entry: ScriptRegistryEntry) -> Result<(), VmError>;

    /// Updates the materialization state of one entry when it exists.
    fn set_state(&self, script_id: &str, state: ScriptMaterializationState) -> Result<(), VmError>;
}

/// In-memory V0 script registry.
#[derive(Default)]
pub struct InMemoryScriptRegistry {
    by_script_id: Mutex<HashMap<ScriptId, ScriptRegistryEntry>>,
}

impl InMemoryScriptRegistry {
    /// Creates an empty script registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Updates the materialization state only when the current source hash still matches.
    #[cfg(feature = "async-promise")]
    pub(crate) fn set_state_if_source_hash(
        &self,
        script_id: &str,
        source_hash: &str,
        state: ScriptMaterializationState,
    ) -> Result<(), VmError> {
        let mut guard = self
            .by_script_id
            .lock()
            .map_err(|_| VmError::WorkerPanicked)?;
        if let Some(entry) = guard.get_mut(script_id)
            && entry.source_hash == source_hash
        {
            entry.state = state;
        }
        Ok(())
    }
}

impl ScriptRegistry for InMemoryScriptRegistry {
    fn get(&self, script_id: &str) -> Result<Option<ScriptRegistryEntry>, VmError> {
        let guard = self
            .by_script_id
            .lock()
            .map_err(|_| VmError::WorkerPanicked)?;
        Ok(guard.get(script_id).cloned())
    }

    fn list(&self) -> Result<Vec<ScriptRegistryEntry>, VmError> {
        let guard = self
            .by_script_id
            .lock()
            .map_err(|_| VmError::WorkerPanicked)?;
        let mut entries = guard.values().cloned().collect::<Vec<_>>();
        entries.sort_by(|left, right| left.script_id.cmp(&right.script_id));
        Ok(entries)
    }

    fn upsert(&self, entry: ScriptRegistryEntry) -> Result<(), VmError> {
        let mut guard = self
            .by_script_id
            .lock()
            .map_err(|_| VmError::WorkerPanicked)?;
        guard.insert(entry.script_id.clone(), entry);
        Ok(())
    }

    fn set_state(&self, script_id: &str, state: ScriptMaterializationState) -> Result<(), VmError> {
        let mut guard = self
            .by_script_id
            .lock()
            .map_err(|_| VmError::WorkerPanicked)?;
        if let Some(entry) = guard.get_mut(script_id) {
            entry.state = state;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry(script_id: &str, state: ScriptMaterializationState) -> ScriptRegistryEntry {
        ScriptRegistryEntry {
            script_id: String::from(script_id),
            source_kind: ScriptSourceKind::Inline,
            source_hash: String::from("hash"),
            compiled_path: String::from("/tmp/script.js"),
            entry_path: None,
            state,
            preferred_runner: Some(1),
        }
    }

    #[test]
    fn upsert_then_get_returns_entry() {
        let registry = InMemoryScriptRegistry::new();
        let entry = sample_entry("alpha", ScriptMaterializationState::Compiled);

        registry.upsert(entry.clone()).unwrap();

        assert_eq!(registry.get("alpha").unwrap(), Some(entry));
    }

    #[test]
    fn upsert_replaces_existing_entry() {
        let registry = InMemoryScriptRegistry::new();

        registry
            .upsert(sample_entry(
                "alpha",
                ScriptMaterializationState::Registered,
            ))
            .unwrap();
        registry
            .upsert(sample_entry("alpha", ScriptMaterializationState::Mounted))
            .unwrap();

        let stored = registry.get("alpha").unwrap().unwrap();
        assert_eq!(stored.state, ScriptMaterializationState::Mounted);
    }

    #[test]
    fn list_returns_entries_sorted_by_script_id() {
        let registry = InMemoryScriptRegistry::new();

        registry
            .upsert(sample_entry("bravo", ScriptMaterializationState::Compiled))
            .unwrap();
        registry
            .upsert(sample_entry("alpha", ScriptMaterializationState::Mounted))
            .unwrap();

        let entries = registry.list().unwrap();
        let ids = entries
            .into_iter()
            .map(|entry| entry.script_id)
            .collect::<Vec<_>>();

        assert_eq!(ids, vec![String::from("alpha"), String::from("bravo")]);
    }

    #[test]
    fn set_state_updates_existing_entry() {
        let registry = InMemoryScriptRegistry::new();
        registry
            .upsert(sample_entry(
                "alpha",
                ScriptMaterializationState::Registered,
            ))
            .unwrap();

        registry
            .set_state("alpha", ScriptMaterializationState::Mounted)
            .unwrap();

        let stored = registry.get("alpha").unwrap().unwrap();
        assert_eq!(stored.state, ScriptMaterializationState::Mounted);
    }

    #[cfg(feature = "async-promise")]
    #[test]
    fn conditional_set_state_ignores_stale_source_hash() {
        let registry = InMemoryScriptRegistry::new();
        registry
            .upsert(sample_entry("alpha", ScriptMaterializationState::Mounted))
            .unwrap();

        registry
            .set_state_if_source_hash("alpha", "other-hash", ScriptMaterializationState::Compiled)
            .unwrap();

        let stored = registry.get("alpha").unwrap().unwrap();
        assert_eq!(stored.state, ScriptMaterializationState::Mounted);
    }

    #[cfg(feature = "async-promise")]
    #[test]
    fn conditional_set_state_updates_matching_source_hash() {
        let registry = InMemoryScriptRegistry::new();
        registry
            .upsert(sample_entry("alpha", ScriptMaterializationState::Mounted))
            .unwrap();

        registry
            .set_state_if_source_hash("alpha", "hash", ScriptMaterializationState::Compiled)
            .unwrap();

        let stored = registry.get("alpha").unwrap().unwrap();
        assert_eq!(stored.state, ScriptMaterializationState::Compiled);
    }
}
