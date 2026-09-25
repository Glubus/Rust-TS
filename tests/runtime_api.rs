mod support;

use std::fs;
use std::path::Path;

use rustts::{
    HostCallback, HostContract, HostContractKind, HostFunction, NativeBytes, RuntimeExecutionLane,
    RuntimeMaterializationState, RustTs, Schema, ScriptMaterializationState, ScriptRetentionPolicy,
    ScriptSourceKind, TsSchema, TsType, VmError, VmEvent,
};
use serde_json::json;

use support::TestCacheDir;

const DEMO_SCRIPT: &str = include_str!("projects/basic_math/main.ts");
const MULTI_MODULE_ENTRY: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/projects/multi_module/main.ts"
);
const INDEX_MODULE_ENTRY: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/projects/index_module/main.ts"
);
const ESM_FORMS_ENTRY: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/projects/esm_forms/main.ts"
);
const EVENT_LISTENER_SCRIPT: &str = include_str!("projects/event_listener/main.ts");
const STATEFUL_PROJECT_ENTRY: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/projects/stateful_project/main.ts"
);
const TYPE_ONLY_RUNTIME_ENTRY: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/projects/type_only_runtime/main.ts"
);
const TSCONFIG_ALIAS_ENTRY: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/projects/tsconfig_alias/main.ts"
);
const PACKAGE_IMPORT_ENTRY: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/projects/package_import/main.ts"
);
const DYNAMIC_IMPORT_ENTRY: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/projects/dynamic_import/main.ts"
);
const INVALID_BARE_IMPORT_ENTRY: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/projects/invalid_bare_import/main.ts"
);
const HOST_BRIDGE_SCRIPT: &str = include_str!("projects/host_bridge/main.ts");
const LAZY_HOST_BRIDGE_SCRIPT: &str = include_str!("projects/lazy_host_bridge/main.ts");
const VERSION_SCRIPT: &str = include_str!("projects/version_only/main.ts");
const THROWING_SCRIPT: &str = include_str!("projects/throwing/main.ts");
const ASYNC_EXPORT_SCRIPT: &str = r#"
async function double(value: number): Promise<number> {
  await null;
  return value * 2;
}

export async function compute(value: number): Promise<{ doubled: number }> {
  return { doubled: await double(value) };
}

export function pending(): Promise<never> {
  return new Promise(() => {});
}
"#;

struct FindUser;
struct FindInvoice;
struct ScoreUpdate;
struct ReadNativeBytes;

impl HostContract for FindUser {
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

impl HostFunction for FindUser {
    type Input = u64;
    type Output = String;

    fn output_schema() -> Schema {
        Schema::typed("FindUserOutput", TsType::String)
    }

    fn call(input: Self::Input) -> Result<Self::Output, VmError> {
        Ok(format!("user-{input}"))
    }
}

impl HostContract for FindInvoice {
    const NAME: &'static str = "billing.invoice.find";
    const IMPORT_MODULE: &'static str = "test";
    const EXPORT_PATH: &'static [&'static str] = &["billing", "invoice", "find"];

    fn schema() -> Schema {
        Schema::typed("FindInvoiceInput", TsType::Number)
    }

    fn kind() -> HostContractKind {
        HostContractKind::Function
    }
}

impl HostFunction for FindInvoice {
    type Input = u64;
    type Output = String;

    fn output_schema() -> Schema {
        Schema::typed("FindInvoiceOutput", TsType::String)
    }

    fn call(input: Self::Input) -> Result<Self::Output, VmError> {
        Ok(format!("invoice-{input}"))
    }
}

impl HostContract for ReadNativeBytes {
    const NAME: &'static str = "bench.bytes.native";

    fn schema() -> Schema {
        Schema::typed("ReadNativeBytesInput", TsType::Json)
    }

    fn kind() -> HostContractKind {
        HostContractKind::Function
    }
}

impl HostFunction for ReadNativeBytes {
    type Input = serde_json::Value;
    type Output = NativeBytes;

    fn output_schema() -> Schema {
        NativeBytes::schema()
    }

    fn call(input: Self::Input) -> Result<Self::Output, VmError> {
        let byte_count = input
            .get("byteCount")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as usize;
        Ok(NativeBytes::new(make_test_bytes(byte_count)))
    }
}

fn make_test_bytes(byte_count: usize) -> Vec<u8> {
    (0..byte_count).map(|index| (index % 251) as u8).collect()
}

