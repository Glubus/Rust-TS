#![cfg(feature = "async-promise")]

mod support;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::json;
use ts_embed_vm::{
    AsyncHostFunction, HostContract, HostContractKind, RuntimeExecutionLane, Schema,
    ScriptMaterializationState, TsType, TsVm, VmError, VmEvent,
};

use support::TestCacheDir;

const ASYNC_TS_SCRIPT: &str = r#"
export async function lookup(id: number): Promise<{ name: string }> {
  return {
    name: await user.find(id),
  };
}
"#;
const ASYNC_TS_SCRIPT_V2: &str = r#"
export async function lookup(id: number): Promise<{ name: string }> {
  return {
    name: `${await user.find(id)}-v2`,
  };
}
"#;
const ASYNC_EVENT_SCRIPT: &str = r#"
ctx.on("score.update", async event => {
  globalThis.lastScoreName = await user.find(event.combo);
});

export function readScoreName(): string {
  return globalThis.lastScoreName ?? "missing";
}
"#;
const ASYNC_EVENT_SCRIPT_NO_HOST: &str = r#"
ctx.on("score.update", async event => {
  globalThis.lastScore = event.combo;
});

export function readScore(): number {
  return globalThis.lastScore ?? 0;
}
"#;
const ASYNC_PARALLEL_SCRIPT: &str = r#"
export async function lookupMany(): Promise<string[]> {
  return await Promise.all([
    user.slowFind(1),
    user.slowFind(2),
    user.slowFind(3),
  ]);
}
"#;
const SYNC_EVENT_SCRIPT: &str = include_str!("projects/event_listener/main.ts");
const ASYNC_PROJECT_ENTRY: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/projects/async_promise_project/main.ts"
);
const ASYNC_MOD_PACK_ENTRY: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/projects/async_mod_pack/src/main.ts"
);

static ACTIVE_SLOW_CALLS: AtomicUsize = AtomicUsize::new(0);
static MAX_ACTIVE_SLOW_CALLS: AtomicUsize = AtomicUsize::new(0);

struct AsyncFindUser;
struct AsyncSlowFindUser;

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

impl HostContract for AsyncSlowFindUser {
    const NAME: &'static str = "user.slowFind";

    fn schema() -> Schema {
        Schema::typed("SlowFindUserInput", TsType::Number)
    }

    fn kind() -> HostContractKind {
        HostContractKind::Function
    }
}

impl AsyncHostFunction for AsyncSlowFindUser {
    type Future =
        std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self::Output, VmError>> + Send>>;
    type Input = u64;
    type Output = String;

    fn output_schema() -> Schema {
        Schema::typed("SlowFindUserOutput", TsType::String)
    }

    fn call_async(input: Self::Input) -> Self::Future {
        Box::pin(async move {
            let active = ACTIVE_SLOW_CALLS.fetch_add(1, Ordering::SeqCst) + 1;
            record_max_active_slow_calls(active);
            std::thread::sleep(Duration::from_millis(50));
            ACTIVE_SLOW_CALLS.fetch_sub(1, Ordering::SeqCst);
            Ok(format!("slow-user-{input}"))
        })
    }
}

fn reset_slow_call_counters() {
    ACTIVE_SLOW_CALLS.store(0, Ordering::SeqCst);
    MAX_ACTIVE_SLOW_CALLS.store(0, Ordering::SeqCst);
}

fn record_max_active_slow_calls(active: usize) {
    let mut current = MAX_ACTIVE_SLOW_CALLS.load(Ordering::SeqCst);
    while active > current {
        match MAX_ACTIVE_SLOW_CALLS.compare_exchange(
            current,
            active,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => break,
            Err(observed) => current = observed,
        }
    }
}

fn max_active_slow_calls() -> usize {
    MAX_ACTIVE_SLOW_CALLS.load(Ordering::SeqCst)
}

