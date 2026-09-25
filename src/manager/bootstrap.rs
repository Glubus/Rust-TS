//! Manager bootstrap, public accessors and teardown.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[cfg(feature = "async-promise")]
use super::async_worker_pool::AsyncWorkerPool;
use super::event_bus::EventBus;
use super::metrics::RuntimeMetrics;
use super::script_manager::{Inner, ScriptManager};
use super::worker_pool::{resolve_worker_count, start_workers};
use crate::cache::ScriptCache;
use crate::compiler::CompilerService;
use crate::config::VmOptions;
use crate::error::VmError;
use crate::registry::{
    InMemoryActiveRuntimeRegistry, InMemoryHostContractRegistry, InMemoryScriptRegistry,
};
use crate::types::{VmEvent, VmSubscription};

impl ScriptManager {
    /// Starts the script manager and its worker pool.
    pub fn new(options: VmOptions) -> Result<Self, VmError> {
        let cache = ScriptCache::new(options.cache_dir.clone())?;
        let worker_count = resolve_worker_count(options.worker_threads)?;
        let latency_histograms = options.latency_histograms;
        let event_queue_capacity = options.event_queue_capacity;
        let host_contract_registry =
            Arc::new(InMemoryHostContractRegistry::with_validation_options(
                options.contract_validation,
                options.unknown_field_validation,
            ));
        let (workers, worker_joins) =
            start_workers(worker_count, &options, host_contract_registry.clone())?;
        #[cfg(feature = "async-promise")]
        let (async_workers, async_worker_joins) =
            AsyncWorkerPool::start(worker_count, &options, host_contract_registry.clone())?;

        Ok(Self {
            inner: Arc::new(Inner {
                #[cfg(feature = "async-promise")]
                async_workers,
                #[cfg(feature = "async-promise")]
                async_worker_joins: Mutex::new(Some(async_worker_joins)),
                options,
                workers,
                worker_joins: Mutex::new(Some(worker_joins)),
                next_worker: AtomicUsize::new(0),
                next_oneshot_script: AtomicUsize::new(0),
                is_shutdown: AtomicBool::new(false),
                shutdown_complete: AtomicBool::new(false),
                event_bus: EventBus::new(event_queue_capacity),
                metrics: RuntimeMetrics::new(latency_histograms),
                cache,
                script_lifecycle_lock: Mutex::new(()),
                compiler: Mutex::new(CompilerService::default()),
                script_registry: InMemoryScriptRegistry::new(),
                host_contract_registry,
                active_runtime_registry: InMemoryActiveRuntimeRegistry::new(),
            }),
        })
    }

    /// Returns the host contract registry owned by the manager.
    pub fn registry(&self) -> &InMemoryHostContractRegistry {
        self.inner.host_contract_registry.as_ref()
    }

    /// Creates one event subscription for VM lifecycle and function-call events.
    #[must_use]
    pub fn subscribe(&self) -> VmSubscription {
        self.inner.event_bus.subscribe()
    }

    /// Interrupts JavaScript and stops all workers, bypassing full command queues.
    /// Returns `ShutdownTimeout` if a host handler has not returned; safe to retry.
    pub fn shutdown(&self) -> Result<(), VmError> {
        self.inner.is_shutdown.store(true, Ordering::SeqCst);
        for worker in &self.inner.workers {
            worker.control.stop();
            let _ = self.dispatch_shutdown(worker.id);
        }
        #[cfg(feature = "async-promise")]
        self.inner.async_workers.shutdown()?;
        let sync_result = self.join_workers();
        #[cfg(feature = "async-promise")]
        let async_result = self.join_async_workers();
        sync_result?;
        #[cfg(feature = "async-promise")]
        async_result?;
        if !self.inner.shutdown_complete.swap(true, Ordering::SeqCst) {
            self.inner.event_bus.publish(VmEvent::Shutdown);
        }
        Ok(())
    }
}

#[cfg(feature = "async-promise")]
impl ScriptManager {
    fn join_async_workers(&self) -> Result<(), VmError> {
        let mut joins = self
            .inner
            .async_worker_joins
            .lock()
            .map_err(|_| VmError::WorkerPanicked)?;
        super::worker_pool::join_with_timeout(&mut joins, self.inner.options.shutdown_timeout)
    }
}

impl Drop for ScriptManager {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) != 1 {
            return;
        }
        let _ = self.shutdown();
    }
}
