type HostEventHandler<K extends keyof HostEvents> = (
  payload: HostEvents[K],
) => void | Promise<void>;

type HostEventContext = {
  on<K extends keyof HostEvents>(
    event: K,
    handler: HostEventHandler<K>,
  ): void;
};
