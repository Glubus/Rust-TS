use std::collections::{BTreeMap, HashMap};

use crate::compiler::CompiledModule;
use crate::error::VmError;

use super::inner::ModuleStoreInner;
use crate::runner::module_loader::graph::{
    RuntimeModuleGraph, build_runtime_module_id_map, runtime_module_id,
};

pub(super) fn insert_inline(
    store: &mut ModuleStoreInner,
    script_id: &str,
    source: String,
    graph_id: u64,
) -> RuntimeModuleGraph {
    let module_id = runtime_module_id(graph_id, script_id);
    store.sources.insert(module_id.clone(), source);
    RuntimeModuleGraph {
        entry_module_id: module_id.clone(),
        module_ids: vec![module_id],
    }
}

pub(super) fn insert_project(
    store: &mut ModuleStoreInner,
    entry_module_id: &str,
    modules: Vec<CompiledModule>,
    graph_id: u64,
) -> Result<RuntimeModuleGraph, VmError> {
    let module_id_map = build_runtime_module_id_map(graph_id, &modules);
    let entry_runtime_id = runtime_entry_module_id(&module_id_map, entry_module_id)?;
    let module_ids = module_id_map.values().cloned().collect::<Vec<_>>();

    for module in modules {
        insert_project_module(store, module, &module_id_map)?;
    }

    Ok(RuntimeModuleGraph {
        entry_module_id: entry_runtime_id,
        module_ids,
    })
}

fn runtime_entry_module_id(
    module_id_map: &HashMap<String, String>,
    entry_module_id: &str,
) -> Result<String, VmError> {
    module_id_map
        .get(entry_module_id)
        .cloned()
        .ok_or_else(|| VmError::Resolve {
            details: format!("entry module is missing from compiled graph: {entry_module_id}"),
        })
}

fn insert_project_module(
    store: &mut ModuleStoreInner,
    module: CompiledModule,
    module_id_map: &HashMap<String, String>,
) -> Result<(), VmError> {
    let runtime_module_id = runtime_module_id_for(module_id_map, &module.module_id)?;
    insert_project_resolutions(
        store,
        &runtime_module_id,
        module.resolved_requests,
        module_id_map,
    )?;
    store
        .sources
        .insert(runtime_module_id, module.transpiled_js);
    Ok(())
}

fn insert_project_resolutions(
    store: &mut ModuleStoreInner,
    runtime_module_id: &str,
    resolved_requests: BTreeMap<String, String>,
    module_id_map: &HashMap<String, String>,
) -> Result<(), VmError> {
    for (request, resolved) in resolved_requests {
        let runtime_resolved_id = runtime_module_id_for(module_id_map, &resolved)?;
        store
            .resolutions
            .insert((runtime_module_id.to_owned(), request), runtime_resolved_id);
    }
    Ok(())
}

fn runtime_module_id_for(
    module_id_map: &HashMap<String, String>,
    module_id: &str,
) -> Result<String, VmError> {
    module_id_map
        .get(module_id)
        .cloned()
        .ok_or_else(|| VmError::Resolve {
            details: format!("module is missing from runtime graph: {module_id}"),
        })
}
