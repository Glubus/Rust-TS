use std::time::{Duration, Instant};

use rustts::{
    Engine, HostContract, HostContractKind, HostFunction, Schema, TsType, VmContractValidation,
    VmError, VmOptions,
};
use serde_json::json;

fn engine_with_timeout(timeout: Duration) -> Engine {
    Engine::new(&VmOptions {
        execution_timeout: timeout,
        ..VmOptions::default()
    })
    .expect("create engine")
}

fn engine() -> Engine {
    Engine::new(&VmOptions::default()).expect("create engine")
}

fn engine_with(source: &str) -> Engine {
    let mut engine = engine();
    engine.load_script("script", source).expect("load script");
    engine
}

struct Double;

impl HostContract for Double {
    const NAME: &'static str = "math.double";
    const IMPORT_MODULE: &'static str = "host";
    const EXPORT_PATH: &'static [&'static str] = &["math", "double"];

    fn schema() -> Schema {
        Schema::typed("DoubleInput", TsType::Number)
    }

    fn kind() -> HostContractKind {
        HostContractKind::Function
    }
}

impl HostFunction for Double {
    type Input = f64;
    type Output = f64;

    fn output_schema() -> Schema {
        Schema::typed("DoubleOutput", TsType::Number)
    }

    fn call(input: Self::Input) -> Result<Self::Output, VmError> {
        Ok(input * 2.0)
    }
}

struct Failing;

impl HostContract for Failing {
    const NAME: &'static str = "host.fail";

    fn schema() -> Schema {
        Schema::typed("FailInput", TsType::Null)
    }

    fn kind() -> HostContractKind {
        HostContractKind::Function
    }
}

impl HostFunction for Failing {
    type Input = ();
    type Output = f64;

    fn output_schema() -> Schema {
        Schema::typed("FailOutput", TsType::Number)
    }

    fn call((): Self::Input) -> Result<Self::Output, VmError> {
        Err(VmError::Execution {
            details: String::from("boom"),
        })
    }
}

#[test]
fn calling_an_unknown_script_is_script_not_found() {
    let result = engine().call::<f64>("missing", "run", ());

    assert!(
        matches!(result, Err(VmError::ScriptNotFound { ref script_id }) if script_id == "missing"),
        "{result:?}"
    );
}

#[test]
fn calling_a_missing_export_is_function_not_found() {
    let engine = engine_with("export function present(): number { return 1; }");

    let result = engine.call::<f64>("script", "absent", ());

    assert!(
        matches!(result, Err(VmError::FunctionNotFound { ref function_name, .. }) if function_name == "absent"),
        "{result:?}"
    );
}

