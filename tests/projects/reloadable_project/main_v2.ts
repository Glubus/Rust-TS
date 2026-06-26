import { current } from "./value";
import { offset } from "./extra";

ctx.on("score.update", event => {
  globalThis.lastScore = event.combo + current + offset;
});

export function read() {
  return current + offset;
}

export function readScore() {
  return globalThis.lastScore ?? 0;
}
