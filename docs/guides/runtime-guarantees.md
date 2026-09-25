# Runtime Guarantees And Limits

## TypeScript and contracts

OXC transpiles TypeScript and checks syntax; loading a script does **not** perform
TypeScript type checking. Run `tsc --noEmit` on author projects. Rust contract
schemas generate SDK declarations; runtime schema validation is independently
controlled by `contract_validation` and `unknown_field_validation`.

Repository SDK tests use the pinned TypeScript package installed by `npm ci`.
CI (or `RUSTTS_REQUIRE_TSC=1`) fails if that compiler is missing. Local Rust-only
development can skip these checks when the package is absent, with a diagnostic.

## Threads and isolation

`Engine` runs every load, call and emit on the thread that owns it, one at a time.
RustTS starts no threads and runs no background work: between two calls, no script
code runs.

Each script owns a separate QuickJS context and ESM module graph, so scripts do not
share globals. Scripts of the same `Engine` share its QuickJS runtime: memory limit,
stack limit and garbage collector. This is not process isolation or a per-script
memory quota. Host functions are capabilities granted to every script of that
engine.

## Execution budget

`VmOptions::execution_timeout` defaults to 5 seconds. It limits the wall time of
each load, call and emit, starting when JavaScript starts running: it excludes
transpilation and module resolution, and includes every Promise job the operation
queues. One `emit` has one budget for all the scripts it reaches.

QuickJS's interrupt handler stops JavaScript still running when the budget
expires; the operation fails with `VmError::Execution` and the engine stays
usable. Limits are cooperative, not real-time scheduling guarantees. A Rust host
function cannot be preempted: it must return, have its own I/O timeouts, and avoid
unbounded blocking. For hostile native extensions, use a separate process.
JavaScript state mutations and host side effects made before the interruption
remain.

## Promises

Promise jobs queued by an operation run before it returns, for every script.
An `async` export resolves before `call` returns its value. A Promise that no
script job can settle fails the call instead of waiting. A Promise rejection that
no handler caught by the end of the operation fails it, even when the operation's
own work succeeded.

## Reload

A new script context and module graph are prepared before replacing the loaded
script. A failure at any step, from transpilation to top-level code, leaves the
previous version loaded with its state; the partially prepared graph is removed.
A successful reload resets script-local state.

This guarantees replacement of the script, not rollback of side effects: host
calls made during a failed initialization may already have changed the host.
Preparing both versions temporarily needs memory for both.

## Transpilation cache

The cache is off by default (`VmOptions::cache_dir: None`). When enabled, cache
writes use temporary files in the cache directory and atomic replacement. A
checksum header detects truncated or modified contents; missing, older-format or
corrupt artifacts are rebuilt. This checksum is not authentication: keep cache
directories writable only by trusted users. Engines sharing a cache directory do
not see partially written artifacts. Atomic replacement is not a guarantee of
durability against every filesystem or power-loss failure.

Artifact keys include the source (for a project: every module source, the
resolved import graph, the `package.json` files enclosing its modules and the
project's lockfiles), the compiler, resolver and runtime versions, and the
registered host contracts. Project loading discovers sources and resolves the
graph before looking up the cache: a hit skips transpilation, but still pays for
filesystem reads, import parsing and resolution, and module evaluation. Changing
one module transpiles the whole project again.
