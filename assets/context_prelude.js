// Installed in every script context, once `globalThis.__rustts_native` holds the
// native host functions, `__rustts_listen` records the events the context listens
// to and, on a reload, `__rustts_hot_data` holds the previous version's saved state.
// One script instead of two: each evaluation in a fresh context pays its own parse
// and compile.
(() => {
  // Hot reload: `ctx.hot.data` is what the previous version's `save` returned, which
  // the engine hands over in `__rustts_hot_data`. The engine only reads the locked
  // `__rustts_hot` hooks when it replaces or unloads this context.
  function hotReload() {
    const data = globalThis.__rustts_hot_data;
    delete globalThis.__rustts_hot_data;
    let save = () => undefined;
    const disposers = [];
    const expectFunction = (name, fn) => {
      if (typeof fn !== "function") {
        throw new TypeError(`ctx.hot.${name} expects a function`);
      }
    };
    Object.defineProperty(globalThis, "__rustts_hot", {
      value: Object.freeze({
        save: () => save(),
        // Every disposer runs even after one throws; the first error is rethrown.
        dispose: () => {
          let failed = false;
          let failure;
          for (const disposer of disposers) {
            try {
              disposer();
            } catch (error) {
              if (!failed) {
                failed = true;
                failure = error;
              }
            }
          }
          if (failed) {
            throw failure;
          }
        },
      }),
    });
    return {
      data,
      save(fn) {
        expectFunction("save", fn);
        save = fn;
      },
      dispose(fn) {
        expectFunction("dispose", fn);
        disposers.push(fn);
      },
    };
  }

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
    hot: hotReload(),
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
