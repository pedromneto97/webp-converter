use std::io::Cursor;

use image::{
  codecs::gif::GifDecoder, metadata::LoopCount, AnimationDecoder, Delay, DynamicImage,
  ImageDecoder, ImageFormat, ImageReader,
};
use libwebp_sys::{
  WebPAnimEncoderAdd, WebPAnimEncoderAssemble, WebPAnimEncoderOptions,
  WebPAnimEncoderOptionsInitInternal, WebPConfig, WebPGetMuxABIVersion,
};

use crate::{
  error::{ConverterError, ConverterResult},
  webp::{
    encode_still, ensure_encodable, AnimData, AnimEncoder, Demuxer, ImageData, ImageType, Picture,
  },
};

/// Minimum quality the sweep starts from when the caller does not pick one.
pub const DEFAULT_MIN_QUALITY: u8 = 80;

/// Quality step used when sweeping a still image.
const STILL_QUALITY_STEP: u8 = 2;

/// Upper bound on how many full-animation encodes a GIF conversion performs.
/// Every candidate re-encodes *every* frame, so this is what keeps a 200-frame
/// GIF from turning into thousands of frame encodes.
const MAX_ANIMATION_CANDIDATES: u8 = 4;

/// A GIF frame with no delay would collapse into a zero-duration WebP frame,
/// so give it the same floor browsers apply.
const MIN_FRAME_DURATION_MS: i32 = 10;

/// WebP stores the loop count in 16 bits (libwebp rejects anything larger).
const MAX_LOOP_COUNT: u32 = u16::MAX as u32;

/// How the output quality is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
  /// Try qualities from `min` to 100 and keep the best size/quality ratio.
  Sweep { min: u8 },
  /// Encode once at exactly this quality.
  Fixed(u8),
}

impl Default for Quality {
  fn default() -> Self {
    Self::Sweep {
      min: DEFAULT_MIN_QUALITY,
    }
  }
}

impl Quality {
  /// Quality levels tried for a still image.
  fn still_candidates(self) -> Vec<u8> {
    match self {
      Self::Sweep { min } => still_candidates(min),
      Self::Fixed(quality) => vec![quality.min(100)],
    }
  }

  /// Quality levels tried for an animation.
  fn animation_candidates(self) -> Vec<u8> {
    match self {
      Self::Sweep { min } => animation_candidates(min),
      Self::Fixed(quality) => vec![quality.min(100)],
    }
  }
}

/// Result of a successful conversion.
#[derive(Debug)]
pub struct Converted {
  /// The WebP bitstream.
  pub data: Vec<u8>,
  /// The quality level the encoder used (0-100).
  pub quality: u8,
  /// Number of frames written to the output. 1 for a still image.
  pub frame_count: u32,
}

/// Converts an encoded image (any format the `image` crate reads) to WebP.
///
/// Animated GIFs become animated WebP, keeping frame delays and loop count.
/// Every other input becomes a still WebP.
pub fn convert(input: &[u8], quality: Quality) -> ConverterResult<Converted> {
  // Sniff the magic bytes rather than trusting any filename: a GIF named .jpg
  // should still animate.
  match detect_format(input)? {
    ImageFormat::Gif => convert_animation(input, quality),
    _ => convert_still(input, quality),
  }
}

fn reader(input: &[u8]) -> ConverterResult<ImageReader<Cursor<&[u8]>>> {
  ImageReader::new(Cursor::new(input))
    .with_guessed_format()
    .map_err(|error| ConverterError::FailedToDecodeImage(error.to_string()))
}

fn detect_format(input: &[u8]) -> ConverterResult<ImageFormat> {
  reader(input)?
    .format()
    .ok_or(ConverterError::UnsupportedFormat)
}

