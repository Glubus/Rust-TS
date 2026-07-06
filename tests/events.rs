mod support;

use serde::{Deserialize, Serialize};
use serde_json::json;
use ts_embed_vm::{
    DeliveryMode, HostCallback, HostContract, HostContractKind, Schema, TsField, TsType, TsVm,
    VmEvent, VmMemoryPressureAlert, VmMemoryPressureThresholds,
};

use support::TestCacheDir;

const EVENT_LISTENER_SCRIPT: &str = include_str!("projects/event_listener/main.ts");
const PASSIVE_SCRIPT: &str = include_str!("projects/passive_script/main.ts");

#[derive(Deserialize, Serialize)]
struct ScorePayload {
    combo: u32,
}

struct ScoreUpdate;
struct FirstScoreUpdate;

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
    type Payload = ScorePayload;
}

impl HostContract for FirstScoreUpdate {
    const NAME: &'static str = "score.update";
    const IMPORT_MODULE: &'static str = "test";
    const EXPORT_PATH: &'static [&'static str] = &["score", "onUpdate"];

    fn schema() -> Schema {
        ScoreUpdate::schema()
    }

    fn kind() -> HostContractKind {
        HostContractKind::Callback
    }
}

impl HostCallback for FirstScoreUpdate {
    type Payload = ScorePayload;

    fn delivery() -> DeliveryMode {
        DeliveryMode::First
    }
}

fn register_score_update(vm: &TsVm) {
    vm.registry()
        .callback::<ScoreUpdate>()
        .expect("register score update callback");
}

#[test]
fn emit_callback_updates_script_state() {
    let cache_dir = TestCacheDir::new("emit-callback");
    let vm = TsVm::new(cache_dir.vm_options()).expect("create vm");
    register_score_update(&vm);

    vm.load_script("listener", EVENT_LISTENER_SCRIPT)
        .expect("load listener");

    let delivered = vm
        .emit_callback::<ScoreUpdate>(&ScorePayload { combo: 21 })
        .expect("emit callback");
    let score = vm
        .call_function("listener", "readScore", Vec::new())
        .expect("read score");
    let doubled = vm
        .call_function("listener", "readScoreDoubled", Vec::new())
        .expect("read doubled score");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(delivered, 2);
    assert_eq!(score, json!(21));
    assert_eq!(doubled, json!(42));
}

#[test]
fn emit_publishes_lifecycle_event() {
    let cache_dir = TestCacheDir::new("emit-event-stream");
    let vm = TsVm::new(cache_dir.vm_options()).expect("create vm");
    register_score_update(&vm);
    let subscription = vm.subscribe();

    vm.load_script("listener", EVENT_LISTENER_SCRIPT)
        .expect("load listener");
    let delivered = vm
        .emit("score.update", json!({ "combo": 7 }))
        .expect("emit raw event");

    vm.shutdown().expect("shutdown vm");

    let _ = subscription.recv();
    assert_eq!(
        subscription.recv(),
        Some(VmEvent::HostEventEmitted {
            event_name: String::from("score.update"),
            delivered_count: delivered,
        })
    );
}

#[test]
fn emit_targets_only_subscribed_scripts() {
    let cache_dir = TestCacheDir::new("emit-targeted-routing");
    let vm = TsVm::new(cache_dir.vm_options()).expect("create vm");
    register_score_update(&vm);

    vm.load_script("listener", EVENT_LISTENER_SCRIPT)
        .expect("load listener");
    vm.load_script("passive", PASSIVE_SCRIPT)
        .expect("load passive script");

    let delivered = vm
        .emit("score.update", json!({ "combo": 9 }))
        .expect("emit routed event");
    let listener_score = vm
        .call_function("listener", "readScore", Vec::new())
        .expect("read listener score");
    let passive_score = vm
        .call_function("passive", "readScore", Vec::new())
        .expect("read passive score");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(delivered, 2);
    assert_eq!(listener_score, json!(9));
    assert_eq!(passive_score, json!(-1));
}

#[test]
fn typed_callback_delivery_first_targets_one_script() {
    let cache_dir = TestCacheDir::new("emit-first-callback");
    let vm = TsVm::new(cache_dir.vm_options()).expect("create vm");
    vm.registry()
        .callback::<FirstScoreUpdate>()
        .expect("register first score update callback");

    vm.load_script("listener-a", EVENT_LISTENER_SCRIPT)
        .expect("load first listener");
    vm.load_script("listener-b", EVENT_LISTENER_SCRIPT)
        .expect("load second listener");

    let delivered = vm
        .emit_callback::<FirstScoreUpdate>(&ScorePayload { combo: 13 })
        .expect("emit first callback");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(delivered, 2);
}

