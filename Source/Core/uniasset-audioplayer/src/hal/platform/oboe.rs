//! Oboe backend for Android.
//!
//! Uses the Oboe (AAudio) native audio library with a data callback
//! that pulls from [`AudioCallback::pull`].
//!
//! # Architecture
//!
//! ```text
//! AudioCallback::pull()  ←  OboeCallback::on_audio_ready()  ←  Oboe Audio Thread
//! ```
//!
//! The stream is created in [`OboeDevice::new`] with a placeholder (null)
//! callback pointer. The real callback is wired in
//! [`AudioDevice::start`](crate::hal::AudioDevice::start) via a
//! [`CallbackPtr`] indirection — the same pattern used by the CoreAudio
//! backend's `CallbackRef`.

use std::cell::UnsafeCell;
use std::panic::{catch_unwind, AssertUnwindSafe};

use oboe::{
    AudioOutputCallback, AudioOutputStream, AudioStream, AudioStreamAsync, AudioStreamBase,
    AudioStreamBuilder, ContentType, DataCallbackResult, Output, PerformanceMode, SharingMode,
    Stereo, Usage,
};

use crate::error::AudioError;
use crate::hal::AudioCallback;
use crate::hal::AudioDevice;
use crate::types::AudioFormat;

/// Default sample rate fallback: 48 kHz.
const DEFAULT_SAMPLE_RATE: i32 = 48000;

/// Default channel count fallback: stereo.
const DEFAULT_CHANNEL_COUNT: u16 = 2;

/// Dummy callback used solely to obtain a valid vtable for constructing
/// a null `*const dyn AudioCallback` fat pointer.
struct NullCallbackStub;
impl AudioCallback for NullCallbackStub {
    fn pull(&self, _buffer: &mut [f32]) -> usize {
        0
    }
}

/// Create a null fat pointer for `*const dyn AudioCallback`.
///
/// Recent Rust requires the [`Thin`] trait for [`std::ptr::null`], which
/// trait objects don't satisfy. We construct a fat pointer with a null
/// data address and a valid (non-null) vtable borrowed from a dummy callback.
///
/// `is_null_callback` checks only the data-pointer portion.
fn null_callback() -> *const dyn AudioCallback {
    let dummy: &dyn AudioCallback = &NullCallbackStub;
    // Decompose the fat pointer into (data_ptr, vtable_ptr), replace
    // data_ptr with null, then recombine. On all Rust targets, a trait
    // object pointer is laid out as (data, vtable).
    let (_, vtable) =
        unsafe { core::mem::transmute::<*const dyn AudioCallback, (*const (), *const ())>(dummy) };
    unsafe {
        core::mem::transmute::<(*const (), *const ()), *const dyn AudioCallback>((
            core::ptr::null(),
            vtable,
        ))
    }
}

/// Check whether a `*const dyn AudioCallback` fat pointer is null by
/// extracting only the data-pointer portion.
fn is_null_callback(ptr: *const dyn AudioCallback) -> bool {
    // Casting a fat pointer to a thin pointer discards the vtable metadata
    // and keeps only the data address.
    (ptr as *const ()).is_null()
}

// ── CallbackPtr ─────────────────────────────────────────────────────────

/// Stores a callback trait-object pointer with interior mutability.
///
/// Written once in [`OboeDevice::start`] (happens-before the audio thread
/// is started), then read lock-free by the Oboe audio callback.
struct CallbackPtr(UnsafeCell<*const dyn AudioCallback>);

// Safety: the pointer is written once before the stream starts (happens-before
// all audio callback invocations). After that it is read-only from the audio
// thread. No concurrent read/write access.
unsafe impl Send for CallbackPtr {}
unsafe impl Sync for CallbackPtr {}

impl CallbackPtr {
    fn new() -> Self {
        Self(UnsafeCell::new(null_callback()))
    }

    fn set(&self, ptr: *const dyn AudioCallback) {
        unsafe {
            *self.0.get() = ptr;
        }
    }

    fn get(&self) -> *const dyn AudioCallback {
        unsafe { *self.0.get() }
    }
}

// ── OboeCallback ────────────────────────────────────────────────────────

/// Bridges Oboe's [`AudioOutputCallback`] to our pull-based [`AudioCallback`].
///
/// Invoked on Oboe's high-priority audio thread. Reads the callback pointer
/// via [`CallbackPtr`] for lock-free access — the same pattern as CoreAudio's
/// `render_callback`.
struct OboeCallback {
    /// Points to a device-owned [`CallbackPtr`] that holds the fat pointer
    /// to the application's `dyn AudioCallback`.
    callback_ptr: *const CallbackPtr,
    /// Number of channels in the stream (determined at open time).
    channel_count: usize,
}

