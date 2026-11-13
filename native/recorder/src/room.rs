use crate::{CameraWriter, RecorderError, VideoEvent, VideoOutput, unix_nanoseconds};
use cameras::conversion::I420Frame;
use livekit::{
    Room, RoomEvent, RoomOptions,
    track::{RemoteTrack, RemoteVideoTrack},
    webrtc::{
        video_frame::{BoxVideoFrame, VideoBuffer},
        video_stream::native::NativeVideoStream,
    },
};
use std::{collections::HashMap, sync::Arc};
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;

fn frame_time(user_timestamp: Option<u64>, arrival_ns: u64) -> Result<u64, RecorderError> {
    match user_timestamp.filter(|timestamp| *timestamp != 0) {
        Some(timestamp) => timestamp.checked_mul(1000).ok_or_else(|| {
            RecorderError::InvalidData("publisher timestamp exceeds nanosecond range".into())
        }),
        None => Ok(arrival_ns),
    }
}

struct CameraTask {
    stopped: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

/// Record subscribed video tracks until cancellation.
///
/// # Errors
/// Returns connection, room closure, or task errors.
pub async fn record_room(
    url: &str,
    token: &str,
    archive: &Arc<dyn VideoOutput>,
    hardware: bool,
    stopped: CancellationToken,
) -> Result<(), RecorderError> {
    let (room, mut events) = tokio::select! {
        () = stopped.cancelled() => return Ok(()),
        result = Room::connect(url, token, RoomOptions::default()) => result?,
    };
    let mut recorders: HashMap<String, CameraTask> = HashMap::new();
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
                let camera_name = participant.identity().to_string();
                if cameras::validate_camera_name(&camera_name).is_err() {
                    continue;
                }
                if let Some(previous) = recorders.remove(&camera_name) {
                    previous.stopped.cancel();
                    if let Err(error) = previous.task.await {
                        eprintln!("Recording {camera_name}: {error}");
                    }
                }
                let camera_stopped = stopped.child_token();
                let task_stopped = camera_stopped.clone();
                let camera_archive = archive.clone();
                let task_name = camera_name.clone();
                eprintln!("Recording {camera_name}");
                let task = tokio::spawn(async move {
                    let result = record_camera(
                        track,
                        task_name.clone(),
                        hardware,
                        camera_archive.clone(),
                        task_stopped,
                    )
                    .await;
                    if let Err(error) = result {
                        eprintln!("Recording {task_name} failed: {error}");
                        if let Err(log_error) = camera_archive.event(VideoEvent::CameraError {
                            camera: &task_name,
                            detail: &error.to_string(),
                            timestamp: unix_nanoseconds(),
                        }) {
                            eprintln!("Could not archive recording error: {log_error}");
                        }
                    }
                });
                recorders.insert(
                    camera_name,
                    CameraTask {
                        stopped: camera_stopped,
                        task,
                    },
                );
            }
            None => break,
            _ => {}
        }
    }
    // Stop reception first; cooperative cancellation leaves every writer alive
    // until its current frame and final archive pass are finished.
    let room_result = room.close().await;
    for recorder in recorders.values() {
        recorder.stopped.cancel();
    }
    for (name, recorder) in recorders {
        if let Err(error) = recorder.task.await {
            eprintln!("Recording {name}: {error}");
        }
    }
    room_result?;
    Ok(())
}

async fn record_camera(
    track: RemoteVideoTrack,
    camera: String,
    hardware: bool,
    archive: Arc<dyn VideoOutput>,
    stopped: CancellationToken,
) -> Result<(), RecorderError> {
    let mut stream = NativeVideoStream::new(track.rtc_track()); // SDK bounded queue: newest frame only.
    let mut writer = CameraWriter::new(camera, hardware, archive)?;
    let result = loop {
        let frame = tokio::select! {
            () = stopped.cancelled() => break Ok(()),
            frame = stream.next() => match frame { Some(frame) => frame, None => break Ok(()) },
        };
        let timestamp = match frame_time(
            frame
                .frame_metadata
                .as_ref()
                .and_then(|metadata| metadata.user_timestamp),
            unix_nanoseconds(),
        ) {
            Ok(timestamp) => timestamp,
            Err(error) => break Err(error),
        };
        let result = tokio::task::spawn_blocking(move || {
            let result =
                pack_i420(&frame).and_then(|i420| writer.write(&i420, frame.rotation, timestamp));
            (writer, result)
        })
        .await?;
        writer = result.0;
        if let Err(error) = result.1 {
            break Err(error);
        }
    };
    stream.close();
    let finish_result = tokio::task::spawn_blocking(move || writer.finish()).await?;
    result?;
    finish_result
}

fn pack_i420(frame: &BoxVideoFrame) -> Result<I420Frame, RecorderError> {
    let buffer = frame.buffer.to_i420();
    let width = buffer.width();
    let height = buffer.height();
    let (stride_y, stride_u, stride_v) = buffer.strides();
    let (plane_y, plane_u, plane_v) = buffer.data();
    let mut data = Vec::new();
    for (plane, stride, columns, rows) in [
        (plane_y, stride_y, width, height),
        (plane_u, stride_u, width.div_ceil(2), height.div_ceil(2)),
        (plane_v, stride_v, width.div_ceil(2), height.div_ceil(2)),
    ] {
        let stride = usize::try_from(stride)
            .map_err(|_| RecorderError::InvalidData("stride overflow".into()))?;
        let columns = usize::try_from(columns)
            .map_err(|_| RecorderError::InvalidData("width overflow".into()))?;
        let rows = usize::try_from(rows)
            .map_err(|_| RecorderError::InvalidData("height overflow".into()))?;
        if stride == 0 || columns > stride {
            return Err(RecorderError::InvalidData("invalid I420 stride".into()));
        }
        for row in plane.chunks(stride).take(rows) {
            data.extend_from_slice(
                row.get(..columns)
                    .ok_or_else(|| RecorderError::InvalidData("truncated I420 row".into()))?,
            );
        }
    }
    let result = I420Frame {
        width,
        height,
        data,
    };
    result.planes()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::frame_time;

    #[test]
    fn uses_nonzero_publisher_timestamp_or_arrival_time() {
        assert_eq!(
            frame_time(Some(1_700_000_000_123_456), 42).unwrap(),
            1_700_000_000_123_456_000
        );
        assert_eq!(frame_time(None, 42).unwrap(), 42);
        assert_eq!(frame_time(Some(0), 42).unwrap(), 42);
    }
}
