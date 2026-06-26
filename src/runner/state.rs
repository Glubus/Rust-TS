//! Mutable state owned by one worker thread.

use std::sync::Arc;

use rquickjs::Runtime;

use crate::config::VmOptions;
use crate::registry::InMemoryHostContractRegistry;
use crate::types::WorkerId;

use super::bridge_capability::WorkerBridgeCapability;
use super::module_loader::{MemoryModuleLoader, MemoryModuleResolver, WorkerModuleStore};
use super::script_store::LoadedScriptMap;

pub(crate) struct WorkerState {
    pub(crate) worker_id: WorkerId,
    pub(crate) options: VmOptions,
    pub(crate) runtime: Runtime,
    pub(crate) bridge_capability: WorkerBridgeCapability,
    pub(crate) module_store: WorkerModuleStore,
    pub(crate) scripts: LoadedScriptMap,
    pub(crate) host_registry: Arc<InMemoryHostContractRegistry>,
    next_module_graph_id: u64,
}

impl WorkerState {
    pub(crate) fn new(
        worker_id: WorkerId,
        options: VmOptions,
        host_registry: Arc<InMemoryHostContractRegistry>,
    ) -> Result<Self, crate::error::VmError> {
        let runtime = Runtime::new().map_err(super::errors::js_error)?;
        runtime.set_memory_limit(options.memory_limit_bytes);
        runtime.set_max_stack_size(options.max_stack_size_bytes);
        let module_store = WorkerModuleStore::default();
        runtime.set_loader(
            MemoryModuleResolver::new(module_store.clone()),
            MemoryModuleLoader::new(module_store.clone()),
        );

        Ok(Self {
            worker_id,
            runtime,
            bridge_capability: WorkerBridgeCapability::Sync,
            module_store,
            scripts: LoadedScriptMap::with_capacity(options.max_scripts_per_worker),
            options,
            host_registry,
            next_module_graph_id: 0,
        })
    }

    pub(crate) fn next_module_graph_id(&mut self) -> u64 {
        let graph_id = self.next_module_graph_id;
        self.next_module_graph_id += 1;
        graph_id
    }
}
