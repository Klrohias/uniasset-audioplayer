//! Audio mixing pipeline: stream sources, playback control, and mixing.
//!
//! # Architecture
//!
//! ```text
//! AudioStream(s) → Mixer (implements AudioCallback) → AudioDevice → OS Audio
//!                  ↑
//!                  PlayHandle (per-stream control)
//! ```
//!
//! # Thread Safety
//!
//! The mixer uses a **snapshot pattern** for lock-free audio thread access:
//! - Control thread builds a read-only snapshot of all streams and swaps
//!   an atomic pointer.
//! - Audio thread reads the snapshot pointer once per `pull()` call,
//!   iterating the snapshot with zero locks.
//! - Stream state (alive, paused, volume, eof) lives behind atomics so
//!   both threads can access it without contention.

mod audio_stream;
mod mixer;
mod play_handle;

pub use audio_stream::*;
pub use mixer::*;
pub use play_handle::*;