/// Decodes a still image into the RGB8/RGBA8 layout libwebp accepts.
///
/// Other color types (grayscale, 16-bit, float) are converted rather than
/// rejected; anything carrying alpha keeps it.
fn decode_still(input: &[u8]) -> ConverterResult<ImageData> {
  let image = reader(input)?
    .decode()
    .map_err(|error| ConverterError::FailedToDecodeImage(error.to_string()))?;

  let (width, height) = (image.width(), image.height());
  ensure_encodable(width, height)?;

  let (data, image_type) = match image {
    DynamicImage::ImageRgb8(buffer) => (buffer.into_raw(), ImageType::Rgb),
    DynamicImage::ImageRgba8(buffer) => (buffer.into_raw(), ImageType::Rgba),
    other if other.color().has_alpha() => (other.to_rgba8().into_raw(), ImageType::Rgba),
    other => (other.to_rgb8().into_raw(), ImageType::Rgb),
  };

  Ok(ImageData {
    data,
    width,
    height,
    image_type,
  })
}

fn open_gif(input: &[u8]) -> ConverterResult<GifDecoder<Cursor<&[u8]>>> {
  GifDecoder::new(Cursor::new(input))
    .map_err(|error| ConverterError::FailedToDecodeAnimation(error.to_string()))
}

/// How long a frame stays on screen, in milliseconds.
///
/// GIF delays are whole centiseconds, so the millisecond conversion is exact.
fn frame_duration_ms(delay: Delay) -> i32 {
  let (numerator, denominator) = delay.numer_denom_ms();
  let duration = numerator
    .checked_div(denominator)
    .unwrap_or(0)
    .min(i32::MAX as u32) as i32;

  duration.max(MIN_FRAME_DURATION_MS)
}

struct EncodedAnimation {
  bytes: Vec<u8>,
  /// Frames in the assembled bitstream. `WebPAnimEncoder` folds a frame into
  /// its predecessor whenever the pixels repeat, and writes a lone survivor
  /// as a plain still WebP, so this can come in under `decoded_frames`.
  frame_count: u32,
  /// Frames read out of the GIF. Unlike `frame_count` this does not move with
  /// the quality candidate, so it is what decides whether the *input* is an
  /// animation at all.
  decoded_frames: u32,
}

/// Encodes the whole GIF in `input` as one animated WebP at `quality`.
///
/// Re-decodes the GIF on every call rather than caching frames. Holding them
/// would cost `width * height * 4 * frames` — 1.6 GB for a 1080p 200-frame GIF
/// — while a GIF decode is cheap next to the WebP encode it feeds.
fn encode_animation(input: &[u8], quality: f32) -> ConverterResult<EncodedAnimation> {
  let decoder = open_gif(input)?;
  let (width, height) = decoder.dimensions();
  ensure_encodable(width, height)?;

  let loop_count = match decoder.loop_count() {
    LoopCount::Infinite => 0,
    LoopCount::Finite(times) => times.get().min(MAX_LOOP_COUNT) as i32,
  };

  let mut config = WebPConfig::new().map_err(|_| ConverterError::AnimationEncodingFailed)?;
  config.quality = quality;

  // Every field is a plain integer, so an all-zero value is a valid
  // `WebPAnimEncoderOptions`; the init call then overwrites all of them.
  let mut options: WebPAnimEncoderOptions = unsafe { std::mem::zeroed() };
  if unsafe { WebPAnimEncoderOptionsInitInternal(&mut options, WebPGetMuxABIVersion()) } == 0 {
    return Err(ConverterError::AnimationEncodingFailed);
  }
  options.anim_params.loop_count = loop_count;
  // GIF frames are palette-based and often compress smaller losslessly, so
  // let libwebp choose per frame. `minimize_size` is deliberately left off:
  // it makes the encoder try both dispose methods per frame, which multiplies
  // across every candidate in the sweep for a marginal size win.
  options.allow_mixed = 1;

  let encoder = AnimEncoder::new(width, height, &options)?;

  let mut timestamp_ms: i32 = 0;
  let mut decoded_frames: u32 = 0;

  for frame in decoder.into_frames() {
    let frame =
      frame.map_err(|error| ConverterError::FailedToDecodeAnimation(error.to_string()))?;
    let duration_ms = frame_duration_ms(frame.delay());

    // The GIF decoder always yields a full canvas at (0, 0), already
    // composited for disposal and blending, so the buffer maps 1:1 onto
    // the encoder canvas. Import it by its own dimensions all the same —
    // the canvas size comes from a different decoder instance, and the
    // import reads straight off the pointer.
    let buffer = frame.into_buffer();
    let (frame_width, frame_height) = buffer.dimensions();
    let mut picture = Picture::from_rgba(buffer.as_raw(), frame_width, frame_height)?;

    // `timestamp_ms` is the frame's *start* time; libwebp derives each
    // duration from the gap to the next one.
    let added =
      unsafe { WebPAnimEncoderAdd(encoder.ptr, &mut picture.inner, timestamp_ms, &config) };
    if added == 0 {
      return Err(ConverterError::AnimationEncodingFailed);
    }

    timestamp_ms = timestamp_ms.saturating_add(duration_ms);
    decoded_frames += 1;
  }

  if decoded_frames == 0 {
    return Err(ConverterError::FailedToDecodeAnimation(
      "GIF has no frames".to_string(),
    ));
  }

  // Required terminator. Without it `WebPAnimEncoderAssemble` guesses the
  // last frame's duration as the average of the earlier ones.
  let terminated = unsafe {
    WebPAnimEncoderAdd(
      encoder.ptr,
      std::ptr::null_mut(),
      timestamp_ms,
      std::ptr::null(),
    )
  };
  if terminated == 0 {
    return Err(ConverterError::AnimationEncodingFailed);
  }

  let mut assembled = AnimData::default();
  if unsafe { WebPAnimEncoderAssemble(encoder.ptr, &mut assembled.inner) } == 0 {
    return Err(ConverterError::AnimationEncodingFailed);
  }

  let bytes = assembled.to_vec();
  if bytes.is_empty() {
    return Err(ConverterError::AnimationEncodingFailed);
  }

  // Count what came out, not what went in: the encoder merges frames it finds
  // redundant, so every frame added is an upper bound, not the answer.
  let frame_count = Demuxer::new(&assembled.inner)?.frame_count();
  if frame_count == 0 {
    return Err(ConverterError::AnimationEncodingFailed);
  }

  Ok(EncodedAnimation {
    bytes,
    frame_count,
    decoded_frames,
  })
}

