use std::fmt;

#[derive(Debug)]
pub enum ConverterError {
  InvalidOption(String),
  FileNotFound(String),
  FailedToReadInput(String),
  UnsupportedFormat,
  FailedToDecodeImage(String),
  DimensionsTooLarge { width: u32, height: u32 },
  EncodingFailed,
  FailedToWriteOutput(String),
  FailedToDecodeAnimation(String),
  AnimationEncodingFailed,
}

impl fmt::Display for ConverterError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::InvalidOption(message) => write!(f, "Invalid option: {message}"),
      Self::FileNotFound(path) => write!(f, "Input file not found: {path}"),
      Self::FailedToReadInput(message) => write!(f, "Failed to read input file: {message}"),
      Self::UnsupportedFormat => write!(f, "Unsupported or unrecognized image format"),
      Self::FailedToDecodeImage(message) => write!(f, "Failed to decode image: {message}"),
      Self::DimensionsTooLarge { width, height } => write!(
        f,
        "Image dimensions {width}x{height} are outside the WebP limit of {max}x{max}",
        max = libwebp_sys::WEBP_MAX_DIMENSION
      ),
      Self::EncodingFailed => write!(f, "WebP encoding failed"),
      Self::FailedToWriteOutput(message) => write!(f, "Failed to write output file: {message}"),
      Self::FailedToDecodeAnimation(message) => {
        write!(f, "Failed to decode GIF animation: {message}")
      }
      Self::AnimationEncodingFailed => write!(f, "Animated WebP encoding failed"),
    }
  }
}

impl std::error::Error for ConverterError {}

impl From<ConverterError> for napi::Error {
  fn from(error: ConverterError) -> Self {
    let status = match error {
      ConverterError::InvalidOption(_) => napi::Status::InvalidArg,
      _ => napi::Status::GenericFailure,
    };

    napi::Error::new(status, error.to_string())
  }
}

pub type ConverterResult<T> = Result<T, ConverterError>;
