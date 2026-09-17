# Operational measurements — 2026-09-17

Local Windows run with Rust 1.96.0, release profile, default features, two workers.
Executed with `cargo run --release --example operational_probe` and then
`./scripts/measure-probe.ps1` for an idle CPU/working-set sample.
These are measurements of the corrected implementation, not a before/after benchmark.

| Scenario | Result |
| --- | --- |
| 31-module project, empty artifact cache, 20 samples | p50 6.249 ms; p99 7.017 ms |
| Same project, populated cache, 20 samples | p50 5.077 ms; p99 6.140 ms |
| Saturation: 16 callers, 100 attempts each, queue capacity 1 | 200 accepted, 1,400 rejected |
| Accepted calls under saturation | p50 6.011 ms; p99 6.117 ms |
| Rejected calls under saturation | p50 0.0031 ms; p99 0.0588 ms |
| 32 serial small-script loads | 7.237 ms total |
| 32 concurrent small-script loads | 9.216 ms total |
| QuickJS memory, 65 mounted scripts, before/after 3 idle seconds | 3,755,465 bytes both times |
| Process working set during idle sample | 12,509,184 bytes before and after |
| Process CPU during 1,510.9 ms of idle wall time | 62.5 ms, approximately 4.14% of one core |

The warm-load median is about 19% lower on this small fixture. The regression test
also checks the compiler's invocation counter: a cache hit performs no transpilation.
Filesystem reads, import parsing, resolution and mounting still run on a hit.
Cold here means an empty RustTS artifact cache, not an empty OS filesystem cache.

The saturation fixture deliberately overloads a single script's worker. Accepted
and rejected latencies are reported separately so fast failures do not disguise
the latency of completed work. The acceptance ratio is specific to this burst and
OS scheduling, not a production throughput claim.

Concurrent loads did not improve wall time in this run. Thread startup, scheduling,
cache work and the global load lock all contribute; this probe does not isolate the
lock's individual cost. Another run concurrent with repository checks took 54.8 ms
for the concurrent batch, illustrating why isolated repetition matters.

Memory stayed stable over the short idle interval. CPU usage was not zero: the
current idle loop still drains jobs and runs GC periodically. This supports profiling
and comparing an adaptive GC policy next, but does not establish that GC alone caused
all the measured CPU. Windows process CPU accounting is quantized and a 1.5-second
sample is too short for precise low-utilization comparisons.

The global load lock and GC policy have deliberately not been redesigned as part
of these correctness fixes. The probe provides a repeatable baseline for that work.
