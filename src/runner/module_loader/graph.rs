//! Runtime-scoped ESM graph identifiers.

use std::collections::HashMap;

use crate::compiler::CompiledModule;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeModuleGraph {
    pub(crate) entry_module_id: String,
    pub(crate) module_ids: Vec<String>,
}

pub(super) fn build_runtime_module_id_map(
    graph_id: u64,
    modules: &[CompiledModule],
) -> HashMap<String, String> {
    modules
        .iter()
        .map(|module| {
            (
                module.module_id.clone(),
                runtime_module_id(graph_id, &module.module_id),
            )
        })
        .collect()
}

pub(super) fn runtime_module_id(graph_id: u64, module_id: &str) -> String {
    format!("tsvm://graph/{graph_id}/{module_id}")
}
