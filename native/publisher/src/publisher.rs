use crate::{
    capture::{CaptureWorker, CapturedI420Frame},
    config::CameraConfiguration,
    diagnostics::PublishingDiagnostics,
    error::PublisherError,
};
use livekit::{
    Room, RoomEvent, RoomOptions,
    options::{FrameMetadataFeatures, TrackPublishOptions, VideoEncoding},
    prelude::{LocalTrack, LocalVideoTrack, TrackSource},
    webrtc::{
        video_frame::{FrameMetadata, I420Buffer, VideoFrame, VideoRotation},
        video_source::{RtcVideoSource, VideoResolution, native::NativeVideoSource},
    },
};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{mpsc, watch},
    time::Instant,
};
use tokio_util::sync::CancellationToken;

const MAXIMUM_FRAMES_PER_SECOND: u32 = 15;
const FRAME_INTERVAL: Duration =
    Duration::from_nanos(1_000_000_000_u64.div_ceil(MAXIMUM_FRAMES_PER_SECOND as u64));

/// Observable publication transitions within one room connection.
#[derive(Debug, Clone, Copy)]
pub enum PublicationEvent {
    Publishing,
    Reconnecting,
}

/// Publish one camera using an already issued room access token.
///
/// # Errors
/// Returns connection, capture, publication or shutdown failures.
pub async fn publish_camera(
    camera: CameraConfiguration,
    server_url: &str,
    access_token: &str,
    stopped: CancellationToken,
) -> Result<(), PublisherError> {
    let capture = CaptureWorker::start(&camera)?;
    let publication_stopped = stopped.child_token();
    let publication = publish_camera_frames(
        &camera,
        server_url,
        access_token,
        capture.frames.clone(),
        publication_stopped.clone(),
        |_| {},
    );
    tokio::pin!(publication);
    let publication_result = tokio::select! {
        () = capture.finished() => {
            publication_stopped.cancel();
            publication.await
        }
        result = &mut publication => result,
    };
    let capture_result = capture.stop().await;
    publication_result?;
    capture_result
}

/// Publish frames from an existing capture worker for one room connection.
///
/// # Errors
/// Returns room connection, publication or shutdown failures.
pub async fn publish_camera_frames(
    camera: &CameraConfiguration,
    server_url: &str,
    access_token: &str,
    mut frames: watch::Receiver<Option<Arc<CapturedI420Frame>>>,
    stopped: CancellationToken,
    observe: impl Fn(PublicationEvent),
) -> Result<(), PublisherError> {
    let mut options = RoomOptions::default();
    options.auto_subscribe = false;
    let (room, mut events) = tokio::select! {
        () = stopped.cancelled() => return Ok(()),
        connected = Room::connect(server_url, access_token, options) => connected?,
    };
    let publication_result =
        publish_frames(camera, &room, &mut events, &mut frames, &stopped, observe).await;
    let disconnect_result = room.close().await;
    // Closing is attempted even after capture or publication fails.
    publication_result?;
    disconnect_result?;
    Ok(())
}

