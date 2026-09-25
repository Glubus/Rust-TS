//! Configuration types for the engine.

use std::path::PathBuf;
use std::time::Duration;

/// Host contract validation policy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VmContractValidation {
    /// Do not validate host function payloads against registered schemas.
    #[default]
    Disabled,
    /// Validate host function inputs before calling Rust handlers.
    Inputs,
    /// Validate host function inputs and outputs around Rust handlers.
    InputsAndOutputs,
}

impl VmContractValidation {
    pub(crate) fn validates_inputs(self) -> bool {
        matches!(self, Self::Inputs | Self::InputsAndOutputs)
    }

    pub(crate) fn validates_outputs(self) -> bool {
        matches!(self, Self::InputsAndOutputs)
    }
}

/// Unknown object field validation policy for schema-backed host contracts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VmUnknownFieldValidation {
    /// Allow object fields that are not declared by the registered schema.
    #[default]
    Allow,
    /// Reject object fields that are not declared by the registered schema.
    Reject,
}

impl VmUnknownFieldValidation {
    pub(crate) fn rejects_unknown_fields(self) -> bool {
        matches!(self, Self::Reject)
    }
}

/// Configuration for [`crate::Engine`].
#[derive(Debug, Clone)]
pub struct VmOptions {
    /// Directory for transpiled JavaScript artifacts, reused across runs and reloads.
    /// `None` (the default) transpiles in memory on every load.
    pub cache_dir: Option<PathBuf>,
    /// Maximum wall time per load, call or emit, including the Promise jobs it queues.
    /// Rust host handlers must return cooperatively; they cannot be preempted.
    pub execution_timeout: Duration,
    /// QuickJS memory limit in bytes.
    pub memory_limit_bytes: usize,
    /// QuickJS stack limit in bytes.
    pub max_stack_size_bytes: usize,
    /// Host contract validation policy.
    pub contract_validation: VmContractValidation,
    /// Unknown object field validation policy for schema-backed host contracts.
    pub unknown_field_validation: VmUnknownFieldValidation,
}

impl Default for VmOptions {
    fn default() -> Self {
        Self {
            cache_dir: None,
            execution_timeout: Duration::from_secs(5),
            memory_limit_bytes: 16 * 1024 * 1024,
            max_stack_size_bytes: 512 * 1024,
            contract_validation: VmContractValidation::Disabled,
            unknown_field_validation: VmUnknownFieldValidation::Allow,
        }
    }
}