impl HostContract for ScoreUpdate {
    const NAME: &'static str = "score.update";
    const IMPORT_MODULE: &'static str = "test";
    const EXPORT_PATH: &'static [&'static str] = &["score", "onUpdate"];

    fn schema() -> Schema {
        Schema::typed("ScorePayload", TsType::Json)
    }

    fn kind() -> HostContractKind {
        HostContractKind::Callback
    }
}

impl HostCallback for ScoreUpdate {
    type Payload = serde_json::Value;
}

#[test]
fn load_call_unload_emits_expected_events() {
    let cache_dir = TestCacheDir::new("load-call-unload");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");
    let subscription = vm.subscribe();

    let snapshot = vm.load_script("math", DEMO_SCRIPT).expect("load script");
    let result = vm
        .call_function("math", "sum", vec![json!({ "left": 20, "right": 22 })])
        .expect("call function");
    vm.unload_script("math").expect("unload script");
    vm.shutdown().expect("shutdown vm");

    assert_eq!(result, json!(42));
    assert_eq!(snapshot.id, "math");
    assert_eq!(snapshot.source_kind, ScriptSourceKind::Inline);
    assert_eq!(snapshot.entry_path, None);
    assert!(snapshot.transpiled_path.ends_with(".js"));

    assert_eq!(
        subscription.recv(),
        Some(VmEvent::ScriptLoaded {
            snapshot: snapshot.clone(),
        })
    );
    assert_eq!(
        subscription.recv(),
        Some(VmEvent::FunctionCalled {
            worker_id: snapshot.worker_id,
            script_id: String::from("math"),
            function_name: String::from("sum"),
            result: json!(42),
        })
    );
    assert_eq!(
        subscription.recv(),
        Some(VmEvent::ScriptUnloaded {
            worker_id: snapshot.worker_id,
            script_id: String::from("math"),
        })
    );
    assert_eq!(subscription.recv(), Some(VmEvent::Shutdown));
}

#[test]
fn cache_artifact_is_reused_for_same_source() {
    let cache_dir = TestCacheDir::new("cache-reuse");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    let first = vm
        .load_script("math-a", DEMO_SCRIPT)
        .expect("load first script");
    vm.unload_script("math-a").expect("unload first script");

    let artifact = fs::read_to_string(&first.transpiled_path).expect("read transpiled artifact");

    let second = vm
        .load_script("math-b", DEMO_SCRIPT)
        .expect("load second script");
    vm.shutdown().expect("shutdown vm");

    assert_eq!(first.cache_key, second.cache_key);
    assert_eq!(first.transpiled_path, second.transpiled_path);
    assert!(!artifact.is_empty());
}

#[test]
fn describe_script_tracks_inline_lifecycle_state() {
    let cache_dir = TestCacheDir::new("describe-inline-script");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    vm.load_script("math", DEMO_SCRIPT).expect("load script");
    let mounted = vm
        .describe_script("math")
        .expect("describe mounted script")
        .expect("mounted entry");
    vm.unload_script("math").expect("unload script");
    let compiled = vm
        .describe_script("math")
        .expect("describe compiled script")
        .expect("compiled entry");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(mounted.source_kind, ScriptSourceKind::Inline);
    assert_eq!(mounted.state, ScriptMaterializationState::Mounted);
    assert_eq!(mounted.entry_path, None);
    assert_eq!(compiled.state, ScriptMaterializationState::Compiled);
}

#[test]
fn list_scripts_returns_registered_entries_sorted() {
    let cache_dir = TestCacheDir::new("list-scripts");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    vm.load_script_project("project", MULTI_MODULE_ENTRY)
        .expect("load project script");
    vm.load_script("math", DEMO_SCRIPT)
        .expect("load inline script");

    let entries = vm.list_scripts().expect("list scripts");

    vm.shutdown().expect("shutdown vm");

    let ids = entries
        .iter()
        .map(|entry| entry.script_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["math", "project"]);
    assert_eq!(entries[0].source_kind, ScriptSourceKind::Inline);
    assert_eq!(entries[0].state, ScriptMaterializationState::Mounted);
    assert_eq!(entries[1].source_kind, ScriptSourceKind::Project);
    assert_eq!(entries[1].state, ScriptMaterializationState::Mounted);
    assert!(entries[1].entry_path.is_some());
}

