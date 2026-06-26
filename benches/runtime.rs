use std::hint::black_box;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use criterion::{Criterion, criterion_group, criterion_main};
use serde_json::json;
use ts_embed_vm::{
    DeliveryMode, HostCallback, HostContract, HostContractKind, HostFunction, Schema, TsField,
    TsType, TsVm, VmError, VmOptions, VmProcessMemoryStats, VmQuickJsMemoryStats, VmStats,
};

const BASIC_SCRIPT: &str = include_str!("../tests/projects/basic_math/main.ts");
const EVENT_LISTENER_SCRIPT: &str = include_str!("../tests/projects/event_listener/main.ts");
const MANY_SMALL_SCRIPT_COUNT: usize = 400;
const MEMORY_REPORT_SCRIPT_COUNTS: &[usize] = &[100, 400, 800, 1000];
const MEMORY_REPORT_WORKER_THREADS: usize = 2;
const MEMORY_REPORT_MEMORY_LIMIT_BYTES: usize = 128 * 1024 * 1024;
const MEMORY_REPORT_CHILD_ENV: &str = "TSVM_MEMORY_REPORT_SCRIPT_COUNT";
const BYTES_PER_KIB: f64 = 1024.0;
const BYTES_PER_MIB: f64 = 1024.0 * 1024.0;

fn runtime_benchmarks(c: &mut Criterion) {
    if let Some(script_count) = memory_report_child_script_count() {
        report_many_small_scripts_process_memory(script_count);
        std::process::exit(0);
    }

    report_many_small_scripts_process_memory_curve();
    bench_cold_inline_load(c);
    bench_hot_function_call(c);
    bench_event_routing(c);
    bench_sdk_generation(c);
    bench_many_small_scripts_memory_shape(c);
}