/// Bytes per quality point; lower is better.
///
/// The divisor floors at 1 so quality 0 scores finitely — `bytes / 0.0` is
/// `+inf`, which made quality 0 unselectable.
fn score(size: usize, quality: u8) -> f32 {
  size as f32 / f32::from(quality.max(1))
}

/// Quality levels tried for a still image: the full 2-point sweep.
fn still_candidates(min_quality: u8) -> Vec<u8> {
  // Note: `step_by(2)` skips 100 when `100 - min_quality` is odd. Kept so the
  // output matches the reference implementation.
  (min_quality.min(100)..=100)
    .step_by(STILL_QUALITY_STEP as usize)
    .collect()
}

/// Quality levels tried for an animation: at most [`MAX_ANIMATION_CANDIDATES`],
/// spread from `min_quality` to 100 no matter how many frames there are.
fn animation_candidates(min_quality: u8) -> Vec<u8> {
  let min = min_quality.min(100);
  let step = ((100 - min) / (MAX_ANIMATION_CANDIDATES - 1)).max(1);

  let mut candidates: Vec<u8> = (min..100)
    .step_by(step as usize)
    .take(MAX_ANIMATION_CANDIDATES as usize - 1)
    .collect();
  candidates.push(100);

  candidates
}

fn convert_still(input: &[u8], quality: Quality) -> ConverterResult<Converted> {
  let image_data = decode_still(input)?;

  // Lower score = better.
  let mut best: Option<(u8, Vec<u8>)> = None;
  let mut best_score = f32::MAX;

  for candidate in quality.still_candidates() {
    let encoded = encode_still(&image_data, f32::from(candidate))?;
    let candidate_score = score(encoded.len(), candidate);

    if best.is_none() || candidate_score < best_score {
      best_score = candidate_score;
      best = Some((candidate, encoded));
    }
  }

  let (quality, data) = best.ok_or(ConverterError::EncodingFailed)?;

  Ok(Converted {
    data,
    quality,
    frame_count: 1,
  })
}

