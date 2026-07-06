mod support;

use std::fs;
use std::sync::{Arc, Barrier};

use serde_json::json;
use ts_embed_vm::{
    HostCallback, HostContract, HostContractKind, Schema, ScriptRetentionPolicy, TsField, TsType,
    TsVm, VmError, VmOptions,
};

use support::TestCacheDir;

const BASIC_SCRIPT: &str = include_str!("projects/basic_math/main.ts");
const EVENT_LISTENER_SCRIPT: &str = include_str!("projects/event_listener/main.ts");
const PROJECT_MAIN_V1: &str = include_str!("projects/reloadable_project/main_v1.ts");
const PROJECT_MAIN_V2: &str = include_str!("projects/reloadable_project/main_v2.ts");
const CONCURRENT_THREAD_COUNT: usize = 8;
const CONCURRENT_ITERATIONS: usize = 25;
const CONCURRENT_MATH_SCRIPTS: usize = 8;
const CONCURRENT_LISTENER_SCRIPTS: usize = 8;
const CONCURRENT_EVENT_HANDLERS_PER_SCRIPT: usize = 2;
const CONCURRENT_RELOAD_ITERATIONS: usize = 20;
const CONCURRENT_RELOAD_EMITTER_THREADS: usize = 4;
const CONCURRENT_RELOAD_EMITS_PER_THREAD: usize = 25;
const CONCURRENT_LOAD_THREADS: usize = 8;
const CONCURRENT_LOADS_PER_THREAD: usize = 6;
const CONCURRENT_SAME_SCRIPT_RELOAD_THREADS: usize = 8;
const CONCURRENT_SAME_SCRIPT_RELOADS_PER_THREAD: usize = 6;
const CONCURRENT_ONESHOT_THREADS: usize = 8;
const CONCURRENT_ONESHOT_CALLS_PER_THREAD: usize = 12;
const CONCURRENT_DEPENDENCY_THREADS: usize = 8;
const CONCURRENT_DEPENDENCY_ROUNDS: usize = 12;
const CONCURRENT_DEPENDENCY_EDGE_CONSUMERS: usize = 8;
const CONCURRENT_DEPENDENCY_EDGE_ROUNDS: usize = 10;
const CONCURRENT_UNLOAD_LISTENER_SCRIPTS: usize = 16;
const CONCURRENT_UNLOAD_EMITTER_THREADS: usize = 4;
const CONCURRENT_UNLOAD_EMITS_PER_THREAD: usize = 30;

struct ScoreUpdate;

impl HostContract for ScoreUpdate {
    const NAME: &'static str = "score.update";
    const IMPORT_MODULE: &'static str = "test";
    const EXPORT_PATH: &'static [&'static str] = &["score", "onUpdate"];

    fn schema() -> Schema {
        Schema::typed(
            "ScorePayload",
            TsType::Object(vec![TsField::required("combo", TsType::Number)]),
        )
    }

    fn kind() -> HostContractKind {
        HostContractKind::Callback
    }
}

impl HostCallback for ScoreUpdate {
    type Payload = serde_json::Value;
}

fn register_score_update(vm: &TsVm) {
    vm.registry()
        .callback::<ScoreUpdate>()
        .expect("register score update callback");
}

#[test]
fn repeated_project_reloads_keep_graph_and_routes_consistent() {
    let cache_dir = TestCacheDir::new("stress-project-reload");
    let vm = TsVm::new(cache_dir.vm_options()).expect("create vm");
    register_score_update(&vm);
    let project_root = cache_dir.path().join("project");
    let entry_path = project_root.join("main.ts");
    let value_path = project_root.join("value.ts");
    let extra_path = project_root.join("extra.ts");

    fs::create_dir_all(&project_root).expect("create project");

    for iteration in 0..30 {
        write_reloadable_project(iteration, &entry_path, &value_path, &extra_path);
        vm.load_script_project("project", &entry_path)
            .expect("reload project");

        let stats = vm.stats().expect("collect stats");
        let delivered = vm
            .emit("score.update", json!({ "combo": 5 }))
            .expect("emit routed event");
        let score = vm
            .call_function("project", "readScore", Vec::new())
            .expect("read score");

        assert_eq!(delivered, 1);
        assert_eq!(stats.memory.event_route_bindings, 1);
        assert_eq!(
            stats.memory.module_dependency_edges,
            expected_module_edges(iteration)
        );
        assert_eq!(score, json!(expected_score(iteration)));
    }

    vm.shutdown().expect("shutdown vm");
}

