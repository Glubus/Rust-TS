# Changelog

## Unreleased (0.4)

### Migration

- `VmError` is `#[non_exhaustive]`: add a wildcard arm to exhaustive matches.
- `VmError::Json` is removed, and with it the `From<serde_json::Error>` conversion:
  a host function that used `?` on a `serde_json` error maps it to a `VmError`
  itself.
- `TsType`, `HostContractKind` and `HostContractAbi` are `#[non_exhaustive]`: add a
  wildcard arm to exhaustive matches.
- `MemoryStats` and `ReloadReport` are `#[non_exhaustive]`: read their fields
  instead of building them with struct literals or destructuring them without `..`.
  `ReloadReport` has a new `dispose_failed` field.
- `VmOptions` and `ScriptBuiltins` stay exhaustive, but new fields may be added in
  minor releases: build them with `..VmOptions::default()` and
  `..ScriptBuiltins::NONE` (or `::ALL`).
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
  `Send + Sync` to be registered, as a function or as a callback (`callback`,
  `register_callback`).
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
- Encoders define each field or key as an own data property, as `JSON.parse` does,
  instead of assigning it: derived structs (`#[derive(TsSchema)]`), maps,
  `serde_json::Value`, `#[serde(flatten)]` fields and internally tagged newtype
  variants. A setter a script put on `Object.prototype` no longer runs, and a field
  or key named `__proto__` becomes an ordinary own property instead of replacing the
  object's prototype.
- `InMemoryHostContractRegistry::types()` is removed: it duplicated `dts()`, which
  returns the same declarations.
- `ObjectSchema`, `push_schema_dependency` and `schema_type_ref` are no longer
  exported at the crate root. They only served `#[derive(TsSchema)]` output, which
  now uses hidden aliases; derived code is unaffected as long as `rustts` and
  `rustts_macros` have the same version.
- `MapKey`, the key bound of the `HashMap` and `BTreeMap` codecs, is exported so the
  bound can be named. It is sealed: `String` and the primitive integers are the only
  key types.
- Fixed-size arrays `[T; N]` are declared as `[T, T, …]` (`N` items) instead of
  `T[]`. Script code that builds such values with a different length, or types them
  as `T[]`, may need updating. Schema validation now rejects arrays of the wrong
  length.
- Types with `#[serde(flatten)]` on an integer-keyed `HashMap` or `BTreeMap`, or
  `#[serde(tag = "…")]` enums with a newtype variant over such a map or a scalar, no
  longer compile with `#[derive(TsSchema)]`. Use a string-keyed map or a struct
  payload. These types already panicked at registration, so no working code is
  affected.
- The error for a dynamic `import()` now reads `<path>: dynamic import() is not
  supported; use a static import` (it was `dynamic import is not supported in V0
  module graphs`); update code that matches on the old text.
- Packages that ship both builds now resolve to their ESM entry (the `exports`
  `import` condition, or the `module` field) instead of `main`.

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
  by default; `Engine::set_console` routes them, with their `ConsoleLevel`
  (`#[non_exhaustive]`) and script id, to the host. Logged error stacks point at the
  TypeScript source.
