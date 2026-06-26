import type { MissingInput } from "./missing_input";
export type { MissingOutput } from "./missing_output";

export function read(input: MissingInput | null) {
  return input === null ? "empty" : "present";
}
