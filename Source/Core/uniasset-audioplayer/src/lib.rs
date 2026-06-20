//! # uniasset-audioplayer
//!
//! Cross-platform, low-latency audio playback library in Rust.
//!
//! ## Supported Platforms
//!
//! - **macOS / iOS**: CoreAudio (AudioUnit)
//! - **Windows**: WASAPI
//! - **Android**: Oboe (AAudio)
//!
//! ## Architecture
//!
//! The library is built around a **pull-based** audio model:
//!
//! 1. **[`AudioStream`] trait** — defines an audio data source interface
//! 2. **[`hal`] layer** — abstracts platform-specific audio APIs
//! 3. **Mixer** — lock-free mixer for multi-stream mixing
//! 4. **[`util::RingBuffer`]** — optional lock-free SPSC ring buffer
//! 5. **AudioPlayer** — top-level wrapper
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use uniasset::hal::{open_device, AudioCallback, AudioDevice};
//! use uniasset::AudioFormat;
//! use uniasset::util::RingBuffer;
//! use std::sync::Arc;
//!
//! // Create an audio device
//! let format = AudioFormat::new(48000, 2);
//! let mut device = open_device(format).unwrap();
//!
//! // Create a ring buffer for sample exchange (optional — use only if
//! // your audio source runs on a separate thread from the audio callback).
//! let ring = Arc::new(RingBuffer::new(8192));
//!
//! // Write some samples into the ring buffer from another thread...
//!
//! // Define the pull callback that reads from the ring buffer
//! struct MyCallback { ring: Arc<RingBuffer>, channels: u16 }
//! impl AudioCallback for MyCallback {
//!     fn pull(&self, buffer: &mut [f32]) -> usize {
//!         self.ring.read(buffer) / self.channels as usize
//!     }
//! }
//!
//! // Start playback — the device will pull from our callback
//! device.start(Box::new(MyCallback { ring, channels: 2 })).unwrap();
//! ```

mod error;
mod types;
pub mod util;

pub use error::AudioError;
pub use types::AudioFormat;