#[test]
fn repeated_event_dispatch_keeps_hot_routes_stable() {
    let cache_dir = TestCacheDir::new("stress-event-dispatch");
    let mut options = cache_dir.vm_options();
    options.worker_threads = 2;
    options.max_scripts_per_worker = 64;
    let vm = TsVm::new(options).expect("create vm");
    register_score_update(&vm);

    for index in 0..20 {
        vm.load_script(format!("listener-{index}"), EVENT_LISTENER_SCRIPT)
            .expect("load listener");
    }

    for combo in 0..100 {
        let delivered = vm
            .emit("score.update", json!({ "combo": combo }))
            .expect("emit event");
        assert_eq!(delivered, 40);
    }

    let snapshot = vm.runtime_snapshot().expect("collect runtime snapshot");
    let last_score = vm
        .call_function("listener-19", "readScore", Vec::new())
        .expect("read last score");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(last_score, json!(99));
    assert_eq!(snapshot.stats.latency.emit_operations, 100);
    assert!(snapshot.stats.latency.emit_average_ns > 0);
    assert!(snapshot.stats.latency.emit_max_ns >= snapshot.stats.latency.emit_average_ns);
    assert_eq!(snapshot.stats.memory.event_route_bindings, 20);
    assert_eq!(snapshot.event_routes.len(), 1);
    assert_eq!(snapshot.event_routes[0].bindings.len(), 20);
}

#[test]
fn concurrent_host_calls_and_events_keep_routes_and_stats_consistent() {
    let cache_dir = TestCacheDir::new("stress-concurrent-host-calls-events");
    let vm = Arc::new(TsVm::new(concurrent_stress_options(&cache_dir)).expect("create vm"));
    register_score_update(&vm);

    load_concurrent_stress_scripts(&vm);
    run_concurrent_host_operations(Arc::clone(&vm));

    let snapshot = vm.runtime_snapshot().expect("collect runtime snapshot");
    vm.shutdown().expect("shutdown vm");

    let expected_operations = (CONCURRENT_THREAD_COUNT * CONCURRENT_ITERATIONS) as u64;
    assert_eq!(snapshot.stats.latency.call_operations, expected_operations);
    assert_eq!(snapshot.stats.latency.emit_operations, expected_operations);
    assert_eq!(
        snapshot.stats.memory.event_route_bindings,
        CONCURRENT_LISTENER_SCRIPTS
    );
    assert_eq!(snapshot.event_routes.len(), 1);
    assert_eq!(
        snapshot.event_routes[0].bindings.len(),
        CONCURRENT_LISTENER_SCRIPTS
    );
    assert_worker_call_and_emit_stats(&snapshot.stats);
}

#[test]
fn concurrent_distinct_script_loads_keep_registry_and_workers_consistent() {
    let cache_dir = TestCacheDir::new("stress-concurrent-distinct-loads");
    let vm = Arc::new(TsVm::new(concurrent_load_options(&cache_dir)).expect("create vm"));

    run_concurrent_distinct_script_loads(Arc::clone(&vm));

    let stats = vm.stats().expect("collect stats after concurrent loads");
    assert_eq!(stats.loaded_scripts, expected_concurrent_load_count());
    assert_eq!(
        stats.memory.active_scripts,
        expected_concurrent_load_count()
    );
    assert_eq!(stats.worker_count, 4);
    assert!(
        stats.workers.iter().all(|worker| worker.loaded_scripts > 0),
        "concurrent loads should be distributed across all workers: {:?}",
        stats.workers
    );

    assert_concurrently_loaded_scripts_are_callable(&vm);

    vm.shutdown().expect("shutdown vm");
}

#[test]
fn concurrent_same_script_reloads_keep_single_active_instance() {
    let cache_dir = TestCacheDir::new("stress-concurrent-same-script-reloads");
    let vm = Arc::new(TsVm::new(concurrent_load_options(&cache_dir)).expect("create vm"));

    run_concurrent_same_script_reloads(Arc::clone(&vm));

    let stats = vm.stats().expect("collect stats after concurrent reloads");
    let version = vm
        .call_function("shared", "version", Vec::new())
        .expect("call shared script after concurrent reloads");
    let version = version
        .as_i64()
        .expect("shared script version should be numeric");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(stats.loaded_scripts, 1);
    assert_eq!(stats.memory.active_scripts, 1);
    assert!(
        (0..expected_concurrent_same_script_reload_count() as i64).contains(&version),
        "unexpected final shared script version: {version}"
    );
}

