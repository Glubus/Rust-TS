#[cfg(feature = "derive")]
use rustts::TsSchema;
use rustts::{
    HostCallback, HostContext, HostContract, HostContractKind, HostFunction, HostMetadata, RustTs,
    Schema, TsField, TsType, VmContractValidation, VmError, VmUnknownFieldValidation,
};
use serde_json::{Value, json};
use std::process::Command;

mod support;

use support::TestCacheDir;

struct FindUser;
struct ScoreUpdate;
struct OverlayContext;
struct EchoValidation;
struct EchoTypeRefValidation;
struct BadOutputValidation;
#[cfg(feature = "derive")]
struct RecordAction;

const HOST_VALIDATION_SCRIPT: &str = include_str!("projects/host_validation/main.ts");

impl HostContract for FindUser {
    const NAME: &'static str = "user.find";
    const IMPORT_MODULE: &'static str = "test";
    const EXPORT_PATH: &'static [&'static str] = &["user", "find"];

    fn schema() -> Schema {
        Schema::typed("FindUserInput", TsType::Number)
    }

    fn metadata() -> HostMetadata {
        HostMetadata {
            name: String::from(Self::NAME),
            tags: vec![String::from("sdk")],
        }
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

impl HostContract for ScoreUpdate {
    const NAME: &'static str = "score.update";
    const IMPORT_MODULE: &'static str = "test";
    const EXPORT_PATH: &'static [&'static str] = &["score", "onUpdate"];

    fn schema() -> Schema {
        Schema::typed(
            "ScoreUpdatePayload",
            TsType::Object(vec![TsField::required("combo", TsType::Number)]),
        )
    }

    fn kind() -> HostContractKind {
        HostContractKind::Callback
    }
}

impl HostCallback for ScoreUpdate {
    type Payload = ();
}

impl HostContract for OverlayContext {
    const NAME: &'static str = "overlay";
    const IMPORT_MODULE: &'static str = "test";
    const EXPORT_PATH: &'static [&'static str] = &["overlay"];

    fn schema() -> Schema {
        Schema::typed(
            "OverlayContext",
            TsType::Object(vec![TsField::required("visible", TsType::Boolean)]),
        )
    }

    fn kind() -> HostContractKind {
        HostContractKind::Context
    }
}

impl HostContext for OverlayContext {}

#[cfg(feature = "derive")]
#[derive(serde::Deserialize, TsSchema)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct ActionActor {
    actor_id: u64,
    display_name: String,
}

#[cfg(feature = "derive")]
#[derive(serde::Deserialize, TsSchema)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct RecordActionInput {
    action_id: String,
    #[serde(flatten)]
    actor: ActionActor,
    confirmed: bool,
}

#[cfg(feature = "derive")]
impl HostContract for RecordAction {
    const NAME: &'static str = "action.record";
    const IMPORT_MODULE: &'static str = "test";
    const EXPORT_PATH: &'static [&'static str] = &["action", "record"];

    fn schema() -> Schema {
        RecordActionInput::schema()
    }

    fn kind() -> HostContractKind {
        HostContractKind::Function
    }
}

#[cfg(feature = "derive")]
impl HostFunction for RecordAction {
    type Input = RecordActionInput;
    type Output = String;

    fn output_schema() -> Schema {
        String::schema()
    }

    fn call(input: Self::Input) -> Result<Self::Output, VmError> {
        Ok(format!("{}:{}", input.action_id, input.actor.actor_id))
    }
}

impl HostContract for EchoValidation {
    const NAME: &'static str = "validation.echo";
    const IMPORT_MODULE: &'static str = "test";
    const EXPORT_PATH: &'static [&'static str] = &["validation", "echo"];

    fn schema() -> Schema {
        validation_input_schema()
    }

    fn kind() -> HostContractKind {
        HostContractKind::Function
    }
}

impl HostFunction for EchoValidation {
    type Input = Value;
    type Output = Value;

    fn output_schema() -> Schema {
        validation_input_schema()
    }

    fn call(input: Self::Input) -> Result<Self::Output, VmError> {
        Ok(input)
    }
}

