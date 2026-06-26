//! Error types exposed by the VM.

use std::io;

use thiserror::Error;

use crate::contract::HostFunctionExecution;

/// Error type for all VM operations.
#[derive(Debug, Error)]
pub enum VmError {
    /// Background worker is no longer reachable.
    #[error("vm worker is offline")]
    WorkerOffline,
    /// Command queue is full.
    #[error("vm command queue is full")]
    QueueFull,
    /// Worker thread terminated unexpectedly.
    #[error("vm worker thread panicked")]
    WorkerPanicked,
    /// No worker could be created from the provided configuration.
    #[error("vm requires at least one worker thread")]
    InvalidWorkerCount,
    /// Script capacity was reached.
    #[error("script limit reached on worker {worker_id}: max={max_scripts}")]
    ScriptLimitReached {
        /// Worker identifier.
        worker_id: usize,
        /// Maximum number of loaded scripts for this worker.
        max_scripts: usize,
    },
    /// The requested script could not be found.
    #[error("script `{script_id}` is not loaded")]
    ScriptNotFound {
        /// Missing script identifier.
        script_id: String,
    },
    /// The requested export is missing or is not callable.
    #[error("function `{function_name}` is not exported by script `{script_id}`")]
    FunctionNotFound {
        /// Script identifier.
        script_id: String,
        /// Exported function name.
        function_name: String,
    },
    /// A host contract requires a bridge capability that the current worker cannot provide.
    #[error(
        "host contract `{contract_name}` requires unsupported worker bridge mode: {execution:?}"
    )]
    UnsupportedHostBridge {
        /// Host contract stable name.
        contract_name: String,
        /// Required execution mode.
        execution: HostFunctionExecution,
    },
    /// A value crossing the host bridge does not match its registered contract schema.
    #[error("host contract `{contract_name}` {direction} validation failed: {details}")]
    ContractValidation {
        /// Host contract stable name.
        contract_name: String,
        /// Bridge direction being validated.
        direction: &'static str,
        /// Validation failure details.
        details: String,
    },
    /// TypeScript transpilation failed.
    #[error("typescript transpilation failed: {details}")]
    Transpile {
        /// Diagnostic text emitted by the compiler.
        details: String,
    },
    /// Project module resolution failed.
    #[error("module resolution failed: {details}")]
    Resolve {
        /// Resolution failure details.
        details: String,
    },
    /// JavaScript execution failed.
    #[error("javascript execution failed: {details}")]
    Execution {
        /// Runtime error details returned by QuickJS.
        details: String,
    },
    /// JSON serialization or deserialization failed.
    #[error("json conversion failed: {0}")]
    Json(#[from] serde_json::Error),
    /// Filesystem operation failed.
    #[error("filesystem error: {0}")]
    Io(#[from] io::Error),
}
