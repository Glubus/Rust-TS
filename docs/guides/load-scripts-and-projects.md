# Load Scripts And Projects

`RustTS` loads TypeScript code into a long-lived QuickJS worker. After a
script is loaded, Rust calls exported TypeScript functions by:

- `script_id`: the host application's stable name for that loaded script
- export name: the TypeScript function exported by the script

`script_id` is not a file path and not a package name. It is an id chosen by
your application. Good examples:

- `weather-rules`
- `mod:zoro_moveset`
- `tenant-42-policy`

Bad examples:

- `plugin`, unless your app really has only one generic plugin
- a random UUID if you want reloads and diagnostics to stay readable

## Single Script Source

Use `load_script` when you already have one TypeScript source string.

```rust
let script_id = "weather-rules";

vm.load_script(
    script_id,
    r#"
export function greet(input) {
  return `hello ${input.name}`;
}
"#,
)?;

let output = vm.call_function(
    script_id,
    "greet",
    vec![serde_json::json!({ "name": "Ada" })],
)?;
```

What the arguments mean:

- `script_id`: `"weather-rules"` is the runtime id for this loaded script.
- `"greet"`: exported TypeScript function name.
- `vec![...]`: JSON arguments passed to the exported function.

The TypeScript source must export the function Rust calls:

```ts
export function greet(input) {
  return `hello ${input.name}`;
}
```

## Project Entrypoint

Use `load_script_project` when the script lives on disk and imports other local
files.

```rust
let script_id = "zoro-moveset";
let entrypoint = "mods/zoro_moveset/main.ts";

vm.load_script_project(script_id, entrypoint)?;
```

Example project:

```text
mods/zoro_moveset/
  main.ts
  math.ts
  tsconfig.json
```

```ts
// main.ts
import { bonus } from "./math";

export function run() {
  return bonus(40) + 2;
}
```

Then Rust calls:

```rust
let output = vm.call_function("zoro-moveset", "run", Vec::new())?;
```

## `load_script` vs `load_script_project`

Use `load_script` when:

- your host already has the TypeScript source in memory
- the script is a single file
- you do not need local imports

Use `load_script_project` when:

- the entry file is on disk
- the script imports local modules
- you want `tsconfig.json` `baseUrl` / `paths`
- you want package imports from project-local `node_modules`

## Supported Project Imports

Project mode supports:

- static relative ESM imports and re-exports
- named, default, and namespace imports
- extensionless local resolution
- `index.*` resolution
- `tsconfig.json` `baseUrl` and `paths`
- package imports from project-local `node_modules`
- type-only imports and re-exports, ignored at runtime

Dynamic `import(...)` is rejected because the runtime cache is built from a
static module graph.

## Reloading

Loading the same `script_id` again replaces the mounted script:

```rust
vm.load_script_project("zoro-moveset", "mods/zoro_moveset/main.ts")?;
```

Use this primitive to build your own watcher or hot-reload tool outside
`RustTS`. The VM keeps the runtime registry consistent; your application
decides when files should be reloaded.
