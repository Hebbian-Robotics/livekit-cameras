// Optional integration: room metadata {"recording": true} locks camera controls.
import type { Room } from "livekit-client";

export function roomIsRecording(room: Room): boolean {
  try {
    const metadata: unknown = JSON.parse(room.metadata ?? "");
    return (
      typeof metadata === "object" &&
      metadata !== null &&
      "recording" in metadata &&
      metadata.recording === true
    );
  } catch {
    return false;
  }
}
