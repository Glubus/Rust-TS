mod support;

use std::fs;

use rustts::{HostCallback, HostContract, HostContractKind, RustTs, Schema, TsField, TsType};
use serde_json::json;

use support::TestCacheDir;

const PROJECT_MAIN_V1: &str = include_str!("projects/reloadable_project/main_v1.ts");
const PROJECT_MAIN_V2: &str = include_str!("projects/reloadable_project/main_v2.ts");
const ALIAS_ENTRY: &str = r#"
import { label as first } from "./first";
import { label as second } from "./second";
import { label as aliased } from "@target";

export function read() {
  return { first, second, aliased };
}
"#;

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
fn project_reload_replaces_dependency_graph_and_preserves_hot_route() {
    let cache_dir = TestCacheDir::new("project-reload-graph");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");
    vm.registry()
        .callback::<ScoreUpdate>()
        .expect("register score update callback");
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

#[test]
fn project_reload_follows_tsconfig_paths_change_between_loaded_modules() {
    let cache_dir = TestCacheDir::new("project-reload-tsconfig-paths");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");
    let project_root = cache_dir.path().join("project");
    let entry_path = project_root.join("main.ts");

    fs::create_dir_all(&project_root).expect("create project");
    fs::write(&entry_path, ALIAS_ENTRY).expect("write entry");
    fs::write(
        project_root.join("first.ts"),
        "export const label = \"first\";\n",
    )
    .expect("write first module");
    fs::write(
        project_root.join("second.ts"),
        "export const label = \"second\";\n",
    )
    .expect("write second module");

    write_alias_tsconfig(&project_root, "./first.ts");
    vm.load_script_project("project", &entry_path)
        .expect("load project");
    let before = vm.call_function("project", "read", Vec::new());

    write_alias_tsconfig(&project_root, "./second.ts");
    vm.load_script_project("project", &entry_path)
        .expect("reload project after tsconfig change");
    let after = vm.call_function("project", "read", Vec::new());

    vm.shutdown().expect("shutdown vm");

    assert_eq!(
        before.expect("read first alias target"),
        json!({ "first": "first", "second": "second", "aliased": "first" })
    );
    assert_eq!(
        after.expect("read second alias target"),
        json!({ "first": "first", "second": "second", "aliased": "second" })
    );
}

fn write_alias_tsconfig(project_root: &std::path::Path, target: &str) {
    let tsconfig = json!({
        "compilerOptions": {
            "baseUrl": ".",
            "paths": { "@target": [target] },
        },
    });
    fs::write(project_root.join("tsconfig.json"), tsconfig.to_string()).expect("write tsconfig");
}
