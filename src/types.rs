//! Shared public types.

use serde::{Deserialize, Serialize};

/// Stable identifier for a loaded script.
pub type ScriptId = String;

/// QuickJS memory counters for one [`Engine`](crate::Engine).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryStats {
    /// Total bytes allocated by the QuickJS allocator.
    pub malloc_size_bytes: u64,
    /// QuickJS allocator limit in bytes, or zero when unlimited.
    pub malloc_limit_bytes: u64,
    /// Bytes currently used by live QuickJS values and runtime structures.
    pub memory_used_bytes: u64,
    /// Allocation count reported by QuickJS.
    pub malloc_count: u64,
    /// Live atom count reported by QuickJS.
    pub atom_count: u64,
    /// Live string count reported by QuickJS.
    pub string_count: u64,
    /// Live object count reported by QuickJS.
    pub object_count: u64,
    /// JavaScript function count reported by QuickJS.
    pub function_count: u64,
}
