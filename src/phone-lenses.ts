// Choosing the phone's back lens (0.5×, 1×, telephoto) and its zoom.
//
// Browsers expose each back lens as its own camera, named by the phone:
// iPhones say which lens it is ("Back Ultra Wide Camera"); Android phones
// usually just number them ("camera2 2, facing back"). Smooth zoom is a
// track setting only some browsers offer. Where neither is available the
// controls stay hidden and the phone streams its default back camera.

export type Lens = { deviceId: string; name: string };

type LensKind = { pattern: RegExp; name: string; order: number };

// iPhone lens names, widest first. Its combined "Dual"/"Triple" cameras
// switch lenses on their own, so they are left out.
const NAMED_LENSES: LensKind[] = [
  { pattern: /ultra wide/i, name: "0.5×", order: 0 },
  { pattern: /^back camera$/i, name: "1×", order: 1 },
  { pattern: /telephoto/i, name: "Tele", order: 2 },
];
const COMBINED_LENS = /dual|triple/i;
const BACK_FACING = /back|rear|environment/i;

// Labels are empty until the page has camera permission, so call this once
// a camera is open.
export async function backLenses(): Promise<Lens[]> {
  const devices = await navigator.mediaDevices.enumerateDevices();
  const backCameras = devices.filter(
    (device) =>
      device.kind === "videoinput" &&
      BACK_FACING.test(device.label) &&
      !COMBINED_LENS.test(device.label),
  );
  const named = backCameras.flatMap((device) => {
    const kind = NAMED_LENSES.find((lens) => lens.pattern.test(device.label));
    return kind === undefined ? [] : [{ device, kind }];
  });
  if (named.length === backCameras.length) {
    return named
      .sort((first, second) => first.kind.order - second.kind.order)
      .map(({ device, kind }) => ({
        deviceId: device.deviceId,
        name: kind.name,
      }));
  }
  // Unnamed lenses: which is wider is unknown, so number them.
  return backCameras.map((device, index) => ({
    deviceId: device.deviceId,
    name: `Lens ${index + 1}`,
  }));
}

export type ZoomRange = { min: number; max: number; step: number };

// Zoom is not yet in TypeScript's DOM types.
type ZoomCapabilities = {
  zoom?: { min?: number; max?: number; step?: number };
};
type ZoomSettings = { zoom?: number };

export function zoomRange(track: MediaStreamTrack): ZoomRange | null {
  if (typeof track.getCapabilities !== "function") return null;
  const { zoom } = track.getCapabilities() as ZoomCapabilities;
  if (zoom?.min === undefined || zoom.max === undefined) return null;
  if (zoom.max <= zoom.min) return null;
  return { min: zoom.min, max: zoom.max, step: zoom.step || 0.1 };
}

export function currentZoom(track: MediaStreamTrack): number | null {
  return (track.getSettings() as ZoomSettings).zoom ?? null;
}

export async function applyZoom(
  track: MediaStreamTrack,
  zoom: number,
): Promise<void> {
  await track.applyConstraints({
    advanced: [{ zoom } as MediaTrackConstraintSet],
  });
}
