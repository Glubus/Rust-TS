import { billing, user } from "test";

export function inspectBindings(id: number) {
  return {
    userFindType: typeof user.find,
    missingType: typeof user.missing,
    invoice: billing.invoice.find(id),
  };
}
