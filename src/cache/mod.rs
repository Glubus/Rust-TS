//! Disk cache and cache keys for transpiled JavaScript modules.

mod identity;
mod store;

pub(crate) use identity::module_cache_key;
pub(crate) use store::ScriptCache;
