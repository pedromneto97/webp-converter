//! Thin safe wrappers over the libwebp FFI.
//!
//! Every libwebp-owned resource lives in a small RAII type, so the `?` early
//! returns in the converter cannot leak it.

use std::marker::PhantomData;

use libwebp_sys::{
  WebPAnimEncoder as WebPAnimEncoderHandle, WebPAnimEncoderDelete, WebPAnimEncoderNewInternal,
  WebPAnimEncoderOptions, WebPData, WebPDataClear, WebPDemuxDelete, WebPDemuxGetI,
  WebPDemuxInternal, WebPDemuxer, WebPEncodeRGB, WebPEncodeRGBA, WebPFormatFeature, WebPFree,
  WebPGetDemuxABIVersion, WebPGetMuxABIVersion, WebPPicture, WebPPictureFree,
  WebPPictureImportRGBA, WEBP_MAX_DIMENSION,
};

use crate::error::{ConverterError, ConverterResult};

pub enum ImageType {
  Rgb,
  Rgba,
}

/// A tightly packed RGB8 or RGBA8 pixel buffer.
pub struct ImageData {
  pub data: Vec<u8>,
  pub width: u32,
  pub height: u32,
  pub image_type: ImageType,
}

/// Whether libwebp can build a canvas this size.
///
/// `WebPAnimEncoderNewInternal` only rejects on total *area*, not on the
/// per-axis limit, so a 20000x100 GIF would sail past it and fail much later
/// with an opaque error.
pub fn canvas_is_encodable(width: u32, height: u32) -> bool {
  width > 0 && height > 0 && width <= WEBP_MAX_DIMENSION && height <= WEBP_MAX_DIMENSION
}

pub fn ensure_encodable(width: u32, height: u32) -> ConverterResult<()> {
  if canvas_is_encodable(width, height) {
    Ok(())
  } else {
    Err(ConverterError::DimensionsTooLarge { width, height })
  }
}

/// Owns an output buffer allocated by `WebPEncode*`.
struct WebPBuffer {
  ptr: *mut u8,
}

impl Drop for WebPBuffer {
  fn drop(&mut self) {
    if !self.ptr.is_null() {
      unsafe {
        WebPFree(self.ptr.cast());
      }
    }
  }
}

/// Encodes a still image at `quality` with the simple lossy API.
pub fn encode_still(image: &ImageData, quality: f32) -> ConverterResult<Vec<u8>> {
  ensure_encodable(image.width, image.height)?;

  let channels: usize = match image.image_type {
    ImageType::Rgb => 3,
    ImageType::Rgba => 4,
  };
  // libwebp reads `height` rows of `stride` bytes straight off the pointer.
  if image.data.len() < image.width as usize * image.height as usize * channels {
    return Err(ConverterError::EncodingFailed);
  }

  let stride = image.width as i32 * channels as i32;
  let mut out_buf = std::ptr::null_mut();

  let len = unsafe {
    match image.image_type {
      ImageType::Rgb => WebPEncodeRGB(
        image.data.as_ptr(),
        image.width as i32,
        image.height as i32,
        stride,
        quality,
        &mut out_buf,
      ),
      ImageType::Rgba => WebPEncodeRGBA(
        image.data.as_ptr(),
        image.width as i32,
        image.height as i32,
        stride,
        quality,
        &mut out_buf,
      ),
    }
  };

  let buffer = WebPBuffer { ptr: out_buf };
  if len == 0 || buffer.ptr.is_null() {
    return Err(ConverterError::EncodingFailed);
  }

  Ok(unsafe { std::slice::from_raw_parts(buffer.ptr, len) }.to_vec())
}

/// Owns a `WebPAnimEncoder`.
pub struct AnimEncoder {
  pub ptr: *mut WebPAnimEncoderHandle,
}

impl AnimEncoder {
  pub fn new(width: u32, height: u32, options: &WebPAnimEncoderOptions) -> ConverterResult<Self> {
    ensure_encodable(width, height)?;

    let ptr = unsafe {
      WebPAnimEncoderNewInternal(width as i32, height as i32, options, WebPGetMuxABIVersion())
    };

    if ptr.is_null() {
      return Err(ConverterError::AnimationEncodingFailed);
    }

    Ok(Self { ptr })
  }
}

impl Drop for AnimEncoder {
  fn drop(&mut self) {
    unsafe {
      WebPAnimEncoderDelete(self.ptr);
    }
  }
}

/// Owns the pixel memory `WebPPictureImportRGBA` allocates.
pub struct Picture {
  pub inner: WebPPicture,
}

