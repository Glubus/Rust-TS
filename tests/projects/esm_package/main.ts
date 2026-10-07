import { label } from "esm-pkg";
import { triple } from "esm-pkg/tools";
import { flavor } from "module-pkg";

export function run(value: number): string {
  return `${label}:${triple(value)}:${flavor}`;
}
