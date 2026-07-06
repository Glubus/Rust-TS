# Getting Started

This guide creates a tiny Rust application that embeds TypeScript and exposes
one Rust function to the script.

The example contract is:

```ts
math.add({ left: 20, right: 22 }) // 42
```

## Create A Rust Project

```text
cargo new rustts-hello
cd rustts-hello
```

Add dependencies:

```text
cargo add rustts --features derive
cargo add serde --features derive
cargo add serde_json
```

When testing from a local checkout before publishing the crate, use a path
dependency instead:

```text
cargo add rustts --path ../RustTS --features derive
cargo add serde --features derive
cargo add serde_json
```

## Create Your First Contract

Replace `src/main.rs` with:

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;
use rustts::{
    HostContract, HostContractKind, HostFunction, Schema, SdkFileNames, TsSchema, RustTs, VmError,
    VmOptions,
};

#[derive(Deserialize, TsSchema)]
#[serde(rename_all = "camelCase")]
struct AddInput {
    left: i32,
    right: i32,
}

#[derive(Serialize, TsSchema)]
#[serde(rename_all = "camelCase")]
struct AddOutput {
    value: i32,
}

struct Add;

impl HostContract for Add {
    const NAME: &'static str = "math.add";

    fn schema() -> Schema {
        AddInput::schema()
    }

    fn kind() -> HostContractKind {
        HostContractKind::Function
    }
}

impl HostFunction for Add {
    type Input = AddInput;
    type Output = AddOutput;

    fn call(input: Self::Input) -> Result<Self::Output, VmError> {
        Ok(AddOutput {
            value: input.left + input.right,
        })
    }
}

fn main() -> Result<(), VmError> {
    let vm = RustTs::new(VmOptions {
        cache_dir: "target/rustts-cache".into(),
        ..VmOptions::default()
    })?;

    vm.registry().typed_function::<Add>()?;
    vm.registry().write_sdk_files_with_names(
        "target/generated",
        &SdkFileNames {
            types: "my_sdk.d.ts".into(),
            sdk: "my_sdk.ts".into(),
        },
    )?;

    vm.load_script("math-script", include_str!("math_script.ts"))?;

    let output: Value = vm.call_function("math-script", "run", Vec::new())?;
    println!("{output}");

    vm.shutdown()
}
```

What matters:

- `HostContract::NAME = "math.add"` controls the TypeScript API path.
- `AddInput` is the script input type.
- `AddOutput` is the script output type.
- `#[derive(TsSchema)]` lets the registry generate TypeScript declarations.
- `typed_function::<Add>()` registers the Rust function in the VM.

## Add The TypeScript Script

Create `src/math_script.ts`:

```ts
export function run() {
  const result = math.add({ left: 20, right: 22 });
  return result.value;
}
```

Run it:

```text
cargo run
```

Expected output:

```text
42
```

## Generated Files

This line:

```rust
vm.registry().write_sdk_files_with_names(
    "target/generated",
    &SdkFileNames {
        types: "my_sdk.d.ts".into(),
        sdk: "my_sdk.ts".into(),
    },
)?;
```

writes:

```text
target/generated/
  my_sdk.d.ts
  my_sdk.ts
```

`my_sdk.d.ts` contains the TypeScript declarations for your contracts. In this
example, it declares that the generated helper `math.add(...)` accepts
`{ left, right }` and returns `{ value }`.

`my_sdk.ts` contains the small runtime helper layer used by scripts. A host
application can generate these files on demand through its own CLI.

For a real application package, you can also expose imports such as:

```ts
import { math } from "my_sdk";
```

That requires setting `HostContract::IMPORT_MODULE = "my_sdk"` on the Rust
contracts. See [Generate TypeScript SDK Files](guides/generate-sdk-files.md).

## Next

Read [Register Host Functions And Callbacks](guides/register-host-functions.md)
when you want callbacks/events, friendlier SDK namespaces, and stricter
validation.