#[test]
fn concurrent_oneshot_calls_demount_without_leaking_active_instances() {
    let cache_dir = TestCacheDir::new("stress-concurrent-oneshot-calls");
    let vm = Arc::new(TsVm::new(concurrent_load_options(&cache_dir)).expect("create vm"));

    run_concurrent_oneshot_calls(Arc::clone(&vm));

    let snapshot = vm
        .runtime_snapshot()
        .expect("collect snapshot after oneshot calls");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(
        snapshot.stats.latency.call_operations,
        expected_concurrent_oneshot_call_count() as u64
    );
    assert_eq!(snapshot.stats.memory.active_scripts, 0);
    assert_eq!(snapshot.stats.memory.event_route_bindings, 0);
    assert!(snapshot.event_routes.is_empty());
    assert!(
        snapshot
            .scripts
            .iter()
            .all(|script| script.active_worker.is_none())
    );
}

#[test]
fn concurrent_dependency_refs_release_without_leaking_retention() {
    let cache_dir = TestCacheDir::new("stress-concurrent-dependency-refs");
    let vm = Arc::new(TsVm::new(concurrent_load_options(&cache_dir)).expect("create vm"));

    run_concurrent_dependency_ref_rounds(Arc::clone(&vm));

    let snapshot = vm
        .runtime_snapshot()
        .expect("collect snapshot after dependency ref stress");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(snapshot.stats.memory.active_scripts, 0);
    assert_eq!(snapshot.stats.memory.dependency_edges, 0);
    assert!(
        snapshot
            .scripts
            .iter()
            .all(|script| script.active_worker.is_none())
    );
}

#[test]
fn concurrent_dependency_edges_release_without_leaking_refs_or_edges() {
    let cache_dir = TestCacheDir::new("stress-concurrent-dependency-edges");
    let vm = Arc::new(TsVm::new(concurrent_load_options(&cache_dir)).expect("create vm"));

    load_dependency_edge_consumers(&vm);
    run_concurrent_dependency_edge_rounds(Arc::clone(&vm));

    let snapshot = vm
        .runtime_snapshot()
        .expect("collect snapshot after dependency edge stress");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(
        snapshot.stats.memory.active_scripts,
        CONCURRENT_DEPENDENCY_EDGE_CONSUMERS
    );
    assert_eq!(snapshot.stats.memory.dependency_edges, 0);
    assert_eq!(snapshot.stats.memory.event_route_bindings, 0);
    assert!(snapshot.dependency_edges.is_empty());
}

#[test]
fn concurrent_listener_unloads_and_emits_clear_hot_routes_without_leaks() {
    let cache_dir = TestCacheDir::new("stress-concurrent-unload-emits");
    let vm = Arc::new(TsVm::new(concurrent_load_options(&cache_dir)).expect("create vm"));
    register_score_update(&vm);

    load_concurrent_unload_listeners(&vm);
    run_concurrent_listener_unloads_and_emits(Arc::clone(&vm));

    let snapshot = vm
        .runtime_snapshot()
        .expect("collect snapshot after unload/emit stress");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(snapshot.stats.loaded_scripts, 0);
    assert_eq!(snapshot.stats.memory.active_scripts, 0);
    assert_eq!(snapshot.stats.memory.event_route_bindings, 0);
    assert!(snapshot.event_routes.is_empty());
    assert!(
        snapshot
            .scripts
            .iter()
            .all(|script| script.active_worker.is_none())
    );
}

#[test]
fn concurrent_project_reloads_and_emits_keep_hot_route_available() {
    let cache_dir = TestCacheDir::new("stress-concurrent-project-reload-emits");
    let vm = Arc::new(TsVm::new(concurrent_reload_options(&cache_dir)).expect("create vm"));
    register_score_update(&vm);
    let project = Arc::new(ConcurrentReloadProject::create(&cache_dir));

    project.write_version(0);
    vm.load_script_project("project", &project.entry_path)
        .expect("load initial project");

    run_concurrent_project_reloads_and_emits(Arc::clone(&vm), Arc::clone(&project));

    let delivered = vm
        .emit("score.update", json!({ "combo": 11 }))
        .expect("emit after concurrent reloads");
    let snapshot = vm.runtime_snapshot().expect("collect runtime snapshot");
    let score = vm
        .call_function("project", "readScore", Vec::new())
        .expect("read score after concurrent reloads");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(delivered, 1);
    assert_eq!(
        score,
        json!(expected_score_for_combo(
            11,
            CONCURRENT_RELOAD_ITERATIONS as i32
        ))
    );
    assert_eq!(snapshot.stats.memory.event_route_bindings, 1);
    assert_eq!(snapshot.event_routes.len(), 1);
    assert_eq!(snapshot.event_routes[0].bindings.len(), 1);
}

