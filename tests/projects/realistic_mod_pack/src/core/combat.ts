import { addScore } from "@core/score";
import { findPlayerName } from "@core/player";
import type { DamageEvent, DamageResult } from "../types";

export function applyDamage(event: DamageEvent): DamageResult {
  const damage = event.critical ? event.baseDamage * 2 : event.baseDamage;
  const playerName = findPlayerName(event.playerId);
  const score = addScore(damage);

  return {
    playerId: event.playerId,
    playerName,
    damage,
    score,
  };
}
