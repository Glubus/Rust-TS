# Changelog

## Unreleased (0.4)

### Migration

- `VmError` is `#[non_exhaustive]`: add a wildcard arm to exhaustive matches.
- `ReloadReport` has a new `dispose_failed` field; add it to struct literals and
  patterns that list every field.
- The generated declarations and SDK always declare and export `ctx` (typed with
  `ctx.hot`), even without host events, so their output is no longer empty for a
  registry without contracts. `rusttsSdk.ctx` is always present.
- Error text changed: `VmError::Execution` stacks name TypeScript locations
  (`lib/math.ts:8:15`, `<id>.ts:3:5`) instead of `rustts://graph/{n}/{path}:{line}:{col}`,
  and `VmError::Transpile` lists `path:line:column: message` diagnostics instead of
  a debug dump. Update code that parses either. Existing cache artifacts are
  rebuilt once: they now carry a source map.
- `HostFunction` is split in two: `HostFunctionSignature` holds `Input` and `Output`;
  `HostFunction` only keeps `call`. Move everything but `call` into an
  `impl HostFunctionSignature` block. The contract type no longer needs
  `Send + Sync` to be registered.
- **The JSON registration path is gone.** Values cross natively only, and the schemas
  come from `TsSchema` of the `Input`, `Output` and `Payload` types:
  - Removed: the untyped `function`, `function_with`, `function_with_caller`,
    `async_function_with`, `async_function_with_caller` and `callback` registrations
    (and their `register_*` trait methods), `HostFunctionSignature::input_schema`,
    `output_schema` and `function_descriptor`, and `HostCallback::payload_schema` and
    `callback_descriptor`. `Input`, `Output` and `Payload` no longer need `serde`
    bounds.
  - The `typed_*` registrations lose their prefix: `typed_function` is now
    `function`, and likewise `function_with`, `function_with_caller`,
    `async_function_with`, `async_function_with_caller`, `callback` and `request`
    (`register_function`, ..., `register_request` on `HostContractRegistry`, which
    custom registries implement).
  - A contract with a free-form value uses `serde_json::Value`, which has the schema
    `Json`; to describe an object, derive `TsSchema` on a struct. A schema written by
    hand for a type that does not declare it is no longer possible.
  - `HostContract::schema()` is removed: delete it from function and callback
    contracts. Only context contracts declare a schema, through the new required
    `HostContext::schema()`. `HostContract::descriptor()` carries a placeholder schema
    until a registration fills it.
  - See [Migrate From 0.3 To 0.4](docs/guides/migrating-0.3-to-0.4.md).
  - Contract validation now checks a JSON snapshot of the values the native codec
    converts. A host function that resolves with an invalid output no longer gets
    `VmError::ContractValidation` from `HostResolver::resolve`: the engine finds it
    when it converts the output, and the script's Promise rejects.
- `HostCallbackDescriptor` has a new `reply_schema` field and
  `HostContractAbi::Callback` a new `reply` field; add them to struct literals and
  patterns that list every field. Serialized descriptors omit them when `None`.
- Generated declarations and SDK with host events declare `HostReplies`,
  `HostEventReply`, `HostEventHandler` and `HostEventContext`, and type `ctx` as
  `HostEventContext & { readonly hot: HostHotContext }`, which adds `ctx.off`.
  Update snapshots of the generated text.
- `ctx.on` misuse now throws `TypeError: ctx.on expects ...` instead of
  `__rustts_on expects ...`.
- Scripts get the globals `console`, `setTimeout`, `setInterval`, `clearTimeout`
  and `clearInterval`, installed before their code runs.
- The `__vm_handlers` global is gone: handler lists live in the prelude and the
  engine only, and change only through `ctx.on` and `ctx.off`.
- `HostFunctionDescriptor` and `HostContractAbi::Function` add a
  `returns_promise: bool` field; add it to literals and exhaustive patterns.
  It is omitted from serialized synchronous descriptors, but async host
  functions render `Promise<Output>` in generated declarations and SDK.
- `VmOptions` has a new `builtins` field; add it to struct literals that list every
  field (`..VmOptions::default()` keeps working).
- Derived encoders (`#[derive(TsSchema)]`) define each field as an own data
  property, as `JSON.parse` does, instead of assigning it: a setter a script put on
  `Object.prototype` no longer runs, and a field named `__proto__` becomes an own
  property instead of changing the prototype.

### Added

- `Engine::interrupt_handle` returns an `InterruptHandle` (`Send + Clone`) that stops
  the running load, call or emit from another thread with `VmError::Interrupted`.
