ctx.on("score.update", event => {
  globalThis.lastScore = event.combo;
});

ctx.on("score.update", event => {
  globalThis.lastScoreDoubled = event.combo * 2;
});

export function readScore() {
  return globalThis.lastScore ?? 0;
}

export function readScoreDoubled() {
  return globalThis.lastScoreDoubled ?? 0;
}
