#[derive(Debug, thiserror::Error)]
pub enum CameraError {
    #[error("camera backend: {0}")]
    Backend(#[from] nokhwa::NokhwaError),
    #[error("file operation: {0}")]
    Io(#[from] std::io::Error),
    #[error("JPEG conversion: {0}")]
    Image(#[from] image::ImageError),
    #[error("invalid camera configuration: {0}")]
    Configuration(String),
    #[error("camera permission denied or initialization failed")]
    PermissionDenied,
    #[error("camera permission callback disconnected")]
    InitializationDisconnected,
    #[error("this platform has no supported native camera backend")]
    UnsupportedPlatform,
    #[error("invalid RGB frame: expected nonzero dimensions and width * height * 3 bytes")]
    InvalidFrame,
    #[error("system clock is before the Unix epoch: {0}")]
    Clock(#[from] std::time::SystemTimeError),
}
