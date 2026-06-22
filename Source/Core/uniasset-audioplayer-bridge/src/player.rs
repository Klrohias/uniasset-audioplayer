//! C FFI functions for the high-level [`AudioPlayer`].
//!
//! The `NativeHandle` for `AudioPlayer` encodes a `Box<Arc<AudioPlayer>>`.

use std::sync::Arc;

use uniasset_audioplayer::mixer::AudioStream;
use uniasset_audioplayer::player::AudioPlayer;

use crate::audio_stream::NativeAudioStream;
use crate::error::{clear_error, set_error};
use crate::object::{failible_to_native, impl_native_handle, NativeHandle, NativeHandleExts};

impl_native_handle!(AudioPlayer);

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

/// Reconstitute an `&Arc<AudioPlayer>` from a handle.
///
/// Returns `None` if the handle is null.
unsafe fn get_player(handle: NativeHandle) -> Option<&'static Arc<AudioPlayer>> {
    if handle.is_null() {
        return None;
    }
    Some(unsafe { Arc::<AudioPlayer>::from_handle(handle) })
}

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

/// Create a new `AudioPlayer` and open the platform audio device.
///
/// Returns a handle on success. Returns null on failure — check
/// `UAP_HasError` / `UAP_GetError` for details.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_AudioPlayer_New() -> NativeHandle {
    clear_error();
    failible_to_native(
        AudioPlayer::new().map(|p| Arc::new(p).into_handle()),
        std::ptr::null(),
    )
}

/// Destroy an `AudioPlayer`.
///
/// Stops playback and releases the audio device.
///
/// # Safety
///
/// `handle` must be a valid handle from [`UAP_AudioPlayer_New`]
/// and must not have been destroyed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_AudioPlayer_Destroy(handle: NativeHandle) {
    if handle.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(handle as *mut Arc<AudioPlayer>);
    }
}

// ---------------------------------------------------------------------------
// Format
// ---------------------------------------------------------------------------

/// Query the audio format (sample rate / channel count) of the output device.
///
/// # Safety
///
/// `out_sample_rate` and `out_channels` must be valid pointers to writable
/// `u32` and `u16` respectively. No-op if any pointer is null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_AudioPlayer_Format(
    handle: NativeHandle,
    out_sample_rate: *mut u32,
    out_channels: *mut u16,
) {
    if out_sample_rate.is_null() || out_channels.is_null() {
        return;
    }
    let player = match unsafe { get_player(handle) } {
        Some(p) => p,
        None => return,
    };
    let fmt = player.format();
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
/// Returns a new [`UAP_PlayHandle`] that controls this stream's playback.
/// Returns null on error — check `UAP_HasError` / `UAP_GetError`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_AudioPlayer_AddStream(
    handle: NativeHandle,
    stream: *const NativeAudioStream,
) -> NativeHandle {
    clear_error();

    let player = match unsafe { get_player(handle) } {
        Some(p) => p,
        None => {
            set_error("null audio player handle");
            return std::ptr::null();
        }
    };

    if stream.is_null() {
        set_error("null audio stream pointer");
        return std::ptr::null();
    }

    // Copy the caller's struct — we own this copy now.
    let stream: NativeAudioStream = unsafe { std::ptr::read(stream) };

    let trait_obj: Arc<dyn AudioStream> = Arc::new(stream);

    let play_handle = player.add_stream(trait_obj);
    Arc::new(play_handle).into_handle()
}

/// Remove all streams that have reached EOF.
///
/// Call periodically (e.g., once per frame or on a timer) to free resources.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_AudioPlayer_CleanupEof(handle: NativeHandle) {
    if let Some(player) = unsafe { get_player(handle) } {
        player.cleanup_eof();
    }
}

/// Return the number of currently active streams.
///
/// Returns 0 for null handles.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_AudioPlayer_StreamCount(handle: NativeHandle) -> u32 {
    unsafe { get_player(handle) }.map_or(0, |p| p.stream_count() as u32)
}

// ---------------------------------------------------------------------------
// Playback control
// ---------------------------------------------------------------------------

/// Pause playback on the audio device.
///
/// Returns true on success. On failure returns false — check `UAP_HasError`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_AudioPlayer_Pause(handle: NativeHandle) -> bool {
    clear_error();
    let player = match unsafe { get_player(handle) } {
        Some(p) => p,
        None => {
            set_error("null audio player handle");
            return false;
        }
    };
    failible_to_native(player.pause().map(|()| true), false)
}

/// Resume playback on the audio device.
///
/// Returns true on success. On failure returns false — check `UAP_HasError`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_AudioPlayer_Resume(handle: NativeHandle) -> bool {
    clear_error();
    let player = match unsafe { get_player(handle) } {
        Some(p) => p,
        None => {
            set_error("null audio player handle");
            return false;
        }
    };
    failible_to_native(player.resume().map(|()| true), false)
}

/// Stop playback and close the audio device.
///
/// Returns true on success. On failure returns false — check `UAP_HasError`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_AudioPlayer_Stop(handle: NativeHandle) -> bool {
    clear_error();
    let player = match unsafe { get_player(handle) } {
        Some(p) => p,
        None => {
            set_error("null audio player handle");
            return false;
        }
    };
    failible_to_native(player.stop().map(|()| true), false)
}
