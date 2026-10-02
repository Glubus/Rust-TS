//! Host contract registry.

mod host_contracts;

pub(crate) use host_contracts::HostModuleStyle;

pub use host_contracts::{
    HostContractRegistry, InMemoryHostContractRegistry,
    render_typescript_declarations_for_descriptors, render_typescript_sdk_for_descriptors,
};
