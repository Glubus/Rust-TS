import { double } from "./lib/math";

export function makeInvoiceLabel(id: number) {
  return `invoice-${double(id)}`;
}
