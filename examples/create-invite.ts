import { writeFile } from "node:fs/promises";
import { AccessToken, TrackSource } from "livekit-server-sdk";
import QRCode from "qrcode";

function requiredEnvironmentVariable(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`Set ${name} before generating an invite.`);
  return value;
}

const accessToken = new AccessToken(
  requiredEnvironmentVariable("LIVEKIT_API_KEY"),
  requiredEnvironmentVariable("LIVEKIT_API_SECRET"),
  {
    identity: process.env["CAMERA_IDENTITY"] ?? "browser_camera",
    ttl: "1h",
  },
);
accessToken.addGrant({
  roomJoin: true,
  room: requiredEnvironmentVariable("LIVEKIT_ROOM"),
  canPublish: true,
  canPublishSources: [TrackSource.CAMERA],
  canSubscribe: false,
  canPublishData: false,
});
const cameraPageUrl = new URL(
  process.env["CAMERA_PAGE_URL"] ??
    "https://hebbian-robotics.github.io/livekit-cameras/",
);
cameraPageUrl.hash = new URLSearchParams({
  url: requiredEnvironmentVariable("LIVEKIT_URL"),
  token: await accessToken.toJwt(),
}).toString();
await writeFile(
  "invite.svg",
  await QRCode.toString(cameraPageUrl.href, { type: "svg" }),
);
console.log(cameraPageUrl.href);
console.log(
  "QR code saved to invite.svg. Treat both the link and QR code as secrets.",
);
