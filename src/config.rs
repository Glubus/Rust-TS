//! Configuration types for the embedded VM.

use std::path::PathBuf;
use std::time::Duration;

use crate::types::VmMemoryPressureThresholds;

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

/// Runtime configuration for [`crate::RustTs`].
#[derive(Debug, Clone)]
pub struct VmOptions {
    /// Number of worker threads to start.
    ///
    /// `0` means automatic sizing from available parallelism.
    pub worker_threads: usize,
    /// Directory used to store transpiled JavaScript artifacts.
    pub cache_dir: PathBuf,
    /// Maximum number of loaded script contexts per worker.
    pub max_scripts_per_worker: usize,
    /// Capacity of the command queue used by each background worker.
    pub queue_capacity: usize,
    /// Sleep duration for a background worker when its queue is idle.
    pub idle_sleep: Duration,
    /// QuickJS memory limit in bytes, applied per worker runtime.
    pub memory_limit_bytes: usize,
    /// QuickJS stack limit in bytes, applied per worker runtime.
    pub max_stack_size_bytes: usize,
    /// Optional QuickJS memory pressure thresholds used to classify runtime stats.
    pub memory_pressure_thresholds: Option<VmMemoryPressureThresholds>,
    /// Enables fixed-bucket operation latency histograms in manager and worker stats.
    pub latency_histograms: bool,
    /// Host contract validation policy.
    pub contract_validation: VmContractValidation,
    /// Unknown object field validation policy for schema-backed host contracts.
    pub unknown_field_validation: VmUnknownFieldValidation,
}

impl Default for VmOptions {
    fn default() -> Self {
        Self {
            worker_threads: 0,
            cache_dir: PathBuf::from(".ts-embed-cache"),
            max_scripts_per_worker: 32,
            queue_capacity: 256,
            idle_sleep: Duration::from_millis(25),
            memory_limit_bytes: 16 * 1024 * 1024,
            max_stack_size_bytes: 512 * 1024,
            memory_pressure_thresholds: None,
            latency_histograms: false,
            contract_validation: VmContractValidation::Disabled,
            unknown_field_validation: VmUnknownFieldValidation::Allow,
        }
    }
}
