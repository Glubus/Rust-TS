import { validation } from "test";

export function echo(input) {
  return validation.echo(input);
}

export function echoRef(input) {
  return validation.echoRef(input);
}

export function badOutput(input) {
  return validation.badOutput(input);
}