// Safety: OboeCallback is moved into the stream and only accessed from
// the audio thread. The raw pointer outlives the stream because the
// device owns the CallbackPtr (declared after `stream` in OboeDevice).
unsafe impl Send for OboeCallback {}

impl AudioOutputCallback for OboeCallback {
    type FrameType = (f32, Stereo);

    fn on_audio_ready(
        &mut self,
        _stream: &mut dyn oboe::AudioOutputStreamSafe,
        audio_data: &mut [(f32, f32)],
    ) -> DataCallbackResult {
        // Cast [(f32, f32)] stereo-frame slice to interleaved [f32] once.
        // (f32, f32) and [f32; 2] share the same in-memory layout on all
        // Rust targets: two consecutive f32 values matching interleaved PCM.
        //
        // Safety: the total byte length is preserved (audio_data.len() * 8
        // bytes → interleaved.len() * 4 bytes, same total).
        let interleaved: &mut [f32] = unsafe {
            std::slice::from_raw_parts_mut(
                audio_data.as_mut_ptr() as *mut f32,
                audio_data.len() * 2,
            )
        };

        // Resolve the callback through the indirection.
        if self.callback_ptr.is_null() {
            interleaved.fill(0.0);
            return DataCallbackResult::Continue;
        }

        let callback_ptr: &CallbackPtr = unsafe { &*self.callback_ptr };
        let cb_ptr: *const dyn AudioCallback = callback_ptr.get();

        if is_null_callback(cb_ptr) {
            interleaved.fill(0.0);
            return DataCallbackResult::Continue;
        }

        // Capture by-value copies for the catch_unwind closure so we don't
        // capture `&mut self` (which would require AssertUnwindSafe).
        let channel_count = self.channel_count;

        // Wrap the callback call in catch_unwind so a panic in user code
        // doesn't unwind through the C AAudio stack frames (UB).
        let result = catch_unwind(AssertUnwindSafe(|| {
            // Safety: cb_ptr is valid for the lifetime of the device, which
            // outlives the stream. AudioCallback::pull takes `&self` — no
            // locks needed.
            let cb: &dyn AudioCallback = unsafe { &*cb_ptr };
            let frames_written = cb.pull(interleaved);

            // Zero-fill remaining samples if the callback wrote fewer frames.
            let written_samples = frames_written * channel_count;
            if written_samples < interleaved.len() {
                interleaved[written_samples..].fill(0.0);
            }
        }));

        // If the callback panicked, zero-fill the entire buffer so the
        // audio output is silent rather than playing uninitialized memory.
        if result.is_err() {
            interleaved.fill(0.0);
        }

        DataCallbackResult::Continue
    }
}

// ── OboeDevice ──────────────────────────────────────────────────────────

/// An audio output device backed by Oboe (AAudio).
///
/// Opens a low-latency stereo output stream with 32-bit float samples.
/// The stream is created in [`new`](OboeDevice::new) and the callback is
/// wired up in [`start`](AudioDevice::start).
///
/// # Field Drop Order
///
/// Fields are dropped in declaration order. `stream` must drop before
/// `_callback_box` and `callback_ptr` to ensure the audio thread has
/// stopped invoking the callback before we free its data.
pub struct OboeDevice {
    /// Hardware format detected at open time.
    format: AudioFormat,
    /// The Oboe audio stream (async, callback-driven).
    stream: Option<AudioStreamAsync<Output, OboeCallback>>,
    /// Indirection for the callback pointer. Written in `start()`, read
    /// by the Oboe audio callback lock-free.
    callback_ptr: Option<Box<CallbackPtr>>,
    /// Owns the callback Box for the lifetime of the device.
    _callback_box: Option<Box<dyn AudioCallback>>,
    running: bool,
}

// Safety: Oboe stream handles are safe to send between threads.
unsafe impl Send for OboeDevice {}

