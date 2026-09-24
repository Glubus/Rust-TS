use std::time::{Duration, Instant};

use rustts::{Engine, VmOptions};
use serde_json::json;

fn engine_with_timeout(timeout: Duration) -> Engine {
    Engine::new(&VmOptions {
        execution_timeout: timeout,
        ..VmOptions::default()
    })
    .expect("create engine")
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