impl Picture {
  /// Builds an ARGB picture from a tightly packed RGBA8 buffer.
  ///
  /// `width`/`height` describe `rgba` itself, never the canvas it will be
  /// drawn onto: libwebp reads `height` rows of `width * 4` bytes straight
  /// off the pointer, so a buffer smaller than that is an out-of-bounds
  /// read. The dimension check also keeps the stride inside `i32`.
  pub fn from_rgba(rgba: &[u8], width: u32, height: u32) -> ConverterResult<Self> {
    ensure_encodable(width, height)?;

    // Both axes are inside libwebp's limit by now, so this cannot overflow.
    if rgba.len() < width as usize * height as usize * 4 {
      return Err(ConverterError::AnimationEncodingFailed);
    }

    let mut inner = WebPPicture::new().map_err(|_| ConverterError::AnimationEncodingFailed)?;
    inner.use_argb = 1;
    inner.width = width as i32;
    inner.height = height as i32;

    // Wrap before importing: the import allocates internally and can still
    // fail afterwards, so Drop has to be on the hook by then.
    let mut picture = Self { inner };
    let imported =
      unsafe { WebPPictureImportRGBA(&mut picture.inner, rgba.as_ptr(), width as i32 * 4) };

    if imported == 0 {
      return Err(ConverterError::AnimationEncodingFailed);
    }

    Ok(picture)
  }
}

impl Drop for Picture {
  fn drop(&mut self) {
    unsafe {
      WebPPictureFree(&mut self.inner);
    }
  }
}

/// Owns the bitstream `WebPAnimEncoderAssemble` hands back.
#[derive(Default)]
pub struct AnimData {
  pub inner: WebPData,
}

impl AnimData {
  pub fn to_vec(&self) -> Vec<u8> {
    if self.inner.bytes.is_null() || self.inner.size == 0 {
      return Vec::new();
    }

    unsafe { std::slice::from_raw_parts(self.inner.bytes, self.inner.size) }.to_vec()
  }
}

impl Drop for AnimData {
  fn drop(&mut self) {
    unsafe {
      WebPDataClear(&mut self.inner);
    }
  }
}

/// Reads an assembled WebP bitstream back to find out what it really contains.
///
/// Borrows the bytes rather than copying them — libwebp parses in place and
/// keeps pointing at the caller's buffer — so the lifetime is load-bearing.
pub struct Demuxer<'a> {
  ptr: *mut WebPDemuxer,
  _data: PhantomData<&'a WebPData>,
}

impl<'a> Demuxer<'a> {
  pub fn new(data: &'a WebPData) -> ConverterResult<Self> {
    // No partial parsing and no state to read back: the whole bitstream is
    // already in memory, so anything short of a clean parse means the
    // encoder handed us something broken.
    let ptr = unsafe { WebPDemuxInternal(data, 0, std::ptr::null_mut(), WebPGetDemuxABIVersion()) };

    if ptr.is_null() {
      return Err(ConverterError::AnimationEncodingFailed);
    }

    Ok(Self {
      ptr,
      _data: PhantomData,
    })
  }

  /// Frames the bitstream actually carries. A still WebP reports 1.
  pub fn frame_count(&self) -> u32 {
    unsafe { WebPDemuxGetI(self.ptr, WebPFormatFeature::WEBP_FF_FRAME_COUNT) }
  }
}

impl Drop for Demuxer<'_> {
  fn drop(&mut self) {
    unsafe {
      WebPDemuxDelete(self.ptr);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::{canvas_is_encodable, encode_still, ImageData, ImageType, Picture};
  use crate::error::ConverterError;

  #[test]
  fn encodes_rgb_images() {
    let image = ImageData {
      data: vec![255, 0, 0],
      width: 1,
      height: 1,
      image_type: ImageType::Rgb,
    };

    assert!(encode_still(&image, 80.0).is_ok());
  }

  #[test]
  fn encodes_rgba_images() {
    let image = ImageData {
      data: vec![255, 0, 0, 128],
      width: 1,
      height: 1,
      image_type: ImageType::Rgba,
    };

    assert!(encode_still(&image, 80.0).is_ok());
  }

  #[test]
  fn rejects_a_still_buffer_smaller_than_its_dimensions() {
    let image = ImageData {
      data: vec![0; 3],
      width: 2,
      height: 2,
      image_type: ImageType::Rgb,
    };

    assert!(matches!(
      encode_still(&image, 80.0),
      Err(ConverterError::EncodingFailed)
    ));
  }

  #[test]
  fn rejects_canvases_beyond_the_webp_limit() {
    assert!(canvas_is_encodable(16383, 16383));
    assert!(!canvas_is_encodable(16384, 100));
    // Area stays tiny, so only the per-axis check catches this one.
    assert!(!canvas_is_encodable(20000, 100));
    assert!(!canvas_is_encodable(0, 100));
  }

  #[test]
  fn rejects_a_buffer_smaller_than_the_dimensions_it_claims() {
    let rgba = vec![0u8; 4 * 4 * 4];

    assert!(Picture::from_rgba(&rgba, 4, 4).is_ok());
    // libwebp would read past the end of the allocation for these.
    assert!(Picture::from_rgba(&rgba, 8, 8).is_err());
    assert!(Picture::from_rgba(&rgba, 20000, 4).is_err());
  }
}