#[test]
fn script_exceptions_reach_rust_with_their_message() {
    let engine = engine_with(r#"export function run(): void { throw new Error("bad input"); }"#);

    let result = engine.call::<()>("script", "run", ());

    assert!(
        matches!(result, Err(VmError::Execution { ref details }) if details.contains("bad input")),
        "{result:?}"
    );
}

#[test]
fn a_throwing_event_handler_fails_the_emit() {
    let engine =
        engine_with(r#"ctx.on("tick", () => { throw new Error("handler failed"); }); export {};"#);

    let result = engine.emit("tick", &json!({}));

    assert!(
        matches!(result, Err(VmError::Execution { ref details }) if details.contains("handler failed")),
        "{result:?}"
    );
}

#[test]
fn unloaded_scripts_are_gone() {
    let mut engine = engine_with("export function run(): number { return 1; }");

    engine.unload_script("script").expect("unload script");

    assert!(matches!(
        engine.call::<f64>("script", "run", ()),
        Err(VmError::ScriptNotFound { .. })
    ));
    assert!(matches!(
        engine.unload_script("script"),
        Err(VmError::ScriptNotFound { .. })
    ));
}

#[test]
fn typed_host_functions_are_reachable_by_import_and_by_global() {
    let mut engine = engine();
    engine
        .registry()
        .typed_function::<Double>()
        .expect("register host function");
    engine
        .load_script(
            "script",
            r#"
            import { math as imported } from "host";
            export function viaImport(value: number): number { return imported.double(value); }
            export function viaGlobal(value: number): number { return (globalThis as any).math.double(value); }
            "#,
        )
        .expect("load script");

    let via_import: f64 = engine
        .call("script", "viaImport", (21,))
        .expect("call via import");
    let via_global: f64 = engine
        .call("script", "viaGlobal", (4,))
        .expect("call via global");

    assert_eq!(via_import, 42.0);
    assert_eq!(via_global, 8.0);
}

#[test]
fn untyped_host_functions_use_the_json_path() {
    let mut engine = engine();
    engine
        .registry()
        .function::<Double>()
        .expect("register host function");
    engine
        .load_script(
            "script",
            r#"export function run(): number { return (globalThis as any).math.double(2.5); }"#,
        )
        .expect("load script");

    let result: f64 = engine.call("script", "run", ()).expect("call");

    assert_eq!(result, 5.0);
}

#[test]
fn host_function_errors_are_catchable_in_scripts() {
    let mut engine = engine();
    engine
        .registry()
        .typed_function::<Failing>()
        .expect("register host function");
    engine
        .load_script(
            "script",
            r#"
            export function caught(): string {
              try { (globalThis as any).host.fail(); return "no error"; }
              catch (error) { return String((error as Error).message); }
            }
            export function uncaught(): number { return (globalThis as any).host.fail(); }
            "#,
        )
        .expect("load script");

    let caught: String = engine.call("script", "caught", ()).expect("call caught");
    let uncaught = engine.call::<f64>("script", "uncaught", ());

    assert!(caught.contains("boom"), "{caught}");
    assert!(
        matches!(uncaught, Err(VmError::Execution { ref details }) if details.contains("boom")),
        "{uncaught:?}"
    );
}

#[test]
fn contract_validation_rejects_inputs_outside_the_schema() {
    let mut engine = Engine::new(&VmOptions {
        contract_validation: VmContractValidation::Inputs,
        ..VmOptions::default()
    })
    .expect("create engine");
    engine
        .registry()
        .typed_function::<Double>()
        .expect("register host function");
    engine
        .load_script(
            "script",
            r#"
            export function valid(): number { return (globalThis as any).math.double(3); }
            export function invalid(): number { return (globalThis as any).math.double("three"); }
            "#,
        )
        .expect("load script");

    let valid: f64 = engine.call("script", "valid", ()).expect("valid input");
    let invalid = engine.call::<f64>("script", "invalid", ());

    assert_eq!(valid, 6.0);
    assert!(
        matches!(invalid, Err(VmError::Execution { ref details }) if details.contains("validation")),
        "{invalid:?}"
    );
}

#[test]
fn exceeding_the_memory_limit_fails_the_call_and_the_engine_stays_usable() {
    let mut engine = Engine::new(&VmOptions {
        memory_limit_bytes: 8 * 1024 * 1024,
        ..VmOptions::default()
    })
    .expect("create engine");
    engine
        .load_script(
            "script",
            r#"
            export function hog(): number {
              const chunks: string[] = [];
              for (let i = 0; i < 1_000_000; i++) chunks.push("x".repeat(1024) + i);
              return chunks.length;
            }
            export function small(): number { return 1; }
            "#,
        )
        .expect("load script");

    let hog = engine.call::<f64>("script", "hog", ());
    let small: f64 = engine
        .call("script", "small", ())
        .expect("call after out of memory");

    assert!(hog.is_err(), "{hog:?}");
    assert_eq!(small, 1.0);
}

#[test]
fn scripts_do_not_share_globals() {
    let mut engine = engine();
    let source = r#"
        (globalThis as any).counter = ((globalThis as any).counter ?? 0) + 1;
        export function counter(): number { return (globalThis as any).counter; }
    "#;
    engine.load_script("first", source).expect("load first");
    engine.load_script("second", source).expect("load second");

    let first: f64 = engine.call("first", "counter", ()).expect("first counter");
    let second: f64 = engine
        .call("second", "counter", ())
        .expect("second counter");

    assert_eq!((first, second), (1.0, 1.0));
}

#[test]
fn a_syntax_error_fails_the_load_and_keeps_the_previous_version() {
    let mut engine = engine_with("export function version(): number { return 1; }");

    let reload = engine.load_script("script", "export function version( { return 2; }");
    let version: f64 = engine
        .call("script", "version", ())
        .expect("previous version");

    assert!(reload.is_err(), "{reload:?}");
    assert_eq!(version, 1.0);
}

#[test]
fn dynamic_imports_are_rejected_at_load() {
    let result = engine().load_script(
        "script",
        r#"export async function run() { return import("./other"); }"#,
    );

    assert!(result.is_err(), "{result:?}");
}

#[test]
fn runaway_call_is_interrupted_and_engine_stays_usable() {
    let mut engine = engine_with_timeout(Duration::from_millis(100));
    engine
        .load_script(
            "loops",
            r#"
            export function spin(): void { while (true) {} }
            export function add(a: number, b: number): number { return a + b; }
            "#,
        )
        .expect("load script");

    let started = Instant::now();
    let result = engine.call::<()>("loops", "spin", ());

    assert!(result.is_err(), "{result:?}");
    assert!(started.elapsed() < Duration::from_secs(5));
    let sum: f64 = engine
        .call("loops", "add", (2, 3))
        .expect("call after interrupt");
    assert_eq!(sum, 5.0);
}

#[test]
fn runaway_top_level_code_fails_the_load_and_keeps_the_previous_version() {
    let mut engine = engine_with_timeout(Duration::from_millis(100));
    engine
        .load_script("script", "export function version(): number { return 1; }")
        .expect("load first version");

    let reload = engine.load_script(
        "script",
        "while (true) {}\nexport function version(): number { return 2; }",
    );

    assert!(reload.is_err(), "{reload:?}");
    let version: f64 = engine
        .call("script", "version", ())
        .expect("previous version");
    assert_eq!(version, 1.0);
}

/// A script controls its handler list; an absurd length must fail the emit, not
/// panic the host (rquickjs `Array::len` panics above `i32::MAX`).
#[test]
fn emit_rejects_handler_list_with_oversized_length() {
    let mut engine = Engine::new(&VmOptions::default()).expect("create engine");
    engine
        .load_script(
            "tampered",
            r#"
            ctx.on("tick", () => {});
            globalThis.__vm_handlers.tick.length = 2 ** 32 - 1;
            export {};
            "#,
        )
        .expect("load script");

    let result = engine.emit("tick", &json!({ "n": 1 }));

    assert!(result.is_err(), "{result:?}");
}

#[test]
fn emit_calls_every_registered_handler() {
    let mut engine = Engine::new(&VmOptions::default()).expect("create engine");
    engine
        .load_script(
            "listener",
            r#"
            let total = 0;
            ctx.on("tick", event => { total += event.n; });
            ctx.on("tick", event => { total += event.n * 10; });
            export function read(): number { return total; }
            "#,
        )
        .expect("load script");

    let delivered = engine.emit("tick", &json!({ "n": 2 })).expect("emit");
    let total: f64 = engine.call("listener", "read", ()).expect("read total");

    assert_eq!(delivered, 1);
    assert_eq!(total, 22.0);
}
