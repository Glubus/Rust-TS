//! In-memory module source and resolution store.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use rquickjs::{Ctx, Error, Module, Result as JsResult};

use crate::compiler::CompiledModule;
use crate::error::VmError;

use super::graph::RuntimeModuleGraph;

mod inner;
mod insertion;
mod removal;
mod resolution;

#[cfg(test)]
mod tests;

use inner::ModuleStoreInner;

#[derive(Debug, Clone, Default)]
pub(crate) struct WorkerModuleStore {
    inner: Arc<Mutex<ModuleStoreInner>>,
}

impl WorkerModuleStore {
    pub(crate) fn insert_host_modules(
        &self,
        modules: BTreeMap<String, String>,
    ) -> std::result::Result<(), VmError> {
        let mut guard = self.lock_store()?;
        insertion::insert_host_modules(&mut guard, modules);
        Ok(())
    }

    pub(crate) fn insert_inline(
        &self,
        script_id: &str,
        source: String,
        graph_id: u64,
    ) -> std::result::Result<RuntimeModuleGraph, VmError> {
        let mut guard = self.lock_store()?;
        Ok(insertion::insert_inline(
            &mut guard, script_id, source, graph_id,
        ))
    }

    pub(crate) fn insert_project(
        &self,
        entry_module_id: &str,
        modules: Vec<CompiledModule>,
        graph_id: u64,
    ) -> std::result::Result<RuntimeModuleGraph, VmError> {
        let mut guard = self.lock_store()?;
        insertion::insert_project(&mut guard, entry_module_id, modules, graph_id)
    }

    pub(crate) fn remove_modules(&self, module_ids: &[String]) -> std::result::Result<(), VmError> {
        let mut guard = self.lock_store()?;
        removal::remove_modules(&mut guard, module_ids);
        Ok(())
    }

    pub(super) fn resolve(&self, base: &str, name: &str) -> JsResult<String> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| Error::new_resolving_message(base, name, "module store lock poisoned"))?;
        resolution::resolve_from_store(&guard, base, name)
    }

    pub(super) fn load<'js>(&self, ctx: &Ctx<'js>, name: &str) -> JsResult<Module<'js>> {
        let source = {
            let guard = self
                .inner
                .lock()
                .map_err(|_| Error::new_loading_message(name, "module store lock poisoned"))?;
            resolution::source_for_load(&guard, name)?
        };

        Module::declare(ctx.clone(), name, source)
    }

    fn lock_store(
        &self,
    ) -> std::result::Result<std::sync::MutexGuard<'_, ModuleStoreInner>, VmError> {
        self.inner.lock().map_err(|_| VmError::LockPoisoned)
    }
}
