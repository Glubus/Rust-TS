use std::sync::Arc;

use super::routes::ActiveEventBinding;
use crate::error::VmError;
use crate::types::{RuntimeExecutionLane, ScriptId, ScriptRetentionPolicy, WorkerId};

/// Access to mounted scripts and active runner affinity.
pub trait ActiveRuntimeRegistry: Send + Sync {
    /// Returns the current runner hosting one script.
    fn get_worker(&self, script_id: &str) -> Result<Option<WorkerId>, VmError>;

    /// Returns the retention policy of one bound script.
    fn retention_policy(&self, script_id: &str) -> Result<Option<ScriptRetentionPolicy>, VmError>;

    /// Returns whether one bound script is idle under a demount-when-idle policy;
    /// `false` when the script is not bound.
    fn should_demount(&self, script_id: &str) -> Result<bool, VmError>;

    /// Binds one script to one runner.
    fn bind_script(
        &self,
        script_id: ScriptId,
        worker_id: WorkerId,
        policy: ScriptRetentionPolicy,
    ) -> Result<(), VmError>;

    /// Binds one script to one runner and execution lane.
    fn bind_script_with_lane(
        &self,
        script_id: ScriptId,
        worker_id: WorkerId,
        execution_lane: RuntimeExecutionLane,
        policy: ScriptRetentionPolicy,
    ) -> Result<(), VmError>;

    /// Replaces the active event subscriptions of one script.
    fn set_subscriptions(&self, script_id: &str, subscriptions: &[String]) -> Result<(), VmError>;

    /// Replaces the internal module dependency graph of one mounted script.
    fn set_module_dependencies(
        &self,
        script_id: &str,
        dependencies: &[(String, String)],
    ) -> Result<(), VmError>;

    /// Returns current active bindings for one event.
    fn get_event_bindings(&self, event_name: &str) -> Result<Arc<[ActiveEventBinding]>, VmError>;

    /// Marks the start of one execution on a mounted script.
    fn begin_execution(&self, script_id: &str) -> Result<(), VmError>;

    /// Marks the end of one execution and returns whether the script should demount.
    fn end_execution(&self, script_id: &str) -> Result<bool, VmError>;

    /// Adds one dependency reference to a mounted script.
    fn retain_dependency(&self, script_id: &str) -> Result<(), VmError>;

    /// Releases one dependency reference and returns whether the script should demount.
    fn release_dependency(&self, script_id: &str) -> Result<bool, VmError>;

    /// Records that `dependent_script_id` depends on `dependency_script_id`.
    fn retain_script_dependency(
        &self,
        dependent_script_id: &str,
        dependency_script_id: &str,
    ) -> Result<(), VmError>;

    /// Removes one script dependency edge and returns scripts that became demountable.
    fn release_script_dependency(
        &self,
        dependent_script_id: &str,
        dependency_script_id: &str,
    ) -> Result<Vec<(ScriptId, WorkerId)>, VmError>;

    /// Removes one script binding.
    fn unbind_script(&self, script_id: &str) -> Result<Vec<(ScriptId, WorkerId)>, VmError>;
}
