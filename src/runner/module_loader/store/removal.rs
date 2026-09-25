use super::inner::ModuleStoreInner;

/// Removes a script's module graph. Only the graph's own module ids are touched: host
/// modules live under bare names that a script id may coincide with.
pub(super) fn remove_modules(store: &mut ModuleStoreInner, module_ids: &[String]) {
    for module_id in module_ids {
        remove_module(store, module_id);
    }
}

fn remove_module(store: &mut ModuleStoreInner, module_id: &str) {
    store.sources.remove(module_id);
    store.origins.remove(module_id);
    store
        .resolutions
        .retain(|(base, _), resolved| base != module_id && resolved != module_id);
}
