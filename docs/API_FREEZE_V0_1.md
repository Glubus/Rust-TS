# V0.1 API Freeze Notes

V0.1 should stabilize the current Rust-first host contract model instead of
adding new public surfaces speculatively.

## Frozen Names

These names are the preferred V0.1 spelling for external usage:

- `InMemoryHostContractRegistry::typed_function::<T>()`
- `InMemoryHostContractRegistry::typed_callback::<T>()`
- `HostContractRegistry::register_typed_function::<T>()`
- `HostContractRegistry::register_typed_callback::<T>()`
- `TsSchema::schema()`
- `Schema::validate_json(...)`
- `Schema::validate_json_strict(...)`
- generated `<TypeName>Model`
- generated `<TypeName>Model::is(...)` in TypeScript output
- generated `<TypeName>Model::wrap(...)` in TypeScript output
- generated `models.Type.create(...)`
- generated `models.Type.is(...)`
- generated `models.Type.wrap(...)`

## Frozen Behaviors

- `typed_function` derives function input and output schemas from
  `HostFunction::Input: TsSchema` and `HostFunction::Output: TsSchema`.
- `typed_callback` derives callback payload schema from
  `HostCallback::Payload: TsSchema`.
- Schema validation stays permissive by default.
- Strict unknown-field rejection remains opt-in through
  `VmUnknownFieldValidation`.
- Generated SDK object models are a cold-path TypeScript convenience layer above
  the stable registry and host bridge metadata.
- `HostContext` remains declarative for V0.1 and does not install mutable Rust
  context objects into QuickJS.

## Change Rule

New public names or SDK surface should come from a real host integration gap.
The first response to friction should be a dogfood test or example, not a new
abstraction.
