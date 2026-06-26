import { recordEvent } from "../state/ledger";

export async function handleScoreUpdate(event: { combo: number; userId: number }) {
  const name = await user.find(event.userId);
  recordEvent(name, event.combo);
}
