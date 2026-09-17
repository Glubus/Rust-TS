# Runtime guarantees and limits

## TypeScript and contracts

OXC transpiles TypeScript and checks syntax; loading a script does **not** perform
TypeScript type checking. Run `tsc --noEmit` on author projects. Rust contract
schemas generate SDK declarations; runtime schema validation is independently
controlled by `contract_validation` and `unknown_field_validation`.

Repository SDK tests use the pinned TypeScript package installed by `npm ci`.
CI (or `RUSTTS_REQUIRE_TSC=1`) fails if that compiler is missing. Local Rust-only
development can skip these checks when the package is absent, with a diagnostic.

## Execution and isolation

Each script owns a separate QuickJS context and runtime-scoped ESM graph. Scripts
on the same worker share its thread, heap limit, stack configuration, GC and host
registry. This is not process isolation or a per-script memory quota. Host APIs
are capabilities granted to every script using that registry.

`VmOptions::execution_timeout` defaults to 5 seconds. It limits wall time starting
when a worker begins JavaScript work, excluding compilation and queue wait.
QuickJS's interrupt handler stops CPU loops during loading, calls, event delivery
and synchronous idle jobs. Promise-aware operations also have an executor-independent
timer, so a Promise that never settles cannot keep a managed operation waiting
forever. Interrupt errors surface as `VmError::Execution`; expiration while waiting
for a Promise surfaces as `VmError::ExecutionTimeout`.

Synchronous event delivery uses one budget for the worker command; async delivery
uses one budget per targeted script. Limits are cooperative, not real-time scheduling
guarantees. A Rust host handler cannot be preempted: it must return, have its own I/O
timeouts, and avoid unbounded blocking. For hostile native extensions, use a separate
process. JavaScript state mutations and host side effects before interruption remain.

Calls are serialized within a worker. The low-level async runtime also serializes
its public operations to keep the shared interrupt deadline consistent.

## Reentrancy and shutdown

The registry lock is released before a host handler runs. A handler may register
another contract without recursively acquiring that lock. However, synchronously
calling back into the same worker through the manager is unsupported: that worker
is already executing the handler. Schedule follow-up work after the handler returns.
Cross-worker dependency cycles can deadlock too. Do not call `shutdown()` from a
handler that is running in the VM being shut down.

Shutdown requests use a separate shared stop signal, independent of queue capacity.
They interrupt current JavaScript, reject new dispatches and abandon queued work;
they do not drain all work gracefully. Disconnected replies return `WorkerOffline`.
Shutdown tracks requested/completed states, joins both execution lanes, and emits
the shutdown event once. Each lane can wait up to `shutdown_timeout` (default 5 seconds).
If a Rust handler is still running, `ShutdownTimeout` is returned and shutdown can
be retried. Rust threads are not forcibly terminated. Dropping the last VM handle
attempts this same bounded shutdown; external host work may outlive it.

## Reload and cancellation

A new script context and module graph are prepared before replacing the mounted
instance. Initialization failure removes the temporary graph and preserves the old
instance, state and registry entry. Successful reload resets script-local state.
This guarantees replacement of the VM instance, not rollback of side effects:
host calls made during failed initialization may already have changed the host.
Preparing both versions temporarily needs memory for both.

Dropping a Tokio façade future does not cancel its `spawn_blocking` task. Dropping
a managed Promise-call future likewise does not retract a command already queued.
Execution limits and shutdown still apply on the worker. Timing out a Promise wait
does not undo mutations or necessarily cancel a host future spawned on another
executor. Do not assume cancellation means the operation had no effect.

## Events and cache

Each lifecycle subscription retains at most `event_queue_capacity` events
(default 256). A full queue drops the **new** event without blocking execution;
`VmSubscription::dropped_events()` reports the cumulative loss. Already queued
events retain FIFO order. This bounds the number of events, not each payload's size.
Shutdown notifications can be dropped too. Script callback delivery is separate
from this observational event bus.

Cache writes use temporary files in the cache directory and atomic replacement.
An integrity header detects truncated or modified contents; missing, legacy or
corrupt artifacts are rebuilt. This checksum is not authentication: keep cache
directories writable only by trusted users. Sharing a cache between VM instances
does not expose partially written artifacts. Atomic replacement is not a guarantee
of durability against every filesystem or power-loss failure.

Project loading discovers sources and resolves the graph before looking up the
cache. A hit skips transpilation, but still pays for filesystem reads, import
parsing, resolution and runtime mounting. Compilation is cached for the whole
project; changing one module currently recompiles that project.

## Measuring changes

Run `cargo run --release --example operational_probe` for JSON measurements of
31-module cold/warm loads, accepted/rejected latency under saturation, serial versus
concurrent loads, and QuickJS memory before/after three idle seconds. The probe
prints its PID before the idle interval for external CPU/RSS sampling. On Windows,
use `Get-Process -Id <pid>`; built-in process-memory snapshots currently require Linux.
After building the release example on Windows, `./scripts/measure-probe.ps1`
runs it and samples process CPU time and working set inside the announced idle phase.

Cold means an empty RustTS artifact cache, not a cold OS filesystem cache. The
global synchronous load lock still serializes loads; idle maintenance still runs
GC. Use repeated measurements on an otherwise idle machine before changing these
policies. Do not interpret one short probe as a throughput or memory guarantee.

The first measured results are recorded in the [operational baseline](../operational-measurements.md).
