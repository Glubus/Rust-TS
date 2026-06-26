type HostEventHandler<K extends keyof HostEvents> = (
  payload: HostEvents[K],
) => void | Promise<void>;

type HostEventContext = {
  on<K extends keyof HostEvents>(
    event: K,
    handler: HostEventHandler<K>,
  ): void;
};

const __ctx = (globalThis as unknown as { ctx: HostEventContext }).ctx;

export const ctx = __ctx;
