//! Common types for the uniasset audio player.

/// Describes the format of an audio stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFormat {
    /// Sample rate in Hz (e.g., 44100, 48000).
    pub sample_rate: u32,
    /// Number of audio channels (1 = mono, 2 = stereo).
    pub channels: u16,
}

impl AudioFormat {
    /// Create a new `AudioFormat` with the given sample rate and channel count.
    pub const fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            sample_rate,
            channels,
        }
    }

    /// Convert a frame count to a sample count.
    ///
    /// A "sample" here means a single `f32` value in an interleaved buffer.
    /// One frame contains `channels` samples.
    #[inline]
    pub fn frames_to_samples(&self, frame_count: usize) -> usize {
        frame_count * self.channels as usize
    }

    /// Return the number of bytes consumed by one frame (all channels).
    /// Each sample is an `f32` = 4 bytes.
    #[inline]
    pub fn bytes_per_frame(&self) -> usize {
        self.channels as usize * 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frames_to_samples() {
        let fmt = AudioFormat::new(48000, 2);
        assert_eq!(fmt.frames_to_samples(100), 200);
        assert_eq!(fmt.frames_to_samples(0), 0);
    }

    #[test]
    fn test_mono_format() {
        let fmt = AudioFormat::new(44100, 1);
        assert_eq!(fmt.frames_to_samples(10), 10);
        assert_eq!(fmt.bytes_per_frame(), 4);
    }

    #[test]
    fn test_bytes_per_frame() {
        let fmt = AudioFormat::new(48000, 2);
        assert_eq!(fmt.bytes_per_frame(), 8);
    }
}
