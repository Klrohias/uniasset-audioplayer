//! Oboe backend for Android.
//!
//! Uses the Oboe (AAudio) native audio library with a data callback
//! that pulls from [`AudioCallback::pull`].

use crate::error::AudioError;
use crate::hal::AudioCallback;
use crate::hal::AudioDevice;
use crate::types::AudioFormat;

/// Default format fallback: 48 kHz stereo.
const DEFAULT_FORMAT: AudioFormat = AudioFormat::new(48000, 2);

/// An audio output device backed by Oboe (AAudio).
pub struct OboeDevice {
    format: AudioFormat,
    /// The Oboe audio stream handle (opaque pointer).
    stream: Option<*mut std::ffi::c_void>,
    running: bool,
}

// Safety: Oboe handles are safe to send between threads.
unsafe impl Send for OboeDevice {}

impl OboeDevice {
    /// Create a new Oboe output device.
    ///
    /// TODO: Query the device's native sample rate and channel count
    /// via `AudioManager::getProperty` or similar AAudio API.
    pub fn new() -> Result<Self, AudioError> {
        // TODO: Create an oboe::AudioStreamBuilder, query device capabilities,
        // set format to Float, and open the stream.
        Ok(Self {
            format: DEFAULT_FORMAT,
            stream: None,
            running: false,
        })
    }
}

impl AudioDevice for OboeDevice {
    fn format(&self) -> AudioFormat {
        self.format
    }
    fn start(&mut self, _callback: Box<dyn AudioCallback>) -> Result<(), AudioError> {
        // TODO: Set the data callback wrapping `callback.pull()`, then call
        // `stream.requestStart()`.
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        // TODO: Call `stream.requestStop()`, `stream.close()`.
        self.running = false;
        Ok(())
    }

    fn pause(&mut self) -> Result<(), AudioError> {
        // TODO: Call `stream.requestPause()`.
        Ok(())
    }

    fn resume(&mut self) -> Result<(), AudioError> {
        // TODO: Call `stream.requestStart()`.
        Ok(())
    }
}

impl Drop for OboeDevice {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
