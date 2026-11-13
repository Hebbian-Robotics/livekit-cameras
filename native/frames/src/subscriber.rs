use crate::{CameraFrames, TrackIdentity, camera_frame};
use livekit::{
    Room, RoomEvent, RoomOptions, track::RemoteTrack,
    webrtc::video_stream::native::NativeVideoStream,
};
use std::{
    collections::HashMap,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;

/// Subscribe to room video until cancelled or disconnected. The caller owns retries.
/// Tracks are removed and decoder tasks joined before returning.
///
/// # Errors
/// Returns a room connection or shutdown error.
pub async fn subscribe(
    url: &str,
    token: &str,
    frames: CameraFrames,
    stopped: CancellationToken,
) -> Result<(), livekit::RoomError> {
    let (room, mut events) = tokio::select! {
        () = stopped.cancelled() => return Ok(()),
        result = Room::connect(url, token, RoomOptions::default()) => result?,
    };
    let mut tasks: HashMap<String, (CancellationToken, tokio::task::JoinHandle<()>)> =
        HashMap::new();
    loop {
        let event = tokio::select! {
            () = stopped.cancelled() => break,
            event = events.recv() => event,
        };
        match event {
            Some(RoomEvent::TrackSubscribed {
                track: RemoteTrack::Video(track),
                participant,
                ..
            }) => {
                let sid = track.sid().to_string();
                if let Some((cancel, task)) = tasks.remove(&sid) {
                    cancel.cancel();
                    let _ = task.await;
                }
                frames.register(TrackIdentity {
                    participant: participant.identity().to_string(),
                    track_sid: sid.clone(),
                });
                let track_stopped = stopped.child_token();
                let task_stopped = track_stopped.clone();
                let output = frames.clone();
                let task_sid = sid.clone();
                let task = tokio::spawn(async move {
                    let mut stream = NativeVideoStream::new(track.rtc_track());
                    loop {
                        let frame = tokio::select! {
                            () = task_stopped.cancelled() => break,
                            frame = stream.next() => match frame { Some(frame) => frame, None => break },
                        };
                        let received_at = Instant::now();
                        let timestamp = frame
                            .frame_metadata
                            .as_ref()
                            .and_then(|metadata| metadata.user_timestamp)
                            .filter(|timestamp| *timestamp != 0)
                            .map_or_else(
                                || {
                                    SystemTime::now()
                                        .duration_since(UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_nanos()
                                },
                                |timestamp| u128::from(timestamp) * 1000,
                            );
                        // One conversion at a time. The SDK retains only the newest waiting frame.
                        let converted = tokio::task::spawn_blocking(move || {
                            camera_frame(
                                &frame.buffer.to_i420(),
                                frame.rotation,
                                timestamp,
                                received_at,
                            )
                        })
                        .await;
                        match converted {
                            Ok(Ok(image)) => {
                                output.insert(&task_sid, image);
                            }
                            Ok(Err(error)) => eprintln!("Frame conversion for {task_sid}: {error}"),
                            Err(error) => {
                                eprintln!("Frame conversion task for {task_sid}: {error}");
                                break;
                            }
                        }
                    }
                    stream.close();
                    output.remove(&task_sid);
                });
                tasks.insert(sid, (track_stopped, task));
            }
            Some(RoomEvent::TrackUnsubscribed { track, .. }) => {
                let sid = track.sid().to_string();
                if let Some((cancel, task)) = tasks.remove(&sid) {
                    cancel.cancel();
                    let _ = task.await;
                }
                frames.remove(&sid);
            }
            Some(RoomEvent::Disconnected { .. }) | None => break,
            _ => {}
        }
    }
    for (cancel, _) in tasks.values() {
        cancel.cancel();
    }
    for (sid, (_, task)) in tasks {
        let _ = task.await;
        frames.remove(&sid);
    }
    room.close().await
}
