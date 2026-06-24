//! OHOS (OpenHarmony) audio backend.
//!
//! Uses the OHAudio C API (`OH_AudioRenderer`) with a write-data callback
//! that pulls from [`AudioCallback::pull`].
//!
//! # Architecture
//!
//! ```text
//! AudioCallback::pull()  <-  write_data_callback()  <-  OH_AudioRenderer Audio Thread
//! ```
//!
//! The renderer is created in [`OhosDevice::new`] via the builder pattern.
//! The real callback is wired in
//! [`AudioDevice::start`](crate::hal::AudioDevice::start) via a
//! [`CallbackRef`] indirection — the same pattern used by the CoreAudio
//! and Oboe backends.

use std::cell::UnsafeCell;
use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};

use ohos_audio_sys::*;

use crate::error::AudioError;
use crate::hal::AudioCallback;
use crate::hal::AudioDevice;
use crate::types::AudioFormat;

/// Default sample rate fallback: 48 kHz.
const DEFAULT_SAMPLE_RATE: u32 = 48000;

/// Default channel count fallback: stereo.
const DEFAULT_CHANNEL_COUNT: u16 = 2;

// ── CallbackRef ─────────────────────────────────────────────────────────

/// Stores a callback trait-object pointer with interior mutability and the
/// channel count needed for byte-to-frame conversion in the callback.
///
/// The pointer is written once in [`OhosDevice::start`] (happens-before the
/// renderer is started), then read lock-free by the OHOS write-data callback.
struct CallbackRef {
    ptr: UnsafeCell<*const dyn AudioCallback>,
    channel_count: usize,
}

// Safety: `ptr` is written once before the renderer starts (happens-before
// all audio callback invocations). After that it is read-only from the audio
// thread. No concurrent read/write access.
unsafe impl Send for CallbackRef {}
unsafe impl Sync for CallbackRef {}

impl CallbackRef {
    fn new(channel_count: usize) -> Self {
        Self {
            ptr: UnsafeCell::new(std::ptr::null()),
            channel_count,
        }
    }

    fn set(&self, ptr: *const dyn AudioCallback) {
        unsafe {
            *self.ptr.get() = ptr;
        }
    }

    fn get(&self) -> *const dyn AudioCallback {
        unsafe { *self.ptr.get() }
    }
}

// ── Write-data callback trampoline (C ABI) ──────────────────────────────

/// The C-callable write-data callback installed on the OH_AudioRenderer.
///
/// Bridges from the OHOS write-data callback signature to our Rust
/// [`AudioCallback::pull`] method. **Lock-free**: the callback is
/// accessed via `&self` through a raw `*const dyn AudioCallback`.
///
/// The `buffer_len` parameter is in bytes. Each `f32` sample is 4 bytes.
extern "C" fn write_data_callback(
    _renderer: *mut OH_AudioRenderer,
    user_data: *mut c_void,
    buffer: *mut c_void,
    buffer_len: i32,
) -> i32 {
    // Null / invalid-argument guards.
    if user_data.is_null() || buffer.is_null() || buffer_len <= 0 {
        return AUDIOSTREAM_SUCCESS;
    }

    let cb_ref: &CallbackRef = unsafe { &*(user_data as *const CallbackRef) };
    let channel_count = cb_ref.channel_count;

    let sample_count = (buffer_len as usize) / 4;
    if sample_count == 0 {
        return AUDIOSTREAM_SUCCESS;
    }
    let frame_count = sample_count / channel_count;
    if frame_count == 0 {
        return AUDIOSTREAM_SUCCESS;
    }

    // Safety: buffer_len bytes of writable memory provided by OHOS.
    let output = unsafe { std::slice::from_raw_parts_mut(buffer as *mut f32, sample_count) };

    // Guard against null callback pointer (before start() or after stop()).
    let cb_ptr: *const dyn AudioCallback = cb_ref.get();
    if cb_ptr.is_null() {
        output.fill(0.0);
        return AUDIOSTREAM_SUCCESS;
    }

    // Wrap the callback call in catch_unwind so a panic in user code
    // doesn't unwind through the C OHOS stack frames (UB).
    let result = catch_unwind(AssertUnwindSafe(|| {
        // Safety: cb_ptr is valid for the lifetime of the device, which
        // outlives the renderer. AudioCallback::pull takes `&self` — no
        // locks needed.
        let cb: &dyn AudioCallback = unsafe { &*cb_ptr };
        cb.pull(output)
    }));

    match result {
        Ok(frames_written) => {
            // Clamp to the actual frame count to guard against a buggy
            // callback returning more frames than the buffer can hold.
            let frames_written = frames_written.min(frame_count);
            // Zero-fill remaining samples if callback wrote fewer frames.
            let written_samples = frames_written * channel_count;
            if written_samples < sample_count {
                output[written_samples..].fill(0.0);
            }
        }
        Err(_) => {
            // Callback panicked — zero-fill the entire buffer rather than
            // playing uninitialized memory.
            output.fill(0.0);
        }
    }

    AUDIOSTREAM_SUCCESS
}

