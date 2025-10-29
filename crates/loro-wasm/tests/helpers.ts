export function expectDefined<T>(
  value: T | undefined,
  message?: string,
): T {
  if (value === undefined) {
    throw new Error(message ?? "Expected value to be defined");
  }
  return value;
}
