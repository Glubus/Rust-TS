type HostBridge = {
  call(name: string, inputJson: string): string;
  callValue?: (name: string, input: unknown) => unknown;
  callAsync?: (name: string, inputJson: string) => Promise<string>;
};

const __host = (globalThis as unknown as { __host: HostBridge }).__host;

function __hostInput(input?: unknown): string {
  return JSON.stringify(input === undefined ? null : input);
}

function __hostCall<T>(name: string, input?: unknown): T {
  if (__host.callValue) {
    return __host.callValue(name, input === undefined ? null : input) as T;
  }

  return JSON.parse(__host.call(name, __hostInput(input))) as T;
}

async function __hostCallAsync<T>(name: string, input?: unknown): Promise<T> {
  if (!__host.callAsync) {
    throw new Error(`host function ${name} requires an async host bridge`);
  }

  return JSON.parse(await __host.callAsync(name, __hostInput(input))) as T;
}