#[test]
fn runtime_snapshot_exposes_routes_retention_and_module_graph() {
    let cache_dir = TestCacheDir::new("runtime-snapshot");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");
    vm.registry()
        .callback::<ScoreUpdate>()
        .expect("register score update callback");

    vm.load_script("listener", EVENT_LISTENER_SCRIPT)
        .expect("load listener");
    vm.load_script_project("project", MULTI_MODULE_ENTRY)
        .expect("load project script");
    vm.retain_script_dependency_edge("project", "listener")
        .expect("retain script edge");

    let snapshot = vm.runtime_snapshot().expect("collect runtime snapshot");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(snapshot.stats.memory.active_scripts, 2);
    assert_eq!(snapshot.stats.workers.len(), 1);
    assert_eq!(snapshot.stats.workers[0].worker_id, 0);
    assert_eq!(snapshot.stats.workers[0].loaded_scripts, 2);
    assert_eq!(snapshot.stats.workers[0].active_scripts, 2);
    assert_eq!(snapshot.stats.workers[0].event_route_bindings, 1);
    assert_eq!(snapshot.stats.workers[0].dependency_edges, 1);
    assert_eq!(snapshot.stats.workers[0].module_dependency_edges, 4);
    assert_eq!(snapshot.stats.workers[0].max_scripts, 8);
    assert_eq!(snapshot.stats.workers[0].queue_capacity, 32);
    assert_eq!(snapshot.stats.workers[0].queue_depth, 0);
    assert!(snapshot.stats.workers[0].queue_peak_depth >= 1);
    assert_eq!(snapshot.stats.workers[0].queue_rejected_sends, 0);
    assert_eq!(snapshot.stats.workers[0].async_queue_depth, 0);
    assert_eq!(snapshot.stats.workers[0].async_queue_peak_depth, 0);
    assert_eq!(snapshot.stats.workers[0].async_queue_rejected_sends, 0);
    assert_eq!(snapshot.stats.workers[0].sync_latency.load_operations, 2);
    assert!(snapshot.stats.workers[0].sync_latency.load_total_ns > 0);
    assert_eq!(snapshot.stats.workers[0].sync_latency.call_operations, 0);
    assert_eq!(snapshot.stats.workers[0].sync_latency.emit_operations, 0);
    assert_eq!(snapshot.stats.workers[0].async_latency.load_operations, 0);
    let sync_memory = snapshot.stats.workers[0]
        .sync_quickjs_memory
        .expect("sync quickjs memory stats");
    assert!(sync_memory.memory_used_bytes > 0);
    assert_eq!(sync_memory.malloc_limit_bytes, 16 * 1024 * 1024);
    #[cfg(feature = "async-promise")]
    assert!(snapshot.stats.workers[0].async_quickjs_memory.is_some());
    #[cfg(not(feature = "async-promise"))]
    assert!(snapshot.stats.workers[0].async_quickjs_memory.is_none());
    assert_eq!(snapshot.scripts.len(), 2);
    assert_eq!(snapshot.dependency_edges.len(), 1);
    assert_eq!(
        snapshot.dependency_edges[0].dependent_script_id,
        String::from("project")
    );
    assert_eq!(
        snapshot.dependency_edges[0].dependency_script_id,
        String::from("listener")
    );

    let listener = snapshot
        .scripts
        .iter()
        .find(|script| script.script_id == "listener")
        .expect("listener script");
    assert_eq!(listener.state, RuntimeMaterializationState::Mounted);
    assert_eq!(listener.active_worker, Some(0));
    assert_eq!(listener.execution_lane, Some(RuntimeExecutionLane::Sync));
    assert_eq!(listener.subscriptions, vec![String::from("score.update")]);
    assert_eq!(
        listener
            .retention
            .as_ref()
            .expect("listener retention")
            .dependency_ref_count,
        1
    );

    let project = snapshot
        .scripts
        .iter()
        .find(|script| script.script_id == "project")
        .expect("project script");
    assert_eq!(project.module_dependencies.len(), 4);

    assert_eq!(snapshot.event_routes.len(), 1);
    assert_eq!(snapshot.event_routes[0].event_name, "score.update");
    assert_eq!(snapshot.event_routes[0].bindings.len(), 1);
    assert_eq!(
        snapshot.event_routes[0].bindings[0].script_id,
        String::from("listener")
    );
    assert_eq!(
        snapshot.event_routes[0].bindings[0].execution_lane,
        RuntimeExecutionLane::Sync
    );
}

#[test]
fn stats_track_multiple_scripts_on_same_vm() {
    let cache_dir = TestCacheDir::new("multi-script-stats");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    vm.load_script("math-a", DEMO_SCRIPT)
        .expect("load first script");
    vm.load_script("math-b", VERSION_SCRIPT)
        .expect("load second script");

    let stats = vm.stats().expect("collect stats");
    let version = vm
        .call_function("math-b", "version", Vec::new())
        .expect("call version");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(stats.worker_count, 1);
    assert_eq!(stats.loaded_scripts, 2);
    assert_eq!(stats.workers.len(), 1);
    assert_eq!(stats.workers[0].loaded_scripts, 2);
    assert_eq!(stats.workers[0].active_scripts, 2);
    assert_eq!(stats.cache_entries, 2);
    assert_eq!(version, json!("v2"));
}