#[test]
fn manager_compiles_caches_and_loads_async_worker_lane_script() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        let cache_dir = TestCacheDir::new("async-promise-manager");
        let vm = TsVm::new(cache_dir.vm_options()).expect("create vm");
        let subscription = vm.subscribe();
        vm.registry()
            .async_promise_function::<AsyncFindUser>()
            .expect("register async promise function");

        let script = vm
            .load_async_script("async-ts", ASYNC_TS_SCRIPT)
            .await
            .expect("load async worker-lane script");
        let loaded_event = subscription.recv().expect("receive async load event");
        let cached_again = vm
            .load_async_script("async-ts-again", ASYNC_TS_SCRIPT)
            .await
            .expect("load async worker-lane script from cache");
        let cached_again_key = cached_again.cache_key().to_owned();
        let _cached_again_loaded = subscription.recv().expect("receive cache probe load event");
        drop(cached_again);
        let _cached_again_unloaded = subscription
            .recv()
            .expect("receive cache probe unload event");
        let mounted_stats = vm.stats().expect("collect mounted async stats");
        let result = script
            .call_function("lookup", &[json!(42)])
            .await
            .expect("call async script export");
        let mounted_entry = vm
            .describe_script("async-ts")
            .expect("describe async script")
            .expect("async script entry exists");
        let called_event = subscription.recv().expect("receive async call event");

        assert_eq!(mounted_entry.state, ScriptMaterializationState::Mounted);
        assert_eq!(mounted_entry.preferred_runner, Some(script.worker_id()));
        assert_eq!(
            loaded_event,
            VmEvent::AsyncScriptLoaded {
                script_id: String::from("async-ts"),
                source_kind: ts_embed_vm::ScriptSourceKind::Inline,
                cache_key: script.cache_key().to_owned(),
            }
        );
        assert_eq!(
            called_event,
            VmEvent::AsyncFunctionCalled {
                script_id: String::from("async-ts"),
                function_name: String::from("lookup"),
                result: json!({ "name": "async-user-42" }),
            }
        );
        assert_eq!(script.cache_key(), cached_again_key);
        assert_eq!(mounted_stats.loaded_scripts, 1);
        assert_eq!(mounted_stats.memory.active_scripts, 1);
        assert!(script.subscriptions().is_empty());
        assert!(script.module_dependencies().is_empty());
        assert_eq!(script.module_ids().len(), 1);
        assert_eq!(result, json!({ "name": "async-user-42" }));

        drop(script);
        let unloaded_event = subscription.recv().expect("receive async unload event");
        let dropped_stats = vm.stats().expect("collect dropped async stats");
        let compiled_entry = vm
            .describe_script("async-ts")
            .expect("describe dropped async script")
            .expect("async script entry exists after drop");
        vm.shutdown().expect("shutdown vm");

        assert_eq!(
            unloaded_event,
            VmEvent::AsyncScriptUnloaded {
                script_id: String::from("async-ts"),
            }
        );
        assert_eq!(dropped_stats.loaded_scripts, 0);
        assert_eq!(dropped_stats.memory.active_scripts, 0);
        assert_eq!(compiled_entry.state, ScriptMaterializationState::Compiled);
    });
}