fn concurrent_stress_options(cache_dir: &TestCacheDir) -> VmOptions {
    let mut options = cache_dir.vm_options();
    options.worker_threads = 4;
    options.max_scripts_per_worker = 64;
    options.queue_capacity = 128;
    options
}

fn concurrent_reload_options(cache_dir: &TestCacheDir) -> VmOptions {
    let mut options = cache_dir.vm_options();
    options.worker_threads = 2;
    options.max_scripts_per_worker = 16;
    options.queue_capacity = 128;
    options
}

fn concurrent_load_options(cache_dir: &TestCacheDir) -> VmOptions {
    let mut options = cache_dir.vm_options();
    options.worker_threads = 4;
    options.max_scripts_per_worker = 64;
    options.queue_capacity = 128;
    options
}

fn load_concurrent_stress_scripts(vm: &TsVm) {
    for index in 0..CONCURRENT_MATH_SCRIPTS {
        vm.load_script(format!("math-{index}"), BASIC_SCRIPT)
            .expect("load math script");
    }
    for index in 0..CONCURRENT_LISTENER_SCRIPTS {
        vm.load_script(format!("listener-{index}"), EVENT_LISTENER_SCRIPT)
            .expect("load listener script");
    }
}

fn run_concurrent_distinct_script_loads(vm: Arc<TsVm>) {
    let start_barrier = Arc::new(Barrier::new(CONCURRENT_LOAD_THREADS));
    let mut handles = Vec::with_capacity(CONCURRENT_LOAD_THREADS);

    for thread_index in 0..CONCURRENT_LOAD_THREADS {
        let vm = Arc::clone(&vm);
        let start_barrier = Arc::clone(&start_barrier);
        handles.push(std::thread::spawn(move || {
            start_barrier.wait();
            load_distinct_scripts_for_thread(&vm, thread_index);
        }));
    }

    for handle in handles {
        handle.join().expect("join concurrent load worker");
    }
}

fn load_distinct_scripts_for_thread(vm: &TsVm, thread_index: usize) {
    for load_index in 0..CONCURRENT_LOADS_PER_THREAD {
        let script_id = concurrent_load_script_id(thread_index, load_index);
        vm.load_script(script_id, BASIC_SCRIPT)
            .expect("load distinct script concurrently");
    }
}

fn assert_concurrently_loaded_scripts_are_callable(vm: &TsVm) {
    for thread_index in 0..CONCURRENT_LOAD_THREADS {
        let script_id = concurrent_load_script_id(thread_index, 0);
        let result = vm
            .call_function(
                script_id,
                "sum",
                vec![json!({ "left": thread_index, "right": 10 })],
            )
            .expect("call concurrently loaded script");
        assert_eq!(result, json!(thread_index + 10));
    }
}

fn concurrent_load_script_id(thread_index: usize, load_index: usize) -> String {
    format!("load-{thread_index}-{load_index}")
}

fn expected_concurrent_load_count() -> usize {
    CONCURRENT_LOAD_THREADS * CONCURRENT_LOADS_PER_THREAD
}

fn run_concurrent_same_script_reloads(vm: Arc<TsVm>) {
    let start_barrier = Arc::new(Barrier::new(CONCURRENT_SAME_SCRIPT_RELOAD_THREADS));
    let mut handles = Vec::with_capacity(CONCURRENT_SAME_SCRIPT_RELOAD_THREADS);

    for thread_index in 0..CONCURRENT_SAME_SCRIPT_RELOAD_THREADS {
        let vm = Arc::clone(&vm);
        let start_barrier = Arc::clone(&start_barrier);
        handles.push(std::thread::spawn(move || {
            start_barrier.wait();
            reload_shared_script_for_thread(&vm, thread_index);
        }));
    }

    for handle in handles {
        handle
            .join()
            .expect("join concurrent same-script reload worker");
    }
}

fn reload_shared_script_for_thread(vm: &TsVm, thread_index: usize) {
    for reload_index in 0..CONCURRENT_SAME_SCRIPT_RELOADS_PER_THREAD {
        let version = thread_index * CONCURRENT_SAME_SCRIPT_RELOADS_PER_THREAD + reload_index;
        vm.load_script("shared", version_script(version))
            .expect("reload shared script concurrently");
    }
}