#[test]
fn calling_after_unload_returns_not_found() {
    let cache_dir = TestCacheDir::new("call-after-unload");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    vm.load_script("math", DEMO_SCRIPT).expect("load script");
    vm.unload_script("math").expect("unload script");

    let error = vm
        .call_function("math", "sum", vec![json!({ "left": 1, "right": 2 })])
        .expect_err("call should fail");

    vm.shutdown().expect("shutdown vm");

    assert!(matches!(
        error,
        VmError::ScriptNotFound { script_id } if script_id == "math"
    ));
}

#[test]
fn javascript_errors_include_message_and_stack() {
    let cache_dir = TestCacheDir::new("js-error-details");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    vm.load_script("throwing", THROWING_SCRIPT)
        .expect("load throwing script");
    let error = vm
        .call_function("throwing", "explode", Vec::new())
        .expect_err("script should throw");

    vm.shutdown().expect("shutdown vm");

    assert!(matches!(
        error,
        VmError::Execution { details }
            if details.contains("boom from script") && details.contains("explode")
    ));
}

#[test]
fn demount_when_idle_unloads_after_call() {
    let cache_dir = TestCacheDir::new("demount-when-idle");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");
    let subscription = vm.subscribe();

    vm.load_script_with_policy("math", DEMO_SCRIPT, ScriptRetentionPolicy::DemountWhenIdle)
        .expect("load oneshot script");
    let result = vm
        .call_function("math", "sum", vec![json!({ "left": 5, "right": 7 })])
        .expect("call function");
    let error = vm
        .call_function("math", "sum", vec![json!({ "left": 1, "right": 2 })])
        .expect_err("script should have demounted");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(result, json!(12));
    assert!(matches!(
        error,
        VmError::ScriptNotFound { script_id } if script_id == "math"
    ));

    let _ = subscription.recv();
    let _ = subscription.recv();
    assert_eq!(
        subscription.recv(),
        Some(VmEvent::ScriptUnloaded {
            worker_id: 0,
            script_id: String::from("math"),
        })
    );
}

#[test]
fn reload_without_policy_keeps_existing_retention_policy() {
    let cache_dir = TestCacheDir::new("reload-keeps-retention-policy");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");
    let args = || vec![json!({ "left": 1, "right": 2 })];

    vm.load_script_with_policy(
        "inline",
        DEMO_SCRIPT,
        ScriptRetentionPolicy::DemountWhenIdle,
    )
    .expect("load idle-demounted inline script");
    vm.load_script("inline", DEMO_SCRIPT)
        .expect("reload inline script without a policy");
    let inline_first = vm.call_function("inline", "sum", args());
    let inline_second = vm.call_function("inline", "sum", args());

    vm.load_script_with_policy(
        "project",
        DEMO_SCRIPT,
        ScriptRetentionPolicy::DemountWhenIdle,
    )
    .expect("load idle-demounted script");
    vm.load_script_project("project", MULTI_MODULE_ENTRY)
        .expect("reload as project without a policy");
    let project_first = vm.call_function("project", "compute", args());
    let project_second = vm.call_function("project", "compute", args());

    vm.load_script_with_policy(
        "explicit",
        DEMO_SCRIPT,
        ScriptRetentionPolicy::DemountWhenIdle,
    )
    .expect("load idle-demounted script");
    vm.load_script_with_policy("explicit", DEMO_SCRIPT, ScriptRetentionPolicy::KeepMounted)
        .expect("reload with an explicit policy");
    let explicit_first = vm.call_function("explicit", "sum", args());
    let explicit_second = vm.call_function("explicit", "sum", args());

    vm.shutdown().expect("shutdown vm");

    assert_eq!(inline_first.expect("call reloaded inline script"), json!(3));
    assert!(matches!(
        inline_second,
        Err(VmError::ScriptNotFound { script_id }) if script_id == "inline"
    ));
    project_first.expect("call reloaded project script");
    assert!(matches!(
        project_second,
        Err(VmError::ScriptNotFound { script_id }) if script_id == "project"
    ));
    assert_eq!(explicit_first.expect("call explicit reload"), json!(3));
    assert_eq!(explicit_second.expect("explicit policy wins"), json!(3));
}

