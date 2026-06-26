use std::collections::HashMap;
#[cfg(feature = "async-promise")]
use std::future::{Future, ready};
use std::marker::PhantomData;
#[cfg(feature = "async-promise")]
use std::pin::Pin;
#[cfg(feature = "tokio")]
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use serde_json::Value;

#[cfg(feature = "tokio")]
use crate::contract::AsyncHostFunction;
use crate::contract::HostFunction;
use crate::error::VmError;

#[cfg(feature = "async-promise")]
type HostFunctionFuture = Pin<Box<dyn Future<Output = Result<Value, VmError>> + Send + 'static>>;
#[cfg(feature = "async-promise")]
type OptionalHostFunctionFuture =
    Pin<Box<dyn Future<Output = Result<Option<Value>, VmError>> + Send + 'static>>;

trait HostFunctionBinding: Send + Sync {
    fn call_value(&self, input: Value) -> Result<Value, VmError>;

    #[cfg(feature = "async-promise")]
    fn call_value_async(&self, input: Value) -> HostFunctionFuture;
}

struct StaticHostFunctionBinding<T> {
    marker: PhantomData<T>,
}

impl<T> StaticHostFunctionBinding<T> {
    fn new() -> Self {
        Self {
            marker: PhantomData,
        }
    }
}

impl<T> HostFunctionBinding for StaticHostFunctionBinding<T>
where
    T: HostFunction + Send + Sync + 'static,
{
    fn call_value(&self, input: Value) -> Result<Value, VmError> {
        let input = serde_json::from_value::<T::Input>(input)?;
        let output = T::call(input)?;
        serde_json::to_value(output).map_err(VmError::from)
    }

    #[cfg(feature = "async-promise")]
    fn call_value_async(&self, input: Value) -> HostFunctionFuture {
        Box::pin(ready(self.call_value(input)))
    }
}

#[cfg(feature = "tokio")]
struct AsyncStaticHostFunctionBinding<T> {
    handle: tokio::runtime::Handle,
    marker: PhantomData<T>,
}

#[cfg(feature = "tokio")]
impl<T> AsyncStaticHostFunctionBinding<T> {
    fn new(handle: tokio::runtime::Handle) -> Self {
        Self {
            handle,
            marker: PhantomData,
        }
    }
}

#[cfg(feature = "tokio")]
impl<T> HostFunctionBinding for AsyncStaticHostFunctionBinding<T>
where
    T: AsyncHostFunction + Send + Sync + 'static,
{
    fn call_value(&self, input: Value) -> Result<Value, VmError> {
        let input = serde_json::from_value::<T::Input>(input)?;
        let output = self.call_on_runtime(input)?;
        serde_json::to_value(output).map_err(VmError::from)
    }

    #[cfg(feature = "async-promise")]
    fn call_value_async(&self, input: Value) -> HostFunctionFuture {
        let input = match serde_json::from_value::<T::Input>(input) {
            Ok(input) => input,
            Err(error) => return Box::pin(ready(Err(VmError::from(error)))),
        };
        let handle = self.handle.clone();
        Box::pin(async move {
            let output = handle
                .spawn(T::call_async(input))
                .await
                .map_err(async_task_error)??;
            serde_json::to_value(output).map_err(VmError::from)
        })
    }
}

#[cfg(feature = "tokio")]
impl<T> AsyncStaticHostFunctionBinding<T>
where
    T: AsyncHostFunction + Send + Sync + 'static,
{
    fn call_on_runtime(&self, input: T::Input) -> Result<T::Output, VmError> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.handle.spawn(async move {
            let _ = tx.send(T::call_async(input).await);
        });
        rx.recv().map_err(|_| VmError::WorkerOffline)?
    }
}

#[cfg(feature = "async-promise")]
fn async_task_error(error: tokio::task::JoinError) -> VmError {
    VmError::Execution {
        details: format!("async host function task failed: {error}"),
    }
}

#[derive(Default)]
pub(super) struct FunctionBindingStore {
    by_name: Mutex<HashMap<String, Arc<dyn HostFunctionBinding>>>,
}

impl FunctionBindingStore {
    pub(super) fn insert_static<T>(&self) -> Result<(), VmError>
    where
        T: HostFunction + Send + Sync + 'static,
    {
        let mut guard = self.by_name.lock().map_err(|_| VmError::WorkerPanicked)?;
        guard.insert(
            T::NAME.to_owned(),
            Arc::new(StaticHostFunctionBinding::<T>::new()),
        );
        Ok(())
    }

    #[cfg(feature = "tokio")]
    pub(super) fn insert_async_static<T>(
        &self,
        handle: tokio::runtime::Handle,
    ) -> Result<(), VmError>
    where
        T: AsyncHostFunction + Send + Sync + 'static,
    {
        let mut guard = self.by_name.lock().map_err(|_| VmError::WorkerPanicked)?;
        guard.insert(
            T::NAME.to_owned(),
            Arc::new(AsyncStaticHostFunctionBinding::<T>::new(handle)),
        );
        Ok(())
    }

    pub(super) fn invoke(&self, name: &str, input: Value) -> Result<Option<Value>, VmError> {
        let guard = self.by_name.lock().map_err(|_| VmError::WorkerPanicked)?;
        let Some(binding) = guard.get(name).cloned() else {
            return Ok(None);
        };
        binding.call_value(input).map(Some)
    }

    #[cfg(feature = "async-promise")]
    pub(super) fn invoke_async(&self, name: String, input: Value) -> OptionalHostFunctionFuture {
        let binding = {
            let guard = match self.by_name.lock() {
                Ok(guard) => guard,
                Err(_) => return Box::pin(ready(Err(VmError::WorkerPanicked))),
            };
            guard.get(&name).cloned()
        };

        let Some(binding) = binding else {
            return Box::pin(ready(Ok(None)));
        };
        Box::pin(async move { binding.call_value_async(input).await.map(Some) })
    }
}
