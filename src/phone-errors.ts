import { errorMessage } from "./errors.js";

export type PhoneStartStage = "camera" | "prepare" | "connect" | "publish";

export function phoneStartError(
  error: unknown,
  stage: PhoneStartStage,
  inviteExpiry: number | undefined,
  now = Date.now() / 1000,
): string {
  if (inviteExpiry !== undefined && now >= inviteExpiry) {
    return "This camera link has expired. Ask your token issuer for a new camera link. If it is new, check your phone's date and time.";
  }
  const name = error instanceof Error ? error.name : "";
  if (stage === "camera") {
    if (name === "NotAllowedError" || name === "SecurityError") {
      return "Camera access was denied. Allow camera access in your browser settings and try again.";
    }
    if (name === "NotFoundError" || name === "OverconstrainedError") {
      return "No suitable camera was found. Check your camera settings and try again.";
    }
    if (name === "NotReadableError") {
      return "The camera could not be opened. Close other apps using it and try again.";
    }
  }
  const prefix = {
    camera: "Could not open the camera",
    prepare: "Could not prepare camera video",
    connect: "Could not connect to the camera service",
    publish: "Connected, but could not publish the camera",
  }[stage];
  const detail = errorMessage(error, "Unknown error");
  return `${prefix}: ${detail}.${stage === "connect" ? " Check your phone's internet connection and try again." : ""}`;
}
