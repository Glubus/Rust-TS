import { label } from "demo-pkg";
import { triple } from "demo-pkg/tools";

export function run(value: number): string {
  return `${label}:${triple(value)}`;
}
