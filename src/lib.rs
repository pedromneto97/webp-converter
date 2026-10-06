#![deny(clippy::all)]

mod converter;
mod error;
mod webp;

use std::{fs, io::ErrorKind};

use napi::{
  bindgen_prelude::{AsyncTask, Buffer},
  Env, Result, Task,
};
use napi_derive::napi;

use crate::{
  converter::{Converted, Quality, DEFAULT_MIN_QUALITY},
  error::ConverterError,
};

/// Options for `convert` and `convertFile`. Every field is optional.
#[napi(object)]
#[derive(Default)]
pub struct ConvertOptions {
  /// Lowest quality the sweep tries, an integer from 0 to 100. Defaults to 80.
  /// Ignored when `quality` is set.
  pub min_quality: Option<f64>,
  /// Encode once at exactly this quality, an integer from 0 to 100, instead of
  /// sweeping for the best size/quality ratio.
  pub quality: Option<f64>,
}

/// Result of `convert`.
#[napi(object)]
pub struct ConvertResult {
  /// The WebP file contents.
  pub data: Buffer,
  /// The quality the output was encoded at (0-100).
  pub quality: u32,
  /// Number of frames in the output. 1 for a still image.
  pub frame_count: u32,
}

/// Result of `convertFile`. The WebP itself is written to `outputPath`.
#[napi(object)]
pub struct ConvertFileResult {
  /// The quality the output was encoded at (0-100).
  pub quality: u32,
  /// Number of frames in the output. 1 for a still image.
  pub frame_count: u32,
}

/// Takes the JS number as-is so negative and fractional values are rejected
/// instead of being wrapped or truncated by the `u32` conversion.
fn quality_value(name: &str, value: f64) -> std::result::Result<u8, ConverterError> {
  if value.fract() == 0.0 && (0.0..=100.0).contains(&value) {
    Ok(value as u8)
  } else {
    Err(ConverterError::InvalidOption(format!(
      "`{name}` must be an integer from 0 to 100, got {value}"
    )))
  }
}

fn to_quality(options: Option<ConvertOptions>) -> std::result::Result<Quality, ConverterError> {
  let options = options.unwrap_or_default();

  if let Some(quality) = options.quality {
    return Ok(Quality::Fixed(quality_value("quality", quality)?));
  }

  let min = match options.min_quality {
    Some(min_quality) => quality_value("minQuality", min_quality)?,
    None => DEFAULT_MIN_QUALITY,
  };

  Ok(Quality::Sweep { min })
}

pub struct ConvertTask {
  input: Buffer,
  quality: Quality,
}

impl Task for ConvertTask {
  type Output = Converted;
  type JsValue = ConvertResult;

  fn compute(&mut self) -> Result<Self::Output> {
    Ok(converter::convert(&self.input, self.quality)?)
  }

  fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
    Ok(ConvertResult {
      data: output.data.into(),
      quality: u32::from(output.quality),
      frame_count: output.frame_count,
    })
  }
}

pub struct ConvertFileTask {
  input_path: String,
  output_path: String,
  quality: Quality,
}

impl Task for ConvertFileTask {
  type Output = Converted;
  type JsValue = ConvertFileResult;

  fn compute(&mut self) -> Result<Self::Output> {
    let input = fs::read(&self.input_path).map_err(|error| match error.kind() {
      ErrorKind::NotFound => ConverterError::FileNotFound(self.input_path.clone()),
      _ => ConverterError::FailedToReadInput(format!("{}: {error}", self.input_path)),
    })?;

    let converted = converter::convert(&input, self.quality)?;

    fs::write(&self.output_path, &converted.data).map_err(|error| {
      ConverterError::FailedToWriteOutput(format!("{}: {error}", self.output_path))
    })?;

    Ok(converted)
  }

  fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
    Ok(ConvertFileResult {
      quality: u32::from(output.quality),
      frame_count: output.frame_count,
    })
  }
}

/// Converts an encoded image to WebP.
///
/// The input format is detected from its magic bytes. Animated GIFs become
/// animated WebP, keeping frame delays and loop count; every other input
/// becomes a still WebP. Encoding runs on the libuv threadpool.
///
/// Throws synchronously when `options` is invalid. The promise rejects when
/// the input cannot be decoded or encoded.
#[napi(ts_return_type = "Promise<ConvertResult>")]
pub fn convert(input: Buffer, options: Option<ConvertOptions>) -> Result<AsyncTask<ConvertTask>> {
  let quality = to_quality(options)?;

  Ok(AsyncTask::new(ConvertTask { input, quality }))
}

/// Reads the image at `inputPath`, converts it to WebP and writes the result
/// to `outputPath`.
///
/// Same conversion rules as `convert`. File IO also runs on the libuv
/// threadpool. Overwrites `outputPath` if it exists; its parent directory must
/// already exist.
#[napi(ts_return_type = "Promise<ConvertFileResult>")]
pub fn convert_file(
  input_path: String,
  output_path: String,
  options: Option<ConvertOptions>,
) -> Result<AsyncTask<ConvertFileTask>> {
  let quality = to_quality(options)?;

  Ok(AsyncTask::new(ConvertFileTask {
    input_path,
    output_path,
    quality,
  }))
}

#[cfg(test)]
mod tests {
  use super::{to_quality, ConvertOptions};
  use crate::converter::Quality;

  #[test]
  fn defaults_to_the_reference_sweep() {
    assert_eq!(to_quality(None).unwrap(), Quality::Sweep { min: 80 });
  }

  #[test]
  fn fixed_quality_wins_over_min_quality() {
    let options = ConvertOptions {
      min_quality: Some(10.0),
      quality: Some(55.0),
    };

    assert_eq!(to_quality(Some(options)).unwrap(), Quality::Fixed(55));
  }

  #[test]
  fn rejects_out_of_range_options() {
    for options in [
      ConvertOptions {
        min_quality: Some(101.0),
        quality: None,
      },
      ConvertOptions {
        min_quality: None,
        quality: Some(-1.0),
      },
      ConvertOptions {
        min_quality: None,
        quality: Some(50.5),
      },
      ConvertOptions {
        min_quality: None,
        quality: Some(f64::NAN),
      },
    ] {
      assert!(to_quality(Some(options)).is_err());
    }
  }
}
