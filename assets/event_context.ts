/**
 * What a handler of event `K` returns: the reply type of a request registered with
 * `request`, nothing for a plain callback. An `async` handler returns a
 * Promise of it.
 */
type HostEventReply<K extends keyof HostEvents> = K extends keyof HostReplies
  ? HostReplies[K] | Promise<HostReplies[K]>
  : void | Promise<void>;

type HostEventHandler<K extends keyof HostEvents> = (
  payload: HostEvents[K],
) => HostEventReply<K>;

type HostEventContext = {
  /** Registers `handler` for `event`; handlers run in registration order. */
  on<K extends keyof HostEvents>(event: K, handler: HostEventHandler<K>): void;
  /**
   * Removes the first registration of `handler` for `event`; does nothing when it is
   * not registered. A delivery already in progress still runs it.
   */
  off<K extends keyof HostEvents>(event: K, handler: HostEventHandler<K>): void;
};
