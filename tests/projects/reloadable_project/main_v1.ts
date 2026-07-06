import { current } from "./value";
import { score } from "test";

score.onUpdate(event => {
  globalThis.lastScore = event.combo + current;
});

export function read() {
  return current;
}

export function readScore() {
  return globalThis.lastScore ?? 0;
}
