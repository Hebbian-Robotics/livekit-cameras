# LiveKit camera recorder

`livekit-camera-recorder` subscribes to LiveKit video and sends timestamped H.264
packets and segment events to a caller-provided `VideoOutput`. It uses the `ffmpeg`
executable on PATH. Linux probes Intel VAAPI and falls back to `libx264`; other
platforms use `libx264`. FFmpeg must provide that encoder.

Call `record_room` with a LiveKit URL, a pre-issued subscribe token, an
`Arc<dyn VideoOutput>`, an encoder selection and a cancellation token. Cancel to
stop receiving frames and finalize each encoder before the function returns.
The caller owns authentication, reconnection and its output format. `CameraWriter`
can also encode I420 frames directly.

The default output names use LiveKit track SIDs, so several video tracks from one
participant remain distinct. Override `VideoOutput::camera_name` to select a
name based on participant identity and track SID, or return `None` to skip a track.
Names must satisfy the camera crate's lowercase name rules and should be unique.
Publisher timestamps, when
present, are Unix microseconds; otherwise frame arrival time is used. These are
not guaranteed exposure times or synchronized clocks. Frames can be dropped.
The synchronous output methods may be called concurrently; a slow destination
can slow encoding and increase dropped frames. Return an error when output fails.

A destination can be shared with the recorder as follows (error handling and
credential acquisition belong to the application):

```rust,ignore
let output: Arc<dyn VideoOutput> = Arc::new(my_output);
let stop = CancellationToken::new();
record_room(&server_url, &subscribe_token, &output, false, stop.clone()).await?;
```

Call `stop.cancel()` from another task to finalize the recording. The packet
callback receives Annex B H.264 with Unix nanosecond timestamps. Segment events
report dimensions and boundaries; output implementations should synchronize their
own writes. A track error is reported as a `CameraError` event, and other tracks
continue recording.