#[test]
fn manager_compiles_caches_and_loads_async_worker_lane_project() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        let cache_dir = TestCacheDir::new("async-promise-manager-project");
        let vm = TsVm::new(cache_dir.vm_options()).expect("create vm");
        let subscription = vm.subscribe();
        vm.registry()
            .async_promise_function::<AsyncFindUser>()
            .expect("register async promise function");

        let script = vm
            .load_async_script_project("async-project", ASYNC_PROJECT_ENTRY)
            .await
            .expect("load async worker-lane project");
        let loaded_event = subscription
            .recv()
            .expect("receive async project load event");
        let cached_again = vm
            .load_async_script_project("async-project-again", ASYNC_PROJECT_ENTRY)
            .await
            .expect("load async worker-lane project from cache");
        let cached_again_key = cached_again.cache_key().to_owned();
        let _cached_again_loaded = subscription
            .recv()
            .expect("receive project cache probe load event");
        drop(cached_again);
        let _cached_again_unloaded = subscription
            .recv()
            .expect("receive project cache probe unload event");
        let mounted_stats = vm.stats().expect("collect mounted async project stats");
        let result = script
            .call_function("lookup", &[json!(7)])
            .await
            .expect("call async project export");
        let mounted_entry = vm
            .describe_script("async-project")
            .expect("describe async project")
            .expect("async project entry exists");
        let called_event = subscription
            .recv()
            .expect("receive async project call event");

        assert_eq!(mounted_entry.state, ScriptMaterializationState::Mounted);
        assert_eq!(mounted_entry.preferred_runner, Some(script.worker_id()));
        assert_eq!(
            loaded_event,
            VmEvent::AsyncScriptLoaded {
                script_id: String::from("async-project"),
                source_kind: ts_embed_vm::ScriptSourceKind::Project,
                cache_key: script.cache_key().to_owned(),
            }
        );
        assert_eq!(
            called_event,
            VmEvent::AsyncFunctionCalled {
                script_id: String::from("async-project"),
                function_name: String::from("lookup"),
                result: json!({ "name": "async-user-7" }),
            }
        );
        assert_eq!(script.cache_key(), cached_again_key);
        assert_eq!(mounted_stats.loaded_scripts, 1);
        assert_eq!(mounted_stats.memory.active_scripts, 1);
        assert!(script.subscriptions().is_empty());
        assert_eq!(script.module_ids().len(), 2);
        assert_eq!(script.module_dependencies().len(), 1);
        assert_eq!(result, json!({ "name": "async-user-7" }));

        drop(script);
        let unloaded_event = subscription
            .recv()
            .expect("receive async project unload event");
        let dropped_stats = vm.stats().expect("collect dropped async project stats");
        let compiled_entry = vm
            .describe_script("async-project")
            .expect("describe dropped async project")
            .expect("async project entry exists after drop");
        vm.shutdown().expect("shutdown vm");

        assert_eq!(
            unloaded_event,
            VmEvent::AsyncScriptUnloaded {
                script_id: String::from("async-project"),
            }
        );
        assert_eq!(dropped_stats.loaded_scripts, 0);
        assert_eq!(dropped_stats.memory.active_scripts, 0);
        assert_eq!(compiled_entry.state, ScriptMaterializationState::Compiled);
    });
}

#[test]
fn realistic_async_mod_pack_uses_host_promises_events_state_and_graphs() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        let cache_dir = TestCacheDir::new("realistic-async-mod-pack");
        let vm = TsVm::new(cache_dir.vm_options()).expect("create vm");
        vm.registry()
            .async_promise_function::<AsyncFindUser>()
            .expect("register async promise function");

        let script = vm
            .load_async_script_project("async-mod-pack", ASYNC_MOD_PACK_ENTRY)
            .await
            .expect("load realistic async mod pack");
        let profile = script
            .call_function("lookupProfile", &[json!(7)])
            .await
            .expect("lookup async profile");
        let party = script
            .call_function("lookupParty", &[json!([2, 3])])
            .await
            .expect("lookup async party");
        let delivered = vm
            .emit_async("score.update", json!({ "combo": 11, "userId": 9 }))
            .await
            .expect("emit async score event");
        let ledger = script
            .call_function("readLedger", &[])
            .await
            .expect("read async ledger");
        let snapshot = vm.runtime_snapshot().expect("collect runtime snapshot");

        vm.shutdown().expect("shutdown vm");

        assert_eq!(profile, json!({ "label": "async-user-7#7", "total": 7 }));
        assert_eq!(
            party,
            json!({
                "labels": ["async-user-2#2", "async-user-3#3"],
                "total": 12,
            })
        );
        assert_eq!(delivered, 1);
        assert_eq!(
            ledger,
            json!({
                "entries": [
                    { "kind": "lookup", "user": "async-user-7", "value": 7 },
                    { "kind": "lookup", "user": "async-user-2", "value": 2 },
                    { "kind": "lookup", "user": "async-user-3", "value": 3 },
                    { "kind": "event", "user": "async-user-9", "value": 11 },
                ],
                "total": 23,
            })
        );
        assert_eq!(script.subscriptions(), &[String::from("score.update")]);
        assert_eq!(script.module_ids().len(), 4);
        assert_eq!(script.module_dependencies().len(), 5);
        assert_eq!(snapshot.stats.memory.active_scripts, 1);
        assert_eq!(snapshot.stats.memory.event_route_bindings, 1);
        assert_eq!(snapshot.stats.memory.module_dependency_edges, 5);
        assert_eq!(snapshot.event_routes.len(), 1);
        assert_eq!(snapshot.event_routes[0].event_name, "score.update");
        assert_eq!(
            snapshot.event_routes[0].bindings[0].execution_lane,
            RuntimeExecutionLane::Async
        );
    });
}

