//! Host functions registered as closures: state, errors, caller identity, validation and
//! generated declarations.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[cfg(feature = "derive")]
use rustts::js::{Ctx, Value as JsValue};
use rustts::{
    Engine, HostContract, HostContractKind, HostFunction, HostFunctionSignature,
    InMemoryHostContractRegistry, Schema, TsType, VmContractValidation, VmError, VmOptions,
};
#[cfg(feature = "derive")]
use rustts::{JsEncode, TsSchema};
#[cfg(feature = "derive")]
use serde_json::Value;

/// `counter.bump(name)`: counts calls, returns the running total.
struct Bump;

impl HostContract for Bump {
    const NAME: &'static str = "counter.bump";

    fn schema() -> Schema {
        Schema::typed("BumpInput", TsType::String)
    }

    fn kind() -> HostContractKind {
        HostContractKind::Function
    }
}

impl HostFunctionSignature for Bump {
    type Input = String;
    type Output = u64;
}

/// `host.whoami()`: the id of the calling script.
struct WhoAmI;

impl HostContract for WhoAmI {
    const NAME: &'static str = "host.whoami";

    fn schema() -> Schema {
        Schema::typed("WhoAmIInput", TsType::Null)
    }

    fn kind() -> HostContractKind {
        HostContractKind::Function
    }
}

impl HostFunctionSignature for WhoAmI {
    type Input = ();
    type Output = String;
}

/// A label declared as a TypeScript `string` that can carry a number, so a handler can
/// break its declared output.
#[cfg(feature = "derive")]
enum Label {
    Text(String),
    Number(f64),
}

#[cfg(feature = "derive")]
impl TsSchema for Label {
    fn ts_type() -> TsType {
        TsType::String
    }
}

#[cfg(feature = "derive")]
impl JsEncode for Label {
    fn encode_js<'js>(&self, ctx: &Ctx<'js>) -> rustts::js::Result<JsValue<'js>> {
        match self {
            Self::Text(text) => text.encode_js(ctx),
            Self::Number(number) => number.encode_js(ctx),
        }
    }
}

/// `tags.add({ id })`: validated input and output.
#[cfg(feature = "derive")]
struct AddTag;

#[cfg(feature = "derive")]
#[derive(Debug, Clone, PartialEq, TsSchema)]
#[rustts(decode_only)]
struct AddTagInput {
    id: u32,
}

#[cfg(feature = "derive")]
#[derive(TsSchema)]
#[rustts(encode_only)]
struct AddTagOutput {
    id: u32,
    script: Label,
}

#[cfg(feature = "derive")]
impl HostContract for AddTag {
    const NAME: &'static str = "tags.add";

    fn schema() -> Schema {
        AddTagInput::schema()
    }

    fn kind() -> HostContractKind {
        HostContractKind::Function
    }
}

#[cfg(feature = "derive")]
impl HostFunctionSignature for AddTag {
    type Input = AddTagInput;
    type Output = AddTagOutput;
}

/// `user.lookup(id)`: implemented statically as well, to compare registrations.
struct Lookup;

impl HostContract for Lookup {
    const NAME: &'static str = "user.lookup";
    const IMPORT_MODULE: &'static str = "host";
    const EXPORT_PATH: &'static [&'static str] = &["user", "lookup"];

    fn schema() -> Schema {
        Schema::typed("LookupInput", TsType::Number)
    }

    fn kind() -> HostContractKind {
        HostContractKind::Function
    }
}

impl HostFunctionSignature for Lookup {
    type Input = u64;
    type Output = String;
}

impl HostFunction for Lookup {
    fn call(input: Self::Input) -> Result<Self::Output, VmError> {
        Ok(format!("user-{input}"))
    }
}

fn engine() -> Engine {
    Engine::new(&VmOptions::default()).expect("create engine")
}

fn validating_engine() -> Engine {
    Engine::new(&VmOptions {
        contract_validation: VmContractValidation::InputsAndOutputs,
        ..VmOptions::default()
    })
    .expect("create engine")
}

const WHO_SCRIPT: &str = r#"export function who(): string { return host.whoami(); }"#;

