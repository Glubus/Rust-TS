import { recordLookup } from "../state/ledger";

export async function resolveUserLabel(id: number): Promise<string> {
  const name = await user.find(id);
  recordLookup(name, id);
  return `${name}#${id}`;
}

export async function resolveManyUserLabels(ids: number[]): Promise<string[]> {
  return await Promise.all(ids.map(resolveUserLabel));
}
