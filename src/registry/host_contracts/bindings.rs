use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

use rquickjs::function::Opt;
use rquickjs::prelude::Func;
use rquickjs::{Ctx, Object, Result as JsResult, Value as JsValue};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::contract::{
    Caller, HostFunctionSignature, JsDecode, JsEncode, js_value_to_json, json_to_js_value,
};
use crate::error::VmError;

type NamedBinding = (String, Arc<dyn HostFunctionBinding>);

/// A registered host function handler.
trait HostFunctionBinding: Send + Sync {
    /// Calls the handler with a JSON input, for the route that validates contracts.
    fn call_json(&self, caller: &Caller<'_>, input: Value) -> Result<Value, VmError>;

    /// Sets the handler on `target` as the native function `name` of script `script_id`.
    fn install<'js>(
        self: Arc<Self>,
        target: &Object<'js>,
        name: &str,
        script_id: &str,
    ) -> JsResult<()>;
}

/// How a host function's input and output cross between QuickJS and Rust.
pub(super) trait FunctionCodec<C: HostFunctionSignature>: 'static {
    /// Converts `input`, runs `handler` on it and converts its output back.
    fn call_native<'js>(
        ctx: &Ctx<'js>,
        input: JsValue<'js>,
        handler: impl FnOnce(C::Input) -> Result<C::Output, VmError>,
    ) -> JsResult<JsValue<'js>>;
}

/// Converts through `serde_json`.
pub(super) struct JsonCodec;

impl<C: HostFunctionSignature> FunctionCodec<C> for JsonCodec {
    fn call_native<'js>(
        ctx: &Ctx<'js>,
        input: JsValue<'js>,
        handler: impl FnOnce(C::Input) -> Result<C::Output, VmError>,
    ) -> JsResult<JsValue<'js>> {
        let input = js_value_to_json(ctx, input)?;
        let output = call_json(input, handler).map_err(js_host_error)?;
        json_to_js_value(ctx, &output)
    }
}

/// Converts natively through [`JsDecode`] and [`JsEncode`], without JSON.
pub(super) struct TypedCodec;

impl<C> FunctionCodec<C> for TypedCodec
where
    C: HostFunctionSignature,
    C::Input: JsDecode,
    C::Output: JsEncode,
{
    fn call_native<'js>(
        ctx: &Ctx<'js>,
        input: JsValue<'js>,
        handler: impl FnOnce(C::Input) -> Result<C::Output, VmError>,
    ) -> JsResult<JsValue<'js>> {
        let input = C::Input::decode_js(ctx, input)?;
        handler(input).map_err(js_host_error)?.encode_js(ctx)
    }
}

fn call_json<I, O>(
    input: Value,
    handler: impl FnOnce(I) -> Result<O, VmError>,
) -> Result<Value, VmError>
where
    I: DeserializeOwned,
    O: Serialize,
{
    let output = handler(serde_json::from_value(input)?)?;
    serde_json::to_value(output).map_err(VmError::from)
}

/// Handler of contract `C` that does not ask which script called it.
struct PlainBinding<C, K, F> {
    handler: F,
    marker: PhantomData<fn() -> (C, K)>,
}

impl<C, K, F> HostFunctionBinding for PlainBinding<C, K, F>
where
    C: HostFunctionSignature + 'static,
    K: FunctionCodec<C>,
    F: Fn(C::Input) -> Result<C::Output, VmError> + Send + Sync + 'static,
{
    fn call_json(&self, _caller: &Caller<'_>, input: Value) -> Result<Value, VmError> {
        call_json(input, &self.handler)
    }

    fn install<'js>(
        self: Arc<Self>,
        target: &Object<'js>,
        name: &str,
        _script_id: &str,
    ) -> JsResult<()> {
        target.set(
            name,
            Func::from(move |ctx: Ctx<'js>, input: Opt<JsValue<'js>>| {
                K::call_native(&ctx, input_or_null(&ctx, input), &self.handler)
            }),
        )
    }
}

/// Handler of contract `C` that receives the calling script as a [`Caller`].
struct CallerBinding<C, K, F> {
    handler: F,
    marker: PhantomData<fn() -> (C, K)>,
}

impl<C, K, F> HostFunctionBinding for CallerBinding<C, K, F>
where
    C: HostFunctionSignature + 'static,
    K: FunctionCodec<C>,
    F: Fn(&Caller<'_>, C::Input) -> Result<C::Output, VmError> + Send + Sync + 'static,
{
    fn call_json(&self, caller: &Caller<'_>, input: Value) -> Result<Value, VmError> {
        call_json(input, |input| (self.handler)(caller, input))
    }

    /// The script id is copied once here, so a call only borrows it.
    fn install<'js>(
        self: Arc<Self>,
        target: &Object<'js>,
        name: &str,
        script_id: &str,
    ) -> JsResult<()> {
        let script_id = Box::<str>::from(script_id);
        target.set(
            name,
            Func::from(move |ctx: Ctx<'js>, input: Opt<JsValue<'js>>| {
                let caller = Caller::new(&script_id);
                K::call_native(&ctx, input_or_null(&ctx, input), |input| {
                    (self.handler)(&caller, input)
                })
            }),
        )
    }
}

