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
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Field,
  FieldGroup,
  FieldLabel,
  FieldLegend,
  FieldSet,
  FieldTitle,
} from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import {
  NativeSelect,
  NativeSelectOption,
} from "@/components/ui/native-select";
import { Slider } from "@/components/ui/slider";
import { Toggle } from "@/components/ui/toggle";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
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

  async function switchLens(deviceId: string): Promise<void> {
    const current = session.current;
    if (!current || roomIsRecording(current.room)) return;
    try {
      await current.track.restartTrack({
        ...captureOptions(),
        deviceId: { exact: deviceId },
      });
      if (session.current !== current) {
        current.track.stop();
        return;
      }
      await current.refreshOutput();
      rememberLens(deviceId);
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
    <section className="phone-camera">
      <header className="phone-header">
        <h1>Browser camera</h1>
        <Badge
          variant="secondary"
          className={`connection-badge ${cameraState === "streaming" ? "is-live" : ""}`}
        >
          {cameraState === "streaming"
            ? "Live"
            : cameraState === "starting"
              ? "Connecting"
              : "Offline"}
        </Badge>
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
          <FieldGroup className="[&_input]:min-h-11">
            <Field>
              <FieldLabel htmlFor="publisher-server-url">
                LiveKit server URL
              </FieldLabel>
              <Input
                id="publisher-server-url"
                name="serverUrl"
                type="url"
                placeholder="wss://your-project.livekit.cloud"
                required
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="publisher-token">
                Publishing token
              </FieldLabel>
              <Input
                id="publisher-token"
                name="token"
                type="password"
                autoComplete="off"
                required
              />
            </Field>
          </FieldGroup>
          <Button type="submit" className="stream-button">
            Use connection
          </Button>
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
        {recording && <Badge className="recording-marker">Recording</Badge>}
      </div>
      <FieldSet className="gap-2">
        <FieldLegend variant="label" id="video-orientation-label">
          Video orientation
        </FieldLegend>
        <div className="camera-toggles flex flex-wrap gap-2">
          <ToggleGroup<CameraOrientation>
            aria-labelledby="video-orientation-label"
            className="flex-[2]"
            variant="outline"
            value={[orientation]}
            disabled={recording}
            onValueChange={([selectedOrientation]) => {
              if (!selectedOrientation) return;
              orientationRef.current = selectedOrientation;
              setOrientation(selectedOrientation);
            }}
          >
            <ToggleGroupItem value="portrait">Portrait</ToggleGroupItem>
            <ToggleGroupItem value="landscape">Landscape</ToggleGroupItem>
          </ToggleGroup>
          <Toggle
            disabled={recording}
            variant="outline"
            pressed={flipped}
            onPressedChange={(pressed) => {
              flippedRef.current = pressed;
              setFlipped(pressed);
            }}
          >
            Flip 180°
          </Toggle>
        </div>
      </FieldSet>
      {cameras.length > 1 && (
        <Field data-disabled={recording || undefined}>
          <FieldLabel htmlFor="camera-device">Camera</FieldLabel>
          <NativeSelect
            id="camera-device"
            className="[&_select]:min-h-11"
            value={activeLens ?? ""}
            disabled={recording}
            onChange={(event) => {
              const selectedDeviceId = event.currentTarget.value;
              if (selectedDeviceId) void switchLens(selectedDeviceId);
            }}
          >
            <NativeSelectOption value="" disabled>
              Select a camera
            </NativeSelectOption>
            {cameras.map((camera) => (
              <NativeSelectOption key={camera.deviceId} value={camera.deviceId}>
                {camera.name}
              </NativeSelectOption>
            ))}
          </NativeSelect>
        </Field>
      )}
      {lenses.length > 1 && (
        <FieldSet className="gap-2">
          <FieldLegend variant="label" id="camera-lens-label">
            Lens
          </FieldLegend>
          <ToggleGroup
            aria-labelledby="camera-lens-label"
            className="camera-toggles w-full flex-wrap"
            variant="outline"
            value={activeLens ? [activeLens] : []}
            disabled={recording}
            onValueChange={([deviceId]) => {
              if (deviceId) void switchLens(deviceId);
            }}
          >
            {lenses.map((lens) => (
              <ToggleGroupItem key={lens.deviceId} value={lens.deviceId}>
                {lens.name}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        </FieldSet>
      )}
      {zoom && (
        <Field orientation="horizontal">
          <FieldTitle id="camera-zoom-label">Zoom</FieldTitle>
          <Slider
            aria-labelledby="camera-zoom-label"
            className="min-w-0 flex-1"
            min={zoom.range.min}
            max={zoom.range.max}
            step={zoom.range.step}
            largeStep={zoom.range.step * 10}
            value={zoom.value}
            disabled={recording}
            onValueChange={(value) => void changeZoom(value)}
          />
        </Field>
      )}
      {recording && (
        <p className="lock-note">
          Orientation, lens and zoom are locked while the room is recording.
        </p>
      )}
      <p className="phone-status" role="status">
        {status}
      </p>
      <Button
        className="stream-button"
        variant={cameraState === "streaming" ? "destructive" : "default"}
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
      </Button>
    </section>
  );
}
