//! High-level audio player that wires the HAL ([`AudioDevice`]) and
//! [`Mixer`] together into a simple playback API.
//!
//! # Architecture
//!
//! ```text
//! AudioStream(s) → Mixer → AudioCallback → AudioDevice → OS Audio
//!                    ↑                        ↑
//!                    AudioPlayer (owns both, wires them together)
//! ```
//!
//! # Example
//!
//! ```ignore
//! use uniasset::player::AudioPlayer;
//! use uniasset::mixer::AudioStream;
//!
//! let player = AudioPlayer::new().expect("Failed to open audio device");
//! let stream: Arc<dyn AudioStream> = ...;
//! let handle = player.add_stream(stream);
//! handle.set_volume(0.5);
//!
//! // Player starts playing immediately.
//! // Call player.cleanup_eof() periodically to remove finished streams.
//! ```

use std::sync::Arc;

use parking_lot::Mutex;

use crate::hal::{open_device, AudioCallback, AudioDevice};
use crate::mixer::{AudioStream, Mixer, PlayHandle};
use crate::AudioError;
use crate::AudioFormat;

/// Thin wrapper that delegates [`AudioCallback::pull`] to an [`Arc<Mixer>`].
///
/// The mixer is shared with [`AudioPlayer`] so streams can be added/removed
/// while the audio device is running.
struct MixerCallback {
    mixer: Arc<Mixer>,
}

impl AudioCallback for MixerCallback {
    fn pull(&self, buffer: &mut [f32]) -> usize {
        self.mixer.pull(buffer)
    }
}

/// A high-level audio player that combines platform audio output ([`AudioDevice`])
/// with lock-free mixing ([`Mixer`]).
///
/// `AudioPlayer` opens the default audio device on construction, creates a
/// [`Mixer`] that matches the hardware format, and starts playback immediately.
/// Streams can be added at any time through [`add_stream`](AudioPlayer::add_stream),
/// which returns a [`PlayHandle`] for per-stream control (volume, pause, seek).
///
/// # Lifecycle
///
/// - **Construction** — opens device, starts audio thread, begins pulling from mixer.
/// - **Playback** — add streams, control them via [`PlayHandle`].
/// - **Cleanup** — call [`cleanup_eof`](AudioPlayer::cleanup_eof) periodically
///   (or on a timer) to remove finished streams.
/// - **Shutdown** — drop the player or call [`stop`](AudioPlayer::stop).
///
/// # Thread Safety
///
/// All methods take `&self` and can be called from any thread. The internal
/// device is protected by a mutex (uncontended — only used for lifecycle calls).
/// The mixer is lock-free on the audio hot path.
pub struct AudioPlayer {
    /// The mixer, shared with the audio callback running on the device thread.
    mixer: Arc<Mixer>,

    /// The platform audio device. Behind a mutex because `start`/`stop`/
    /// `pause`/`resume` take `&mut self`.
    device: Mutex<Box<dyn AudioDevice>>,
}

impl AudioPlayer {
    /// Open the default audio device and start playback.
    ///
    /// The mixer is automatically configured to match the hardware's native
    /// sample rate and channel count for lowest latency.
    ///
    /// Returns an error if no output device is available or if the device
    /// cannot be started.
    pub fn new() -> Result<Self, AudioError> {
        let mut device = open_device()?;
        let format = device.format();

        let mixer = Arc::new(Mixer::new(format.sample_rate, format.channels));

        let callback = Box::new(MixerCallback {
            mixer: Arc::clone(&mixer),
        });
        device.start(callback)?;

        Ok(Self {
            mixer,
            device: Mutex::new(device),
        })
    }

    /// Return the hardware audio format (sample rate + channels).
    ///
    /// This matches the mixer's target format. Streams with different rates
    /// are automatically resampled.
    pub fn format(&self) -> AudioFormat {
        self.mixer.format()
    }

    /// Add an audio stream to the player.
    ///
    /// Returns a [`PlayHandle`] that can be used to control playback
    /// (volume, pause/resume, seek, per-stream effects).
    ///
    /// The stream will begin playing immediately (unless paused via the handle).
    pub fn add_stream(&self, stream: Arc<dyn AudioStream>) -> PlayHandle {
        self.mixer.add_stream(stream)
    }

    /// Remove streams that have reached EOF.
    ///
    /// This should be called periodically from the application thread
    /// (e.g., on a timer or at idle). The audio thread only sets the EOF
    /// flag; it never deallocates finished streams.
    pub fn cleanup_eof(&self) {
        self.mixer.cleanup_eof();
    }

    /// Number of streams currently in the mixer.
    pub fn stream_count(&self) -> usize {
        self.mixer.stream_count()
    }

    /// Pause playback (device-level, not per-stream).
    ///
    /// The audio callback will no longer be invoked. Use [`resume`](Self::resume)
    /// to continue playback.
    ///
    /// Returns an error if the underlying device operation fails.
    pub fn pause(&self) -> Result<(), AudioError> {
        self.device.lock().pause()
    }

    /// Resume playback after a [`pause`](Self::pause).
    ///
    /// Returns an error if the underlying device operation fails.
    pub fn resume(&self) -> Result<(), AudioError> {
        self.device.lock().resume()
    }

    /// Stop playback and release the audio device.
    ///
    /// After calling `stop`, the player can no longer be used for playback.
    /// Further calls to any method are safe but will have no effect.
    ///
    /// Returns an error if the underlying device operation fails.
    pub fn stop(&self) -> Result<(), AudioError> {
        self.device.lock().stop()
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        // Best-effort stop on drop. Ignore errors — the OS will clean up.
        let _ = self.device.lock().stop();
    }
}
