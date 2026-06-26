mod support;

use serde_json::json;
use ts_embed_vm::TsVm;

use support::TestCacheDir;

const DEMO_SCRIPT: &str = include_str!("projects/basic_math/main.ts");
const EVENT_LISTENER_SCRIPT: &str = include_str!("projects/event_listener/main.ts");

#[test]
fn distinct_scripts_are_distributed_across_workers() {
    let cache_dir = TestCacheDir::new("multi-runner-distribution");
    let mut options = cache_dir.vm_options();
    options.worker_threads = 2;
    let vm = TsVm::new(options).expect("create vm");

    let first = vm.load_script("alpha", DEMO_SCRIPT).expect("load alpha");
    let second = vm.load_script("beta", DEMO_SCRIPT).expect("load beta");
    let third = vm.load_script("gamma", DEMO_SCRIPT).expect("load gamma");
    let fourth = vm.load_script("delta", DEMO_SCRIPT).expect("load delta");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(first.worker_id, 0);
    assert_eq!(second.worker_id, 1);
    assert_eq!(third.worker_id, 0);
    assert_eq!(fourth.worker_id, 1);
}

#[test]
fn reloading_same_script_keeps_preferred_worker_affinity() {
    let cache_dir = TestCacheDir::new("reload-worker-affinity");
    let mut options = cache_dir.vm_options();
    options.worker_threads = 2;
    let vm = TsVm::new(options).expect("create vm");

    let first = vm.load_script("alpha", DEMO_SCRIPT).expect("load alpha");
    vm.unload_script("alpha").expect("unload alpha");
    let second = vm.load_script("alpha", DEMO_SCRIPT).expect("reload alpha");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(first.worker_id, second.worker_id);
}

#[test]
fn many_scripts_across_runners_keep_hot_routes_and_calls_working() {
    let cache_dir = TestCacheDir::new("many-scripts-across-runners");
    let mut options = cache_dir.vm_options();
    options.worker_threads = 2;
    options.max_scripts_per_worker = 80;
    options.queue_capacity = 256;
    let vm = TsVm::new(options).expect("create vm");

    for index in 0..120 {
        let script = if index % 3 == 0 {
            EVENT_LISTENER_SCRIPT
        } else {
            DEMO_SCRIPT
        };
        vm.load_script(format!("script-{index}"), script)
            .expect("load script");
    }

    let routed = vm
        .emit("score.update", json!({ "combo": 11 }))
        .expect("emit routed event");
    let sum = vm
        .call_function("script-1", "sum", vec![json!({ "left": 6, "right": 9 })])
        .expect("call script function");
    let stats = vm.stats().expect("collect stats");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(routed, 80);
    assert_eq!(sum, json!(15));
    assert_eq!(stats.worker_count, 2);
    assert_eq!(stats.loaded_scripts, 120);
    assert_eq!(stats.workers.len(), 2);
    assert_eq!(stats.workers[0].worker_id, 0);
    assert_eq!(stats.workers[0].loaded_scripts, 60);
    assert_eq!(stats.workers[0].active_scripts, 60);
    assert_eq!(stats.workers[0].event_route_bindings, 20);
    assert_eq!(stats.workers[0].max_scripts, 80);
    assert_eq!(stats.workers[0].queue_capacity, 256);
    assert_eq!(stats.workers[0].queue_depth, 0);
    assert!(stats.workers[0].queue_peak_depth >= 1);
    assert_eq!(stats.workers[0].queue_rejected_sends, 0);
    assert_eq!(stats.workers[1].worker_id, 1);
    assert_eq!(stats.workers[1].loaded_scripts, 60);
    assert_eq!(stats.workers[1].active_scripts, 60);
    assert_eq!(stats.workers[1].event_route_bindings, 20);
    assert_eq!(stats.workers[1].max_scripts, 80);
    assert_eq!(stats.workers[1].queue_capacity, 256);
    assert_eq!(stats.workers[1].queue_depth, 0);
    assert!(stats.workers[1].queue_peak_depth >= 1);
    assert_eq!(stats.workers[1].queue_rejected_sends, 0);
    assert_eq!(stats.memory.active_scripts, 120);
    assert_eq!(stats.memory.event_route_bindings, 40);
}