#[test]
fn sync_lane_call_function_awaits_async_exports() {
    let cache_dir = TestCacheDir::new("sync-lane-async-export");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    vm.load_script("async-export", ASYNC_EXPORT_SCRIPT)
        .expect("load script with async exports");
    let resolved = vm.call_function("async-export", "compute", vec![json!(21)]);
    let pending = vm.call_function("async-export", "pending", Vec::new());

    vm.shutdown().expect("shutdown vm");

    assert_eq!(
        resolved.expect("call async export"),
        json!({ "doubled": 42 })
    );
    let error = pending.expect_err("a Promise that never settles has no result");
    assert!(
        matches!(
            &error,
            VmError::Execution { details }
                if details.contains("`pending`") && details.contains("never settles")
        ),
        "unexpected error: {error}"
    );
}

#[test]
fn dependency_ref_keeps_demount_when_idle_script_mounted() {
    let cache_dir = TestCacheDir::new("dependency-ref-lifecycle");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    vm.load_script_with_policy("math", DEMO_SCRIPT, ScriptRetentionPolicy::DemountWhenIdle)
        .expect("load retained script");
    vm.retain_script_dependency("math")
        .expect("retain dependency");
    let result = vm
        .call_function("math", "sum", vec![json!({ "left": 3, "right": 4 })])
        .expect("call while retained");
    let still_mounted = vm
        .call_function("math", "sum", vec![json!({ "left": 1, "right": 1 })])
        .expect("call still mounted");
    vm.release_script_dependency("math")
        .expect("release dependency");
    let error = vm
        .call_function("math", "sum", vec![json!({ "left": 1, "right": 2 })])
        .expect_err("script should demount after dependency release");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(result, json!(7));
    assert_eq!(still_mounted, json!(2));
    assert!(matches!(
        error,
        VmError::ScriptNotFound { script_id } if script_id == "math"
    ));
}

#[test]
fn dependency_edge_keeps_dependency_script_mounted() {
    let cache_dir = TestCacheDir::new("dependency-edge-lifecycle");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    vm.load_script("consumer", VERSION_SCRIPT)
        .expect("load consumer script");
    vm.load_script_with_policy(
        "dependency",
        DEMO_SCRIPT,
        ScriptRetentionPolicy::DemountWhenIdle,
    )
    .expect("load dependency script");
    vm.retain_script_dependency_edge("consumer", "dependency")
        .expect("retain dependency edge");
    let result = vm
        .call_function("dependency", "sum", vec![json!({ "left": 10, "right": 5 })])
        .expect("call dependency while retained");
    let still_mounted = vm
        .call_function("dependency", "sum", vec![json!({ "left": 1, "right": 2 })])
        .expect("dependency should still be mounted");

    vm.release_script_dependency_edge("consumer", "dependency")
        .expect("release dependency edge");
    let error = vm
        .call_function("dependency", "sum", vec![json!({ "left": 1, "right": 2 })])
        .expect_err("dependency should demount after edge release");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(result, json!(15));
    assert_eq!(still_mounted, json!(3));
    assert!(matches!(
        error,
        VmError::ScriptNotFound { script_id } if script_id == "dependency"
    ));
}

#[test]
fn call_function_once_executes_and_releases_script() {
    let cache_dir = TestCacheDir::new("call-function-once");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");
    let subscription = vm.subscribe();

    let result = vm
        .call_function_once(DEMO_SCRIPT, "sum", vec![json!({ "left": 8, "right": 13 })])
        .expect("call oneshot function");
    let stats = vm.stats().expect("collect stats");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(result, json!(21));
    assert_eq!(stats.loaded_scripts, 0);
    assert_eq!(stats.cache_entries, 1);

    let loaded = subscription.recv().expect("script loaded event");
    let called = subscription.recv().expect("function called event");
    let unloaded = subscription.recv().expect("script unloaded event");

    let oneshot_script_id = match loaded {
        VmEvent::ScriptLoaded { snapshot } => snapshot.id,
        other => panic!("unexpected first event: {other:?}"),
    };

    match called {
        VmEvent::FunctionCalled {
            script_id, result, ..
        } => {
            assert_eq!(script_id, oneshot_script_id);
            assert_eq!(result, json!(21));
        }
        other => panic!("unexpected second event: {other:?}"),
    }

    match unloaded {
        VmEvent::ScriptUnloaded { script_id, .. } => {
            assert_eq!(script_id, oneshot_script_id);
        }
        other => panic!("unexpected third event: {other:?}"),
    }
}

