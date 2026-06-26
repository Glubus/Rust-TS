use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use super::entry::ActiveRuntimeEntry;
use super::interface::ActiveRuntimeRegistry;
use super::routes::{ActiveEventBinding, EventRouteStore};
use crate::error::VmError;
use crate::types::{
    RuntimeDependencyEdge, RuntimeEventBinding, RuntimeEventRoute, RuntimeExecutionLane,
    RuntimeModuleDependency, RuntimeRetentionStats, ScriptId, ScriptRetentionPolicy, WorkerId,
};

/// In-memory V0 active runtime registry.
#[derive(Default)]
pub struct InMemoryActiveRuntimeRegistry {
    state: Mutex<ActiveRuntimeState>,
    event_routes: EventRouteStore,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ActiveRuntimeMemoryStats {
    pub active_scripts: usize,
    pub event_route_bindings: usize,
    pub dependency_edges: usize,
    pub module_dependency_edges: usize,
}

pub(crate) struct ActiveRuntimeSnapshot {
    pub(crate) scripts: Vec<ActiveRuntimeScriptSnapshot>,
    pub(crate) dependency_edges: Vec<RuntimeDependencyEdge>,
    pub(crate) module_dependency_edges: Vec<(ScriptId, RuntimeModuleDependency)>,
    pub(crate) event_routes: Vec<RuntimeEventRoute>,
}

pub(crate) struct ActiveRuntimeScriptSnapshot {
    pub(crate) script_id: ScriptId,
    pub(crate) worker_id: WorkerId,
    pub(crate) execution_lane: RuntimeExecutionLane,
    pub(crate) policy: ScriptRetentionPolicy,
    pub(crate) retention: RuntimeRetentionStats,
    pub(crate) subscriptions: Vec<String>,
}

#[derive(Default)]
struct ActiveRuntimeState {
    by_script_id: HashMap<ScriptId, ActiveRuntimeEntry>,
    dependency_edges: HashSet<(ScriptId, ScriptId)>,
    module_dependency_edges: HashSet<(ScriptId, String, String)>,
}

impl InMemoryActiveRuntimeRegistry {
    /// Creates an empty active runtime registry.
    pub fn new() -> Self {
        Self::default()
    }

    fn with_entry_mut<T>(
        &self,
        script_id: &str,
        action: impl FnOnce(&mut ActiveRuntimeEntry) -> T,
    ) -> Result<T, VmError> {
        let mut guard = self.state.lock().map_err(|_| VmError::WorkerPanicked)?;
        let entry =
            guard
                .by_script_id
                .get_mut(script_id)
                .ok_or_else(|| VmError::ScriptNotFound {
                    script_id: script_id.to_owned(),
                })?;
        Ok(action(entry))
    }

    fn rebuild_routes(&self, by_script_id: &HashMap<ScriptId, ActiveRuntimeEntry>) {
        self.event_routes.rebuild(by_script_id);
    }

    pub(crate) fn memory_stats(&self) -> Result<ActiveRuntimeMemoryStats, VmError> {
        let state = self.state.lock().map_err(|_| VmError::WorkerPanicked)?;
        Ok(ActiveRuntimeMemoryStats {
            active_scripts: state.by_script_id.len(),
            event_route_bindings: self.event_routes.binding_count(),
            dependency_edges: state.dependency_edges.len(),
            module_dependency_edges: state.module_dependency_edges.len(),
        })
    }

