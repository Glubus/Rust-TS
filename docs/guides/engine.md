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
  message.

## Scripts And Projects

- `load_script(id, source)` loads one TypeScript source string. It may import the
  registered host modules, not other files.
- `load_project(id, entry_path)` loads a multi-file project from its entry file:
  the static ESM graph is resolved from disk (relative imports, `index` modules,
  `tsconfig.json` `paths` and `baseUrl`, packages in the project's
  `node_modules`).
- Dynamic `import()` is rejected at load in both cases.
- `unload_script(id)` removes a script and releases its modules.

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
stays loaded. A successful reload starts from fresh script state.

## Execution Budget

Every load, call and emit runs under `VmOptions::execution_timeout` (5 seconds by
default), including the Promise jobs it queues. For a load, the budget starts when
the script's top-level code runs, after transpilation. JavaScript still running when
it expires is interrupted and the operation fails with `VmError::Execution`; the
engine stays usable. The budget is cooperative: it cannot stop a Rust host function
that blocks.

## Transpilation Cache

`VmOptions::cache_dir` is `None` by default: every load transpiles the TypeScript
in memory. Set it to a directory to keep the transpiled JavaScript on disk and
reuse it across reloads and runs:

```rust
use rustts::{Engine, VmOptions};

let engine = Engine::new(&VmOptions {
    cache_dir: Some("target/rustts-cache".into()),
    ..VmOptions::default()
})?;
```

The directory is created if it does not exist. An artifact is keyed by the source
(the whole resolved graph for a project), the compiler, resolver and runtime
versions, and the registered host contracts, so changing any of them transpiles
again. See [Runtime Guarantees](runtime-guarantees.md#transpilation-cache) for
integrity and atomic writes.

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
| One call making 1,000 host calls | 24.7 µs | 62.8 µs | 74.9 µs |
| Pure compute, `fib(20)` | 294 µs | 968 µs | 896 µs |
| Event to one handler | 181 ns | 135 ns | 250 ns |
| Reload a small script | 8.7 µs | 72 µs | 156 µs |

QuickJS itself is about 3× slower than Lua on pure compute; `Engine` adds tens of
nanoseconds per crossing on top of it. The `Engine` reload runs with the default
options, without a disk cache, so it transpiles the TypeScript every time.
Run `cargo run --release --example native_roundtrip` for a quick check of the same
comparison.
