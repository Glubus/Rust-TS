// Installed in every script context, once `globalThis.__rustts_native` holds the
// native host functions and `__rustts_listen` records the events the context listens
// to. One script instead of two: each evaluation in a fresh context pays its own
// parse and compile.
(() => {
  // Event handlers: `ctx.on(event, handler)`; the engine reads `__vm_handlers` and
  // only visits contexts whose events `listen` recorded.
  const listen = globalThis.__rustts_listen;
  delete globalThis.__rustts_listen;
  const handlers = Object.create(null);
  globalThis.__rustts_on = (eventName, handler) => {
    if (typeof eventName !== "string") {
      throw new TypeError("__rustts_on expects a string event name");
    }
    if (typeof handler !== "function") {
      throw new TypeError("__rustts_on expects a function handler");
    }
    const list = handlers[eventName] ?? (handlers[eventName] = []);
    list.push(handler);
    listen(eventName);
  };
  globalThis.ctx = {
    on: globalThis.__rustts_on,
  };
  globalThis.__vm_handlers = handlers;

  // Host functions, reachable three ways that all end in the same native function:
  // host module imports, namespaced globals (`user.find(...)`) and the `__host`
  // bridge used by the generated SDK.
  const native = globalThis.__rustts_native;

  const hostFunction = (name) => {
    const fn = native[name];
    if (typeof fn !== "function") {
      throw new Error(`missing host function: ${name}`);
    }
    return fn;
  };

  globalThis.__host = {
    callValue: (name, input) => hostFunction(name)(input === undefined ? null : input),
  };

  const namespaceChild = (parent, segment) => {
    const existing = parent[segment];
    if (existing !== null && (typeof existing === "object" || typeof existing === "function")) {
      return existing;
    }
    return (parent[segment] = {});
  };

  for (const name of Object.keys(native)) {
    const segments = name.split(".");
    const leaf = segments.pop();
    let parent = globalThis;
    for (const segment of segments) {
      parent = namespaceChild(parent, segment);
    }
    parent[leaf] = native[name];
  }
})();
