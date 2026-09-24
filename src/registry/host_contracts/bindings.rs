use std::collections::HashMap;
#[cfg(feature = "async-promise")]
use std::future::{Future, ready};
use std::marker::PhantomData;
#[cfg(feature = "async-promise")]
use std::pin::Pin;
#[cfg(feature = "tokio")]
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use rquickjs::{Ctx, Result as JsResult, Value as JsValue};
use serde_json::Value;

#[cfg(feature = "tokio")]
use crate::contract::AsyncHostFunction;
use crate::contract::{HostFunction, JsDecode, JsEncode, js_value_to_json, json_to_js_value};
use crate::error::VmError;

#[cfg(feature = "async-promise")]
type HostFunctionFuture = Pin<Box<dyn Future<Output = Result<Value, VmError>> + Send + 'static>>;
#[cfg(feature = "async-promise")]
type OptionalHostFunctionFuture =
    Pin<Box<dyn Future<Output = Result<Option<Value>, VmError>> + Send + 'static>>;

type NamedBinding = (String, Arc<dyn HostFunctionBinding>);

trait HostFunctionBinding: Send + Sync {
    fn call_value(&self, input: Value) -> Result<Value, VmError>;

    fn call_js_value<'js>(&self, ctx: &Ctx<'js>, input: JsValue<'js>) -> JsResult<JsValue<'js>> {
        let input = js_value_to_json(ctx, input)?;
        let output = self.call_value(input).map_err(js_host_error)?;
        json_to_js_value(ctx, &output)
    }

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

struct TypedStaticHostFunctionBinding<T> {
    marker: PhantomData<T>,
}

impl<T> TypedStaticHostFunctionBinding<T> {
    fn new() -> Self {
        Self {
            marker: PhantomData,
        }
    }
}

impl<T> HostFunctionBinding for TypedStaticHostFunctionBinding<T>
where
    T: HostFunction + Send + Sync + 'static,
    T::Input: JsDecode,
    T::Output: JsEncode,
{
    fn call_value(&self, input: Value) -> Result<Value, VmError> {
        let input = serde_json::from_value::<T::Input>(input)?;
        let output = T::call(input)?;
        serde_json::to_value(output).map_err(VmError::from)
    }

    fn call_js_value<'js>(&self, ctx: &Ctx<'js>, input: JsValue<'js>) -> JsResult<JsValue<'js>> {
        let input = T::Input::decode_js(ctx, input)?;
        let output = T::call(input).map_err(js_host_error)?;
        output.encode_js(ctx)
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

fn js_host_error(error: impl ToString) -> rquickjs::Error {
    rquickjs::Error::new_from_js_message("host", "function", error.to_string())
}

#[cfg(test)]
mod lock_tests {
    use super::*;

    struct ChecksRegistryLock(std::sync::Weak<FunctionBindingStore>);
    impl HostFunctionBinding for ChecksRegistryLock {
        fn call_value(&self, _: Value) -> Result<Value, VmError> {
            let store = self.0.upgrade().unwrap();
            let mut registry = store
                .by_name
                .try_lock()
                .expect("handler must be able to register another binding");
            registry.insert(
                "nested".into(),
                Arc::new(ChecksRegistryLock(self.0.clone())),
            );
            Ok(Value::Null)
        }
        #[cfg(feature = "async-promise")]
        fn call_value_async(&self, input: Value) -> HostFunctionFuture {
            Box::pin(ready(self.call_value(input)))
        }
    }

    #[test]
    fn json_handler_runs_without_registry_lock() {
        let store = Arc::new(FunctionBindingStore::default());
        store.by_name.lock().unwrap().insert(
            "test".into(),
            Arc::new(ChecksRegistryLock(Arc::downgrade(&store))),
        );
        assert_eq!(
            store.invoke("test", Value::Null).unwrap(),
            Some(Value::Null)
        );
        assert!(store.by_name.lock().unwrap().contains_key("nested"));
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

    pub(super) fn insert_typed_static<T>(&self) -> Result<(), VmError>
    where
        T: HostFunction + Send + Sync + 'static,
        T::Input: JsDecode,
        T::Output: JsEncode,
    {
        let mut guard = self.by_name.lock().map_err(|_| VmError::WorkerPanicked)?;
        guard.insert(
            T::NAME.to_owned(),
            Arc::new(TypedStaticHostFunctionBinding::<T>::new()),
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
        let binding = {
            let guard = self.by_name.lock().map_err(|_| VmError::WorkerPanicked)?;
            guard.get(name).cloned()
        };
        let Some(binding) = binding else {
            return Ok(None);
        };
        binding.call_value(input).map(Some)
    }

    pub(super) fn invoke_js<'js>(
        &self,
        ctx: &Ctx<'js>,
        name: &str,
        input: JsValue<'js>,
    ) -> JsResult<Option<JsValue<'js>>> {
        let binding = {
            let guard = self
                .by_name
                .lock()
                .map_err(|_| js_host_error(VmError::WorkerPanicked))?;
            guard.get(name).cloned()
        };
        let Some(binding) = binding else {
            return Ok(None);
        };
        binding.call_js_value(ctx, input).map(Some)
    }

    pub(super) fn names(&self) -> Result<Vec<String>, VmError> {
        let guard = self.by_name.lock().map_err(|_| VmError::WorkerPanicked)?;
        Ok(guard.keys().cloned().collect())
    }

    /// Sets one native QuickJS function per binding on `target`. Each function owns its
    /// binding, so a call performs no name lookup and takes no lock.
    pub(super) fn install_native<'js>(&self, target: &rquickjs::Object<'js>) -> JsResult<()> {
        for (name, binding) in self.snapshot().map_err(js_host_error)? {
            target.set(
                name,
                rquickjs::prelude::Func::from(
                    move |ctx: Ctx<'js>, input: rquickjs::function::Opt<JsValue<'js>>| {
                        binding.call_js_value(&ctx, input_or_null(&ctx, input))
                    },
                ),
            )?;
        }
        Ok(())
    }

    fn snapshot(&self) -> Result<Vec<NamedBinding>, VmError> {
        let guard = self.by_name.lock().map_err(|_| VmError::WorkerPanicked)?;
        Ok(guard
            .iter()
            .map(|(name, binding)| (name.clone(), binding.clone()))
            .collect())
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

/// A host function called without an argument receives `null`, like the bridge path.
pub(super) fn input_or_null<'js>(
    ctx: &Ctx<'js>,
    input: rquickjs::function::Opt<JsValue<'js>>,
) -> JsValue<'js> {
    input.0.unwrap_or_else(|| JsValue::new_null(ctx.clone()))
}
