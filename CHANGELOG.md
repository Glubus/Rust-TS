# Changelog

## 0.2.0 — 2026-09-17

### Migration

- `VmOptions` adds `execution_timeout` (5 seconds), `shutdown_timeout`
  (5 seconds per worker lane), and `event_queue_capacity` (256 events).
  Use `..VmOptions::default()` when constructing options, or set all three fields.
- Exhaustive matches on `VmError` must handle `ExecutionTimeout` and `ShutdownTimeout`.
- Shutdown interrupts JavaScript and abandons queued work instead of draining it.
- Lifecycle subscriptions drop new events when full; inspect `dropped_events()`.
- Legacy cache artifacts are automatically rebuilt with integrity headers.

### Fixes and improvements

- Interrupt runaway JavaScript and bound asynchronous Promise waits.
- Preserve mounted scripts and module graphs when replacement initialization fails.
- Signal shutdown independently of worker queue capacity and allow retries.
- Release the host registry lock before invoking JSON bridge handlers.
- Bound lifecycle event subscriptions and expose dropped-event counts.
- Consult project caches before transpilation, atomically replace artifacts, and
  rebuild corrupt entries; retry transient Windows replacement conflicts.
- Use the default platform linker instead of requiring `/usr/bin/mold` on Linux.
- Require pinned TypeScript SDK validation in Linux/Windows CI.
- Add regression tests, runtime guarantee documentation and operational measurements.

`rustts` and `rustts_macros` now share version 0.2.0. Native Rust host handlers
remain cooperative: JavaScript execution budgets cannot preempt blocking Rust code.
