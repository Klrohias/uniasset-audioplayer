//! WASAPI backend for Windows.
//!
//! # Architecture
//!
//! WASAPI in shared mode provides an event-driven push model — the OS signals
//! a Win32 event when the audio buffer needs filling. We bridge this to our
//! pull-based [`AudioCallback`] trait via a dedicated worker thread.
//!
//! ```text
//! [Worker thread]             WASAPI                 AudioCallback
//!      |                         |                         |
//!      |── WaitForSingleObject   |                         |
//!      |   (buffer_event)        |                         |
//!      |<─ event signaled ───────|                         |
//!      |── GetBuffer() ─────────>|                         |
//!      |                         |                         |
//!      |── pull(buffer) ─────────────────────────────────>|
//!      |<─ fill f32 samples ─────|                         |
//!      |                         |                         |
//!      |── ReleaseBuffer() ────>|                         |
//! ```
//!
//! # Device Switching
//!
//! Device switching is handled through a two-tier loop architecture:
//!
//! - **Inner loop** drives the audio playback cycle on the current device
//!   (wait for buffer event → pull from callback → write to WASAPI).
//!
//! - **Outer loop** manages device lifecycle. When the inner loop detects
//!   a device error (e.g. `AUDCLNT_E_DEVICE_INVALIDATED` from unplugging
//!   headphones), it tears down the session and re-enumerates the default
//!   render endpoint. This seamlessly migrates playback to the new device.
//!
//! A `device_changed` atomic flag lets external code (e.g. a future
//! `IMMNotificationClient` implementation) proactively trigger
//! re-enumeration.
//!
//! # Memory Safety
//!
//! - The [`AudioCallback`] trait object is heap-allocated and owned by
//!   [`WasapiDevice`]. A raw `*const dyn AudioCallback` is passed to the
//!   worker thread. The callback is dropped **only after** the worker thread
//!   has been joined, guaranteeing the pointer stays valid for the thread's
//!   entire lifetime.
//! - All COM objects use the `windows` crate's RAII wrappers — they call
//!   `Release()` on drop. Their lifetimes are tied to the worker thread's
//!   stack frames.
//! - Cross-thread signals use `Arc<AtomicBool>` — lock-free on the audio
//!   hot path.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;

use crate::error::AudioError;
use crate::hal::AudioCallback;
use crate::hal::AudioDevice;
use crate::types::AudioFormat;

// Import the COM initialization guard defined in the sibling `com` module.
use super::ensure_com_initialized;

// ── Constants ────────────────────────────────────────────────────────────

/// How long the worker thread waits on the WASAPI buffer event before
/// timing out (ms). A timeout lets us periodically check stop / pause /
/// device-change flags without busy-waiting.
const EVENT_TIMEOUT_MS: u32 = 200;

/// How long the worker sleeps between poll iterations while paused (ms).
const PAUSE_SLEEP_MS: u64 = 25;

/// Back-off delay after a device disconnect before attempting
/// re-enumeration (ms). Prevents a tight reconnect loop.
const RECONNECT_DELAY_MS: u64 = 200;

/// Maximum number of consecutive reconnect attempts before giving up.
const MAX_RECONNECT_ATTEMPTS: u32 = 30;

/// Speaker configuration masks for common channel layouts.
/// Defined locally to avoid depending on the exact naming in the `windows`
/// crate's `KernelStreaming` module.
const SPEAKER_MONO: u32 = 4; // SPEAKER_FRONT_CENTER
const SPEAKER_STEREO: u32 = 3; // SPEAKER_FRONT_LEFT | SPEAKER_FRONT_RIGHT

/// Sub-type GUID for IEEE 32-bit float PCM audio.
/// {00000003-0000-0010-8000-00AA00389B71}
fn ieee_float_guid() -> windows::core::GUID {
    windows::core::GUID::from_values(
        0x00000003,
        0x0000,
        0x0010,
        [0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71],
    )
}

// ── CallbackRef ──────────────────────────────────────────────────────────

