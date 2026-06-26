import { findPlayerName } from "@core/player";

export function createInvoiceLabel(playerId: number, total: number): string {
  const playerName = findPlayerName(playerId);
  return `invoice:${playerName}:${total.toFixed(2)}`;
}
