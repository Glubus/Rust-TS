import { sumPair } from "./src/math";
import { decoratedVersion } from "./src/version";

export function compute(input: { left: number; right: number }) {
  return {
    total: sumPair(input.left, input.right),
    version: decoratedVersion(),
  };
}
