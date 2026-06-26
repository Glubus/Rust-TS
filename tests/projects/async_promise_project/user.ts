export async function buildName(id: number): Promise<string> {
  return await user.find(id);
}
