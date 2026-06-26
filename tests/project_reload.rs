mod support;

use std::fs;

use serde_json::json;
use ts_embed_vm::TsVm;

use support::TestCacheDir;

const PROJECT_MAIN_V1: &str = include_str!("projects/reloadable_project/main_v1.ts");
const PROJECT_MAIN_V2: &str = include_str!("projects/reloadable_project/main_v2.ts");

#[test]
fn project_reload_replaces_dependency_graph_and_preserves_hot_route() {
    let cache_dir = TestCacheDir::new("project-reload-graph");
    let vm = TsVm::new(cache_dir.vm_options()).expect("create vm");
    let project_root = cache_dir.path().join("project");
    let entry_path = project_root.join("main.ts");
    let value_path = project_root.join("value.ts");
    let extra_path = project_root.join("extra.ts");

    fs::create_dir_all(&project_root).expect("create project");
    fs::write(&entry_path, PROJECT_MAIN_V1).expect("write first entry");
    fs::write(&value_path, "export const current = 1;\n").expect("write first value");

    let first = vm
        .load_script_project("project", &entry_path)
        .expect("load first project");
    let first_stats = vm.stats().expect("collect first stats");
    let first_routed = vm
        .emit("score.update", json!({ "combo": 5 }))
        .expect("emit first event");
    let first_score = vm
        .call_function("project", "readScore", Vec::new())
        .expect("read first score");

    fs::write(&entry_path, PROJECT_MAIN_V2).expect("write second entry");
    fs::write(&value_path, "export const current = 2;\n").expect("write second value");
    fs::write(&extra_path, "export const offset = 10;\n").expect("write extra value");

    let second = vm
        .load_script_project("project", &entry_path)
        .expect("reload project");
    let second_stats = vm.stats().expect("collect second stats");
    let second_routed = vm
        .emit("score.update", json!({ "combo": 5 }))
        .expect("emit second event");
    let second_value = vm
        .call_function("project", "read", Vec::new())
        .expect("read second value");
    let second_score = vm
        .call_function("project", "readScore", Vec::new())
        .expect("read second score");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(first_stats.memory.module_dependency_edges, 1);
    assert_eq!(first_stats.memory.event_route_bindings, 1);
    assert_eq!(first_routed, 1);
    assert_eq!(first_score, json!(6));
    assert_ne!(first.cache_key, second.cache_key);
    assert_ne!(first.transpiled_path, second.transpiled_path);
    assert_eq!(second_stats.memory.module_dependency_edges, 2);
    assert_eq!(second_stats.memory.event_route_bindings, 1);
    assert_eq!(second_routed, 1);
    assert_eq!(second_value, json!(12));
    assert_eq!(second_score, json!(17));
}