    pub(crate) fn runtime_snapshot(&self) -> Result<ActiveRuntimeSnapshot, VmError> {
        let state = self.state.lock().map_err(|_| VmError::WorkerPanicked)?;
        Ok(ActiveRuntimeSnapshot {
            scripts: script_snapshots(&state),
            dependency_edges: dependency_edge_snapshots(&state),
            module_dependency_edges: module_dependency_edge_snapshots(&state),
            event_routes: event_route_snapshots(&self.event_routes),
        })
    }
}

fn script_snapshots(state: &ActiveRuntimeState) -> Vec<ActiveRuntimeScriptSnapshot> {
    let mut scripts = state
        .by_script_id
        .iter()
        .map(|(script_id, entry)| ActiveRuntimeScriptSnapshot {
            script_id: script_id.clone(),
            worker_id: entry.worker_id,
            execution_lane: entry.execution_lane,
            policy: entry.mount_policy.into(),
            retention: RuntimeRetentionStats {
                running_count: entry.retention.running_count,
                callback_binding_count: entry.retention.callback_binding_count,
                subscription_count: entry.retention.subscription_count,
                dependency_ref_count: entry.retention.dependency_ref_count,
            },
            subscriptions: sorted_subscriptions(entry),
        })
        .collect::<Vec<_>>();
    scripts.sort_by(|left, right| left.script_id.cmp(&right.script_id));
    scripts
}

fn sorted_subscriptions(entry: &ActiveRuntimeEntry) -> Vec<String> {
    let mut subscriptions = entry.subscriptions.iter().cloned().collect::<Vec<_>>();
    subscriptions.sort();
    subscriptions
}

fn dependency_edge_snapshots(state: &ActiveRuntimeState) -> Vec<RuntimeDependencyEdge> {
    let mut edges = state
        .dependency_edges
        .iter()
        .map(
            |(dependent_script_id, dependency_script_id)| RuntimeDependencyEdge {
                dependent_script_id: dependent_script_id.clone(),
                dependency_script_id: dependency_script_id.clone(),
            },
        )
        .collect::<Vec<_>>();
    edges.sort_by(|left, right| {
        left.dependent_script_id
            .cmp(&right.dependent_script_id)
            .then_with(|| left.dependency_script_id.cmp(&right.dependency_script_id))
    });
    edges
}

fn module_dependency_edge_snapshots(
    state: &ActiveRuntimeState,
) -> Vec<(ScriptId, RuntimeModuleDependency)> {
    let mut edges = state
        .module_dependency_edges
        .iter()
        .map(|(script_id, module_id, dependency_id)| {
            (
                script_id.clone(),
                RuntimeModuleDependency {
                    module_id: module_id.clone(),
                    dependency_id: dependency_id.clone(),
                },
            )
        })
        .collect::<Vec<_>>();
    edges.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.module_id.cmp(&right.1.module_id))
            .then_with(|| left.1.dependency_id.cmp(&right.1.dependency_id))
    });
    edges
}

fn event_route_snapshots(routes: &EventRouteStore) -> Vec<RuntimeEventRoute> {
    routes
        .snapshot()
        .into_iter()
        .map(|(event_name, bindings)| RuntimeEventRoute {
            event_name,
            bindings: event_bindings(bindings),
        })
        .collect()
}

fn event_bindings(bindings: Vec<ActiveEventBinding>) -> Vec<RuntimeEventBinding> {
    let mut bindings = bindings
        .into_iter()
        .map(|binding| RuntimeEventBinding {
            script_id: binding.script_id,
            worker_id: binding.worker_id,
            execution_lane: binding.execution_lane,
        })
        .collect::<Vec<_>>();
    bindings.sort_by(|left, right| {
        left.script_id
            .cmp(&right.script_id)
            .then_with(|| left.worker_id.cmp(&right.worker_id))
    });
    bindings
}

impl ActiveRuntimeRegistry for InMemoryActiveRuntimeRegistry {
    fn get_worker(&self, script_id: &str) -> Result<Option<WorkerId>, VmError> {
        let guard = self.state.lock().map_err(|_| VmError::WorkerPanicked)?;
        Ok(guard
            .by_script_id
            .get(script_id)
            .map(|entry| entry.worker_id))
    }

    fn bind_script(
        &self,
        script_id: ScriptId,
        worker_id: WorkerId,
        policy: ScriptRetentionPolicy,
    ) -> Result<(), VmError> {
        self.bind_script_with_lane(script_id, worker_id, RuntimeExecutionLane::Sync, policy)
    }

    fn bind_script_with_lane(
        &self,
        script_id: ScriptId,
        worker_id: WorkerId,
        execution_lane: RuntimeExecutionLane,
        policy: ScriptRetentionPolicy,
    ) -> Result<(), VmError> {
        let mut guard = self.state.lock().map_err(|_| VmError::WorkerPanicked)?;
        if let Some(entry) = guard.by_script_id.get_mut(&script_id) {
            entry.worker_id = worker_id;
            entry.execution_lane = execution_lane;
            entry.mount_policy = policy.into();
        } else {
            guard.by_script_id.insert(
                script_id,
                ActiveRuntimeEntry::new_with_lane(worker_id, execution_lane, policy),
            );
        }
        self.rebuild_routes(&guard.by_script_id);
        Ok(())
    }

    fn set_subscriptions(&self, script_id: &str, subscriptions: &[String]) -> Result<(), VmError> {
        let mut state = self.state.lock().map_err(|_| VmError::WorkerPanicked)?;
        let entry =
            state
                .by_script_id
                .get_mut(script_id)
                .ok_or_else(|| VmError::ScriptNotFound {
                    script_id: script_id.to_owned(),
                })?;

        let next = subscriptions.iter().cloned().collect::<HashSet<_>>();
        if entry.replace_subscriptions(next) {
            self.rebuild_routes(&state.by_script_id);
        }
        Ok(())
    }