#[test]
fn closure_state_is_shared_by_every_script() {
    let mut engine = engine();
    let total = Arc::new(AtomicU64::new(0));
    let names = Arc::new(Mutex::new(Vec::new()));
    let (handler_total, handler_names) = (Arc::clone(&total), Arc::clone(&names));
    engine
        .registry()
        .function_with::<Bump>(move |name| {
            handler_names.lock().expect("names").push(name);
            Ok(handler_total.fetch_add(1, Ordering::SeqCst) + 1)
        })
        .expect("register closure");
    for id in ["alpha", "beta"] {
        let source =
            format!(r#"export function bump(): number {{ return counter.bump("{id}"); }}"#);
        engine.load_script(id, &source).expect("load script");
    }

    let totals: Vec<u64> = ["alpha", "beta", "alpha"]
        .into_iter()
        .map(|id| engine.call(id, "bump", ()).expect("call bump"))
        .collect();

    assert_eq!(totals, [1, 2, 3]);
    assert_eq!(total.load(Ordering::SeqCst), 3);
    assert_eq!(*names.lock().expect("names"), ["alpha", "beta", "alpha"]);
}

#[test]
fn closure_errors_are_catchable_in_scripts() {
    let mut engine = engine();
    engine
        .registry()
        .function_with::<Bump>(|name| {
            Err(VmError::Execution {
                details: format!("{name} may not bump"),
            })
        })
        .expect("register closure");
    engine
        .load_script(
            "script",
            r#"
            export function caught(): string {
              try { counter.bump("mallory"); return "no error"; }
              catch (error) { return String((error as Error).message); }
            }
            export function uncaught(): number { return counter.bump("eve"); }
            "#,
        )
        .expect("load script");

    let caught: String = engine.call("script", "caught", ()).expect("call caught");
    let uncaught = engine.call::<f64>("script", "uncaught", ());

    assert!(caught.contains("mallory may not bump"), "{caught}");
    assert!(
        matches!(uncaught, Err(VmError::Execution { ref details }) if details.contains("eve may not bump")),
        "{uncaught:?}"
    );
}

/// Loads `alpha` and `beta`, reloads `alpha`, and returns what `who()` answers for each.
fn ids_seen_by(engine: &mut Engine) -> [String; 3] {
    engine.load_script("alpha", WHO_SCRIPT).expect("load alpha");
    engine.load_script("beta", WHO_SCRIPT).expect("load beta");
    let alpha: String = engine.call("alpha", "who", ()).expect("call alpha");
    engine
        .load_script(
            "alpha",
            r#"export function who(): string { return "v2:" + host.whoami(); }"#,
        )
        .expect("reload alpha");
    let reloaded: String = engine
        .call("alpha", "who", ())
        .expect("call reloaded alpha");
    let beta: String = engine.call("beta", "who", ()).expect("call beta");
    [alpha, reloaded, beta]
}

#[test]
fn typed_caller_handlers_see_the_calling_script_across_reloads() {
    let mut engine = engine();
    engine
        .registry()
        .function_with_caller::<WhoAmI>(|caller, ()| Ok(caller.script_id().to_owned()))
        .expect("register closure");

    assert_eq!(ids_seen_by(&mut engine), ["alpha", "v2:alpha", "beta"]);
}

#[test]
fn caller_handlers_see_the_calling_script_when_validating() {
    let mut engine = validating_engine();
    engine
        .registry()
        .function_with_caller::<WhoAmI>(|caller, ()| Ok(caller.script_id().to_owned()))
        .expect("register closure");

    assert_eq!(ids_seen_by(&mut engine), ["alpha", "v2:alpha", "beta"]);
}

#[cfg(feature = "derive")]
#[test]
fn validation_guards_closure_inputs_and_outputs() {
    let mut engine = validating_engine();
    let handled = Arc::new(Mutex::new(Vec::new()));
    let handler_handled = Arc::clone(&handled);
    engine
        .registry()
        .function_with_caller::<AddTag>(move |caller, input| {
            handler_handled.lock().expect("handled").push(input.clone());
            let script = if input.id == 0 {
                Label::Number(0.0)
            } else {
                Label::Text(caller.script_id().to_owned())
            };
            Ok(AddTagOutput {
                id: input.id,
                script,
            })
        })
        .expect("register closure");
    engine
        .load_script(
            "tagger",
            r#"
            export function valid(): string { return tags.add({ id: 7 }).script; }
            export function badInput(): unknown { return tags.add({ id: "seven" }); }
            export function badOutput(): unknown { return tags.add({ id: 0 }); }
            "#,
        )
        .expect("load script");

    let valid: String = engine.call("tagger", "valid", ()).expect("valid call");
    let bad_input = engine.call::<Value>("tagger", "badInput", ());
    let bad_output = engine.call::<Value>("tagger", "badOutput", ());

    assert_eq!(valid, "tagger");
    for (result, direction) in [(bad_input, "input"), (bad_output, "output")] {
        let expected = format!("{direction} validation failed");
        assert!(
            matches!(result, Err(VmError::Execution { ref details }) if details.contains(&expected)),
            "{direction}: {result:?}"
        );
    }
    assert_eq!(
        *handled.lock().expect("handled"),
        [AddTagInput { id: 7 }, AddTagInput { id: 0 }],
        "an invalid input never reaches the handler"
    );
}

#[test]
fn stateful_typed_closures_run_when_validating() {
    let mut engine = validating_engine();
    let total = Arc::new(AtomicU64::new(0));
    let handler_total = Arc::clone(&total);
    engine
        .registry()
        .function_with::<Bump>(move |_| Ok(handler_total.fetch_add(1, Ordering::SeqCst) + 1))
        .expect("register closure");
    engine
        .load_script(
            "script",
            r#"
            export function twice(): number { counter.bump("a"); return counter.bump("b"); }
            export function invalid(): number { return counter.bump(42 as any); }
            "#,
        )
        .expect("load script");

    let second: u64 = engine.call("script", "twice", ()).expect("valid calls");
    let invalid = engine.call::<u64>("script", "invalid", ());

    assert_eq!(second, 2);
    assert!(
        matches!(invalid, Err(VmError::Execution { .. })),
        "{invalid:?}"
    );
    assert_eq!(total.load(Ordering::SeqCst), 2);
}

/// Declarations and SDK source of a registry holding only what `register` adds.
fn generated(
    register: impl FnOnce(
        &InMemoryHostContractRegistry,
    ) -> Result<&InMemoryHostContractRegistry, VmError>,
) -> (String, String) {
    let registry = InMemoryHostContractRegistry::new();
    register(&registry).expect("register contract");
    (
        registry.dts().expect("render declarations"),
        registry.sdk().expect("render sdk"),
    )
}

#[test]
fn closures_generate_the_same_typed_contract_as_static_functions() {
    let from_static = generated(|registry| registry.function::<Lookup>());
    let from_closure =
        generated(|registry| registry.function_with::<Lookup>(|id| Ok(format!("closure-{id}"))));
    let from_caller = generated(|registry| {
        registry
            .function_with_caller::<Lookup>(|caller, id| Ok(format!("{}-{id}", caller.script_id())))
    });

    assert_eq!(from_closure, from_static);
    assert_eq!(from_caller, from_static);
}
