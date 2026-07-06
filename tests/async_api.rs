#![cfg(feature = "tokio")]

mod support;

use rustts::{
    AsyncHostFunction, HostContract, HostContractKind, HostFunctionExecution, RustTs, Schema,
    TsType, VmError,
};
use serde_json::json;
use support::TestCacheDir;

const BASIC_SCRIPT: &str = include_str!("projects/basic_math/main.ts");
const ASYNC_HOST_BRIDGE_SCRIPT: &str = include_str!("projects/async_host_bridge/main.ts");

struct AsyncFindUser;

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
    type Input = u64;
    type Output = String;
    type Future = std::future::Ready<Result<Self::Output, VmError>>;

    fn output_schema() -> Schema {
        Schema::typed("FindUserOutput", TsType::String)
    }

    fn call_async(input: Self::Input) -> Self::Future {
        std::future::ready(Ok(format!(
            "async-user-{input}-{}",
            std::thread::current().name().unwrap_or("unnamed")
        )))
    }
}

#[test]
fn tokio_async_api_loads_and_calls_script() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        let cache_dir = TestCacheDir::new("tokio-async-api");
        let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

        vm.load_script_async("math", BASIC_SCRIPT)
            .await
            .expect("load script");
        let result = vm
            .call_function_async("math", "sum", vec![json!({ "left": 3, "right": 4 })])
            .await
            .expect("call function");

        vm.shutdown().expect("shutdown vm");

        assert_eq!(result, json!(7));
    });
}

#[test]
fn tokio_async_host_function_is_callable_from_script() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        let cache_dir = TestCacheDir::new("tokio-async-host-function");
        let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

        vm.registry()
            .async_function::<AsyncFindUser>()
            .expect("register async host function");
        let descriptor = vm
            .registry()
            .descriptor(AsyncFindUser::NAME)
            .expect("get async host function descriptor")
            .expect("async host function descriptor");
        assert_eq!(
            descriptor.function.expect("function metadata").execution,
            HostFunctionExecution::AsyncBlockingJs
        );
        vm.load_script_async("async-host", ASYNC_HOST_BRIDGE_SCRIPT)
            .await
            .expect("load script");
        let result = vm
            .call_function_async("async-host", "lookup", Vec::new())
            .await
            .expect("call async host bridge");

        vm.shutdown().expect("shutdown vm");

        assert!(
            result
                .as_str()
                .unwrap_or_default()
                .starts_with("async-user-42-")
        );
        assert_ne!(result, json!("async-user-42-unnamed"));
    });
}
