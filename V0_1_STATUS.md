# V0.1 Status

## Implemented

- Central `ScriptManager` orchestration facade with sync and async worker lanes.
- Native QuickJS ESM execution for inline scripts and multi-file projects.
- Static ESM project graphs with local imports, `tsconfig` aliases, package imports, and cache invalidation from source/package metadata.
- Script, host contract, and active runtime registries.
- Rust-first host contracts for functions, callbacks, and declarative contexts.
- Lazy host bridge namespaces such as `user.find(...)`.
- Schema-driven `.d.ts` and TypeScript SDK generation.
- Generated SDK wrappers for functions, events, contexts, aggregate access, and typed dynamic `call(...)`.
- Runtime validation for host inputs/outputs with optional unknown-field rejection.
- `TsSchema` derive support for common structs, enums, generics, serde naming, transparent newtypes, untagged enums, arrays, records, sets, and std string-like types.
- Hot event route table with sync and async-lane delivery.
- Runtime introspection for scripts, workers, routes, dependencies, queues, latency, and memory pressure.
- Benchmarks for load/call/event/SDK/mount/memory shape.
- Stress tests for concurrent loads, reloads, oneshot demount, dependency refs, dependency edges, unload/emit races, calls/events, and project reloads.

## Current Verification

- `rtk cargo fmt --all --check`
- `rtk cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `rtk cargo test --workspace --all-features`

Latest full test run: `164 passed`.

## Remaining Direction

- Expand derives only when real host contracts expose schema gaps.
- Improve validation hooks as derive/proc-macro maturity grows.
- Keep generated SDK ergonomics evolving above the stable schema/ABI bridge.
- Keep benchmarks current when runtime behavior changes.
- Keep richer mutable `HostContext` out of the V0 core.
