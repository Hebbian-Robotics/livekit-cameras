import { randomUUID } from "node:crypto";
import { AccessToken } from "livekit-server-sdk";

function requiredEnvironmentVariable(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`Set ${name} before generating an invite.`);
  return value;
}

const accessToken = new AccessToken(
  requiredEnvironmentVariable("LIVEKIT_API_KEY"),
  requiredEnvironmentVariable("LIVEKIT_API_SECRET"),
  {
    identity: process.env["VIEWER_IDENTITY"] ?? `viewer-${randomUUID()}`,
    ttl: "1h",
  },
);
accessToken.addGrant({
  roomJoin: true,
  room: requiredEnvironmentVariable("LIVEKIT_ROOM"),
  canPublish: false,
  canSubscribe: true,
  canPublishData: false,
});
const viewerPageUrl = new URL(
  process.env["CAMERA_PAGE_URL"] ??
    "https://hebbian-robotics.github.io/livekit-cameras/",
);
viewerPageUrl.hash = new URLSearchParams({
  view: "room",
  url: requiredEnvironmentVariable("LIVEKIT_URL"),
  token: await accessToken.toJwt(),
}).toString();
console.log(viewerPageUrl.href);
console.log(
  "Treat this viewer link as a secret: it grants access to the room's media.",
);
