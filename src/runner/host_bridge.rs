//! Host bridge installation for QuickJS contexts.

use std::sync::Arc;

use rquickjs::{
    Context, Ctx, Exception, Object, Result as JsResult, Value as JsValue, prelude::Func,
};
use serde_json::Value;

use crate::error::VmError;
use crate::registry::InMemoryHostContractRegistry;

pub(crate) fn install_host_bridge(
    context: &Context,
    host_registry: Arc<InMemoryHostContractRegistry>,
) -> std::result::Result<(), VmError> {
    context.with(|ctx| install_host_items(ctx, host_registry))
}

fn install_host_items(
    ctx: Ctx<'_>,
    host_registry: Arc<InMemoryHostContractRegistry>,
) -> std::result::Result<(), VmError> {
    let globals = ctx.globals();
    let raw_host = Object::new(ctx.clone()).map_err(js_error)?;
    raw_host
        .set(
            "call",
            Func::from({
                let host_registry = host_registry.clone();
                move |ctx: Ctx<'_>, name: String, input_json: String| -> JsResult<String> {
                    invoke_host_function(&ctx, host_registry.as_ref(), &name, &input_json)
                }
            }),
        )
        .map_err(js_error)?;
    raw_host
        .set(
            "callValue",
            Func::from({
                let host_registry = host_registry.clone();
                move |ctx, name: String, input| {
                    invoke_host_function_value(&ctx, host_registry.as_ref(), &name, input)
                }
            }),
        )
        .map_err(js_error)?;
    globals.set("__host", raw_host).map_err(js_error)?;
    Ok(())
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

fn js_error(error: rquickjs::Error) -> VmError {
    VmError::Execution {
        details: error.to_string(),
    }
}
