//! Shared public types.

use std::sync::mpsc::Receiver;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Stable identifier for a loaded script.
pub type ScriptId = String;

/// Stable identifier for one worker runtime.
pub type WorkerId = usize;

/// Origin of one loaded script artifact.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ScriptSourceKind {
    /// One inline TypeScript source string compiled directly.
    Inline,
    /// One filesystem-backed multi-file ESM project from an entry file.
    Project,
}

/// Retention policy applied when a script is mounted.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ScriptRetentionPolicy {
    /// Keep the script mounted until explicit unload.
    KeepMounted,
    /// Demount automatically when no execution or subscription keeps it alive.
    DemountWhenIdle,
}

/// Runtime statistics for the VM worker.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VmStats {
    /// Number of started workers.
    pub worker_count: usize,
    /// Number of loaded scripts across all workers.
    pub loaded_scripts: usize,
    /// Per-worker runtime and capacity statistics.
    pub workers: Vec<VmWorkerStats>,
    /// Number of transpiled artifacts already present on disk.
    pub cache_entries: usize,
    /// Aggregated manager-level operation latency counters.
    pub latency: VmLatencyStats,
    /// Structural memory footprint counters.
    pub memory: VmMemoryStats,
    /// Real process memory snapshot when supported by the current platform.
    pub process_memory: Option<VmProcessMemoryStats>,
}

/// Per-worker runtime and capacity statistics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VmWorkerStats {
    /// Worker identifier.
    pub worker_id: WorkerId,
    /// Number of scripts currently loaded in this worker's execution lane.
    pub loaded_scripts: usize,
    /// Number of mounted scripts tracked as active for this worker.
    pub active_scripts: usize,
    /// Number of hot event route bindings targeting this worker.
    pub event_route_bindings: usize,
    /// Number of retained script dependency edges owned by scripts on this worker.
    pub dependency_edges: usize,
    /// Number of retained module dependency edges owned by scripts on this worker.
    pub module_dependency_edges: usize,
    /// Configured script capacity for this worker.
    pub max_scripts: usize,
    /// Configured command queue capacity for this worker.
    pub queue_capacity: usize,
    /// Current depth of this worker's synchronous command queue.
    pub queue_depth: usize,
    /// Highest observed depth of this worker's synchronous command queue.
    pub queue_peak_depth: usize,
    /// Number of rejected sends on this worker's synchronous command queue.
    pub queue_rejected_sends: usize,
    /// Current depth of this worker's async command queue.
    pub async_queue_depth: usize,
    /// Highest observed depth of this worker's async command queue.
    pub async_queue_peak_depth: usize,
    /// Number of rejected sends on this worker's async command queue.
    pub async_queue_rejected_sends: usize,
    /// Latency counters for synchronous worker commands.
    pub sync_latency: VmLatencyStats,
    /// Latency counters for async Promise-aware worker commands.
    pub async_latency: VmLatencyStats,
    /// QuickJS memory usage for the synchronous execution lane.
    pub sync_quickjs_memory: Option<VmQuickJsMemoryStats>,
    /// QuickJS memory usage for the async Promise-aware execution lane.
    pub async_quickjs_memory: Option<VmQuickJsMemoryStats>,
    /// Configured QuickJS memory limit for this worker.
    pub memory_limit_bytes: usize,
    /// Configured QuickJS stack limit for this worker.
    pub max_stack_size_bytes: usize,
}

/// QuickJS runtime memory counters for one execution lane.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct VmQuickJsMemoryStats {
    /// Total bytes allocated by the QuickJS allocator.
    pub malloc_size_bytes: u64,
    /// QuickJS allocator limit in bytes, or zero when unlimited.
    pub malloc_limit_bytes: u64,
    /// Bytes currently used by live QuickJS values and runtime structures.
    pub memory_used_bytes: u64,
    /// Allocation count reported by QuickJS.
    pub malloc_count: u64,
    /// Live atom count reported by QuickJS.
    pub atom_count: u64,
    /// Live string count reported by QuickJS.
    pub string_count: u64,
    /// Live object count reported by QuickJS.
    pub object_count: u64,
    /// JavaScript function count reported by QuickJS.
    pub function_count: u64,
    /// Memory pressure against the QuickJS allocator limit, in basis points.
    pub memory_pressure_bps: Option<u64>,
    /// Optional policy classification computed from configured memory pressure thresholds.
    pub memory_pressure_alert: Option<VmMemoryPressureAlert>,
}

