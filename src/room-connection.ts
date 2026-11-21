import { decodeJwt } from "jose";
import { liveKitServerUrl } from "./livekit-url.js";

type RoomToken = {
  value: string;
  identity: string;
  room?: string;
  expiresAt?: number;
  canStream: boolean;
  canView: boolean;
};

export type RoomConnection = { serverUrl: string; token: RoomToken };

export type RoomSettings = {
  serverUrl: string;
  token: RoomToken;
  viewerToken?: RoomToken;
};

function readRoomToken(value: string, label: string): RoomToken {
  let claims: ReturnType<typeof decodeJwt>;
  try {
    claims = decodeJwt(value);
  } catch {
    throw new Error(`Enter a valid ${label}.`);
  }
  if (!claims.sub?.trim()) {
    throw new Error(`The ${label} must include a participant identity.`);
  }
  // These claims guide the UI. LiveKit verifies the token and enforces the grants.
  const video = claims["video"];
  const grants =
    typeof video === "object" && video !== null
      ? (video as Record<string, unknown>)
      : undefined;
  const publishSources = grants?.["canPublishSources"];
  return {
    value,
    identity: claims.sub,
    ...(typeof grants?.["room"] === "string" ? { room: grants["room"] } : {}),
    ...(claims.exp === undefined ? {} : { expiresAt: claims.exp }),
    canStream:
      grants?.["canPublish"] !== false &&
      (!Array.isArray(publishSources) ||
        publishSources.length === 0 ||
        publishSources.includes("camera")),
    canView: grants?.["canSubscribe"] !== false,
  };
}

export function readRoomSettings(
  serverUrl: string,
  token: string,
  viewerToken = "",
): RoomSettings {
  const url = liveKitServerUrl(serverUrl);
  const connectionToken = readRoomToken(token.trim(), "connection token");
  const optionalViewerToken = viewerToken.trim();
  return {
    serverUrl: url.href,
    token: connectionToken,
    ...(optionalViewerToken
      ? { viewerToken: readRoomToken(optionalViewerToken, "viewer token") }
      : {}),
  };
}

export type ClaimRoomConnection = (
  connection: RoomConnection,
) => (() => void) | undefined;

/** Prevent Stream and Room viewer from replacing each other's LiveKit participant. */
export function createRoomConnectionClaims(): ClaimRoomConnection {
  const participants = new Set<string>();
  return ({ serverUrl, token }) => {
    const participant = JSON.stringify([serverUrl, token.room, token.identity]);
    if (participants.has(participant)) return undefined;
    participants.add(participant);
    let released = false;
    return () => {
      if (released) return;
      released = true;
      participants.delete(participant);
    };
  };
}
