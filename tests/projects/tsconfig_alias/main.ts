import { makeInvoiceLabel } from "@app/billing";
import { status } from "@features/status";

export function renderInvoice(id: number) {
  return {
    label: makeInvoiceLabel(id),
    audit: status(),
  };
}
