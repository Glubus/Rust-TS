//! QuickJS loader and resolver adapters backed by the worker module store.

use rquickjs::loader::{ImportAttributes, Loader, Resolver};
use rquickjs::{Ctx, Module, Result as JsResult};

use super::store::WorkerModuleStore;

#[derive(Debug, Clone)]
pub(crate) struct MemoryModuleResolver {
    store: WorkerModuleStore,
}

impl MemoryModuleResolver {
    pub(crate) fn new(store: WorkerModuleStore) -> Self {
        Self { store }
    }
}

impl Resolver for MemoryModuleResolver {
    fn resolve<'js>(
        &mut self,
        _ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> JsResult<String> {
        self.store.resolve(base, name)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MemoryModuleLoader {
    store: WorkerModuleStore,
}

impl MemoryModuleLoader {
    pub(crate) fn new(store: WorkerModuleStore) -> Self {
        Self { store }
    }
}

impl Loader for MemoryModuleLoader {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> JsResult<Module<'js>> {
        self.store.load(ctx, name)
    }
}
