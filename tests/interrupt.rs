use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use rustts::{Engine, VmError, VmOptions};

const SCRIPT: &str = r#"
export function spin(): void { while (true) {} }
export function add(a: number, b: number): number { return a + b; }
"#;

/// An engine whose timeout is far longer than the tests, so only the handle can stop it.
fn engine() -> Engine {
    let mut engine = Engine::new(&VmOptions {
        execution_timeout: Duration::from_secs(60),
        ..VmOptions::default()
    })
    .expect("create engine");
    engine.load_script("loops", SCRIPT).expect("load script");
    engine
}

#[test]
fn another_thread_interrupts_a_runaway_call() {
    let engine = engine();
    let handle = engine.interrupt_handle();
    let finished = Arc::new(AtomicBool::new(false));
    // A request sent before the call starts is dropped by design, so keep asking
    // until the call returns.
    let interrupter = thread::spawn({
        let finished = Arc::clone(&finished);
        move || {
            while !finished.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(10));
                handle.interrupt();
            }
        }
    });

    let begin = Instant::now();
    let result = engine.call::<()>("loops", "spin", ());
    finished.store(true, Ordering::Release);
    interrupter.join().expect("interrupter thread");

    assert!(matches!(result, Err(VmError::Interrupted)), "{result:?}");
    assert!(begin.elapsed() < Duration::from_secs(10));
    let sum: f64 = engine
        .call("loops", "add", (2, 3))
        .expect("call after interrupt");
    assert_eq!(sum, 5.0);
}

#[test]
fn an_interrupt_requested_while_idle_does_not_stop_the_next_call() {
    let engine = engine();

    engine.interrupt_handle().interrupt();
    let sum: f64 = engine
        .call("loops", "add", (1, 1))
        .expect("call after idle interrupt");

    assert_eq!(sum, 2.0);
}

#[test]
fn a_timeout_is_not_reported_as_an_interrupt() {
    let mut engine = Engine::new(&VmOptions {
        execution_timeout: Duration::from_millis(50),
        ..VmOptions::default()
    })
    .expect("create engine");
    engine.load_script("loops", SCRIPT).expect("load script");

    let result = engine.call::<()>("loops", "spin", ());

    assert!(
        matches!(result, Err(VmError::Execution { .. })),
        "{result:?}"
    );
}
