//! Timestamp-preserving camera video encoding.

mod nut;
mod room;
pub use room::record_room;
mod video;
pub use video::{CameraWriter, hardware_encoder_available};

use std::time::{SystemTime, UNIX_EPOCH};

pub enum VideoEvent<'a> {
    CameraError {
        camera: &'a str,
        detail: &'a str,
        timestamp: u64,
    },
    SegmentStarted {
        camera: &'a str,
        segment: &'a str,
        timestamp: u64,
        width: u32,
        height: u32,
    },
    SegmentEnded {
        camera: &'a str,
        segment: &'a str,
        timestamp: u64,
    },
    Statistics {
        camera: &'a str,
        received: u64,
        encoded: u64,
        timestamp: u64,
    },
}

pub trait VideoOutput: Send + Sync {
    /// Choose a stable output name for a participant and track.
    fn camera_name(&self, _participant_identity: &str, track_sid: &str) -> Option<String> {
        Some(track_sid.to_ascii_lowercase())
    }
    /// # Errors
    /// Returns an error when the destination cannot accept the packet.
    fn video(&self, camera: &str, timestamp: u64, packet: Vec<u8>) -> Result<(), RecorderError>;
    /// # Errors
    /// Returns an error when the destination cannot accept the event.
    fn event(&self, event: VideoEvent<'_>) -> Result<(), RecorderError>;
}

#[derive(Debug, thiserror::Error)]
pub enum RecorderError {
    #[error("LiveKit: {0}")]
    Room(#[from] livekit::RoomError),
    #[error("task: {0}")]
    Task(#[from] tokio::task::JoinError),
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("camera: {0}")]
    Camera(#[from] cameras::CameraError),
    #[error("invalid recording data: {0}")]
    InvalidData(String),
    #[error("{operation} exited with {status}")]
    Process {
        operation: &'static str,
        status: std::process::ExitStatus,
    },
    #[error("{0} timed out")]
    Timeout(&'static str),
    #[error("output: {0}")]
    Output(String),
}

fn unix_nanoseconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .try_into()
        .unwrap_or(u64::MAX)
}
