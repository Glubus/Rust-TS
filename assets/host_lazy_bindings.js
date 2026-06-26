(() => {
  const contracts = __contracts__;
  const bridgeMethod = __bridge_method__;
  const returnsPromise = __returns_promise__;
  const roots = Object.create(null);
  const cache = new Map();

  for (const contractName of contracts) {
    let node = roots;
    const segments = contractName.split(".");
    for (const segment of segments) {
      node[segment] = node[segment] ?? { contractName: undefined, children: Object.create(null) };
      node = node[segment].children;
    }

    let leaf = roots;
    for (const segment of segments) {
      leaf = leaf[segment].children;
    }
    const leafName = segments[segments.length - 1];
    let parent = roots;
    for (const segment of segments.slice(0, -1)) {
      parent = parent[segment].children;
    }
    parent[leafName].contractName = contractName;
  }

  const hostPayload = (args) => {
    const payload = args.length <= 1 ? args[0] : args;
    return JSON.stringify(payload === undefined ? null : payload);
  };

  const invokeHost = (contractName, args) => {
    const response = globalThis.__host[bridgeMethod](contractName, hostPayload(args));
    if (returnsPromise) {
      return response.then((json) => JSON.parse(json));
    }
    return JSON.parse(response);
  };

  const materialize = (path, node) => {
    if (cache.has(path)) {
      return cache.get(path);
    }

    const target = node.contractName ? function (...args) {
      return invokeHost(node.contractName, args);
    } : {};

    const proxy = new Proxy(target, {
      get(currentTarget, property, receiver) {
        if (property === "then" && !node.contractName) {
          return undefined;
        }
        if (property in currentTarget) {
          return Reflect.get(currentTarget, property, receiver);
        }
        if (typeof property !== "string") {
          return undefined;
        }
        const child = node.children[property];
        if (!child) {
          return undefined;
        }
        return materialize(path ? `${path}.${property}` : property, child);
      },
      apply(_target, _thisArg, args) {
        if (!node.contractName) {
          throw new TypeError(`host namespace ${path} is not callable`);
        }
        return invokeHost(node.contractName, args);
      },
    });

    cache.set(path, proxy);
    return proxy;
  };

  for (const [name, node] of Object.entries(roots)) {
    globalThis[name] = materialize(name, node);
  }
})();