- Timers on a host-driven clock: `setTimeout`, `setInterval`, `clearTimeout` and
  `clearInterval`, fired by `Engine::advance_timers(elapsed)`, the only thing that
  moves the clock, so they are deterministic. `await`ing a timer works in event
  handlers. See [Timers](docs/guides/engine.md#timers).
- `emit` and `request` cost about a third of what they did per script (from about
  300 to about 100 ns for one handler on the development machine, 15 ns above a
  raw QuickJS call): the engine keeps a snapshot of each script's handler functions,
  so delivery reads no global, no property by name and no array. The budget
  clock starts when the operation first reaches a QuickJS interrupt check or a host
  function, so the time of host functions counts and a short operation reads the clock
  not at all: delivering a number to one handler went from about 230-290 to about
  135-180 ns on the loaded development machine, and an event nobody listens to from
  about 55-60 to about 15 ns.
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

- Context groups: `Engine::load_script_in(group, id, source)` and
  `Engine::load_project_in(group, id, entry)` load scripts that trust each other into one
  shared QuickJS context. Each script keeps its own events, timers, `console`,
  `Caller` identity, `ctx.hot` state and lifecycle, and reaches them with
  `import { ctx, console, setTimeout } from "rustts:env"` (also valid in a context of
  its own); the generated declarations type that module. A group is a trust boundary:
  its scripts share the built-ins and the global object. Scripts of a group that follow
  one another in load order are served in one visit to their context, with the event
  encoded once. On a loaded development machine loading 400 scripts took 90 ms instead
  of 383 ms and delivering an event to 250 scripts 58 µs instead of 266 µs, to 1000
  scripts 1.5 ms instead of 4.1 ms. See
  [Context Groups](docs/guides/engine.md#context-groups).

- `Engine::pump` and `Engine::advance_timers` cost one atomic read or one comparison on
  a frame where nothing waits, instead of a look at every script: `pump` entered the
  context of every loaded script (71 µs per call at 1000 scripts, 15 µs at 250, on the
  loaded development machine) and `advance_timers` read every script's next due time
  (1.4 µs at 1000). Both are now about 4 ns. A script is only entered when it has an
  answer or a timer.
- A due timer costs less: the prelude keeps timers in an array instead of iterating a
  `Map` (which allocates an iterator and a result per step), and fires a single due
  timer without building a list. One script with a timer due on every call went from
  2.1 to 0.84 µs, 250 of them from 2.3 ms to 0.28 ms and 1000 from 8.2 to 3.8 ms.
  Scripts of a context group that follow one another are served in one visit, as for
  events: 1000 grouped scripts took 2.2 ms.

- The `disable-assertions` feature builds QuickJS without its internal assertions:
  a call-heavy script runs about 23 % faster, host calls and event delivery about 10 %
  (see [Release builds without QuickJS
  assertions](docs/guides/engine.md#release-builds-without-quickjs-assertions)).

- Delivering an event takes no reference to a handler or to the event: handlers are
  kept as raw function values and called through the C API with borrowed values, where
  `Function::call` took and released several references to the function, its context
  and the event per handler (a context reference count is a write to memory of that
  context, which is a cache miss once there are hundreds of them), and nothing wraps
  the returned value when `emit` drops it. `emit` of a number to one handler went from
  192 to 87 ns, and from 1.8 to 0.5 µs per script with 1000 scripts, each in a context
  of its own. An event built from an object is dominated by building it, 360 to 480 ns
  for a two-field `serde_json::Value` or a derived struct. Ending an operation is
  cheaper too (an empty job queue and no rejection are checked, not run).

- Calling a host function from a script is about 40 % cheaper: a host function is a
  callable object of a QuickJS class of its own that the engine calls through the C API
  with the arguments borrowed, instead of an rquickjs `Function` that clones the context
  and the arguments about seven times per call and dispatches through a table. A script
  looping over a one-argument host call went from 205 to 138 ns per iteration (the loop
  alone is 50), 19.6 to 15.3 billion instructions for 20 million calls. `async` host
  functions use the same objects.

- `Engine::call` and `call_deferred` are 15 to 25 % cheaper: the module's namespace is
  kept as a raw reference instead of a `Persistent` cloned and restored at each call, the
  export name's atom is kept (the property is still read at each call, so a binding a
  module reassigns is seen) and the result is checked with one `JS_IsFunction` instead of
  a full type probe. A call with no argument went from 1237 to 1006 instructions
  (203 to 163 ns on the loaded development machine), one with two numbers from 1715 to
  1482. A name that is not a function (`export const answer = 42`) is now
  `VmError::FunctionNotFound`, where it was a conversion error.

- `Engine::call` and `call_deferred` with only numbers and booleans as arguments (up
  to 8), and a number, a boolean or `()` as the result, build and read them as raw
  QuickJS values: no wrapper, so no context reference taken and released per argument
  and per result. A call with two numbers went from 1342 to 996 instructions and from
  about 660 to 422 cycles (about -35 %), a call without arguments from 1006 to 817.
  Any other argument list or result (text, objects, a mix) goes through the same
  encoders as before. `JsEncode::encode_scalar`, `JsDecode::decode_scalar` and
  `JsArgs::encode_scalars` are the hooks, hidden and defaulted: nothing to migrate.

- `benches/native_bridge.rs` group `native_bridge_encode_objects`: `Engine::call`
  with a 16- and a 1024-entry `HashMap<String, u32>`, `HashMap<u32, u32>` and
  `serde_json::Value` object as its argument.

### Fixes

- A script can no longer import another script's modules by naming their internal
  ids (`rustts://graph/<n>/...`, `rustts:env/<n>`, `rustts:host/<n>/...`), statically
  or through a run-time `import()`; only host modules and modules of the script's own
  graph resolve.
- `emit`, `request`, `request_deferred` and `advance_timers` stop entering handlers
  and timers once the operation is interrupted or a QuickJS interrupt check finds its
  execution budget spent, instead of running every remaining one; timers not reached
  stay due for the next call. The check between two handlers reads two flags, not
  the clock.
- An interrupt during `ctx.hot.save` or `ctx.hot.dispose` is reported as
  `VmError::Interrupted` instead of `VmError::Execution`.
- After a failed reload, `reload_changed` also watches the files the failed attempt
  reached, so fixing a module that only the new version imports triggers the reload.
- Configs that a project's `tsconfig.json` `extends` through a relative or absolute
  path are watched: editing `paths` or `baseUrl` in a base config is picked up by
  `reload_changed` and by the next `load_project`.
- Packages in `node_modules` resolve as ES modules: `exports` use the `import`,
  `module` and `default` conditions, and packages without `exports` use their
  `module` field before `main`.
- The transpile cache no longer fails a load: an artifact that cannot be read is a
  cache miss, and an artifact that cannot be written is skipped.
- Contract validation (`VmContractValidation::Inputs`/`InputsAndOutputs`) no longer
  rejects `NativeBytes` values: a `Uint8Array` or `ArrayBuffer` is checked as its
  bytes instead of failing with `expected array, got object`. `NaN` and `±Infinity`
  now pass `number` schemas, matching the native codec. `serde_json::Value` decoding
  still rejects non-finite numbers.
- Generated `.d.ts` and SDK types quote property names that are not identifiers (for
  example `#[serde(rename_all = "kebab-case")]` fields such as `"max-speed"`), so they
  are valid TypeScript. Reserved words such as `default` stay bare.
- Map, `serde_json::Value`, `#[serde(flatten)]` and internally tagged newtype encoders
  define own data properties instead of assigning them, as derived struct encoders do
  (see Migration): a `"__proto__"` key no longer replaces the object's prototype, and
  inherited setters never run. They define through the QuickJS C API, which costs no
  more than the assignment did for a 16-key object and is about 5 % cheaper for a
  1024-key one.
- `TsSchema` for `[T; N]` declares a tuple of exactly `N` items, matching the codec,
  which requires that length. Schema validation and SDK predicates check the length
  too.
- `TsSchema` is implemented for `HashMap<K, V, S>` and `HashSet<T, S>` with any
  hasher, like the codecs.
- `#[derive(TsSchema)]` on a `#[serde(flatten)]` map with integer keys
  (`HashMap<u32, _>`, `BTreeMap<i64, _>`, …), or on an internally tagged newtype
  variant over such a map or a scalar, is a compile error. Before, building the schema
  panicked when the contract was registered.
- A panic in a host function can no longer be swallowed by a script's `try`/`catch`. It
  was stored and re-raised by the next rquickjs call, which a `catch` block in the script
  could precede, so the call returned normally and the panic surfaced later, from an
  unrelated call or never. It now waits until the operation ends and panics out of the
  call that reached the handler. The same holds for `async` host functions and for a
  `console` sink that panics (both returned normally with `"caught"` before). See [Errors And Panics In A Host
  Function](docs/guides/register-host-functions.md#errors-and-panics-in-a-host-function).
- An operation that ran out of memory failed with `non-error exception: Null`,
  `Allocation failed while creating object` or a bare `out of memory`, depending on
  which allocation failed. Once the allocated memory is within 10 % of the limit, it
  now always says `out of memory` with the bytes allocated and names
  `VmOptions::memory_limit_bytes`. The default 16 MiB holds about 200 scripts.
- Host functions took the `Function.prototype` of the first script loaded, in every
  script (an rquickjs behavior). A script that patched its own `Function.prototype`
  changed the host functions every other script saw, and `hostFunction instanceof
  Function` was false in all but that first script. Each host function now has its own
  script's `Function.prototype`.
- `rustts-sdk --help` (or `-h`) prints its usage to stdout and exits with 0; it
  printed the usage as an error and exited with 1. Other errors print `error: …` on
  stderr and exit with 1.

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
