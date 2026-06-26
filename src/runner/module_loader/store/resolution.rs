use rquickjs::{Error, Result as JsResult};

use super::inner::ModuleStoreInner;

pub(super) fn resolve_from_store(
    store: &ModuleStoreInner,
    base: &str,
    name: &str,
) -> JsResult<String> {
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
    store
        .sources
        .get(name)
        .cloned()
        .ok_or_else(|| Error::new_loading(name))
}