fn version_script(version: usize) -> String {
    format!("export function version() {{ return {version}; }}\n")
}

fn expected_concurrent_same_script_reload_count() -> usize {
    CONCURRENT_SAME_SCRIPT_RELOAD_THREADS * CONCURRENT_SAME_SCRIPT_RELOADS_PER_THREAD
}

fn run_concurrent_oneshot_calls(vm: Arc<TsVm>) {
    let start_barrier = Arc::new(Barrier::new(CONCURRENT_ONESHOT_THREADS));
    let mut handles = Vec::with_capacity(CONCURRENT_ONESHOT_THREADS);

    for thread_index in 0..CONCURRENT_ONESHOT_THREADS {
        let vm = Arc::clone(&vm);
        let start_barrier = Arc::clone(&start_barrier);
        handles.push(std::thread::spawn(move || {
            start_barrier.wait();
            run_oneshot_worker(&vm, thread_index);
        }));
    }

    for handle in handles {
        handle.join().expect("join concurrent oneshot worker");
    }
}

fn run_oneshot_worker(vm: &TsVm, thread_index: usize) {
    for call_index in 0..CONCURRENT_ONESHOT_CALLS_PER_THREAD {
        let left = thread_index as i64;
        let right = call_index as i64;
        let result = vm
            .call_function_once(
                BASIC_SCRIPT,
                "sum",
                vec![json!({ "left": left, "right": right })],
            )
            .expect("call oneshot script concurrently");
        assert_eq!(result, json!(left + right));
    }
}

fn expected_concurrent_oneshot_call_count() -> usize {
    CONCURRENT_ONESHOT_THREADS * CONCURRENT_ONESHOT_CALLS_PER_THREAD
}

fn run_concurrent_dependency_ref_rounds(vm: Arc<TsVm>) {
    for round in 0..CONCURRENT_DEPENDENCY_ROUNDS {
        vm.load_script_with_policy(
            dependency_ref_script_id(round),
            BASIC_SCRIPT,
            ScriptRetentionPolicy::DemountWhenIdle,
        )
        .expect("load dependency-ref stress script");
        run_concurrent_dependency_ref_round(Arc::clone(&vm), round);
        assert_dependency_ref_script_was_demounted(&vm, round);
    }
}

fn run_concurrent_dependency_ref_round(vm: Arc<TsVm>, round: usize) {
    let retain_barrier = Arc::new(Barrier::new(CONCURRENT_DEPENDENCY_THREADS));
    let release_barrier = Arc::new(Barrier::new(CONCURRENT_DEPENDENCY_THREADS));
    let mut handles = Vec::with_capacity(CONCURRENT_DEPENDENCY_THREADS);

    for thread_index in 0..CONCURRENT_DEPENDENCY_THREADS {
        let vm = Arc::clone(&vm);
        let retain_barrier = Arc::clone(&retain_barrier);
        let release_barrier = Arc::clone(&release_barrier);
        handles.push(std::thread::spawn(move || {
            run_dependency_ref_worker(&vm, round, thread_index, &retain_barrier, &release_barrier);
        }));
    }

    for handle in handles {
        handle
            .join()
            .expect("join concurrent dependency ref worker");
    }
}

fn run_dependency_ref_worker(
    vm: &TsVm,
    round: usize,
    thread_index: usize,
    retain_barrier: &Barrier,
    release_barrier: &Barrier,
) {
    let script_id = dependency_ref_script_id(round);
    vm.retain_script_dependency(&script_id)
        .expect("retain dependency ref concurrently");
    retain_barrier.wait();

    let result = vm
        .call_function(
            &script_id,
            "sum",
            vec![json!({ "left": round, "right": thread_index })],
        )
        .expect("call retained dependency script");
    assert_eq!(result, json!(round + thread_index));

    release_barrier.wait();
    vm.release_script_dependency(script_id)
        .expect("release dependency ref concurrently");
}

fn assert_dependency_ref_script_was_demounted(vm: &TsVm, round: usize) {
    let script_id = dependency_ref_script_id(round);
    let error = vm
        .call_function(&script_id, "sum", vec![json!({ "left": 1, "right": 1 })])
        .expect_err("dependency ref script should demount after final release");
    assert!(matches!(
        error,
        VmError::ScriptNotFound { script_id: missing } if missing == script_id
    ));
}

