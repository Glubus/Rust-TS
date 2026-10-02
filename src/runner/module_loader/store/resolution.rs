use rquickjs::{Error, Result as JsResult};

use super::inner::ModuleStoreInner;
use crate::runner::module_loader::graph::{
    ENV_SPECIFIER, env_module_id, graph_of, host_instance_id, host_instance_module,
};

pub(super) fn resolve_from_store(
    store: &ModuleStoreInner,
    base: &str,
    name: &str,
) -> JsResult<String> {
    // A script in a context group reaches its own environment and its own instance of
    // each host module, since the context has no per-script globals to read.
    if let Some(graph_id) = graph_of(base).filter(|id| store.grouped_graphs.contains(id)) {
        if name == ENV_SPECIFIER {
            return Ok(env_module_id(graph_id));
        }
        if store.group_host_sources.contains_key(name) {
            return Ok(host_instance_id(graph_id, name));
        }
    }

    if store.sources.contains_key(name) {
        return Ok(name.to_owned());
    }

    store
        .resolutions
        .get(&(base.to_owned(), name.to_owned()))
        .cloned()
        .ok_or_else(|| Error::new_resolving(base, name))
}

pub(super) fn source_for_load(store: &ModuleStoreInner, name: &str) -> JsResult<String> {
    if let Some(host_module) = host_instance_module(name) {
        return store
            .group_host_sources
            .get(host_module)
            .cloned()
            .ok_or_else(|| Error::new_loading(name));
    }
    store
        .sources
        .get(name)
        .cloned()
        .ok_or_else(|| Error::new_loading(name))
}
