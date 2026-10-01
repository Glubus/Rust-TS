// Installed in every script context, once `globalThis.__rustts_native` holds the
// native host functions, `__rustts_handlers` keeps the engine's copy of each event's
// handler list and, on a reload, `__rustts_hot_data` holds the previous version's
// saved state.
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

  // Event handlers: `ctx.on(event, handler)` / `ctx.off(event, handler)`. Every change
  // hands the event's list to the engine through `setHandlers` (`undefined` once it
  // has no handler), which keeps a snapshot of its functions: delivering an event
  // looks nothing up by name, skips contexts that do not listen to it, and a
  // delivery in progress keeps the snapshot it took.
  const setHandlers = globalThis.__rustts_handlers;
  delete globalThis.__rustts_handlers;
  const handlers = new Map();
  const expectHandler = (method, eventName, handler) => {
    if (typeof eventName !== "string") {
      throw new TypeError(`${method} expects a string event name`);
    }
    if (typeof handler !== "function") {
      throw new TypeError(`${method} expects a function handler`);
    }
  };
  globalThis.__rustts_on = (eventName, handler) => {
    expectHandler("ctx.on", eventName, handler);
    let list = handlers.get(eventName);
    if (list === undefined) {
      list = [];
      handlers.set(eventName, list);
    }
    list.push(handler);
    setHandlers(eventName, list);
  };
  const off = (eventName, handler) => {
    expectHandler("ctx.off", eventName, handler);
    const list = handlers.get(eventName);
    const index = list === undefined ? -1 : list.indexOf(handler);
    if (index < 0) {
      return;
    }
    list.splice(index, 1);
    if (list.length === 0) {
      handlers.delete(eventName);
      setHandlers(eventName, undefined);
    } else {
      setHandlers(eventName, list);
    }
  };
  globalThis.ctx = {
    on: globalThis.__rustts_on,
    off,
    hot: hotReload(),
  };

  // `console`: each call joins its arguments into one message for the host sink.
  const writeConsole = globalThis.__rustts_console;
  delete globalThis.__rustts_console;
  const describe = (value) => {
    if (typeof value === "string") {
      return value;
    }
    if (value instanceof Error) {
      const stack = typeof value.stack === "string" ? value.stack.trimEnd() : "";
      return stack === "" ? `${value.name}: ${value.message}` : `${value.name}: ${value.message}\n${stack}`;
    }
    try {
      const json = JSON.stringify(value);
      if (json !== undefined) {
        return json;
      }
    } catch {
      // Cycles and BigInt have no JSON form.
    }
    return String(value);
  };
  const consoleMethod = (level) => (...args) => {
    writeConsole(level, args.map(describe).join(" "));
  };
  globalThis.console = {
    debug: consoleMethod("debug"),
    log: consoleMethod("log"),
    info: consoleMethod("info"),
    warn: consoleMethod("warn"),
    error: consoleMethod("error"),
  };

  // Timers on the engine clock, which only the host's `advance_timers` moves. The
  // engine enters this context only once `schedule` says a timer is due.
  const now = globalThis.__rustts_now;
  const schedule = globalThis.__rustts_schedule;
  delete globalThis.__rustts_now;
  delete globalThis.__rustts_schedule;
  const timers = new Map();
  let nextTimerId = 1;
  let earliest = Infinity;
  const reschedule = () => {
    let due = Infinity;
    for (const timer of timers.values()) {
      if (timer.due < due) {
        due = timer.due;
      }
    }
    earliest = due;
    schedule(due);
  };
  const addTimer = (name, callback, delay, args, repeats) => {
    if (typeof callback !== "function") {
      throw new TypeError(`${name} expects a function callback`);
    }
    const wait = Math.max(0, Number(delay) || 0);
    const id = nextTimerId++;
    const timer = { id, callback, args, due: now() + wait, interval: repeats ? wait : undefined };
    timers.set(id, timer);
    if (timer.due < earliest) {
      earliest = timer.due;
      schedule(earliest);
    }
    return id;
  };
  const clearTimer = (id) => {
    const timer = timers.get(id);
    if (timer !== undefined) {
      timers.delete(id);
      if (timer.due === earliest) {
        reschedule();
      }
    }
  };
  globalThis.setTimeout = (callback, delay, ...args) =>
    addTimer("setTimeout", callback, delay, args, false);
  globalThis.setInterval = (callback, delay, ...args) =>
    addTimer("setInterval", callback, delay, args, true);
  globalThis.clearTimeout = clearTimer;
  globalThis.clearInterval = clearTimer;
  Object.defineProperty(globalThis, "__rustts_timers", {
    value: Object.freeze({
      // Fires the timers due at `time` in due order, then creation order; every one
      // runs even after one throws, and the first error is rethrown.
      run: (time) => {
        let due;
        for (const timer of timers.values()) {
          if (timer.due <= time) {
            if (due === undefined) {
              due = [timer];
            } else {
              due.push(timer);
            }
          }
        }
        if (due === undefined) {
          reschedule();
          return;
        }
        if (due.length > 1) {
          due.sort((left, right) => left.due - right.due || left.id - right.id);
        }
        let failed = false;
        let failure;
        for (const timer of due) {
          if (timers.get(timer.id) !== timer) {
            continue; // cleared by a callback that ran before it
          }
          if (timer.interval === undefined) {
            timers.delete(timer.id);
          } else {
            timer.due += timer.interval;
          }
          try {
            if (timer.args.length === 0) {
              timer.callback();
            } else {
              timer.callback(...timer.args);
            }
          } catch (error) {
            if (!failed) {
              failed = true;
              failure = error;
            }
          }
        }
        reschedule();
        if (failed) {
          throw failure;
        }
      },
    }),
  });

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
