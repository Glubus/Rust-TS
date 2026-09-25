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
cargo add rustts@0.3 --features derive
cargo add serde --features derive
cargo add serde_json
```

## Create Your First Contract

Replace `src/main.rs` with:

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;
use rustts::{
    Engine, HostContract, HostContractKind, HostFunction, Schema, SdkFileNames, TsSchema,
    VmError, VmOptions,
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
    let mut engine = Engine::new(&VmOptions::default())?;

    engine.registry().typed_function::<Add>()?;
    engine.registry().write_sdk_files_with_names(
        "target/generated",
        &SdkFileNames {
            types: "my_sdk.d.ts".into(),
            sdk: "my_sdk.ts".into(),
        },
    )?;

    engine.load_script("math-script", include_str!("math_script.ts"))?;

    let output: Value = engine.call("math-script", "run", ())?;
    println!("{output}");

    Ok(())
}
```

What matters:

- `HostContract::NAME = "math.add"` controls the TypeScript API path.
- `AddInput` is the script input type.
- `AddOutput` is the script output type.
- `#[derive(TsSchema)]` lets the registry generate TypeScript declarations.
- `typed_function::<Add>()` registers the Rust function in the engine; register
  contracts before loading the scripts that call them.
- `Engine` runs the script on the current thread: `call` returns once the script
  function has returned.

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
engine.registry().write_sdk_files_with_names(
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
example, it declares that the global `math.add(...)` accepts `{ left, right }` and
returns `{ value }`. Include it in the scripts' `tsconfig.json` so `tsc --noEmit`
type-checks them: loading a script transpiles it without type checking.

`my_sdk.ts` is the generated SDK module, with typed helpers built on the same
host functions. A host application can generate these files on demand through
its own CLI.

For a real application package, you can also expose imports such as:

```ts
import { math } from "my_sdk";
```

That requires setting `HostContract::IMPORT_MODULE = "my_sdk"` and
`HostContract::EXPORT_PATH = &["math", "add"]` on the Rust contract. See
[Generate TypeScript SDK Files](guides/generate-sdk-files.md).

## Next

Read [Register Host Functions And Callbacks](guides/register-host-functions.md)
when you want callbacks/events, friendlier SDK namespaces, and stricter
validation.