/// RAII guard that unregisters an `IMMNotificationClient` from an
/// `IMMDeviceEnumerator` on drop.
///
/// Takes shared references so the enumerator remains accessible for
/// [`setup_wasapi_session`] calls during the outer loop.  Rust's
/// reverse drop order guarantees the guard is dropped before the
/// values it borrows — unregistration happens while both COM objects
/// are still alive.
struct UnregisterGuard<'a> {
    enumerator: &'a Option<IMMDeviceEnumerator>,
    client: &'a Option<IMMNotificationClient>,
}

impl<'a> Drop for UnregisterGuard<'a> {
    fn drop(&mut self) {
        if let (Some(ref dev_enum), Some(ref nc)) = (self.enumerator, self.client) {
            unsafe {
                let _ = dev_enum.UnregisterEndpointNotificationCallback(nc);
            }
        }
    }
}

// ── CallbackRef ──────────────────────────────────────────────────────────

/// Holds a fat pointer to the callback trait object.
///
/// Because raw pointers across thread boundaries are thin (`*mut c_void`),
/// we store the fat `*const dyn AudioCallback` inside an indirection that we
/// can safely share with the worker thread.
struct CallbackRef {
    ptr: *const dyn AudioCallback,
}

// SAFETY: `CallbackRef` is created on the main thread and only accessed
// from the worker thread via `&self` (immutable reference). The owning
// `Box<CallbackRef>` outlives the worker thread (dropped after join).
unsafe impl Send for CallbackRef {}
unsafe impl Sync for CallbackRef {}

/// RAII guard for the callback and its `CallbackRef` indirection during
/// thread spawn.  If spawning the worker thread fails, the guard frees both
/// allocations in the correct order — `CallbackRef` first (it holds a
/// pointer into the callback's allocation), then the callback itself.
/// On success, [`disarm`](CallbackGuard::disarm) extracts the owned pieces
/// so `WasapiDevice` can keep them alive.
struct CallbackGuard {
    fat_ptr: *const dyn AudioCallback,
    cb_ref: Option<Box<CallbackRef>>,
}

impl CallbackGuard {
    fn new(callback: Box<dyn AudioCallback>) -> Self {
        let fat_ptr: *const dyn AudioCallback = Box::into_raw(callback);
        let cb_ref = Box::new(CallbackRef { ptr: fat_ptr });
        Self {
            fat_ptr,
            cb_ref: Some(cb_ref),
        }
    }

    /// A reference into the guard's `CallbackRef` that can be sent to the
    /// worker thread.  Valid until the guard is dropped or disarmed.
    fn cb_ref_ptr(&self) -> *const CallbackRef {
        &**self.cb_ref.as_ref().unwrap()
    }

    /// Relinquish ownership — the caller is now responsible for freeing
    /// both the callback and the `CallbackRef`.
    fn disarm(mut self) -> (*const dyn AudioCallback, Box<CallbackRef>) {
        let cb_ref = self.cb_ref.take().unwrap();
        (self.fat_ptr, cb_ref)
    }
}

impl Drop for CallbackGuard {
    fn drop(&mut self) {
        // Drop CallbackRef first — its `ptr` points into the callback.
        if let Some(cb_ref) = self.cb_ref.take() {
            drop(cb_ref);
            // SAFETY: fat_ptr was created from Box::into_raw and hasn't been
            // freed yet (this Drop only runs on the error path where we
            // never called disarm).
            unsafe {
                drop(Box::from_raw(self.fat_ptr as *mut dyn AudioCallback));
            }
        }
    }
}

// ── WasapiSession ────────────────────────────────────────────────────────

/// A successfully initialized WASAPI playback session.
///
/// Holds the COM interfaces and the buffer event for one playback cycle on
/// a single audio endpoint. When a device change is detected this entire
/// session is torn down and re-created against the new default device.
struct WasapiSession {
    /// The WASAPI audio client.
    audio_client: IAudioClient,
    /// Interface for writing sample data into the output buffer.
    render_client: IAudioRenderClient,
    /// Auto-reset event signaled by WASAPI when the buffer needs data.
    buffer_event: HANDLE,
    /// Number of audio frames the endpoint buffer can hold.
    buffer_frame_count: u32,
    /// Number of channels in the current stream.
    channels: usize,
}

impl WasapiSession {
    /// Stop playback and reset the audio client before dropping.
    fn cleanup(self) {
        unsafe {
            let _ = self.audio_client.Stop();
            let _ = self.audio_client.Reset();
        }
        // `self` is dropped here; the event handle is closed by Drop.
    }
}