impl OboeDevice {
    /// Create a new Oboe output device.
    ///
    /// Opens a low-latency stereo output stream with 32-bit float samples.
    /// The actual hardware sample rate and channel count are queried after
    /// opening and exposed via [`AudioDevice::format`].
    ///
    /// The stream is opened with a placeholder (null) callback — the real
    /// callback is wired in [`start`](AudioDevice::start).
    pub fn new() -> Result<Self, AudioError> {
        // Shared indirection for the callback pointer (see CallbackPtr docs).
        let callback_ptr = Box::new(CallbackPtr::new());
        let callback_ptr_raw: *const CallbackPtr = &*callback_ptr;

        let channel_count = DEFAULT_CHANNEL_COUNT as usize;

        let oboe_callback = OboeCallback {
            callback_ptr: callback_ptr_raw,
            channel_count,
        };

        // Build and open a low-latency stereo output stream.
        let stream: AudioStreamAsync<Output, OboeCallback> = AudioStreamBuilder::default()
            .set_performance_mode(PerformanceMode::LowLatency)
            .set_sharing_mode(SharingMode::Shared)
            .set_usage(Usage::Media)
            .set_content_type(ContentType::Music)
            .set_format::<f32>()
            .set_channel_count::<Stereo>()
            .set_sample_rate(DEFAULT_SAMPLE_RATE)
            .set_callback(oboe_callback)
            .open_stream()
            .map_err(|e| AudioError::BackendError(format!("failed to open oboe stream: {e}")))?;

        // Query the actual hardware format from the opened stream.
        // We requested f32 stereo, so the channel count is known.
        // Only the sample rate may be adjusted by the system.
        let sample_rate = stream.get_sample_rate().max(1) as u32;
        let format = AudioFormat::new(sample_rate, DEFAULT_CHANNEL_COUNT);

        Ok(Self {
            format,
            stream: Some(stream),
            callback_ptr: Some(callback_ptr),
            _callback_box: None,
            running: false,
        })
    }
}

impl AudioDevice for OboeDevice {
    fn format(&self) -> AudioFormat {
        self.format
    }

    fn start(&mut self, callback: Box<dyn AudioCallback>) -> Result<(), AudioError> {
        let stream = self.stream.as_mut().ok_or(AudioError::DeviceNotFound)?;

        if self.running {
            return Ok(());
        }

        // Convert the callback Box to a raw fat pointer and store it
        // through the CallbackPtr indirection for lock-free audio-thread access.
        let fat_ptr: *const dyn AudioCallback = Box::into_raw(callback);
        if let Some(ref cb_ptr) = self.callback_ptr {
            cb_ptr.set(fat_ptr);
        }

        // Start the stream. The audio callback will begin firing immediately.
        stream
            .start_with_timeout(oboe::DEFAULT_TIMEOUT_NANOS)
            .map_err(|e| {
                // Clean up on failure: clear the pointer and drop the callback.
                if let Some(ref cb_ptr) = self.callback_ptr {
                    cb_ptr.set(null_callback());
                }
                unsafe {
                    drop(Box::from_raw(fat_ptr as *mut dyn AudioCallback));
                }
                AudioError::BackendError(format!("failed to start oboe stream: {e}"))
            })?;

        // Reconstruct the Box so Drop can reclaim it later.
        // Safety: fat_ptr was created from Box::into_raw just above.
        self._callback_box = Some(unsafe { Box::from_raw(fat_ptr as *mut dyn AudioCallback) });
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        if let Some(ref mut stream) = self.stream {
            if self.running {
                // Stop the stream, then close it.
                let _ = stream.stop_with_timeout(oboe::DEFAULT_TIMEOUT_NANOS);
                let _ = stream.close();
            }
        }

        // Clear the callback pointer so the audio thread won't access
        // freed memory if a spurious callback fires during teardown.
        if let Some(ref cb_ptr) = self.callback_ptr {
            cb_ptr.set(null_callback());
        }

        self.running = false;
        self._callback_box = None;
        Ok(())
    }

    fn pause(&mut self) -> Result<(), AudioError> {
        if let Some(ref mut stream) = self.stream {
            if self.running {
                stream
                    .pause_with_timeout(oboe::DEFAULT_TIMEOUT_NANOS)
                    .map_err(|e| {
                        AudioError::BackendError(format!("failed to pause oboe stream: {e}"))
                    })?;
            }
        }
        Ok(())
    }

    fn resume(&mut self) -> Result<(), AudioError> {
        if let Some(ref mut stream) = self.stream {
            if self.running {
                stream
                    .start_with_timeout(oboe::DEFAULT_TIMEOUT_NANOS)
                    .map_err(|e| {
                        AudioError::BackendError(format!("failed to resume oboe stream: {e}"))
                    })?;
            }
        }
        Ok(())
    }
}

impl Drop for OboeDevice {
    fn drop(&mut self) {
        let _ = self.stop();
        // Ensure callbacks are dropped before the stream (fields drop in order).
        self._callback_box = None;
        self.callback_ptr = None;
    }
}
