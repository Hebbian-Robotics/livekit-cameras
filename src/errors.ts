/** The message of a thrown value; `fallback` (default: its string form) when it is not an Error. */
export function errorMessage(
  error: unknown,
  fallback: string = String(error),
): string {
  return error instanceof Error ? error.message : fallback;
}