impl Drop for WasapiSession {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.buffer_event);
        }
    }
}

// ── WasapiDevice ─────────────────────────────────────────────────────────

/// An audio output device backed by WASAPI (Windows Audio Session API).
///
/// Uses a dedicated worker thread that waits on a WASAPI buffer event and
/// pulls PCM samples from an [`AudioCallback`]. Device switching (e.g.
/// unplugging headphones) is handled transparently — the worker detects
/// the change, tears down the old endpoint, and re-initializes against
/// the new default device.
pub struct WasapiDevice {
    /// Hardware format (sample rate + channels) detected at open time.
    /// Shared mode typically uses 48 kHz stereo on all endpoints, so
    /// this rarely changes across device switches.
    format: AudioFormat,

    /// Shared flag: set to `true` to signal the worker thread to stop.
    stop_flag: Arc<AtomicBool>,

    /// Shared flag: `true` = paused, `false` = running.
    pause_flag: Arc<AtomicBool>,

    /// Shared flag: set by the `IMMNotificationClient` callback when the
    /// default render device changes.
    device_changed: Arc<AtomicBool>,

    /// Handle to the worker thread. `None` until [`start`](AudioDevice::start)
    /// is called.
    worker: Option<JoinHandle<()>>,

    /// Owns the callback [`Box`]. Dropped **after** the worker thread is
    /// joined, ensuring the raw pointer inside `CallbackRef` stays valid.
    _callback_box: Option<Box<dyn AudioCallback>>,

    /// Owns the `CallbackRef` indirection.
    _callback_ref: Option<Box<CallbackRef>>,

    /// Whether the device is currently running.
    running: bool,
}

// SAFETY: WASAPI handles and the worker thread are safe to send between
// threads. The only non-Send parts are guarded by WasapiDevice's API
// which takes `&mut self`.
unsafe impl Send for WasapiDevice {}

impl WasapiDevice {
    /// Create a new WASAPI output device.
    ///
    /// Probes the default render endpoint to detect the hardware's native
    /// format (sample rate + channels). Does **not** start playback — call
    /// [`start`](AudioDevice::start) to begin.
    pub fn new() -> Result<Self, AudioError> {
        // COM must be initialized on the calling thread before we can
        // enumerate devices.
        ensure_com_initialized();

        let format = probe_format().unwrap_or(AudioFormat::new(48000, 2));

        Ok(Self {
            format,
            stop_flag: Arc::new(AtomicBool::new(false)),
            pause_flag: Arc::new(AtomicBool::new(false)),
            device_changed: Arc::new(AtomicBool::new(false)),
            worker: None,
            _callback_box: None,
            _callback_ref: None,
            running: false,
        })
    }
}

impl AudioDevice for WasapiDevice {
    fn format(&self) -> AudioFormat {
        self.format
    }

    fn start(&mut self, callback: Box<dyn AudioCallback>) -> Result<(), AudioError> {
        if self.running {
            return Ok(());
        }

        // Reset all flags for a fresh start.
        self.stop_flag.store(false, Ordering::Release);
        self.pause_flag.store(false, Ordering::Release);
        self.device_changed.store(false, Ordering::Release);

        // Convert the callback `Box` into a raw pointer via an indirection.
        // The CallbackGuard ensures both allocations are freed if thread
        // spawn fails (the guard is dropped on the error path).
        let guard = CallbackGuard::new(callback);
        let cb_ref_ptr = guard.cb_ref_ptr();

        // Clone `Arc`s for the worker thread.
        let stop_flag = Arc::clone(&self.stop_flag);
        let pause_flag = Arc::clone(&self.pause_flag);
        let device_changed = Arc::clone(&self.device_changed);

        let handle = thread::Builder::new()
            .name("uniasset-wasapi".into())
            .spawn(move || {
                worker_main(cb_ref_ptr, stop_flag, pause_flag, device_changed);
            })
            .map_err(|e| {
                AudioError::BackendError(format!("failed to spawn WASAPI worker thread: {e}"))
            })?;

        // Thread spawned successfully — disarm the guard and take ownership.
        let (fat_ptr, cb_ref) = guard.disarm();
        self.worker = Some(handle);
        self._callback_ref = Some(cb_ref);
        self._callback_box = Some(unsafe { Box::from_raw(fat_ptr as *mut dyn AudioCallback) });
        self.running = true;

        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        // Signal the worker to exit its loops.
        self.stop_flag.store(true, Ordering::Release);

        // Join the worker thread.
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }

        // Drop the callback AFTER the worker thread has stopped.
        // Order matters: drop `_callback_box` before `_callback_ref` because
        // `CallbackRef.ptr` points into the callback's allocation.
        self._callback_box = None;
        self._callback_ref = None;
        self.running = false;

        Ok(())
    }

    fn pause(&mut self) -> Result<(), AudioError> {
        self.pause_flag.store(true, Ordering::Release);
        Ok(())
    }

    fn resume(&mut self) -> Result<(), AudioError> {
        self.pause_flag.store(false, Ordering::Release);
        Ok(())
    }
}

impl Drop for WasapiDevice {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Worker Thread
// ═══════════════════════════════════════════════════════════════════════════

/// Main entry point for the WASAPI worker thread.
///
/// Runs an outer loop that handles device (re-)initialization and an inner
/// loop that drives the audio playback cycle on the current device. When a
/// device disconnect or change is detected the inner loop breaks and the
/// outer loop re-enumerates the default endpoint.
fn worker_main(
    cb_ref: *const CallbackRef,
    stop_flag: Arc<AtomicBool>,
    pause_flag: Arc<AtomicBool>,
    device_changed: Arc<AtomicBool>,
) {
    // COM must be initialized on the worker thread before any WASAPI calls.
    ensure_com_initialized();

    // ── Create & register IMMNotificationClient ──────────────────────
    // Proactive device change detection: when the default render endpoint
    // changes (e.g. headphones plugged in), the COM callback sets the
    // `device_changed` atomic flag. The inner loop checks this and
    // re-enumerates the new default device.
    let notification_client =
        create_device_change_notifier(Arc::clone(&device_changed));

    // Create the device enumerator once and reuse it for both
    // notification registration and session setup (avoids repeated
    // CoCreateInstance calls on every device switch).
    let shared_enumerator = create_device_enumerator().ok();
    if let (Some(ref dev_enum), Some(ref nc)) = (&shared_enumerator, &notification_client) {
        let _ = unsafe { dev_enum.RegisterEndpointNotificationCallback(nc) };
    }

    // RAII guard: unregisters the notification client on drop, covering
    // both normal return and panic-unwind through the inner loop.
    let _unregister_guard = UnregisterGuard {
        enumerator: &shared_enumerator,
        client: &notification_client,
    };

    let mut reconnect_attempts: u32 = 0;

    // ── Outer loop: device lifecycle ─────────────────────────────────
    'outer: loop {
        if stop_flag.load(Ordering::Acquire) {
            break;
        }

        // Try to set up WASAPI on the current default render device.
        let session = match setup_wasapi_session(shared_enumerator.as_ref()) {
            Ok(s) => {
                reconnect_attempts = 0; // Reset counter on success.
                s
            }
            Err(_) => {
                reconnect_attempts += 1;
                if reconnect_attempts > MAX_RECONNECT_ATTEMPTS {
                    break;
                }
                if stop_flag.load(Ordering::Acquire) {
                    break;
                }
                thread::sleep(Duration::from_millis(RECONNECT_DELAY_MS));
                continue;
            }
        };

        let channels = session.channels;

        // ── Inner loop: audio playback on the current device ─────────
        loop {
            // --- stop check ---
            if stop_flag.load(Ordering::Acquire) {
                session.cleanup();
                break 'outer;
            }

            // --- device change check ---
            if device_changed.swap(false, Ordering::AcqRel) {
                session.cleanup();
                continue 'outer;
            }

            // --- pause check ---
            if pause_flag.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(PAUSE_SLEEP_MS));
                continue;
            }

            // --- wait for buffer event ---
            let wait_result =
                unsafe { WaitForSingleObject(session.buffer_event, EVENT_TIMEOUT_MS) };

