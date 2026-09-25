#![cfg(feature = "async-promise")]

mod support;

use rustts::{AsyncHostFunction, HostContract, HostContractKind, RustTs, Schema, TsType, VmError};
use serde_json::json;

use support::TestCacheDir;

const SYNC_ONLY_SCRIPT: &str = r#"
export function add(left: number, right: number): number {
  return left + right;
}
"#;
const GLOBAL_ASYNC_CALL_SCRIPT: &str = r#"
export function lookup(id: number) {
  return user.find(id);
}
"#;
const IMPORTED_ASYNC_CALL_SCRIPT: &str = r#"
import { user } from "test";

export function lookup(id: number) {
  return user.find(id);
}
"#;

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

fn tokio_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("build tokio runtime")
}

fn vm_with_async_promise_function(cache_dir: &TestCacheDir) -> RustTs {
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");
    vm.registry()
        .async_promise_function::<AsyncFindUser>()
        .expect("register async promise function");
    vm
}

#[test]
fn sync_lane_loads_scripts_that_do_not_use_a_registered_async_promise_function() {
    let runtime = tokio_runtime();
    let _tokio = runtime.enter();
    let cache_dir = TestCacheDir::new("async-promise-sync-unrelated");
    let vm = vm_with_async_promise_function(&cache_dir);

    let loaded = vm.load_script("adder", SYNC_ONLY_SCRIPT);
    let sum = vm.call_function("adder", "add", vec![json!(2), json!(3)]);

    vm.shutdown().expect("shutdown vm");
    loaded.expect("load sync script next to an async promise function");
    assert_eq!(sum.expect("call sync script"), json!(5));
}

#[test]
fn sync_lane_call_to_async_promise_function_fails_naming_the_contract() {
    let runtime = tokio_runtime();
    let _tokio = runtime.enter();

    for (script_id, source) in [
        ("global-access", GLOBAL_ASYNC_CALL_SCRIPT),
        ("module-import", IMPORTED_ASYNC_CALL_SCRIPT),
    ] {
        let cache_dir = TestCacheDir::new("async-promise-sync-use");
        let vm = vm_with_async_promise_function(&cache_dir);

        let loaded = vm.load_script(script_id, source);
        let result = vm.call_function(script_id, "lookup", vec![json!(7)]);

        vm.shutdown().expect("shutdown vm");
        loaded.expect("load sync script that references an async promise function");
        let error = result.expect_err("sync lane cannot return a host Promise");
        assert!(
            matches!(
                &error,
                VmError::Execution { details }
                    if details.contains("`user.find`") && details.contains("AsyncPromise")
            ),
            "{script_id}: unexpected error: {error}"
        );
    }
}
