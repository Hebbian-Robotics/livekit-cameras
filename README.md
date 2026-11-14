# LiveKit Cameras: browser streaming and Rust camera tools

[![CI](https://github.com/Hebbian-Robotics/livekit-cameras/actions/workflows/deploy.yml/badge.svg)](https://github.com/Hebbian-Robotics/livekit-cameras/actions/workflows/deploy.yml)

Stream phone, tablet, laptop and USB camera video into a LiveKit room. This
open-source project includes a browser publisher, a room viewer and Rust camera
libraries. Maintained by Hebbian Robotics for robotics and other shared-video
applications. This is an independent project, not an official LiveKit product.

## Choose a tool

| Task                                    | Start here                                          |
| --------------------------------------- | --------------------------------------------------- |
| Publish a browser camera                | [Browser camera](#browser-camera)                   |
| Watch published camera feeds            | [Room viewer](#room-viewer)                         |
| Publish a native USB camera             | [Native camera publisher](#native-camera-publisher) |
| Read the latest image                   | [Rust frame API](native/frames/README.md)           |
| Record timestamped H.264 video          | [Rust recorder](native/recorder/README.md)          |
| Build browser invite links and QR codes | [Rust invite API](native/invite/README.md)          |

## Browser camera

**[Open the camera page](https://hebbian-robotics.github.io/livekit-cameras/)**

Before connecting, obtain a LiveKit server URL and a room-scoped participant token.
See LiveKit's [tokens and grants](https://docs.livekit.io/frontends/reference/tokens-grants/)
for token creation and permissions.

1. Paste the URL and a camera-publishing token, or open an invite link.
2. Allow camera access, then select **Start streaming**.
3. Keep the page visible and the device unlocked while streaming.

The publisher sends video only, targeting 720p at 15 fps without simulcast.
Orientation, flip, lens and zoom controls change the published image. Device
support varies. Camera capture requires a secure origin; see MDN's
[getUserMedia security requirements](https://developer.mozilla.org/en-US/docs/Web/API/MediaDevices/getUserMedia#privacy_and_security).

### Create an invite

Install Node.js 24 and pnpm, then run `pnpm install --frozen-lockfile`.
On a trusted computer or backend, set:

```sh
export LIVEKIT_URL='wss://your-project.livekit.cloud'
export LIVEKIT_API_KEY='your-api-key'
export LIVEKIT_API_SECRET='your-api-secret'
export LIVEKIT_ROOM='your-room'
export CAMERA_IDENTITY='phone-table'
pnpm invite
```

The [example](examples/create-invite.ts) prints a one-hour publishing link and
writes `invite.svg`. API secrets remain on the issuing computer. Use a unique
participant identity for each camera.

Treat invite links and QR codes as secrets. Anyone holding one can use its room
token. Only use a hosted page you trust, or host your own copy. See the
[browser integration reference](docs/browser-integration.md) for the URL contract,
token handling and recording indicator.

## Room viewer

Select **Room viewer** and provide a URL and subscribe-enabled token. It watches
existing tracks, including cameras that join later, without opening a camera or
playing audio. Switching tabs keeps an active browser publication running.
A publishing invite cannot subscribe to the room.

With the same server credentials and room configured above, run:

```sh
pnpm viewer-invite
```

This creates a one-hour subscribe-only link. `VIEWER_IDENTITY` is optional; the
example otherwise generates one. Keep the link private. The viewer is a LiveKit
participant and consumes room resources. See LiveKit's
[track subscription documentation](https://docs.livekit.io/transport/media/subscribe/)
for the underlying model.

## Native camera publisher

The Rust publisher accepts a pre-issued camera-publishing token:

```sh
export LIVEKIT_URL='wss://your-project.livekit.cloud'
export LIVEKIT_TOKEN='<camera-publishing-token>'
cargo run --release -p livekit-camera-publisher -- --name front --device /dev/video0
cargo run -p cameras -- list
```

It publishes the newest frame at up to 15 fps, without simulcast. Persistent
device identifiers reconnect; numeric indices stop on failure to avoid selecting
a different camera. Capture runs in a subprocess so blocked reads can be stopped.
See [native publisher integration](native/publisher/README.md) before embedding it.

For the upstream SDK and examples, see
[LiveKit's Rust SDK](https://github.com/livekit/rust-sdks/tree/main/examples/local_video).
Build prerequisites and platform checks are in [Contributing](CONTRIBUTING.md#native-rust).

## Develop and host

```sh
pnpm install --frozen-lockfile
pnpm dev
pnpm build
```

Deploy `dist/` to a static HTTPS host. GitHub Actions validates changes and deploys
`main` to GitHub Pages; forks must select **GitHub Actions** as the Pages source.
Deployment needs no LiveKit credentials. For another public URL, update the
canonical and social-preview URLs in `index.html`. The preview image source is
`public/social-preview.svg`; its PNG copy is used for sharing.

See [Contributing](CONTRIBUTING.md) for validation and documentation style.

## Related tools and license

[MCAP Tools](https://github.com/Hebbian-Robotics/mcap-tools) writes MCAP recordings
and exports camera topics to MP4 from Rust/Python applications.

[MIT license](LICENSE). See [security reporting](SECURITY.md) and the hosted
[third-party notices](https://hebbian-robotics.github.io/livekit-cameras/third-party-licenses.txt).
Native dependencies and system libraries retain their own licenses.
