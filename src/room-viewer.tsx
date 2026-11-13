import { RemoteVideoTrack, Room, RoomEvent, Track } from "livekit-client";
import { useCallback, useEffect, useRef, useState } from "react";
import { errorMessage } from "./errors.js";
import { liveKitServerUrl } from "./livekit-url.js";

type VideoFeed = {
  id: string;
  participant: string;
  name: string;
  track: RemoteVideoTrack;
};
type ViewerState = "disconnected" | "connecting" | "connected" | "reconnecting";

function VideoTile({ feed }: { feed: VideoFeed }) {
  const element = useRef<HTMLVideoElement>(null);
  useEffect(() => {
    const video = element.current;
    if (!video) return;
    feed.track.attach(video);
    return () => {
      feed.track.detach(video);
    };
  }, [feed.track]);
  return (
    <article className="feed-tile">
      <video
        ref={element}
        autoPlay
        muted
        playsInline
        aria-label={`${feed.participant}: ${feed.name}`}
      />
      <div>
        <strong>{feed.participant}</strong>
        <span>{feed.name}</span>
      </div>
    </article>
  );
}

function inviteField(name: string): string {
  const fields = new URLSearchParams(location.hash.slice(1));
  return fields.get("view") === "room" ? (fields.get(name) ?? "") : "";
}

export function RoomViewer() {
  const [viewerState, setViewerState] = useState<ViewerState>("disconnected");
  const [status, setStatus] = useState(
    "Enter a LiveKit server URL and a viewer token.",
  );
  const [feeds, setFeeds] = useState<VideoFeed[]>([]);
  const [serverUrl, setServerUrl] = useState(() => inviteField("url"));
  const [token, setToken] = useState(() => inviteField("token"));
  const room = useRef<Room | null>(null);
  const generation = useRef(0);

  const disconnect = useCallback(() => {
    generation.current += 1;
    const previousRoom = room.current;
    room.current = null;
    if (previousRoom) void previousRoom.disconnect();
    setFeeds([]);
    setViewerState("disconnected");
  }, []);
  useEffect(
    () => () => {
      generation.current += 1;
      const previousRoom = room.current;
      room.current = null;
      if (previousRoom) void previousRoom.disconnect();
    },
    [],
  );

  async function connect(): Promise<void> {
    disconnect();
    const attempt = generation.current;
    let nextRoom: Room | null = null;
    try {
      const url = liveKitServerUrl(serverUrl);
      if (!token.trim()) throw new Error("Enter a viewer token.");
      nextRoom = new Room({ adaptiveStream: true, dynacast: false });
      const currentRoom = nextRoom;
      room.current = currentRoom;
      currentRoom.on(
        RoomEvent.TrackSubscribed,
        (track, publication, participant) => {
          if (
            room.current !== currentRoom ||
            !(track instanceof RemoteVideoTrack)
          )
            return;
          const feed = {
            id: publication.trackSid,
            participant: participant.identity,
            name: publication.trackName || "Camera",
            track,
          };
          setFeeds((previous) => [
            ...previous.filter((item) => item.id !== feed.id),
            feed,
          ]);
        },
      );
      currentRoom.on(RoomEvent.TrackPublished, (publication) => {
        if (
          room.current === currentRoom &&
          publication.kind === Track.Kind.Video
        )
          publication.setSubscribed(true);
      });
      currentRoom.on(RoomEvent.TrackUnsubscribed, (_track, publication) => {
        if (room.current === currentRoom)
          setFeeds((previous) =>
            previous.filter((item) => item.id !== publication.trackSid),
          );
      });
      currentRoom.on(RoomEvent.Reconnecting, () => {
        if (room.current === currentRoom) {
          setViewerState("reconnecting");
          setStatus("Reconnecting to room…");
        }
      });
      currentRoom.on(RoomEvent.Reconnected, () => {
        if (room.current === currentRoom) {
          setViewerState("connected");
          setStatus("Connected. Waiting for camera feeds.");
        }
      });
      currentRoom.on(RoomEvent.Disconnected, () => {
        if (room.current === currentRoom) {
          room.current = null;
          setFeeds([]);
          setViewerState("disconnected");
          setStatus("Disconnected from the room.");
        }
      });
      setViewerState("connecting");
      setStatus("Connecting to room…");
      await currentRoom.connect(url.href, token.trim(), {
        autoSubscribe: false,
      });
      if (attempt !== generation.current || room.current !== currentRoom) {
        await currentRoom.disconnect();
        return;
      }
      for (const participant of currentRoom.remoteParticipants.values()) {
        for (const publication of participant.trackPublications.values()) {
          if (publication.kind === Track.Kind.Video)
            publication.setSubscribed(true);
        }
      }
      setViewerState("connected");
      setStatus("Connected. Waiting for camera feeds.");
    } catch (error: unknown) {
      if (nextRoom) await nextRoom.disconnect().catch(() => {});
      if (attempt !== generation.current) return;
      room.current = null;
      setViewerState("disconnected");
      setStatus(`Could not join room: ${errorMessage(error, "unknown error")}`);
    }
  }

  const connected =
    viewerState === "connected" || viewerState === "reconnecting";
  return (
    <main className="phone-camera viewer-page">
      <header className="phone-header">
        <div className="phone-branding">
          <img className="phone-logo" src="./hebbian-logo.svg" alt="" />
          <div>
            <p className="phone-brand">Hebbian Robotics</p>
            <h1>Room cameras</h1>
          </div>
        </div>
        <span className={`connection-badge ${connected ? "is-live" : ""}`}>
          {viewerState === "reconnecting"
            ? "Reconnecting"
            : connected
              ? "Connected"
              : viewerState === "connecting"
                ? "Connecting"
                : "Offline"}
        </span>
      </header>
      <p className="phone-intro">
        Watch video tracks in a LiveKit room. Use a token with permission to
        subscribe; a camera publishing invite cannot view the room.
      </p>
      {!connected && (
        <form
          className="connection-form"
          onSubmit={(event) => {
            event.preventDefault();
            void connect();
          }}
        >
          <label>
            LiveKit server URL
            <input
              type="url"
              value={serverUrl}
              onChange={(event) => setServerUrl(event.currentTarget.value)}
              placeholder="wss://your-project.livekit.cloud"
              required
            />
          </label>
          <label>
            Viewer token
            <input
              type="password"
              value={token}
              onChange={(event) => setToken(event.currentTarget.value)}
              autoComplete="off"
              required
            />
          </label>
          <button
            className="stream-button"
            type="submit"
            disabled={viewerState === "connecting"}
          >
            {viewerState === "connecting" ? "Connecting…" : "Join room"}
          </button>
        </form>
      )}
      {connected && (
        <>
          <div className="feed-grid">
            {feeds.map((feed) => (
              <VideoTile key={feed.id} feed={feed} />
            ))}
          </div>
          <button
            className="stream-button is-stop"
            type="button"
            onClick={() => {
              disconnect();
              setStatus("Disconnected from the room.");
            }}
          >
            Leave room
          </button>
        </>
      )}
      <p className="phone-status" role="status">
        {viewerState === "connected" && feeds.length > 0
          ? `Viewing ${feeds.length} ${feeds.length === 1 ? "video feed" : "video feeds"}.`
          : status}
      </p>
    </main>
  );
}
