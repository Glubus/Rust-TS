use std::collections::HashMap;

#[derive(Debug, Default)]
pub(super) struct ModuleStoreInner {
    pub(super) sources: HashMap<String, String>,
    pub(super) resolutions: HashMap<(String, String), String>,
}
