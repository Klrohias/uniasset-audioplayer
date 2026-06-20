//! Demo: play a 440 Hz sine wave for 3 seconds via uniasset-audioplayer.
//!
//! The `AudioCallback::pull` takes `&self` (not `&mut self`) so the audio
//! thread can share the callback lock-free. All mutable state (the phase
//! accumulator) lives behind atomics.

use std::f64::consts::TAU;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use uniasset::hal::{open_device, AudioCallback};

/// A simple sine-wave tone generator.
///
/// Phase is stored as an `AtomicU64` holding the bits of an `f64` so it
/// can be atomically advanced on every call to `pull`.
struct SineWave {
    /// Current phase (0.0 .. 1.0) stored as `f64::to_bits()`.
    phase: AtomicU64,
    /// Frequency in Hz.
    freq: f64,
    /// Number of channels the device expects.
    channels: usize,
    /// Sample rate reported by the device.
    sample_rate: u32,
    /// Amplitude (0.0 .. 1.0).
    amplitude: f32,
}

impl SineWave {
    fn new(freq: f64, channels: usize, sample_rate: u32) -> Self {
        Self {
            phase: AtomicU64::new(0u64),
            freq,
            channels,
            sample_rate,
            amplitude: 0.3,
        }
    }
}

impl AudioCallback for SineWave {
    fn pull(&self, buffer: &mut [f32]) -> usize {
        let channels = self.channels;
        let frame_count = buffer.len() / channels;

        // Load current phase (as f64 bits), compute all samples,
        // then write back the final phase.
        let phase_bits = self.phase.load(Ordering::Relaxed);
        let mut phase = f64::from_bits(phase_bits);

        let phase_inc = self.freq / self.sample_rate as f64;

        for frame in 0..frame_count {
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
        self.phase
            .store(phase.to_bits(), Ordering::Relaxed);

        frame_count
    }
}

fn main() {
    println!("Opening default audio device...");
    let mut device = open_device().expect("Failed to open audio device");

    let format = device.format();
    println!(
        "Device opened: {} Hz, {} channel(s)",
        format.sample_rate, format.channels
    );

    let callback = Box::new(SineWave::new(
        440.0, // A4
        format.channels as usize,
        format.sample_rate,
    ));

    println!("Starting 440 Hz sine wave...");
    device.start(callback).expect("Failed to start playback");

    println!("Playing for 3 seconds...");
    thread::sleep(Duration::from_secs(3));

    println!("Stopping...");
    device.stop().expect("Failed to stop playback");

    println!("Done.");
}
