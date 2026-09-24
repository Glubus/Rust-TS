# Run Scripts On Your Thread With `Engine`

`Engine` runs QuickJS on the thread that owns it. Calls between Rust and
TypeScript are direct function calls with native value conversion: no message to
another thread, no generated source, no JSON text.

Use it when your application already has a main loop that calls scripts, such as a
game loop, a simulation step or an editor command handler.

## `Engine` Or `RustTs`

| | `Engine` | `RustTs` |
| --- | --- | --- |
| Threads | Runs on the calling thread; cannot be sent to another thread | Worker threads; callable from any thread |
| Call cost | About the cost of calling QuickJS directly | Adds a thread hop and JSON conversion per call |
| Scripts | Inline TypeScript sources | Inline sources and multi-file projects |
| Host functions | Synchronous | Synchronous, blocking async and Promise-returning |
| Execution timeout | Yes (`execution_timeout`) | Yes |
| Disk cache, stats, lifecycle events | Not yet | Yes |

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

- Register host contracts before loading the scripts that import them.
- `call` takes its arguments as a tuple, a `Vec` or a slice of values implementing
  [`JsEncode`](type-mapping.md), and decodes the result with `JsDecode`.
  `engine.call::<serde_json::Value>(id, name, vec![json])` keeps a JSON-shaped API.
- `emit` encodes the payload once per script that has handlers for the event and
  returns how many scripts received it.

## Reload

Calling `load_script` again with the same id replaces the script. The new version
is compiled and initialized first; if that fails (syntax error, exception or
timeout in top-level code), the previous version stays loaded. A successful reload
starts from fresh script state.

`unload_script(id)` removes a script and its modules.

## Execution Budget

Every load, call and emit runs under `VmOptions::execution_timeout` (5 seconds by
default). JavaScript still running when it expires is interrupted and the
operation fails with `VmError::Execution`; the engine stays usable. The budget is
cooperative: it cannot stop a Rust host function that blocks.

## Performance

`cargo bench --bench vs_lua` runs the same workloads on mlua (Lua 5.4), QuickJS
called directly through rquickjs, `Engine` and `RustTs`. Medians on one Windows
machine, rquickjs 0.14:

| Workload | mlua | QuickJS direct | `Engine` | `RustTs` |
| --- | --- | --- | --- | --- |
| Call `sum(a, b)` | 36 ns | 71 ns | 130 ns | 27.2 µs |
| Call with a 20-field object in and out | 2.76 µs | 2.46 µs | 2.65 µs | 44.4 µs |
| One call making 1,000 host calls | 24.7 µs | 62.8 µs | 74.9 µs | 244 µs |
| Pure compute, `fib(20)` | 294 µs | 968 µs | 896 µs | 957 µs |
| Event to one handler | 181 ns | 135 ns | 250 ns | 28.3 µs |
| Reload a small script | 8.7 µs | 72 µs | 156 µs | 435 µs |

QuickJS itself is about 3× slower than Lua on pure compute; `Engine` adds tens of
nanoseconds per crossing on top of it. `Engine` reloads transpile the TypeScript
every time, since it has no cache yet; `RustTs` reloads use its disk cache.
Run `cargo run --release --example native_roundtrip` for a quick check of the same
comparison.
