import { score } from "test";

score.onUpdate(event => {
  globalThis.lastScore = event.combo;
});

score.onUpdate(event => {
  globalThis.lastScoreDoubled = event.combo * 2;
});

export function readScore() {
  return globalThis.lastScore ?? 0;
}

export function readScoreDoubled() {
  return globalThis.lastScoreDoubled ?? 0;
}
