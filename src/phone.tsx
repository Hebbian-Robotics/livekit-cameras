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
import { errorMessage } from "./errors.js";
import { liveKitServerUrl } from "./livekit-url.js";
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
  refreshOutput: () => Promise<void>;
};

function readInvite(): Invite | null {
  const parameters = new URLSearchParams(location.hash.slice(1));
  if (parameters.get("view") === "room") return null;
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
): Promise<{
  track: LocalVideoTrack;
  stop: () => void;
  refresh: () => Promise<void>;
}> {
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
      refresh: async () => {
        source.srcObject = new MediaStream([cameraTrack.mediaStreamTrack]);
        await source.play();
      },
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

export function PhoneCamera() {
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
  const [cameras, setCameras] = useState<Lens[]>([]);
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
    setCameras([]);
    setZoom(null);
  }, [sendCameraEvent]);

  async function updateCameraControls(track: LocalVideoTrack): Promise<void> {
    const [available, devices] = await Promise.all([
      backLenses().catch((): Lens[] => []),
      navigator.mediaDevices
        .enumerateDevices()
        .catch((): MediaDeviceInfo[] => []),
    ]);
    if (session.current?.track !== track) return;
    setLenses(available);
    setCameras(
      devices
        .filter((device) => device.kind === "videoinput" && device.deviceId)
        .map((device, index) => ({
          deviceId: device.deviceId,
          name: device.label || `Camera ${index + 1}`,
        })),
    );
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
      session.current = {
        room,
        track,
        stopOutput: output.stop,
        refreshOutput: output.refresh,
      };
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
      if (session.current !== current) {
        current.track.stop();
        return;
      }
      await current.refreshOutput();
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
    function onVisibilityChange(): void {
      const current = session.current;
      if (document.hidden || !current) return;
      void keepScreenAwake();
      if (current.track.mediaStreamTrack.readyState === "ended")
        void current.track
          .restartTrack()
          .then(async () => {
            if (session.current === current) await current.refreshOutput();
            else current.track.stop();
          })
          .catch((error: unknown) => {
            setStatus(
              `Could not restart camera: ${errorMessage(error, "unknown error")}`,
            );
          });
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
        <div className="phone-branding">
          <img className="phone-logo" src="./hebbian-logo.svg" alt="" />
          <div>
            <p className="phone-brand">Hebbian Robotics</p>
            <h1>Browser camera</h1>
          </div>
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
        Place your device where you need a camera. Keep this page open while
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
              liveKitServerUrl(serverUrl);
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
      {cameras.length > 1 && (
        <label className="camera-select">
          Camera
          <select
            value={activeLens ?? ""}
            disabled={recording}
            onChange={(event) => {
              const camera = cameras.find(
                (item) => item.deviceId === event.currentTarget.value,
              );
              if (camera) void switchLens(camera);
            }}
          >
            <option value="" disabled>
              Select a camera
            </option>
            {cameras.map((camera) => (
              <option key={camera.deviceId} value={camera.deviceId}>
                {camera.name}
              </option>
            ))}
          </select>
        </label>
      )}
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
          href="https://github.com/Hebbian-Robotics/livekit-cameras"
          aria-label="View the source code on GitHub"
        >
          <span className="visually-hidden">
            View the source code on GitHub
          </span>
          <svg
            viewBox="0 0 98 96"
            width="24"
            height="24"
            fill="currentColor"
            aria-hidden="true"
          >
            <path d="M41.4395 69.3848C28.8066 67.8535 19.9062 58.7617 19.9062 46.9902C19.9062 42.2051 21.6289 37.0371 24.5 33.5918C23.2559 30.4336 23.4473 23.7344 24.8828 20.959C28.7109 20.4805 33.8789 22.4902 36.9414 25.2656C40.5781 24.1172 44.4062 23.543 49.0957 23.543C53.7852 23.543 57.6133 24.1172 61.0586 25.1699C64.0254 22.4902 69.2891 20.4805 73.1172 20.959C74.457 23.543 74.6484 30.2422 73.4043 33.4961C76.4668 37.1328 78.0937 42.0137 78.0937 46.9902C78.0937 58.7617 69.1934 67.6621 56.3691 69.2891C59.623 71.3945 61.8242 75.9883 61.8242 81.252L61.8242 91.2051C61.8242 94.0762 64.2168 95.7031 67.0879 94.5547C84.4102 87.9512 98 70.6289 98 49.1914C98 22.1074 75.9883 6.69539e-07 48.9043 4.309e-07C21.8203 1.92261e-07 -1.9479e-07 22.1074 -4.3343e-07 49.1914C-6.20631e-07 70.4375 13.4941 88.0469 31.6777 94.6504C34.2617 95.6074 36.75 93.8848 36.75 91.3008L36.75 83.6445C35.4102 84.2188 33.6875 84.6016 32.1562 84.6016C25.8398 84.6016 22.1074 81.1563 19.4277 74.7441C18.375 72.1602 17.2266 70.6289 15.0254 70.3418C13.877 70.2461 13.4941 69.7676 13.4941 69.1934C13.4941 68.0449 15.4082 67.1836 17.3223 67.1836C20.0977 67.1836 22.4902 68.9063 24.9785 72.4473C26.8926 75.2227 28.9023 76.4668 31.2949 76.4668C33.6875 76.4668 35.2187 75.6055 37.4199 73.4043C39.0469 71.7773 40.291 70.3418 41.4395 69.3848Z" />
          </svg>
        </a>
      </footer>
    </main>
  );
}
