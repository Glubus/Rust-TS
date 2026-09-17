mod support;

use rustts::{RustTs, VmError};
use serde_json::json;
use std::time::Duration;
use support::TestCacheDir;

// A broken interrupt must fail the test instead of hanging the entire suite.
fn bounded(test: impl FnOnce() + Send + 'static) {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(test));
        let _ = tx.send(result);
    });
    if let Err(panic) = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("runtime operation hung")
    {
        std::panic::resume_unwind(panic);
    }
}

#[test]
fn infinite_javascript_is_interrupted_and_worker_remains_usable() {
    bounded(|| {
        let cache = TestCacheDir::new("interrupt");
        let mut options = cache.vm_options();
        options.execution_timeout = Duration::from_millis(30);
        let vm = RustTs::new(options).unwrap();
        assert!(vm.load_script("bad-load", "while (true) {}").is_err());
        vm.load_script(
            "script",
            "export function spin() { while(true) {} } export function ok() { return 42; }",
        )
        .unwrap();
        assert!(vm.call_function("script", "spin", vec![]).is_err());
        assert_eq!(vm.call_function("script", "ok", vec![]).unwrap(), json!(42));
        vm.shutdown().unwrap();
        vm.shutdown().unwrap();
    });
}

#[test]
fn failed_reload_preserves_old_script_and_registration() {
    bounded(|| {
        let cache = TestCacheDir::new("reload-rollback");
        let vm = RustTs::new(cache.vm_options()).unwrap();
        let original = vm
            .load_script(
                "script",
                "let count = 0; export function next() { return ++count; }",
            )
            .unwrap();
        assert_eq!(
            vm.call_function("script", "next", vec![]).unwrap(),
            json!(1)
        );
        for _ in 0..10 {
            assert!(
                vm.load_script(
                    "script",
                    "throw new Error('broken'); export function next() { return 999; }"
                )
                .is_err()
            );
        }
        assert_eq!(
            vm.call_function("script", "next", vec![]).unwrap(),
            json!(2)
        );
        assert_eq!(
            vm.describe_script("script").unwrap().unwrap().source_hash,
            original.cache_key
        );
        vm.shutdown().unwrap();
    });
}

#[test]
fn slow_subscriber_drops_new_events_and_reports_loss() {
    let cache = TestCacheDir::new("bounded-events");
    let mut options = cache.vm_options();
    options.event_queue_capacity = 2;
    let vm = RustTs::new(options).unwrap();
    let events = vm.subscribe();
    vm.load_script("script", "export function run() { return 42; }")
        .unwrap();
    for _ in 0..10 {
        vm.call_function("script", "run", vec![]).unwrap();
    }
    assert_eq!(events.dropped_events(), 9);
    assert!(events.try_recv().is_some());
    assert!(events.try_recv().is_some());
    assert!(events.try_recv().is_none());
}

#[test]
fn shutdown_interrupts_running_javascript_before_its_deadline() {
    bounded(|| {
        use rustts::{HostContract, HostContractKind, HostFunction, Schema, TsType};
        use std::sync::atomic::{AtomicBool, Ordering};
        static ENTERED: AtomicBool = AtomicBool::new(false);
        struct Started;
        impl HostContract for Started {
            const NAME: &'static str = "started";
            fn schema() -> Schema {
                Schema::typed("Input", TsType::Null)
            }
            fn kind() -> HostContractKind {
                HostContractKind::Function
            }
        }
        impl HostFunction for Started {
            type Input = ();
            type Output = ();
            fn call(_: ()) -> Result<(), VmError> {
                ENTERED.store(true, Ordering::Release);
                Ok(())
            }
        }
        let cache = TestCacheDir::new("shutdown-js");
        let mut options = cache.vm_options();
        options.execution_timeout = Duration::from_secs(60);
        let vm = RustTs::new(options).unwrap();
        vm.registry().function::<Started>().unwrap();
        vm.load_script(
            "script",
            "export function run() { __host.call('started', 'null'); while(true) {} }",
        )
        .unwrap();
        let other = vm.clone();
        let active = std::thread::spawn(move || other.call_function("script", "run", vec![]));
        while !ENTERED.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
        vm.shutdown().unwrap();
        assert!(active.join().unwrap().is_err());
    });
}

