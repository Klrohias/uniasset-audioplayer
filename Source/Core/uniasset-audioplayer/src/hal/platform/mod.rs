//! Platform detection and backend selection.
//!
//! Conditionally compiles the appropriate audio backend for the target OS.

use crate::error::AudioError;
use crate::hal::AudioDevice;
use crate::types::AudioFormat;

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod coreaudio;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use coreaudio::CoreAudioDevice;

#[cfg(target_os = "windows")]
mod wasapi;
#[cfg(target_os = "windows")]
pub use wasapi::WasapiDevice;

#[cfg(target_os = "android")]
mod oboe;
#[cfg(target_os = "android")]
pub use oboe::OboeDevice;

#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "windows",
    target_os = "android"
)))]
mod dummy;
#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "windows",
    target_os = "android"
)))]
pub use dummy::DummyDevice;

/// Create a platform-appropriate audio device.
pub(crate) fn create_device(format: AudioFormat) -> Result<Box<dyn AudioDevice>, AudioError> {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        CoreAudioDevice::new(format).map(|d| Box::new(d) as Box<dyn AudioDevice>)
    }

    #[cfg(target_os = "windows")]
    {
        WasapiDevice::new(format).map(|d| Box::new(d) as Box<dyn AudioDevice>)
    }

    #[cfg(target_os = "android")]
    {
        OboeDevice::new(format).map(|d| Box::new(d) as Box<dyn AudioDevice>)
    }

    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "windows",
        target_os = "android"
    )))]
    {
        Ok(Box::new(DummyDevice::new(format)))
    }
}