fn bench_cold_inline_load(c: &mut Criterion) {
    c.bench_function("cold_inline_load", |b| {
        b.iter_batched(
            || new_vm("bench-cold-inline-load"),
            |vm| {
                vm.load_script("math", BASIC_SCRIPT).expect("load script");
                vm.shutdown().expect("shutdown vm");
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_hot_function_call(c: &mut Criterion) {
    let vm = new_vm("bench-hot-call");
    vm.load_script("math", BASIC_SCRIPT).expect("load script");

    c.bench_function("hot_function_call", |b| {
        b.iter(|| {
            black_box(
                vm.call_function("math", "sum", vec![json!({ "left": 20, "right": 22 })])
                    .expect("call function"),
            )
        });
    });

    vm.shutdown().expect("shutdown vm");
}

fn bench_event_routing(c: &mut Criterion) {
    let vm = new_vm("bench-event-routing");
    vm.load_script("listener", EVENT_LISTENER_SCRIPT)
        .expect("load listener");

    c.bench_function("hot_event_routing", |b| {
        b.iter(|| {
            black_box(
                vm.emit("score.update", json!({ "combo": 7 }))
                    .expect("emit event"),
            )
        });
    });

    vm.shutdown().expect("shutdown vm");
}

fn bench_sdk_generation(c: &mut Criterion) {
    let vm = new_vm("bench-sdk-generation");
    vm.registry()
        .function::<BenchFindUser>()
        .and_then(|registry| registry.function::<BenchCreateInvoice>())
        .and_then(|registry| registry.callback::<BenchScoreUpdate>())
        .expect("register host contracts");

    c.bench_function("sdk_generation", |b| {
        b.iter(|| {
            let types = vm.registry().types().expect("render types");
            let sdk = vm.registry().sdk().expect("render sdk");
            black_box((types, sdk));
        });
    });

    vm.shutdown().expect("shutdown vm");
}

fn bench_many_small_scripts_memory_shape(c: &mut Criterion) {
    c.bench_function("mount_400_small_scripts_memory_shape", |b| {
        b.iter_batched(
            || new_vm_with_capacity("bench-400-small-scripts", 2, 512),
            |vm| {
                load_many_small_scripts(&vm, MANY_SMALL_SCRIPT_COUNT);
                let stats = vm.stats().expect("collect stats");
                assert_eq!(stats.memory.active_scripts, MANY_SMALL_SCRIPT_COUNT);
                black_box(stats.process_memory);
                vm.shutdown().expect("shutdown vm");
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn report_many_small_scripts_process_memory_curve() {
    eprintln!();
    eprintln!("memory_rss_small_scripts_curve:");
    for &script_count in MEMORY_REPORT_SCRIPT_COUNTS {
        run_isolated_process_memory_report(script_count);
    }
    eprintln!();
}

fn run_isolated_process_memory_report(script_count: usize) {
    let current_exe = std::env::current_exe().expect("resolve current benchmark executable");
    let status = Command::new(current_exe)
        .env(MEMORY_REPORT_CHILD_ENV, script_count.to_string())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .expect("run isolated memory benchmark process");
    assert!(
        status.success(),
        "isolated memory benchmark failed for {script_count} scripts: {status}"
    );
}

fn memory_report_child_script_count() -> Option<usize> {
    std::env::var(MEMORY_REPORT_CHILD_ENV)
        .ok()?
        .parse::<usize>()
        .ok()
        .filter(|script_count| *script_count > 0)
}

fn report_many_small_scripts_process_memory(script_count: usize) {
    let before = read_current_process_memory();
    let label = format!("bench-rss-{script_count}-small-scripts");
    let vm = new_vm_for_memory_report(&label, script_count);
    let after_vm_start = read_current_process_memory();

    load_many_small_scripts(&vm, script_count);

    let stats = vm.stats().expect("collect stats");
    assert_eq!(stats.memory.active_scripts, script_count);
    let quickjs_memory = quickjs_memory_report(&stats);
    let after_mount = stats.process_memory.or_else(read_current_process_memory);

    vm.shutdown().expect("shutdown vm");
    let after_shutdown = read_current_process_memory();
    drop(vm);
    let after_drop = read_current_process_memory();

    let report = ProcessMemoryReport {
        script_count,
        worker_threads: MEMORY_REPORT_WORKER_THREADS,
        memory_limit_bytes: MEMORY_REPORT_MEMORY_LIMIT_BYTES,
        before,
        after_vm_start,
        after_mount,
        after_shutdown,
        after_drop,
        quickjs_memory,
    };
    print_process_memory_report(&report);
}

fn max_scripts_per_worker_for(script_count: usize) -> usize {
    script_count
        .div_ceil(MEMORY_REPORT_WORKER_THREADS)
        .saturating_add(32)
        .max(64)
}

fn load_many_small_scripts(vm: &TsVm, script_count: usize) {
    for index in 0..script_count {
        vm.load_script(format!("script-{index}"), BASIC_SCRIPT)
            .expect("load small script");
    }
}

#[derive(Debug, Clone, Copy)]
struct ProcessMemoryReport {
    script_count: usize,
    worker_threads: usize,
    memory_limit_bytes: usize,
    before: Option<VmProcessMemoryStats>,
    after_vm_start: Option<VmProcessMemoryStats>,
    after_mount: Option<VmProcessMemoryStats>,
    after_shutdown: Option<VmProcessMemoryStats>,
    after_drop: Option<VmProcessMemoryStats>,
    quickjs_memory: QuickJsMemoryReport,
}

#[derive(Debug, Clone, Copy)]
struct QuickJsMemoryReport {
    sync_used_bytes: u64,
    async_used_bytes: u64,
    limit_bytes: u64,
}

fn print_process_memory_report(report: &ProcessMemoryReport) {
    eprintln!();
    eprintln!("memory_rss_{}_small_scripts:", report.script_count);
    eprintln!(
        "  config: workers={}, quickjs_memory_limit_per_worker={:.2} MiB",
        report.worker_threads,
        bytes_to_mib(report.memory_limit_bytes as u64)
    );
    print_process_memory_snapshot("before", report.before);
    print_process_memory_snapshot("after_vm_start", report.after_vm_start);
    print_process_memory_snapshot("after_mount", report.after_mount);
    print_process_memory_snapshot("after_shutdown", report.after_shutdown);
    print_process_memory_snapshot("after_drop", report.after_drop);
    print_process_memory_delta("vm_start_delta", report.before, report.after_vm_start);
    print_process_memory_delta("mount_delta", report.after_vm_start, report.after_mount);
    print_process_memory_delta("total_delta", report.before, report.after_mount);
    print_quickjs_memory_report(report.quickjs_memory);
    print_per_script_delta(
        report.script_count,
        report.after_vm_start,
        report.after_mount,
    );
    eprintln!();
}

fn quickjs_memory_report(stats: &VmStats) -> QuickJsMemoryReport {
    let mut report = QuickJsMemoryReport {
        sync_used_bytes: 0,
        async_used_bytes: 0,
        limit_bytes: 0,
    };

    for worker in &stats.workers {
        add_quickjs_memory(
            &mut report.sync_used_bytes,
            &mut report.limit_bytes,
            worker.sync_quickjs_memory,
        );
        add_quickjs_memory(
            &mut report.async_used_bytes,
            &mut report.limit_bytes,
            worker.async_quickjs_memory,
        );
    }

    report
}

fn add_quickjs_memory(
    used_total: &mut u64,
    limit_total: &mut u64,
    memory: Option<VmQuickJsMemoryStats>,
) {
    if let Some(memory) = memory {
        *used_total = used_total.saturating_add(memory.memory_used_bytes);
        *limit_total = limit_total.saturating_add(memory.malloc_limit_bytes);
    }
}

fn print_quickjs_memory_report(report: QuickJsMemoryReport) {
    eprintln!(
        "  quickjs_sync_used: {:.2} MiB",
        bytes_to_mib(report.sync_used_bytes)
    );
    eprintln!(
        "  quickjs_async_used: {:.2} MiB",
        bytes_to_mib(report.async_used_bytes)
    );
    eprintln!(
        "  quickjs_total_pressure: {}",
        format_pressure(
            report.sync_used_bytes + report.async_used_bytes,
            report.limit_bytes
        )
    );
}

fn print_process_memory_snapshot(label: &str, snapshot: Option<VmProcessMemoryStats>) {
    match snapshot {
        Some(stats) => {
            eprintln!(
                "  {label}: rss={:.2} MiB, virtual={:.2} MiB",
                bytes_to_mib(stats.resident_bytes),
                bytes_to_mib(stats.virtual_bytes)
            );
        }
        None => {
            eprintln!("  {label}: unavailable");
        }
    }
}

fn print_process_memory_delta(
    label: &str,
    before: Option<VmProcessMemoryStats>,
    after: Option<VmProcessMemoryStats>,
) {
    match (before, after) {
        (Some(before), Some(after)) => {
            let resident_delta = byte_delta(after.resident_bytes, before.resident_bytes);
            let virtual_delta = byte_delta(after.virtual_bytes, before.virtual_bytes);
            eprintln!(
                "  {label}: rss={}, virtual={}",
                format_mib_delta(resident_delta),
                format_mib_delta(virtual_delta)
            );
        }
        _ => {
            eprintln!("  {label}: unavailable");
        }
    }
}

fn print_per_script_delta(
    script_count: usize,
    before: Option<VmProcessMemoryStats>,
    after: Option<VmProcessMemoryStats>,
) {
    match (script_count, before, after) {
        (0, _, _) | (_, None, _) | (_, _, None) => {
            eprintln!("  mount_rss_per_script: unavailable");
        }
        (script_count, Some(before), Some(after)) => {
            let resident_delta = byte_delta(after.resident_bytes, before.resident_bytes);
            eprintln!(
                "  mount_rss_per_script: {}",
                format_kib_per_item(resident_delta, script_count)
            );
        }
    }
}

fn read_current_process_memory() -> Option<VmProcessMemoryStats> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    parse_process_status_memory(&status)
}

fn parse_process_status_memory(status: &str) -> Option<VmProcessMemoryStats> {
    Some(VmProcessMemoryStats {
        resident_bytes: parse_status_kib_value(status, "VmRSS:")?,
        virtual_bytes: parse_status_kib_value(status, "VmSize:")?,
    })
}

fn parse_status_kib_value(status: &str, key: &str) -> Option<u64> {
    let line = status.lines().find(|line| line.starts_with(key))?;
    let value_kib = line.split_whitespace().nth(1)?.parse::<u64>().ok()?;
    Some(value_kib * 1024)
}

fn byte_delta(after: u64, before: u64) -> i128 {
    i128::from(after) - i128::from(before)
}

fn bytes_to_mib(bytes: u64) -> f64 {
    bytes as f64 / BYTES_PER_MIB
}

fn format_mib_delta(delta: i128) -> String {
    format_signed_float(delta as f64 / BYTES_PER_MIB, "MiB")
}

fn format_kib_per_item(delta: i128, item_count: usize) -> String {
    let per_item = delta as f64 / item_count as f64 / BYTES_PER_KIB;
    format_signed_float(per_item, "KiB")
}

fn format_pressure(used_bytes: u64, limit_bytes: u64) -> String {
    if limit_bytes == 0 {
        return String::from("unlimited");
    }

    format!("{:.2}%", used_bytes as f64 / limit_bytes as f64 * 100.0)
}

fn format_signed_float(value: f64, unit: &str) -> String {
    format!("{value:+.2} {unit}")
}

struct BenchFindUser;
struct BenchCreateInvoice;
struct BenchScoreUpdate;

impl HostContract for BenchFindUser {
    const NAME: &'static str = "user.find";

    fn schema() -> Schema {
        Schema::typed("FindUserInput", TsType::Number)
    }

    fn kind() -> HostContractKind {
        HostContractKind::Function
    }
}

impl HostFunction for BenchFindUser {
    type Input = u64;
    type Output = String;

    fn output_schema() -> Schema {
        Schema::typed("FindUserOutput", TsType::String)
    }

    fn call(input: Self::Input) -> Result<Self::Output, VmError> {
        Ok(format!("user-{input}"))
    }
}

impl HostContract for BenchCreateInvoice {
    const NAME: &'static str = "billing.invoice.create";

    fn schema() -> Schema {
        Schema::typed(
            "CreateInvoiceInput",
            TsType::Object(vec![
                TsField::required("accountId", TsType::String),
                TsField::required("total", TsType::Number),
            ]),
        )
    }

    fn kind() -> HostContractKind {
        HostContractKind::Function
    }
}

impl HostFunction for BenchCreateInvoice {
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    fn output_schema() -> Schema {
        Schema::typed(
            "CreateInvoiceOutput",
            TsType::Object(vec![
                TsField::required("id", TsType::String),
                TsField::required("accepted", TsType::Boolean),
            ]),
        )
    }

    fn call(_input: Self::Input) -> Result<Self::Output, VmError> {
        Ok(json!({ "id": "invoice_1", "accepted": true }))
    }
}

impl HostContract for BenchScoreUpdate {
    const NAME: &'static str = "score.update";

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

impl HostCallback for BenchScoreUpdate {
    type Payload = serde_json::Value;

    fn delivery() -> DeliveryMode {
        DeliveryMode::Broadcast
    }
}

fn new_vm(label: &str) -> TsVm {
    new_vm_with_capacity(label, 1, 64)
}

fn new_vm_with_capacity(label: &str, worker_threads: usize, max_scripts_per_worker: usize) -> TsVm {
    TsVm::new(VmOptions {
        worker_threads,
        cache_dir: unique_cache_dir(label),
        max_scripts_per_worker,
        ..VmOptions::default()
    })
    .expect("create vm")
}

fn new_vm_for_memory_report(label: &str, script_count: usize) -> TsVm {
    TsVm::new(VmOptions {
        worker_threads: MEMORY_REPORT_WORKER_THREADS,
        cache_dir: unique_cache_dir(label),
        max_scripts_per_worker: max_scripts_per_worker_for(script_count),
        memory_limit_bytes: MEMORY_REPORT_MEMORY_LIMIT_BYTES,
        ..VmOptions::default()
    })
    .expect("create memory report vm")
}

fn unique_cache_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_nanos();
    std::env::temp_dir().join(format!("ts-embed-vm-{label}-{nanos}"))
}

criterion_group!(benches, runtime_benchmarks);
criterion_main!(benches);
