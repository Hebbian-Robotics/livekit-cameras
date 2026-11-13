use crate::CameraError;

/// Contiguous planar BT.601 limited-range Y, U, V, with 2x2 chroma subsampling.
/// Odd edges use ceil(width / 2) and ceil(height / 2) chroma dimensions.
#[derive(Debug, PartialEq, Eq)]
pub struct I420Frame {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

/// Borrowed planar storage, with tightly packed rows and ceil-divided chroma.
pub struct I420Planes<'frame> {
    pub y: &'frame [u8],
    pub u: &'frame [u8],
    pub v: &'frame [u8],
}

impl I420Frame {
    /// Borrow the Y, U and V planes without copying.
    ///
    /// # Errors
    /// Returns `InvalidFrame` if dimensions and storage disagree.
    pub fn planes(&self) -> Result<I420Planes<'_>, CameraError> {
        let width = usize::try_from(self.width).map_err(|_| CameraError::InvalidFrame)?;
        let height = usize::try_from(self.height).map_err(|_| CameraError::InvalidFrame)?;
        let y_length = width.checked_mul(height).ok_or(CameraError::InvalidFrame)?;
        let chroma_length = width
            .div_ceil(2)
            .checked_mul(height.div_ceil(2))
            .ok_or(CameraError::InvalidFrame)?;
        let total_length = chroma_length
            .checked_mul(2)
            .and_then(|chroma| y_length.checked_add(chroma))
            .ok_or(CameraError::InvalidFrame)?;
        if y_length == 0 || self.data.len() != total_length {
            return Err(CameraError::InvalidFrame);
        }
        let (y, chroma) = self.data.split_at(y_length);
        let (u, v) = chroma.split_at(chroma_length);
        Ok(I420Planes { y, u, v })
    }
}

/// Convert packed RGB to limited-range BT.601 I420.
///
/// # Errors
/// Returns `InvalidFrame` for empty dimensions, overflow or mismatched storage.
pub fn rgb_to_i420(width: u32, height: u32, rgb: &[u8]) -> Result<I420Frame, CameraError> {
    let width_pixels = usize::try_from(width).map_err(|_| CameraError::InvalidFrame)?;
    let height_pixels = usize::try_from(height).map_err(|_| CameraError::InvalidFrame)?;
    let pixel_count = width_pixels
        .checked_mul(height_pixels)
        .ok_or(CameraError::InvalidFrame)?;
    let rgb_stride = width.checked_mul(3).ok_or(CameraError::InvalidFrame)?;
    if pixel_count == 0 || pixel_count.checked_mul(3) != Some(rgb.len()) {
        return Err(CameraError::InvalidFrame);
    }
    let chroma_count = width_pixels
        .div_ceil(2)
        .checked_mul(height_pixels.div_ceil(2))
        .ok_or(CameraError::InvalidFrame)?;
    let frame_length = chroma_count
        .checked_mul(2)
        .and_then(|chroma| pixel_count.checked_add(chroma))
        .ok_or(CameraError::InvalidFrame)?;
    let mut data = vec![0; frame_length];
    let (y_plane, chroma_planes) = data.split_at_mut(pixel_count);
    let (u_plane, v_plane) = chroma_planes.split_at_mut(chroma_count);
    let mut image = yuv::YuvPlanarImageMut {
        y_plane: yuv::BufferStoreMut::Borrowed(y_plane),
        y_stride: width,
        u_plane: yuv::BufferStoreMut::Borrowed(u_plane),
        u_stride: width.div_ceil(2),
        v_plane: yuv::BufferStoreMut::Borrowed(v_plane),
        v_stride: width.div_ceil(2),
        width,
        height,
    };
    yuv::rgb_to_yuv420(
        &mut image,
        rgb,
        rgb_stride,
        yuv::YuvRange::Limited,
        yuv::YuvStandardMatrix::Bt601,
        yuv::YuvConversionMode::Balanced,
    )
    .map_err(|_| CameraError::InvalidFrame)?;
    Ok(I420Frame {
        width,
        height,
        data,
    })
}

/// Encode packed RGB as quality-85 JPEG.
///
/// # Errors
/// Returns an image error for invalid storage or an encoding failure.
pub fn rgb_to_jpeg(width: u32, height: u32, rgb: &[u8]) -> Result<Vec<u8>, CameraError> {
    // Validate before calling the encoder, whose buffer length contract can panic.
    let expected_length = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(3));
    if width == 0 || height == 0 || u64::try_from(rgb.len()).ok() != expected_length {
        return Err(CameraError::InvalidFrame);
    }
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 85).encode(
        rgb,
        width,
        height,
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(jpeg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_colour_blocks_have_bt601_planes() -> Result<(), CameraError> {
        // Four uniform 2x2 blocks: red, green, blue, white. Plane ordering matters.
        let mut rgb = Vec::new();
        for _ in 0..2 {
            for colour in [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255]] {
                rgb.extend_from_slice(&colour);
                rgb.extend_from_slice(&colour);
            }
        }
        let converted = rgb_to_i420(8, 2, &rgb)?;
        assert_eq!(
            &converted.data[..16],
            &[
                81, 81, 145, 145, 41, 41, 235, 235, 81, 81, 145, 145, 41, 41, 235, 235
            ]
        );
        let planes = converted.planes()?;
        assert_eq!(planes.u, &[90, 54, 240, 128]);
        assert_eq!(planes.v, &[240, 34, 110, 128]);
        Ok(())
    }

    #[test]
    fn mixed_block_averages_chroma_and_odd_edges_keep_samples() -> Result<(), CameraError> {
        let rgb = [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255];
        assert_eq!(rgb_to_i420(2, 2, &rgb)?.data, [81, 145, 41, 235, 128, 128]);
        assert_eq!(rgb_to_i420(1, 1, &[0, 0, 0])?.data, [16, 128, 128]);
        assert!(matches!(
            rgb_to_i420(2, 2, &[0; 3]),
            Err(CameraError::InvalidFrame)
        ));
        assert!(matches!(
            rgb_to_jpeg(2, 2, &[0; 3]),
            Err(CameraError::InvalidFrame)
        ));
        Ok(())
    }
}
