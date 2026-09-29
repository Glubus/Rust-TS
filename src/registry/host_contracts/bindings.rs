use std::collections::HashMap;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use rquickjs::function::Opt;
use rquickjs::prelude::Func;
use rquickjs::{Ctx, Object, Result as JsResult, Value as JsValue};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::contract::{
    Caller, EncodeReply, HostFunctionSignature, HostResolver, JsDecode, JsEncode, ReplyValue,
    js_value_to_json, json_to_js_value,
};
use crate::error::VmError;
use crate::runner::host_promises::WeakHostPromises;

type NamedBinding = (String, Binding);

/// The resolver an async handler of contract `C` receives.
type Resolver<C> = HostResolver<<C as HostFunctionSignature>::Output>;

/// A registered host function, sync or async.
#[derive(Clone)]
enum Binding {
    Sync(Arc<dyn HostFunctionBinding>),
    Async(Arc<dyn AsyncFunctionBinding>),
}

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

/// A registered async host function handler: each call returns a Promise that a
/// [`HostResolver`] settles.
trait AsyncFunctionBinding: Send + Sync {
    /// Sets the handler on `target` as the native function `name` of script
    /// `script_id`, whose calls start in `promises`.
    fn install<'js>(
        self: Arc<Self>,
        target: &Object<'js>,
        name: &str,
        script_id: &Rc<str>,
        promises: &WeakHostPromises,
    ) -> JsResult<()>;

    /// Like [`Self::install`], for the route that validates contracts: values cross as
    /// JSON, and `validator` checks the input before the handler runs and the output
    /// when the handler resolves.
    fn install_validated<'js>(
        self: Arc<Self>,
        target: &Object<'js>,
        name: &str,
        script_id: &Rc<str>,
        promises: &WeakHostPromises,
        validator: Arc<dyn ContractValidator>,
    ) -> JsResult<()>;
}

/// Checks the values of one contract as the registry's validation policy asks.
pub(super) trait ContractValidator: Send + Sync {
    /// Checks a call's input before the handler runs.
    fn validate_input(&self, input: &Value) -> Result<(), VmError>;

    /// Checks a handler's output before the script receives it.
    fn validate_output(&self, output: &Value) -> Result<(), VmError>;
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

/// How an async host function's input and resolved output cross between QuickJS and
/// Rust.
pub(super) trait AsyncFunctionCodec<C: HostFunctionSignature>: 'static {
    /// Converts a call's argument.
    fn decode_input<'js>(ctx: &Ctx<'js>, input: JsValue<'js>) -> JsResult<C::Input>;

    /// Keeps a resolved output until the engine thread converts it.
    fn encode_output(output: C::Output) -> Result<Box<dyn ReplyValue>, VmError>;
}

impl<C: HostFunctionSignature> AsyncFunctionCodec<C> for JsonCodec {
    fn decode_input<'js>(ctx: &Ctx<'js>, input: JsValue<'js>) -> JsResult<C::Input> {
        from_json(js_value_to_json(ctx, input)?)
    }

    /// Serializes on the resolving thread; only the JSON value waits.
    fn encode_output(output: C::Output) -> Result<Box<dyn ReplyValue>, VmError> {
        Ok(Box::new(JsonReply(serde_json::to_value(output)?)))
    }
}

impl<C> AsyncFunctionCodec<C> for TypedCodec
where
    C: HostFunctionSignature + 'static,
    C::Input: JsDecode,
    C::Output: JsEncode + Send,
{
    fn decode_input<'js>(ctx: &Ctx<'js>, input: JsValue<'js>) -> JsResult<C::Input> {
        C::Input::decode_js(ctx, input)
    }

    fn encode_output(output: C::Output) -> Result<Box<dyn ReplyValue>, VmError> {
        Ok(Box::new(TypedReply(output)))
    }
}

/// Deserializes a call's JSON input the way the sync JSON route does.
fn from_json<T: DeserializeOwned>(input: Value) -> JsResult<T> {
    serde_json::from_value(input).map_err(|error| js_host_error(VmError::from(error)))
}

/// A resolved output converted through `serde_json`.
struct JsonReply(Value);

impl ReplyValue for JsonReply {
    fn into_js<'js>(self: Box<Self>, ctx: &Ctx<'js>) -> JsResult<JsValue<'js>> {
        json_to_js_value(ctx, &self.0)
    }
}

/// A resolved output converted natively through [`JsEncode`].
struct TypedReply<T>(T);

impl<T: JsEncode + Send> ReplyValue for TypedReply<T> {
    fn into_js<'js>(self: Box<Self>, ctx: &Ctx<'js>) -> JsResult<JsValue<'js>> {
        self.0.encode_js(ctx)
    }
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

/// Async handler of contract `C`, whose values `K` converts. A handler that does not
/// ask which script called it is stored wrapped to ignore the [`Caller`].
struct AsyncBinding<C: HostFunctionSignature, K, F> {
    handler: F,
    encode: EncodeReply<C::Output>,
    codec: PhantomData<fn() -> K>,
}

