let score = 0;

export function addScore(points: number): number {
  score += points;
  return score;
}

export function currentScore(): number {
  return score;
}

export function resetScore(): number {
  score = 0;
  return score;
}