            match wait_result {
                WAIT_OBJECT_0 => {
                    // Buffer is ready — fill it with audio data.
                    // SAFETY: `cb_ref` outlives the worker thread
                    // (see CallbackRef documentation).
                    let cb: &dyn AudioCallback = unsafe { &*(*cb_ref).ptr };

                    if let Err(_) = fill_wasapi_buffer(
                        &session.render_client,
                        cb,
                        session.buffer_frame_count,
                        channels,
                    ) {
                        // Error filling the buffer — device likely
                        // disconnected. Tear down and re-enumerate.
                        session.cleanup();
                        continue 'outer;
                    }
                }
                WAIT_TIMEOUT => {
                    // Timeout — loop around to re-check flags.
                }
                _ => {
                    // `WaitForSingleObject` failed — device probably
                    // invalidated.
                    session.cleanup();
                    continue 'outer;
                }
            }
        }
    }

    // `_unregister_guard` drops here, unregistering the notification
    // client before the COM objects are released.
}

// ═══════════════════════════════════════════════════════════════════════════
// WASAPI Session Setup
// ═══════════════════════════════════════════════════════════════════════════

/// Create an `IMMDeviceEnumerator` instance via COM.
fn create_device_enumerator() -> Result<IMMDeviceEnumerator, AudioError> {
    unsafe {
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| {
            AudioError::BackendError(format!("CoCreateInstance(MMDeviceEnumerator): {e}"))
        })
    }
}

/// Result of activating an `IAudioClient` and reading its mix format.
struct ClientFormat {
    audio_client: IAudioClient,
    sample_rate: u32,
    channels: usize,
}

/// Activate an `IAudioClient` on the default render endpoint and read the
/// engine's mix format.  Shared by [`probe_format`] (one-shot at init) and
/// [`setup_wasapi_session`] (each device-connection cycle).
fn activate_audio_client(
    enumerator: &IMMDeviceEnumerator,
) -> Result<ClientFormat, AudioError> {
    // 1. Get the default render device (multimedia role).
    let device = unsafe {
        enumerator
            .GetDefaultAudioEndpoint(EDataFlow::eRender, ERole::eMultimedia)
            .map_err(|e| {
                AudioError::BackendError(format!("GetDefaultAudioEndpoint: {e}"))
            })?
    };

    // 2. Activate the IAudioClient interface.
    let audio_client: IAudioClient = unsafe {
        device.Activate(CLSCTX_ALL, None).map_err(|e| {
            AudioError::BackendError(format!("Activate(IAudioClient): {e}"))
        })?
    };

    // 3. Get the mix format to learn the engine's sample rate / channels.
    let mix_format_ptr = unsafe {
        audio_client
            .GetMixFormat()
            .map_err(|e| AudioError::BackendError(format!("GetMixFormat: {e}")))?
    };

    let (sample_rate, channels) = unsafe {
        let fmt = &*mix_format_ptr;
        (
            fmt.nSamplesPerSec,
            (fmt.nChannels as usize).clamp(1, 8),
        )
    };

    // The mix format buffer was allocated by WASAPI via CoTaskMemAlloc.
    unsafe {
        CoTaskMemFree(Some(mix_format_ptr as *mut std::ffi::c_void));
    }

    Ok(ClientFormat {
        audio_client,
        sample_rate,
        channels,
    })
}