// ── OhosDevice ──────────────────────────────────────────────────────────

/// An audio output device backed by OHAudio (OpenHarmony).
///
/// Creates a stereo output stream with 32-bit float samples using the
/// [`OH_AudioRenderer`] API. The stream is created in
/// [`new`](OhosDevice::new) and the callback is wired up in
/// [`start`](AudioDevice::start).
///
/// # Field Drop Order
///
/// Fields are dropped in declaration order. `renderer` must be stopped and
/// released before `_callback_box` and `_callback_ref` are dropped to ensure
/// the audio thread has stopped invoking the callback before we free its data.
pub struct OhosDevice {
    /// Hardware format detected at open time.
    format: AudioFormat,
    /// The OH_AudioRenderer handle (audio stream).
    renderer: Option<OH_AudioRenderer>,
    /// Indirection for the callback pointer. Created in `new()`, passed as
    /// OHOS userData, and the real pointer is set in `start()`.
    _callback_ref: Option<Box<CallbackRef>>,
    /// Owns the callback Box for the lifetime of the device.
    _callback_box: Option<Box<dyn AudioCallback>>,
    running: bool,
}

// Safety: OH_AudioRenderer handles are safe to send between threads.
unsafe impl Send for OhosDevice {}

impl OhosDevice {
    /// Create a new OHOS output device.
    ///
    /// Opens a stereo output stream with 32-bit float samples at 48 kHz
    /// using the OHAudio C API. The actual hardware sample rate and channel
    /// count are queried after opening and exposed via
    /// [`AudioDevice::format`].
    ///
    /// The stream is created with a placeholder (null) callback — the real
    /// callback is wired in [`start`](AudioDevice::start).
    pub fn new() -> Result<Self, AudioError> {
        let channel_count = DEFAULT_CHANNEL_COUNT as usize;

        // Create the callback indirection (with null ptr) that will be
        // registered as the OHOS userData. The real pointer is set in start().
        let cb_ref = Box::new(CallbackRef::new(channel_count));
        let user_data = Box::into_raw(cb_ref) as *mut c_void;

        // Create the stream builder.
        let mut builder: OH_AudioStreamBuilder = std::ptr::null_mut();
        let ret = unsafe { OH_AudioStreamBuilder_Create(&mut builder) };
        if ret != AUDIOSTREAM_SUCCESS || builder.is_null() {
            unsafe {
                drop(Box::from_raw(user_data as *mut CallbackRef));
            }
            return Err(AudioError::DeviceNotFound);
        }

        // Configure the stream for music playback at 48 kHz stereo f32.
        let ret =
            unsafe { OH_AudioStreamBuilder_SetRendererInfo(builder, AUDIOSTREAM_USAGE_MUSIC) };
        if ret != AUDIOSTREAM_SUCCESS {
            unsafe {
                OH_AudioStreamBuilder_Destroy(builder);
                drop(Box::from_raw(user_data as *mut CallbackRef));
            }
            return Err(AudioError::BackendError(format!(
                "failed to set renderer info: {ret}"
            )));
        }

        let ret =
            unsafe { OH_AudioStreamBuilder_SetLatencyMode(builder, AUDIOSTREAM_LATENCY_MODE_FAST) };
        if ret != AUDIOSTREAM_SUCCESS {
            unsafe {
                OH_AudioStreamBuilder_Destroy(builder);
                drop(Box::from_raw(user_data as *mut CallbackRef));
            }
            return Err(AudioError::BackendError(format!(
                "failed to set latency mode: {ret}"
            )));
        }

        let ret =
            unsafe { OH_AudioStreamBuilder_SetSamplingRate(builder, DEFAULT_SAMPLE_RATE as i32) };
        if ret != AUDIOSTREAM_SUCCESS {
            unsafe {
                OH_AudioStreamBuilder_Destroy(builder);
                drop(Box::from_raw(user_data as *mut CallbackRef));
            }
            return Err(AudioError::BackendError(format!(
                "failed to set sampling rate: {ret}"
            )));
        }

        let ret =
            unsafe { OH_AudioStreamBuilder_SetChannelCount(builder, DEFAULT_CHANNEL_COUNT as i32) };
        if ret != AUDIOSTREAM_SUCCESS {
            unsafe {
                OH_AudioStreamBuilder_Destroy(builder);
                drop(Box::from_raw(user_data as *mut CallbackRef));
            }
            return Err(AudioError::BackendError(format!(
                "failed to set channel count: {ret}"
            )));
        }

        let ret = unsafe {
            OH_AudioStreamBuilder_SetSampleFormat(builder, AUDIOSTREAM_SAMPLE_FORMAT_F32LE)
        };
        if ret != AUDIOSTREAM_SUCCESS {
            unsafe {
                OH_AudioStreamBuilder_Destroy(builder);
                drop(Box::from_raw(user_data as *mut CallbackRef));
            }
            return Err(AudioError::BackendError(format!(
                "failed to set sample format: {ret}"
            )));
        }

        let ret = unsafe {
            OH_AudioStreamBuilder_SetEncodingType(builder, AUDIOSTREAM_ENCODING_TYPE_RAW)
        };
        if ret != AUDIOSTREAM_SUCCESS {
            unsafe {
                OH_AudioStreamBuilder_Destroy(builder);
                drop(Box::from_raw(user_data as *mut CallbackRef));
            }
            return Err(AudioError::BackendError(format!(
                "failed to set encoding type: {ret}"
            )));
        }

        // Register the write-data callback with our CallbackRef as userData.
        let ret = unsafe {
            OH_AudioStreamBuilder_SetRendererWriteDataCallback(
                builder,
                Some(write_data_callback),
                user_data,
            )
        };
        if ret != AUDIOSTREAM_SUCCESS {
            unsafe {
                OH_AudioStreamBuilder_Destroy(builder);
                drop(Box::from_raw(user_data as *mut CallbackRef));
            }
            return Err(AudioError::BackendError(format!(
                "failed to set write-data callback: {ret}"
            )));
        }

        // Generate the renderer from the builder.
        let mut renderer: OH_AudioRenderer = std::ptr::null_mut();
        let ret = unsafe { OH_AudioStreamBuilder_GenerateRenderer(builder, &mut renderer) };
        // Builder is consumed; always destroy it.
        unsafe {
            OH_AudioStreamBuilder_Destroy(builder);
        }
        if ret != AUDIOSTREAM_SUCCESS || renderer.is_null() {
            unsafe {
                drop(Box::from_raw(user_data as *mut CallbackRef));
            }
            return Err(AudioError::DeviceBusy);
        }

        // Reconstruct the CallbackRef Box from its raw pointer so we own it.
        let cb_ref = unsafe { Box::from_raw(user_data as *mut CallbackRef) };

        // Query the actual hardware format from the renderer.
        // Fall back to defaults if the query fails.
        let mut sample_rate: i32 = DEFAULT_SAMPLE_RATE as i32;
        let ret = unsafe { OH_AudioRenderer_GetSamplingRate(renderer, &mut sample_rate) };
        if ret != AUDIOSTREAM_SUCCESS {
            sample_rate = DEFAULT_SAMPLE_RATE as i32;
        }

        let mut actual_channels: i32 = DEFAULT_CHANNEL_COUNT as i32;
        let ret = unsafe { OH_AudioRenderer_GetChannelCount(renderer, &mut actual_channels) };
        if ret != AUDIOSTREAM_SUCCESS {
            actual_channels = DEFAULT_CHANNEL_COUNT as i32;
        }

        // Update the callback's channel count to match the actual hardware.
        if actual_channels > 0 {
            cb_ref.channel_count = actual_channels as usize;
        }

        let format = AudioFormat::new(sample_rate.max(1) as u32, actual_channels.max(1) as u16);

        Ok(Self {
            format,
            renderer: Some(renderer),
            _callback_ref: Some(cb_ref),
            _callback_box: None,
            running: false,
        })
    }
}

