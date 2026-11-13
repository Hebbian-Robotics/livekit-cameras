# Latest camera frames

`livekit-camera-frames` provides the newest received image for model calls and
other still-image consumers. It keeps one JPEG per registered video track;
readers share bytes through `Arc`, without consuming the image for other readers.

```rust,no_run
use livekit_camera_frames::{CameraFrames, LatestFrame};
let frames = CameraFrames::default();
match frames.latest("track SID") {
    LatestFrame::UnknownTrack => {},
    LatestFrame::Waiting => {},
    LatestFrame::Available(frame) => {
        let age = frame.received_at.elapsed();
        let jpeg = frame.jpeg;
    }
}
```

Run `subscribe(url, token, frames.clone(), cancellation_token)` in a task to
receive LiveKit video. Supply a pre-issued subscribe token. The caller owns
credential acquisition, retries, camera naming and freshness policy. `tracks()`
returns both participant identities and track SIDs, including multiple tracks
from one participant. Cancellation, unsubscription and disconnect remove images
and join decoding tasks before returning.

`camera_frame` converts decoded I420 video to an upright JPEG using the frame's
rotation metadata. It does not infer orientation or apply browser mirroring.
Use `register` and `insert` to supply frames from native capture instead.

`captured_at_unix_ns` uses a nonzero publisher timestamp when supplied, otherwise
local arrival time. Source clocks may differ and timestamps do not establish
exposure time. `received_at` is a local monotonic clock suitable for freshness
checks. A frame can be old even though its source timestamp appears recent.
Conversion errors are logged and the last successful image remains until the
track ends; callers should reject images that exceed their age limit.

Conversion runs away from the async executor, with one conversion per track at a
time. The SDK's latest-frame queue bounds pending video instead of building a
backlog. This API exposes snapshots; live browser playback should subscribe to
LiveKit directly.