/// Set up a full WASAPI playback session on the default render endpoint.
///
/// If `shared_enumerator` is provided it is reused; otherwise a fresh
/// `IMMDeviceEnumerator` is created (fallback for the rare case where the
/// shared enumerator couldn't be constructed).
///
/// Returns a [`WasapiSession`] with the audio client started and the
/// buffer event primed. On failure the caller may retry after a delay
/// (e.g. if no output device is currently available).
fn setup_wasapi_session(
    shared_enumerator: Option<&IMMDeviceEnumerator>,
) -> Result<WasapiSession, AudioError> {
    // Reuse the shared enumerator when available; create a local one as a
    // fallback (only exercised if COM was unavailable during init).
    let local_enum;
    let enumerator: &IMMDeviceEnumerator = if let Some(e) = shared_enumerator {
        e
    } else {
        local_enum = create_device_enumerator()?;
        &local_enum
    };

    let ClientFormat {
        audio_client,
        sample_rate,
        channels,
    } = activate_audio_client(enumerator)?;

    // Build a WAVEFORMATEXTENSIBLE for 32-bit float at the engine rate.
    let wave_fmt = build_wave_format(sample_rate, channels as u16);

    // 4. Initialize the audio client in shared mode, event-driven.
    // hnsBufferDuration and hnsPeriodicity MUST be 0 in event-driven mode.
    unsafe {
        audio_client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                0,
                0,
                &wave_fmt as *const _ as *const WAVEFORMATEX,
                None,
            )
            .map_err(|e| {
                AudioError::BackendError(format!("IAudioClient::Initialize: {e}"))
            })?
    };

    // 5. Query the buffer size (in audio frames).
    let buffer_frame_count = unsafe {
        audio_client
            .GetBufferSize()
            .map_err(|e| AudioError::BackendError(format!("GetBufferSize: {e}")))?
    };

    // 6. Obtain the render client for writing sample data.
    let render_client: IAudioRenderClient = unsafe {
        audio_client
            .GetService()
            .map_err(|e| {
                AudioError::BackendError(format!("GetService(IAudioRenderClient): {e}"))
            })?
    };

    // 7. Create an auto-reset event for buffer-available notifications.
    let buffer_event = unsafe {
        CreateEventW(None, false, false, None)
            .map_err(|e| AudioError::BackendError(format!("CreateEventW: {e}")))?
    };

    // 8. Associate the event with the audio client.
    unsafe {
        audio_client
            .SetEventHandle(buffer_event)
            .map_err(|e| AudioError::BackendError(format!("SetEventHandle: {e}")))?
    };

    // 9. Start the audio stream.
    unsafe {
        audio_client
            .Start()
            .map_err(|e| AudioError::BackendError(format!("IAudioClient::Start: {e}")))?
    };

    Ok(WasapiSession {
        audio_client,
        render_client,
        buffer_event,
        buffer_frame_count,
        channels,
    })
}

// ═══════════════════════════════════════════════════════════════════════════
// Audio Buffer I/O
// ═══════════════════════════════════════════════════════════════════════════

/// Fill the WASAPI output buffer by pulling PCM samples from the callback.
///
/// # Safety
///
/// The caller must ensure that `render_client` is valid and that `callback`
/// outlives this call.
///
/// Returns an error if any WASAPI call fails (typically indicates a
/// disconnected device).
unsafe fn fill_wasapi_buffer(
    render_client: &IAudioRenderClient,
    callback: &dyn AudioCallback,
    frame_count: u32,
    channels: usize,
) -> Result<(), AudioError> {
    // Obtain a write pointer to the next chunk of the output buffer.
    let buffer: *mut u8 = render_client
        .GetBuffer(frame_count)
        .map_err(|e| AudioError::BackendError(format!("GetBuffer: {e}")))?;

    let sample_count = frame_count as usize * channels;
    let samples = unsafe { std::slice::from_raw_parts_mut(buffer as *mut f32, sample_count) };

    // Pull PCM samples from the callback — `&self`, zero locks.
    let frames_written = callback.pull(samples);

    // Clamp to the actual frame count to guard against a buggy / malicious
    // callback that returns more frames than the buffer can hold, which would
    // cause either an out-of-bounds slice panic or a `usize` overflow in the
    // `frames_written * channels` multiplication below.
    let frames_written = frames_written.min(frame_count as usize);

    // Zero-fill any samples the callback didn't write.
    let written_samples = frames_written * channels;
    if written_samples < sample_count {
        samples[written_samples..].fill(0.0);
    }

    // Release the buffer back to WASAPI for playback.
    render_client
        .ReleaseBuffer(frame_count, 0)
        .map_err(|e| AudioError::BackendError(format!("ReleaseBuffer: {e}")))?;

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════
// WAVEFORMATEXTENSIBLE Helper
// ═══════════════════════════════════════════════════════════════════════════

/// Build a [`WAVEFORMATEXTENSIBLE`] describing 32-bit float PCM at the
/// given sample rate and channel count. This is the optimal format for
/// WASAPI shared mode — sample-accurate and zero driver resampling on
/// modern Windows audio stacks.
fn build_wave_format(sample_rate: u32, channels: u16) -> WAVEFORMATEXTENSIBLE {
    let channels_u32 = channels as u32;
    let block_align = channels_u32 * 4; // 4 bytes per f32 sample
    let bytes_per_sec = sample_rate * block_align;

    // Channel mask for common configurations.
    let channel_mask = match channels {
        1 => SPEAKER_MONO,
        2 => SPEAKER_STEREO,
        // For > 2 channels we pass 0 — the audio engine infers the layout
        // from the channel count.
        _ => 0,
    };

    // SAFETY: `zeroed()` is valid for `WAVEFORMATEXTENSIBLE` (all bit-patterns
    // are valid for its fields, and the struct has no invalid states).
    let mut fmt: WAVEFORMATEXTENSIBLE = unsafe { std::mem::zeroed() };

    fmt.Format = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_EXTENSIBLE as u16,
        nChannels: channels,
        nSamplesPerSec: sample_rate,
        nAvgBytesPerSec: bytes_per_sec,
        nBlockAlign: block_align as u16,
        wBitsPerSample: 32,
        cbSize: 22, // sizeof(WAVEFORMATEXTENSIBLE) - sizeof(WAVEFORMATEX)
    };
    // Set 32 valid bits per sample inside the Samples union.
    fmt.Samples.Anonymous.wValidBitsPerSample = 32;
    fmt.dwChannelMask = channel_mask;
    fmt.SubFormat = ieee_float_guid();

    fmt
}

