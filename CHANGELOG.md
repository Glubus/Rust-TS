# Changelog

## Unreleased (0.4)

### Migration

- `VmError` is `#[non_exhaustive]`: add a wildcard arm to exhaustive matches.

### Added

- `Engine::interrupt_handle` returns an `InterruptHandle` (`Send + Clone`) that stops
  the running load, call or emit from another thread with `VmError::Interrupted`.

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