async fn publish_frames(
    camera: &CameraConfiguration,
    room: &Room,
    events: &mut mpsc::UnboundedReceiver<RoomEvent>,
    frames: &mut watch::Receiver<Option<Arc<CapturedI420Frame>>>,
    stopped: &CancellationToken,
    observe: impl Fn(PublicationEvent),
) -> Result<(), PublisherError> {
    let name = camera.name();
    let participant = room.local_participant();
    let mut diagnostics = None;
    let mut source: Option<NativeVideoSource> = None;
    let mut next_publish_at = Instant::now();
    loop {
        tokio::select! {
            biased;
            () = stopped.cancelled() => return Ok(()),
            event = events.recv() => {
                match event {
                    Some(RoomEvent::Disconnected { reason }) => return Err(PublisherError::Disconnected(format!("{reason:?}"))),
                    Some(RoomEvent::Reconnecting) => observe(PublicationEvent::Reconnecting),
                    Some(RoomEvent::Reconnected) if source.is_some() => observe(PublicationEvent::Publishing),
                    None => return Err(PublisherError::Disconnected("event stream closed".to_owned())),
                    Some(_) => {}
                }
                continue;
            }
            changed = frames.changed() => {
                if changed.is_err() { return Ok(()); }
            }
        }
        tokio::select! {
            () = stopped.cancelled() => return Ok(()),
            () = tokio::time::sleep_until(next_publish_at) => {}
        }
        // Read only after pacing: any intervening frames have replaced this slot.
        let Some(frame) = frames.borrow_and_update().clone() else {
            continue;
        };
        if source.is_none() {
            let video_source = NativeVideoSource::new(
                VideoResolution {
                    width: frame.i420.width,
                    height: frame.i420.height,
                },
                false,
            );
            let track = LocalVideoTrack::create_video_track(
                name,
                RtcVideoSource::Native(video_source.clone()),
            );
            let mut metadata_features = FrameMetadataFeatures::default();
            metadata_features.user_timestamp = true;
            let options = TrackPublishOptions {
                source: TrackSource::Camera,
                video_codec: camera.publishing.codec.livekit_codec(),
                simulcast: false,
                video_encoding: Some(VideoEncoding {
                    max_framerate: f64::from(MAXIMUM_FRAMES_PER_SECOND),
                    max_bitrate: 1_500_000,
                }),
                frame_metadata_features: metadata_features,
                ..Default::default()
            };
            tokio::select! {
                () = stopped.cancelled() => return Ok(()),
                published = participant.publish_track(LocalTrack::Video(track.clone()), options) => { published?; }
            }
            eprintln!(
                "Camera {name}: publishing {}x{}",
                frame.i420.width, frame.i420.height
            );
            diagnostics = Some(PublishingDiagnostics::start(
                name.to_owned(),
                track,
                camera.publishing.diagnostics,
            ));
            source = Some(video_source);
            observe(PublicationEvent::Publishing);
        }
        if let Some(video_source) = &source {
            let Some(frame) = frames.borrow_and_update().clone() else {
                continue;
            };
            let video_frame = livekit_frame(&frame)?;
            let _ = video_source.capture_frame(&video_frame);
            if let Some(diagnostics) = &diagnostics {
                diagnostics.record_submission(frame.captured_at_unix_ns);
            }
            // Schedule from actual submission, so slow reads/encodes never cause catch-up bursts.
            next_publish_at = Instant::now() + FRAME_INTERVAL;
        }
    }
}

fn livekit_frame(
    frame: &crate::capture::CapturedI420Frame,
) -> Result<VideoFrame<I420Buffer>, PublisherError> {
    let planes = frame.i420.planes()?;
    // Explicit packed strides match the cameras crate's I420 plane layout.
    let mut buffer = I420Buffer::with_strides(
        frame.i420.width,
        frame.i420.height,
        frame.i420.width,
        frame.i420.width.div_ceil(2),
        frame.i420.width.div_ceil(2),
    );
    let (target_y, target_u, target_v) = buffer.data_mut();
    target_y.copy_from_slice(planes.y);
    target_u.copy_from_slice(planes.u);
    target_v.copy_from_slice(planes.v);
    let timestamp_us = u64::try_from(frame.captured_at_unix_ns / 1_000)
        .map_err(|_| PublisherError::TimestampOutOfRange)?;
    Ok(VideoFrame {
        rotation: VideoRotation::VideoRotation0,
        timestamp_us: i64::try_from(timestamp_us)
            .map_err(|_| PublisherError::TimestampOutOfRange)?,
        frame_metadata: Some(FrameMetadata {
            user_timestamp: Some(timestamp_us),
            frame_id: None,
            user_data: None,
        }),
        buffer,
    })
}
