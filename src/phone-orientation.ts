export type CameraOrientation = "portrait" | "landscape";

export function orientedFrameSize(
  width: number,
  height: number,
  orientation: CameraOrientation,
): { width: number; height: number; rotate: boolean } {
  const rotate = orientation === "landscape" ? height > width : width > height;
  return rotate
    ? { width: height, height: width, rotate }
    : { width, height, rotate };
}
