use std::collections::BTreeMap;

use super::*;

#[test]
fn project_graph_uses_runtime_scoped_module_ids() {
    let store = WorkerModuleStore::default();
    let graph = store
        .insert_project(
            "/app/main.ts",
            vec![
                CompiledModule {
                    module_id: String::from("/app/main.ts"),
                    transpiled_js: String::from("import { value } from './dep';"),
                    resolved_requests: BTreeMap::from([(
                        String::from("./dep"),
                        String::from("/app/dep.ts"),
                    )]),
                },
                CompiledModule {
                    module_id: String::from("/app/dep.ts"),
                    transpiled_js: String::from("export const value = 1;"),
                    resolved_requests: BTreeMap::new(),
                },
            ],
            7,
        )
        .expect("insert project graph");

    assert_eq!(graph.entry_module_id, "tsvm://graph/7//app/main.ts");
    assert_eq!(graph.module_ids.len(), 2);
    let resolved = store
        .resolve("tsvm://graph/7//app/main.ts", "./dep")
        .expect("resolve runtime module");
    assert_eq!(resolved, "tsvm://graph/7//app/dep.ts");
}

#[test]
fn removing_graph_clears_sources_and_resolutions() {
    let store = WorkerModuleStore::default();
    let graph = store
        .insert_project(
            "/app/main.ts",
            vec![
                CompiledModule {
                    module_id: String::from("/app/main.ts"),
                    transpiled_js: String::from("import { value } from './dep';"),
                    resolved_requests: BTreeMap::from([(
                        String::from("./dep"),
                        String::from("/app/dep.ts"),
                    )]),
                },
                CompiledModule {
                    module_id: String::from("/app/dep.ts"),
                    transpiled_js: String::from("export const value = 1;"),
                    resolved_requests: BTreeMap::new(),
                },
            ],
            7,
        )
        .expect("insert project graph");

    store
        .remove_script_modules("/app/main.ts", &graph.module_ids)
        .expect("remove graph modules");

    let error = store
        .resolve("tsvm://graph/7//app/main.ts", "./dep")
        .expect_err("graph resolution removed");
    assert!(matches!(error, rquickjs::Error::Resolving { .. }));
}
