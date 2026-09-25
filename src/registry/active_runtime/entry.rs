use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::types::{RuntimeExecutionLane, ScriptRetentionPolicy, WorkerId};

/// Mount policy applied to a script instance.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ScriptMountPolicy {
    /// Demount when idle and unreferenced.
    DemountWhenIdle,
    /// Keep mounted across idle periods.
    KeepMounted,
}

impl From<ScriptRetentionPolicy> for ScriptMountPolicy {
    fn from(value: ScriptRetentionPolicy) -> Self {
        match value {
            ScriptRetentionPolicy::KeepMounted => Self::KeepMounted,
            ScriptRetentionPolicy::DemountWhenIdle => Self::DemountWhenIdle,
        }
    }
}

impl From<ScriptMountPolicy> for ScriptRetentionPolicy {
    fn from(value: ScriptMountPolicy) -> Self {
        match value {
            ScriptMountPolicy::KeepMounted => Self::KeepMounted,
            ScriptMountPolicy::DemountWhenIdle => Self::DemountWhenIdle,
        }
    }
}

/// Runtime retention counters owned by the active runtime registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RuntimeRetention {
    /// Number of active executions.
    pub running_count: usize,
    /// Number of active callback bindings.
    pub callback_binding_count: usize,
    /// Number of active subscriptions.
    pub subscription_count: usize,
    /// Number of dependency references.
    pub dependency_ref_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct ActiveRuntimeEntry {
    pub(super) worker_id: WorkerId,
    pub(super) execution_lane: RuntimeExecutionLane,
    pub(super) retention: RuntimeRetention,
    pub(super) mount_policy: ScriptMountPolicy,
    pub(super) subscriptions: HashSet<String>,
}

impl ActiveRuntimeEntry {
    pub(super) fn new_with_lane(
        worker_id: WorkerId,
        execution_lane: RuntimeExecutionLane,
        policy: ScriptRetentionPolicy,
    ) -> Self {
        Self {
            worker_id,
            execution_lane,
            retention: RuntimeRetention::default(),
            mount_policy: policy.into(),
            subscriptions: HashSet::new(),
        }
    }

    pub(super) fn begin_execution(&mut self) {
        self.retention.running_count += 1;
    }

    pub(super) fn end_execution(&mut self) -> bool {
        if self.retention.running_count > 0 {
            self.retention.running_count -= 1;
        }
        self.should_demount()
    }

    pub(super) fn retain_dependency(&mut self) {
        self.retention.dependency_ref_count += 1;
    }

    pub(super) fn release_dependency(&mut self) -> bool {
        if self.retention.dependency_ref_count > 0 {
            self.retention.dependency_ref_count -= 1;
        }
        self.should_demount()
    }

    pub(super) fn replace_subscriptions(&mut self, subscriptions: HashSet<String>) -> bool {
        if self.subscriptions == subscriptions {
            return false;
        }

        self.retention.subscription_count = subscriptions.len();
        self.retention.callback_binding_count = subscriptions.len();
        self.subscriptions = subscriptions;
        true
    }

    pub(super) fn should_demount(&self) -> bool {
        matches!(self.mount_policy, ScriptMountPolicy::DemountWhenIdle)
            && self.retention.running_count == 0
            && self.retention.subscription_count == 0
            && self.retention.callback_binding_count == 0
            && self.retention.dependency_ref_count == 0
    }
}
