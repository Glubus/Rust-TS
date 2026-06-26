#![cfg(feature = "async-promise")]

mod support;

use ts_embed_vm::{
    AsyncHostFunction, HostContract, HostContractKind, HostFunctionExecution, Schema, TsType, TsVm,
    VmError,
};

use support::TestCacheDir;

const SYNC_WORKER_PROBE: &str = r#"
export function hasUserFind() {
  if (typeof user === "undefined") {
    return "missing";
  }

  return typeof user.find;
}
"#;

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
fn sync_worker_rejects_async_promise_functions_instead_of_exposing_fake_direct_values() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        let cache_dir = TestCacheDir::new("async-promise-sync-guard");
        let vm = TsVm::new(cache_dir.vm_options()).expect("create vm");

        vm.registry()
            .async_promise_function::<AsyncFindUser>()
            .expect("register async promise function");
        let result = vm.load_script("probe", SYNC_WORKER_PROBE);

        vm.shutdown().expect("shutdown vm");
        assert!(matches!(
            result,
            Err(VmError::UnsupportedHostBridge {
                contract_name,
                execution: HostFunctionExecution::AsyncPromise,
            }) if contract_name == "user.find"
        ));
    });
}