/// Memory pressure state computed from configured thresholds.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum VmMemoryPressureAlert {
    /// Memory pressure is below the warning threshold.
    Normal,
    /// Memory pressure crossed the warning threshold.
    Warning,
    /// Memory pressure crossed the critical threshold.
    Critical,
}

/// Optional QuickJS memory pressure thresholds, expressed in basis points.
///
/// `10_000` basis points means `100%` of the configured QuickJS allocator limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VmMemoryPressureThresholds {
    /// Warning threshold in basis points.
    pub warning_bps: u64,
    /// Critical threshold in basis points.
    pub critical_bps: u64,
}

impl VmMemoryPressureThresholds {
    /// Creates memory pressure thresholds from basis-point values.
    pub const fn from_basis_points(warning_bps: u64, critical_bps: u64) -> Self {
        Self {
            warning_bps,
            critical_bps,
        }
    }

    /// Creates memory pressure thresholds from integer percentages.
    pub const fn from_percent(warning_percent: u64, critical_percent: u64) -> Self {
        Self {
            warning_bps: warning_percent * 100,
            critical_bps: critical_percent * 100,
        }
    }

    pub(crate) fn classify(self, pressure_bps: u64) -> VmMemoryPressureAlert {
        if pressure_bps >= self.critical_bps {
            VmMemoryPressureAlert::Critical
        } else if pressure_bps >= self.warning_bps {
            VmMemoryPressureAlert::Warning
        } else {
            VmMemoryPressureAlert::Normal
        }
    }
}

#[cfg(test)]
mod memory_pressure_tests {
    use super::{VmMemoryPressureAlert, VmMemoryPressureThresholds};

    #[test]
    fn memory_pressure_thresholds_classify_basis_points() {
        let thresholds = VmMemoryPressureThresholds::from_basis_points(7_000, 9_000);

        assert_eq!(thresholds.classify(6_999), VmMemoryPressureAlert::Normal);
        assert_eq!(thresholds.classify(7_000), VmMemoryPressureAlert::Warning);
        assert_eq!(thresholds.classify(8_999), VmMemoryPressureAlert::Warning);
        assert_eq!(thresholds.classify(9_000), VmMemoryPressureAlert::Critical);
    }
}

/// Aggregated latency counters for manager operations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct VmLatencyStats {
    /// Number of script load operations observed by the manager.
    pub load_operations: u64,
    /// Total wall-clock time spent in script load operations, in nanoseconds.
    pub load_total_ns: u64,
    /// Average wall-clock time per script load operation, in nanoseconds.
    pub load_average_ns: u64,
    /// Maximum observed script load operation latency, in nanoseconds.
    pub load_max_ns: u64,
    /// Optional fixed-bucket load latency histogram.
    pub load_histogram: Option<Vec<VmLatencyHistogramBucket>>,
    /// Number of exported function calls observed by the manager.
    pub call_operations: u64,
    /// Total wall-clock time spent in exported function calls, in nanoseconds.
    pub call_total_ns: u64,
    /// Average wall-clock time per exported function call, in nanoseconds.
    pub call_average_ns: u64,
    /// Maximum observed exported function call latency, in nanoseconds.
    pub call_max_ns: u64,
    /// Optional fixed-bucket exported function call latency histogram.
    pub call_histogram: Option<Vec<VmLatencyHistogramBucket>>,
    /// Number of host event emissions observed by the manager.
    pub emit_operations: u64,
    /// Total wall-clock time spent in host event emissions, in nanoseconds.
    pub emit_total_ns: u64,
    /// Average wall-clock time per host event emission, in nanoseconds.
    pub emit_average_ns: u64,
    /// Maximum observed host event emission latency, in nanoseconds.
    pub emit_max_ns: u64,
    /// Optional fixed-bucket host event emission latency histogram.
    pub emit_histogram: Option<Vec<VmLatencyHistogramBucket>>,
}

