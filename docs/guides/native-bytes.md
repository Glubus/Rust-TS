# Use Native Bytes

Use `NativeBytes` when a host function returns large byte payloads to
TypeScript.

Without `NativeBytes`, byte buffers usually become JSON arrays. That is simple,
but it is expensive for large payloads. With the typed `callValue` host path,
`NativeBytes` is exposed to JavaScript as a `Uint8Array`.

## Rust Contract

```rust
use serde::Deserialize;
use rustts::{
    HostContract, HostContractKind, HostFunction, NativeBytes, Schema, TsSchema, VmError,
};

#[derive(Deserialize, TsSchema)]
#[serde(rename_all = "camelCase")]
struct ReadAssetInput {
    path: String,
}

struct ReadAsset;

impl HostContract for ReadAsset {
    const NAME: &'static str = "asset.read";

    fn schema() -> Schema {
        ReadAssetInput::schema()
    }

    fn kind() -> HostContractKind {
        HostContractKind::Function
    }
}

impl HostFunction for ReadAsset {
    type Input = ReadAssetInput;
    type Output = NativeBytes;

    fn output_schema() -> Schema {
        NativeBytes::schema()
    }

    fn call(input: Self::Input) -> Result<Self::Output, VmError> {
        let bytes = std::fs::read(input.path).map_err(VmError::from)?;
        Ok(NativeBytes::new(bytes))
    }
}
```

## TypeScript Shape

The generated declarations expose:

```ts
type NativeBytes = Uint8Array;
```

Scripts can use normal typed-array APIs:

```ts
const bytes = asset.read({ path: "payload.bin" });

if (bytes.byteLength >= 4) {
  const magic = bytes[0];
}
```

## Validation Caveat

When contract validation requires JSON-compatible input or output validation,
the runtime may use the JSON-compatible fallback path. The native `Uint8Array`
path is the fast path for typed host calls when validation does not force JSON
materialization.

Use this type for byte payloads. Do not use JSON arrays for large buffers unless
you specifically need JSON compatibility.
