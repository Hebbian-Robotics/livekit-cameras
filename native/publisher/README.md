# Native LiveKit publisher integration

Use `publish_camera` with your LiveKit server URL and a pre-issued publishing
token. Use the `cameras` crate directly for capture without publishing. Start with
the [command-line example](../../README.md#native-camera-publisher).

## Capture worker contract

`CaptureWorker` starts the current executable with the private `__capture`
subcommand. An embedding executable must dispatch that subcommand to
`capture::run_capture_process` before its normal argument parser. This also
applies to snapshot consumers that embed the worker.

Capture supplies the newest frame, not a queue of every frame. Persistent device
identifiers retry; numeric indices stop after failure. Published frame metadata
contains host receipt time, not sensor exposure time or synchronized camera time.

## Build and test

See [native build prerequisites](../../CONTRIBUTING.md#native-rust). CI builds and
runs non-hardware tests on Linux, macOS and Windows. Real devices and LiveKit
publishing need testing on the target hardware.

The upstream [LiveKit local video example](https://github.com/livekit/rust-sdks/tree/main/examples/local_video)
provides SDK-level publishing and diagnostic examples.
