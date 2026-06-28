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

## Quickstart In 10 Minutes

Enable the derive feature when you want Rust payload structs to drive the
generated TypeScript types:

```toml
[dependencies]
ts_embed_vm = { version = "0.1.0", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
```

Create a VM, declare host contracts in Rust, and register them through the
typed helpers:

```rust
use serde::{Deserialize, Serialize};
use ts_embed_vm::{
    HostCallback, HostContract, HostContractKind, HostFunction, Schema, TsSchema, TsVm, VmError,
    VmOptions,
};

#[derive(Deserialize, TsSchema)]
#[serde(rename_all = "camelCase")]
struct FindUserInput {
    user_id: u64,
    include_roles: bool,
}

#[derive(Serialize, TsSchema)]
#[serde(rename_all = "camelCase")]
struct FindUserOutput {
    user_id: u64,
    display_name: String,
    active: bool,
    roles: Vec<String>,
}

#[derive(Deserialize, Serialize, TsSchema)]
#[serde(rename_all = "camelCase")]
struct UserFoundPayload {
    user_id: u64,
    display_name: String,
    roles: Vec<String>,
}

struct FindUser;
struct UserFound;

impl HostContract for FindUser {
    const NAME: &'static str = "user.find";

    fn schema() -> Schema {
        FindUserInput::schema()
    }

    fn kind() -> HostContractKind {
        HostContractKind::Function
    }
}

impl HostFunction for FindUser {
    type Input = FindUserInput;
    type Output = FindUserOutput;

    fn call(input: Self::Input) -> Result<Self::Output, VmError> {
        Ok(FindUserOutput {
            user_id: input.user_id,
            display_name: format!("user-{}", input.user_id),
            active: true,
            roles: if input.include_roles { vec!["admin".into()] } else { Vec::new() },
        })
    }
}

impl HostContract for UserFound {
    const NAME: &'static str = "user.found";

    fn schema() -> Schema {
        UserFoundPayload::schema()
    }

    fn kind() -> HostContractKind {
        HostContractKind::Callback
    }
}

impl HostCallback for UserFound {
    type Payload = UserFoundPayload;
}

fn main() -> Result<(), VmError> {
    let mut options = VmOptions::default();
    options.cache_dir = "target/tsvm-cache".into();

    let vm = TsVm::new(options)?;
    vm.registry()
        .typed_function::<FindUser>()?
        .typed_callback::<UserFound>()?;

    vm.registry().write_sdk_files("target/tsvm-generated")?;
    vm.load_script("plugin", include_str!("plugin.ts"))?;

    let result = vm.call_function("plugin", "lookup", Vec::new())?;
    println!("{result}");

    vm.emit_callback::<UserFound>(&UserFoundPayload {
        user_id: 7,
        display_name: "user-7".into(),
        roles: vec!["admin".into()],
    })?;

    vm.shutdown()
}
```

The SDK export writes:

- `tsvm.d.ts` for declarations
- `tsvm.sdk.ts` for ergonomic helpers such as `user.find(...)`,
  `ctx.on(...)`, `events.user.found(...)`, `call(...)`, and
  `models.Type.create/is/wrap(...)`

The script can then use the generated contract shape:

```ts
let lastFound = "none";

ctx.on("user.found", event => {
  lastFound = `${event.displayName}:${event.roles.join(",")}`;
});

export function lookup() {
  const result = user.find({ userId: 7, includeRoles: true });
  return `${result.displayName}:${result.roles.length}:${result.active}`;
}

export function observed() {
  return lastFound;
}
```

See `tests/dogfood_usage.rs` for an executable version that registers typed
contracts, generates the SDK files, typechecks a small SDK consumer when `tsc`
is available, and runs the script through `TsVm`.

For a friendlier guide that can be dropped into Docusaurus later, see
`docs/guides/register-host-functions.md`.

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

V0 limits:

- imports must be statically discoverable; dynamic `import(...)` is rejected
- `HostContext` is declarative metadata only: it contributes schema,
  declarations, SDK typing, and ABI identity, but does not inject a mutable Rust
  object graph into every script
- `async-promise` is experimental and requires the async worker-lane path; normal
  sync workers reject Promise-returning host contracts instead of faking a direct
  return value

## Toolchain And Release Checks

V0.1 pins the repository toolchain in `rust-toolchain.toml`:

- `nightly-2026-03-06`
- `rustc 1.96.0-nightly`
- components: `rustfmt`, `clippy`

The realistic MSRV for this snapshot is Rust `1.96`-era nightly. A stable MSRV is
not promised yet because the project uses Rust 2024 plus current `oxc` and
`rquickjs` dependencies that move with a recent compiler ecosystem.

The local release-check matrix should cover Windows and Linux before external
use. CI is intentionally left for a later pass. The feature matrix to keep green
is:

- default features
- `derive`
- `tokio`
- `async-promise`
- `all-features`

## Next Steps

- keep cold-load, hot-call, hot-event, SDK-generation, 400-script mount, and memory benchmarks current
- keep adding heavier concurrent stress tests around reload, event routing, async worker lanes, and registry invariants
- expand `#[derive(TsSchema)]` beyond the current V0 serde compatibility only when real host contracts require it
- improve generated validation hooks as the schema/derive layer matures
- evolve the generated TypeScript SDK ergonomics above the stable schema/ABI bridge
- keep rich mutable `HostContext` object models out of the core V0 runtime
- keep dynamic import graph discovery as a post-V0 feature; V0 intentionally uses static ESM graphs