// ═══════════════════════════════════════════════════════════════════════════
// Device Format Probe
// ═══════════════════════════════════════════════════════════════════════════

/// Query the default render endpoint's mix format to determine the native
/// sample rate and channel count. Used at device construction time so the
/// caller can configure the mixer before starting playback.
fn probe_format() -> Result<AudioFormat, AudioError> {
    let enumerator = create_device_enumerator()?;
    let fmt = activate_audio_client(&enumerator)?;
    // `fmt.audio_client` is dropped here — we only needed the mix format.
    Ok(AudioFormat::new(fmt.sample_rate, fmt.channels as u16))
}

// ═══════════════════════════════════════════════════════════════════════════
// IMMNotificationClient — Device Change Detection
// ═══════════════════════════════════════════════════════════════════════════

/// Creates a COM object implementing `IMMNotificationClient`.
///
/// When the default render device changes, the callback sets the shared
/// `device_changed` atomic flag. The worker thread checks this flag every
/// iteration of its inner loop and re-initializes the WASAPI session when
/// it sees the flag go high.
fn create_device_change_notifier(
    device_changed: Arc<AtomicBool>,
) -> Option<IMMNotificationClient> {
    // The `#[implement]` macro from the `windows` crate generates the
    // COM vtable and reference-counting boilerplate.
    #[windows::core::implement(Windows::Win32::Media::Audio::IMMNotificationClient)]
    struct NotificationClient {
        device_changed: Arc<AtomicBool>,
    }

    impl NotificationClient {
        /// Returns `true` when the notification is for a render (output)
        /// device in the default multimedia role — the only change we
        /// care about for an audio player.
        fn is_relevant(flow: EDataFlow, role: ERole) -> bool {
            flow == EDataFlow::eRender && role == ERole::eMultimedia
        }
    }

    impl IMMNotificationClient_Impl for NotificationClient_Impl {
        fn OnDeviceStateChanged(
            &self,
            _pwstrdeviceid: &PWSTR,
            _pdwnewstate: *const u32,
        ) -> Result<()> {
            Ok(())
        }

        fn OnDeviceAdded(&self, _pwstrdeviceid: &PWSTR) -> Result<()> {
            Ok(())
        }

        fn OnDeviceRemoved(&self, _pwstrdeviceid: &PWSTR) -> Result<()> {
            Ok(())
        }

        fn OnDefaultDeviceChanged(
            &self,
            flow: EDataFlow,
            role: ERole,
            _pwstrdefaultdeviceid: &PWSTR,
        ) -> Result<()> {
            if NotificationClient::is_relevant(flow, role) {
                self.device_changed.store(true, Ordering::Release);
            }
            Ok(())
        }

        fn OnPropertyValueChanged(
            &self,
            _pwstrdeviceid: &PWSTR,
            _key: *const PROPERTYKEY,
        ) -> Result<()> {
            Ok(())
        }
    }

    let client: IMMNotificationClient = NotificationClient { device_changed }.into();
    Some(client)
}
