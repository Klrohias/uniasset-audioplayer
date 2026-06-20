//! Demo: mix two sine-wave streams for 3 seconds via uniasset-audioplayer.
//!
//! Uses `AudioPlayer` (which wires HAL + Mixer together) and an
//! `AudioStream`-based sine wave generator.
//!
//! The `AudioStream::read` takes `&self` (not `&mut self`) so the audio
//! thread can share the stream lock-free. All mutable state (the phase
//! accumulator) lives behind atomics.

use std::f64::consts::TAU;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use uniasset::mixer::AudioStream;
use uniasset::player::AudioPlayer;
use uniasset::AudioError;

/// A simple sine-wave tone generator implementing [`AudioStream`].
///
/// Phase is stored as an `AtomicU64` holding the bits of an `f64` so it
/// can be atomically advanced on every call to `read`.
struct SineStream {
    /// Current phase (0.0 .. 1.0) stored as `f64::to_bits()`.
    phase: AtomicU64,
    /// Frequency in Hz.
    freq: f64,
    /// Sample rate in Hz.
    sample_rate: u32,
    /// Number of channels.
    channels: u16,
    /// Amplitude (0.0 .. 1.0).
    amplitude: f32,
}

impl SineStream {
    fn new(freq: f64, sample_rate: u32, channels: u16, amplitude: f32) -> Self {
        Self {
            phase: AtomicU64::new(0u64),
            freq,
            sample_rate,
            channels,
            amplitude,
        }
    }
}

impl AudioStream for SineStream {
    fn read(&self, buffer: &mut [f32], frame_count: u64) -> usize {
        let channels = self.channels as usize;
        let max_frames = buffer.len() / channels;
        let actual_frames = max_frames.min(frame_count as usize);

        // Load current phase (as f64 bits), compute all samples,
        // then write back the final phase.
        let phase_bits = self.phase.load(Ordering::Relaxed);
        let mut phase = f64::from_bits(phase_bits);

        let phase_inc = self.freq / self.sample_rate as f64;

        for frame in 0..actual_frames {
            let sample = (phase * TAU).sin() as f32 * self.amplitude;

            // Write interleaved samples (same value for all channels).
            for ch in 0..channels {
                buffer[frame * channels + ch] = sample;
            }

            phase += phase_inc;
            if phase >= 1.0 {
                phase -= 1.0;
            }
        }

        // Store updated phase back into the atomic.
        self.phase.store(phase.to_bits(), Ordering::Relaxed);

        actual_frames * channels
    }

    fn seek(&self, frame: u64) -> Result<(), AudioError> {
        let phase = (frame as f64 * self.freq / self.sample_rate as f64).fract();
        self.phase.store(phase.to_bits(), Ordering::Relaxed);
        Ok(())
    }

    fn is_eof(&self) -> bool {
        // Continuous tone — never ends.
        false
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}

fn main() {
    println!("Opening default audio device...");
    let player = AudioPlayer::new().expect("Failed to create audio player");

    let format = player.format();
    println!(
        "Device opened: {} Hz, {} channel(s)",
        format.sample_rate, format.channels
    );

    let tone_a: Arc<dyn AudioStream> = Arc::new(SineStream::new(
        440.0,
        format.sample_rate,
        format.channels,
        0.20,
    ));
    let tone_b: Arc<dyn AudioStream> = Arc::new(SineStream::new(
        660.0,
        format.sample_rate,
        format.channels,
        0.15,
    ));

    println!("Adding two sine streams to mixer: 440 Hz + 660 Hz...");
    let _handle_a = player.add_stream(tone_a);
    let _handle_b = player.add_stream(tone_b);

    println!("Playing for 1 seconds...");
    thread::sleep(Duration::from_secs(1));

    _handle_a.pause();

    println!("Playing for 1 seconds...");
    thread::sleep(Duration::from_secs(1));

    println!("Stopping...");
    player.stop().expect("Failed to stop playback");

    println!("Done.");
}
