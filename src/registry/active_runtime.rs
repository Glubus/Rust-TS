//! Registry for mounted instances and runner affinity.

mod entry;
mod interface;
mod memory;
mod routes;

#[cfg(test)]
mod tests;

pub use interface::ActiveRuntimeRegistry;
pub use memory::InMemoryActiveRuntimeRegistry;
pub(crate) use memory::{ActiveRuntimeScriptSnapshot, ActiveRuntimeSnapshot};
pub use routes::ActiveEventBinding;
