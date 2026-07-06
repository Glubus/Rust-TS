# RustTS

Small embedded TypeScript runtime for Rust applications.

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
- [Load Scripts And Projects](docs/guides/load-scripts-and-projects.md)
- [Use Native Bytes](docs/guides/native-bytes.md)

Build the book with:

```text
mdbook build
```

## Status

The current core is focused on the Rust-first contract model:

- long-lived QuickJS workers through `rquickjs`
- TypeScript transpilation through `oxc`
- static ESM project graphs
- typed host functions and callbacks
- generated TypeScript declarations and SDK helpers
- optional contract validation
- native `Uint8Array` output through `NativeBytes`

## Toolchain

The repository pins its Rust toolchain in [`rust-toolchain.toml`](rust-toolchain.toml).

Local checks:

```text
cargo fmt --all --check
cargo clippy --workspace --all-features --all-targets -- -D warnings
cargo test --workspace --all-features
cargo check --benches --features derive
```
