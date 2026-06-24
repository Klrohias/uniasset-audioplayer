//! C-callback-backed implementation of the [`AudioStream`] trait.
//!
//! `NativeAudioStream` wraps C function pointers so that external code can
//! supply audio data (file readers, network streams, procedural generators)
//! through a stable C ABI.
//!
//! The struct is `#[repr(C)]` — the caller allocates it and passes a pointer
//! to [`UAP_AudioPlayer_AddNativeStream`]. The Rust side stores a copy; the caller
//! must keep the callbacks and `user_data` valid for the stream's lifetime.

use std::ffi::c_void;

use uniasset_audioplayer::mixer::AudioStream;
use uniasset_audioplayer::AudioError;
use std::sync::Arc;

pub type AudioStreamWrapper = Box<Arc<dyn AudioStream>>;

// ---------------------------------------------------------------------------
// C callback types
// ---------------------------------------------------------------------------

/// Called from the audio thread. Must be wait-free.
///
/// Reads up to `frame_count` frames into `buffer` (interleaved f32 samples).
/// Returns the number of **samples** written (frames × channels), or 0 at EOF.
type ReadFn =
    unsafe extern "C" fn(user_data: *mut c_void, buffer: *mut f32, frame_count: u64) -> u64;

/// Seek to the given frame position. Returns true on success.
type SeekFn = unsafe extern "C" fn(user_data: *mut c_void, frame: u64) -> bool;

/// Returns true if the stream has reached its end. Must be wait-free.
type IsEofFn = unsafe extern "C" fn(user_data: *mut c_void) -> bool;

/// Returns the number of channels (1 = mono, 2 = stereo).
type ChannelsFn = unsafe extern "C" fn(user_data: *mut c_void) -> u16;

/// Returns the sample rate in Hz (e.g., 44100, 48000).
type SampleRateFn = unsafe extern "C" fn(user_data: *mut c_void) -> u32;

// ---------------------------------------------------------------------------
// NativeAudioStream
// ---------------------------------------------------------------------------

/// An [`AudioStream`] backed by C function pointers.
///
/// `#[repr(C)]` so the C side can allocate and populate it directly.
/// Each method delegates to the corresponding C callback, passing through
/// the opaque `user_data` pointer that the C caller provided.
///
/// # Safety
///
/// The caller must ensure that `user_data` and all function pointers remain
/// valid for as long as the stream is alive in the mixer.
#[repr(C)]
pub struct NativeAudioStream {
    pub user_data: *mut c_void,
    pub read_fn: ReadFn,
    pub seek_fn: SeekFn,
    pub is_eof_fn: IsEofFn,
    pub channels_fn: ChannelsFn,
    pub sample_rate_fn: SampleRateFn,
}

// Safety: the C side is responsible for callback thread safety.
unsafe impl Send for NativeAudioStream {}
unsafe impl Sync for NativeAudioStream {}

impl AudioStream for NativeAudioStream {
    fn read(&self, buffer: &mut [f32], frame_count: u64) -> usize {
        let samples = unsafe { (self.read_fn)(self.user_data, buffer.as_mut_ptr(), frame_count) };
        samples as usize
    }

    fn seek(&self, frame: u64) -> Result<(), AudioError> {
        let ok = unsafe { (self.seek_fn)(self.user_data, frame) };
        if ok {
            Ok(())
        } else {
            Err(AudioError::StreamError("seek failed".into()))
        }
    }

    fn is_eof(&self) -> bool {
        unsafe { (self.is_eof_fn)(self.user_data) }
    }

    fn channels(&self) -> u16 {
        unsafe { (self.channels_fn)(self.user_data) }
    }

    fn sample_rate(&self) -> u32 {
        unsafe { (self.sample_rate_fn)(self.user_data) }
    }
}