impl HostContract for EchoTypeRefValidation {
    const NAME: &'static str = "validation.echoRef";
    const IMPORT_MODULE: &'static str = "test";
    const EXPORT_PATH: &'static [&'static str] = &["validation", "echoRef"];

    fn schema() -> Schema {
        Schema::typed(
            "ValidationTypeRefInput",
            TsType::Object(vec![TsField::required(
                "user_id",
                TsType::TypeRef(String::from("UserId")),
            )]),
        )
        .with_dependencies(vec![Schema::typed("UserId", TsType::Number)])
    }

    fn kind() -> HostContractKind {
        HostContractKind::Function
    }
}

impl HostFunction for EchoTypeRefValidation {
    type Input = Value;
    type Output = Value;

    fn output_schema() -> Schema {
        Schema::typed("ValidationTypeRefOutput", TsType::Json)
    }

    fn call(input: Self::Input) -> Result<Self::Output, VmError> {
        Ok(input)
    }
}

impl HostContract for BadOutputValidation {
    const NAME: &'static str = "validation.badOutput";
    const IMPORT_MODULE: &'static str = "test";
    const EXPORT_PATH: &'static [&'static str] = &["validation", "badOutput"];

    fn schema() -> Schema {
        Schema::typed("BadOutputInput", TsType::Number)
    }

    fn kind() -> HostContractKind {
        HostContractKind::Function
    }
}

impl HostFunction for BadOutputValidation {
    type Input = Value;
    type Output = Value;

    fn output_schema() -> Schema {
        Schema::typed("BadOutputOutput", TsType::Number)
    }

    fn call(_input: Self::Input) -> Result<Self::Output, VmError> {
        Ok(json!("not-a-number"))
    }
}

fn validation_input_schema() -> Schema {
    Schema::typed(
        "ValidationInput",
        TsType::Object(vec![TsField::required("id", TsType::Number)]),
    )
}

#[test]
fn manager_registry_supports_fluent_contract_registration() {
    let cache_dir = TestCacheDir::new("host-registry-api");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    vm.registry()
        .callback::<ScoreUpdate>()
        .and_then(|registry| registry.function::<FindUser>())
        .and_then(|registry| registry.context::<OverlayContext>())
        .expect("register host contracts");

    let function = vm
        .registry()
        .descriptor(FindUser::NAME)
        .expect("get function")
        .expect("function descriptor");
    let callback = vm
        .registry()
        .descriptor(ScoreUpdate::NAME)
        .expect("get callback")
        .expect("callback descriptor");
    let context = vm
        .registry()
        .descriptor(OverlayContext::NAME)
        .expect("get context")
        .expect("context descriptor");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(function.kind, HostContractKind::Function);
    assert_eq!(function.schema.name, "FindUserInput");
    let function_metadata = function.function.expect("function metadata");
    assert_eq!(function_metadata.input_schema.name, "FindUserInput");
    assert_eq!(function_metadata.output_schema.name, "FindUserOutput");
    assert_eq!(callback.kind, HostContractKind::Callback);
    assert_eq!(callback.schema.name, "ScoreUpdatePayload");
    assert_eq!(context.kind, HostContractKind::Context);
    assert_eq!(
        vm.registry().dts().expect("render dts"),
        "type OverlayContext = { visible: boolean; };\n\ndeclare const overlay: OverlayContext;\n\ntype ScoreUpdatePayload = { combo: number; };\n\ntype FindUserInput = number;\n\ntype FindUserOutput = string;\n\ndeclare namespace user {\n  export function find(input: FindUserInput): FindUserOutput;\n}\n\ntype HostEvents = {\n  \"score.update\": ScoreUpdatePayload;\n};\n\ndeclare const ctx: {\n  on<K extends keyof HostEvents>(event: K, handler: (payload: HostEvents[K]) => void | Promise<void>): void;\n};\n"
    );
}

