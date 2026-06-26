//! Minimal host context contract trait.

use super::HostContract;

/// Minimal declarative host context contract.
///
/// In V0, a context is contract metadata only: schema, ABI seed input,
/// declaration output, and optional generated SDK typing. It does not install a
/// mutable Rust object graph into every QuickJS context and does not create a
/// host-call bridge by itself.
pub trait HostContext: HostContract {}
