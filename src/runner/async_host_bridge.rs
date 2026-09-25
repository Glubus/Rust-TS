//! Async host bridge installation for QuickJS async contexts.

use std::sync::Arc;

use rquickjs::{
    AsyncContext, Ctx, Error as JsError, Function, Result as JsResult, function::Async,
};
use serde_json::Value;

use crate::error::VmError;
use crate::registry::InMemoryHostContractRegistry;

use super::errors::js_error;
use super::host_bridge::install_host_items;

/// Installs the AsyncContext host bridge.
///
/// `HostFunctionExecution::AsyncPromise` functions return real JavaScript promises;
/// synchronous host functions return their value directly, as on the sync lane.
pub async fn install_async_host_bridge(
    context: &AsyncContext,
    host_registry: Arc<InMemoryHostContractRegistry>,
) -> Result<(), VmError> {
    context
        .async_with(async |ctx| {
            let call_async = build_call_async_function(&ctx, host_registry.clone())?;
            install_host_items(&ctx, host_registry.clone(), call_async)
        })
        .await
}

fn build_call_async_function<'js>(
    ctx: &Ctx<'js>,
    host_registry: Arc<InMemoryHostContractRegistry>,
) -> Result<Function<'js>, VmError> {
    Function::new(
        ctx.clone(),
        Async(move |name: String, input_json: String| {
            let host_registry = host_registry.clone();
            async move { invoke_host_function_async(host_registry, name, input_json).await }
        }),
    )
    .map_err(js_error)
}

async fn invoke_host_function_async(
    host_registry: Arc<InMemoryHostContractRegistry>,
    name: String,
    input_json: String,
) -> JsResult<String> {
    let input = serde_json::from_str::<Value>(&input_json).map_err(js_host_error)?;
    let output = host_registry
        .invoke_function_async(name.clone(), input)
        .await
        .map_err(js_host_error)?
        .ok_or_else(|| js_host_error(format!("missing host function: {name}")))?;
    serde_json::to_string(&output).map_err(js_host_error)
}

fn js_host_error(error: impl ToString) -> JsError {
    JsError::new_from_js_message("host", "function", error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{AsyncHostFunction, HostContract, HostContractKind, Schema, TsType};
    use rquickjs::{CatchResultExt, Promise};

    struct AsyncFindUser;

    impl HostContract for AsyncFindUser {
        const NAME: &'static str = "user.find";
        const IMPORT_MODULE: &'static str = "test";
        const EXPORT_PATH: &'static [&'static str] = &["user", "find"];

        fn schema() -> Schema {
            Schema::typed("FindUserInput", TsType::Number)
        }

        fn kind() -> HostContractKind {
            HostContractKind::Function
        }
    }

    impl AsyncHostFunction for AsyncFindUser {
        type Future = std::future::Ready<Result<Self::Output, VmError>>;
        type Input = u64;
        type Output = String;

        fn output_schema() -> Schema {
            Schema::typed("FindUserOutput", TsType::String)
        }

        fn call_async(input: Self::Input) -> Self::Future {
            std::future::ready(Ok(format!("async-user-{input}")))
        }
    }

    #[test]
    fn async_bridge_exposes_registered_host_function_as_js_promise() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("build tokio runtime");

        runtime.block_on(async {
            let host_registry = Arc::new(InMemoryHostContractRegistry::new());
            host_registry
                .async_promise_function::<AsyncFindUser>()
                .expect("register async promise host function");
            let quickjs_runtime =
                rquickjs::AsyncRuntime::new().expect("create async quickjs runtime");
            let context = AsyncContext::full(&quickjs_runtime)
                .await
                .expect("create async quickjs context");

            install_async_host_bridge(&context, host_registry)
                .await
                .expect("install async host bridge");

            context
                .async_with(async |ctx| {
                    let lookup = ctx
                        .eval::<Function<'_>, _>(
                            r#"
                            (async function() {
                              const result = user.find(42);
                              if (!(result instanceof Promise)) {
                                throw new Error("expected Promise");
                              }
                              return await result;
                            })
                            "#,
                        )
                        .catch(&ctx)
                        .expect("compile async lookup");
                    let result = lookup
                        .call::<_, Promise<'_>>(())
                        .expect("call async lookup")
                        .into_future::<String>()
                        .await
                        .catch(&ctx)
                        .expect("await async lookup");

                    assert_eq!(result, "async-user-42");
                })
                .await;
        });
    }
}
