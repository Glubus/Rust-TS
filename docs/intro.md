---
title: Welcome
sidebar_position: 1
---

# ts-embed-vm

`ts-embed-vm` lets a Rust application embed TypeScript scripts, expose Rust host
APIs to those scripts, and generate TypeScript declarations plus a small SDK from
the Rust-side contract registry.

The short version:

- Rust owns the truth: host functions, callbacks, and context metadata are
  declared in Rust.
- Scripts call generated helpers such as `user.find(...)`.
- Scripts subscribe to callbacks with `ctx.on("user.found", ...)`, generated
  event wrappers such as `events.user.found(...)`, or friendly domain aliases
  such as `user.onFound(...)`.
- The generated files are regular TypeScript: `tsvm.d.ts` and `tsvm.sdk.ts`.

## What You Can Build

This is a good fit for game scripting, modding tools, editor automation,
simulation rules, or plugin-like systems where the host is Rust and scripts are
TypeScript.

Example script:

```ts
let lastUser = "none";

user.onFound(event => {
  lastUser = `${event.displayName}:${event.roles.join(",")}`;
});

export function lookup() {
  const result = user.find({ userId: 7, includeRoles: true });
  return `${result.displayName}:${result.roles.length}`;
}
```

## Current V0 Boundaries

V0 intentionally keeps the core small:

- imports must be statically discoverable; dynamic `import(...)` is rejected
- `HostContext` is declarative metadata only
- the normal host bridge is synchronous
- `async-promise` is experimental and uses the async worker-lane path

Start with synchronous host functions and typed callbacks. They are the stable
path today.