/// One latency histogram bucket.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VmLatencyHistogramBucket {
    /// Inclusive bucket upper bound in nanoseconds.
    ///
    /// `None` means the final overflow bucket.
    pub upper_bound_ns: Option<u64>,
    /// Number of observed operations in this bucket.
    pub count: u64,
}

/// Structural memory counters for the VM runtime.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct VmMemoryStats {
    /// Number of entries retained in the script registry.
    pub script_registry_entries: usize,
    /// Number of mounted script instances.
    pub active_scripts: usize,
    /// Number of hot-path event route bindings.
    pub event_route_bindings: usize,
    /// Number of retained inter-script dependency edges.
    pub dependency_edges: usize,
    /// Number of retained module dependency edges inside mounted project scripts.
    pub module_dependency_edges: usize,
    /// Number of registered host contracts.
    pub host_contracts: usize,
}

/// Real process memory counters collected from the host OS.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct VmProcessMemoryStats {
    /// Resident set size in bytes.
    pub resident_bytes: u64,
    /// Virtual memory size in bytes.
    pub virtual_bytes: u64,
}

/// Materialization state exposed by runtime introspection.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RuntimeMaterializationState {
    /// Script is known by the registry but does not have a compiled artifact yet.
    Registered,
    /// Script has a compiled artifact but is not currently mounted.
    Compiled,
    /// Script is currently mounted or represented as mounted by an async handle.
    Mounted,
}

/// Execution lane currently hosting a mounted script.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RuntimeExecutionLane {
    /// Default synchronous QuickJS worker lane.
    Sync,
    /// Promise-aware async QuickJS worker lane.
    Async,
}

/// Runtime retention counters for one mounted script.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RuntimeRetentionStats {
    /// Number of active exported function executions.
    pub running_count: usize,
    /// Number of active callback bindings.
    pub callback_binding_count: usize,
    /// Number of active hot subscriptions.
    pub subscription_count: usize,
    /// Number of dependency references retaining this script.
    pub dependency_ref_count: usize,
}

/// One internal module dependency edge inside a mounted project script.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeModuleDependency {
    /// Runtime module identifier that owns the import.
    pub module_id: String,
    /// Runtime module identifier that satisfies the import.
    pub dependency_id: String,
}

/// One script-to-script retention dependency edge.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeDependencyEdge {
    /// Script retaining another script.
    pub dependent_script_id: ScriptId,
    /// Script retained by the dependent script.
    pub dependency_script_id: ScriptId,
}

/// One script binding in a hot event route.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeEventBinding {
    /// Script receiving the event.
    pub script_id: ScriptId,
    /// Worker currently hosting the script.
    pub worker_id: WorkerId,
    /// Execution lane currently hosting the script.
    pub execution_lane: RuntimeExecutionLane,
}

/// One prebuilt hot event route.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeEventRoute {
    /// Event name used for dispatch.
    pub event_name: String,
    /// Active script bindings for this event.
    pub bindings: Vec<RuntimeEventBinding>,
}

/// Introspection view for one script known by the manager.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeScriptView {
    /// Stable script identity.
    pub script_id: ScriptId,
    /// Source kind used to produce the current artifact.
    pub source_kind: ScriptSourceKind,
    /// Current materialization state.
    pub state: RuntimeMaterializationState,
    /// Preferred or last known runner affinity from the script registry.
    pub preferred_runner: Option<WorkerId>,
    /// Actual active worker if the script is currently mounted in the sync worker pool.
    pub active_worker: Option<WorkerId>,
    /// Execution lane currently hosting the script when mounted.
    pub execution_lane: Option<RuntimeExecutionLane>,
    /// Cache key derived from the original TypeScript source.
    pub cache_key: String,
    /// Path to the compiled JavaScript artifact.
    pub compiled_path: String,
    /// Original filesystem entry path when the source kind is project.
    pub entry_path: Option<String>,
    /// Retention policy when the script is mounted in the sync worker pool.
    pub retention_policy: Option<ScriptRetentionPolicy>,
    /// Retention counters when the script is mounted in the sync worker pool.
    pub retention: Option<RuntimeRetentionStats>,
    /// Hot event subscriptions currently known for this script.
    pub subscriptions: Vec<String>,
    /// Internal ESM module dependency edges for this mounted project script.
    pub module_dependencies: Vec<RuntimeModuleDependency>,
}

