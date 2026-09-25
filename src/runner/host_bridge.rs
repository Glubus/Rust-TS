//! Host bridge installation for QuickJS contexts, shared by the sync and async lanes.

use std::sync::Arc;

use rquickjs::{
    CatchResultExt, Context, Ctx, Exception, IntoJs, Object, Result as JsResult, Value as JsValue,
    prelude::Func,
};
use serde_json::Value;

use crate::contract::{HostContractAbi, HostFunctionExecution};
use crate::error::VmError;
use crate::registry::{HostModuleStyle, InMemoryHostContractRegistry};

use super::errors::{caught_js_error, js_error};
use super::module_loader::WorkerModuleStore;
use super::render::{global_eval_options, host_lazy_bindings_source};

/// Installs the synchronous lane bridge. Its `callAsync` throws: a synchronous
/// context cannot await a host Promise.
pub(crate) fn install_host_bridge(
    context: &Context,
    host_registry: Arc<InMemoryHostContractRegistry>,
) -> Result<(), VmError> {
    context.with(|ctx| install_host_items(&ctx, host_registry, Func::from(reject_promise_call)))
}

/// Installs `__host` (`call`, `callValue`, and the lane's `callAsync`) and one lazy
/// global namespace per host function root, e.g. `user` for `user.find`.
pub(crate) fn install_host_items<'js>(
    ctx: &Ctx<'js>,
    host_registry: Arc<InMemoryHostContractRegistry>,
    call_async: impl IntoJs<'js>,
) -> Result<(), VmError> {
    let host = sync_host_object(ctx, host_registry.clone())?;
    host.set("callAsync", call_async).map_err(js_error)?;
    ctx.globals().set("__host", host).map_err(js_error)?;
    install_host_globals(ctx, &host_registry)
}

/// Makes the generated host import modules resolvable by scripts loaded into `store`.
pub(crate) fn insert_host_import_modules(
    store: &WorkerModuleStore,
    host_registry: &InMemoryHostContractRegistry,
) -> Result<(), VmError> {
    store.insert_host_modules(host_registry.import_modules(HostModuleStyle::Bridge)?)
}

fn sync_host_object<'js>(
    ctx: &Ctx<'js>,
    host_registry: Arc<InMemoryHostContractRegistry>,
) -> Result<Object<'js>, VmError> {
    let host = Object::new(ctx.clone()).map_err(js_error)?;
    host.set(
        "call",
        Func::from({
            let host_registry = host_registry.clone();
            move |ctx: Ctx<'_>, name: String, input_json: String| -> JsResult<String> {
                invoke_host_function(&ctx, host_registry.as_ref(), &name, &input_json)
            }
        }),
    )
    .map_err(js_error)?;
    host.set(
        "callValue",
        Func::from(move |ctx, name: String, input| {
            invoke_host_function_value(&ctx, host_registry.as_ref(), &name, input)
        }),
    )
    .map_err(js_error)?;
    Ok(host)
}

fn install_host_globals(
    ctx: &Ctx<'_>,
    host_registry: &InMemoryHostContractRegistry,
) -> Result<(), VmError> {
    let functions = host_function_globals(host_registry)?;
    if functions.is_empty() {
        return Ok(());
    }

    let source = host_lazy_bindings_source(&serde_json::to_string(&functions)?);
    ctx.eval_with_options::<(), _>(source, global_eval_options("host-lazy-bindings"))
        .catch(ctx)
        .map_err(caught_js_error)
}

/// `(contract name, returns a Promise)` for every registered host function.
fn host_function_globals(
    host_registry: &InMemoryHostContractRegistry,
) -> Result<Vec<(String, bool)>, VmError> {
    Ok(host_registry
        .descriptors()?
        .into_iter()
        .filter_map(|descriptor| match descriptor.abi {
            HostContractAbi::Function { execution, .. } => Some((
                descriptor.name,
                execution == HostFunctionExecution::AsyncPromise,
            )),
            HostContractAbi::Callback { .. }
            | HostContractAbi::Context { .. }
            | HostContractAbi::Unknown => None,
        })
        .collect())
}

fn reject_promise_call(ctx: Ctx<'_>, name: String) -> JsResult<()> {
    let unsupported = VmError::UnsupportedHostBridge {
        contract_name: name,
        execution: HostFunctionExecution::AsyncPromise,
    };
    Err(Exception::throw_message(
        &ctx,
        &format!("{unsupported}; load the script with `load_async_script` to await it"),
    ))
}

fn invoke_host_function(
    ctx: &Ctx<'_>,
    host_registry: &InMemoryHostContractRegistry,
    name: &str,
    input_json: &str,
) -> JsResult<String> {
    let input = serde_json::from_str::<Value>(input_json)
        .map_err(|error| Exception::throw_message(ctx, &error.to_string()))?;
    let output = host_registry
        .invoke_function(name, input)
        .map_err(|error| Exception::throw_message(ctx, &error.to_string()))?
        .ok_or_else(|| Exception::throw_message(ctx, &format!("missing host function: {name}")))?;
    serde_json::to_string(&output)
        .map_err(|error| Exception::throw_message(ctx, &error.to_string()))
}

fn invoke_host_function_value<'js>(
    ctx: &Ctx<'js>,
    host_registry: &InMemoryHostContractRegistry,
    name: &str,
    input: JsValue<'js>,
) -> JsResult<JsValue<'js>> {
    let output = host_registry
        .invoke_function_js(ctx, name, input)
        .map_err(|error| Exception::throw_message(ctx, &error.to_string()))?
        .ok_or_else(|| Exception::throw_message(ctx, &format!("missing host function: {name}")))?;
    Ok(output)
}
