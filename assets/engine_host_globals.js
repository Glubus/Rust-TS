// Exposes the native host functions in `globalThis.__rustts_native` the two other ways
// scripts reach them: namespaced globals (`user.find(...)`) and the `__host` bridge
// used by the generated SDK. Every path ends in the same native function.
(() => {
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
    call: (name, inputJson) => {
      const output = hostFunction(name)(JSON.parse(inputJson));
      return JSON.stringify(output === undefined ? null : output);
    },
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
