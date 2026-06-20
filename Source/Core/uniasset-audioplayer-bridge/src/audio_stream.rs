//! C-callback-backed implementation of the [`AudioStream`] trait.
//!
//! `NativeAudioStream` wraps C function pointers so that external code can
//! supply audio data (file readers, network streams, procedural generators)
//! through a stable C ABI, analogous to how `NativeIOProvider` bridges
//! C I/O callbacks into Rust's `Read` + `Seek` in uniasset-bridge.

use std::ffi::c_void;
use std::sync::Arc;

use uniasset_audioplayer::mixer::AudioStream;
use uniasset_audioplayer::AudioError;

use crate::error::clear_error;
use crate::object::{impl_native_handle, NativeHandle, NativeHandleExts};

// ---------------------------------------------------------------------------
// C callback types
// ---------------------------------------------------------------------------

/// Called from the audio thread. Must be wait-free.
///
/// Reads up to `frame_count` frames into `buffer` (interleaved f32 samples).
/// Returns the number of **samples** written (frames × channels), or 0 at EOF.
type ReadFn = unsafe extern "C" fn(user_data: *mut c_void, buffer: *mut f32, frame_count: u64) -> u64;

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
/// Each method delegates to the corresponding C callback, passing through
/// the opaque `user_data` pointer that the C caller provided at creation time.
///
/// # Thread safety
///
/// The C callbacks are expected to be thread-safe — the C side is responsible
/// for any required synchronization. The struct is marked `Send + Sync` so
/// it can be shared via `Arc` to the audio thread.
pub struct NativeAudioStream {
    user_data: *mut c_void,
    read_fn: ReadFn,
    seek_fn: SeekFn,
    is_eof_fn: IsEofFn,
    channels_fn: ChannelsFn,
    sample_rate_fn: SampleRateFn,
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

impl_native_handle!(NativeAudioStream);

// ---------------------------------------------------------------------------
// Exported C API
// ---------------------------------------------------------------------------

/// Create a new audio stream backed by C callbacks.
///
/// `user_data` is an opaque pointer passed through to every callback.
/// All five callbacks must be non-null.
///
/// Returns a handle that can be passed to [`UAP_AudioPlayer_AddStream`].
/// Returns null and sets the thread-local error on failure.
///
/// # Safety
///
/// All five callbacks must be valid function pointers or null. Null callbacks
/// will cause an error to be returned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_AudioStream_Create(
    user_data: *mut c_void,
    read_fn: Option<ReadFn>,
    seek_fn: Option<SeekFn>,
    is_eof_fn: Option<IsEofFn>,
    channels_fn: Option<ChannelsFn>,
    sample_rate_fn: Option<SampleRateFn>,
) -> NativeHandle {
    clear_error();

    let read_fn = match read_fn {
        Some(f) => f,
        None => {
            crate::error::set_error("all audio stream callbacks must be non-null");
            return std::ptr::null();
        }
    };
    let seek_fn = match seek_fn {
        Some(f) => f,
        None => {
            crate::error::set_error("all audio stream callbacks must be non-null");
            return std::ptr::null();
        }
    };
    let is_eof_fn = match is_eof_fn {
        Some(f) => f,
        None => {
            crate::error::set_error("all audio stream callbacks must be non-null");
            return std::ptr::null();
        }
    };
    let channels_fn = match channels_fn {
        Some(f) => f,
        None => {
            crate::error::set_error("all audio stream callbacks must be non-null");
            return std::ptr::null();
        }
    };
    let sample_rate_fn = match sample_rate_fn {
        Some(f) => f,
        None => {
            crate::error::set_error("all audio stream callbacks must be non-null");
            return std::ptr::null();
        }
    };

    let stream = NativeAudioStream {
        user_data,
        read_fn,
        seek_fn,
        is_eof_fn,
        channels_fn,
        sample_rate_fn,
    };

    Arc::new(stream).into_handle()
}

/// Destroy an audio stream handle.
///
/// Drops the C caller's reference. The stream remains alive as long as
/// any [`UAP_AudioPlayer_AddStream`] references still exist.
///
/// # Safety
///
/// `handle` must be a valid handle from [`UAP_AudioStream_Create`],
/// and must not have been destroyed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_AudioStream_Destroy(handle: NativeHandle) {
    if handle.is_null() {
        return;
    }
    // Reconstruct and drop the Box<Arc<NativeAudioStream>>.
    unsafe {
        let _ = Box::from_raw(handle as *mut Arc<NativeAudioStream>);
    }
}
