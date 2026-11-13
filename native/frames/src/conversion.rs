use crate::CameraFrame;
use livekit::webrtc::video_frame::{
    VideoBuffer, VideoFormatType, VideoRotation, native::VideoFrameBufferExt,
};
use std::{sync::Arc, time::Instant};

/// Convert a decoded video frame to an upright JPEG.
///
/// # Errors
/// Returns an error when dimensions, conversion or JPEG encoding are invalid.
pub fn camera_frame(
    buffer: &livekit::webrtc::video_frame::I420Buffer,
    rotation: VideoRotation,
    captured_at_unix_ns: u128,
    received_at: Instant,
) -> Result<CameraFrame, FrameError> {
    let jpeg = encode_jpeg(buffer, rotation).ok_or(FrameError::InvalidImage)?;
    let (width, height) = match rotation {
        VideoRotation::VideoRotation90 | VideoRotation::VideoRotation270 => {
            (buffer.height(), buffer.width())
        }
        _ => (buffer.width(), buffer.height()),
    };
    Ok(CameraFrame {
        jpeg: Arc::from(jpeg),
        width,
        height,
        captured_at_unix_ns,
        received_at,
    })
}

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("invalid image dimensions, conversion or JPEG encoding")]
    InvalidImage,
}

fn encode_jpeg(
    buffer: &livekit::webrtc::video_frame::I420Buffer,
    rotation: VideoRotation,
) -> Option<Vec<u8>> {
    let width = buffer.width();
    let height = buffer.height();
    let mut rgba = vec![0; usize::try_from(width.checked_mul(height)?.checked_mul(4)?).ok()?];
    buffer.to_argb(
        VideoFormatType::ABGR,
        &mut rgba,
        width.checked_mul(4)?,
        i32::try_from(width).ok()?,
        i32::try_from(height).ok()?,
    );
    let (out_width, out_height) = match rotation {
        VideoRotation::VideoRotation90 | VideoRotation::VideoRotation270 => (height, width),
        _ => (width, height),
    };
    let mut rgb =
        vec![0; usize::try_from(out_width.checked_mul(out_height)?.checked_mul(3)?).ok()?];
    for y in 0..height {
        for x in 0..width {
            let (target_x, target_y) = match rotation {
                VideoRotation::VideoRotation0 => (x, y),
                VideoRotation::VideoRotation90 => (height - 1 - y, x),
                VideoRotation::VideoRotation180 => (width - 1 - x, height - 1 - y),
                VideoRotation::VideoRotation270 => (y, width - 1 - x),
            };
            let source = usize::try_from((y * width + x) * 4).ok()?;
            let target = usize::try_from((target_y * out_width + target_x) * 3).ok()?;
            rgb[target..target + 3].copy_from_slice(&rgba[source..source + 3]);
        }
    }
    cameras::conversion::rgb_to_jpeg(out_width, out_height, &rgb).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jpeg_pixels_follow_video_rotation() -> Result<(), Box<dyn std::error::Error>> {
        let mut buffer = livekit::webrtc::video_frame::I420Buffer::new(32, 16);
        let (y, u, v) = buffer.data_mut();
        for row in y.chunks_mut(32) {
            row[..16].fill(32);
            row[16..].fill(220);
        }
        u.fill(128);
        v.fill(128);
        for (rotation, dimensions, dark, bright) in [
            (VideoRotation::VideoRotation0, (32, 16), (4, 8), (28, 8)),
            (VideoRotation::VideoRotation90, (16, 32), (8, 4), (8, 28)),
            (VideoRotation::VideoRotation180, (32, 16), (28, 8), (4, 8)),
            (VideoRotation::VideoRotation270, (16, 32), (8, 28), (8, 4)),
        ] {
            let frame = camera_frame(&buffer, rotation, 123, Instant::now())?;
            assert_eq!((frame.width, frame.height), dimensions);
            assert_eq!(frame.captured_at_unix_ns, 123);
            let image = image::load_from_memory(&frame.jpeg)?.to_luma8();
            assert_eq!(image.dimensions(), dimensions);
            assert!(image.get_pixel(dark.0, dark.1)[0] < 50);
            assert!(image.get_pixel(bright.0, bright.1)[0] > 200);
        }
        Ok(())
    }
}
