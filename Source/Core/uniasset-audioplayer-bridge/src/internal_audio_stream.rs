//! Bridge functions for operating on an `InternalAudioStream` handle
//! (`Box<Arc<dyn AudioStream>>`) — binding the [`AudioStream`] trait
//! methods through the FFI.
//!
//! These work on any handle that encodes `Box<Arc<dyn AudioStream>>`,
//! whether created by [`UAP_BufferedAudioStream_Create`] or by other
//! native stream factories.

use std::mem::ManuallyDrop;

use crate::audio_stream::AudioStreamWrapper;
use crate::error::{clear_error, set_error};
use crate::object::{NativeHandle, NativeHandleExts};

// ---------------------------------------------------------------------------
// AudioStream trait bindings
// ---------------------------------------------------------------------------

/// Read interleaved `f32` samples from the stream.
///
/// `buffer` must be at least `frame_count * channels()` samples.
/// Returns the number of **samples** written, or 0 at EOF.
///
/// Must be wait-free — called from the real-time audio thread.
///
/// # Safety
/// `buffer` must be valid for writes of `frame_count * channels()` samples.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_InternalAudioStream_Read(
    handle: NativeHandle,
    buffer: *mut f32,
    frame_count: u64,
) -> u64 {
    clear_error();
    let stream = ManuallyDrop::new(AudioStreamWrapper::from_handle(handle));
    let channels = stream.channels() as usize;
    let len = frame_count as usize * channels;
    let buf = unsafe { std::slice::from_raw_parts_mut(buffer, len) };
    stream.read(buf, frame_count) as u64
}

/// Seek to the given frame position.
///
/// Returns 1 on success, 0 on failure (error reported via
/// [`UAP_HasError`] / [`UAP_GetError`]).
///
/// # Safety
/// `handle` must be a valid handle and must not have been destroyed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_InternalAudioStream_Seek(handle: NativeHandle, frame: u64) -> u8 {
    clear_error();
    let stream = ManuallyDrop::new(AudioStreamWrapper::from_handle(handle));
    match stream.seek(frame) {
        Ok(()) => 1,
        Err(e) => {
            set_error(e);
            0
        }
    }
}

/// Returns 1 if the stream has reached its end, 0 otherwise.
///
/// Must be wait-free — called from the real-time audio thread.
///
/// # Safety
/// `handle` must be a valid handle and must not have been destroyed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_InternalAudioStream_IsEof(handle: NativeHandle) -> u8 {
    clear_error();
    let stream = ManuallyDrop::new(AudioStreamWrapper::from_handle(handle));
    stream.is_eof() as u8
}

/// Return the number of channels (1 = mono, 2 = stereo).
///
/// # Safety
/// `handle` must be a valid handle and must not have been destroyed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_InternalAudioStream_Channels(handle: NativeHandle) -> u16 {
    clear_error();
    let stream = ManuallyDrop::new(AudioStreamWrapper::from_handle(handle));
    stream.channels()
}

/// Return the sample rate in Hz (e.g., 44100, 48000).
///
/// # Safety
/// `handle` must be a valid handle and must not have been destroyed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_InternalAudioStream_SampleRate(handle: NativeHandle) -> u32 {
    clear_error();
    let stream = ManuallyDrop::new(AudioStreamWrapper::from_handle(handle));
    stream.sample_rate()
}

// ---------------------------------------------------------------------------
// Ownership
// ---------------------------------------------------------------------------

/// Destroy an `InternalAudioStream` handle.
///
/// Drops the C caller's reference. The underlying stream (and any mixer
/// references) continue to live independently.
///
/// # Safety
/// `handle` must be a valid handle and must not have been destroyed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_InternalAudioStream_Destroy(handle: NativeHandle) {
    drop(AudioStreamWrapper::from_handle(handle));
}