    fn set_module_dependencies(
        &self,
        script_id: &str,
        dependencies: &[(String, String)],
    ) -> Result<(), VmError> {
        let mut state = self.state.lock().map_err(|_| VmError::WorkerPanicked)?;
        ensure_script_exists(&state, script_id)?;
        state
            .module_dependency_edges
            .retain(|(edge_script_id, _, _)| edge_script_id != script_id);
        state
            .module_dependency_edges
            .extend(dependencies.iter().map(|(module_id, dependency_id)| {
                (
                    script_id.to_owned(),
                    module_id.clone(),
                    dependency_id.clone(),
                )
            }));
        Ok(())
    }

    fn get_event_bindings(&self, event_name: &str) -> Result<Arc<[ActiveEventBinding]>, VmError> {
        Ok(self.event_routes.bindings_for(event_name))
    }

    fn begin_execution(&self, script_id: &str) -> Result<(), VmError> {
        self.with_entry_mut(script_id, ActiveRuntimeEntry::begin_execution)
    }

    fn end_execution(&self, script_id: &str) -> Result<bool, VmError> {
        self.with_entry_mut(script_id, ActiveRuntimeEntry::end_execution)
    }

    fn retain_dependency(&self, script_id: &str) -> Result<(), VmError> {
        self.with_entry_mut(script_id, ActiveRuntimeEntry::retain_dependency)
    }

    fn release_dependency(&self, script_id: &str) -> Result<bool, VmError> {
        self.with_entry_mut(script_id, ActiveRuntimeEntry::release_dependency)
    }

    fn retain_script_dependency(
        &self,
        dependent_script_id: &str,
        dependency_script_id: &str,
    ) -> Result<(), VmError> {
        let mut state = self.state.lock().map_err(|_| VmError::WorkerPanicked)?;
        ensure_script_exists(&state, dependent_script_id)?;
        ensure_script_exists(&state, dependency_script_id)?;

        let edge = (
            dependent_script_id.to_owned(),
            dependency_script_id.to_owned(),
        );
        if state.dependency_edges.insert(edge)
            && let Some(dependency) = state.by_script_id.get_mut(dependency_script_id)
        {
            dependency.retain_dependency();
        }
        Ok(())
    }

    fn release_script_dependency(
        &self,
        dependent_script_id: &str,
        dependency_script_id: &str,
    ) -> Result<Vec<(ScriptId, WorkerId)>, VmError> {
        let mut state = self.state.lock().map_err(|_| VmError::WorkerPanicked)?;
        ensure_script_exists(&state, dependent_script_id)?;
        ensure_script_exists(&state, dependency_script_id)?;

        let edge = (
            dependent_script_id.to_owned(),
            dependency_script_id.to_owned(),
        );
        if !state.dependency_edges.remove(&edge) {
            return Ok(Vec::new());
        }

        Ok(release_dependency_ref(&mut state, dependency_script_id)
            .into_iter()
            .collect())
    }

    fn unbind_script(&self, script_id: &str) -> Result<Vec<(ScriptId, WorkerId)>, VmError> {
        let mut state = self.state.lock().map_err(|_| VmError::WorkerPanicked)?;
        if state.by_script_id.remove(script_id).is_none() {
            return Ok(Vec::new());
        }

        state
            .module_dependency_edges
            .retain(|(edge_script_id, _, _)| edge_script_id != script_id);
        let released_dependencies = remove_dependency_edges_for_script(&mut state, script_id);
        self.rebuild_routes(&state.by_script_id);
        Ok(released_dependencies)
    }
}

fn ensure_script_exists(state: &ActiveRuntimeState, script_id: &str) -> Result<(), VmError> {
    if state.by_script_id.contains_key(script_id) {
        return Ok(());
    }

    Err(VmError::ScriptNotFound {
        script_id: script_id.to_owned(),
    })
}

fn remove_dependency_edges_for_script(
    state: &mut ActiveRuntimeState,
    script_id: &str,
) -> Vec<(ScriptId, WorkerId)> {
    let outgoing_dependencies = state
        .dependency_edges
        .iter()
        .filter(|(dependent, _)| dependent == script_id)
        .map(|(_, dependency)| dependency.clone())
        .collect::<Vec<_>>();

    state
        .dependency_edges
        .retain(|(dependent, dependency)| dependent != script_id && dependency != script_id);

    outgoing_dependencies
        .into_iter()
        .filter_map(|dependency| release_dependency_ref(state, &dependency))
        .collect()
}

fn release_dependency_ref(
    state: &mut ActiveRuntimeState,
    dependency_script_id: &str,
) -> Option<(ScriptId, WorkerId)> {
    let dependency = state.by_script_id.get_mut(dependency_script_id)?;
    dependency
        .release_dependency()
        .then_some((dependency_script_id.to_owned(), dependency.worker_id))
}
