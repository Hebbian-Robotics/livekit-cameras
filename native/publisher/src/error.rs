#[derive(Debug, thiserror::Error)]
pub enum PublisherError {
    #[error(transparent)]
    Camera(#[from] cameras::CameraError),
    #[error("LiveKit room: {0}")]
    Room(#[from] livekit::RoomError),
    #[error("invalid configuration: {0}")]
    Configuration(String),
    #[error("system operation: {0}")]
    Io(#[from] std::io::Error),
    #[error("camera subprocess exited with {0}")]
    CaptureProcess(std::process::ExitStatus),
    #[error("publisher task: {0}")]
    Task(#[from] tokio::task::JoinError),
    #[error("capture timestamp is outside the supported microsecond range")]
    TimestampOutOfRange,
    #[error("LiveKit disconnected: {0}")]
    Disconnected(String),
}