#[test]
fn script_can_call_registered_host_function() {
    let cache_dir = TestCacheDir::new("host-function-bridge");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");
    vm.registry()
        .function::<FindUser>()
        .expect("register host function");

    vm.load_script("host-bridge", HOST_BRIDGE_SCRIPT)
        .expect("load host bridge script");
    let from_namespace = vm
        .call_function("host-bridge", "lookupWithNamespace", vec![json!(7)])
        .expect("call namespace bridge");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(from_namespace, json!("user-7"));
}

#[test]
fn host_function_bridge_uses_imported_namespace_resolution() {
    let cache_dir = TestCacheDir::new("lazy-host-function-bridge");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");
    vm.registry()
        .function::<FindUser>()
        .and_then(|registry| registry.function::<FindInvoice>())
        .expect("register host functions");

    vm.load_script("lazy-host-bridge", LAZY_HOST_BRIDGE_SCRIPT)
        .expect("load lazy host bridge script");
    let result = vm
        .call_function("lazy-host-bridge", "inspectBindings", vec![json!(9)])
        .expect("inspect lazy host bridge");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(
        result,
        json!({
            "userFindType": "function",
            "missingType": "undefined",
            "invoice": "invoice-9",
        })
    );
}

#[test]
fn typed_host_function_can_return_native_bytes_as_uint8array() {
    const SCRIPT: &str = r#"
export function inspectNativeBytes(byteCount: number) {
  const bytes = (globalThis as any).__host.callValue("bench.bytes.native", { byteCount });
  return {
    isView: ArrayBuffer.isView(bytes),
    constructorName: bytes.constructor.name,
    length: bytes.length,
    first: bytes[0],
    fourth: bytes[3],
    sampled: bytes[0] + bytes[4096],
  };
}

export function inspectJsonFallback(byteCount: number) {
  const bytes = JSON.parse((globalThis as any).__host.call(
    "bench.bytes.native",
    JSON.stringify({ byteCount }),
  ));
  return {
    isArray: Array.isArray(bytes),
    length: bytes.length,
    first: bytes[0],
    fourth: bytes[3],
  };
}
"#;

    let cache_dir = TestCacheDir::new("native-bytes-host-bridge");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");
    vm.registry()
        .typed_function::<ReadNativeBytes>()
        .expect("register native bytes host function");
    let declarations = vm.registry().types().expect("render declarations");
    assert!(declarations.contains("type NativeBytes = Uint8Array;"));

    vm.load_script("native-bytes", SCRIPT)
        .expect("load native bytes script");
    let native = vm
        .call_function("native-bytes", "inspectNativeBytes", vec![json!(8192)])
        .expect("inspect native bytes");
    let fallback = vm
        .call_function("native-bytes", "inspectJsonFallback", vec![json!(8)])
        .expect("inspect json fallback");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(
        native,
        json!({
            "isView": true,
            "constructorName": "Uint8Array",
            "length": 8192,
            "first": 0,
            "fourth": 3,
            "sampled": 80,
        })
    );
    assert_eq!(
        fallback,
        json!({
            "isArray": true,
            "length": 8,
            "first": 0,
            "fourth": 3,
        })
    );
}

#[test]
fn load_script_project_resolves_relative_imports() {
    let cache_dir = TestCacheDir::new("load-script-project");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    let snapshot = vm
        .load_script_project("project", MULTI_MODULE_ENTRY)
        .expect("load multi-file project");
    let result = vm
        .call_function(
            "project",
            "compute",
            vec![json!({ "left": 10, "right": 32 })],
        )
        .expect("call project export");
    let stats = vm.stats().expect("collect stats");

    vm.shutdown().expect("shutdown vm");

    let expected_entry_path = Path::new(MULTI_MODULE_ENTRY)
        .canonicalize()
        .expect("canonicalize project entry")
        .to_string_lossy()
        .into_owned();

    assert_eq!(snapshot.id, "project");
    assert_eq!(snapshot.source_kind, ScriptSourceKind::Project);
    assert_eq!(
        snapshot.entry_path.as_deref(),
        Some(expected_entry_path.as_str())
    );
    assert_eq!(result, json!({ "total": 42, "version": "v3" }));
    assert_eq!(stats.memory.module_dependency_edges, 4);
}

#[test]
fn load_script_project_resolves_index_modules() {
    let cache_dir = TestCacheDir::new("load-script-project-index");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    vm.load_script_project("project", INDEX_MODULE_ENTRY)
        .expect("load project with index modules");
    let result = vm
        .call_function("project", "render", vec![json!("world")])
        .expect("call index-backed export");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(result, json!("hello WORLD"));
}

