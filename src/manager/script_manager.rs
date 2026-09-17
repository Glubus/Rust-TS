//! Shared manager state types.

use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::cache::ScriptCache;
use crate::compiler::CompilerService;
use crate::config::VmOptions;
use crate::registry::{
    InMemoryActiveRuntimeRegistry, InMemoryHostContractRegistry, InMemoryScriptRegistry,
};
use crate::runner::WorkerHandle;

#[cfg(feature = "async-promise")]
use super::async_worker_pool::AsyncWorkerPool;
use super::event_bus::EventBus;
use super::metrics::RuntimeMetrics;

/// Long-lived TypeScript VM backed by a pool of QuickJS runtimes.
#[derive(Clone)]
pub struct ScriptManager {
    pub(super) inner: Arc<Inner>,
}

pub(super) struct Inner {
    #[cfg(feature = "async-promise")]
    pub(super) async_workers: AsyncWorkerPool,
    #[cfg(feature = "async-promise")]
    pub(super) async_worker_joins: Mutex<Option<Vec<JoinHandle<()>>>>,
    pub(super) options: VmOptions,
    pub(super) workers: Vec<WorkerHandle>,
    pub(super) worker_joins: Mutex<Option<Vec<JoinHandle<()>>>>,
    pub(super) next_worker: AtomicUsize,
    pub(super) next_oneshot_script: AtomicUsize,
    pub(super) is_shutdown: AtomicBool,
    pub(super) shutdown_complete: AtomicBool,
    pub(super) event_bus: EventBus,
    pub(super) metrics: RuntimeMetrics,
    pub(super) cache: ScriptCache,
    pub(super) script_load_lock: Mutex<()>,
    #[allow(dead_code)]
    pub(super) compiler: Mutex<CompilerService>,
    pub(super) script_registry: InMemoryScriptRegistry,
    pub(super) host_contract_registry: Arc<InMemoryHostContractRegistry>,
    pub(super) active_runtime_registry: InMemoryActiveRuntimeRegistry,
}
