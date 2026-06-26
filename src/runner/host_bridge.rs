//! Host bridge installation for QuickJS contexts.

use std::sync::Arc;

use rquickjs::{Context, Ctx, Exception, Object, Result as JsResult, prelude::Func};
use serde_json::Value;

use crate::contract::{HostContractDescriptor, HostContractKind};
use crate::error::VmError;
use crate::registry::InMemoryHostContractRegistry;

use super::bridge_capability::WorkerBridgeCapability;

const HOST_LAZY_BINDINGS_TEMPLATE: &str = include_str!("../../assets/host_lazy_bindings.js");

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
    globals.set("__host", raw_host).map_err(js_error)?;
    install_host_function_namespaces(&ctx, host_registry)?;
    Ok(())
}

fn install_host_function_namespaces(
    ctx: &Ctx<'_>,
    host_registry: Arc<InMemoryHostContractRegistry>,
) -> std::result::Result<(), VmError> {
    let contract_names = host_registry
        .descriptors()?
        .into_iter()
        .filter(is_sync_bridge_function)
        .map(|descriptor| descriptor.name)
        .collect::<Vec<_>>();
    install_host_function_namespaces_from_names(ctx, &contract_names)
}

fn is_sync_bridge_function(descriptor: &HostContractDescriptor) -> bool {
    if descriptor.kind != HostContractKind::Function {
        return false;
    }

    match descriptor.function.as_ref() {
        Some(function) => {
            WorkerBridgeCapability::Sync.supports_function_execution(function.execution)
        }
        None => true,
    }
}

fn install_host_function_namespaces_from_names(
    ctx: &Ctx<'_>,
    contract_names: &[String],
) -> std::result::Result<(), VmError> {
    if contract_names.is_empty() {
        return Ok(());
    }
    ctx.eval::<(), _>(build_lazy_binding_source(contract_names, "call", false))
        .map_err(js_error)?;
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

fn js_error(error: rquickjs::Error) -> VmError {
    VmError::Execution {
        details: error.to_string(),
    }
}

fn build_lazy_binding_source(
    contract_names: &[String],
    bridge_method: &str,
    returns_promise: bool,
) -> String {
    let contracts = serde_json::to_string(contract_names).expect("serialize contract names");
    let bridge_method = serde_json::to_string(bridge_method).expect("serialize bridge method");
    HOST_LAZY_BINDINGS_TEMPLATE
        .replace("__contracts__", &contracts)
        .replace("__bridge_method__", &bridge_method)
        .replace(
            "__returns_promise__",
            if returns_promise { "true" } else { "false" },
        )
}