fn dependency_ref_script_id(round: usize) -> String {
    format!("dependency-ref-{round}")
}

fn load_dependency_edge_consumers(vm: &TsVm) {
    for consumer_index in 0..CONCURRENT_DEPENDENCY_EDGE_CONSUMERS {
        vm.load_script(dependency_edge_consumer_id(consumer_index), BASIC_SCRIPT)
            .expect("load dependency edge consumer");
    }
}

fn run_concurrent_dependency_edge_rounds(vm: Arc<TsVm>) {
    for round in 0..CONCURRENT_DEPENDENCY_EDGE_ROUNDS {
        vm.load_script_with_policy(
            dependency_edge_script_id(round),
            BASIC_SCRIPT,
            ScriptRetentionPolicy::DemountWhenIdle,
        )
        .expect("load dependency-edge stress script");
        run_concurrent_dependency_edge_round(Arc::clone(&vm), round);
        assert_dependency_edge_script_was_demounted(&vm, round);
    }
}

fn run_concurrent_dependency_edge_round(vm: Arc<TsVm>, round: usize) {
    let retain_barrier = Arc::new(Barrier::new(CONCURRENT_DEPENDENCY_EDGE_CONSUMERS));
    let release_barrier = Arc::new(Barrier::new(CONCURRENT_DEPENDENCY_EDGE_CONSUMERS));
    let mut handles = Vec::with_capacity(CONCURRENT_DEPENDENCY_EDGE_CONSUMERS);

    for consumer_index in 0..CONCURRENT_DEPENDENCY_EDGE_CONSUMERS {
        let vm = Arc::clone(&vm);
        let retain_barrier = Arc::clone(&retain_barrier);
        let release_barrier = Arc::clone(&release_barrier);
        handles.push(std::thread::spawn(move || {
            run_dependency_edge_worker(
                &vm,
                round,
                consumer_index,
                &retain_barrier,
                &release_barrier,
            );
        }));
    }

    for handle in handles {
        handle
            .join()
            .expect("join concurrent dependency edge worker");
    }
}

fn run_dependency_edge_worker(
    vm: &TsVm,
    round: usize,
    consumer_index: usize,
    retain_barrier: &Barrier,
    release_barrier: &Barrier,
) {
    let consumer_id = dependency_edge_consumer_id(consumer_index);
    let dependency_id = dependency_edge_script_id(round);

    vm.retain_script_dependency_edge(&consumer_id, &dependency_id)
        .expect("retain dependency edge concurrently");
    retain_barrier.wait();

    let result = vm
        .call_function(
            &dependency_id,
            "sum",
            vec![json!({ "left": round, "right": consumer_index })],
        )
        .expect("call retained dependency-edge script");
    assert_eq!(result, json!(round + consumer_index));

    release_barrier.wait();
    vm.release_script_dependency_edge(consumer_id, dependency_id)
        .expect("release dependency edge concurrently");
}

fn assert_dependency_edge_script_was_demounted(vm: &TsVm, round: usize) {
    let script_id = dependency_edge_script_id(round);
    let error = vm
        .call_function(&script_id, "sum", vec![json!({ "left": 1, "right": 1 })])
        .expect_err("dependency edge script should demount after final release");
    assert!(matches!(
        error,
        VmError::ScriptNotFound { script_id: missing } if missing == script_id
    ));
}

fn dependency_edge_consumer_id(consumer_index: usize) -> String {
    format!("dependency-edge-consumer-{consumer_index}")
}

fn dependency_edge_script_id(round: usize) -> String {
    format!("dependency-edge-{round}")
}

fn load_concurrent_unload_listeners(vm: &TsVm) {
    for listener_index in 0..CONCURRENT_UNLOAD_LISTENER_SCRIPTS {
        vm.load_script(
            concurrent_unload_listener_id(listener_index),
            EVENT_LISTENER_SCRIPT,
        )
        .expect("load unload/emit listener");
    }
}

fn run_concurrent_listener_unloads_and_emits(vm: Arc<TsVm>) {
    let participant_count = CONCURRENT_UNLOAD_LISTENER_SCRIPTS + CONCURRENT_UNLOAD_EMITTER_THREADS;
    let start_barrier = Arc::new(Barrier::new(participant_count));
    let mut handles =
        Vec::with_capacity(CONCURRENT_UNLOAD_LISTENER_SCRIPTS + CONCURRENT_UNLOAD_EMITTER_THREADS);

    spawn_concurrent_listener_unload_workers(&mut handles, &vm, &start_barrier);
    spawn_concurrent_unload_emit_workers(&mut handles, &vm, &start_barrier);

    for handle in handles {
        handle.join().expect("join unload/emit stress worker");
    }
}

