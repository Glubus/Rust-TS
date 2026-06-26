# ts-embed-vm

Small embedded TypeScript runtime for Rust applications.

## Goal

Expose a lightweight library that can be embedded into unrelated Rust projects to:

- accept TypeScript source at runtime
- transpile it once with `oxc`
- persist the JavaScript artifact on disk
- execute it inside a long-lived `rquickjs` runtime
- keep several scripts loaded at the same time
- stay alive with a sleeping idle loop instead of recreating a VM per call

## Design

- one central `ScriptManager` orchestration facade
- one QuickJS `Runtime` per worker thread
- one bounded command queue per worker for backpressure
- per-worker stats expose sync/async queue depth, peak depth, and rejected sends
- manager stats expose load/call/emit latency count, total, average, and max
- optional fixed-bucket latency histograms can be enabled for production tuning
- optional QuickJS memory pressure thresholds classify stats as normal, warning, or critical
- one sticky script registry: `script_id -> worker_id`
- round-robin placement for newly loaded scripts
- one disk cache keyed by versioned source/compiler/resolver/runtime/host ABI identity
- one cloneable Rust handle so multiple threads can use the same VM
- one subscription stream for lifecycle and call events
- optional `tokio` feature for async wrappers with no default overhead

Worker sizing:

- `worker_threads = 0` means automatic sizing
- `worker_threads = 1` forces single-runtime mode
- `worker_threads = N` starts `N` independent QuickJS runtimes

## Status

Current scope:

- load or replace a TypeScript script
- load one multi-file TypeScript project from an entry file
- call one exported function through JSON values
- call one project export as a oneshot module graph
- call registered Rust host functions from scripts
- emit typed Rust-declared callbacks to subscribed scripts
- unload a script
- inspect known script registry entries
- query runtime stats, including per-worker queue/backpressure counters, operation latency, and QuickJS memory pressure
- subscribe to VM events
- shut down cleanly

## Script Contract

The supported script export contract is native ESM:

```ts
export function sum(input: { left: number; right: number }) {
  return input.left + input.right;
}
```

Multi-file project support in V0 is intentionally narrow:

- local relative static ESM imports and re-exports
- named, default, and namespace import forms
- extensionless local resolution backed by `oxc_resolver`
- `index.*` resolution backed by `oxc_resolver`
- `tsconfig.json` `baseUrl` / `paths` aliases backed by `oxc_resolver`
- package imports resolved from project-local `node_modules`
- one ESM module graph cached from the full project source graph
- one runtime-scoped ESM graph per script mount to isolate module state
- type-only imports and re-exports are ignored by the runtime graph
- dynamic imports are rejected in V0 because they are outside the static cache graph
- scripts execute through QuickJS native ESM modules

Host bridge support in V0:

- Rust host functions can be registered through the host contract registry
- registered host functions are exposed as lazy JS namespaces like `user.find(...)`
- the runtime bridge is currently synchronous
- async host function descriptors are explicit about blocking JS bridge behavior
- typed callbacks carry delivery metadata such as broadcast or first-listener delivery
- registered contracts render TypeScript declarations from schema/ABI metadata
- optional Tokio async wrappers are available behind the `tokio` feature
- experimental `async-promise` feature enables `rquickjs/futures` and includes an AsyncContext host bridge that returns real JavaScript Promises through a manager-owned async worker lane; `emit_async` uses async worker replies for async-lane event delivery, while the main synchronous worker pool still uses the synchronous bridge

## Next Steps

- keep cold-load, hot-call, hot-event, SDK-generation, 400-script mount, and memory benchmarks current
- keep adding heavier concurrent stress tests around reload, event routing, async worker lanes, and registry invariants
- expand `#[derive(TsSchema)]` beyond the current V0 serde compatibility only when real host contracts require it
- improve generated validation hooks as the schema/derive layer matures
- evolve the generated TypeScript SDK ergonomics above the stable schema/ABI bridge
- keep rich mutable `HostContext` object models out of the core V0 runtime
- keep dynamic import graph discovery as a post-V0 feature; V0 intentionally uses static ESM graphs
