//! Worker maintenance jobs and stats.

use crate::error::VmError;
use crate::types::VmQuickJsMemoryStats;

use super::memory::quickjs_memory_stats;
use super::state::WorkerState;

pub(crate) struct WorkerRuntimeStats {
    pub(crate) loaded_scripts: usize,
    pub(crate) quickjs_memory: VmQuickJsMemoryStats,
}

pub(crate) fn collect_worker_stats(state: &WorkerState) -> Result<WorkerRuntimeStats, VmError> {
    Ok(WorkerRuntimeStats {
        loaded_scripts: state.scripts.len(),
        quickjs_memory: quickjs_memory_stats(state.runtime.memory_usage()),
    })
}

pub(crate) fn drain_pending_jobs(state: &WorkerState) {
    for script in state.scripts.values() {
        script
            .context
            .with(|ctx| while ctx.execute_pending_job() {});
    }
    state.runtime.run_gc();
}
