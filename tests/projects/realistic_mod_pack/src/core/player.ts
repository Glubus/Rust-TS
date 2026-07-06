import { user } from "test";

export function findPlayerName(playerId: number): string {
  return user.find(playerId);
}