fn spawn_concurrent_listener_unload_workers(
    handles: &mut Vec<std::thread::JoinHandle<()>>,
    vm: &Arc<TsVm>,
    start_barrier: &Arc<Barrier>,
) {
    for listener_index in 0..CONCURRENT_UNLOAD_LISTENER_SCRIPTS {
        let vm = Arc::clone(vm);
        let start_barrier = Arc::clone(start_barrier);
        handles.push(std::thread::spawn(move || {
            start_barrier.wait();
            vm.unload_script(concurrent_unload_listener_id(listener_index))
                .expect("unload listener during concurrent emit stress");
        }));
    }
}

fn spawn_concurrent_unload_emit_workers(
    handles: &mut Vec<std::thread::JoinHandle<()>>,
    vm: &Arc<TsVm>,
    start_barrier: &Arc<Barrier>,
) {
    for thread_index in 0..CONCURRENT_UNLOAD_EMITTER_THREADS {
        let vm = Arc::clone(vm);
        let start_barrier = Arc::clone(start_barrier);
        handles.push(std::thread::spawn(move || {
            start_barrier.wait();
            run_concurrent_unload_emit_worker(&vm, thread_index);
        }));
    }
}

fn run_concurrent_unload_emit_worker(vm: &TsVm, thread_index: usize) {
    for iteration in 0..CONCURRENT_UNLOAD_EMITS_PER_THREAD {
        let result = vm.emit(
            "score.update",
            json!({ "combo": thread_index * CONCURRENT_UNLOAD_EMITS_PER_THREAD + iteration }),
        );
        assert_valid_unload_race_emit_result(result);
    }
}

fn assert_valid_unload_race_emit_result(result: Result<usize, VmError>) {
    match result {
        Ok(delivered) => assert!(delivered <= max_concurrent_unload_delivery_count()),
        Err(VmError::ScriptNotFound { .. }) => {}
        Err(error) => panic!("unexpected emit error during unload race: {error}"),
    }
}

fn max_concurrent_unload_delivery_count() -> usize {
    CONCURRENT_UNLOAD_LISTENER_SCRIPTS * CONCURRENT_EVENT_HANDLERS_PER_SCRIPT
}

fn concurrent_unload_listener_id(listener_index: usize) -> String {
    format!("unload-listener-{listener_index}")
}

fn run_concurrent_host_operations(vm: Arc<TsVm>) {
    let start_barrier = Arc::new(Barrier::new(CONCURRENT_THREAD_COUNT));
    let mut handles = Vec::with_capacity(CONCURRENT_THREAD_COUNT);

    for thread_index in 0..CONCURRENT_THREAD_COUNT {
        let vm = Arc::clone(&vm);
        let start_barrier = Arc::clone(&start_barrier);
        handles.push(std::thread::spawn(move || {
            start_barrier.wait();
            run_concurrent_host_worker(&vm, thread_index);
        }));
    }

    for handle in handles {
        handle.join().expect("join concurrent host worker");
    }
}

fn run_concurrent_host_worker(vm: &TsVm, thread_index: usize) {
    for iteration in 0..CONCURRENT_ITERATIONS {
        let script_index = (thread_index + iteration) % CONCURRENT_MATH_SCRIPTS;
        let left = thread_index as i64;
        let right = iteration as i64;
        let result = vm
            .call_function(
                format!("math-{script_index}"),
                "sum",
                vec![json!({ "left": left, "right": right })],
            )
            .expect("call math script during concurrent stress");
        assert_eq!(result, json!(left + right));

        let delivered = vm
            .emit(
                "score.update",
                json!({ "combo": thread_index * CONCURRENT_ITERATIONS + iteration }),
            )
            .expect("emit event during concurrent stress");
        assert_eq!(
            delivered,
            CONCURRENT_LISTENER_SCRIPTS * CONCURRENT_EVENT_HANDLERS_PER_SCRIPT
        );
    }
}

fn assert_worker_call_and_emit_stats(stats: &ts_embed_vm::VmStats) {
    let total_worker_calls = stats
        .workers
        .iter()
        .map(|worker| worker.sync_latency.call_operations)
        .sum::<u64>();
    let total_worker_emits = stats
        .workers
        .iter()
        .map(|worker| worker.sync_latency.emit_operations)
        .sum::<u64>();

    assert_eq!(total_worker_calls, stats.latency.call_operations);
    assert!(total_worker_emits >= stats.latency.emit_operations);
    assert!(
        stats
            .workers
            .iter()
            .any(|worker| worker.queue_peak_depth > 1)
    );
}