#[test]
fn manager_registry_generates_sdk_source_from_contracts() {
    let cache_dir = TestCacheDir::new("host-registry-sdk");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    vm.registry()
        .callback::<ScoreUpdate>()
        .and_then(|registry| registry.function::<FindUser>())
        .expect("register host contracts");

    let sdk = vm.registry().sdk().expect("render sdk");

    vm.shutdown().expect("shutdown vm");

    assert!(sdk.contains("type FindUserInput = number;"));
    assert!(sdk.contains("type ScoreUpdatePayload = { combo: number; };"));
    assert!(sdk.contains("type HostFunctions = {"));
    assert!(sdk.contains(
        "\"user.find\": { input: FindUserInput; output: FindUserOutput; async: false; };"
    ));
    assert!(sdk.contains("export function call<K extends keyof HostFunctions>"));
    assert!(sdk.contains("export const user = {"));
    assert!(sdk.contains("find(input: FindUserInput): FindUserOutput"));
    assert!(sdk.contains("return __hostCall<FindUserOutput>(\"user.find\", input);"));
    assert!(sdk.contains("export const events = {"));
    assert!(sdk.contains("update(handler: HostEventHandler<\"score.update\">): void"));
    assert!(sdk.contains("export const score = {"));
    assert!(sdk.contains("onUpdate(handler: HostEventHandler<\"score.update\">): void"));
    assert!(sdk.contains("export const rusttsSdk = {"));
    assert!(sdk.contains("functions: {"));
    assert!(sdk.contains("call,"));
    assert!(sdk.contains("events,"));
    assert!(sdk.contains("ctx,"));
}

#[cfg(feature = "derive")]
#[test]
fn generated_sdk_uses_flattened_derived_input_schema() {
    let cache_dir = TestCacheDir::new("host-registry-sdk-flatten");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    vm.registry()
        .function::<RecordAction>()
        .expect("register flattened host contract");

    let sdk = vm.registry().sdk().expect("render sdk");

    vm.shutdown().expect("shutdown vm");

    assert!(sdk.contains(
        "type RecordActionInput = { actionId: string; actorId: number; displayName: string; confirmed: boolean; };"
    ));
    assert!(sdk.contains("export const models = {"));
    assert!(sdk.contains("export class RecordActionInputModel {"));
    assert!(sdk.contains("constructor(public readonly value: RecordActionInput) {}"));
    assert!(sdk.contains("static is(value: unknown): value is RecordActionInput"));
    assert!(sdk.contains("static wrap(value: RecordActionInput): RecordActionInputModel"));
    assert!(sdk.contains("toJSON(): RecordActionInput"));
    assert!(sdk.contains("RecordActionInput: {"));
    assert!(sdk.contains("create(value: RecordActionInput): RecordActionInput"));
    assert!(sdk.contains("is(value: unknown): value is RecordActionInput"));
    assert!(sdk.contains("wrap(value: RecordActionInput): RecordActionInputModel"));
    assert!(sdk.contains("record(input: RecordActionInput): string"));
    assert!(sdk.contains("return __hostCall<string>(\"action.record\", input);"));
    assert!(sdk.contains("models,"));
}

#[test]
fn host_context_v0_is_declarative_sdk_surface_only() {
    let cache_dir = TestCacheDir::new("host-context-v0-declarative");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    vm.registry()
        .context::<OverlayContext>()
        .expect("register host context");
    let sdk = vm.registry().sdk().expect("render sdk");

    vm.shutdown().expect("shutdown vm");

    assert!(sdk.contains("type OverlayContext = { visible: boolean; };"));
    assert!(sdk.contains(
        "export const overlay = (globalThis as unknown as Record<string, unknown>)[\"overlay\"] as OverlayContext;"
    ));
    assert!(sdk.contains("export const rusttsSdk = {"));
    assert!(sdk.contains("contexts: {"));
    assert!(sdk.contains("overlay,"));
    assert!(!sdk.contains("__hostCall"));
    assert!(!sdk.contains("__hostOn"));
}