fn js_host_error(error: impl ToString) -> rquickjs::Error {
    rquickjs::Error::new_from_js_message("host", "function", error.to_string())
}

#[derive(Default)]
pub(super) struct FunctionBindingStore {
    by_name: Mutex<HashMap<String, Arc<dyn HostFunctionBinding>>>,
}

impl FunctionBindingStore {
    /// Stores `handler`, converted by `K`, as the implementation of contract `C`.
    pub(super) fn insert_plain<C, K>(
        &self,
        handler: impl Fn(C::Input) -> Result<C::Output, VmError> + Send + Sync + 'static,
    ) -> Result<(), VmError>
    where
        C: HostFunctionSignature + 'static,
        K: FunctionCodec<C>,
    {
        self.insert(
            C::NAME,
            Arc::new(PlainBinding::<C, K, _> {
                handler,
                marker: PhantomData,
            }),
        )
    }

    /// Stores `handler`, which receives the calling script and whose values `K`
    /// converts, as the implementation of contract `C`.
    pub(super) fn insert_with_caller<C, K>(
        &self,
        handler: impl Fn(&Caller<'_>, C::Input) -> Result<C::Output, VmError> + Send + Sync + 'static,
    ) -> Result<(), VmError>
    where
        C: HostFunctionSignature + 'static,
        K: FunctionCodec<C>,
    {
        self.insert(
            C::NAME,
            Arc::new(CallerBinding::<C, K, _> {
                handler,
                marker: PhantomData,
            }),
        )
    }

    fn insert(&self, name: &str, binding: Arc<dyn HostFunctionBinding>) -> Result<(), VmError> {
        let mut guard = self.by_name.lock().map_err(|_| VmError::LockPoisoned)?;
        guard.insert(name.to_owned(), binding);
        Ok(())
    }

    /// Calls the function `name` with a JSON input; the registry lock is released
    /// before the handler runs, so a handler can register functions.
    pub(super) fn invoke(
        &self,
        name: &str,
        caller: &Caller<'_>,
        input: Value,
    ) -> Result<Option<Value>, VmError> {
        let binding = {
            let guard = self.by_name.lock().map_err(|_| VmError::LockPoisoned)?;
            guard.get(name).cloned()
        };
        let Some(binding) = binding else {
            return Ok(None);
        };
        binding.call_json(caller, input).map(Some)
    }

    pub(super) fn names(&self) -> Result<Vec<String>, VmError> {
        let guard = self.by_name.lock().map_err(|_| VmError::LockPoisoned)?;
        Ok(guard.keys().cloned().collect())
    }

    /// Sets one native QuickJS function per binding on `target`, the host functions of
    /// script `script_id`. Each function owns its binding, so a call performs no name
    /// lookup and takes no lock.
    pub(super) fn install_native<'js>(
        &self,
        target: &Object<'js>,
        script_id: &str,
    ) -> JsResult<()> {
        for (name, binding) in self.snapshot().map_err(js_host_error)? {
            binding.install(target, &name, script_id)?;
        }
        Ok(())
    }

    fn snapshot(&self) -> Result<Vec<NamedBinding>, VmError> {
        let guard = self.by_name.lock().map_err(|_| VmError::LockPoisoned)?;
        Ok(guard
            .iter()
            .map(|(name, binding)| (name.clone(), binding.clone()))
            .collect())
    }
}

/// A host function called without an argument receives `null`, like the bridge path.
pub(super) fn input_or_null<'js>(ctx: &Ctx<'js>, input: Opt<JsValue<'js>>) -> JsValue<'js> {
    input.0.unwrap_or_else(|| JsValue::new_null(ctx.clone()))
}

#[cfg(test)]
mod lock_tests {
    use super::*;
    use crate::contract::{HostContract, HostContractKind, Schema};

    struct Reentrant;

    impl HostContract for Reentrant {
        const NAME: &'static str = "test.reentrant";

        fn schema() -> Schema {
            Schema::named("null")
        }

        fn kind() -> HostContractKind {
            HostContractKind::Function
        }
    }

    impl HostFunctionSignature for Reentrant {
        type Input = ();
        type Output = ();
    }

    #[test]
    fn json_handler_runs_without_registry_lock() {
        let store = Arc::new(FunctionBindingStore::default());
        let weak = Arc::downgrade(&store);
        store
            .insert_plain::<Reentrant, JsonCodec>(move |()| {
                let store = weak.upgrade().expect("store alive");
                let registry = store.by_name.try_lock();
                assert!(
                    registry.is_ok(),
                    "handler must be able to register another binding"
                );
                Ok(())
            })
            .unwrap();

        let output = store
            .invoke(Reentrant::NAME, &Caller::new("script"), Value::Null)
            .unwrap();

        assert_eq!(output, Some(Value::Null));
    }
}
