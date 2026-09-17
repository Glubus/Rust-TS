#![cfg(feature = "derive")]

mod support;

use rustts::{
    HostCallback, HostContract, HostContractKind, HostFunction, RustTs, Schema, TsSchema, VmError,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use support::TestCacheDir;

const DOGFOOD_SCRIPT: &str = r#"
let lastFound = "none";

ctx.on("user.found", event => {
  lastFound = `${event.displayName}:${event.roles.join(",")}`;
});

export function lookup() {
  const result = user.find({ userId: 7, includeRoles: true });
  return `${result.displayName}:${result.roles.length}:${result.active}`;
}

export function observed() {
  return lastFound;
}
"#;

#[derive(Deserialize, TsSchema)]
#[serde(rename_all = "camelCase")]
struct DogfoodFindUserInput {
    user_id: u64,
    include_roles: bool,
}

#[derive(Serialize, TsSchema)]
#[serde(rename_all = "camelCase")]
struct DogfoodFindUserOutput {
    user_id: u64,
    display_name: String,
    active: bool,
    roles: Vec<String>,
}

#[derive(Deserialize, Serialize, TsSchema)]
#[serde(rename_all = "camelCase")]
struct DogfoodUserFoundPayload {
    user_id: u64,
    display_name: String,
    roles: Vec<String>,
}

struct DogfoodFindUser;
struct DogfoodUserFound;

impl HostContract for DogfoodFindUser {
    const NAME: &'static str = "user.find";

    fn schema() -> Schema {
        DogfoodFindUserInput::schema()
    }

    fn kind() -> HostContractKind {
        HostContractKind::Function
    }
}

impl HostFunction for DogfoodFindUser {
    type Input = DogfoodFindUserInput;
    type Output = DogfoodFindUserOutput;

    fn call(input: Self::Input) -> Result<Self::Output, VmError> {
        Ok(DogfoodFindUserOutput {
            user_id: input.user_id,
            display_name: format!("user-{}", input.user_id),
            active: true,
            roles: if input.include_roles {
                vec![String::from("admin"), String::from("editor")]
            } else {
                Vec::new()
            },
        })
    }
}

impl HostContract for DogfoodUserFound {
    const NAME: &'static str = "user.found";

    fn schema() -> Schema {
        DogfoodUserFoundPayload::schema()
    }

    fn kind() -> HostContractKind {
        HostContractKind::Callback
    }
}

impl HostCallback for DogfoodUserFound {
    type Payload = DogfoodUserFoundPayload;
}

#[test]
fn typed_contracts_generated_sdk_and_script_bridge_are_usable_together() {
    let cache_dir = TestCacheDir::new("dogfood-typed-contracts");
    let output_dir = cache_dir.path().join("generated");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    vm.registry()
        .typed_function::<DogfoodFindUser>()
        .and_then(|registry| registry.typed_callback::<DogfoodUserFound>())
        .expect("register typed dogfood contracts");
    let written = vm
        .registry()
        .write_sdk_files(&output_dir)
        .expect("write generated SDK files");

    let sdk = std::fs::read_to_string(&written.sdk_path).expect("read generated SDK");
    assert_sdk_exposes_dogfood_surface(&sdk);
    typecheck_sdk_usage_when_tsc_is_available(cache_dir.path(), &sdk);

    vm.load_script("dogfood", DOGFOOD_SCRIPT)
        .expect("load dogfood script");
    let lookup = vm
        .call_function("dogfood", "lookup", Vec::new())
        .expect("call dogfood lookup");
    let delivered = vm
        .emit_callback::<DogfoodUserFound>(&DogfoodUserFoundPayload {
            user_id: 7,
            display_name: String::from("user-7"),
            roles: vec![String::from("admin"), String::from("editor")],
        })
        .expect("emit dogfood callback");
    let observed = vm
        .call_function("dogfood", "observed", Vec::new())
        .expect("read callback state");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(written.types_path, output_dir.join("rustts.d.ts"));
    assert_eq!(written.sdk_path, output_dir.join("rustts.sdk.ts"));
    assert_eq!(lookup, json!("user-7:2:true"));
    assert_eq!(delivered, 1);
    assert_eq!(observed, json!("user-7:admin,editor"));
}

fn assert_sdk_exposes_dogfood_surface(sdk: &str) {
    assert!(
        sdk.contains("type DogfoodFindUserInput = { userId: number; includeRoles: boolean; };")
    );
    assert!(sdk.contains(
        "type DogfoodFindUserOutput = { userId: number; displayName: string; active: boolean; roles: string[]; };"
    ));
    assert!(sdk.contains(
        "type DogfoodUserFoundPayload = { userId: number; displayName: string; roles: string[]; };"
    ));
    assert!(sdk.contains("find(input: DogfoodFindUserInput): DogfoodFindUserOutput"));
    assert!(sdk.contains("onFound(handler: HostEventHandler<\"user.found\">): void"));
    assert!(sdk.contains("found(handler: HostEventHandler<\"user.found\">): void"));
    assert!(sdk.contains("export class DogfoodFindUserInputModel"));
    assert!(sdk.contains("static is(value: unknown): value is DogfoodFindUserInput"));
    assert!(sdk.contains("static wrap(value: DogfoodFindUserInput): DogfoodFindUserInputModel"));
    assert!(sdk.contains("DogfoodFindUserInput: {"));
    assert!(sdk.contains("wrap(value: DogfoodFindUserInput): DogfoodFindUserInputModel"));
}

fn typecheck_sdk_usage_when_tsc_is_available(cache_dir: &std::path::Path, sdk: &str) {
    let usage_path = cache_dir.join("dogfood-sdk-usage.ts");
    std::fs::write(&usage_path, dogfood_sdk_usage_source(sdk)).expect("write SDK usage source");

    let Some(output) = run_tsc(&usage_path) else {
        return;
    };

    assert!(
        output.status.success(),
        "dogfood SDK usage failed tsc\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn dogfood_sdk_usage_source(sdk: &str) -> String {
    format!(
        "{sdk}\n\
const input = models.DogfoodFindUserInput.create({{ userId: 7, includeRoles: true }});\n\
const wrappedInput = models.DogfoodFindUserInput.wrap(input);\n\
const directWrappedInput = DogfoodFindUserInputModel.wrap(input);\n\
if (DogfoodFindUserInputModel.is(wrappedInput.value)) {{\n\
  wrappedInput.toJSON().userId.toFixed();\n\
}}\n\
if (models.DogfoodFindUserInput.is(directWrappedInput.valueOf())) {{\n\
  directWrappedInput.toJSON().includeRoles.valueOf();\n\
}}\n\
const found = user.find(input);\n\
found.displayName.toUpperCase();\n\
found.roles.map(role => role.toUpperCase());\n\
ctx.on(\"user.found\", event => {{\n\
  event.displayName.toUpperCase();\n\
  event.roles.length.toFixed();\n\
}});\n\
events.user.found(event => {{\n\
  event.userId.toFixed();\n\
}});\n\
user.onFound(event => {{\n\
  event.roles.map(role => role.toUpperCase());\n\
}});\n\
rusttsSdk.functions.user.find(input).active.valueOf();\n\
rusttsSdk.models.DogfoodUserFoundPayload.wrap({{ userId: 7, displayName: \"user-7\", roles: [\"admin\"] }}).toJSON().roles.length.toFixed();\n"
    )
}

fn run_tsc(path: &std::path::Path) -> Option<std::process::Output> {
    support::run_tsc(path)
}