#[test]
fn failed_project_reload_keeps_previous_dependency_graph() {
    let root = TestCacheDir::new("project-rollback");
    let entry = root.path().join("main.ts");
    let dependency = root.path().join("dependency.ts");
    std::fs::write(
        &entry,
        "import { value } from './dependency'; export function run() { return value; }",
    )
    .unwrap();
    std::fs::write(&dependency, "export const value = 42;").unwrap();
    let vm = RustTs::new(root.vm_options()).unwrap();
    vm.load_script_project("script", &entry).unwrap();
    let original = vm.runtime_snapshot().unwrap();
    std::fs::write(
        &dependency,
        "throw new Error('bad module'); export const value = 99;",
    )
    .unwrap();
    assert!(vm.load_script_project("script", &entry).is_err());
    assert_eq!(
        vm.call_function("script", "run", vec![]).unwrap(),
        json!(42)
    );
    let after = vm.runtime_snapshot().unwrap();
    assert_eq!(after.scripts, original.scripts);
    assert_eq!(after.dependency_edges, original.dependency_edges);
    assert_eq!(after.event_routes, original.event_routes);
}

#[test]
fn shutdown_can_be_retried_while_host_handler_finishes() {
    bounded(|| {
        use rustts::{HostContract, HostContractKind, HostFunction, Schema, TsType};
        use std::sync::atomic::{AtomicBool, Ordering};
        static ENTERED: AtomicBool = AtomicBool::new(false);
        struct Slow;
        impl HostContract for Slow {
            const NAME: &'static str = "slow";
            fn schema() -> Schema {
                Schema::typed("Input", TsType::Null)
            }
            fn kind() -> HostContractKind {
                HostContractKind::Function
            }
        }
        impl HostFunction for Slow {
            type Input = ();
            type Output = ();
            fn call(_: ()) -> Result<(), VmError> {
                ENTERED.store(true, Ordering::Release);
                std::thread::sleep(Duration::from_millis(200));
                Ok(())
            }
        }
        let cache = TestCacheDir::new("shutdown-retry");
        let mut options = cache.vm_options();
        options.shutdown_timeout = Duration::from_millis(10);
        options.queue_capacity = 1;
        let vm = RustTs::new(options).unwrap();
        vm.registry().function::<Slow>().unwrap();
        vm.load_script(
            "script",
            "export function run() { return __host.call('slow', 'null'); }",
        )
        .unwrap();
        let other = vm.clone();
        let active = std::thread::spawn(move || other.call_function("script", "run", vec![]));
        while !ENTERED.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
        let other = vm.clone();
        let queued = std::thread::spawn(move || other.call_function("script", "run", vec![]));
        std::thread::sleep(Duration::from_millis(10));
        assert!(matches!(vm.shutdown(), Err(VmError::ShutdownTimeout)));
        let _ = active.join().unwrap();
        let _ = queued.join().unwrap();
        vm.shutdown().unwrap();
    });
}

#[cfg(feature = "async-promise")]
#[test]
fn async_budgets_cover_cpu_loops_pending_promises_and_failed_reload() {
    bounded(|| {
        let cache = TestCacheDir::new("async-budget");
        let mut options = cache.vm_options();
        options.execution_timeout = Duration::from_millis(40);
        let vm = RustTs::new(options).unwrap();
        let executor = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        executor.block_on(async {
            let script = vm.load_async_script("script", "export function ok() { return 42; } export function spin() { while(true) {} } export async function wait() { await new Promise(() => {}); }").await.unwrap();
            assert!(script.call_function("spin", &[]).await.is_err());
            assert!(matches!(script.call_function("wait", &[]).await, Err(VmError::ExecutionTimeout)));
            assert!(vm.load_async_script("script", "throw new Error('broken');").await.is_err());
            assert_eq!(script.call_function("ok", &[]).await.unwrap(), json!(42));
            assert!(vm.load_async_script("bad", "await new Promise(() => {});").await.is_err());
        });
        vm.shutdown().unwrap();
    });
}
