import { joinName } from "./helpers";

export function buildMessage(name: string) {
  return `hello ${joinName(name)}`;
}
