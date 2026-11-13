# Native camera capture

A native camera capture library and `cameras` binary. `livekit-camera-publisher`
uses it to publish frames to a LiveKit room.

Run from the repository root:

```sh
cargo run -p cameras -- list
cargo run -p cameras -- --camera chest=/dev/video0 grab chest --out frame.jpg --count 150
cargo run -p cameras -- grab 0 --out frame.jpg
```

`--camera name=device` is repeatable and supplies logical camera names explicitly.
Names use `[a-z0-9_]{1,40}`, making them safe participant identities and directory
names. For example, use `--camera chest=/run/cameras/cam_chest`.
Native indices, Linux `/dev/videoN` paths and stable symlinks (including
`/run/cameras/*` and `/dev/v4l/by-id/*`), and native identifiers on macOS and Windows
are supported.
The capture tool does not support URLs or video files.

Capture uses nokhwa **=0.10.10**. V4L2 on Linux, AVFoundation on macOS and Media Foundation
on Windows are enabled. Windows camera capture requires validation on Windows hardware.
Linux builds need libclang for the V4L2 bindings (Ubuntu: `libclang-dev`), and GLib development headers for the publisher. macOS requires camera access for the terminal
running the binary; initialization requests permission before enumeration.

The preferred format is MJPEG, then the closest available resolution to
1280×720 and frame rate to 15 fps. Cameras without MJPEG use an available
RGB-decodable native format. The CLI prints the negotiated format rather than
assuming a camera accepted the defaults. `--count` captures and converts every
frame and writes the last JPEG, once, to `--out`.

Each result contains planar BT.601 limited-range I420 (Y then U then V, with
ceil-divided chroma dimensions), a quality-85 JPEG, and a shared Unix nanosecond
receipt timestamp taken immediately after the native frame read, before decode.
This is a host receipt time rather than a measured exposure time. A separate backend timestamp is preserved when available, without assuming
its clock domain is Unix time.

The CLI reports achieved throughput, mean blocking capture wait, and mean
RGB decode + I420 + JPEG conversion time. Throughput excludes device opening and
includes the final file write. These timings do not measure sensor-to-host
latency or guarantee newest-frame delivery; the native backend controls buffering.
Real-camera and macOS runtime checks remain unverified.

```sh
cargo test -p cameras
cargo fmt
CXX=clang++-21 cargo clippy --all-targets --all-features
```
