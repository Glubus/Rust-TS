//! Registry for Rust-declared host contracts.

mod bindings;
mod declarations;
mod import_modules;
mod interface;
mod memory;
mod sdk;

#[cfg(test)]
mod tests;

pub(crate) use import_modules::HostModuleStyle;
pub use interface::HostContractRegistry;
pub use memory::InMemoryHostContractRegistry;

use crate::contract::HostContractDescriptor;

/// Renders TypeScript declarations from host contract descriptors.
#[must_use]
pub fn render_typescript_declarations_for_descriptors(
    descriptors: &[HostContractDescriptor],
) -> String {
    declarations::render_typescript_declarations(descriptors)
}

/// Renders TypeScript SDK source from host contract descriptors.
#[must_use]
pub fn render_typescript_sdk_for_descriptors(descriptors: &[HostContractDescriptor]) -> String {
    sdk::render_typescript_sdk(descriptors)
}
