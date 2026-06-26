//! Registry interfaces and in-memory V0 implementations.

mod active_runtime;
mod host_contracts;
mod scripts;

pub use active_runtime::{
    ActiveEventBinding, ActiveRuntimeRegistry, InMemoryActiveRuntimeRegistry,
};
pub(crate) use active_runtime::{ActiveRuntimeScriptSnapshot, ActiveRuntimeSnapshot};
pub use host_contracts::{
    HostContractRegistry, InMemoryHostContractRegistry,
    render_typescript_declarations_for_descriptors, render_typescript_sdk_for_descriptors,
};
pub use scripts::{
    InMemoryScriptRegistry, ScriptMaterializationState, ScriptRegistry, ScriptRegistryEntry,
};
