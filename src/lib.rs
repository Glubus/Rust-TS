//! Lightweight embedded TypeScript VM built on top of `oxc` and `rquickjs`.

#![deny(clippy::correctness)]
#![warn(clippy::suspicious)]
#![warn(clippy::style)]
#![warn(clippy::complexity)]
#![warn(clippy::perf)]
#![warn(clippy::undocumented_unsafe_blocks)]
#![warn(missing_docs)]

mod api;
mod cache;
mod compiler;
mod config;
mod contract;
mod error;
mod latency_metrics;
mod manager;
mod queue_metrics;
mod registry;
mod runner;
mod sdk_files;
mod types;

pub use api::TsVm;
pub use config::{VmContractValidation, VmOptions, VmUnknownFieldValidation};
#[cfg(feature = "tokio")]
pub use contract::AsyncHostFunction;
pub use contract::{
    DeliveryMode, HostCallback, HostCallbackDescriptor, HostContext, HostContract, HostContractAbi,
    HostContractDescriptor, HostContractKind, HostFunction, HostFunctionDescriptor,
    HostFunctionExecution, HostMetadata, Schema, TsEnumVariant, TsField, TsLiteral, TsRecordKey,
    TsSchema, TsType, push_schema_dependency, schema_type_ref,
};
pub use error::VmError;
#[cfg(feature = "async-promise")]
pub use manager::AsyncManagedScript;
pub use registry::{
    HostContractRegistry, InMemoryHostContractRegistry, ScriptMaterializationState,
    ScriptRegistryEntry, render_typescript_declarations_for_descriptors,
    render_typescript_sdk_for_descriptors,
};
#[cfg(feature = "async-promise")]
pub use runner::async_host_bridge::install_async_host_bridge;
#[cfg(feature = "async-promise")]
pub use runner::async_script_runtime::{AsyncLoadedScript, AsyncScriptRuntime};
pub use sdk_files::{
    GeneratedSdkFiles, SdkFileNames, write_host_sdk_files, write_host_sdk_files_with_names,
};
#[cfg(feature = "derive")]
pub use ts_embed_vm_macros::TsSchema;
pub use types::{
    RuntimeDependencyEdge, RuntimeEventBinding, RuntimeEventRoute, RuntimeExecutionLane,
    RuntimeMaterializationState, RuntimeModuleDependency, RuntimeRetentionStats, RuntimeScriptView,
    ScriptId, ScriptRetentionPolicy, ScriptSnapshot, ScriptSourceKind, VmEvent,
    VmLatencyHistogramBucket, VmLatencyStats, VmMemoryPressureAlert, VmMemoryPressureThresholds,
    VmMemoryStats, VmProcessMemoryStats, VmQuickJsMemoryStats, VmRuntimeSnapshot, VmStats,
    VmSubscription, VmWorkerStats, WorkerId,
};