fn convert_animation(input: &[u8], quality: Quality) -> ConverterResult<Converted> {
  // The frame count travels with the bytes: reporting the last candidate's
  // count next to a different candidate's file would be wrong the moment a
  // decode ever stops early on one pass.
  let mut best: Option<(u8, EncodedAnimation)> = None;
  let mut best_score = f32::MAX;

  for candidate in quality.animation_candidates() {
    let encoded = encode_animation(input, f32::from(candidate))?;

    // A one-frame GIF is a still image; the still encoder gives it a smaller,
    // non-animated container. This asks the *input* frame count, not the
    // encoded one: how many frames survive encoding varies with quality, so
    // keying on that would abandon the sweep on some candidates and not
    // others. A multi-frame GIF the encoder flattens to one frame is already
    // a still WebP by then, and `frame_count` reports it as such.
    if encoded.decoded_frames <= 1 {
      return convert_still(input, quality);
    }

    let candidate_score = score(encoded.bytes.len(), candidate);
    if best.is_none() || candidate_score < best_score {
      best_score = candidate_score;
      best = Some((candidate, encoded));
    }
  }

  let (
    quality,
    EncodedAnimation {
      bytes, frame_count, ..
    },
  ) = best.ok_or(ConverterError::AnimationEncodingFailed)?;

  Ok(Converted {
    data: bytes,
    quality,
    frame_count,
  })
}

#[cfg(test)]
mod tests {
  use std::io::Cursor;

  use image::{
    codecs::gif::{GifEncoder, Repeat},
    Delay, DynamicImage, Frame, GrayAlphaImage, GrayImage, ImageBuffer, ImageFormat, Rgb,
    RgbaImage,
  };

  use super::{
    animation_candidates, convert, score, still_candidates, Converted, Quality,
    MAX_ANIMATION_CANDIDATES,
  };
  use crate::error::ConverterError;

  const MAX_CANDIDATES: usize = MAX_ANIMATION_CANDIDATES as usize;
  const FRAME_DELAY_MS: u32 = 100;

  fn gif(frame_count: u32, repeat: Repeat) -> Vec<u8> {
    // Distinct colours per frame so libwebp cannot merge them away.
    let shades = (0..frame_count).map(|index| (index * 60 % 200 + 20) as u8);

    gif_frames(shades, repeat)
  }

  /// Encodes one frame per shade, so a repeated shade is a repeated frame.
  fn gif_frames(shades: impl IntoIterator<Item = u8>, repeat: Repeat) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
      let mut encoder = GifEncoder::new(&mut bytes);
      encoder.set_repeat(repeat).expect("set repeat");

