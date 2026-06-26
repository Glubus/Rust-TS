//! Disk cache and cache identity for transpiled JavaScript artifacts.

mod identity;
mod store;

pub(crate) use identity::CacheIdentity;
pub(crate) use store::ScriptCache;
