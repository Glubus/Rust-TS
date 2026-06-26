(() => {
  const handlers = globalThis.__vm_handlers;
  if (!handlers || typeof handlers !== "object") {
    return "[]";
  }
  return JSON.stringify(Object.keys(handlers));
})()
