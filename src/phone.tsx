import { useMachine } from "@xstate/react";
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
import {
  RectangleHorizontalIcon,
  RectangleVerticalIcon,
  RotateCwIcon,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuIconTrigger,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
} from "@/components/ui/dropdown-menu";
import {
  Field,
  FieldLabel,
  FieldLegend,
  FieldSet,
  FieldTitle,
} from "@/components/ui/field";
import {
  NativeSelect,
  NativeSelectOption,
} from "@/components/ui/native-select";
import { Slider } from "@/components/ui/slider";
import { Toggle } from "@/components/ui/toggle";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
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
import type { ClaimRoomConnection, RoomConnection } from "./room-connection.js";
import { roomIsRecording } from "./room-recording.js";
import "./phone.css";

const PHONE_CAMERA_FRAME_RATE = 15;
const CHOSEN_LENS_KEY = "phone-camera-lens";

const orientationOptions = {
  portrait: { label: "Portrait", Icon: RectangleVerticalIcon },
  landscape: { label: "Landscape", Icon: RectangleHorizontalIcon },
} satisfies Record<
  CameraOrientation,
  { label: string; Icon: typeof RectangleVerticalIcon }
>;

type CameraSession = {
  room: Room;
  track: LocalVideoTrack;
  stopOutput: () => void;
  refreshOutput: () => Promise<void>;
  releaseConnection: () => void;
};

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

export function PhoneCamera({
  connection,
  claimRoomConnection,
}: {
  connection: RoomConnection;
  claimRoomConnection: ClaimRoomConnection;
}) {
  const [cameraSnapshot, sendCameraEvent] = useMachine(phoneCameraMachine, {
    input: { start: () => startCamera(), stop: () => closeSession() },
  });
  const cameraState = cameraSnapshot.matches({ active: "starting" })
    ? "starting"
    : cameraSnapshot.matches({ active: "streaming" })
      ? "streaming"
      : "idle";
  const [status, setStatus] = useState("Ready to stream.");
  const [recording, setRecording] = useState(false);
  const [orientation, setOrientation] =
    useState<CameraOrientation>("landscape");
  const [orientationMenuOpen, setOrientationMenuOpen] = useState(false);
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
      void current.room
        .disconnect()
        .catch(() => {})
        .finally(current.releaseConnection);
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

  function updateRecordingState(room: Room): void {
    const roomRecording = roomIsRecording(room);
    setRecording(roomRecording);
    if (roomRecording) setOrientationMenuOpen(false);
  }

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
    const releaseConnection = claimRoomConnection(connection);
    if (!releaseConnection) {
      setStatus(
        "Leave the room in Room viewer or use a separate viewer token in Setup.",
      );
      sendCameraEvent({ type: "FAIL" });
      return;
    }
    let sessionOwnsConnection = false;
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
        .on(RoomEvent.RoomMetadataChanged, () => updateRecordingState(room))
        .on(RoomEvent.Disconnected, () => {
          if (session.current?.room === room) {
            stop();
            setStatus("Disconnected. Tap Start streaming to try again.");
          }
        });
      sendCameraEvent({ type: "PREPARED" });
      stage = "connect";
      await room.connect(connection.serverUrl, connection.token.value);
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
        releaseConnection,
      };
      sessionOwnsConnection = true;
      sendCameraEvent({ type: "PUBLISHED" });
      setStatus("Live: publishing camera video to the room.");
      updateRecordingState(room);
      void keepScreenAwake();
      await updateCameraControls(track);
    } catch (error: unknown) {
      output?.stop();
      track?.stop();
      if (preview.current) preview.current.srcObject = null;
      await room.disconnect().catch(() => {});
      if (generation.current !== currentGeneration) return;
      sendCameraEvent({ type: "FAIL" });
      setStatus(phoneStartError(error, stage, connection.token.expiresAt));
    } finally {
      if (!sessionOwnsConnection) releaseConnection();
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

  const { label: orientationLabel, Icon: OrientationIcon } =
    orientationOptions[orientation];

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
      <p className="camera-name">Camera: {connection.token.identity}</p>
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
        <FieldLegend variant="label">Video orientation</FieldLegend>
        <div className="flex items-center gap-2">
          <DropdownMenu
            disabled={recording}
            modal={false}
            open={orientationMenuOpen}
            onOpenChange={setOrientationMenuOpen}
          >
            <DropdownMenuIconTrigger
              label={`Video orientation: ${orientationLabel}`}
              tooltip="Change video orientation"
              variant="outline"
            >
              <OrientationIcon aria-hidden="true" />
            </DropdownMenuIconTrigger>
            <DropdownMenuContent className="min-w-42" collisionPadding={12}>
              <DropdownMenuRadioGroup
                aria-label="Video orientation"
                value={orientation}
                onValueChange={(selectedOrientation) => {
                  if (
                    selectedOrientation !== "portrait" &&
                    selectedOrientation !== "landscape"
                  )
                    return;
                  orientationRef.current = selectedOrientation;
                  setOrientation(selectedOrientation);
                }}
              >
                {(["portrait", "landscape"] as const).map((option) => {
                  const { label, Icon } = orientationOptions[option];
                  return (
                    <DropdownMenuRadioItem
                      className="min-h-11"
                      closeOnClick
                      key={option}
                      value={option}
                    >
                      <Icon aria-hidden="true" />
                      {label}
                    </DropdownMenuRadioItem>
                  );
                })}
              </DropdownMenuRadioGroup>
            </DropdownMenuContent>
          </DropdownMenu>
          <Tooltip>
            <TooltipTrigger
              render={
                <Toggle
                  aria-label="Flip 180°"
                  className="size-11 aria-pressed:border-primary aria-pressed:bg-primary aria-pressed:text-primary-foreground"
                  disabled={recording}
                  variant="outline"
                  pressed={flipped}
                  onPressedChange={(pressed) => {
                    flippedRef.current = pressed;
                    setFlipped(pressed);
                  }}
                />
              }
            >
              <RotateCwIcon
                aria-hidden="true"
                className="group-aria-pressed/toggle:rotate-180"
              />
            </TooltipTrigger>
            <TooltipContent sideOffset={8}>Flip 180°</TooltipContent>
          </Tooltip>
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
        variant={cameraState === "streaming" ? "stop" : "default"}
        type="button"
        disabled={cameraState === "starting"}
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
