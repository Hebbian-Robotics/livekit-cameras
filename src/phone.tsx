import { useMachine } from "@xstate/react";
import { decodeJwt } from "jose";
import {
  ConnectionState,
  createLocalVideoTrack,
  LocalVideoTrack,
  Room,
  RoomEvent,
  Track,
  type VideoCaptureOptions,
  VideoPresets,
} from "livekit-client";
import { useCallback, useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { errorMessage } from "./errors.js";
import { phoneCameraMachine } from "./phone-camera-machine.js";
import { type PhoneStartStage, phoneStartError } from "./phone-errors.js";
import {
  applyZoom,
  backLenses,
  currentZoom,
  type Lens,
  type ZoomRange,
  zoomRange,
} from "./phone-lenses.js";
import {
  type CameraOrientation,
  orientedFrameSize,
} from "./phone-orientation.js";
import { roomIsRecording } from "./room-recording.js";
import "./phone.css";

const PHONE_CAMERA_FRAME_RATE = 15;
const CHOSEN_LENS_KEY = "phone-camera-lens";

type Invite = { serverUrl: string; token: string; cameraName: string };
type CameraSession = {
  room: Room;
  track: LocalVideoTrack;
  stopOutput: () => void;
};

function readInvite(): Invite | null {
  const parameters = new URLSearchParams(location.hash.slice(1));
  const serverUrl = parameters.get("url");
  const token = parameters.get("token");
  if (!serverUrl || !token) return null;
  try {
    const cameraName = decodeJwt(token).sub;
    return cameraName ? { serverUrl, token, cameraName } : null;
  } catch {
    return null;
  }
}

function captureOptions(deviceId?: string): VideoCaptureOptions {
  return {
    ...(deviceId ? { deviceId: { ideal: deviceId } } : {}),
    facingMode: "environment",
    resolution: {
      ...VideoPresets.h720.resolution,
      frameRate: PHONE_CAMERA_FRAME_RATE,
    },
  };
}

function rememberedLens(): string | undefined {
  try {
    return localStorage.getItem(CHOSEN_LENS_KEY) ?? undefined;
  } catch {
    return undefined;
  }
}

function rememberLens(deviceId: string): void {
  try {
    localStorage.setItem(CHOSEN_LENS_KEY, deviceId);
  } catch {
    // Streaming works even when browser storage is unavailable.
  }
}

async function createOrientedOutput(
  cameraTrack: LocalVideoTrack,
  orientation: { current: CameraOrientation },
  flipped: { current: boolean },
): Promise<{ track: LocalVideoTrack; stop: () => void }> {
  const source = document.createElement("video");
  source.muted = true;
  source.playsInline = true;
  // Not cameraTrack.attach(): on Safari and Firefox it reassigns srcObject in a
  // timeout, which aborts the play() awaited below.
  source.srcObject = new MediaStream([cameraTrack.mediaStreamTrack]);
  const canvas = document.createElement("canvas");
  const context = canvas.getContext("2d");
  if (!context || typeof canvas.captureStream !== "function") {
    source.srcObject = null;
    throw new Error("This browser does not support rotating the camera stream");
  }
  let animationFrame = 0;
  let lastFrameTime = 0;
  const drawFrame = (time: number): void => {
    if (
      source.videoWidth &&
      source.videoHeight &&
      time - lastFrameTime >= 1000 / PHONE_CAMERA_FRAME_RATE
    ) {
      const frame = orientedFrameSize(
        source.videoWidth,
        source.videoHeight,
        orientation.current,
      );
      if (canvas.width !== frame.width || canvas.height !== frame.height) {
        canvas.width = frame.width;
        canvas.height = frame.height;
      }
      context.save();
      if ((orientation.current === "landscape") !== flipped.current) {
        context.translate(canvas.width, canvas.height);
        context.rotate(Math.PI);
      }
      if (frame.rotate) {
        context.translate(canvas.width, 0);
        context.rotate(Math.PI / 2);
      }
      context.drawImage(source, 0, 0, source.videoWidth, source.videoHeight);
      context.restore();
      lastFrameTime = time;
    }
    animationFrame = requestAnimationFrame(drawFrame);
  };
  try {
    await source.play();
    const initialFrame = orientedFrameSize(
      source.videoWidth || 1280,
      source.videoHeight || 720,
      orientation.current,
    );
    canvas.width = initialFrame.width;
    canvas.height = initialFrame.height;
    const output = canvas
      .captureStream(PHONE_CAMERA_FRAME_RATE)
      .getVideoTracks()[0];
    if (!output) throw new Error("Could not create the rotated camera stream");
    animationFrame = requestAnimationFrame(drawFrame);
    return {
      track: new LocalVideoTrack(output, undefined, true),
      stop: () => {
        cancelAnimationFrame(animationFrame);
        output.stop();
        source.srcObject = null;
      },
    };
  } catch (error) {
    source.srcObject = null;
    throw error;
  }
}

function PhoneCamera() {
  const [invite, setInvite] = useState(readInvite);
  const inviteRef = useRef(invite);
  const [cameraSnapshot, sendCameraEvent] = useMachine(phoneCameraMachine, {
    input: { start: () => startCamera(), stop: () => closeSession() },
  });
  const cameraState = cameraSnapshot.matches({ active: "starting" })
    ? "starting"
    : cameraSnapshot.matches({ active: "streaming" })
      ? "streaming"
      : "idle";
  const [status, setStatus] = useState(
    invite
      ? "Ready to stream."
      : "Enter your LiveKit server URL and publishing token, or open a camera invite link.",
  );
  const [recording, setRecording] = useState(false);
  const [orientation, setOrientation] =
    useState<CameraOrientation>("landscape");
  const orientationRef = useRef<CameraOrientation>("landscape");
  const [flipped, setFlipped] = useState(false);
  const flippedRef = useRef(false);
  const [lenses, setLenses] = useState<Lens[]>([]);
  const [activeLens, setActiveLens] = useState<string | undefined>();
  const [zoom, setZoom] = useState<{ range: ZoomRange; value: number } | null>(
    null,
  );
  const preview = useRef<HTMLVideoElement>(null);
  const session = useRef<CameraSession | null>(null);
  const wakeLock = useRef<WakeLockSentinel | null>(null);
  const generation = useRef(0);

  const keepScreenAwake = useCallback(async (): Promise<void> => {
    if (wakeLock.current || !("wakeLock" in navigator)) return;
    try {
      const lock = await navigator.wakeLock.request("screen");
      if (!session.current) {
        await lock.release();
        return;
      }
      wakeLock.current = lock;
      lock.addEventListener("release", () => {
        if (wakeLock.current === lock) wakeLock.current = null;
      });
    } catch {
      // The stream remains usable if the browser refuses a wake lock.
    }
  }, []);

  const closeSession = useCallback((): void => {
    generation.current += 1;
    const current = session.current;
    session.current = null;
    if (current) {
      current.stopOutput();
      current.track.stop();
      if (preview.current) preview.current.srcObject = null;
      void current.room.disconnect();
    }
    void wakeLock.current?.release();
    wakeLock.current = null;
  }, []);

  const stop = useCallback((): void => {
    sendCameraEvent({ type: "STOP" });
    setStatus("Not streaming.");
    setRecording(false);
    setLenses([]);
    setZoom(null);
  }, [sendCameraEvent]);

  async function updateCameraControls(track: LocalVideoTrack): Promise<void> {
    const available = await backLenses().catch((): Lens[] => []);
    if (session.current?.track !== track) return;
    setLenses(available);
    setActiveLens(track.mediaStreamTrack.getSettings().deviceId);
    const range = zoomRange(track.mediaStreamTrack);
    setZoom(
      range
        ? { range, value: currentZoom(track.mediaStreamTrack) ?? range.min }
        : null,
    );
  }

  async function startCamera(): Promise<void> {
    const currentInvite = inviteRef.current;
    if (!currentInvite) return;
    const currentGeneration = ++generation.current;
    setStatus("Connecting…");
    const room = new Room({ adaptiveStream: false, dynacast: false });
    let stage: PhoneStartStage = "camera";
    let track: LocalVideoTrack | null = null;
    let output: Awaited<ReturnType<typeof createOrientedOutput>> | null = null;
    try {
      track = await createLocalVideoTrack(captureOptions(rememberedLens()));
      if (generation.current !== currentGeneration) {
        track.stop();
        return;
      }
      sendCameraEvent({ type: "CAPTURED" });
      stage = "prepare";
      output = await createOrientedOutput(track, orientationRef, flippedRef);
      if (generation.current !== currentGeneration) {
        output.stop();
        track.stop();
        return;
      }
      if (preview.current) output.track.attach(preview.current);
      room
        .on(RoomEvent.Reconnecting, () => setStatus("Reconnecting…"))
        .on(RoomEvent.Reconnected, () =>
          setStatus("Live: publishing camera video to the room."),
        )
        .on(RoomEvent.RoomMetadataChanged, () =>
          setRecording(roomIsRecording(room)),
        )
        .on(RoomEvent.Disconnected, () => {
          if (session.current?.room === room) {
            stop();
            setStatus("Disconnected. Tap Start streaming to try again.");
          }
        });
      sendCameraEvent({ type: "PREPARED" });
      stage = "connect";
      await room.connect(currentInvite.serverUrl, currentInvite.token);
      if (generation.current !== currentGeneration) {
        output.stop();
        track.stop();
        await room.disconnect();
        return;
      }
      sendCameraEvent({ type: "CONNECTED" });
      stage = "publish";
      await room.localParticipant.publishTrack(output.track, {
        source: Track.Source.Camera,
        simulcast: false,
        videoEncoding: {
          ...VideoPresets.h720.encoding,
          maxFramerate: PHONE_CAMERA_FRAME_RATE,
        },
      });
      if (generation.current !== currentGeneration) {
        output.stop();
        track.stop();
        await room.disconnect();
        return;
      }
      session.current = { room, track, stopOutput: output.stop };
      sendCameraEvent({ type: "PUBLISHED" });
      setStatus("Live: publishing camera video to the room.");
      setRecording(roomIsRecording(room));
      void keepScreenAwake();
      await updateCameraControls(track);
    } catch (error: unknown) {
      output?.stop();
      track?.stop();
      if (preview.current) preview.current.srcObject = null;
      await room.disconnect().catch(() => {});
      if (generation.current !== currentGeneration) return;
      sendCameraEvent({ type: "FAIL" });
      setStatus(
        phoneStartError(error, stage, decodeJwt(currentInvite.token).exp),
      );
    }
  }

  async function switchLens(lens: Lens): Promise<void> {
    const current = session.current;
    if (!current || roomIsRecording(current.room)) return;
    try {
      await current.track.restartTrack({
        ...captureOptions(),
        deviceId: { exact: lens.deviceId },
      });
      rememberLens(lens.deviceId);
    } catch (error: unknown) {
      setStatus(
        `Could not switch lens: ${errorMessage(error, "unknown error")}`,
      );
    }
    if (session.current === current) await updateCameraControls(current.track);
  }

  async function changeZoom(value: number): Promise<void> {
    const current = session.current;
    if (!current || roomIsRecording(current.room) || !zoom) return;
    setZoom({ ...zoom, value });
    try {
      await applyZoom(current.track.mediaStreamTrack, value);
    } catch {
      setZoom(null);
    }
  }

  useEffect(() => {
    const systemTheme = window.matchMedia("(prefers-color-scheme: dark)");
    function applySystemTheme(): void {
      document.documentElement.setAttribute(
        "data-theme",
        systemTheme.matches ? "dark" : "light",
      );
    }
    applySystemTheme();
    systemTheme.addEventListener("change", applySystemTheme);
    return () => systemTheme.removeEventListener("change", applySystemTheme);
  }, []);

  useEffect(() => {
    function onVisibilityChange(): void {
      const current = session.current;
      if (document.hidden || !current) return;
      void keepScreenAwake();
      if (current.track.mediaStreamTrack.readyState === "ended")
        void current.track.restartTrack();
      if (current.room.state === ConnectionState.Disconnected) stop();
    }
    document.addEventListener("visibilitychange", onVisibilityChange);
    return () => {
      document.removeEventListener("visibilitychange", onVisibilityChange);
      closeSession();
    };
  }, [closeSession, keepScreenAwake, stop]);

  return (
    <main className="phone-camera">
      <header className="phone-header">
        <div>
          <p className="phone-brand">LiveKit camera publisher</p>
          <h1>Phone camera</h1>
        </div>
        <span
          className={`connection-badge ${cameraState === "streaming" ? "is-live" : ""}`}
        >
          {cameraState === "streaming"
            ? "Live"
            : cameraState === "starting"
              ? "Connecting"
              : "Offline"}
        </span>
      </header>
      <p className="phone-intro">
        Place your phone where you need a camera. Keep this page open while
        streaming.
      </p>
      {!invite && (
        <form
          className="connection-form"
          onSubmit={(event) => {
            event.preventDefault();
            const fields = new FormData(event.currentTarget);
            const serverUrl = String(fields.get("serverUrl") ?? "").trim();
            const token = String(fields.get("token") ?? "").trim();
            try {
              const parsedUrl = new URL(serverUrl);
              if (parsedUrl.protocol !== "wss:")
                throw new Error("Use a wss:// LiveKit server URL.");
              const cameraName = decodeJwt(token).sub;
              if (!cameraName)
                throw new Error(
                  "The token must include a participant identity.",
                );
              inviteRef.current = { serverUrl, token, cameraName };
              setInvite(inviteRef.current);
              setStatus("Ready to stream.");
            } catch (error: unknown) {
              setStatus(errorMessage(error, "Invalid connection details."));
            }
          }}
        >
          <label>
            LiveKit server URL
            <input
              name="serverUrl"
              type="url"
              placeholder="wss://your-project.livekit.cloud"
              required
            />
          </label>
          <label>
            Publishing token
            <input name="token" type="password" autoComplete="off" required />
          </label>
          <button type="submit" className="stream-button">
            Use connection
          </button>
        </form>
      )}
      {invite && <p className="camera-name">Camera: {invite.cameraName}</p>}
      <div
        className={`picture ${cameraState === "streaming" ? "is-live" : ""}`}
      >
        <video
          ref={preview}
          autoPlay
          muted
          playsInline
          aria-label="Camera preview"
        />
        {cameraState !== "streaming" && (
          <p className="preview-placeholder">Camera preview</p>
        )}
        {recording && <span className="recording-marker">Recording</span>}
      </div>
      <fieldset className="lenses">
        <legend>Video orientation</legend>
        {(["portrait", "landscape"] as const).map((option) => (
          <button
            key={option}
            type="button"
            disabled={recording}
            aria-pressed={orientation === option}
            onClick={() => {
              orientationRef.current = option;
              setOrientation(option);
            }}
          >
            {option === "portrait" ? "Portrait" : "Landscape"}
          </button>
        ))}
        <button
          type="button"
          disabled={recording}
          aria-pressed={flipped}
          onClick={() => {
            flippedRef.current = !flippedRef.current;
            setFlipped(flippedRef.current);
          }}
        >
          Flip 180°
        </button>
      </fieldset>
      {lenses.length > 1 && (
        <fieldset className="lenses">
          <legend>Lens</legend>
          {lenses.map((lens) => (
            <button
              key={lens.deviceId}
              type="button"
              disabled={recording}
              aria-pressed={lens.deviceId === activeLens}
              onClick={() => void switchLens(lens)}
            >
              {lens.name}
            </button>
          ))}
        </fieldset>
      )}
      {zoom && (
        <label className="zoom">
          Zoom{" "}
          <input
            type="range"
            min={zoom.range.min}
            max={zoom.range.max}
            step={zoom.range.step}
            value={zoom.value}
            disabled={recording}
            onChange={(event) =>
              void changeZoom(Number(event.currentTarget.value))
            }
          />
        </label>
      )}
      {recording && (
        <p className="lock-note">
          Orientation, lens and zoom are locked while the room is recording.
        </p>
      )}
      <p className="phone-status" role="status">
        {status}
      </p>
      <button
        className={`stream-button ${cameraState === "streaming" ? "is-stop" : ""}`}
        type="button"
        disabled={!invite || cameraState === "starting"}
        onClick={() =>
          cameraState === "streaming"
            ? stop()
            : sendCameraEvent({ type: "START" })
        }
      >
        {cameraState === "streaming"
          ? "Stop streaming"
          : cameraState === "starting"
            ? "Connecting…"
            : "Start streaming"}
      </button>
      <footer className="phone-footer">
        <a
          href="https://github.com/Hebbian-Robotics/phone-camera"
          aria-label="View the source code on GitHub"
        >
          <span className="visually-hidden">
            View the source code on GitHub
          </span>
          <svg
            viewBox="0 0 24 24"
            width="24"
            height="24"
            fill="currentColor"
            aria-hidden="true"
          >
            <path d="M12 .75a11.25 11.25 0 0 0-3.56 21.92c.56.1.77-.24.77-.54v-1.9c-3.13.68-3.79-1.33-3.79-1.33-.51-1.3-1.25-1.64-1.25-1.64-1.02-.7.08-.68.08-.68 1.13.08 1.72 1.16 1.72 1.16 1 1.72 2.64 1.22 3.28.93.1-.72.39-1.22.71-1.5-2.5-.29-5.13-1.25-5.13-5.56 0-1.23.44-2.24 1.16-3.03-.12-.28-.5-1.43.11-2.99 0 0 .94-.3 3.09 1.16a10.77 10.77 0 0 1 5.62 0c2.15-1.46 3.09-1.16 3.09-1.16.61 1.56.23 2.71.11 2.99.72.79 1.16 1.8 1.16 3.03 0 4.32-2.63 5.27-5.14 5.55.4.35.76 1.03.76 2.08v2.88c0 .3.2.65.77.54A11.25 11.25 0 0 0 12 .75Z" />
          </svg>
        </a>
      </footer>
    </main>
  );
}

const root = document.getElementById("root");
if (!root) throw new Error("Phone camera root is missing");
createRoot(root).render(<PhoneCamera />);