#[test]
fn stats_expose_manager_latency_counters() {
    let cache_dir = TestCacheDir::new("manager-latency-counters");
    let vm = TsVm::new(cache_dir.vm_options()).expect("create vm");
    register_score_update(&vm);

    vm.load_script("listener", EVENT_LISTENER_SCRIPT)
        .expect("load listener");
    vm.emit("score.update", json!({ "combo": 5 }))
        .expect("emit event");
    vm.call_function("listener", "readScore", Vec::new())
        .expect("read score");

    let stats = vm.stats().expect("collect stats");
    vm.shutdown().expect("shutdown vm");

    assert_eq!(stats.latency.load_operations, 1);
    assert_eq!(stats.latency.emit_operations, 1);
    assert_eq!(stats.latency.call_operations, 1);
    assert!(stats.latency.load_total_ns > 0);
    assert!(stats.latency.emit_total_ns > 0);
    assert!(stats.latency.call_total_ns > 0);
    assert_eq!(
        stats.latency.load_average_ns,
        stats.latency.load_total_ns / stats.latency.load_operations
    );
    assert_eq!(
        stats.latency.emit_average_ns,
        stats.latency.emit_total_ns / stats.latency.emit_operations
    );
    assert_eq!(
        stats.latency.call_average_ns,
        stats.latency.call_total_ns / stats.latency.call_operations
    );
    assert!(stats.latency.load_max_ns >= stats.latency.load_average_ns);
    assert!(stats.latency.emit_max_ns >= stats.latency.emit_average_ns);
    assert!(stats.latency.call_max_ns >= stats.latency.call_average_ns);
    assert!(stats.latency.load_histogram.is_none());
    assert!(stats.latency.emit_histogram.is_none());
    assert!(stats.latency.call_histogram.is_none());
    assert_eq!(stats.workers[0].sync_latency.load_operations, 1);
    assert_eq!(stats.workers[0].sync_latency.emit_operations, 1);
    assert_eq!(stats.workers[0].sync_latency.call_operations, 1);
    assert!(stats.workers[0].sync_latency.load_total_ns > 0);
    assert!(stats.workers[0].sync_latency.emit_total_ns > 0);
    assert!(stats.workers[0].sync_latency.call_total_ns > 0);
    assert_eq!(stats.workers[0].async_latency.load_operations, 0);
}

#[test]
fn stats_expose_latency_histograms_when_enabled() {
    let cache_dir = TestCacheDir::new("manager-latency-histograms");
    let mut options = cache_dir.vm_options();
    options.latency_histograms = true;
    let vm = TsVm::new(options).expect("create vm");
    register_score_update(&vm);

    vm.load_script("listener", EVENT_LISTENER_SCRIPT)
        .expect("load listener");
    vm.emit("score.update", json!({ "combo": 5 }))
        .expect("emit event");
    vm.call_function("listener", "readScore", Vec::new())
        .expect("read score");

    let stats = vm.stats().expect("collect stats");
    vm.shutdown().expect("shutdown vm");

    assert_histogram_total(&stats.latency.load_histogram, 1);
    assert_histogram_total(&stats.latency.emit_histogram, 1);
    assert_histogram_total(&stats.latency.call_histogram, 1);
    assert_histogram_total(&stats.workers[0].sync_latency.load_histogram, 1);
    assert_histogram_total(&stats.workers[0].sync_latency.emit_histogram, 1);
    assert_histogram_total(&stats.workers[0].sync_latency.call_histogram, 1);
}

#[test]
fn stats_expose_memory_shape_counters() {
    let cache_dir = TestCacheDir::new("memory-shape-counters");
    let vm = TsVm::new(cache_dir.vm_options()).expect("create vm");
    register_score_update(&vm);

    vm.load_script("listener", EVENT_LISTENER_SCRIPT)
        .expect("load listener");
    vm.load_script("passive", PASSIVE_SCRIPT)
        .expect("load passive");
    vm.retain_script_dependency_edge("passive", "listener")
        .expect("retain dependency edge");

    let stats = vm.stats().expect("collect stats");
    vm.shutdown().expect("shutdown vm");

    assert_eq!(stats.memory.script_registry_entries, 2);
    assert_eq!(stats.memory.active_scripts, 2);
    assert_eq!(stats.memory.event_route_bindings, 1);
    assert_eq!(stats.memory.dependency_edges, 1);
    assert_eq!(stats.memory.module_dependency_edges, 0);
}

#[test]
fn stats_classify_quickjs_memory_pressure_when_thresholds_are_configured() {
    let cache_dir = TestCacheDir::new("memory-pressure-thresholds");
    let mut options = cache_dir.vm_options();
    options.memory_pressure_thresholds =
        Some(VmMemoryPressureThresholds::from_basis_points(0, u64::MAX));
    let vm = TsVm::new(options).expect("create vm");
    register_score_update(&vm);

    vm.load_script("listener", EVENT_LISTENER_SCRIPT)
        .expect("load listener");

    let stats = vm.stats().expect("collect stats");
    vm.shutdown().expect("shutdown vm");

    let memory = stats.workers[0]
        .sync_quickjs_memory
        .expect("sync quickjs memory stats");
    assert_eq!(
        memory.memory_pressure_alert,
        Some(VmMemoryPressureAlert::Warning)
    );
}

#[test]
fn stats_leave_quickjs_memory_pressure_alert_disabled_by_default() {
    let cache_dir = TestCacheDir::new("memory-pressure-thresholds-disabled");
    let vm = TsVm::new(cache_dir.vm_options()).expect("create vm");
    register_score_update(&vm);

    vm.load_script("listener", EVENT_LISTENER_SCRIPT)
        .expect("load listener");

    let stats = vm.stats().expect("collect stats");
    vm.shutdown().expect("shutdown vm");

    let memory = stats.workers[0]
        .sync_quickjs_memory
        .expect("sync quickjs memory stats");
    assert_eq!(memory.memory_pressure_alert, None);
}

fn assert_histogram_total(
    histogram: &Option<Vec<ts_embed_vm::VmLatencyHistogramBucket>>,
    expected_total: u64,
) {
    let histogram = histogram.as_ref().expect("latency histogram");
    assert_eq!(
        histogram.iter().map(|bucket| bucket.count).sum::<u64>(),
        expected_total
    );
    assert!(
        histogram
            .last()
            .expect("overflow bucket")
            .upper_bound_ns
            .is_none()
    );
}
