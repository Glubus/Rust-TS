/** State a script keeps across hot reloads, through `ctx.hot`. */
type HostHotContext = {
  /**
   * What the previous version's `save` callback returned, set before this version's
   * code runs; `undefined` on a first load. It is the same object, not a copy.
   */
  readonly data: unknown;
  /**
   * Registers the callback that runs on this version when a reload replaces it; its
   * result becomes the new version's `ctx.hot.data`. It runs before the new version
   * is known to load, so it must not have side effects. The last registration wins.
   */
  save(snapshot: () => unknown): void;
  /**
   * Registers a cleanup that runs on this version once a new version has loaded, or
   * when the script is unloaded. Cleanups run in registration order.
   */
  dispose(cleanup: () => void): void;
};
