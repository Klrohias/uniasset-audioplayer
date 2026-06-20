//! Hardware Abstraction Layer for cross-platform audio output.
//!
//! This module defines the [`AudioCallback`] and [`AudioDevice`] traits that
//! abstract over platform-specific audio APIs (CoreAudio, WASAPI, Oboe).
//!
//! # Pull Model
//!
//! The audio device "pulls" PCM samples from the application via the
//! [`AudioCallback::pull`] method whenever the OS audio subsystem needs
//! data to fill its output buffer. The callback should produce samples
//! directly on the audio thread — no intermediate ring buffer is mandated
//! by the HAL. If a particular [`AudioStream`] implementation needs
//! buffering, it can use [`crate::util::RingBuffer`] internally.
//!
//! ```text
//! Mixer / AudioStream          Audio Callback             OS Audio
//!        │                         │                         │
//!        │<──── pull(buffer) ──────│                         │
//!        │── fill buffer ─────────>│                         │
//!        │                         │── output samples ──────>│
//! ```

pub mod platform;

use crate::error::AudioError;
use crate::types::AudioFormat;

// ── AudioCallback ──────────────────────────────────────────────────────

/// Callback trait for pulling PCM audio samples on demand.
///
/// Implementations are invoked by the platform audio thread whenever the
/// output device needs more samples. The callback should fill `buffer` with
/// interleaved `f32` samples and return the number of **frames** written.
///
/// # Buffer Layout
///
/// For a stereo stream, `buffer` is laid out as:
/// `[L0, R0, L1, R1, L2, R2, ...]`
///
/// `buffer.len()` equals `frame_count * channels`, where `frame_count` is
/// determined by the platform audio subsystem (typically the hardware buffer
/// size).
///
/// # Return Value
///
/// The return value is the number of **frames** (not samples) written.
/// Returning fewer frames than requested, or returning 0, causes the
/// remaining space to be filled with silence (zeros).
pub trait AudioCallback: Send + Sync {
    /// Called when the audio device needs PCM samples.
    ///
    /// This method takes `&self` (not `&mut self`) so the audio thread
    /// can share the callback without locks. All mutable state should
    /// live behind atomics (e.g., `AtomicU64` for volume, or
    /// [`crate::util::RingBuffer`] cursors).
    /// for volume).
    ///
    /// - `buffer`: interleaved f32 samples to fill. Length is always a
    ///   multiple of the channel count.
    ///
    /// Returns the number of frames actually written. A return value of 0
    /// means silence.
    fn pull(&self, buffer: &mut [f32]) -> usize;
}

// ── AudioDevice ────────────────────────────────────────────────────────

/// A platform audio output device.
///
/// An `AudioDevice` wraps the platform-specific audio API and drives an
/// [`AudioCallback`] on the audio thread.
pub trait AudioDevice: Send {
    /// Start playback. The device will begin calling `callback.pull()` on
    /// the audio thread to fetch samples.
    fn start(&mut self, callback: Box<dyn AudioCallback>) -> Result<(), AudioError>;

    /// Stop playback and release the audio resources.
    fn stop(&mut self) -> Result<(), AudioError>;

    /// Pause playback. The callback will no longer be invoked.
    fn pause(&mut self) -> Result<(), AudioError>;

    /// Resume playback after a pause.
    fn resume(&mut self) -> Result<(), AudioError>;
}

// ── Factory ────────────────────────────────────────────────────────────

/// Open the default audio output device for the given format.
///
/// Selects the appropriate platform backend at compile time:
///
/// | Platform   | Backend    |
/// |------------|-----------|
/// | macOS/iOS  | CoreAudio |
/// | Windows    | WASAPI    |
/// | Android    | Oboe      |
/// | Other      | Dummy     |
pub fn open_device(format: AudioFormat) -> Result<Box<dyn AudioDevice>, AudioError> {
    platform::create_device(format)
}
