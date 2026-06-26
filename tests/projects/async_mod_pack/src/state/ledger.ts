export type LedgerEntry = {
  kind: "lookup" | "event";
  user: string;
  value: number;
};

const entries: LedgerEntry[] = [];

export function recordLookup(user: string, value: number): LedgerEntry {
  const entry: LedgerEntry = { kind: "lookup", user, value };
  entries.push(entry);
  return entry;
}

export function recordEvent(user: string, value: number): LedgerEntry {
  const entry: LedgerEntry = { kind: "event", user, value };
  entries.push(entry);
  return entry;
}

export function totalValue(): number {
  return entries.reduce((total, entry) => total + entry.value, 0);
}

export function ledgerSnapshot(): LedgerEntry[] {
  return entries.slice();
}