struct ConcurrentReloadProject {
    entry_path: std::path::PathBuf,
    value_path: std::path::PathBuf,
    extra_path: std::path::PathBuf,
}

impl ConcurrentReloadProject {
    fn create(cache_dir: &TestCacheDir) -> Self {
        let root = cache_dir.path().join("concurrent-project");
        fs::create_dir_all(&root).expect("create concurrent project");
        Self {
            entry_path: root.join("main.ts"),
            value_path: root.join("value.ts"),
            extra_path: root.join("extra.ts"),
        }
    }

    fn write_version(&self, iteration: i32) {
        write_reloadable_project(
            iteration,
            &self.entry_path,
            &self.value_path,
            &self.extra_path,
        );
    }
}

fn run_concurrent_project_reloads_and_emits(vm: Arc<TsVm>, project: Arc<ConcurrentReloadProject>) {
    let participant_count = CONCURRENT_RELOAD_EMITTER_THREADS + 1;
    let start_barrier = Arc::new(Barrier::new(participant_count));
    let reload_handle = spawn_reload_worker(Arc::clone(&vm), Arc::clone(&project), &start_barrier);
    let emit_handles = spawn_emit_workers(Arc::clone(&vm), &start_barrier);

    reload_handle.join().expect("join reload worker");
    for handle in emit_handles {
        handle.join().expect("join emit worker");
    }
}

fn spawn_reload_worker(
    vm: Arc<TsVm>,
    project: Arc<ConcurrentReloadProject>,
    start_barrier: &Arc<Barrier>,
) -> std::thread::JoinHandle<()> {
    let start_barrier = Arc::clone(start_barrier);
    std::thread::spawn(move || {
        start_barrier.wait();
        for iteration in 1..=CONCURRENT_RELOAD_ITERATIONS {
            project.write_version(iteration as i32);
            vm.load_script_project("project", &project.entry_path)
                .expect("reload project during concurrent emit stress");
        }
    })
}

fn spawn_emit_workers(
    vm: Arc<TsVm>,
    start_barrier: &Arc<Barrier>,
) -> Vec<std::thread::JoinHandle<()>> {
    let mut handles = Vec::with_capacity(CONCURRENT_RELOAD_EMITTER_THREADS);
    for thread_index in 0..CONCURRENT_RELOAD_EMITTER_THREADS {
        let vm = Arc::clone(&vm);
        let start_barrier = Arc::clone(start_barrier);
        handles.push(std::thread::spawn(move || {
            start_barrier.wait();
            run_concurrent_reload_emit_worker(&vm, thread_index);
        }));
    }
    handles
}

fn run_concurrent_reload_emit_worker(vm: &TsVm, thread_index: usize) {
    for iteration in 0..CONCURRENT_RELOAD_EMITS_PER_THREAD {
        let delivered = vm
            .emit(
                "score.update",
                json!({ "combo": thread_index * CONCURRENT_RELOAD_EMITS_PER_THREAD + iteration }),
            )
            .expect("emit while project reloads");
        assert_eq!(delivered, 1);
    }
}

fn write_reloadable_project(
    iteration: i32,
    entry_path: &std::path::Path,
    value_path: &std::path::Path,
    extra_path: &std::path::Path,
) {
    if uses_extra_module(iteration) {
        fs::write(extra_path, "export const offset = 10;\n").expect("write extra module");
        fs::write(entry_path, PROJECT_MAIN_V2).expect("write v2 entry");
    } else {
        fs::write(entry_path, PROJECT_MAIN_V1).expect("write v1 entry");
    }
    fs::write(value_path, format!("export const current = {iteration};\n"))
        .expect("write value module");
}

fn expected_module_edges(iteration: i32) -> usize {
    if uses_extra_module(iteration) { 2 } else { 1 }
}

fn expected_score(iteration: i32) -> i32 {
    expected_score_for_combo(5, iteration)
}

fn expected_score_for_combo(combo: i32, iteration: i32) -> i32 {
    if uses_extra_module(iteration) {
        combo + iteration + 10
    } else {
        combo + iteration
    }
}

fn uses_extra_module(iteration: i32) -> bool {
    iteration % 2 == 1
}
