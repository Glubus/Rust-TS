import { applyDamage } from "@core/combat";
import { addScore, currentScore, resetScore } from "@core/score";
import { createInvoiceLabel } from "@economy/invoice";
import { sessionSnapshot, tickSession } from "@state/session";
import { overlayState, pushOverlayMessage } from "@ui/overlay";
import type { DamageEvent } from "./types";

ctx.on("score.update", event => {
  tickSession();
  addScore(event.combo);
  pushOverlayMessage(`combo:${event.combo}`);
});

export function simulateDamage(event: DamageEvent) {
  const result = applyDamage(event);
  pushOverlayMessage(`damage:${result.playerName}:${result.damage}`);
  return result;
}

export function invoiceFor(playerId: number, total: number): string {
  return createInvoiceLabel(playerId, total);
}

export function readState() {
  return {
    score: currentScore(),
    overlay: overlayState(),
    session: sessionSnapshot(),
  };
}

export function resetAll() {
  resetScore();
  return readState();
}
