# LiveKit Cameras: browser streaming and Rust camera tools

[![CI](https://github.com/Hebbian-Robotics/livekit-cameras/actions/workflows/deploy.yml/badge.svg)](https://github.com/Hebbian-Robotics/livekit-cameras/actions/workflows/deploy.yml)

Stream phone, tablet, laptop and USB webcam video into a **LiveKit room**.
LiveKit Cameras combines an open-source browser camera publisher and room viewer
with Rust libraries for native camera capture, WebRTC publishing, latest-frame
snapshots and video recording. Maintained by Hebbian Robotics for robotics,
teleoperation and other applications that need shared camera feeds.

## Choose a camera tool

| What you need                                                      | Tool                                                                                          |
| ------------------------------------------------------------------ | --------------------------------------------------------------------------------------------- |
| Use a phone, tablet or laptop as a LiveKit camera                  | [Browser camera publisher](#browser-camera), hosted on GitHub Pages                           |
| Watch several published cameras without opening a USB device again | [LiveKit room viewer](#room-viewer)                                                           |
| Publish a local USB camera from Rust or the command line           | [Native camera publisher](#native-camera-publisher)                                           |
| Get the latest camera image for a model or application             | [Rust latest-frame API](native/frames/README.md)                                              |
| Record LiveKit camera tracks as H.264 video                        | [Rust recording primitives](native/recorder/README.md)                                        |
| Create browser invite links and QR codes on your backend           | [Rust invite API](native/invite/README.md) or [TypeScript example](examples/create-invite.ts) |

## Browser camera

Open a link or scan a QR code, allow camera access, and tap **Start streaming**.
No app install or device-side backend connection is needed.

**[Open the camera page](https://hebbian-robotics.github.io/livekit-cameras/)**

We use phones as extra cameras in our robotics setups when we don't have enough
dedicated cameras. This app works with any application that can subscribe to a
LiveKit camera video track.

### Use it

You need a LiveKit Cloud project or a reachable self-hosted LiveKit server, and a
participant token for the room you want to publish into. Paste the server's
`wss://` URL and token into the page, or generate an invite link below.

The app publishes video only, targeting 720p at 15 fps without simulcast. It
supports portrait/landscape output, a 180-degree flip, rear lens selection and
zoom where the browser exposes them, reconnect status, and a screen wake lock
where supported. Camera controls affect the published video, not just the preview.
Keep the page visible and the device unlocked; mobile browsers may suspend capture
in the background. Lens and zoom availability varies by device and browser.

Subscribers join the same room using their own tokens and subscribe to the
camera track. The publisher's LiveKit participant identity is the camera name;
use a different identity for every camera. Reusing an identity can disconnect an
existing participant.

### Generate an invite and QR code

Install Node.js 24 and pnpm, then run `pnpm install --frozen-lockfile`.
Set these environment variables on your trusted computer or backend:

```sh
export LIVEKIT_URL='wss://your-project.livekit.cloud'
export LIVEKIT_API_KEY='your-api-key'
export LIVEKIT_API_SECRET='your-api-secret'
export LIVEKIT_ROOM='your-room'
export CAMERA_IDENTITY='phone-table'
pnpm invite
```

The [TypeScript example](examples/create-invite.ts) prints a link and writes
`invite.svg`. It signs a token valid for one hour that permits joining the named
room and publishing camera video, with subscription and data publishing disabled.
API credentials are used locally and are never included in the link. Set
`CAMERA_PAGE_URL` to use a self-hosted copy of the app.

Your own backend can implement the same interface in any language using a LiveKit
server SDK. There is no dependency on this repository's TypeScript code:

```text
https://your-camera-page.example/#url=<encoded-livekit-url>&token=<encoded-jwt>
```

Construct the fragment with a URL encoder, such as `URLSearchParams`. The JWT
must include a participant identity (`sub`) and grants to join your chosen room
and publish camera video. LiveKit verifies the token; the page only decodes its
identity and expiration for display. A room name alone does not grant access.

### Optional recording integration

Set LiveKit room metadata to `{"recording": true}` to show a recording indicator
and lock orientation, flip, lens and zoom controls. Set `recording` to `false` or
omit it to unlock. This is an app convention, not automatic detection of LiveKit
Egress or an access-control mechanism. Other metadata fields are ignored.

### Privacy and access

Treat invite links and QR codes as secrets: anyone holding one can use its token
until it expires. Use short-lived, room-scoped, camera-only tokens and unique
participant identities. The hosting operator's JavaScript can read tokens, so
only use a hosted page you trust or host your own copy.

Tokens in URL fragments are not sent in HTTP page requests. Manually pasted
tokens stay in memory; the app does not persist them. Invite URLs can remain in
browser history. Lens selection is stored locally. There are no analytics,
third-party runtime scripts, microphone capture, or API secrets in the app.
Video travels through your configured LiveKit service to authorized subscribers.

### Develop and host

```sh
pnpm install --frozen-lockfile
pnpm dev
pnpm format
pnpm lint
pnpm typecheck
pnpm test
pnpm build
```

Serve `dist/` on any static HTTPS host. Relative asset URLs support GitHub Pages
project paths. For a deployment at a different public URL, update the canonical
and social-preview URLs in `index.html`. The social preview source lives in
`public/social-preview.svg`, with a PNG copy for sharing services. Camera access requires HTTPS (or localhost during development).
For development on a physical phone, use an HTTPS development origin.

The GitHub Actions workflow validates every push and pull request, and deploys
`main` to GitHub Pages. Set **Settings → Pages → Source → GitHub Actions** in
your fork. No LiveKit credentials are needed to build or deploy the page.

LiveKit supplies the underlying [camera publishing APIs](https://docs.livekit.io/transport/media/publish/)
and [LiveKit Meet for testing rooms](https://docs.livekit.io/reference/developer-tools/livekit-cli/rooms/).
This project provides a browser camera interface around those APIs and is
not an official LiveKit product.

## Room viewer

The **Room viewer** tab watches video already published to a LiveKit room,
without opening the physical camera a second time. This lets other subscribers
continue receiving the same track. Switching between the Stream and Room viewer
tabs keeps an active browser-camera publication running. After camera access is
granted, the Stream tab can also select from available local camera devices.

A camera that is only available to a local application and is not published to
LiveKit cannot be viewed through the room viewer. Direct browser access to a
camera may conflict with another application already using it, so this page does
not offer a separate local-only camera preview.

Room viewer displays video tracks from participants in a LiveKit room, including
cameras that join later. It does not request a camera or play audio. Provide the
LiveKit server URL and a token granting room join and subscription. A publishing
invite from `pnpm invite` is publish-only and cannot watch the room. The viewer
is a participant and consumes your LiveKit project's resources.

To create a one-hour, subscribe-only viewer link on a trusted computer, set
`LIVEKIT_URL`, `LIVEKIT_API_KEY`, `LIVEKIT_API_SECRET`, and `LIVEKIT_ROOM` as
shown above, then run:

```sh
pnpm viewer-invite
```

Optionally set `VIEWER_IDENTITY`; otherwise the example generates a unique
identity. Keep the resulting link secret: it permits the holder to watch media
in that room. As with publishing links, the token is carried in the URL fragment
and may remain in browser history. The page never receives the API secret.

## Native camera publisher

The Rust workspace includes a native camera capture crate (`cameras`) and a
LiveKit publisher crate (`livekit-camera-publisher`). The publisher accepts a
LiveKit URL and a pre-issued camera-publishing token. Generate the token on a
trusted backend and use a distinct identity for each camera.

```sh
export LIVEKIT_URL='wss://your-project.livekit.cloud'
export LIVEKIT_TOKEN='<camera-publishing-token>'
cargo run --release -p livekit-camera-publisher -- --name front --device /dev/video0
cargo run -p cameras -- list
```

The native publisher uses the newest available frame, at up to 15 fps, and
publishes a camera track without simulcast. It records host receipt time as
frame metadata; this is not the sensor exposure time. Persistent device paths
retry on failure, whereas numeric indices stop to avoid accidentally selecting
a different camera after reconnection. Capture runs in a subprocess so a
blocked camera read can be stopped.

Library users can use the `cameras` crate directly or `publish_camera` in the
publisher crate with their own URL and token. `CaptureWorker` starts the
current executable with the private `__capture` subcommand. An executable
embedding the worker must dispatch that subcommand to `capture::run_capture_process`
before starting its normal argument parser. This also applies to snapshot
consumers using the worker directly.

Native Linux builds need libclang and GLib development headers, and the
LiveKit WebRTC C++ build needs clang 21 or newer. For example:

```sh
CXX=clang++-21 cargo build -p livekit-camera-publisher
cargo fmt --all
CXX=clang++-21 cargo clippy --all-targets --all-features
CXX=clang++-21 cargo test --workspace
```

Real-camera and live-room behavior must be verified on the intended hardware.
The capture backend supports Linux, macOS and Windows, but native publishing
and camera behavior have not been validated on macOS or Windows.

For an upstream comparison and additional diagnostic tools, see LiveKit's
[local video example](https://github.com/livekit/rust-sdks/tree/main/examples/local_video).

## Related MCAP recording tools

[MCAP Tools](https://github.com/Hebbian-Robotics/mcap-tools) provides a Rust and
Python writer for standard MCAP files, ROS 2/Foxglove message encodings, and
camera video export from MCAP to MP4. Use it when building recording workflows
that store camera video alongside telemetry.

## License

MIT. Copyright Hebbian Robotics. See [LICENSE](LICENSE). Contributions are
welcome; see [CONTRIBUTING.md](CONTRIBUTING.md). To report a vulnerability,
see [SECURITY.md](SECURITY.md).

The hosted browser bundle includes third-party code under its own licenses.
Its [third-party license notices](https://hebbian-robotics.github.io/livekit-cameras/third-party-licenses.txt)
are generated from the production dependencies at build time. The source
dependencies, WebRTC archive, and native system libraries have their own
licenses; review and include applicable notices when distributing native builds.

Native recording primitives are documented in [native/recorder](native/recorder/README.md).

For still images used by model calls, see the [latest-frame API](native/frames/README.md).

For browser invite URLs and QR codes, see [native/invite](native/invite/README.md).