#[test]
fn load_script_project_executes_native_esm_import_forms() {
    let cache_dir = TestCacheDir::new("load-script-project-esm-forms");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    vm.load_script_project("project", ESM_FORMS_ENTRY)
        .expect("load project with native ESM forms");
    let result = vm
        .call_function(
            "project",
            "render",
            vec![json!({ "left": 19, "right": 23 })],
        )
        .expect("call export using native ESM forms");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(result, json!("42 points"));
}

#[test]
fn load_script_project_isolates_module_state_between_scripts() {
    let cache_dir = TestCacheDir::new("project-module-state-isolation");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    vm.load_script_project("project-a", STATEFUL_PROJECT_ENTRY)
        .expect("load first stateful project");
    vm.load_script_project("project-b", STATEFUL_PROJECT_ENTRY)
        .expect("load second stateful project");
    let first_a = vm
        .call_function("project-a", "tick", Vec::new())
        .expect("call first project");
    let second_a = vm
        .call_function("project-a", "tick", Vec::new())
        .expect("call first project again");
    let first_b = vm
        .call_function("project-b", "tick", Vec::new())
        .expect("call second project");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(first_a, json!(1));
    assert_eq!(second_a, json!(2));
    assert_eq!(first_b, json!(1));
}

#[test]
fn load_script_project_reload_resets_module_state_for_same_script_id() {
    let cache_dir = TestCacheDir::new("project-module-state-reload");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    vm.load_script_project("project", STATEFUL_PROJECT_ENTRY)
        .expect("load stateful project");
    let first = vm
        .call_function("project", "tick", Vec::new())
        .expect("call stateful project");
    let second = vm
        .call_function("project", "tick", Vec::new())
        .expect("call stateful project again");
    vm.load_script_project("project", STATEFUL_PROJECT_ENTRY)
        .expect("reload stateful project");
    let after_reload = vm
        .call_function("project", "tick", Vec::new())
        .expect("call reloaded stateful project");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(first, json!(1));
    assert_eq!(second, json!(2));
    assert_eq!(after_reload, json!(1));
}

#[test]
fn load_script_project_ignores_type_only_module_references() {
    let cache_dir = TestCacheDir::new("load-script-project-type-only");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    vm.load_script_project("project", TYPE_ONLY_RUNTIME_ENTRY)
        .expect("load project with missing type-only modules");
    let result = vm
        .call_function("project", "read", vec![json!(null)])
        .expect("call export after type-only references are erased");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(result, json!("empty"));
}

#[test]
fn load_script_project_resolves_tsconfig_aliases() {
    let cache_dir = TestCacheDir::new("load-script-project-tsconfig-alias");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    vm.load_script_project("project", TSCONFIG_ALIAS_ENTRY)
        .expect("load project with tsconfig aliases");
    let result = vm
        .call_function("project", "renderInvoice", vec![json!(21)])
        .expect("call alias-backed project export");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(
        result,
        json!({
            "label": "invoice-42",
            "audit": "feature-ready"
        })
    );
}

#[test]
fn load_script_project_resolves_package_imports_from_local_node_modules() {
    let cache_dir = TestCacheDir::new("load-script-project-package-import");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    vm.load_script_project("project", PACKAGE_IMPORT_ENTRY)
        .expect("load project with package import");
    let result = vm
        .call_function("project", "run", vec![json!(4)])
        .expect("call package-backed project export");
    let stats = vm.stats().expect("collect stats");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(result, json!("demo:12"));
    assert_eq!(stats.memory.module_dependency_edges, 2);
}

#[test]
fn load_script_project_reuses_module_graph_cache_for_same_project() {
    let cache_dir = TestCacheDir::new("project-cache-reuse");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    let first = vm
        .load_script_project("project-a", MULTI_MODULE_ENTRY)
        .expect("load first project");
    vm.unload_script("project-a").expect("unload first project");

    let second = vm
        .load_script_project("project-b", MULTI_MODULE_ENTRY)
        .expect("load second project");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(first.cache_key, second.cache_key);
    assert_eq!(first.transpiled_path, second.transpiled_path);
}

