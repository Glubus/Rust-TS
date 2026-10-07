# Migrate From 0.3 To 0.4

0.4 changes how host contracts are declared and registered. Scripts keep working;
the Rust side needs edits. Do them in this order: each step fixes a class of
compile errors, and the list at the end covers what the compiler cannot tell you.

## 1. Split `HostFunction`

`HostFunctionSignature` now holds what scripts see (`Input` and `Output`);
`HostFunction` only keeps `call`. A contract implemented by a closure no longer
needs `HostFunction` at all.

```rust
// 0.3
impl HostFunction for FindUser {
    type Input = FindUserInput;
    type Output = FindUserOutput;
    fn call(input: Self::Input) -> Result<Self::Output, VmError> { /* ... */ }
}

// 0.4
impl HostFunctionSignature for FindUser {
    type Input = FindUserInput;
    type Output = FindUserOutput;
}
impl HostFunction for FindUser {
    fn call(input: Self::Input) -> Result<Self::Output, VmError> { /* ... */ }
}
```

The contract type no longer needs `Send + Sync`.

## 2. Drop the JSON registration path

Values cross natively only, and every schema comes from `TsSchema` of the
`Input`, `Output` and `Payload` types. Two things change.

**Rename the registrations.** The `typed_` prefix is gone:

| 0.3 | 0.4 |
| --- | --- |
| `typed_function::<C>()` | `function::<C>()` |
| `typed_callback::<C>()` | `callback::<C>()` |
| `register_typed_function`, `register_typed_callback` on `HostContractRegistry` | `register_function`, `register_callback` |

0.4 adds on top: closure registrations (`function_with`, `function_with_caller`,
`async_function_with`, `async_function_with_caller`, and the matching `register_*`
methods) and `request` for events whose handlers answer. See [Register Host
Functions And Callbacks](register-host-functions.md).

**Stop using the untyped registrations.** The old `function`, `callback` and
their JSON-converting siblings are gone, and so are the hooks that fed them:
`HostFunctionSignature::input_schema`, `output_schema` and `function_descriptor`,
and `HostCallback::payload_schema` and `callback_descriptor`. `Input`, `Output`
and `Payload` no longer need `serde::Serialize` or `DeserializeOwned`.

If a contract declared its schema by hand, give its types a schema instead:

```rust
// 0.3: input_schema() and output_schema() written by hand, values through serde_json
impl HostFunctionSignature for FindUser {
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    fn output_schema() -> Schema {
        Schema::typed("FindUserOutput", TsType::Object(/* ... */))
    }
}

// 0.4: derive the schema on the types the handler really uses
#[derive(TsSchema)]
struct FindUserOutput { /* ... */ }

impl HostFunctionSignature for FindUser {
    type Input = FindUserInput;
    type Output = FindUserOutput;
}
```

The derive needs the `derive` feature and also reads `#[serde(...)]` attributes
(`rename_all`, `flatten`, ...), so existing field renames keep working. A value
with no fixed shape stays `serde_json::Value`, whose schema is `Json` (`unknown` in
TypeScript). A schema for a type that does not declare it can no longer be
written by hand; implement `TsSchema`, `JsDecode` and `JsEncode` for a wrapper type
when you need that.

## 3. Move `schema()` to `HostContext`

`HostContract::schema()` is gone. Functions and callbacks take their schema from
their types, so delete the method from their `HostContract` impls (a hand-written
`Schema` import usually becomes unused). Only a context contract still declares one,
on `HostContext`:

```rust
impl HostContract for Overlay {
    const NAME: &'static str = "overlay";
    fn kind() -> HostContractKind { HostContractKind::Context }
}

impl HostContext for Overlay {
    fn schema() -> Schema { /* what scripts see as `overlay` */ }
}
```

`HostContract::descriptor()` now carries a placeholder schema until a registration
fills it.

## 4. Custom registries

