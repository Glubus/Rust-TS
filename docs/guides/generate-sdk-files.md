# Generate TypeScript SDK Files

`RustTS` can generate TypeScript files from the Rust-side registry. This is
how a host application exposes a typed scripting API to users.

## Register Contracts First

The registry is the source of truth. Register every host function and callback
before generating files.

```rust
let vm = RustTs::new(options)?;

vm.registry()
    .typed_function::<FindUser>()?
    .typed_callback::<UserFound>()?;
```

## Write Files

```rust
use rustts::SdkFileNames;

let written = vm.registry().write_sdk_files_with_names(
    "generated",
    &SdkFileNames {
        types: "my_sdk.d.ts".into(),
        sdk: "my_sdk.ts".into(),
    },
)?;

assert!(written.types_path.ends_with("my_sdk.d.ts"));
assert!(written.sdk_path.ends_with("my_sdk.ts"));
```

This creates:

```text
generated/
  my_sdk.d.ts
  my_sdk.ts
```

`write_sdk_files(...)` also exists for quick experiments, but real applications
should usually call `write_sdk_files_with_names(...)` and choose names that
match their package.

File names do not define the import name by themselves. The import name comes
from each Rust contract's `IMPORT_MODULE`.

```rust
impl HostContract for FindUser {
    const NAME: &'static str = "user.find";
    const IMPORT_MODULE: &'static str = "my_sdk";
    const EXPORT_PATH: &'static [&'static str] = &["user", "find"];

    fn schema() -> Schema {
        FindUserInput::schema()
    }

    fn kind() -> HostContractKind {
        HostContractKind::Function
    }
}
```

Then a script can use:

```ts
import { user } from "my_sdk";

const result = user.find({ userId: 7, includeRoles: true });
```

Use the same `IMPORT_MODULE` for every contract that should be exported from the
same virtual SDK module.

## What The Declaration File Contains

The declaration file contains the public TypeScript API:

- host function input and output types
- callback payload types
- global helpers such as `ctx.on(...)`
- generated namespaces such as `user.find(...)`
- event maps for typed callback subscriptions

Use it from a `tsconfig.json` with `include` or `typeRoots`, depending on how
your application packages scripts.

## What The SDK Source File Contains

The SDK file contains runtime helper code. Scripts can import or bundle it when
the host application wants an explicit module, or the host can inject helpers
globally through the VM's generated bootstrap path.

For a Rust contract named `user.find`, the SDK exposes:

```ts
const result = user.find({ userId: 7, includeRoles: true });
```

For a callback named `user.found`, it exposes:

```ts
ctx.on("user.found", event => {
  event.displayName.toUpperCase();
});

user.onFound(event => {
  event.roles.length;
});
```

## Application Tooling

Most applications should wrap this in their own CLI. A game SDK might expose:

```text
my-sdk init my-mod
my-sdk generate-sdk
my-sdk check
my-sdk build
```

That CLI can call `write_sdk_files_with_names(...)`, create `tsconfig.json`, and
decide where generated files live. `RustTS` only owns the
registry-to-TypeScript generation.
