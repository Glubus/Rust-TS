use super::inner::ModuleStoreInner;

pub(super) fn remove_script_modules(
    store: &mut ModuleStoreInner,
    script_id: &str,
    module_ids: &[String],
) {
    store.sources.remove(script_id);
    for module_id in module_ids {
        remove_module(store, module_id);
    }
}

fn remove_module(store: &mut ModuleStoreInner, module_id: &str) {
    store.sources.remove(module_id);
    store
        .resolutions
        .retain(|(base, _), resolved| base != module_id && resolved != module_id);
}
