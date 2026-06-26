(() => {
  const eventName = __EVENT_NAME__;
  const payload = __EVENT_PAYLOAD__;
  const handlers = globalThis.__vm_handlers?.[eventName];
  if (!Array.isArray(handlers) || handlers.length === 0) {
    return 0;
  }
  for (const handler of handlers) {
    handler(payload);
  }
  return handlers.length;
})()