- State-preserving hot reload through `ctx.hot`: a script registers
  `ctx.hot.save(fn)`, whose result the next version reads as `ctx.hot.data` before
  its top-level code runs, and `ctx.hot.dispose(fn)` cleanups that run on the old
  version once the new one has loaded, or on `unload_script`. A throwing `save` fails
  the reload and keeps the old version; a new version that fails to load leaves the
  old one undisposed; a throwing `dispose` keeps the new version and fails the load
  call, or is listed in the new `ReloadReport::dispose_failed` by `reload_changed`.
  See [Keep State Across Reloads](docs/guides/engine.md#keep-state-across-reloads).
- Errors point at the TypeScript source: every script frame of a
  `VmError::Execution` stack (loads, calls, `async` exports, event handlers,
  Promise jobs, unhandled rejections) shows the file (relative to the project root,
  or `<id>.ts` for an inline script) and the TypeScript line and column. Source maps
  are built at transpile time, cached with the JavaScript and only read on errors.
  See [Error Locations](docs/guides/engine.md#error-locations).
- Host functions implemented by closures: `function_with::<C>(handler)` registers a
  `Send + Sync` closure for a contract implementing `HostFunctionSignature`, so a
  handler can hold state. `function_with_caller`
  also hands it a `Caller`, whose `script_id()` names the
  calling script. See
  [Implement A Host Function With A Closure](docs/guides/register-host-functions.md#implement-a-host-function-with-a-closure).
- Requests, events whose handlers answer: `Engine::request(event, &payload)`
  returns `Vec<(&str, R)>`, each handler's reply with its script id in delivery
  order, awaiting `async` handlers. `HostRequest` and `request` type the
  handlers' return value in the generated TypeScript. See
  [Requests](docs/guides/engine.md#requests).
- `ctx.off(event, handler)` removes a handler; a script without handlers left for
  an event is no longer entered by `emit` for it.
- `console.debug`, `log`, `info`, `warn` and `error` in scripts, written to stderr
  by default; `Engine::set_console` routes them, with their `ConsoleLevel` and
  script id, to the host. Logged error stacks point at the TypeScript source.
- Timers on a host-driven clock: `setTimeout`, `setInterval`, `clearTimeout` and
  `clearInterval`, fired by `Engine::advance_timers(elapsed)`, the only thing that
  moves the clock, so they are deterministic. `await`ing a timer works in event
  handlers. See [Timers](docs/guides/engine.md#timers).
- `emit` and `request` cost about a third of what they did per script (from about
  300 to about 100 ns for one handler on the development machine, 15 ns above a
  raw QuickJS call): the engine keeps a snapshot of each script's handler functions,
  so delivery reads no global, no property by name and no array. The budget
  clock starts at operation entry, including time in host functions.
- Derived encoders look each field name's atom up once per runtime instead of on
  every field of every value: about 15 % less time to encode a small struct.
  [Per-Frame Data](docs/guides/engine.md#per-frame-data) shows how to shape payloads
  sent every frame.
- `Engine::run_gc` collects reference cycles now and `Engine::set_gc_threshold`
  sets or, with `None`, turns off automatic collection, so a game can keep cycle
  collection pauses (tens of milliseconds on a large heap) out of time-critical
  stretches. See [Garbage Collection](docs/guides/engine.md#garbage-collection).
- Host-driven async functions: register contracts with
  `async_function_with` (and `_with_caller`), answer
  via the one-shot `HostResolver<T>` from any thread, then call `Engine::pump`
  on the engine thread to resume scripts. `call_deferred` and `request_deferred`
  yield `PendingCall` handles across frames; pending work cancels on successful
  reload or unload but survives failed reload. See
  [Deferred Script Results](docs/guides/engine.md#deferred-script-results).
- Firing a due timer allocates less in the script prelude (no per-call tuple list,
  no sort for a single due timer, no spread for a callback without arguments):
  `advance_timers` with one due script went from about 4.4 to 2.6 µs on the loaded
  development machine. `cargo bench --bench runtime -- frame_budget` now keeps the
  per-frame cost of `emit` and `advance_timers` from 1 to 1000 scripts.
- Loading a script costs about half as much: the context prelude is compiled once per
  `Engine` and every context loads its bytecode instead of parsing and compiling the
  source (an empty script went from about 1.5 ms to about 0.8 ms on the loaded
  development machine, where compiling the prelude took about 0.45 ms of it).
- `VmOptions::builtins` (`ScriptBuiltins`) chooses the optional JavaScript built-ins of
  each script context: `RegExp`, `Date`, `Proxy`, typed arrays, `WeakRef` and `atob` /
  `btoa` / `performance`. `ScriptBuiltins::NONE` loads a script in about half the time
  and a quarter less memory. See
  [Script Built-ins](docs/guides/engine.md#script-built-ins).

### Fixes

- A load that ran out of memory failed with `non-error exception: Null`. It now says
  `out of memory` and names `VmOptions::memory_limit_bytes`. The default 16 MiB holds
  about 200 scripts.

## 0.3.0 — 2026-09-25

### Migration

- **The worker pool is removed; `Engine` is the only runtime.** RustTS no longer
  starts threads: scripts run on the thread that owns the `Engine`. Replace
  `RustTs::new(options)` with `Engine::new(&options)` (`load_script`,
  `load_project` and `unload_script` take `&mut self`):

  | 0.2 (`RustTs`) | 0.3 (`Engine`) |
  | --- | --- |
  | `vm.load_script(id, src)` → `ScriptSnapshot` | `engine.load_script(id, src)` → `()` |
  | `vm.load_script_project(id, entry)` | `engine.load_project(id, entry)` |
  | `vm.call_function(id, f, vec![json])` → `Value` | `engine.call::<Value>(id, f, vec![json])`, or typed: `engine.call::<R>(id, f, (a, b))` |
  | `vm.emit(event, json)` | `engine.emit(event, &payload)` |
  | `vm.registry()` | `engine.registry()` |
  | `vm.call_function_once(src, f, args)` | `load_script`, `call`, then `unload_script` |
  | `vm.shutdown()` | drop the `Engine` |

  To call scripts from other threads, own the `Engine` on one thread and send it work
  over a channel.
- Removed with the pool: `ScriptSnapshot`, retention policies and dependency
  references, runtime introspection and stats (`VmStats`, `VmRuntimeSnapshot`,
  latency histograms, memory pressure thresholds), lifecycle events (`VmEvent`,
  `subscribe`), the `ScriptRegistry`, and the `rustts-` worker threads.
  `Engine::memory_stats` reports the QuickJS memory counters (`MemoryStats`).
- Removed async host functions: `AsyncHostFunction`, `async_function`,
  `async_promise_function`, the async worker lane, and the `tokio` and
  `async-promise` features. Host functions are synchronous; scripts can still use
  Promises and `async` exports.
- Removed `DeliveryMode`, `HostCallback::delivery`/`hot`, `HostFunctionExecution`
  and the matching `execution`, `delivery` and `hot` fields of descriptors and ABIs.
  Every callback now appears in the generated declarations and SDK.
- `VmOptions` keeps `cache_dir`, `execution_timeout`, `memory_limit_bytes`,
  `max_stack_size_bytes`, `contract_validation` and `unknown_field_validation`.
  `cache_dir` is now `Option<PathBuf>` and `None` by default: nothing is written to
  disk unless a cache directory is set.
- `VmError` drops the pool variants (`ShutdownTimeout`, `ExecutionTimeout`,
  `WorkerOffline`, `QueueFull`, `InvalidWorkerCount`, `ScriptLimitReached`,
  `UnsupportedHostBridge`); `WorkerPanicked` becomes `LockPoisoned`.
- The generated SDK calls host functions through `__host.callValue` only; the JSON
  `__host.call` bridge is gone.
- The transpile cache is now per module: existing cache artifacts are rebuilt once.
  `HostContractRegistry::abi_seed` is removed: host contracts no longer take part in
  cache keys, since they do not change transpiled code.
- `load_project` reuses what it read from an unchanged file: a module whose size and
  modification time are unchanged is not read again.
- Typed registration now requires native codecs: `typed_function` /
  `register_typed_function` need `Input: TsSchema + JsDecode` and
  `Output: TsSchema + JsEncode`; `typed_callback` / `register_typed_callback` need
  `Payload: TsSchema + JsEncode`. `#[derive(TsSchema)]` provides them; hand-written
  types implement `JsEncode` / `JsDecode`.
- `TsSchema` no longer has the hidden `__rustts_from_js_value` /
  `__rustts_into_js_value` methods.
- `#[derive(TsSchema)]` always emits both codecs. Restrict with
  `#[rustts(encode_only)]`, `#[rustts(decode_only)]` or `#[rustts(schema_only)]`.
- `#[rustts(rename)]` and `#[rustts(optional)]` on fields are only accepted on
  `schema_only` types; use the serde attributes elsewhere.
- Serde attributes only serde can execute (`with`, `serialize_with`,
  `deserialize_with`, `from`, `into`, `try_from`, `remote`, `bound`, …) are compile
  errors until the type or field opts in with `#[rustts(codec = "json")]`.
- Generated TypeScript now matches serde's wire format: unit structs are `null`,
  newtype structs are their inner type, externally tagged enums are
  `"Name" | { Name: payload }`, tagged unit-only enums keep their tag objects.
- Integers outside ±(2^53 − 1) fail instead of rounding, including inside
  `serde_json::Value`.
- `rquickjs` 0.12 → 0.14 (quickjs-ng 0.16.2). The hidden `rustts::__rquickjs`
  re-export is replaced by the public `rustts::js` module (`Ctx`, `Value`,
  `Result`, `Error`, `Object`, `Array`, `Function`, `String`, `TypedArray`,
  `ArrayBuffer`, `Args`, `Runtime`, `Context`). Existing cache artifacts are
  rebuilt once.
- `TsType` has a new `OpenObject { fields, rest }` variant (objects with a flattened
  map) and `TsEnumVariant` a new optional `rest`; exhaustive matches on `TsType`
  must handle it.
- Syntax errors in scripts now surface as `VmError::Transpile` instead of
  `VmError::Resolve`.

### Added

- `JsEncode`, `JsDecode` and `JsArgs`: native conversion between Rust values and
  QuickJS values, with `serde_json` semantics and path-carrying errors
  (`position.x: …`). Implemented for primitives, strings, network addresses,
  paths, `Option`, sequences, sets, string- and integer-keyed maps, tuples up to
  12, smart pointers, `serde_json::Value` and `NativeBytes`.
- Derived codecs for every struct shape and all four serde enum representations,
  mirroring serde's rename, alias, skip, default, flatten and
  `deny_unknown_fields` attributes.
- Field opt-ins: `#[rustts(codec = "json")]`, `#[rustts(with = "module")]`,
  `#[rustts(type = "...")]` for third-party types.
- `NativeBytes` as input: decodes from `Uint8Array`, `ArrayBuffer` or byte arrays,
  and implements `Deserialize`.
- `Engine`: single-thread runtime with native calls in both directions (`call`,
  `emit`) under `execution_timeout`. It loads inline scripts and multi-file projects
  (`load_project`), reuses the transpile cache when `cache_dir` is set, and reports
  QuickJS memory counters. Scripts reach host functions by ESM import, by namespaced
  global (`user.find(...)`) or through the generated SDK. Promise jobs run before
  each operation returns, `async` exports resolve to their value, and an unhandled
  Promise rejection fails the operation. Events reach scripts in load order, and a
  throwing handler does not stop the others. See
  [the Engine guide](docs/guides/engine.md) and `examples/native_roundtrip.rs`.
- `Engine::reload_changed` and `ReloadReport`: hot reload without a watcher thread.
  It reloads, in load order, the projects whose files changed (module files, their
  directories up to the project root, `tsconfig.json`, the `package.json` files
  used), reports failed reloads once, and keeps their previous version running.
  Reloads reuse the resolver and the resolutions of unchanged modules while the
  project structure is unchanged, and only transpile the edited files: reloading a
  9-file project after editing one file went from about 1.5 ms to 0.7 ms on the
  development machine.
- `emit` only enters the scripts that registered a handler for the event: `ctx.on`
  records it on the Rust side. An event nobody listens to costs about 55 ns instead
  of about 215 ns per script, and 10 scripts with one listener about 0.5 µs instead
  of 1.5 µs.
- `benches/vs_lua.rs`: the same workloads on mlua, raw QuickJS and RustTS.
- Flattened string-keyed maps (`#[serde(flatten)] rest: BTreeMap<String, V>`,
  `HashMap`, `serde_json::Value`, or an `Option` of one) in derived schemas: rendered
  as an index signature that `tsc` accepts, validated key by key. Flattening a
  non-object type is a compile error.
- Features `uuid`, `chrono` and `glam`: native `TsSchema`/`JsEncode`/`JsDecode` for
  `Uuid`, chrono dates and times, and glam vectors, quaternions and matrices, with
  each crate's serde representation.

### Fixes

- Integral JavaScript numbers above 2^31 no longer come back to Rust as floats on
  the JSON bridge.
- The derived native bridge of 0.2 only ran when `TsSchema` was listed before
  `Serialize`/`Deserialize` in `#[derive]`; derived types now always convert
  natively.
- Unloading or replacing a script no longer deletes a host import module whose name
  equals the script id.
- Deriving `TsSchema` for a struct with a flattened map no longer panics.
- Generated SDK `models.X.is()` predicates now check every field: optional and
  union fields were missing parentheses inside `&&` chains.
- Importing a `HostContext` as a host module fails at load instead of binding
  `undefined`.
- Reloading a project after a `tsconfig.json` `paths` change resolves imports
  against the new configuration instead of reusing the cached graph. Existing project
  cache artifacts are rebuilt once.

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
