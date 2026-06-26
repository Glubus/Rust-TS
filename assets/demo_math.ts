type Input = { left: number; right: number };

export function sum(input: Input) {
  return input.left + input.right;
}

export function version() {
  return "v1";
}
