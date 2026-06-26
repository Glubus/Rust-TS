//! Manager-level runtime and registry introspection.

use std::collections::HashMap;

use crate::error::VmError;
use crate::queue_metrics::QueueMetricsSnapshot;
use crate::registry::{
    ActiveRuntimeRegistry, ScriptMaterializationState, ScriptRegistry, ScriptRegistryEntry,
};
use crate::types::{
    RuntimeDependencyEdge, RuntimeEventRoute, RuntimeMaterializationState, RuntimeModuleDependency,
    RuntimeRetentionStats, RuntimeScriptView, ScriptId, ScriptRetentionPolicy, VmMemoryStats,
    VmQuickJsMemoryStats, VmRuntimeSnapshot, VmStats, VmWorkerStats, WorkerId,
};

use super::process_memory::current_process_memory;
use super::script_manager::ScriptManager;

impl ScriptManager {
    /// Returns current manager-level statistics.
    pub fn stats(&self) -> Result<VmStats, VmError> {
        let active_memory = self.inner.active_runtime_registry.memory_stats()?;
        let script_entries = self.inner.script_registry.list()?;
        let script_registry_entries = script_entries.len();
        let async_mounted_by_worker =
            self.count_mounted_scripts_outside_sync_registry_by_worker(&script_entries)?;
        let async_mounted_scripts = async_mounted_by_worker.values().sum::<usize>();
        let host_contracts = self.inner.host_contract_registry.descriptors()?.len();
        let active_snapshot = self.inner.active_runtime_registry.runtime_snapshot()?;
        let workers = self.worker_stats(&active_snapshot, &async_mounted_by_worker)?;
        let loaded_scripts = workers
            .iter()
            .map(|worker| worker.loaded_scripts)
            .sum::<usize>();

        Ok(VmStats {
            worker_count: self.inner.workers.len(),
            loaded_scripts,
            workers,
            cache_entries: self.inner.cache.entry_count()?,
            latency: self.inner.metrics.snapshot(),
            memory: VmMemoryStats {
                script_registry_entries,
                active_scripts: active_memory.active_scripts + async_mounted_scripts,
                event_route_bindings: active_memory.event_route_bindings,
                dependency_edges: active_memory.dependency_edges,
                module_dependency_edges: active_memory.module_dependency_edges,
                host_contracts,
            },
            process_memory: current_process_memory(),
        })
    }

    /// Returns one registered script entry when known by the manager.
    pub fn describe_script(
        &self,
        script_id: impl Into<ScriptId>,
    ) -> Result<Option<ScriptRegistryEntry>, VmError> {
        self.inner.script_registry.get(&script_id.into())
    }

    /// Returns every script entry currently known by the manager.
    pub fn list_scripts(&self) -> Result<Vec<ScriptRegistryEntry>, VmError> {
        self.inner.script_registry.list()
    }

