import { user } from "test";

export function lookupWithNamespace(id: number) {
  return user.find(id);
}
