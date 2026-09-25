use std::collections::HashMap;

use crate::compiler::ModuleOrigin;

#[derive(Debug, Default)]
pub(super) struct ModuleStoreInner {
    pub(super) sources: HashMap<String, String>,
    pub(super) resolutions: HashMap<(String, String), String>,
    /// TypeScript origin of each script module, by runtime module id; host modules
    /// have none.
    pub(super) origins: HashMap<String, ModuleOrigin>,
}
