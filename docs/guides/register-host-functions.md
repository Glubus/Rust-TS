# Register Host Functions And Callbacks

This guide shows the normal flow:

1. Create an `Engine`.
2. Declare Rust payload structs.
3. Derive `TsSchema`.
4. Implement `HostFunction` or `HostCallback`.
5. Register contracts with `typed_function` and `typed_callback`.
6. Generate declaration and SDK files for your package.
7. Load scripts that call your functions and handle your events.

## Install With Derive Support

```toml
[dependencies]
rustts = { version = "0.3", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
```

## Create The Engine

```rust
use rustts::{Engine, VmError, VmOptions};

fn create_engine() -> Result<Engine, VmError> {
    Engine::new(&VmOptions {
        cache_dir: Some("target/rustts-cache".into()),
        ..VmOptions::default()
    })
}
```

The cache directory stores transpiled JavaScript artifacts, so reloads and later
runs reuse them. Without it (the default), every load transpiles in memory.

## Declare A Host Function

Host functions are Rust calls exposed to TypeScript. The contract name controls
the generated TypeScript namespace. For example, `user.find` becomes
`user.find(...)`.

```rust
use serde::{Deserialize, Serialize};
use rustts::{
    HostContract, HostContractKind, HostFunction, HostFunctionSignature, Schema, TsSchema, VmError,
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

impl HostFunctionSignature for FindUser {
    type Input = FindUserInput;
    type Output = FindUserOutput;
}

impl HostFunction for FindUser {
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

`HostFunctionSignature` is what scripts see: the input and output types.
`HostFunction` adds the static `call` that implements it.

## Implement A Host Function With A Closure

A contract that only implements `HostFunctionSignature` is registered together
with its handler, a closure that can hold state. Here `SpawnEnemy` is declared like
`FindUser` above, without the `HostFunction` impl, and `world` is a
`Send + Sync` handle to game state:

```rust
engine
    .registry()
    .typed_function_with::<SpawnEnemy>(move |input| world.spawn(input.kind, input.position))?;
```

The generated TypeScript is the same as for a static function. The closure must be
`Send + Sync + 'static` because the registry is; `function_with` is the untyped
counterpart of `function`.

### Know The Calling Script

`typed_function_with_caller` (or `function_with_caller`) hands the handler a
`Caller` too: `caller.script_id()` is the id the calling script was loaded under,
kept across reloads. Use it to scope a mod's permissions, storage or logs:

```rust
engine
    .registry()
    .typed_function_with_caller::<SaveSetting>(move |caller, input| {
        settings.save(caller.script_id(), &input.key, input.value)
    })?;
```

## Declare A Callback

Callbacks are events the host emits to scripts. The contract name is the event
name scripts subscribe to with `ctx.on("user.found", ...)`; the generated SDK adds
the ergonomic alias `user.onFound(...)`.

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

## Declare A Request

A request is a callback whose handlers answer: `Engine::request` returns what each
handler returned. Declare the callback like `UserFound`, here `MenuLabel` named
`menu.label` with a `String` payload, implement `HostRequest` on top of
`HostCallback`, and register it with `typed_request` so the generated TypeScript
types the handlers' return value:

```rust
use rustts::HostRequest;

impl HostRequest for MenuLabel {
    type Reply = String;
}

engine.registry().typed_request::<MenuLabel>()?;

let labels: Vec<(&str, String)> = engine.request("menu.label", "save")?;
```

Each reply comes with the id of the script that gave it. An `async` handler's
Promise is awaited; see [Requests](engine.md#requests).

## Register Contracts In The Registry

Register contracts in the engine's registry before loading the scripts that use
them:

```rust
let mut engine = create_engine()?;

engine
    .registry()
    .typed_function::<FindUser>()?
    .typed_callback::<UserFound>()?;
```

That registry is the source of truth for:

- the host functions installed in every script
- generated `.d.ts` declarations
- generated TypeScript SDK helpers
- the cache key of transpiled scripts

Host functions are synchronous: a script's call to `user.find(...)` runs the Rust
`call` on the engine's thread and returns its value directly. An `Err` returned by
the handler is thrown in the script as an exception the script can catch.

## Generate The SDK Files

```rust
use rustts::SdkFileNames;

let written = engine.registry().write_sdk_files_with_names(
    "target/generated",
    &SdkFileNames {
        types: "my_sdk.d.ts".into(),
        sdk: "my_sdk.ts".into(),
    },
)?;

assert!(written.types_path.ends_with("my_sdk.d.ts"));
assert!(written.sdk_path.ends_with("my_sdk.ts"));
```

The generated SDK is a TypeScript module that exposes:

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

Scripts reach host functions through namespaced globals such as `user.find(...)`
and subscribe to events with `ctx.on(...)`, without an import:

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

Rust loads the script, calls its exports and emits events to it:

```rust
engine.load_script("user-rules", include_str!("user_rules.ts"))?;

let result: String = engine.call("user-rules", "lookup", ())?;
assert_eq!(result, "user-7:2:true");

engine.emit(
    UserFound::NAME,
    &UserFoundPayload {
        user_id: 7,
        display_name: "user-7".into(),
        roles: vec!["admin".into(), "editor".into()],
    },
)?;

let observed: String = engine.call("user-rules", "observed", ())?;
assert_eq!(observed, "user-7:admin,editor");
```

To use the SDK helpers (`user.onFound`, `events`, `models`) in an inline script,
load the SDK source followed by the script:

```rust
let sdk = engine.registry().sdk()?;
engine.load_script("user-rules", &format!("{sdk}\n{}", include_str!("user_rules.ts")))?;
```

A project can import the generated SDK file instead, for example
`import { user } from "./generated/my_sdk";`.

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
  `TsSchema`; use the `_with` variants when the handler needs state, and the
  `_with_caller` ones when it needs to know which script called.
- Keep event names stable; they become part of the generated TypeScript API.
- Use `user.onFound(...)`-style aliases for friendly game SDKs, and keep
  `ctx.on("user.found", ...)` / `ctx.off(...)` available for low-level dynamic cases.
