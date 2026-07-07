# Register Host Functions And Callbacks

This guide shows the normal V0 flow:

1. Create a `RustTs`.
2. Declare Rust payload structs.
3. Derive `TsSchema`.
4. Implement `HostFunction` or `HostCallback`.
5. Register contracts with `typed_function` and `typed_callback`.
6. Generate declaration and SDK files for your package.
7. Use the generated SDK from TypeScript.

## Install With Derive Support

```toml
[dependencies]
rustts = { version = "0.1.0", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
```

## Create The VM

```rust
use rustts::{RustTs, VmError, VmOptions};

fn create_vm() -> Result<RustTs, VmError> {
    let mut options = VmOptions::default();
    options.cache_dir = "target/rustts-cache".into();
    RustTs::new(options)
}
```

The cache directory stores compiled JavaScript artifacts. Use a stable directory
for a real application so reloads can reuse compiled scripts.

## Declare A Host Function

Host functions are Rust calls exposed to TypeScript. The contract name controls
the generated TypeScript namespace. For example, `user.find` becomes
`user.find(...)`.

```rust
use serde::{Deserialize, Serialize};
use rustts::{
    HostContract, HostContractKind, HostFunction, Schema, TsSchema, VmError,
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

struct FindUser;

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
            roles: if input.include_roles {
                vec!["admin".into(), "editor".into()]
            } else {
                Vec::new()
            },
        })
    }
}
```

`typed_function::<FindUser>()` uses `FindUserInput::schema()` and
`FindUserOutput::schema()` automatically, so you do not need to hand-write the
TypeScript shape.

## Declare A Callback

Callbacks are host-emitted events delivered to subscribed scripts. The contract
name controls both `ctx.on("user.found", ...)` and the generated ergonomic alias
`user.onFound(...)`.

```rust
use serde::{Deserialize, Serialize};
use rustts::{HostCallback, HostContract, HostContractKind, Schema, TsSchema};

#[derive(Deserialize, Serialize, TsSchema)]
#[serde(rename_all = "camelCase")]
struct UserFoundPayload {
    user_id: u64,
    display_name: String,
    roles: Vec<String>,
}

struct UserFound;

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
```

## Register Contracts In The Registry

Use the VM's host contract registry before loading scripts:

```rust
let vm = create_vm()?;

vm.registry()
    .typed_function::<FindUser>()?
    .typed_callback::<UserFound>()?;
```

That registry is the source of truth for:

- runtime host bridge routing
- generated `.d.ts` declarations
- generated TypeScript SDK helpers
- contract ABI cache identity

## Generate The SDK Files

```rust
use rustts::SdkFileNames;

let written = vm.registry().write_sdk_files_with_names(
    "target/generated",
    &SdkFileNames {
        types: "my_sdk.d.ts".into(),
        sdk: "my_sdk.ts".into(),
    },
)?;

assert!(written.types_path.ends_with("my_sdk.d.ts"));
assert!(written.sdk_path.ends_with("my_sdk.ts"));
```

The generated SDK includes:

```ts
user.find({ userId: 7, includeRoles: true });

ctx.on("user.found", event => {
  event.displayName.toUpperCase();
});

events.user.found(event => {
  event.roles.length.toFixed();
});

user.onFound(event => {
  event.roles.map(role => role.toUpperCase());
});
```

For object schemas, the SDK also emits lightweight helpers:

```ts
const input = models.FindUserInput.create({
  userId: 7,
  includeRoles: true,
});

if (FindUserInputModel.is(input)) {
  const wrapped = FindUserInputModel.wrap(input);
  wrapped.toJSON().userId.toFixed();
}
```

## Load And Run A Script

```rust
vm.load_script("user-rules", include_str!("user_rules.ts"))?;

let result = vm.call_function("user-rules", "lookup", Vec::new())?;
println!("{result}");

vm.emit_callback::<UserFound>(&UserFoundPayload {
    user_id: 7,
    display_name: "user-7".into(),
    roles: vec!["admin".into(), "editor".into()],
})?;

vm.shutdown()?;
```

And the script:

```ts
let lastFound = "none";

user.onFound(event => {
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

## Validation Policy

Runtime contract validation is configurable. The default is permissive. Use
strict unknown-field rejection when you want scripts to fail fast on extra input
fields.

```rust
use rustts::{VmContractValidation, VmUnknownFieldValidation, VmOptions};

let mut options = VmOptions::default();
options.contract_validation = VmContractValidation::Inputs;
options.unknown_field_validation = VmUnknownFieldValidation::Reject;
```

That is useful for SDK development, tests, and modding APIs where clear error
messages matter more than accepting loose payloads.

## What To Remember

- Register contracts before loading scripts.
- Prefer `typed_function` and `typed_callback` when payload types implement
  `TsSchema`.
- Keep event names stable; they become part of the generated TypeScript API.
- Use `user.onFound(...)`-style aliases for friendly game SDKs, and keep
  `ctx.on("user.found", ...)` available for low-level dynamic cases.
