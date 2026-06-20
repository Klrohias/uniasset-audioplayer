use crate::AudioError;

/// A source of interleaved `f32` PCM audio samples.
///
/// Implementations are called from the audio thread via [`Mixer`] and must
/// therefore be **wait-free** — no blocking, no locks, no allocation in
/// `read()`.
///
/// The trait takes `&self` (not `&mut self`) so streams can be shared
/// between the mixer and [`PlayHandle`] without locks. Mutable state
/// should live behind atomics.
///
/// # Example (sine wave generator)
///
/// ```ignore
/// struct SineStream {
///     phase: AtomicU64,  // f64::to_bits()
///     freq: f64,
///     sample_rate: u32,
/// }
///
/// impl AudioStream for SineStream {
///     fn read(&self, buffer: &mut [f32], _frame_count: u64) -> usize {
///         // fill buffer with sine wave samples...
///         buffer.len()
///     }
///     fn seek(&self, _frame: u64) -> Result<(), AudioError> { Ok(()) }
///     fn is_eof(&self) -> bool { false }
///     fn channels(&self) -> u16 { 1 }
///     fn sample_rate(&self) -> u32 { self.sample_rate }
/// }
/// ```
pub trait AudioStream: Send + Sync {
    /// Read interleaved `f32` samples into `buffer`.
    ///
    /// `frame_count` indicates how many frames the mixer would like to read.
    /// The implementation may read fewer.
    ///
    /// Returns the number of **samples** (not frames) written. Returns 0
    /// if the stream has reached EOF or has no data available yet.
    ///
    /// Must be wait-free — called from the real-time audio thread.
    fn read(&self, buffer: &mut [f32], frame_count: u64) -> usize;

    /// Seek to the given frame position.
    fn seek(&self, frame: u64) -> Result<(), AudioError>;

    /// Whether the stream has reached its end.
    /// Must be wait-free — called from the real-time audio thread.
    fn is_eof(&self) -> bool;

    /// Number of audio channels (1 = mono, 2 = stereo).
    fn channels(&self) -> u16;

    /// Sample rate in Hz (e.g., 44100, 48000).
    fn sample_rate(&self) -> u32;
}
