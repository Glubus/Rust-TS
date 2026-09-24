# Changelog

## Unreleased (0.3)

### Migration

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
  `emit`), under the same `execution_timeout` as the worker pool (runaway loads,
  calls and emits are interrupted). See [the Engine guide](docs/guides/engine.md)
  and `examples/native_roundtrip.rs`.
- `benches/vs_lua.rs`: the same workloads on mlua, raw QuickJS and RustTS.

### Fixes

- Integral JavaScript numbers above 2^31 no longer come back to Rust as floats on
  the JSON bridge.
- The derived native bridge of 0.2 only ran when `TsSchema` was listed before
  `Serialize`/`Deserialize` in `#[derive]`; derived types now always convert
  natively.
- Unloading or replacing a script no longer deletes a host import module whose name
  equals the script id.

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