#[test]
fn importing_a_host_context_fails_at_load_instead_of_binding_undefined() {
    let cache_dir = TestCacheDir::new("host-context-import");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");
    vm.registry()
        .context::<OverlayContext>()
        .and_then(|registry| registry.function::<FindUser>())
        .expect("register host contracts");

    let loaded = vm.load_script(
        "overlay-reader",
        r#"
        import { overlay } from "test";

        export function overlayType() {
          return typeof overlay;
        }
        "#,
    );
    let result = vm.call_function("overlay-reader", "overlayType", Vec::new());

    vm.shutdown().expect("shutdown vm");

    let Err(error) = loaded else {
        panic!("context import loaded and evaluated to {result:?}");
    };
    assert!(
        matches!(&error, VmError::Execution { details } if details.contains("overlay")),
        "unexpected error: {error}"
    );
}

#[test]
fn manager_registry_writes_sdk_files() {
    let cache_dir = TestCacheDir::new("host-registry-sdk-files");
    let output_dir = cache_dir.path().join("generated");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    vm.registry()
        .callback::<ScoreUpdate>()
        .and_then(|registry| registry.function::<FindUser>())
        .expect("register host contracts");
    let written = vm
        .registry()
        .write_sdk_files(&output_dir)
        .expect("write sdk files");

    vm.shutdown().expect("shutdown vm");

    let types = std::fs::read_to_string(&written.types_path).expect("read types");
    let sdk = std::fs::read_to_string(&written.sdk_path).expect("read sdk");
    assert_eq!(written.types_path, output_dir.join("rustts.d.ts"));
    assert_eq!(written.sdk_path, output_dir.join("rustts.sdk.ts"));
    assert!(types.contains("declare namespace user"));
    assert!(sdk.contains("export const user = {"));
}