#[test]
fn manager_distributes_async_scripts_across_async_worker_lanes() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        let cache_dir = TestCacheDir::new("async-promise-worker-lanes");
        let mut options = cache_dir.vm_options();
        options.worker_threads = 2;
        let vm = TsVm::new(options).expect("create vm");
        vm.registry()
            .async_promise_function::<AsyncFindUser>()
            .expect("register async promise function");

        let first = vm
            .load_async_script("async-a", ASYNC_TS_SCRIPT)
            .await
            .expect("load first async script");
        let second = vm
            .load_async_script("async-b", ASYNC_TS_SCRIPT)
            .await
            .expect("load second async script");

        let first_entry = vm
            .describe_script("async-a")
            .expect("describe first async script")
            .expect("first async script entry");
        let second_entry = vm
            .describe_script("async-b")
            .expect("describe second async script")
            .expect("second async script entry");

        vm.shutdown().expect("shutdown vm");

        assert_ne!(first.worker_id(), second.worker_id());
        assert_eq!(first_entry.preferred_runner, Some(first.worker_id()));
        assert_eq!(second_entry.preferred_runner, Some(second.worker_id()));
    });
}

#[test]
fn manager_routes_host_events_to_async_worker_lane_subscriptions() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        let cache_dir = TestCacheDir::new("async-promise-event-routing");
        let vm = TsVm::new(cache_dir.vm_options()).expect("create vm");
        vm.registry()
            .async_promise_function::<AsyncFindUser>()
            .expect("register async promise function");

        let script = vm
            .load_async_script("async-listener", ASYNC_EVENT_SCRIPT)
            .await
            .expect("load async listener");
        let mounted_stats = vm.stats().expect("collect async listener stats");
        let snapshot = vm.runtime_snapshot().expect("collect runtime snapshot");

        let delivered = vm
            .emit_async("score.update", json!({ "combo": 12 }))
            .await
            .expect("emit host event to async listener");
        let score_name = script
            .call_function("readScoreName", &[])
            .await
            .expect("read async event side effect");
        let routed_stats = vm.stats().expect("collect routed async stats");

        vm.shutdown().expect("shutdown vm");

        assert_eq!(script.subscriptions(), &[String::from("score.update")]);
        assert_eq!(mounted_stats.loaded_scripts, 1);
        assert_eq!(mounted_stats.memory.active_scripts, 1);
        assert_eq!(mounted_stats.memory.event_route_bindings, 1);
        assert_eq!(mounted_stats.workers[0].loaded_scripts, 1);
        assert_eq!(mounted_stats.workers[0].active_scripts, 1);
        assert_eq!(mounted_stats.workers[0].event_route_bindings, 1);
        assert_eq!(mounted_stats.workers[0].queue_depth, 0);
        assert_eq!(mounted_stats.workers[0].queue_rejected_sends, 0);
        assert_eq!(mounted_stats.workers[0].async_queue_depth, 0);
        assert!(mounted_stats.workers[0].async_queue_peak_depth >= 1);
        assert_eq!(mounted_stats.workers[0].async_queue_rejected_sends, 0);
        assert_eq!(mounted_stats.workers[0].async_latency.load_operations, 1);
        assert!(mounted_stats.workers[0].async_latency.load_total_ns > 0);
        assert_eq!(mounted_stats.workers[0].async_latency.call_operations, 0);
        assert_eq!(mounted_stats.workers[0].async_latency.emit_operations, 0);
        assert_eq!(routed_stats.workers[0].async_latency.load_operations, 1);
        assert_eq!(routed_stats.workers[0].async_latency.call_operations, 1);
        assert_eq!(routed_stats.workers[0].async_latency.emit_operations, 1);
        assert!(routed_stats.workers[0].async_latency.call_total_ns > 0);
        assert!(routed_stats.workers[0].async_latency.emit_total_ns > 0);
        assert!(mounted_stats.workers[0].sync_quickjs_memory.is_some());
        let async_memory = mounted_stats.workers[0]
            .async_quickjs_memory
            .expect("async quickjs memory stats");
        assert!(async_memory.memory_used_bytes > 0);
        assert_eq!(async_memory.malloc_limit_bytes, 16 * 1024 * 1024);
        assert_eq!(snapshot.event_routes.len(), 1);
        assert_eq!(snapshot.event_routes[0].event_name, "score.update");
        assert_eq!(snapshot.event_routes[0].bindings.len(), 1);
        assert_eq!(
            snapshot.event_routes[0].bindings[0].execution_lane,
            RuntimeExecutionLane::Async
        );
        assert_eq!(delivered, 1);
        assert_eq!(score_name, json!("async-user-12"));
    });
}

