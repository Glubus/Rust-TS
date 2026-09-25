//! Host contract registry.

mod host_contracts;

pub use host_contracts::{
    HostContractRegistry, InMemoryHostContractRegistry,
    render_typescript_declarations_for_descriptors, render_typescript_sdk_for_descriptors,
};