#[test]
fn rustts_sdk_binary_writes_files_from_descriptor_json() {
    let cache_dir = TestCacheDir::new("host-registry-sdk-bin");
    let descriptors_path = cache_dir.path().join("descriptors.json");
    let output_dir = cache_dir.path().join("out");
    let registry = rustts::InMemoryHostContractRegistry::new();
    registry
        .callback::<ScoreUpdate>()
        .and_then(|registry| registry.function::<FindUser>())
        .expect("register host contracts");
    std::fs::write(
        &descriptors_path,
        serde_json::to_string(&registry.descriptors().expect("descriptors"))
            .expect("serialize descriptors"),
    )
    .expect("write descriptors");

    let output = Command::new(env!("CARGO_BIN_EXE_rustts-sdk"))
        .arg(&descriptors_path)
        .arg(&output_dir)
        .output()
        .expect("run rustts-sdk");

    assert!(
        output.status.success(),
        "rustts-sdk failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output_dir.join("rustts.d.ts").exists());
    assert!(output_dir.join("rustts.sdk.ts").exists());
}

#[test]
fn generated_sdk_typechecks_when_tsc_is_available() {
    let cache_dir = TestCacheDir::new("host-registry-sdk-tsc");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    vm.registry()
        .callback::<ScoreUpdate>()
        .and_then(|registry| registry.function::<FindUser>())
        .and_then(|registry| registry.context::<OverlayContext>())
        .expect("register host contracts");
    let sdk = vm.registry().sdk().expect("render sdk");
    let sdk_path = cache_dir.path().join("sdk.ts");
    std::fs::write(&sdk_path, sdk_usage_source(&sdk)).expect("write sdk");

    vm.shutdown().expect("shutdown vm");

    let Some(output) = run_tsc(&sdk_path) else {
        return;
    };

    assert!(
        output.status.success(),
        "generated sdk failed tsc\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn sdk_usage_source(sdk: &str) -> String {
    format!(
        "{sdk}\n\
const found = user.find(1);\n\
found.toUpperCase();\n\
const foundFromDynamicCall = call(\"user.find\", 3);\n\
foundFromDynamicCall.toUpperCase();\n\
overlay.visible.valueOf();\n\
events.score.update(event => {{\n\
  event.combo.toFixed();\n\
}});\n\
const foundFromAggregate = rusttsSdk.functions.user.find(2);\n\
foundFromAggregate.toUpperCase();\n\
const foundFromAggregateCall = rusttsSdk.call(\"user.find\", 4);\n\
foundFromAggregateCall.toUpperCase();\n\
const scorePayload = models.ScoreUpdatePayload.create({{ combo: 8 }});\n\
scorePayload.combo.toFixed();\n\
if (ScoreUpdatePayloadModel.is({{ combo: 13 }})) {{\n\
  ScoreUpdatePayloadModel.create({{ combo: 13 }}).combo.toFixed();\n\
}}\n\
if (models.ScoreUpdatePayload.is({{ combo: 14 }})) {{\n\
  models.ScoreUpdatePayload.create({{ combo: 14 }}).combo.toFixed();\n\
}}\n\
const scorePayloadModel = models.ScoreUpdatePayload.wrap({{ combo: 10 }});\n\
scorePayloadModel.value.combo.toFixed();\n\
scorePayloadModel.toJSON().combo.toFixed();\n\
const directScorePayloadModel = new ScoreUpdatePayloadModel({{ combo: 11 }});\n\
directScorePayloadModel.valueOf().combo.toFixed();\n\
const scorePayloadFromAggregate = rusttsSdk.models.ScoreUpdatePayload.create({{ combo: 9 }});\n\
scorePayloadFromAggregate.combo.toFixed();\n\
const scorePayloadModelFromAggregate = rusttsSdk.models.ScoreUpdatePayload.wrap({{ combo: 12 }});\n\
scorePayloadModelFromAggregate.toJSON().combo.toFixed();\n\
rusttsSdk.contexts.overlay.visible.valueOf();\n\
rusttsSdk.events.score.update(event => {{\n\
  event.combo.toFixed();\n\
}});\n\
score.onUpdate(event => {{\n\
  event.combo.toFixed();\n\
}});\n\
rusttsSdk.ctx.on(\"score.update\", event => {{\n\
  event.combo.toFixed();\n\
}});\n"
    )
}

fn run_tsc(path: &std::path::Path) -> Option<std::process::Output> {
    support::run_tsc(path)
}

#[test]
fn host_contract_input_validation_is_configurable() {
    let cache_dir = TestCacheDir::new("host-contract-input-validation");
    let mut options = cache_dir.vm_options();
    options.contract_validation = VmContractValidation::Inputs;
    let vm = RustTs::new(options).expect("create vm");

    vm.registry()
        .function::<EchoValidation>()
        .expect("register host function");
    vm.load_script("validation", HOST_VALIDATION_SCRIPT)
        .expect("load validation script");

    let error = vm
        .call_function("validation", "echo", vec![json!({ "id": "bad" })])
        .expect_err("invalid input should fail");

    vm.shutdown().expect("shutdown vm");

    assert!(matches!(
        error,
        VmError::Execution { details }
            if details.contains("validation.echo")
                && details.contains("input validation failed")
                && details.contains("$.id")
    ));
}

#[test]
fn host_contract_input_validation_resolves_type_ref_dependencies() {
    let cache_dir = TestCacheDir::new("host-contract-input-validation-typeref");
    let mut options = cache_dir.vm_options();
    options.contract_validation = VmContractValidation::Inputs;
    let vm = RustTs::new(options).expect("create vm");

    vm.registry()
        .function::<EchoTypeRefValidation>()
        .expect("register host function");
    vm.load_script("validation", HOST_VALIDATION_SCRIPT)
        .expect("load validation script");

    let error = vm
        .call_function("validation", "echoRef", vec![json!({ "user_id": "bad" })])
        .expect_err("invalid TypeRef input should fail");

    vm.shutdown().expect("shutdown vm");

    assert!(matches!(
        error,
        VmError::Execution { details }
            if details.contains("validation.echoRef")
                && details.contains("input validation failed")
                && details.contains("$.user_id: expected number, got string")
    ));
}

#[test]
fn host_contract_validation_disabled_keeps_bridge_permissive() {
    let cache_dir = TestCacheDir::new("host-contract-validation-disabled");
    let vm = RustTs::new(cache_dir.vm_options()).expect("create vm");

    vm.registry()
        .function::<EchoValidation>()
        .expect("register host function");
    vm.load_script("validation", HOST_VALIDATION_SCRIPT)
        .expect("load validation script");
    let result = vm
        .call_function("validation", "echo", vec![json!({ "id": "bad" })])
        .expect("validation disabled");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(result, json!({ "id": "bad" }));
}

#[test]
fn host_contract_output_validation_can_be_enabled_for_debug() {
    let cache_dir = TestCacheDir::new("host-contract-output-validation");
    let mut options = cache_dir.vm_options();
    options.contract_validation = VmContractValidation::InputsAndOutputs;
    let vm = RustTs::new(options).expect("create vm");

    vm.registry()
        .function::<BadOutputValidation>()
        .expect("register host function");
    vm.load_script("validation", HOST_VALIDATION_SCRIPT)
        .expect("load validation script");

    let error = vm
        .call_function("validation", "badOutput", vec![json!(1)])
        .expect_err("invalid output should fail");

    vm.shutdown().expect("shutdown vm");

    assert!(matches!(
        error,
        VmError::Execution { details }
            if details.contains("validation.badOutput")
                && details.contains("output validation failed")
    ));
}

#[test]
fn host_contract_validation_can_reject_unknown_input_fields() {
    let cache_dir = TestCacheDir::new("host-contract-unknown-fields");
    let mut options = cache_dir.vm_options();
    options.contract_validation = VmContractValidation::Inputs;
    options.unknown_field_validation = VmUnknownFieldValidation::Reject;
    let vm = RustTs::new(options).expect("create vm");

    vm.registry()
        .function::<EchoValidation>()
        .expect("register host function");
    vm.load_script("validation", HOST_VALIDATION_SCRIPT)
        .expect("load validation script");

    let error = vm
        .call_function(
            "validation",
            "echo",
            vec![json!({ "id": 1, "extra": true })],
        )
        .expect_err("unknown input field should fail");

    vm.shutdown().expect("shutdown vm");

    assert!(matches!(
        error,
        VmError::Execution { details }
            if details.contains("validation.echo")
                && details.contains("input validation failed")
                && details.contains("$.extra: unknown field")
    ));
}

#[cfg(feature = "derive")]
#[test]
fn host_contract_validation_uses_flattened_derived_input_schema() {
    let cache_dir = TestCacheDir::new("host-contract-flatten-validation");
    let mut options = cache_dir.vm_options();
    options.contract_validation = VmContractValidation::Inputs;
    options.unknown_field_validation = VmUnknownFieldValidation::Reject;
    let vm = RustTs::new(options).expect("create vm");

    vm.registry()
        .function::<RecordAction>()
        .expect("register flattened host function");
    vm.load_script(
        "action-script",
        r#"
        export function recordGood() {
          return action.record({
            actionId: "act-1",
            actorId: 7,
            displayName: "Nami",
            confirmed: true,
          });
        }

        export function recordBad() {
          return action.record({
            actionId: "act-1",
            actorId: 7,
            displayName: "Nami",
            confirmed: true,
            actor: { actorId: 7, displayName: "Nami" },
          });
        }
        "#,
    )
    .expect("load action script");

    let result = vm
        .call_function("action-script", "recordGood", Vec::new())
        .expect("flattened input should validate");
    let error = vm
        .call_function("action-script", "recordBad", Vec::new())
        .expect_err("nested pre-flatten field should be unknown");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(result, json!("act-1:7"));
    assert!(matches!(
        error,
        VmError::Execution { details }
            if details.contains("action.record")
                && details.contains("input validation failed")
                && details.contains("$.actor: unknown field")
    ));
}

#[test]
fn host_contract_validation_allows_unknown_input_fields_by_default() {
    let cache_dir = TestCacheDir::new("host-contract-unknown-fields-default");
    let mut options = cache_dir.vm_options();
    options.contract_validation = VmContractValidation::Inputs;
    let vm = RustTs::new(options).expect("create vm");

    vm.registry()
        .function::<EchoValidation>()
        .expect("register host function");
    vm.load_script("validation", HOST_VALIDATION_SCRIPT)
        .expect("load validation script");
    let result = vm
        .call_function(
            "validation",
            "echo",
            vec![json!({ "id": 1, "extra": true })],
        )
        .expect("unknown fields allowed by default");

    vm.shutdown().expect("shutdown vm");

    assert_eq!(result, json!({ "id": 1, "extra": true }));
}
