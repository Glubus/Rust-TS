import { current } from "./value";

ctx.on("score.update", event => {
  globalThis.lastScore = event.combo + current;
});

export function read() {
  return current;
}

export function readScore() {
  return globalThis.lastScore ?? 0;
}
