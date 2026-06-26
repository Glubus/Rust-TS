import buildLabel from "./src/default_tool";
import * as numbers from "./src/numbers";
import { suffix } from "./src/reexports";

export function render(input: { left: number; right: number }) {
  return buildLabel(numbers.sum(input.left, input.right), suffix);
}
