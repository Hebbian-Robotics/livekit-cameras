import { describe, expect, it } from "vitest";
import { orientedFrameSize } from "../src/phone-orientation.js";

describe("phone camera orientation", () => {
  it.each([
    [720, 1280, "landscape", 1280, 720, true],
    [1280, 720, "portrait", 720, 1280, true],
    [720, 1280, "portrait", 720, 1280, false],
    [1280, 720, "landscape", 1280, 720, false],
  ] as const)(
    "orients a %i by %i frame as %s",
    (width, height, orientation, expectedWidth, expectedHeight, rotate) => {
      expect(orientedFrameSize(width, height, orientation)).toEqual({
        width: expectedWidth,
        height: expectedHeight,
        rotate,
      });
    },
  );
});
