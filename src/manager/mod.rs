//! Central script manager and orchestration facade.

#[cfg(feature = "tokio")]
mod async_api;
#[cfg(feature = "async-promise")]
mod async_promise;
#[cfg(feature = "async-promise")]
mod async_worker_pool;
mod bootstrap;
mod calls;
mod dispatch;
mod event_bus;
mod events;
mod introspection;
mod lifecycle;
mod metrics;
mod operations;
mod prepare;
mod process_memory;
mod script_manager;
mod worker_pool;

#[cfg(feature = "async-promise")]
pub use async_promise::AsyncManagedScript;
pub use script_manager::ScriptManager;
