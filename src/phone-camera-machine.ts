import { fromCallback, setup } from "xstate";

/** The invoked session adapter owns media resources while the camera is active. */
export const phoneCameraMachine = setup({
  types: {} as {
    input: { start: () => Promise<void>; stop: () => void };
    context: { start: () => Promise<void>; stop: () => void };
    events:
      | { type: "START" }
      | { type: "STOP" }
      | { type: "FAIL" }
      | { type: "CAPTURED" }
      | { type: "PREPARED" }
      | { type: "CONNECTED" }
      | { type: "PUBLISHED" };
  },
  actors: {
    cameraSession: fromCallback<
      { type: "STOP" },
      { start: () => Promise<void>; stop: () => void }
    >(({ input }) => {
      void input.start();
      return input.stop;
    }),
  },
}).createMachine({
  id: "phone-camera",
  initial: "idle",
  context: ({ input }) => input,
  states: {
    idle: { on: { START: "active" } },
    active: {
      invoke: { src: "cameraSession", input: ({ context }) => context },
      initial: "starting",
      on: { STOP: "idle", FAIL: "idle" },
      states: {
        starting: {
          initial: "capture",
          states: {
            capture: { on: { CAPTURED: "prepare" } },
            prepare: { on: { PREPARED: "connect" } },
            connect: { on: { CONNECTED: "publish" } },
            publish: { on: { PUBLISHED: "#phone-camera.active.streaming" } },
          },
        },
        streaming: {},
      },
    },
  },
});
