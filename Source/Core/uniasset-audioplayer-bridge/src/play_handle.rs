//! C FFI functions for per-stream playback control.
//!
//! All functions operate on a `NativeHandle` that wraps a
//! `Box<Arc<PlayHandle>>`.

use std::ffi::c_void;
use std::sync::Arc;

use uniasset_audioplayer::mixer::PlayHandle;

use crate::error::{clear_error, set_error};
use crate::object::{failible_to_native, impl_native_handle, NativeHandle, NativeHandleExts};

impl_native_handle!(PlayHandle);

// ---------------------------------------------------------------------------
// Helper: retrieve a reference without consuming the handle
// ---------------------------------------------------------------------------

/// Reconstitute an `&Arc<PlayHandle>` from a handle.
///
/// Returns `None` if the handle is null.
///
/// # Safety
///
/// `handle` must be a valid `NativeHandle` for `PlayHandle`.
unsafe fn get_play_handle(handle: NativeHandle) -> Option<&'static Arc<PlayHandle>> {
    if handle.is_null() {
        return None;
    }
    Some(unsafe { Arc::<PlayHandle>::from_handle(handle) })
}

// ---------------------------------------------------------------------------
// Ownership
// ---------------------------------------------------------------------------

/// Destroy a `PlayHandle`.
///
/// Drops the C caller's reference. The handle remains valid to the mixer
/// until the stream is cleaned up via [`UAP_AudioPlayer_CleanupEof`].
///
/// # Safety
///
/// `handle` must be a valid handle from [`UAP_AudioPlayer_AddStream`]
/// and must not have been destroyed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_PlayHandle_Destroy(handle: NativeHandle) {
    if handle.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(handle as *mut Arc<PlayHandle>);
    }
}

// ---------------------------------------------------------------------------
// Playback control
// ---------------------------------------------------------------------------

/// Pause playback for this stream.
///
/// No-op if the handle is null or the stream is no longer alive.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_PlayHandle_Pause(handle: NativeHandle) {
    if let Some(ph) = unsafe { get_play_handle(handle) } {
        ph.pause();
    }
}

/// Resume playback for this stream.
///
/// No-op if the handle is null or the stream is no longer alive.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_PlayHandle_Resume(handle: NativeHandle) {
    if let Some(ph) = unsafe { get_play_handle(handle) } {
        ph.resume();
    }
}

/// Returns true if the stream is currently paused.
///
/// Returns false for null handles.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_PlayHandle_IsPaused(handle: NativeHandle) -> bool {
    unsafe { get_play_handle(handle) }.map_or(false, |ph| ph.is_paused())
}

/// Returns true if the stream is still alive (not cleaned up by EOF cleanup).
///
/// Returns false for null handles.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_PlayHandle_IsAlive(handle: NativeHandle) -> bool {
    unsafe { get_play_handle(handle) }.map_or(false, |ph| ph.is_alive())
}

/// Signal the stream to stop. The mixer will clean it up on the next
/// [`UAP_AudioPlayer_CleanupEof`] call.
///
/// No-op if the handle is null or the stream is no longer alive.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_PlayHandle_Stop(handle: NativeHandle) {
    if let Some(ph) = unsafe { get_play_handle(handle) } {
        ph.stop();
    }
}

// ---------------------------------------------------------------------------
// Volume
// ---------------------------------------------------------------------------

/// Set the volume for this stream. `volume` is clamped to `[0.0, 1.0]`.
///
/// No-op if the handle is null or the stream is no longer alive.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_PlayHandle_SetVolume(handle: NativeHandle, volume: f32) {
    if let Some(ph) = unsafe { get_play_handle(handle) } {
        ph.set_volume(volume);
    }
}

/// Return the current volume in `[0.0, 1.0]`.
///
/// Returns 0.0 for null handles.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_PlayHandle_GetVolume(handle: NativeHandle) -> f32 {
    unsafe { get_play_handle(handle) }.map_or(0.0, |ph| ph.get_volume())
}

// ---------------------------------------------------------------------------
// Seek
// ---------------------------------------------------------------------------

/// Seek the stream to the given frame position.
///
/// Returns true on success. On failure returns false — check `UAP_HasError`
/// / `UAP_GetError` for details.
///
/// Returns false for null handles.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_PlayHandle_Seek(handle: NativeHandle, frame: u64) -> bool {
    clear_error();
    let ph = match unsafe { get_play_handle(handle) } {
        Some(ph) => ph,
        None => {
            set_error("null play handle");
            return false;
        }
    };
    failible_to_native(ph.seek(frame).map(|()| true), false)
}

// ---------------------------------------------------------------------------
// Modifier
// ---------------------------------------------------------------------------

/// C callback type for the pre-mix modifier.
///
/// Called on the audio thread with the interleaved f32 buffer for a
/// single stream. `sample_count` is the number of f32 values in the buffer.
/// Must be wait-free.
type ModifierCallback = unsafe extern "C" fn(
    buffer: *mut f32,
    sample_count: u64,
    user_data: *mut c_void,
);

/// Build a [`ModifierFn`] from a C callback + opaque user data.
///
/// Wraps both in a single struct marked `Send + Sync` so the resulting
/// closure satisfies the thread-safety bounds of `ModifierFn`.
fn make_modifier(cb: ModifierCallback, user_data: *mut c_void) -> uniasset_audioplayer::mixer::ModifierFn {
    struct Wrapper {
        cb: ModifierCallback,
        user_data: *mut c_void,
    }
    // Safety: the C caller is responsible for thread-safety of the
    // callback and the pointed-to user data.
    unsafe impl Send for Wrapper {}
    unsafe impl Sync for Wrapper {}

    impl Wrapper {
        /// Dispatch the modifier callback.
        /// The `unsafe` is internal — the C caller guarantees thread safety.
        unsafe fn call(&self, buffer: *mut f32, sample_count: u64) {
            (self.cb)(buffer, sample_count, self.user_data);
        }
    }

    let w = Wrapper { cb, user_data };
    Box::new(move |buffer: &mut [f32]| unsafe {
        w.call(buffer.as_mut_ptr(), buffer.len() as u64);
    })
}

/// Install a pre-mix modifier callback for this stream.
///
/// The modifier is called on the audio thread with the interleaved PCM
/// buffer for this stream before it is mixed into the output. The callback
/// may transform samples in-place (e.g., filtering, panning, gain effects).
///
/// Pass a null `callback` to remove any previously installed modifier.
/// `user_data` is an opaque pointer passed to the callback on each invocation.
/// It must remain valid for as long as the modifier is installed.
///
/// Returns true on success, false on error (null handle).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_PlayHandle_SetModifier(
    handle: NativeHandle,
    callback: Option<ModifierCallback>,
    user_data: *mut c_void,
) -> bool {
    clear_error();
    let ph = match unsafe { get_play_handle(handle) } {
        Some(ph) => ph,
        None => {
            set_error("null play handle");
            return false;
        }
    };

    let modifier: Option<uniasset_audioplayer::mixer::ModifierFn> = match callback {
        Some(cb) => Some(make_modifier(cb, user_data)),
        None => None,
    };

    ph.set_modifier(modifier);
    true
}
