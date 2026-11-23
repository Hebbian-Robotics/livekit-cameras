# Native LiveKit publisher integration

Use `publish_camera` with your LiveKit server URL and a pre-issued publishing
token. Use the `cameras` crate directly for capture without publishing. Start with
the [command-line example](../../README.md#native-camera-publisher).

## Capture worker contract

`CaptureWorker` starts the current executable with the private `__capture`
subcommand. An embedding executable must dispatch that subcommand to
`capture::run_capture_process` before its normal argument parser. This also
applies to snapshot consumers that embed the worker.

Capture supplies the newest frame. The worker stops and reaps the capture process
if the first complete frame takes more than 15 seconds or a subsequent frame
takes more than 5 seconds. A caller can then resolve the current device and
start another worker. Numeric indices stop after a capture failure because the
index may point to a different camera after a reconnect.

Use `publish_camera_frames` to publish from an existing capture worker if the
application manages room reconnects and needs to keep capturing between
connections. Published frame metadata contains host receipt time.

## Codec selection and diagnostics

The native publisher defaults to VP8. Pass `--video-codec h264` to request H.264,
then compare it with `--video-codec vp8` using the same cameras and settings.
The SDK selects the encoder automatically and may fall back if the requested
codec is unavailable. The CLI also accepts `LIVEKIT_CAMERA_CODEC=h264`.
Library callers set `CameraConfiguration.publishing` using `PublishingOptions`.

Once publishing starts, one statistics request reports the negotiated codec and
encoder implementation, or says they are unavailable. Requesting H.264 does not
prove that hardware encoding is active.

Pass `--camera-diagnostics` or set `LIVEKIT_CAMERA_DIAGNOSTICS=true` to collect
statistics in a separate task every five seconds. Reports show interval encoded
frame rate, sent bitrate, mean encoding time, and the latest frame's host-receipt
to submission age. The first sample establishes the interval baseline. Normal
operation does not poll statistics or measure submission age. Diagnostic output
goes to stderr and stops with the publication.

Submission age excludes sensor exposure and receiver playback. Dropped frames,
conversion time, and end-to-end display latency are not inferred from these
counters. Missing encoder names and intervals with no encoded frames are reported
as unavailable. Encoding time depends on the SDK/backend's instrumentation.

## Build and test

See [native build prerequisites](../../CONTRIBUTING.md#native-rust). CI builds and
runs non-hardware tests on Linux, macOS and Windows. Real devices and LiveKit
publishing need testing on the target hardware.

The upstream [LiveKit local video example](https://github.com/livekit/rust-sdks/tree/main/examples/local_video)
provides SDK-level publishing and diagnostic examples.
