# Run Scripts With `Engine`

`Engine` is the RustTS runtime. It runs QuickJS on the thread that creates it:
calls between Rust and TypeScript are direct function calls with native value
conversion, with no message to another thread, no generated source and no JSON
text.

It fits the way a game or an application already works: its main loop, simulation
step or command handler calls into scripts when it needs them. RustTS spawns no
threads and has no event loop; nothing runs unless the host calls `Engine`.

## Load And Call

```rust
use rustts::{Engine, VmOptions};

let mut engine = Engine::new(&VmOptions::default())?;
engine
    .registry()
    .typed_function::<FindUser>()?
    .typed_callback::<UserFound>()?;

engine.load_script("rules", include_str!("rules.ts"))?;

let total: f64 = engine.call("rules", "score", (3, 4))?;
let report: Report = engine.call("rules", "report", (&input,))?;
engine.emit("user.found", &UserFoundPayload { user_id: 7 })?;
```

- Register host contracts before loading the scripts that use them: host functions
  are installed in a script when it loads.
- `call` takes its arguments as a tuple (`()` for none), a `Vec` or a slice of
  values implementing [`JsEncode`](type-mapping.md), and decodes the result with
  `JsDecode`. `engine.call::<serde_json::Value>(id, name, vec![json])` keeps a
  JSON-shaped API.