impl<C, K, F> AsyncFunctionBinding for AsyncBinding<C, K, F>
where
    C: HostFunctionSignature + 'static,
    K: AsyncFunctionCodec<C>,
    F: Fn(&Caller<'_>, C::Input, Resolver<C>) -> Result<(), VmError> + Send + Sync + 'static,
{
    /// The function holds the scope weakly, so it neither keeps the scope's Promises
    /// alive nor starts calls once the scope is cancelled.
    fn install<'js>(
        self: Arc<Self>,
        target: &Object<'js>,
        name: &str,
        script_id: &Rc<str>,
        promises: &WeakHostPromises,
    ) -> JsResult<()> {
        let script_id = Rc::clone(script_id);
        let promises = promises.clone();
        target.set(
            name,
            Func::from(move |ctx: Ctx<'js>, input: Opt<JsValue<'js>>| {
                let caller = Caller::new(&script_id);
                promises.call(
                    &ctx,
                    C::NAME,
                    &self.encode,
                    || K::decode_input(&ctx, input_or_null(&ctx, input)),
                    |input, resolver| (self.handler)(&caller, input, resolver),
                )
            }),
        )
    }

    fn install_validated<'js>(
        self: Arc<Self>,
        target: &Object<'js>,
        name: &str,
        script_id: &Rc<str>,
        promises: &WeakHostPromises,
        validator: Arc<dyn ContractValidator>,
    ) -> JsResult<()> {
        let script_id = Rc::clone(script_id);
        let promises = promises.clone();
        let output_validator = Arc::clone(&validator);
        let encode: EncodeReply<C::Output> = Arc::new(
            move |output: C::Output| -> Result<Box<dyn ReplyValue>, VmError> {
                let output = serde_json::to_value(output)?;
                output_validator.validate_output(&output)?;
                Ok(Box::new(JsonReply(output)))
            },
        );
        target.set(
            name,
            Func::from(move |ctx: Ctx<'js>, input: Opt<JsValue<'js>>| {
                let caller = Caller::new(&script_id);
                promises.call(
                    &ctx,
                    C::NAME,
                    &encode,
                    || {
                        let input = js_value_to_json(&ctx, input_or_null(&ctx, input))?;
                        validator.validate_input(&input).map_err(js_host_error)?;
                        from_json(input)
                    },
                    |input, resolver| (self.handler)(&caller, input, resolver),
                )
            }),
        )
    }
}

fn js_host_error(error: impl ToString) -> rquickjs::Error {
    rquickjs::Error::new_from_js_message("host", "function", error.to_string())
}

#[derive(Default)]
pub(super) struct FunctionBindingStore {
    by_name: Mutex<HashMap<String, Binding>>,
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
            Binding::Sync(Arc::new(PlainBinding::<C, K, _> {
                handler,
                marker: PhantomData,
            })),
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
            Binding::Sync(Arc::new(CallerBinding::<C, K, _> {
                handler,
                marker: PhantomData,
            })),
        )
    }

    /// Stores async `handler`, which receives the calling script and whose values `K`
    /// converts, as the implementation of contract `C`.
    pub(super) fn insert_async<C, K, F>(&self, handler: F) -> Result<(), VmError>
    where
        C: HostFunctionSignature + 'static,
        K: AsyncFunctionCodec<C>,
        F: Fn(&Caller<'_>, C::Input, Resolver<C>) -> Result<(), VmError> + Send + Sync + 'static,
    {
        self.insert(
            C::NAME,
            Binding::Async(Arc::new(AsyncBinding::<C, K, F> {
                handler,
                encode: Arc::new(K::encode_output),
                codec: PhantomData,
            })),
        )
    }

    fn insert(&self, name: &str, binding: Binding) -> Result<(), VmError> {
        let mut guard = self.by_name.lock().map_err(|_| VmError::LockPoisoned)?;
        guard.insert(name.to_owned(), binding);
        Ok(())
    }

    /// Calls the sync function `name` with a JSON input; the registry lock is released
    /// before the handler runs, so a handler can register functions. `None` when no
    /// sync function is registered under `name`.
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
        let Some(Binding::Sync(binding)) = binding else {
            return Ok(None);
        };
        binding.call_json(caller, input).map(Some)
    }

    /// Names of the sync functions.
    pub(super) fn sync_names(&self) -> Result<Vec<String>, VmError> {
        let guard = self.by_name.lock().map_err(|_| VmError::LockPoisoned)?;
        Ok(guard
            .iter()
            .filter(|(_, binding)| matches!(binding, Binding::Sync(_)))
            .map(|(name, _)| name.clone())
            .collect())
    }

    /// Sets one native QuickJS function per binding on `target`, the host functions of
    /// script `script_id`, whose async calls start in `promises`. Each function owns
    /// its binding, so a call performs no name lookup and takes no lock.
    pub(super) fn install_native<'js>(
        &self,
        target: &Object<'js>,
        script_id: &str,
        promises: &WeakHostPromises,
    ) -> JsResult<()> {
        let mut shared_id = None;
        for (name, binding) in self.snapshot().map_err(js_host_error)? {
            match binding {
                Binding::Sync(binding) => binding.install(target, &name, script_id)?,
                Binding::Async(binding) => {
                    let shared_id = shared_id.get_or_insert_with(|| Rc::<str>::from(script_id));
                    binding.install(target, &name, shared_id, promises)?;
                }
            }
        }
        Ok(())
    }

    /// Sets the async functions on `target` for the route that validates contracts:
    /// `validator_for` gives the validator of each function by name.
    pub(super) fn install_validated_async<'js>(
        &self,
        target: &Object<'js>,
        script_id: &Rc<str>,
        promises: &WeakHostPromises,
        validator_for: impl Fn(&str) -> Arc<dyn ContractValidator>,
    ) -> JsResult<()> {
        for (name, binding) in self.snapshot().map_err(js_host_error)? {
            if let Binding::Async(binding) = binding {
                let validator = validator_for(&name);
                binding.install_validated(target, &name, script_id, promises, validator)?;
            }
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
