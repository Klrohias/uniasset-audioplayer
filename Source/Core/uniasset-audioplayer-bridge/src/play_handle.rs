//! C FFI functions for per-stream playback control.
//!
//! All functions operate on a `NativeHandle` that wraps a
//! `Box<Arc<PlayHandle>>`.

use std::ffi::c_void;
use std::mem::ManuallyDrop;

use std::ptr;
use std::sync::Arc;

use uniasset_audioplayer::mixer::{ModifierFn, PlayHandle};

use crate::error::clear_error;
use crate::object::{failible_to_native, NativeHandle, NativeHandleExts};

pub type PlayHandleWrapper = Box<Arc<PlayHandle>>;

// ---------------------------------------------------------------------------
// Ownership
// ---------------------------------------------------------------------------

/// Destroy a `PlayHandle`.
///
/// Drops the C caller's reference. The mixer holds its own references
/// independently; the underlying stream continues playing until it
/// reaches EOF or [`UAP_PlayHandle_Stop`] is called.
///
/// # Safety
/// `handle` must be a valid handle from [`UAP_AudioPlayer_AddStream`] or
/// [`UAP_AudioPlayer_AddNativeStream`] and must not have been destroyed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_PlayHandle_Destroy(handle: NativeHandle) {
    clear_error();
    drop(PlayHandleWrapper::from_handle(handle));
}

// ---------------------------------------------------------------------------
// Playback control
// ---------------------------------------------------------------------------

/// Pause playback for this stream.
///
/// No-op if the stream is no longer alive.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_PlayHandle_Pause(handle: NativeHandle) {
    clear_error();

    let wrapper = ManuallyDrop::new(PlayHandleWrapper::from_handle(handle));
    wrapper.pause();
}

/// Resume playback for this stream.
///
/// No-op if the stream is no longer alive.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_PlayHandle_Resume(handle: NativeHandle) {
    clear_error();

    let wrapper = ManuallyDrop::new(PlayHandleWrapper::from_handle(handle));
    wrapper.resume();
}

/// Returns true if the stream is currently paused.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_PlayHandle_IsPaused(handle: NativeHandle) -> u8 {
    clear_error();

    let wrapper = ManuallyDrop::new(PlayHandleWrapper::from_handle(handle));
    if wrapper.is_paused() {
        1
    } else {
        0
    }
}

/// Returns true if the stream is still active in the mixer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_PlayHandle_IsAlive(handle: NativeHandle) -> u8 {
    clear_error();

    let wrapper = ManuallyDrop::new(PlayHandleWrapper::from_handle(handle));
    if wrapper.is_alive() {
        1
    } else {
        0
    }
}

/// Signal the stream to stop. The mixer will remove it from the
/// active stream set once it observes the stop signal.
///
/// No-op if the stream is no longer alive.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_PlayHandle_Stop(handle: NativeHandle) {
    clear_error();

    let wrapper = ManuallyDrop::new(PlayHandleWrapper::from_handle(handle));
    wrapper.stop();
}

// ---------------------------------------------------------------------------
// Volume
// ---------------------------------------------------------------------------

/// Set the volume for this stream. `volume` is clamped to `[0.0, 1.0]`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_PlayHandle_SetVolume(handle: NativeHandle, volume: f32) {
    clear_error();

    let wrapper = ManuallyDrop::new(PlayHandleWrapper::from_handle(handle));
    wrapper.set_volume(volume);
}

/// Return the current volume in `[0.0, 1.0]`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_PlayHandle_GetVolume(handle: NativeHandle) -> f32 {
    clear_error();

    let wrapper = ManuallyDrop::new(PlayHandleWrapper::from_handle(handle));
    wrapper.get_volume()
}

// ---------------------------------------------------------------------------
// Seek
// ---------------------------------------------------------------------------

/// Seek the stream to the given frame position.
///
/// On failure, the error is reported via [`UAP_HasError`] / [`UAP_GetError`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_PlayHandle_Seek(handle: NativeHandle, frame: u64) {
    clear_error();

    let wrapper = ManuallyDrop::new(PlayHandleWrapper::from_handle(handle));
    failible_to_native(|| wrapper.seek(frame), || ())
}

// ---------------------------------------------------------------------------
// Modifier
// ---------------------------------------------------------------------------

/// C callback type for the pre-mix modifier.
///
/// Called on the audio thread with the interleaved f32 buffer for a
/// single stream. `sample_count` is the number of f32 values in the buffer.
/// Must be wait-free.
type NativeModifierFn =
    unsafe extern "C" fn(buffer: *mut f32, sample_count: u64, user_data: *mut c_void);

#[repr(C)]
pub struct NativeModifier {
    cb: NativeModifierFn,
    user_data: *mut c_void,
}

/// Build a [`ModifierFn`] from a C callback + opaque user data.
///
/// Wraps both in a single struct marked `Send + Sync` so the resulting
/// closure satisfies the thread-safety bounds of `ModifierFn`.
fn make_modifier(modifier: *const NativeModifier) -> ModifierFn {
    struct Wrapper(NativeModifier);
    // Safety: the C caller is responsible for thread-safety of the
    // callback and the pointed-to user data.
    unsafe impl Send for Wrapper {}
    unsafe impl Sync for Wrapper {}

    impl Wrapper {
        /// Dispatch the modifier callback.
        /// The `unsafe` is internal — the C caller guarantees thread safety.
        unsafe fn call(&self, buffer: *mut f32, sample_count: u64) {
            (self.0.cb)(buffer, sample_count, self.0.user_data);
        }
    }

    let w = Wrapper(unsafe { ptr::read(modifier) });
    Box::new(move |buffer: &mut [f32]| unsafe {
        w.call(buffer.as_mut_ptr(), buffer.len() as u64);
    })
}

/// Install a pre-mix modifier callback for this stream.
///
/// `modifier` points to a [`NativeModifier`] containing the callback
/// function pointer and opaque user data. The modifier is called on the
/// audio thread with the interleaved PCM buffer for this stream before
/// it is mixed into the output.
///
/// The callback must be wait-free (no locks, allocations, or blocking I/O).
/// `user_data` must remain valid for as long as the modifier is installed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_PlayHandle_SetModifier(
    handle: NativeHandle,
    modifier: *const NativeModifier,
) {
    clear_error();
    let wrapper = ManuallyDrop::new(PlayHandleWrapper::from_handle(handle));
    if modifier.is_null() {
        wrapper.set_modifier(None);
    } else {
        wrapper.set_modifier(Some(make_modifier(modifier)));
    }
}
