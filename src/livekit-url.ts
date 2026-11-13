/** Parse a secure server URL, allowing unencrypted loopback for local development. */
export function liveKitServerUrl(value: string): URL {
  const url = new URL(value.trim());
  if (
    url.protocol !== "wss:" &&
    !(
      url.protocol === "ws:" &&
      ["localhost", "127.0.0.1", "[::1]"].includes(url.hostname)
    )
  ) {
    throw new Error(
      "Use a wss:// LiveKit URL (ws:// is supported for localhost).",
    );
  }
  return url;
}
