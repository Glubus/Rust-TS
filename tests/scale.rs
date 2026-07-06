mod support;

use serde_json::json;
use ts_embed_vm::{HostCallback, HostContract, HostContractKind, Schema, TsField, TsType, TsVm};

use support::TestCacheDir;

const DEMO_SCRIPT: &str = include_str!("projects/basic_math/main.ts");
const EVENT_LISTENER_SCRIPT: &str = include_str!("projects/event_listener/main.ts");

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

#[test]
fn mounts_400_small_scripts_on_two_runners_and_keeps_hot_routes() {
    let cache_dir = TestCacheDir::new("scale-400-scripts");
    let mut options = cache_dir.vm_options();
    options.worker_threads = 2;
    options.max_scripts_per_worker = 256;
    options.queue_capacity = 512;
    let vm = TsVm::new(options).expect("create vm");
    vm.registry()
        .callback::<ScoreUpdate>()
        .expect("register score update callback");

    for index in 0..400 {
        let source = if index % 10 == 0 {
            EVENT_LISTENER_SCRIPT
        } else {
            DEMO_SCRIPT
        };
        vm.load_script(format!("script-{index}"), source)
            .expect("load script");
    }

    let routed = vm
        .emit("score.update", json!({ "combo": 9 }))
        .expect("emit routed event");
    let sum = vm
        .call_function(
            "script-399",
            "sum",
            vec![json!({ "left": 20, "right": 22 })],
        )
        .expect("call script function");
    let stats = vm.stats().expect("collect stats");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(routed, 80);
    assert_eq!(sum, json!(42));
    assert_eq!(stats.worker_count, 2);
    assert_eq!(stats.loaded_scripts, 400);
    assert_eq!(stats.memory.active_scripts, 400);
    assert_eq!(stats.memory.event_route_bindings, 40);
    #[cfg(target_os = "linux")]
    assert!(stats.process_memory.is_some());
}
