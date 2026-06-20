//! Oboe backend for Android.
//!
//! Uses the Oboe (AAudio) native audio library with a data callback
//! that pulls from [`AudioCallback::pull`].

use crate::error::AudioError;
use crate::hal::AudioCallback;
use crate::hal::AudioDevice;
use crate::types::AudioFormat;

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
    /// Create a new Oboe output device for the requested format.
    pub fn new(format: AudioFormat) -> Result<Self, AudioError> {
        // TODO: Create an oboe::AudioStreamBuilder, set direction to Output,
        // set performance mode to LowLatency, set sharing mode to Shared,
        // set format to Float, set sample rate and channel count, set the
        // data callback, and open the stream.

        Ok(Self {
            format,
            stream: None,
            running: false,
        })
    }
}

impl AudioDevice for OboeDevice {
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