#[test]
fn async_worker_lane_host_promises_run_concurrently() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        reset_slow_call_counters();
        let cache_dir = TestCacheDir::new("async-promise-concurrent-host-calls");
        let vm = TsVm::new(cache_dir.vm_options()).expect("create vm");
        vm.registry()
            .async_promise_function::<AsyncSlowFindUser>()
            .expect("register slow async promise function");

        let script = vm
            .load_async_script("async-parallel", ASYNC_PARALLEL_SCRIPT)
            .await
            .expect("load async parallel script");
        let result = script
            .call_function("lookupMany", &[])
            .await
            .expect("call parallel async host functions");

        vm.shutdown().expect("shutdown vm");

        assert_eq!(result, json!(["slow-user-1", "slow-user-2", "slow-user-3"]));
        assert!(
            max_active_slow_calls() >= 2,
            "host promise calls were serialized instead of overlapping"
        );
    });
}

#[test]
fn emit_async_routes_to_sync_and_async_worker_lanes() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        let cache_dir = TestCacheDir::new("async-promise-mixed-event-routing");
        let vm = TsVm::new(cache_dir.vm_options()).expect("create vm");

        vm.load_script("sync-listener", SYNC_EVENT_SCRIPT)
            .expect("load sync listener");
        let async_script = vm
            .load_async_script("async-listener", ASYNC_EVENT_SCRIPT_NO_HOST)
            .await
            .expect("load async listener");

        let delivered = vm
            .emit_async("score.update", json!({ "combo": 21 }))
            .await
            .expect("emit event through async facade");
        let sync_score = vm
            .call_function("sync-listener", "readScore", Vec::new())
            .expect("read sync listener score");
        let async_score = async_script
            .call_function("readScore", &[])
            .await
            .expect("read async listener score");

        vm.shutdown().expect("shutdown vm");

        assert_eq!(delivered, 3);
        assert_eq!(sync_score, json!(21));
        assert_eq!(async_score, json!(21));
    });
}

#[test]
fn async_managed_script_can_be_called_from_tokio_spawned_task() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        let cache_dir = TestCacheDir::new("async-promise-send-handle");
        let vm = TsVm::new(cache_dir.vm_options()).expect("create vm");
        vm.registry()
            .async_promise_function::<AsyncFindUser>()
            .expect("register async promise function");

        let script = vm
            .load_async_script("async-send", ASYNC_TS_SCRIPT)
            .await
            .expect("load async script");
        let join = tokio::spawn(async move { script.call_function("lookup", &[json!(11)]).await });
        let result = join
            .await
            .expect("join spawned async script call")
            .expect("call async script from spawned task");

        vm.shutdown().expect("shutdown vm");

        assert_eq!(result, json!({ "name": "async-user-11" }));
    });
}

#[test]
fn dropping_stale_async_handle_does_not_demount_newer_same_id_entry() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        let cache_dir = TestCacheDir::new("async-promise-stale-drop");
        let vm = TsVm::new(cache_dir.vm_options()).expect("create vm");
        vm.registry()
            .async_promise_function::<AsyncFindUser>()
            .expect("register async promise function");

        let first = vm
            .load_async_script("same-id", ASYNC_TS_SCRIPT)
            .await
            .expect("load first async script");
        let second = vm
            .load_async_script("same-id", ASYNC_TS_SCRIPT_V2)
            .await
            .expect("load second async script");

        assert_ne!(first.cache_key(), second.cache_key());

        drop(first);
        let still_mounted = vm
            .describe_script("same-id")
            .expect("describe same-id after stale drop")
            .expect("same-id remains registered");
        let still_active = vm.stats().expect("collect stats after stale drop");
        let result = second
            .call_function("lookup", &[json!(5)])
            .await
            .expect("call newer async script");

        vm.shutdown().expect("shutdown vm");

        assert_eq!(still_mounted.state, ScriptMaterializationState::Mounted);
        assert_eq!(still_mounted.source_hash, second.cache_key());
        assert_eq!(still_active.memory.active_scripts, 1);
        assert_eq!(result, json!({ "name": "async-user-5-v2" }));
    });
}