A type implementing `HostContractRegistry` keeps `register_function` and
`register_callback`, now with the `TsSchema` and codec bounds the `typed_` versions
had, and drops `register_typed_function` and `register_typed_callback`. It must also
provide the new `register_function_with`, `register_function_with_caller`,
`register_async_function_with`, `register_async_function_with_caller` and
`register_request`.

## 5. Descriptors and generated text

- `HostFunctionDescriptor` and `HostContractAbi::Function` gain `returns_promise`;
  `HostCallbackDescriptor` gains `reply_schema` and `HostContractAbi::Callback`
  gains `reply`. Add them to struct literals and patterns that list every field.
  Serialized descriptors omit them when unset, so older descriptor JSON still loads.
- The generated declarations and SDK always declare and export `ctx` (typed with
  `ctx.hot`), even without host events. With events they also declare
  `HostReplies`, `HostEventReply`, `HostEventHandler` and `HostEventContext`, and
  `ctx` gains `ctx.off`. Async host functions render as `Promise<Output>`. Update
  snapshots of the generated text.
- Regenerate the `.d.ts` and SDK files you ship with your scripts.

## 6. Errors and reloads

- `VmError` is `#[non_exhaustive]`: add a wildcard arm to exhaustive matches.
- `VmError::Execution` stacks name TypeScript locations (`lib/math.ts:8:15`,
  `<id>.ts:3:5`) instead of `rustts://graph/{n}/{path}:{line}:{col}`, and
  `VmError::Transpile` lists `path:line:column: message` diagnostics. Update code
  that parses either. Transpile cache artifacts are rebuilt once.
- `ReloadReport` has a new `dispose_failed` field; add it to struct literals and
  patterns that list every field.
- A dynamic `import()` now fails with `<path>: dynamic import() is not supported;
  use a static import` instead of `dynamic import is not supported in V0 module
  graphs`; update code that matches on the old text.

## 7. Contract validation

Validation still checks inputs and outputs against the declared schema, but now on
a JSON snapshot of the converted value. One behaviour changed: an async host
function that resolves with an invalid output no longer gets
`VmError::ContractValidation` from `HostResolver::resolve`. The engine finds the
problem when it converts the output on its thread, and the script's Promise
rejects with `output validation failed`. Synchronous functions and every input
check behave as before.

## 8. Script-visible changes

- Scripts get `console`, `setTimeout`, `setInterval`, `clearTimeout` and
  `clearInterval`, installed before their code runs. A script that defines its own
  keeps working, since its definition runs after; one that tested for their absence
  now finds them.
- `ctx.on` misuse throws `TypeError: ctx.on expects ...` instead of
  `__rustts_on expects ...`.
- The `__vm_handlers` global is gone. Handlers change only through `ctx.on` and
  `ctx.off`.
- Encoders define each field or key as an own data property, as `JSON.parse` does:
  derived structs (`#[derive(TsSchema)]`), maps, `serde_json::Value`,
  `#[serde(flatten)]` fields and internally tagged newtype variants. A setter a
  script put on `Object.prototype` no longer runs, and a field or key named
  `__proto__` becomes an ordinary own property.
- Packages in `node_modules` that ship both builds resolve to their ESM entry (the
  `exports` `import` condition, or the `module` field) instead of `main`. Check that
  the scripts still work with the ESM build of each such dependency.

## 9. Options

`VmOptions` has a new `builtins` field (see [Script
Built-ins](engine.md#script-built-ins)); add it to struct literals that list every
field. The default keeps every built-in on, as before.

## Check Your Migration

1. `cargo build` with the `derive` feature; fix the errors in the order above.
2. Run your tests with contract validation on
   (`VmOptions { contract_validation: VmContractValidation::InputsAndOutputs, .. }`)
   at least once: a schema that used to be hand-written may now describe the type
   more strictly or more loosely than before.
3. Regenerate the SDK files and run `tsc --noEmit` on your scripts.
