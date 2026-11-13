import { expect, test } from "vitest";
import { createActor } from "xstate";
import { phoneCameraMachine } from "../src/phone-camera-machine.js";

test("a stopped phone camera ignores a late publish result", () => {
  let sessionStopped = false;
  const camera = createActor(phoneCameraMachine, {
    input: {
      start: async () => {},
      stop: () => {
        sessionStopped = true;
      },
    },
  }).start();
  camera.send({ type: "START" });
  camera.send({ type: "CAPTURED" });
  camera.send({ type: "PREPARED" });
  camera.send({ type: "CONNECTED" });
  camera.send({ type: "STOP" });
  expect(sessionStopped).toBe(true);
  camera.send({ type: "PUBLISHED" });
  expect(camera.getSnapshot().matches("idle")).toBe(true);
  camera.stop();
});