/// Full read-only runtime introspection snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VmRuntimeSnapshot {
    /// Current manager statistics collected at snapshot time.
    pub stats: VmStats,
    /// Scripts known by the manager, including compiled-but-unmounted entries.
    pub scripts: Vec<RuntimeScriptView>,
    /// Script-to-script retention dependency edges.
    pub dependency_edges: Vec<RuntimeDependencyEdge>,
    /// Prebuilt hot event routes.
    pub event_routes: Vec<RuntimeEventRoute>,
}

/// Snapshot of a loaded script.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScriptSnapshot {
    /// Script identifier chosen by the caller.
    pub id: ScriptId,
    /// Source kind used to produce this mounted artifact.
    pub source_kind: ScriptSourceKind,
    /// Worker chosen to host this script.
    pub worker_id: WorkerId,
    /// Cache key derived from the original TypeScript source.
    pub cache_key: String,
    /// Path to the transpiled JavaScript artifact.
    pub transpiled_path: String,
    /// Original filesystem entry path when the source kind is project.
    pub entry_path: Option<String>,
}

/// Lifecycle or execution event emitted by the VM.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub enum VmEvent {
    /// A script has been loaded or replaced.
    ScriptLoaded {
        /// Loaded script metadata.
        snapshot: ScriptSnapshot,
    },
    /// An async worker-lane script has been loaded or replaced.
    #[cfg(feature = "async-promise")]
    AsyncScriptLoaded {
        /// Loaded script identifier.
        script_id: ScriptId,
        /// Source kind used to produce the mounted artifact.
        source_kind: ScriptSourceKind,
        /// Cache key derived from the original TypeScript source.
        cache_key: String,
    },
    /// An exported function has been called successfully.
    FunctionCalled {
        /// Worker that executed the call.
        worker_id: WorkerId,
        /// Script identifier.
        script_id: ScriptId,
        /// Function name.
        function_name: String,
        /// JSON result returned by the call.
        result: Value,
    },
    /// An exported function has been called successfully on an async worker-lane script.
    #[cfg(feature = "async-promise")]
    AsyncFunctionCalled {
        /// Script identifier.
        script_id: ScriptId,
        /// Function name.
        function_name: String,
        /// JSON result returned by the call.
        result: Value,
    },
    /// A script has been unloaded.
    ScriptUnloaded {
        /// Worker that previously hosted the script.
        worker_id: WorkerId,
        /// Script identifier.
        script_id: ScriptId,
    },
    /// An async worker-lane script handle has been dropped.
    #[cfg(feature = "async-promise")]
    AsyncScriptUnloaded {
        /// Script identifier.
        script_id: ScriptId,
    },
    /// A host event was emitted to active scripts.
    HostEventEmitted {
        /// Event name used for dispatch.
        event_name: String,
        /// Number of handler invocations completed across active scripts.
        delivered_count: usize,
    },
    /// The VM is shutting down.
    Shutdown,
}

/// One subscription to the VM event stream.
#[derive(Debug)]
pub struct VmSubscription {
    pub(crate) rx: Receiver<VmEvent>,
    pub(crate) dropped: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl VmSubscription {
    /// Number of new events discarded because this subscription's queue was full.
    #[must_use]
    pub fn dropped_events(&self) -> u64 {
        self.dropped.load(std::sync::atomic::Ordering::Relaxed)
    }
    /// Blocks until the next event is available.
    ///
    /// Returns `None` when the VM is no longer producing events.
    #[must_use]
    pub fn recv(&self) -> Option<VmEvent> {
        self.rx.recv().ok()
    }

    /// Attempts to receive one event without blocking.
    #[must_use]
    pub fn try_recv(&self) -> Option<VmEvent> {
        self.rx.try_recv().ok()
    }
}