#[test]
fn load_script_project_invalidates_cache_when_dependency_changes() {
    let cache_dir = TestCacheDir::new("project-cache-invalidation");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");
    let project_root = cache_dir.path().join("project");
    let src_dir = project_root.join("src");
    let entry_path = project_root.join("main.ts");
    let dep_path = src_dir.join("value.ts");

    let _ = fs::remove_dir_all(&project_root);
    fs::create_dir_all(&src_dir).expect("create temp project src");
    fs::write(
        &entry_path,
        "import { current } from \"./src/value\";\nexport function read() {\n  return current;\n}\n",
    )
    .expect("write project entry");
    fs::write(&dep_path, "export const current = 1;\n").expect("write project dependency");

    let first = vm
        .load_script_project("project-a", &entry_path)
        .expect("load first project version");
    let first_value = vm
        .call_function("project-a", "read", Vec::new())
        .expect("call first project version");
    vm.unload_script("project-a").expect("unload first project");

    fs::write(&dep_path, "export const current = 2;\n").expect("rewrite project dependency");

    let second = vm
        .load_script_project("project-b", &entry_path)
        .expect("load second project version");
    let second_value = vm
        .call_function("project-b", "read", Vec::new())
        .expect("call second project version");

    vm.shutdown().expect("shutdown vm");
    let _ = fs::remove_dir_all(&project_root);

    assert_eq!(first_value, json!(1));
    assert_eq!(second_value, json!(2));
    assert_ne!(first.cache_key, second.cache_key);
    assert_ne!(first.transpiled_path, second.transpiled_path);
}

#[test]
fn load_script_project_invalidates_cache_when_package_manifest_changes() {
    let cache_dir = TestCacheDir::new("project-package-cache-invalidation");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");
    let project_root = cache_dir.path().join("project");
    let package_dir = project_root.join("node_modules").join("demo-pkg");
    let entry_path = project_root.join("main.ts");
    let manifest_path = package_dir.join("package.json");

    let _ = fs::remove_dir_all(&project_root);
    fs::create_dir_all(&package_dir).expect("create temp package");
    fs::write(
        &entry_path,
        "import { value } from \"demo-pkg\";\nexport function read() {\n  return value;\n}\n",
    )
    .expect("write project entry");
    fs::write(package_dir.join("index.ts"), "export const value = 7;\n")
        .expect("write package entry");
    fs::write(
        &manifest_path,
        r#"{"name":"demo-pkg","version":"1.0.0","main":"index.ts"}"#,
    )
    .expect("write package manifest");

    let first = vm
        .load_script_project("project-a", &entry_path)
        .expect("load first package project version");
    vm.unload_script("project-a").expect("unload first project");

    fs::write(
        &manifest_path,
        r#"{"name":"demo-pkg","version":"1.0.1","main":"index.ts"}"#,
    )
    .expect("rewrite package manifest");

    let second = vm
        .load_script_project("project-b", &entry_path)
        .expect("load second package project version");

    vm.shutdown().expect("shutdown vm");
    let _ = fs::remove_dir_all(&project_root);

    assert_ne!(first.cache_key, second.cache_key);
    assert_ne!(first.transpiled_path, second.transpiled_path);
}

#[test]
fn load_script_project_rejects_unresolved_package_imports() {
    let cache_dir = TestCacheDir::new("project-bare-import");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    let error = vm
        .load_script_project("project", INVALID_BARE_IMPORT_ENTRY)
        .expect_err("missing package imports should be rejected");

    vm.shutdown().expect("shutdown vm");

    assert!(matches!(
        error,
        VmError::Resolve { details }
            if details.contains("unable to resolve import `pkg/math`")
    ));
}

#[test]
fn load_script_project_rejects_dynamic_imports() {
    let cache_dir = TestCacheDir::new("project-dynamic-import");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    let error = vm
        .load_script_project("project", DYNAMIC_IMPORT_ENTRY)
        .expect_err("dynamic imports should be rejected");

    vm.shutdown().expect("shutdown vm");

    assert!(matches!(
        error,
        VmError::Resolve { details }
            if details.contains("dynamic import is not supported in V0 module graphs")
    ));
}

#[test]
fn load_script_rejects_dynamic_imports_before_cache_lookup() {
    let cache_dir = TestCacheDir::new("inline-dynamic-import");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    let error = vm
        .load_script(
            "inline",
            r#"
                export async function load() {
                    return import("./lazy");
                }
            "#,
        )
        .expect_err("dynamic imports should be rejected");

    vm.shutdown().expect("shutdown vm");

    assert!(matches!(
        error,
        VmError::Resolve { details }
            if details.contains("dynamic import is not supported in V0 module graphs")
    ));
}

#[test]
fn call_function_once_project_executes_and_releases_module_graph() {
    let cache_dir = TestCacheDir::new("call-function-once-project");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    let result = vm
        .call_function_once_project(
            MULTI_MODULE_ENTRY,
            "compute",
            vec![json!({ "left": 1, "right": 2 })],
        )
        .expect("call project oneshot");
    let stats = vm.stats().expect("collect stats");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(result, json!({ "total": 3, "version": "v3" }));
    assert_eq!(stats.loaded_scripts, 0);
}
