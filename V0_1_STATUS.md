# V0.1 Status

## Implemented

- Central `ScriptManager` orchestration facade with sync and async worker lanes.
- Native QuickJS ESM execution for inline scripts and multi-file projects.
- Static ESM project graphs with local imports, `tsconfig` aliases, package imports, and cache invalidation from source/package metadata.
- Script, host contract, and active runtime registries.
- Rust-first host contracts for functions, callbacks, and declarative contexts.
- Typed host contract registration helpers that derive function input/output and callback payload schemas from `TsSchema`.
- Lazy host bridge namespaces such as `user.find(...)`.
- Schema-driven `.d.ts` and TypeScript SDK generation.
- Generated SDK wrappers for functions, events, contexts, aggregate access, typed dynamic `call(...)`, and lightweight object model classes/helpers with generated type guards.
- Runtime validation for host inputs/outputs with optional unknown-field rejection.
- `TsSchema` derive support for common structs, enums, generics, serde naming, transparent newtypes, untagged/tagged/adjacently tagged enums, arrays, records, sets, and std string-like types.
- Hot event route table with sync and async-lane delivery.
- Runtime introspection for scripts, workers, routes, dependencies, queues, latency, and memory pressure.
- Dogfood test coverage for a real typed `user.find(...)` function plus `ctx.on("user.found", ...)` callback flow using generated SDK files.
- Benchmarks for load/call/event/SDK/mount/memory shape.
- Stress tests for concurrent loads, reloads, oneshot demount, dependency refs, dependency edges, unload/emit races, calls/events, and project reloads.

## Current Verification

- `cargo +nightly-2026-03-06 fmt --all --check`
- `cargo +nightly-2026-03-06 clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo +nightly-2026-03-06 test --workspace`
- `cargo +nightly-2026-03-06 test --workspace --no-default-features --features derive`
- `cargo +nightly-2026-03-06 test --workspace --no-default-features --features tokio`
- `cargo +nightly-2026-03-06 test --workspace --no-default-features --features async-promise`
- `cargo +nightly-2026-03-06 test --workspace --all-features`

Latest full all-features test run: `174 passed`.

## Remaining Direction

- Expand derives only when real host contracts expose schema gaps.
- Improve validation hooks as derive/proc-macro maturity grows.
- Keep generated SDK ergonomics evolving above the stable schema/ABI bridge.
- Keep benchmarks current when runtime behavior changes.
- Keep the V0.1 public surface aligned with `docs/API_FREEZE_V0_1.md`.
- Keep richer mutable `HostContext` out of the V0 core.
