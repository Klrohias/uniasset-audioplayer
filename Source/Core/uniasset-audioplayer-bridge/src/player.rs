//! C FFI functions for the high-level [`AudioPlayer`].
//!
//! The `NativeHandle` for `AudioPlayer` encodes a `Box<Arc<AudioPlayer>>`.

use std::mem::ManuallyDrop;
use std::ptr::{self, null};
use std::sync::Arc;

use uniasset_audioplayer::mixer::AudioStream;
use uniasset_audioplayer::player::AudioPlayer;

use crate::audio_stream::NativeAudioStream;
use crate::error::clear_error;
use crate::object::{failible_to_native, NativeHandle, NativeHandleExts};

pub(crate) type AudioPlayerWrapper = Box<Arc<AudioPlayer>>;

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

/// Create a new `AudioPlayer` and open the platform audio device.
///
/// Returns a handle on success, or null on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_AudioPlayer_New() -> NativeHandle {
    clear_error();
    failible_to_native(
        || AudioPlayer::new().map(|it| Box::new(Arc::new(it)).into_handle()),
        || null(),
    )
}

/// Destroy an `AudioPlayer`.
///
/// Drops the C caller's reference. When the last reference is dropped,
/// playback stops and the audio device is released.
///
/// # Safety
/// `handle` must be a valid handle from [`UAP_AudioPlayer_New`]
/// and must not have been destroyed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_AudioPlayer_Destroy(handle: NativeHandle) {
    drop(AudioPlayerWrapper::from_handle(handle))
}

// ---------------------------------------------------------------------------
// Format
// ---------------------------------------------------------------------------

/// Query the audio format (sample rate / channel count) of the output device.
///
/// # Safety
/// `out_sample_rate` and `out_channels` must be valid, non-null pointers to
/// writable `u32` and `u16` respectively.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_AudioPlayer_Format(
    handle: NativeHandle,
    out_sample_rate: *mut u32,
    out_channels: *mut u16,
) {
    clear_error();

    let wrapper = ManuallyDrop::new(AudioPlayerWrapper::from_handle(handle));

    let fmt = wrapper.format();
    unsafe {
        *out_sample_rate = fmt.sample_rate;
        *out_channels = fmt.channels;
    }
}

// ---------------------------------------------------------------------------
// Stream management
// ---------------------------------------------------------------------------

/// Add an audio stream to the player.
///
/// `stream` must point to a valid `#[repr(C)] NativeAudioStream` that the
/// caller will keep alive for the duration of playback. The Rust side stores
/// a copy of the struct.
///
/// If `play_immediate` is non-zero, the stream begins playing immediately;
/// otherwise it is added in a paused state.
///
/// Returns a new [`UAP_PlayHandle`] that controls this stream's playback.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_AudioPlayer_AddStream(
    handle: NativeHandle,
    stream: *const NativeAudioStream,
    play_immediate: u8,
) -> NativeHandle {
    clear_error();
    let wrapper = ManuallyDrop::new(AudioPlayerWrapper::from_handle(handle));
    let stream: Arc<dyn AudioStream> = Arc::new(unsafe { ptr::read(stream) });
    let play_immediate = play_immediate == 1;

    let play_handle = wrapper.add_stream(stream, play_immediate);
    Box::new(Arc::new(play_handle)).into_handle()
}

/// Return the number of currently active streams.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_AudioPlayer_StreamCount(handle: NativeHandle) -> u32 {
    clear_error();

    let wrapper = ManuallyDrop::new(AudioPlayerWrapper::from_handle(handle));
    wrapper.stream_count() as u32
}

// ---------------------------------------------------------------------------
// Playback control
// ---------------------------------------------------------------------------

/// Pause playback on the audio device.
///
/// On failure, the error is reported via [`UAP_HasError`] / [`UAP_GetError`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_AudioPlayer_Pause(handle: NativeHandle) {
    clear_error();

    let wrapper = ManuallyDrop::new(AudioPlayerWrapper::from_handle(handle));
    failible_to_native(|| wrapper.pause(), || ())
}

/// Resume playback on the audio device.
///
/// On failure, the error is reported via [`UAP_HasError`] / [`UAP_GetError`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_AudioPlayer_Resume(handle: NativeHandle) {
    clear_error();

    let wrapper = ManuallyDrop::new(AudioPlayerWrapper::from_handle(handle));
    failible_to_native(|| wrapper.resume(), || ())
}