      for shade in shades {
        let buffer = RgbaImage::from_fn(16, 16, |x, y| {
          image::Rgba([shade, x as u8 * 12, y as u8 * 12, 255])
        });

        encoder
          .encode_frame(Frame::from_parts(
            buffer,
            0,
            0,
            Delay::from_numer_denom_ms(FRAME_DELAY_MS, 1),
          ))
          .expect("encode gif frame");
      }
    }

    bytes
  }

  fn png(image: DynamicImage) -> Vec<u8> {
    let mut bytes = Vec::new();
    image
      .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
      .expect("encode png");

    bytes
  }

  fn sweep(input: &[u8]) -> Converted {
    convert(input, Quality::Sweep { min: 80 }).expect("conversion should succeed")
  }

  /// Walks the top-level RIFF chunks of a WebP file.
  fn riff_chunks(webp: &[u8]) -> Vec<(&[u8], &[u8])> {
    let mut chunks = Vec::new();
    let mut cursor = 12; // 'RIFF' + u32 size + 'WEBP'

    while cursor + 8 <= webp.len() {
      let id = &webp[cursor..cursor + 4];
      let size_bytes: [u8; 4] = webp[cursor + 4..cursor + 8]
        .try_into()
        .expect("four size bytes");
      let size = u32::from_le_bytes(size_bytes) as usize;
      let Some(body) = webp.get(cursor + 8..cursor + 8 + size) else {
        break;
      };

      chunks.push((id, body));
      // Chunk payloads are padded to an even length.
      cursor += 8 + size + (size & 1);
    }

    chunks
  }

  fn chunks_named<'a>(chunks: &[(&'a [u8], &'a [u8])], id: &[u8; 4]) -> Vec<&'a [u8]> {
    chunks
      .iter()
      .filter(|(chunk_id, _)| chunk_id == id)
      .map(|(_, body)| *body)
      .collect()
  }

  fn assert_is_webp(bytes: &[u8]) {
    assert!(bytes.len() > 12, "output is too short to be a WebP");
    assert_eq!(&bytes[0..4], b"RIFF", "missing RIFF header");
    assert_eq!(&bytes[8..12], b"WEBP", "missing WEBP fourcc");
  }

  #[test]
  fn quality_zero_scores_finitely() {
    assert!(score(1_000, 0).is_finite());
    assert_eq!(score(1_000, 0), score(1_000, 1));
  }

  #[test]
  fn still_candidates_keep_the_two_point_sweep() {
    let candidates = still_candidates(80);

    assert_eq!(candidates.len(), 11);
    assert_eq!(candidates.first(), Some(&80));
    assert_eq!(candidates.last(), Some(&100));
    // A minimum above the valid range used to produce an empty sweep.
    assert_eq!(still_candidates(200), vec![100]);
  }

  #[test]
  fn animation_candidates_are_capped() {
    for min_quality in 0..=255u8 {
      let candidates = animation_candidates(min_quality);

      assert!(
        !candidates.is_empty(),
        "min quality {min_quality} produced an empty sweep"
      );
      assert!(
        candidates.len() <= MAX_CANDIDATES,
        "expected at most {MAX_CANDIDATES} candidates for min quality {min_quality}, got {candidates:?}"
      );
      assert_eq!(
        candidates.last(),
        Some(&100),
        "quality 100 should always be tried"
      );
      assert!(
        candidates
          .iter()
          .all(|&quality| quality >= min_quality.min(100)),
        "candidates must not drop below the requested minimum"
      );
    }

    assert_eq!(animation_candidates(80), vec![80, 86, 92, 100]);
    assert_eq!(animation_candidates(0), vec![0, 33, 66, 100]);
    assert_eq!(animation_candidates(100), vec![100]);
  }

  #[test]
  fn fixed_quality_is_a_single_candidate() {
    assert_eq!(Quality::Fixed(42).still_candidates(), vec![42]);
    assert_eq!(Quality::Fixed(42).animation_candidates(), vec![42]);
    assert_eq!(Quality::Fixed(200).still_candidates(), vec![100]);
  }

  #[test]
  fn converts_animated_gif_to_animated_webp() {
    let converted = sweep(&gif(3, Repeat::Infinite));

    assert_eq!(converted.frame_count, 3);
    assert_is_webp(&converted.data);

    let chunks = riff_chunks(&converted.data);
    assert_eq!(chunks_named(&chunks, b"VP8X").len(), 1, "missing VP8X");
    assert_eq!(chunks_named(&chunks, b"ANIM").len(), 1, "missing ANIM");
    assert_eq!(
      chunks_named(&chunks, b"ANMF").len(),
      3,
      "expected one ANMF per frame"
    );
  }

  #[test]
  fn preserves_frame_delays() {
    let converted = sweep(&gif(3, Repeat::Infinite));
    let chunks = riff_chunks(&converted.data);

    // ANMF payload: x/64, y/64, width-1, height-1 (24 bits each), then the
    // frame duration in ms as a 24-bit little-endian value.
    for body in chunks_named(&chunks, b"ANMF") {
      let duration = u32::from_le_bytes([body[12], body[13], body[14], 0]);

      assert_eq!(
        duration, FRAME_DELAY_MS,
        "frame duration should survive the round trip"
      );
    }
  }

  #[test]
  fn preserves_loop_count() {
    for (repeat, expected) in [(Repeat::Infinite, 0), (Repeat::Finite(3), 3)] {
      let converted = sweep(&gif(3, repeat));
      let chunks = riff_chunks(&converted.data);
      let anim = chunks_named(&chunks, b"ANIM");
      let body = anim.first().expect("missing ANIM chunk");

      // ANIM payload: 4-byte BGRA background, then a 16-bit loop count.
      let loop_count = u16::from_le_bytes([body[4], body[5]]);

      assert_eq!(
        loop_count, expected,
        "loop count should survive for {repeat:?}"
      );
    }
  }

  #[test]
  fn single_frame_gif_falls_back_to_the_still_path() {
    let converted = sweep(&gif(1, Repeat::Infinite));

    assert_eq!(converted.frame_count, 1);
    assert_is_webp(&converted.data);

    let chunks = riff_chunks(&converted.data);
    assert!(
      chunks_named(&chunks, b"ANIM").is_empty(),
      "a single frame should not produce an animation"
    );
  }

  #[test]
  fn reports_the_frame_count_the_encoder_actually_wrote() {
    // Five frames, three distinct: libwebp folds each repeat into the frame
    // before it and stretches that frame's duration instead.
    let converted = sweep(&gif_frames([20u8, 20, 90, 90, 160], Repeat::Infinite));
    let chunks = riff_chunks(&converted.data);

    assert_eq!(
      chunks_named(&chunks, b"ANMF").len(),
      3,
      "expected the repeated frames to be merged away"
    );
    assert_eq!(
      converted.frame_count, 3,
      "frame count should describe the encoded file, not the decoded input"
    );
  }

  #[test]
  fn frames_merged_down_to_one_are_not_reported_as_animated() {
    let converted = sweep(&gif_frames([40u8; 5], Repeat::Infinite));
    assert_is_webp(&converted.data);

    // Nothing moves, so the encoder drops the animation container entirely.
    let chunks = riff_chunks(&converted.data);
    assert!(
      chunks_named(&chunks, b"ANIM").is_empty(),
      "a fully merged animation should not stay animated"
    );
    assert_eq!(converted.frame_count, 1);
  }

  #[test]
  fn fixed_quality_is_reported_back() {
    let still = png(DynamicImage::ImageRgb8(ImageBuffer::from_fn(
      8,
      8,
      |x, y| Rgb([x as u8 * 30, y as u8 * 30, 128]),
    )));

    assert_eq!(
      convert(&still, Quality::Fixed(42)).expect("still").quality,
      42
    );
    assert_eq!(
      convert(&gif(3, Repeat::Infinite), Quality::Fixed(42))
        .expect("animation")
        .quality,
      42
    );
  }

  #[test]
  fn converts_color_types_libwebp_does_not_take_directly() {
    let gray =
      DynamicImage::ImageLuma8(GrayImage::from_fn(8, 8, |x, _| image::Luma([x as u8 * 30])));
    let gray_alpha = DynamicImage::ImageLumaA8(GrayAlphaImage::from_fn(8, 8, |x, y| {
      image::LumaA([x as u8 * 30, y as u8 * 30])
    }));
    let rgb16 = DynamicImage::ImageRgb16(ImageBuffer::from_fn(8, 8, |x, y| {
      Rgb([x as u16 * 8000, y as u16 * 8000, 30000])
    }));

    for image in [gray, gray_alpha, rgb16] {
      let color = image.color();
      let converted = convert(&png(image), Quality::default())
        .unwrap_or_else(|error| panic!("{color:?}: {error}"));

      assert_is_webp(&converted.data);
      assert_eq!(converted.frame_count, 1);
    }
  }

  #[test]
  fn rejects_unrecognized_input() {
    let result = convert(b"definitely not an image", Quality::default());

    assert!(matches!(result, Err(ConverterError::UnsupportedFormat)));
  }

  #[test]
  fn rejects_images_wider_than_the_webp_limit() {
    let wide = png(DynamicImage::ImageRgb8(ImageBuffer::new(20_000, 1)));

    let result = convert(&wide, Quality::default());

    assert!(matches!(
      result,
      Err(ConverterError::DimensionsTooLarge {
        width: 20_000,
        height: 1
      })
    ));
  }
}
