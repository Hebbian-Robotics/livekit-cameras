import { RemoteVideoTrack, Room, RoomEvent, Track } from "livekit-client";
import { useCallback, useEffect, useRef, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { errorMessage } from "./errors.js";
import type { ClaimRoomConnection, RoomConnection } from "./room-connection.js";

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
    <Card render={<article />} size="sm" className="feed-tile pt-0">
      <video
        ref={element}
        autoPlay
        muted
        playsInline
        aria-label={`${feed.participant}: ${feed.name}`}
      />
      <CardHeader className="wrap-anywhere">
        <CardTitle>{feed.participant}</CardTitle>
        <CardDescription>{feed.name}</CardDescription>
      </CardHeader>
    </Card>
  );
}

export function RoomViewer({
  connection,
  claimRoomConnection,
}: {
  connection: RoomConnection;
  claimRoomConnection: ClaimRoomConnection;
}) {
  const [viewerState, setViewerState] = useState<ViewerState>("disconnected");
  const [status, setStatus] = useState("Ready to join.");
  const [feeds, setFeeds] = useState<VideoFeed[]>([]);
  const room = useRef<Room | null>(null);
  const closeRoom = useRef<(() => void) | null>(null);
  const generation = useRef(0);

  const disposeRoom = useCallback(() => {
    generation.current += 1;
    room.current = null;
    closeRoom.current?.();
    closeRoom.current = null;
  }, []);
  const disconnect = useCallback(() => {
    disposeRoom();
    setFeeds([]);
    setViewerState("disconnected");
  }, [disposeRoom]);
  useEffect(() => disposeRoom, [disposeRoom]);

  async function connect(): Promise<void> {
    if (room.current) return;
    const releaseConnection = claimRoomConnection(connection);
    if (!releaseConnection) {
      setStatus("Stop streaming or add a separate viewer token in Setup.");
      return;
    }
    const attempt = ++generation.current;
    const currentRoom = new Room({ adaptiveStream: true, dynacast: false });
    room.current = currentRoom;
    closeRoom.current = () => {
      void currentRoom
        .disconnect()
        .catch(() => {})
        .finally(releaseConnection);
    };
    try {
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
          closeRoom.current = null;
          releaseConnection();
          setFeeds([]);
          setViewerState("disconnected");
          setStatus("Disconnected from the room.");
        }
      });
      setViewerState("connecting");
      setStatus("Connecting to room…");
      await currentRoom.connect(connection.serverUrl, connection.token.value, {
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
      if (attempt !== generation.current) return;
      disconnect();
      setStatus(`Could not join room: ${errorMessage(error, "unknown error")}`);
    }
  }

  const connected =
    viewerState === "connected" || viewerState === "reconnecting";
  return (
    <section className="phone-camera viewer-page">
      <header className="phone-header">
        <h1>Room cameras</h1>
        <Badge
          variant="secondary"
          className={`connection-badge ${connected ? "is-live" : ""}`}
        >
          {viewerState === "reconnecting"
            ? "Reconnecting"
            : connected
              ? "Connected"
              : viewerState === "connecting"
                ? "Connecting"
                : "Offline"}
        </Badge>
      </header>
      <p className="phone-intro">
        Watch camera feeds in the configured LiveKit room.
      </p>
      {!connected && (
        <Button
          className="stream-button"
          type="button"
          disabled={viewerState === "connecting"}
          onClick={() => void connect()}
        >
          {viewerState === "connecting" ? "Connecting…" : "Join room"}
        </Button>
      )}
      {connected && (
        <>
          <div className="feed-grid">
            {feeds.map((feed) => (
              <VideoTile key={feed.id} feed={feed} />
            ))}
          </div>
          <Button
            className="stream-button"
            variant="destructive"
            type="button"
            onClick={() => {
              disconnect();
              setStatus("Disconnected from the room.");
            }}
          >
            Leave room
          </Button>
        </>
      )}
      <p className="phone-status" role="status">
        {viewerState === "connected" && feeds.length > 0
          ? `Viewing ${feeds.length} ${feeds.length === 1 ? "video feed" : "video feeds"}.`
          : status}
      </p>
    </section>
  );
}
