//! Repeatable operational measurements; run with --release for performance comparisons.
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use rustts::{RustTs, VmError, VmOptions};
use serde_json::{Value, json};

type ProbeResult<T> = Result<T, Box<dyn std::error::Error>>;
const MODULE_COUNT: usize = 30;
const CACHE_SAMPLES: usize = 20;
const CLIENTS: usize = 16;
const CALLS_PER_CLIENT: usize = 100;
const LOAD_COUNT: usize = 32;
const SMALL_SCRIPT: &str = "export const value = 1;";
const PRESSURE_SCRIPT: &str =
    "export function run() { let n = 0; for(let i=0;i<100000;i++) n+=i; return n; }";

fn main() -> ProbeResult<()> {
    let root = tempfile::tempdir()?;
    let project = create_project(root.path())?;
    let cache_loads = measure_cache_loads(root.path(), &project)?;
    let vm = create_pressure_vm(root.path())?;
    let saturation = measure_saturation(&vm);
    let load_contention = measure_load_contention(&vm)?;
    let idle = measure_idle(&vm)?;
    vm.shutdown()?;

    let report = json!({
        "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        "project_modules": MODULE_COUNT + 1,
        "workers": 2,
        "cache_loads": cache_loads,
        "saturation": saturation,
        "load_contention": load_contention,
        "idle": idle,
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn create_project(root: &Path) -> ProbeResult<PathBuf> {
    let mut imports = String::new();
    for index in 0..MODULE_COUNT {
        std::fs::write(
            root.join(format!("m{index}.ts")),
            format!("export const v{index}: number = {index};"),
        )?;
        imports.push_str(&format!("import {{v{index}}} from './m{index}';\n"));
    }
    imports.push_str("export function run() { return v0 + v29; }");
    let entry = root.join("main.ts");
    std::fs::write(&entry, imports)?;
    Ok(entry)
}

fn measure_cache_loads(root: &Path, project: &Path) -> ProbeResult<Value> {
    let mut cold = Vec::new();
    let mut warm = Vec::new();
    for index in 0..CACHE_SAMPLES {
        let vm = RustTs::new(VmOptions {
            worker_threads: 2,
            cache_dir: root.join(format!("cache{index}")),
            ..VmOptions::default()
        })?;
        cold.push(timed_project_load(&vm, project)?);
        warm.push(timed_project_load(&vm, project)?);
        vm.shutdown()?;
    }
    Ok(json!({"cold": distribution(cold), "warm": distribution(warm)}))
}

fn timed_project_load(vm: &RustTs, project: &Path) -> Result<f64, VmError> {
    let start = Instant::now();
    vm.load_script_project("project", project)?;
    Ok(elapsed_ms(start))
}

fn create_pressure_vm(root: &Path) -> Result<RustTs, VmError> {
    let vm = RustTs::new(VmOptions {
        worker_threads: 2,
        queue_capacity: 1,
        max_scripts_per_worker: 128,
        cache_dir: root.join("pressure"),
        ..VmOptions::default()
    })?;
    vm.load_script("pressure", PRESSURE_SCRIPT)?;
    Ok(vm)
}

#[derive(Default)]
struct CallSamples {
    accepted: Vec<f64>,
    rejected: Vec<f64>,
}

fn measure_saturation(vm: &RustTs) -> Value {
    let barrier = Arc::new(Barrier::new(CLIENTS));
    let samples = std::thread::scope(|scope| {
        let threads: Vec<_> = (0..CLIENTS)
            .map(|_| {
                let barrier = barrier.clone();
                scope.spawn(move || run_client(vm, &barrier))
            })
            .collect();
        let mut combined = CallSamples::default();
        for thread in threads {
            let client = thread.join().expect("saturation client panicked");
            combined.accepted.extend(client.accepted);
            combined.rejected.extend(client.rejected);
        }
        combined
    });
    json!({"accepted": distribution(samples.accepted), "rejected": distribution(samples.rejected)})
}

fn run_client(vm: &RustTs, barrier: &Barrier) -> CallSamples {
    barrier.wait();
    let mut samples = CallSamples::default();
    for _ in 0..CALLS_PER_CLIENT {
        let start = Instant::now();
        let result = vm.call_function("pressure", "run", vec![]);
        let elapsed = elapsed_ms(start);
        match result {
            Ok(_) => samples.accepted.push(elapsed),
            Err(VmError::QueueFull) => samples.rejected.push(elapsed),
            Err(error) => panic!("unexpected failure: {error}"),
        }
    }
    samples
}

fn measure_load_contention(vm: &RustTs) -> Result<Value, VmError> {
    let start = Instant::now();
    for index in 0..LOAD_COUNT {
        vm.load_script(format!("serial{index}"), SMALL_SCRIPT)?;
    }
    let serial_ms = elapsed_ms(start);
    let start = Instant::now();
    std::thread::scope(|scope| {
        for index in 0..LOAD_COUNT {
            scope.spawn(move || {
                vm.load_script(format!("parallel{index}"), SMALL_SCRIPT)
                    .unwrap()
            });
        }
    });
    Ok(json!({"loads": LOAD_COUNT, "serial_ms": serial_ms, "concurrent_ms": elapsed_ms(start)}))
}

fn measure_idle(vm: &RustTs) -> Result<Value, VmError> {
    let before = memory_summary(&vm.stats()?);
    eprintln!("idle measurement: pid={} for 3 seconds", std::process::id());
    std::thread::sleep(Duration::from_secs(3));
    let after = memory_summary(&vm.stats()?);
    Ok(json!({"seconds": 3, "before": before, "after": after}))
}

fn memory_summary(stats: &rustts::VmStats) -> Value {
    let quickjs_bytes: u64 = stats
        .workers
        .iter()
        .filter_map(|worker| worker.sync_quickjs_memory)
        .map(|memory| memory.memory_used_bytes)
        .sum();
    json!({"quickjs_bytes": quickjs_bytes, "loaded_scripts": stats.loaded_scripts, "process_memory": stats.process_memory})
}

fn distribution(mut samples: Vec<f64>) -> Value {
    samples.sort_by(f64::total_cmp);
    if samples.is_empty() {
        return Value::Null;
    }
    json!({"samples": samples.len(), "p50_ms": samples[samples.len()/2], "p99_ms": samples[(samples.len()*99/100).min(samples.len()-1)]})
}

fn elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}
