//! Bridge for [`BufferedAudioStream`] — wraps any [`AudioStream`] in a
//! 4-second ring buffer for smooth playback.
//!
//! The `NativeHandle` for a buffered stream is the same `AudioStreamWrapper`
//! type (`Box<Arc<dyn AudioStream>>`) returned by [`UAP_BufferedAudioStream_Create`],
//! so it can be passed directly to [`UAP_AudioPlayer_AddStream`].

use std::mem::ManuallyDrop;
use std::ptr;
use std::sync::Arc;

use uniasset_audioplayer::mixer::AudioStream;
use uniasset_audioplayer::stream::buffered_stream::BufferedAudioStream;

use crate::audio_stream::{AudioStreamWrapper, NativeAudioStream};
use crate::error::clear_error;
use crate::object::{failible_to_native, NativeHandle, NativeHandleExts};

/// Wrap a native audio stream in a [`BufferedAudioStream`].
///
/// `stream` must be a valid `NativeHandle` encoding a
/// `Box<Arc<dyn AudioStream>>`. The handle is <b>not</b> consumed —
/// the caller remains responsible for destroying it.
///
/// Returns a new `NativeHandle` encoding `Box<Arc<dyn AudioStream>>`
/// (the buffered wrapper), or null on failure.
///
/// Destroy the returned handle with [`UAP_InternalAudioStream_Destroy`].
///
/// # Safety
/// `stream` must be a valid handle and must not have been destroyed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_BufferedAudioStream_Create(stream: NativeHandle) -> NativeHandle {
    clear_error();
    let wrapper = ManuallyDrop::new(AudioStreamWrapper::from_handle(stream));
    let inner = Arc::clone(&wrapper);

    failible_to_native(
        || {
            let buffered = BufferedAudioStream::new(inner)
                .map_err(|e| uniasset_audioplayer::AudioError::StreamError(e.to_string()))?;
            let arc: Arc<dyn AudioStream> = Arc::new(buffered);
            Ok::<_, uniasset_audioplayer::AudioError>(Box::new(arc).into_handle())
        },
        || ptr::null(),
    )
}

/// Wrap a native audio stream (callbacks struct) in a [`BufferedAudioStream`].
///
/// `stream` must point to a valid, initialized [`NativeAudioStream`].
/// The struct is copied — the caller retains ownership of the original.
///
/// Returns a new `NativeHandle` encoding `Box<Arc<dyn AudioStream>>`
/// (the buffered wrapper), or null on failure.
///
/// Destroy the returned handle with [`UAP_InternalAudioStream_Destroy`].
///
/// # Safety
/// `stream` must point to a valid `NativeAudioStream` whose callbacks
/// and `user_data` remain valid for the stream's lifetime.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_BufferedAudioStream_CreateFromNative(
    stream: *const NativeAudioStream,
) -> NativeHandle {
    clear_error();
    let native: Arc<dyn AudioStream> = Arc::new(unsafe { ptr::read(stream) });

    failible_to_native(
        || {
            let buffered = BufferedAudioStream::new(native)
                .map_err(|e| uniasset_audioplayer::AudioError::StreamError(e.to_string()))?;
            let arc: Arc<dyn AudioStream> = Arc::new(buffered);
            Ok::<_, uniasset_audioplayer::AudioError>(Box::new(arc).into_handle())
        },
        || ptr::null(),
    )
}
