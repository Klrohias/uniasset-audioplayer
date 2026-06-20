//! Error types for the uniasset audio player.

use std::fmt;

/// Errors that can occur during audio device operations.
#[derive(Debug)]
pub enum AudioError {
    /// No suitable audio output device was found.
    DeviceNotFound,
    /// The device is already in use or locked by another process.
    DeviceBusy,
    /// The requested audio format (sample rate, channels) is not supported.
    FormatNotSupported,
    /// An error occurred in the audio stream layer.
    StreamError(String),
    /// A platform-specific backend error occurred.
    BackendError(String),
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioError::DeviceNotFound => write!(f, "no audio output device found"),
            AudioError::DeviceBusy => write!(f, "audio device is busy"),
            AudioError::FormatNotSupported => write!(f, "audio format not supported"),
            AudioError::StreamError(msg) => write!(f, "stream error: {msg}"),
            AudioError::BackendError(msg) => write!(f, "backend error: {msg}"),
        }
    }
}

impl std::error::Error for AudioError {}

impl From<String> for AudioError {
    fn from(s: String) -> Self {
        AudioError::BackendError(s)
    }
}

impl From<&str> for AudioError {
    fn from(s: &str) -> Self {
        AudioError::BackendError(s.to_string())
    }
}