impl AudioDevice for OhosDevice {
    fn format(&self) -> AudioFormat {
        self.format
    }

    fn start(&mut self, callback: Box<dyn AudioCallback>) -> Result<(), AudioError> {
        let renderer = self.renderer.as_ref().ok_or(AudioError::DeviceNotFound)?;

        if self.running {
            return Ok(());
        }

        // Convert the callback Box to a raw fat pointer and store it
        // through the CallbackRef indirection for lock-free audio-thread access.
        let fat_ptr: *const dyn AudioCallback = Box::into_raw(callback);

        // Store the fat pointer in the CallbackRef (OHOS reads it in the callback).
        if let Some(ref cb_ref) = self._callback_ref {
            cb_ref.set(fat_ptr);
        }

        // Start the renderer. The audio callback will begin firing immediately.
        let ret = unsafe { OH_AudioRenderer_Start(*renderer) };
        if ret != AUDIOSTREAM_SUCCESS {
            // Clean up on failure: clear the pointer and drop the callback.
            if let Some(ref cb_ref) = self._callback_ref {
                cb_ref.set(std::ptr::null());
            }
            unsafe {
                drop(Box::from_raw(fat_ptr as *mut dyn AudioCallback));
            }
            return Err(AudioError::BackendError(format!(
                "failed to start OHOS renderer: {ret}"
            )));
        }

        // Reconstruct the Box so Drop can reclaim it later.
        // Safety: fat_ptr was created from Box::into_raw just above.
        self._callback_box = Some(unsafe { Box::from_raw(fat_ptr as *mut dyn AudioCallback) });
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        if let Some(renderer) = self.renderer.as_ref() {
            if self.running {
                unsafe {
                    OH_AudioRenderer_Stop(*renderer);
                    OH_AudioRenderer_Release(*renderer);
                }
            }
        }

        // Clear the callback pointer so the audio thread won't access
        // freed memory if a spurious callback fires during teardown.
        if let Some(ref cb_ref) = self._callback_ref {
            cb_ref.set(std::ptr::null());
        }

        self.running = false;
        self._callback_box = None;
        self.renderer = None;
        Ok(())
    }

    fn pause(&mut self) -> Result<(), AudioError> {
        if let Some(renderer) = self.renderer.as_ref() {
            if self.running {
                let ret = unsafe { OH_AudioRenderer_Pause(*renderer) };
                if ret != AUDIOSTREAM_SUCCESS {
                    return Err(AudioError::BackendError(format!(
                        "failed to pause OHOS renderer: {ret}"
                    )));
                }
            }
        }
        Ok(())
    }

    fn resume(&mut self) -> Result<(), AudioError> {
        if let Some(renderer) = self.renderer.as_ref() {
            if self.running {
                let ret = unsafe { OH_AudioRenderer_Start(*renderer) };
                if ret != AUDIOSTREAM_SUCCESS {
                    return Err(AudioError::BackendError(format!(
                        "failed to resume OHOS renderer: {ret}"
                    )));
                }
            }
        }
        Ok(())
    }
}

impl Drop for OhosDevice {
    fn drop(&mut self) {
        let _ = self.stop();
        // Ensure callbacks are dropped in the correct order. The renderer
        // must be released before the callback data is freed.
        self._callback_box = None;
        self._callback_ref = None;
    }
}
