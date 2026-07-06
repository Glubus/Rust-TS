import { current } from "./value";
import { offset } from "./extra";
import { score } from "test";

score.onUpdate(event => {
  globalThis.lastScore = event.combo + current + offset;
});

export function read() {
  return current + offset;
}

export function readScore() {
  return globalThis.lastScore ?? 0;
}
