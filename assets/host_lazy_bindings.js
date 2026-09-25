(() => {
  // [contract name, returns a Promise] for every host function contract.
  const contracts = __contracts__;
  const roots = Object.create(null);
  const cache = new Map();

  for (const [contractName, returnsPromise] of contracts) {
    let children = roots;
    let node;
    for (const segment of contractName.split(".")) {
      node = children[segment] ??
        (children[segment] = { contractName: undefined, returnsPromise: false, children: Object.create(null) });
      children = node.children;
    }
    node.contractName = contractName;
    node.returnsPromise = returnsPromise;
  }

  const hostPayload = (args) => {
    const payload = args.length <= 1 ? args[0] : args;
    return payload === undefined ? null : payload;
  };

  const invokeHost = (node, args) => {
    const payload = hostPayload(args);
    if (node.returnsPromise) {
      return globalThis.__host
        .callAsync(node.contractName, JSON.stringify(payload))
        .then((json) => JSON.parse(json));
    }
    return globalThis.__host.callValue(node.contractName, payload);
  };

  const materialize = (path, node) => {
    if (cache.has(path)) {
      return cache.get(path);
    }

    const target = node.contractName ? function (...args) {
      return invokeHost(node, args);
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
        return invokeHost(node, args);
      },
    });

    cache.set(path, proxy);
    return proxy;
  };

  for (const [name, node] of Object.entries(roots)) {
    globalThis[name] = materialize(name, node);
  }
})();
