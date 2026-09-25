# RustTS

Embeddable TypeScript scripting for Rust applications and games.

RustTS is licensed under MIT. Version 0.3 is Engine-only: it removed the worker pool
(`RustTs`). See [the changelog](CHANGELOG.md) for migration notes.

`RustTS` lets a Rust host run TypeScript scripts on its own thread through
`Engine`, expose typed Rust host functions and callbacks to them, and generate
the scripts' TypeScript declarations and SDK from those Rust contracts. It spawns
no threads and has no event loop: scripts run when the host calls them. It is a
scripting layer, not a server runtime.

## What It Is For

- game UI and gameplay rules
- modding APIs
- plugin systems inside Rust applications
- editor or tool automation
- simulation rules written in TypeScript
- any Rust host that wants a typed scripting layer without embedding Node or Deno

## Core Flow

1. Define Rust host contracts with `HostFunction` and `HostCallback`.
2. Derive `TsSchema` for input/output payloads.
3. Register contracts in the engine's registry.
4. Generate TypeScript declaration and SDK files for your package.
5. Load TypeScript scripts, call their exports and emit events to them.

```rust
let mut engine = Engine::new(&VmOptions::default())?;

engine
    .registry()
    .typed_function::<FindUser>()?
    .typed_callback::<UserFound>()?;

engine.registry().write_sdk_files_with_names("generated", &SdkFileNames {
    types: "my_sdk.d.ts".into(),
    sdk: "my_sdk.ts".into(),
})?;
engine.load_script("weather-rules", include_str!("weather_rules.ts"))?;

let output: serde_json::Value = engine.call("weather-rules", "run", ())?;
engine.emit("user.found", &UserFoundPayload { user_id: 7, display_name: "Ada".into() })?;
```

Scripts call host functions and handle host events:

```ts
let lastFound = "none";

ctx.on("user.found", event => {
  lastFound = event.displayName;
});

export function run() {
  return user.find({ userId: 7 });
}
```

## Documentation

The docs are an mdBook source tree in [`docs/`](docs/SUMMARY.md).

Useful starting points:

- [Getting Started](docs/getting-started.md)
- [Run Scripts With Engine](docs/guides/engine.md)
- [Register Host Functions And Callbacks](docs/guides/register-host-functions.md)
- [Generate TypeScript SDK Files](docs/guides/generate-sdk-files.md)
- [Rust And TypeScript Types](docs/guides/type-mapping.md)
- [Load Scripts And Projects](docs/guides/load-scripts-and-projects.md)
- [Use Native Bytes](docs/guides/native-bytes.md)
- [Runtime Guarantees](docs/guides/runtime-guarantees.md)

Build the book with:

```text
mdbook build
```

## Status

The current core is focused on the Rust-first contract model:

- `Engine`: QuickJS (through `rquickjs`) on your own thread, with direct native
  calls in both directions
- TypeScript transpilation through `oxc`, with an optional disk cache
- inline scripts and static ESM project graphs
- typed host functions and callbacks
- native Rust ↔ JavaScript value conversion with `serde_json` semantics
- generated TypeScript declarations and SDK helpers
- optional contract validation
- native `Uint8Array` in both directions through `NativeBytes`
- execution budget per load, call and emit

## Performance

A Rust → TypeScript call costs about 130 ns and a round trip with a 20-field
object about 2.7 µs, close to calling QuickJS directly and to mlua (36 ns and
2.8 µs), on the reference Windows machine. See
[the Engine guide](docs/guides/engine.md#performance) for the full
`cargo bench --bench vs_lua` comparison.

## Toolchain

The repository pins its Rust toolchain in [`rust-toolchain.toml`](rust-toolchain.toml).

Local checks:

```text
cargo fmt --all --check
cargo clippy --workspace --all-features --all-targets -- -D warnings
cargo test --workspace --all-features
cargo test --workspace --no-default-features
cargo check --benches --features derive
```

Install the pinned SDK type checker with `npm ci` before running the tests.
CI requires it; locally, set `RUSTTS_REQUIRE_TSC=1` to enforce the same rule.

Execution budget, reload and cache guarantees are documented in
[Runtime Guarantees](docs/guides/runtime-guarantees.md).
