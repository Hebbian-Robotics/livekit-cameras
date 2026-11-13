import { expect, test } from "vitest";
import { liveKitServerUrl } from "../src/livekit-url.js";

test("allows secure server URLs and loopback development servers", () => {
  for (const address of [
    "wss://example.livekit.cloud",
    "ws://127.0.0.1:7880",
    "ws://localhost:7880",
    "ws://[::1]:7880",
  ]) {
    expect(liveKitServerUrl(address)).toBeInstanceOf(URL);
  }
});

test("rejects insecure connections to remote hosts", () => {
  for (const address of [
    "ws://example.com",
    "ws://localhost.example.com",
    "http://127.0.0.1:7880",
    "invalid",
  ]) {
    expect(() => liveKitServerUrl(address)).toThrow();
  }
});
