import { handleScoreUpdate } from "./events/score";
import { ledgerSnapshot, totalValue } from "./state/ledger";
import { resolveManyUserLabels, resolveUserLabel } from "./services/users";

ctx.on("score.update", handleScoreUpdate);

export async function lookupProfile(id: number) {
  return {
    label: await resolveUserLabel(id),
    total: totalValue(),
  };
}

export async function lookupParty(ids: number[]) {
  return {
    labels: await resolveManyUserLabels(ids),
    total: totalValue(),
  };
}

export function readLedger() {
  return {
    entries: ledgerSnapshot(),
    total: totalValue(),
  };
}
