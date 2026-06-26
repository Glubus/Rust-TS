mod support;

use serde_json::json;
use ts_embed_vm::{HostContract, HostContractKind, HostFunction, Schema, TsType, TsVm, VmError};

use support::TestCacheDir;

const REALISTIC_MOD_PACK_ENTRY: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/projects/realistic_mod_pack/src/main.ts"
);

struct FindUser;

impl HostContract for FindUser {
    const NAME: &'static str = "user.find";

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

#[test]
fn realistic_mod_pack_uses_aliases_host_calls_events_and_state() {
    let cache_dir = TestCacheDir::new("realistic-mod-pack");
    let vm = TsVm::new(cache_dir.vm_options()).expect("create vm");
    vm.registry()
        .function::<FindUser>()
        .expect("register host function");

    vm.load_script_project("raid-mod", REALISTIC_MOD_PACK_ENTRY)
        .expect("load realistic mod pack");
    let damage = vm
        .call_function(
            "raid-mod",
            "simulateDamage",
            vec![json!({
                "playerId": 7,
                "baseDamage": 9,
                "critical": true,
            })],
        )
        .expect("simulate damage");
    let invoice = vm
        .call_function("raid-mod", "invoiceFor", vec![json!(7), json!(12.5)])
        .expect("create invoice");
    let delivered = vm
        .emit("score.update", json!({ "combo": 5 }))
        .expect("emit score event");
    let state = vm
        .call_function("raid-mod", "readState", Vec::new())
        .expect("read mod state");
    let snapshot = vm.runtime_snapshot().expect("collect runtime snapshot");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(
        damage,
        json!({
            "playerId": 7,
            "playerName": "user-7",
            "damage": 18,
            "score": 18,
        })
    );
    assert_eq!(invoice, json!("invoice:user-7:12.50"));
    assert_eq!(delivered, 1);
    assert_eq!(
        state,
        json!({
            "score": 23,
            "overlay": {
                "visible": true,
                "messages": ["damage:user-7:18", "combo:5"],
            },
            "session": {
                "id": "raid-night-01",
                "ticks": 1,
            },
        })
    );
    assert_eq!(snapshot.stats.memory.active_scripts, 1);
    assert_eq!(snapshot.stats.memory.event_route_bindings, 1);
    assert_eq!(snapshot.stats.memory.module_dependency_edges, 8);
    assert_eq!(snapshot.scripts.len(), 1);
    assert_eq!(snapshot.scripts[0].subscriptions, vec!["score.update"]);
    assert_eq!(snapshot.scripts[0].module_dependencies.len(), 8);
}
