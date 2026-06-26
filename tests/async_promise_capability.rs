#![cfg(feature = "async-promise")]

use std::sync::Arc;

use rquickjs::{AsyncContext, AsyncRuntime, CatchResultExt, Function, Promise, promise::Promised};
use serde_json::json;
use ts_embed_vm::{
    AsyncHostFunction, AsyncScriptRuntime, HostContract, HostContractKind,
    InMemoryHostContractRegistry, Schema, TsType, VmError, VmOptions, install_async_host_bridge,
};

struct AsyncFindUser;

const ASYNC_EXPORT_SCRIPT: &str = r#"
export async function lookup(id) {
  return {
    name: await user.find(id),
  };
}
"#;

impl HostContract for AsyncFindUser {
    const NAME: &'static str = "user.find";

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
fn rquickjs_future_value_can_be_awaited_as_js_promise() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        let quickjs_runtime = AsyncRuntime::new().expect("create async quickjs runtime");
        let context = AsyncContext::full(&quickjs_runtime)
            .await
            .expect("create async quickjs context");

        context
            .async_with(async |ctx| {
                let promised = Promised::from(async { 42 });
                let await_value = ctx
                    .eval::<Function<'_>, _>(
                        r#"
                        (async function(value) {
                          return await value;
                        })
                        "#,
                    )
                    .catch(&ctx)
                    .expect("compile async JS function");

                let promise = await_value
                    .call::<_, Promise<'_>>((promised,))
                    .expect("call async JS function");
                let result = promise
                    .into_future::<i32>()
                    .await
                    .catch(&ctx)
                    .expect("await JS promise");

                assert_eq!(result, 42);
            })
            .await;
    });
}

#[test]
fn public_async_host_bridge_exposes_promise_returning_sdk_function() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        let host_registry = Arc::new(InMemoryHostContractRegistry::new());
        host_registry
            .async_promise_function::<AsyncFindUser>()
            .expect("register async promise host function");
        let quickjs_runtime = AsyncRuntime::new().expect("create async quickjs runtime");
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

#[test]
fn public_async_script_runtime_awaits_host_promise_inside_exported_function() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        let host_registry = Arc::new(InMemoryHostContractRegistry::new());
        host_registry
            .async_promise_function::<AsyncFindUser>()
            .expect("register async promise host function");

        let mut runtime = AsyncScriptRuntime::new(&VmOptions::default(), host_registry)
            .await
            .expect("create async script runtime");
        let (script, subscriptions) = runtime
            .load_inline_script("async-export", ASYNC_EXPORT_SCRIPT)
            .await
            .expect("load async export script");
        let result = script
            .call_function("async-export", "lookup", &[json!(42)])
            .await
            .expect("call async export");

        assert!(subscriptions.is_empty());
        assert_eq!(result, json!({ "name": "async-user-42" }));
    });
}
