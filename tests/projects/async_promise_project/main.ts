import { buildName } from "./user";

export async function lookup(id: number): Promise<{ name: string }> {
  return {
    name: await buildName(id),
  };
}