    /// Returns a read-only snapshot of scripts, routes, retention counters, and runtime stats.
    pub fn runtime_snapshot(&self) -> Result<VmRuntimeSnapshot, VmError> {
        let stats = self.stats()?;
        let script_entries = self.inner.script_registry.list()?;
        let active_snapshot = self.inner.active_runtime_registry.runtime_snapshot()?;
        let active_by_script = active_snapshot
            .scripts
            .into_iter()
            .map(|script| {
                (
                    script.script_id.clone(),
                    ActiveScriptView {
                        worker_id: script.worker_id,
                        execution_lane: script.execution_lane,
                        policy: script.policy,
                        retention: script.retention,
                        subscriptions: script.subscriptions,
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        let mut module_dependencies =
            group_module_dependencies(active_snapshot.module_dependency_edges);
        let scripts = script_entries
            .into_iter()
            .map(|entry| {
                let entry_module_dependencies = module_dependencies
                    .remove(&entry.script_id)
                    .unwrap_or_default();
                let active = active_by_script.get(&entry.script_id);
                script_view(entry, active, entry_module_dependencies)
            })
            .collect();

        Ok(VmRuntimeSnapshot {
            stats,
            scripts,
            dependency_edges: active_snapshot.dependency_edges,
            event_routes: active_snapshot.event_routes,
        })
    }

    fn count_mounted_scripts_outside_sync_registry_by_worker(
        &self,
        entries: &[ScriptRegistryEntry],
    ) -> Result<HashMap<WorkerId, usize>, VmError> {
        let mut counts = HashMap::new();
        for entry in entries {
            if is_mounted_outside_sync_registry(&self.inner.active_runtime_registry, entry)?
                && let Some(worker_id) = entry.preferred_runner
            {
                *counts.entry(worker_id).or_default() += 1;
            }
        }
        Ok(counts)
    }

    fn worker_stats(
        &self,
        active_snapshot: &crate::registry::ActiveRuntimeSnapshot,
        async_mounted_by_worker: &HashMap<WorkerId, usize>,
    ) -> Result<Vec<VmWorkerStats>, VmError> {
        let active_counts = active_script_counts_by_worker(&active_snapshot.scripts);
        let async_counts = async_script_counts_by_worker(&active_snapshot.scripts);
        let script_workers = script_workers_by_script(&active_snapshot.scripts);
        let route_counts = event_route_counts_by_worker(&active_snapshot.event_routes);
        let dependency_counts =
            dependency_counts_by_worker(&active_snapshot.dependency_edges, &script_workers);
        let module_dependency_counts = module_dependency_counts_by_worker(
            &active_snapshot.module_dependency_edges,
            &script_workers,
        );
        let async_queue_snapshots = self.async_queue_snapshots();
        let async_latency_snapshots = self.async_latency_snapshots();
        let async_runtime_stats = self.async_runtime_stats()?;

        let mut workers = Vec::with_capacity(self.inner.workers.len());
        for worker in &self.inner.workers {
            let sync_queue = worker.queue_snapshot();
            let async_queue = async_queue_snapshots
                .get(worker.id)
                .copied()
                .unwrap_or_default();
            let async_latency = async_latency_snapshots
                .get(worker.id)
                .cloned()
                .unwrap_or_default();
            let async_runtime = async_runtime_stats.get(worker.id);
            let worker_runtime = self.dispatch_stats(worker.id)?;
            let sync_quickjs_memory = self.classify_memory_pressure(worker_runtime.quickjs_memory);
            let async_quickjs_memory =
                async_runtime.map(|stats| self.classify_memory_pressure(stats.quickjs_memory));
            let async_mounted = async_mounted_by_worker
                .get(&worker.id)
                .copied()
                .unwrap_or_default();
            workers.push(VmWorkerStats {
                worker_id: worker.id,
                loaded_scripts: worker_runtime.loaded_scripts
                    + count_for_worker(&async_counts, worker.id)
                    + async_mounted,
                active_scripts: count_for_worker(&active_counts, worker.id) + async_mounted,
                event_route_bindings: count_for_worker(&route_counts, worker.id),
                dependency_edges: count_for_worker(&dependency_counts, worker.id),
                module_dependency_edges: count_for_worker(&module_dependency_counts, worker.id),
                max_scripts: self.inner.options.max_scripts_per_worker,
                queue_capacity: self.inner.options.queue_capacity,
                queue_depth: sync_queue.current_depth,
                queue_peak_depth: sync_queue.peak_depth,
                queue_rejected_sends: sync_queue.rejected_sends,
                async_queue_depth: async_queue.current_depth,
                async_queue_peak_depth: async_queue.peak_depth,
                async_queue_rejected_sends: async_queue.rejected_sends,
                sync_latency: worker.latency_snapshot(),
                async_latency,
                sync_quickjs_memory: Some(sync_quickjs_memory),
                async_quickjs_memory,
                memory_limit_bytes: self.inner.options.memory_limit_bytes,
                max_stack_size_bytes: self.inner.options.max_stack_size_bytes,
            });
        }
        Ok(workers)
    }

    fn classify_memory_pressure(&self, mut stats: VmQuickJsMemoryStats) -> VmQuickJsMemoryStats {
        stats.memory_pressure_alert =
            self.inner
                .options
                .memory_pressure_thresholds
                .and_then(|thresholds| {
                    stats
                        .memory_pressure_bps
                        .map(|bps| thresholds.classify(bps))
                });
        stats
    }

    #[cfg(feature = "async-promise")]
    fn async_queue_snapshots(&self) -> Vec<QueueMetricsSnapshot> {
        self.inner.async_workers.queue_snapshots()
    }

    #[cfg(not(feature = "async-promise"))]
    fn async_queue_snapshots(&self) -> Vec<QueueMetricsSnapshot> {
        Vec::new()
    }

    #[cfg(feature = "async-promise")]
    fn async_latency_snapshots(&self) -> Vec<crate::types::VmLatencyStats> {
        self.inner.async_workers.latency_snapshots()
    }

    #[cfg(not(feature = "async-promise"))]
    fn async_latency_snapshots(&self) -> Vec<crate::types::VmLatencyStats> {
        Vec::new()
    }

    #[cfg(feature = "async-promise")]
    fn async_runtime_stats(&self) -> Result<Vec<crate::runner::WorkerRuntimeStats>, VmError> {
        self.inner.async_workers.runtime_stats()
    }

    #[cfg(not(feature = "async-promise"))]
    fn async_runtime_stats(&self) -> Result<Vec<crate::runner::WorkerRuntimeStats>, VmError> {
        Ok(Vec::new())
    }
}

struct ActiveScriptView {
    worker_id: WorkerId,
    execution_lane: crate::types::RuntimeExecutionLane,
    policy: ScriptRetentionPolicy,
    retention: RuntimeRetentionStats,
    subscriptions: Vec<String>,
}

fn is_mounted_outside_sync_registry(
    active_registry: &impl ActiveRuntimeRegistry,
    entry: &ScriptRegistryEntry,
) -> Result<bool, VmError> {
    if entry.state != ScriptMaterializationState::Mounted {
        return Ok(false);
    }

    active_registry
        .get_worker(&entry.script_id)
        .map(|worker| worker.is_none())
}

fn group_module_dependencies(
    edges: Vec<(ScriptId, RuntimeModuleDependency)>,
) -> HashMap<ScriptId, Vec<RuntimeModuleDependency>> {
    let mut by_script = HashMap::<ScriptId, Vec<RuntimeModuleDependency>>::new();
    for (script_id, edge) in edges {
        by_script.entry(script_id).or_default().push(edge);
    }
    by_script
}

fn active_script_counts_by_worker(
    scripts: &[crate::registry::ActiveRuntimeScriptSnapshot],
) -> HashMap<WorkerId, usize> {
    let mut counts = HashMap::new();
    for script in scripts {
        *counts.entry(script.worker_id).or_default() += 1;
    }
    counts
}

fn async_script_counts_by_worker(
    scripts: &[crate::registry::ActiveRuntimeScriptSnapshot],
) -> HashMap<WorkerId, usize> {
    let mut counts = HashMap::new();
    for script in scripts {
        if script.execution_lane == crate::types::RuntimeExecutionLane::Async {
            *counts.entry(script.worker_id).or_default() += 1;
        }
    }
    counts
}

fn script_workers_by_script(
    scripts: &[crate::registry::ActiveRuntimeScriptSnapshot],
) -> HashMap<ScriptId, WorkerId> {
    scripts
        .iter()
        .map(|script| (script.script_id.clone(), script.worker_id))
        .collect()
}

fn event_route_counts_by_worker(routes: &[RuntimeEventRoute]) -> HashMap<WorkerId, usize> {
    let mut counts = HashMap::new();
    for route in routes {
        for binding in &route.bindings {
            *counts.entry(binding.worker_id).or_default() += 1;
        }
    }
    counts
}

fn dependency_counts_by_worker(
    edges: &[RuntimeDependencyEdge],
    script_workers: &HashMap<ScriptId, WorkerId>,
) -> HashMap<WorkerId, usize> {
    let mut counts = HashMap::new();
    for edge in edges {
        if let Some(worker_id) = script_workers.get(&edge.dependent_script_id) {
            *counts.entry(*worker_id).or_default() += 1;
        }
    }
    counts
}

fn module_dependency_counts_by_worker(
    edges: &[(ScriptId, RuntimeModuleDependency)],
    script_workers: &HashMap<ScriptId, WorkerId>,
) -> HashMap<WorkerId, usize> {
    let mut counts = HashMap::new();
    for (script_id, _) in edges {
        if let Some(worker_id) = script_workers.get(script_id) {
            *counts.entry(*worker_id).or_default() += 1;
        }
    }
    counts
}

fn count_for_worker(counts: &HashMap<WorkerId, usize>, worker_id: WorkerId) -> usize {
    counts.get(&worker_id).copied().unwrap_or_default()
}

fn script_view(
    entry: ScriptRegistryEntry,
    active: Option<&ActiveScriptView>,
    module_dependencies: Vec<RuntimeModuleDependency>,
) -> RuntimeScriptView {
    RuntimeScriptView {
        script_id: entry.script_id,
        source_kind: entry.source_kind,
        state: materialization_state(entry.state),
        preferred_runner: entry.preferred_runner,
        active_worker: active.map(|script| script.worker_id),
        execution_lane: active.map(|script| script.execution_lane),
        cache_key: entry.source_hash,
        compiled_path: entry.compiled_path,
        entry_path: entry.entry_path,
        retention_policy: active.map(|script| script.policy),
        retention: active.map(|script| script.retention.clone()),
        subscriptions: active
            .map(|script| script.subscriptions.clone())
            .unwrap_or_default(),
        module_dependencies,
    }
}

fn materialization_state(state: ScriptMaterializationState) -> RuntimeMaterializationState {
    match state {
        ScriptMaterializationState::Registered => RuntimeMaterializationState::Registered,
        ScriptMaterializationState::Compiled => RuntimeMaterializationState::Compiled,
        ScriptMaterializationState::Mounted => RuntimeMaterializationState::Mounted,
    }
}