- Calling a script that is not loaded fails with `VmError::ScriptNotFound`; calling
  an export the script does not have fails with `VmError::FunctionNotFound`. An
  exception thrown by the script is returned as `VmError::Execution` with its
  message and stack, located in the TypeScript source (see
  [Error Locations](#error-locations)).

## Scripts And Projects

- `load_script(id, source)` loads one TypeScript source string. It may import the
  registered host modules, not other files.
- `load_project(id, entry_path)` loads a multi-file project from its entry file:
  the static ESM graph is resolved from disk (relative imports, `index` modules,
  `tsconfig.json` `paths` and `baseUrl`, packages in the project's
  `node_modules`).
- Dynamic `import()` is rejected at load in both cases.
- `unload_script(id)` removes a script and releases its modules, after running its
  `ctx.hot.dispose` callbacks (see [Keep State Across Reloads](#keep-state-across-reloads)).

Each script runs in its own QuickJS context: scripts do not share globals. All
scripts of one `Engine` share its QuickJS runtime, and so its memory limit, stack
limit and garbage collector. See
[Load Scripts And Projects](load-scripts-and-projects.md) for details.

## Events

`emit(event, &payload)` delivers the payload to every handler registered with
`ctx.on(event, handler)`, script by script in load order, and returns how many
scripts had at least one handler.

- The payload is encoded once per script that has handlers, and shared by all of
  that script's handlers.
- A reload keeps the script's place in the delivery order.
- A handler that throws does not stop the others: every handler runs, then `emit`
  returns the first error.

## Promises

`Engine` has no event loop, so it runs Promise jobs itself: the jobs a load, call or
emit queues (`then` callbacks, `await` continuations, `queueMicrotask`) run before
it returns, within the same execution budget.

- `call` on an `async` export returns the resolved value, or fails with the
  rejection reason.
- A Promise that no script job can settle, such as one waiting on a timer or on a
  host, fails the call: `Engine` has no timers and host functions are synchronous.
- A Promise rejection that no handler caught by the end of the operation fails it
  with `unhandled promise rejection: …`, as Node does. Attach a `catch` to promises
  you do not await.

## Error Locations

Errors point at the TypeScript you wrote. Each script frame of a
`VmError::Execution` stack names its file and its TypeScript line and column:

```text
javascript execution failed: division by zero
    at divide (lib/math.ts:8:15)
    at run (main.ts:7:17)
```

- A project module is named by its path relative to the project root (the
  `tsconfig.json` directory, else the entry file's), with `/` separators; an inline
  script is `<id>.ts`.
- Lines and columns start at 1; columns count UTF-16 code units, as editors and
  `tsc` do. A frame points where QuickJS places it: the callee or the last argument
  of a call, the object of a failed property access.
- Errors from top-level code during a load, calls, `async` exports, event handlers,
  Promise jobs and unhandled rejections are all located. Frames outside script
  modules, such as `native` ones, stay as they are.
- A syntax error fails the load with `VmError::Transpile`, one diagnostic per line
  as `path:line:column: message`, followed by its labels and help, indented.
- Each module's source map is built when it is transpiled, kept with it in memory
  and in the disk cache, and only read when an error is reported: successful calls
  and emits do no extra work.

## Reaching Host Functions From Scripts

A script reaches every registered host function in three ways, all ending in the
same native function:

- ESM import of the contract's module: `import { user } from "app";` (see
  [`IMPORT_MODULE`](generate-sdk-files.md#import-modules))
- the namespaced global, without an import: `user.find({ userId: 7 })`
- the generated SDK (`registry().sdk()`), which calls `__host.callValue`; an inline
  script can include the SDK source followed by its own code, and a project can
  import the generated SDK file

## Reload

Loading a script or project again with the same id replaces it. The new version is
transpiled, resolved and initialized first; if any of that fails (syntax error,
resolution error, exception or timeout in top-level code), the previous version
stays loaded. A successful reload starts from fresh script state, except for what
the script hands over through `ctx.hot` (see
[Keep State Across Reloads](#keep-state-across-reloads)).

## Hot Reload

`reload_changed()` reloads every project whose files changed since it was loaded,
in load order, and returns a `ReloadReport` with the reloaded ids, the failed ones,
and the reloaded ones whose previous version's `ctx.hot.dispose` threw. Call it from
your loop; it starts no thread:

```rust
// For example once per second during development.
let report = engine.reload_changed();
for (id, error) in &report.failed {
    eprintln!("{id} kept its previous version: {error}");
}
for (id, error) in &report.dispose_failed {
    eprintln!("{id} reloaded, but its previous version failed to clean up: {error}");
}
```

- A project is checked through the size and modification time of its module
  files, the directories from each module up to the project root, its
  `tsconfig.json` and the `package.json` files its imports resolved through. When
  nothing changed, the check reads no file.
- A reload only reads, parses and transpiles the modules whose files changed. While
  no file was added or removed and `tsconfig.json` / `package.json` are unchanged,
  the imports of unchanged modules are not resolved again either.
- A failed reload keeps the previous version running and is reported once; the
  project is retried after its next change.
- Inline scripts (`load_script`) have no files and are never reloaded here.
- Changes outside the watched paths (for example a new file in a `paths` fallback
  directory that holds no loaded module) need an explicit `load_project`.

### Keep State Across Reloads

A reload starts the script from fresh module state. To keep some of it, the script
hands it over through `ctx.hot`, which the generated declarations and SDK type:

```ts
type Saved = { score: number };

let score = (ctx.hot.data as Saved | undefined)?.score ?? 0;
const timer = scheduler.every(1000, "tick");

ctx.hot.save(() => ({ score }));
ctx.hot.dispose(() => scheduler.cancel(timer));

export function add(points: number): number {
  return (score += points);
}
```

- `ctx.hot.data` is the value the previous version's `save` callback returned. It
  is set before any of the new version's code runs, so top-level code can read it;
  it is `undefined` on a first load, after `unload_script` and when the previous
  version registered no `save`.
- `ctx.hot.save(fn)` registers the callback run on the old version when a reload
  replaces it. It runs before the new version loads, so it must only read state:
  if the new version then fails to load, the old one keeps running as if nothing
  happened. The last registration wins. A `save` that throws fails the reload, and
  the old version keeps running.
- `ctx.hot.dispose(fn)` registers a cleanup run on the old version only once the
  new version has loaded, and by `unload_script`. Every registered cleanup runs, in
  registration order, even after one throws. A reload whose new version fails to
  load does not dispose the old one.
- A throwing `dispose` does not undo the reload: the new version stays loaded and
  the load call (`load_script`, `load_project`) returns `VmError::Execution` saying
  the previous version's `ctx.hot.dispose` failed and the new version is loaded.
  `reload_changed` lists that script both in `reloaded` and in `dispose_failed`.
  `unload_script` returns the error too, and the script is unloaded all the same.
- `save` and `dispose` run under the execution budget, with their Promise jobs,
  like a call. They are synchronous: a Promise returned by `save` becomes
  `ctx.hot.data` as is.
- Scripts share one QuickJS runtime, so `data` is the very object `save` returned,
  not a copy: nothing is serialized. Prefer plain data (objects, arrays, numbers,
  strings). A function or class instance from the old version keeps the old
  version's context in memory while it is referenced, and `instanceof` against the
  new version's classes is false for it.
- Dropping the `Engine` runs no `dispose` callback.

## Execution Budget

Every load, call and emit runs under `VmOptions::execution_timeout` (5 seconds by
default), including the Promise jobs it queues. For a load, the budget starts when
the script's top-level code runs, after transpilation. JavaScript still running when
it expires is interrupted and the operation fails with `VmError::Execution`; the
engine stays usable. The budget is cooperative: it cannot stop a Rust host function
that blocks.

To stop a script earlier, for example from a UI "stop" button or a watchdog thread,
take an `InterruptHandle` before handing work to the engine's thread. It is `Send`
and `Clone`:

```rust
let handle = engine.interrupt_handle();
std::thread::spawn(move || {
    std::thread::sleep(std::time::Duration::from_millis(200));
    handle.interrupt();
});
// Fails with VmError::Interrupted if still running after 200 ms.
let result = engine.call::<()>("rules", "simulate", ());
```

`interrupt` stops the load, call or emit in progress; the engine stays usable. A
request made while nothing runs has no effect on the next operation.

## Transpilation Cache

`VmOptions::cache_dir` is `None` by default: every load transpiles the TypeScript
in memory. Set it to a directory to keep the transpiled JavaScript and its source
map on disk and reuse them across reloads and runs:

```rust
use rustts::{Engine, VmOptions};

let engine = Engine::new(&VmOptions {
    cache_dir: Some("target/rustts-cache".into()),
    ..VmOptions::default()
})?;
```

The directory is created if it does not exist. The cache holds one artifact per
module, keyed by the module source, its file extension, and the compiler and crate
versions: two projects sharing a file share its artifact, and editing one file of a
project transpiles that file only. Import resolution is never cached on disk; it
runs on every load. See
[Runtime Guarantees](runtime-guarantees.md#transpilation-cache) for integrity and
atomic writes.

Independently of the disk cache, an `Engine` remembers in memory the transpiled
modules of its loaded scripts, so reloads within one run never transpile an
unchanged module.

## Memory

`memory_stats()` returns the QuickJS counters for the whole engine as a
`MemoryStats`: allocated bytes (`malloc_size_bytes`), the limit
(`malloc_limit_bytes`, from `VmOptions::memory_limit_bytes`), bytes used by live
values (`memory_used_bytes`), and allocation, atom, string, object and function
counts. A call that exceeds the memory limit fails, and the engine stays usable.

## Threading

`Engine` cannot be sent to another thread: it lives on the thread that created it,
and so do its scripts. To use scripts from several threads, own the engine on one
dedicated thread and send it work over a channel:

```rust
use std::sync::mpsc;
use std::thread;

use rustts::{Engine, VmError, VmOptions};

type Job = Box<dyn FnOnce(&mut Engine) + Send>;

let (jobs, inbox) = mpsc::channel::<Job>();
thread::spawn(move || {
    let mut engine = Engine::new(&VmOptions::default()).expect("create engine");
    for job in inbox {
        job(&mut engine);
    }
});

let (reply, answer) = mpsc::channel::<Result<f64, VmError>>();
jobs.send(Box::new(move |engine| {
    let _ = reply.send(engine.call("rules", "score", (3, 4)));
}))
.expect("script thread is running");
let total = answer.recv().expect("script thread replied")?;
```

## Performance

`cargo bench --bench vs_lua` runs the same workloads on mlua (Lua 5.4), QuickJS
called directly through rquickjs, and `Engine`. Medians on one Windows machine,
rquickjs 0.14:

| Workload | mlua | QuickJS direct | `Engine` |
| --- | --- | --- | --- |
| Call `sum(a, b)` | 36 ns | 71 ns | 130 ns |
| Call with a 20-field object in and out | 2.76 µs | 2.46 µs | 2.65 µs |
| One call making 1,000 host calls¹ | 24.7 µs | 62.8 µs | 74.9 µs |
| Pure compute, `fib(20)` | 294 µs | 968 µs | 896 µs |
| Event to one handler | 181 ns | 135 ns | 250 ns |
| Reload a small script | 8.7 µs | 72 µs | 156 µs |

QuickJS itself is about 3× slower than Lua on pure compute; `Engine` adds tens of
nanoseconds per crossing on top of it. The `Engine` reload runs with the default
options, without a disk cache, so it transpiles the TypeScript every time.
Run `cargo run --release --example native_roundtrip` for a quick check of the same
comparison.

¹ Measured when the direct QuickJS script called a global `inc`, while the `Engine`
script calls `math.inc` from its host module. The bench now uses `math.inc` on both
sides; with the same member access, a host call through `Engine` costs the same as
through QuickJS directly (about 100 ns each on the development machine).
