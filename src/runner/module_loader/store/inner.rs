use std::collections::{HashMap, HashSet};

use crate::compiler::ModuleOrigin;

#[derive(Debug, Default)]
pub(super) struct ModuleStoreInner {
    pub(super) sources: HashMap<String, String>,
    /// Host module sources for scripts in a context group, which import the script's
    /// own environment instead of reading globals.
    pub(super) group_host_sources: HashMap<String, String>,
    /// Graphs loaded into a context group.
    pub(super) grouped_graphs: HashSet<u64>,
    pub(super) resolutions: HashMap<(String, String), String>,
    /// TypeScript origin of each script module, by runtime module id; host modules
    /// have none.
    pub(super) origins: HashMap<String, ModuleOrigin>,
}
