//! Native camera capture, JPEG and planar I420 without a publishing transport.
pub mod config;
pub mod conversion;
mod error;
pub use config::validate_camera_name;
pub use error::CameraError;

use conversion::{I420Frame, rgb_to_i420, rgb_to_jpeg};
use nokhwa::{
    Camera,
    pixel_format::RgbFormat,
    utils::{
        ApiBackend, CameraFormat, CameraIndex, CameraInfo, FrameFormat, RequestedFormat,
        RequestedFormatType,
    },
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const CAPTURE_WIDTH: u32 = 1280;
pub const CAPTURE_HEIGHT: u32 = 720;
pub const CAPTURE_FPS: u32 = 15;

/// Initialize camera permissions before enumeration or opening a device.
///
/// # Errors
/// Returns an error if permission is denied or the native callback disconnects.
pub fn initialize() -> Result<(), CameraError> {
    let (permission_sender, permission_receiver) = std::sync::mpsc::sync_channel(1);
    nokhwa::nokhwa_initialize(move |allowed| {
        let _ = permission_sender.send(allowed);
    });
    if permission_receiver
        .recv()
        .map_err(|_| CameraError::InitializationDisconnected)?
    {
        Ok(())
    } else {
        Err(CameraError::PermissionDenied)
    }
}

fn native_backend() -> Result<ApiBackend, CameraError> {
    backend_for_platform(std::env::consts::OS)
}

fn backend_for_platform(platform: &str) -> Result<ApiBackend, CameraError> {
    match platform {
        "linux" => Ok(ApiBackend::Video4Linux),
        "macos" => Ok(ApiBackend::AVFoundation),
        "windows" => Ok(ApiBackend::MediaFoundation),
        _ => Err(CameraError::UnsupportedPlatform),
    }
}

/// Enumerate identifiers accepted by `CameraCapture::open`.
///
/// # Errors
/// Returns native enumeration or unsupported-platform errors.
pub fn list_cameras() -> Result<Vec<CameraInfo>, CameraError> {
    Ok(nokhwa::query(native_backend()?)?)
}

fn device_index(device: &str) -> Result<CameraIndex, CameraError> {
    if device.bytes().all(|character| character.is_ascii_digit()) && !device.is_empty() {
        return Ok(CameraIndex::Index(device.parse().map_err(|_| {
            CameraError::Configuration(format!("camera index out of range: {device}"))
        })?));
    }
    if cfg!(target_os = "linux") {
        // nokhwa's V4L2 backend accepts indices only. Resolve stable symlinks first.
        let resolved = std::fs::canonicalize(device)?;
        let index_text = resolved
            .to_str()
            .and_then(|path| path.strip_prefix("/dev/video"))
            .ok_or_else(|| {
                CameraError::Configuration(format!(
                    "expected a /dev/videoN device or symlink, got {device}"
                ))
            })?;
        let index = index_text
            .parse()
            .map_err(|_| CameraError::Configuration(format!("invalid V4L2 device: {device}")))?;
        Ok(CameraIndex::Index(index))
    } else {
        Ok(CameraIndex::String(device.to_owned()))
    }
}

#[derive(Debug)]
pub struct CapturedFrame {
    /// Receipt time immediately after native capture, before colour conversion.
    pub captured_at_unix_ns: u128,
    /// Native timestamp, when supplied; its clock domain is backend-specific.
    pub backend_timestamp: Option<Duration>,
    pub i420: I420Frame,
    /// JPEG bytes, empty for video-only conversion.
    pub jpeg: Vec<u8>,
    pub capture_wait: Duration,
    pub conversion_latency: Duration,
}

/// A freshly dequeued native frame, before color conversion.
pub struct RawCapturedFrame {
    buffer: nokhwa::Buffer,
    captured_at_unix_ns: u128,
    capture_wait: Duration,
}

pub struct CameraCapture {
    camera: Camera,
}

impl CameraCapture {
    /// Open an index, Linux path/symlink, or native identifier on macOS or Windows.
    /// Prefer MJPEG on USB; fall back to native decodable formats (e.g. macOS NV12).
    ///
    /// # Errors
    /// Returns configuration or native errors for absent, busy or unsupported devices.
    pub fn open(device: &str) -> Result<Self, CameraError> {
        let index = device_index(device)?;
        #[cfg(target_os = "macos")]
        let formats = nokhwa_bindings_macos::AVCaptureDevice::new(&index)?.supported_formats()?;
        #[cfg(not(target_os = "macos"))]
        let formats = Camera::with_backend(
            index.clone(),
            RequestedFormat::new::<RgbFormat>(RequestedFormatType::None),
            native_backend()?,
        )?
        .compatible_camera_formats()?;
        let decodable_formats: Vec<_> = formats
            .into_iter()
            .filter(|format| {
                use nokhwa::FormatDecoder;
                RgbFormat::FORMATS.contains(&format.format())
            })
            .collect();
        let selected = decodable_formats
            .iter()
            .min_by_key(|format| {
                (
                    format.format() != FrameFormat::MJPEG,
                    u64::from(format.width().abs_diff(CAPTURE_WIDTH))
                        + u64::from(format.height().abs_diff(CAPTURE_HEIGHT)),
                    format.frame_rate().abs_diff(CAPTURE_FPS),
                )
            })
            .copied()
            .ok_or_else(|| {
                CameraError::Configuration(format!("no RGB-decodable format for {device}"))
            })?;
        let mut camera = Camera::with_backend(
            index,
            RequestedFormat::new::<RgbFormat>(RequestedFormatType::Exact(selected)),
            native_backend()?,
        )?;
        camera.open_stream()?;
        Ok(Self { camera })
    }

    #[must_use]
    pub fn format(&self) -> CameraFormat {
        self.camera.camera_format()
    }

    /// Read a native frame promptly, before potentially slow color conversion.
    ///
    /// # Errors
    /// Returns native capture or clock errors.
    pub fn read_raw_frame(&mut self) -> Result<RawCapturedFrame, CameraError> {
        let capture_started = Instant::now();
        let buffer = self.camera.frame()?;
        Ok(RawCapturedFrame {
            buffer,
            captured_at_unix_ns: SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos(),
            capture_wait: capture_started.elapsed(),
        })
    }

    /// Read one frame and produce both encodings with one shared timestamp.
    ///
    /// # Errors
    /// Returns capture, decode, clock or conversion errors.
    pub fn capture(&mut self) -> Result<CapturedFrame, CameraError> {
        self.read_raw_frame()?.convert(true)
    }
}

impl RawCapturedFrame {
    /// Convert a native frame for streaming without the unused JPEG encoding.
    ///
    /// # Errors
    /// Returns decode or conversion errors.
    pub fn into_video(self) -> Result<CapturedFrame, CameraError> {
        self.convert(false)
    }

    fn convert(self, include_jpeg: bool) -> Result<CapturedFrame, CameraError> {
        let conversion_started = Instant::now();
        let rgb = self.buffer.decode_image::<RgbFormat>()?;
        // ImageBuffer permits trailing bytes; encode only its declared pixels.
        let rgb_length = usize::try_from(u64::from(rgb.width()) * u64::from(rgb.height()) * 3)
            .map_err(|_| CameraError::InvalidFrame)?;
        let pixels = rgb
            .as_raw()
            .get(..rgb_length)
            .ok_or(CameraError::InvalidFrame)?;
        let i420 = rgb_to_i420(rgb.width(), rgb.height(), pixels)?;
        let jpeg = if include_jpeg {
            rgb_to_jpeg(rgb.width(), rgb.height(), pixels)?
        } else {
            Vec::new()
        };
        Ok(CapturedFrame {
            captured_at_unix_ns: self.captured_at_unix_ns,
            backend_timestamp: self.buffer.capture_timestamp(),
            i420,
            jpeg,
            capture_wait: self.capture_wait,
            conversion_latency: conversion_started.elapsed(),
        })
    }
}

impl Drop for CameraCapture {
    fn drop(&mut self) {
        let _ = self.camera.stop_stream();
    }
}

#[cfg(test)]
mod platform_tests {
    use super::*;

    #[test]
    fn each_supported_platform_selects_its_native_backend() {
        assert_eq!(
            backend_for_platform("windows").unwrap(),
            ApiBackend::MediaFoundation
        );
        assert_eq!(
            backend_for_platform("macos").unwrap(),
            ApiBackend::AVFoundation
        );
        assert_eq!(
            backend_for_platform("linux").unwrap(),
            ApiBackend::Video4Linux
        );
        assert!(backend_for_platform("other").is_err());
    }
}
