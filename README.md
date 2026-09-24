# RustTS

Small embedded TypeScript runtime for Rust applications.

Version 0.2.0 is licensed under MIT. See [the changelog](CHANGELOG.md) for migration notes.

`RustTS` lets a Rust host load TypeScript scripts, expose Rust host
functions and callbacks, and generate TypeScript declaration/SDK files from the
Rust-side contract registry.

## What It Is For

- game scripting and modding APIs
- plugin systems inside Rust applications
- editor or tool automation
- simulation rules written in TypeScript
- any Rust host that wants a typed scripting layer without embedding Node or Deno

## Core Flow

1. Define Rust host contracts with `HostFunction` and `HostCallback`.
2. Derive `TsSchema` for input/output payloads.
3. Register contracts in the VM registry.
4. Generate TypeScript declaration and SDK files for your package.
5. Load TypeScript scripts and call exported functions.

```rust
let vm = RustTs::new(VmOptions::default())?;

vm.registry()
    .typed_function::<FindUser>()?
    .typed_callback::<UserFound>()?;

vm.registry().write_sdk_files_with_names("generated", &SdkFileNames {
    types: "my_sdk.d.ts".into(),
    sdk: "my_sdk.ts".into(),
})?;
vm.load_script("weather-rules", include_str!("weather_rules.ts"))?;

let output = vm.call_function("weather-rules", "run", Vec::new())?;
```

Scripts then use the generated API:

```ts
const result = user.find({ userId: 7 });

ctx.on("user.found", event => {
  console.log(event.displayName);
});
```

## Documentation

The docs are an mdBook source tree in [`docs/`](docs/SUMMARY.md).

Useful starting points:

- [Getting Started](docs/getting-started.md)
- [Register Host Functions And Callbacks](docs/guides/register-host-functions.md)
- [Generate TypeScript SDK Files](docs/guides/generate-sdk-files.md)
- [Rust And TypeScript Types](docs/guides/type-mapping.md)
- [Run Scripts On Your Thread With Engine](docs/guides/engine.md)
- [Load Scripts And Projects](docs/guides/load-scripts-and-projects.md)
- [Use Native Bytes](docs/guides/native-bytes.md)

Build the book with:

```text
mdbook build
```

## Status

The current core is focused on the Rust-first contract model:

- `Engine`: QuickJS on your own thread, with direct native calls in both directions
- `RustTs`: long-lived QuickJS worker threads through `rquickjs`
- TypeScript transpilation through `oxc`
- static ESM project graphs
- typed host functions and callbacks
- native Rust ↔ JavaScript value conversion with `serde_json` semantics
- generated TypeScript declarations and SDK helpers
- optional contract validation
- native `Uint8Array` in both directions through `NativeBytes`

## Performance

With `Engine`, a Rust → TypeScript call costs about 130 ns and a round trip with a
20-field object about 2.7 µs, close to calling QuickJS directly and to mlua
(36 ns and 2.8 µs). See [the Engine guide](docs/guides/engine.md#performance) for
the full `cargo bench --bench vs_lua` comparison.

## Toolchain

The repository pins its Rust toolchain in [`rust-toolchain.toml`](rust-toolchain.toml).

Local checks:

```text
cargo fmt --all --check
cargo clippy --workspace --all-features --all-targets -- -D warnings
cargo test --workspace --all-features
cargo check --benches --features derive
```

Install the pinned SDK type checker with `npm ci` before running the tests.
CI requires it; locally, set `RUSTTS_REQUIRE_TSC=1` to enforce the same rule.

Execution budgets, shutdown, reload, cancellation and cache guarantees are
documented in [Runtime Guarantees](docs/guides/runtime-guarantees.md).
Run `cargo run --release --example operational_probe` for operational measurements.
