(() => {
  const __vmHandlers = Object.create(null);
  globalThis.__rustts_on = (eventName, handler) => {
    if (typeof eventName !== "string") {
      throw new TypeError("__rustts_on expects a string event name");
    }
    if (typeof handler !== "function") {
      throw new TypeError("__rustts_on expects a function handler");
    }
    const handlers = __vmHandlers[eventName] ?? (__vmHandlers[eventName] = []);
    handlers.push(handler);
  };
  globalThis.ctx = {
    on: globalThis.__rustts_on,
  };
  globalThis.__vm_handlers = __vmHandlers;
})();
