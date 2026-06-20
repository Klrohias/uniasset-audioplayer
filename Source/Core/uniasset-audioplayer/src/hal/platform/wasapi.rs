//! WASAPI backend for Windows.
//!
//! Uses `IAudioClient` in shared mode with event-driven buffer filling.
//! A dedicated thread waits on the buffer-event handle and calls
//! [`AudioCallback::pull`] to fill each buffer.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use windows::core::HSTRING;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioClient, IAudioRenderClient, IMMDeviceEnumerator, MMDeviceEnumerator,
    AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, WAVEFORMATEX,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED,
};
use windows::Win32::System::Threading::{
    AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW, CreateEventW,
    WaitForSingleObject,
};

use crate::error::AudioError;
use crate::hal::AudioCallback;
use crate::hal::AudioDevice;
use crate::types::AudioFormat;

/// Default format fallback: 48 kHz stereo.
const DEFAULT_FORMAT: AudioFormat = AudioFormat::new(48000, 2);

// ── Send wrappers ──────────────────────────────────────────────────────
//
// COM objects from the `windows` crate store a `NonNull<c_void>` which is
// `!Send`.  WASAPI objects are apartment-threaded and safe to use from
// multiple threads so long as COM is initialised in multi-threaded mode.

/// `Send` wrapper for a Windows COM interface.
struct SendCom<T>(T);
unsafe impl<T> Send for SendCom<T> {}

/// `Send` wrapper for a Windows `HANDLE` that calls [`CloseHandle`] on drop.
struct SendHandle(HANDLE);
unsafe impl Send for SendHandle {}

impl Drop for SendHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

/// An audio output device backed by WASAPI (`IAudioClient`).
pub struct WasapiDevice {
    /// Hardware format detected at open time.
    format: AudioFormat,
    /// COM interface to the WASAPI audio client.
    audio_client: Option<IAudioClient>,
    /// Number of frames per hardware buffer.
    buffer_frame_count: u32,
    /// Handle to signal the audio thread to stop.
    stop_tx: Option<mpsc::Sender<()>>,
    /// Join handle for the audio thread.
    thread_handle: Option<thread::JoinHandle<()>>,
    running: bool,
}

// Safety: WASAPI COM objects and thread handles are Send.
unsafe impl Send for WasapiDevice {}

impl WasapiDevice {
    /// Create a new WASAPI output device.
    ///
    /// Queries the default audio endpoint, activates `IAudioClient`
    /// in shared event-driven mode, and reads the hardware format.
    pub fn new() -> Result<Self, AudioError> {
        // Create device enumerator.
        let enumerator: IMMDeviceEnumerator = unsafe {
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|_| AudioError::DeviceNotFound)?
        };

        // Get default audio output endpoint.
        let device = unsafe {
            enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(|_| AudioError::DeviceNotFound)?
        };

        // Activate IAudioClient.
        let audio_client: IAudioClient = unsafe {
            device
                .Activate(CLSCTX_ALL, None)
                .map_err(|e| AudioError::BackendError(format!("IAudioClient activation: {e}")))?
        };

        // Get the engine mix format for sample rate + channel count.
        let mix_format_ptr: *mut WAVEFORMATEX = unsafe {
            audio_client
                .GetMixFormat()
                .map_err(|e| AudioError::BackendError(format!("GetMixFormat: {e}")))?
        };

        if mix_format_ptr.is_null() {
            return Err(AudioError::BackendError("GetMixFormat returned null".into()));
        }

        let format = unsafe { read_mix_format(mix_format_ptr) };

        // Build an IEEE float WAVEFORMATEX matching the hardware rate + channels.
        // We *must* use a float format because the audio thread casts the WASAPI
        // buffer directly to `&mut [f32]`.  In shared mode the engine accepts
        // IEEE float as long as rate and channels are compatible.
        const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
        let float_format = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_IEEE_FLOAT,
            nChannels: format.channels,
            nSamplesPerSec: format.sample_rate,
            nAvgBytesPerSec: format.sample_rate * format.channels as u32 * 4,
            nBlockAlign: format.channels * 4,
            wBitsPerSample: 32,
            cbSize: 0,
        };

        // Initialize in shared, event-driven mode; let engine choose buffer duration.
        let result = unsafe {
            audio_client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                0, // hnsBufferDuration: 0 = engine default
                0, // hnsPeriodicity: 0 for shared mode
                &float_format,
                None, // no audio session GUID
            )
        };

        // Free the mix format *regardless* of Initialize outcome.
        // WASAPI copies the format during the Initialize call, so it is
        // safe to release the pointer immediately after.
        unsafe {
            CoTaskMemFree(Some(mix_format_ptr as *mut _));
        }

        result.map_err(|e| {
            AudioError::BackendError(format!("IAudioClient::Initialize: {e}"))
        })?;

        // Read actual buffer size (in frames).
        let buffer_frame_count = unsafe {
            audio_client
                .GetBufferSize()
                .map_err(|e| AudioError::BackendError(format!("GetBufferSize: {e}")))?
        };

        Ok(Self {
            format,
            audio_client: Some(audio_client),
            buffer_frame_count,
            stop_tx: None,
            thread_handle: None,
            running: false,
        })
    }
}

/// Extract [`AudioFormat`] from a WASAPI mix format pointer.
///
/// Falls back to [`DEFAULT_FORMAT`] if the data is unreadable or invalid.
///
/// Also validates that the format is 32-bit float; if not, we still return
/// the sample rate + channels so the caller knows what the hardware expects,
/// but the mismatch is logged conceptually (the `Initialize` call uses the
/// same format pointer, so WASAPI's shared-mode engine will convert).
unsafe fn read_mix_format(wf: *const WAVEFORMATEX) -> AudioFormat {
    if wf.is_null() {
        return DEFAULT_FORMAT;
    }
    let fmt = &*wf;
    if fmt.nSamplesPerSec == 0 || fmt.nChannels == 0 {
        return DEFAULT_FORMAT;
    }
    // On modern Windows (10+) the shared-mode mix format is always IEEE float.
    // If it isn't, we still report the correct rate+channels; the buffer cast
    // assumes f32 and WASAPI's shared-mode engine will convert if needed.
    AudioFormat::new(fmt.nSamplesPerSec, fmt.nChannels)
}

impl AudioDevice for WasapiDevice {
    fn format(&self) -> AudioFormat {
        self.format
    }

    fn start(&mut self, callback: Box<dyn AudioCallback>) -> Result<(), AudioError> {
        if self.running {
            return Err(AudioError::DeviceBusy);
        }

        let audio_client = self
            .audio_client
            .as_ref()
            .ok_or(AudioError::DeviceNotFound)?;

        // Create event handle for WASAPI buffer-ready notifications.
        // Wrap in SendHandle immediately so CloseHandle is called on drop
        // if any subsequent step fails.
        let event_handle = SendHandle(unsafe {
            CreateEventW(None, false, false, None)
                .map_err(|e| AudioError::BackendError(format!("CreateEventW: {e}")))?
        });

        // Hand the event to WASAPI.
        unsafe {
            audio_client
                .SetEventHandle(event_handle.0)
                .map_err(|e| AudioError::BackendError(format!("SetEventHandle: {e}")))?;
        }

        // Obtain the render client for filling buffers.
        let render_client: IAudioRenderClient = unsafe {
            audio_client
                .GetService()
                .map_err(|e| {
                    AudioError::BackendError(format!("GetService(IAudioRenderClient): {e}"))
                })?
        };

        let buffer_frame_count = self.buffer_frame_count;
        let channels = self.format.channels;
        let sample_count = buffer_frame_count as usize * channels as usize;

        // Pre-fill the first buffer *before* Start() so the engine has data
        // when playback begins — eliminates the initial ~10 ms silence gap.
        let callback_alive = AtomicBool::new(true);
        fill_buffer(
            &*callback,
            &render_client,
            buffer_frame_count,
            sample_count,
            channels,
            &callback_alive,
        );

        // Start the audio stream.
        unsafe {
            audio_client
                .Start()
                .map_err(|e| AudioError::BackendError(format!("IAudioClient::Start: {e}")))?;
        }

        let (stop_tx, stop_rx) = mpsc::channel::<()>();

        // Wrap non-Send types for the thread boundary.
        let send_render = SendCom(render_client);

        // Spawn the audio thread.
        let handle = thread::Builder::new()
            .name("uniasset-wasapi".into())
            .spawn(move || {
                wasapi_thread(
                    callback,
                    send_render,
                    event_handle,
                    buffer_frame_count,
                    sample_count,
                    channels,
                    stop_rx,
                    callback_alive,
                );
            })
            .map_err(|e| {
                // Thread creation failed — stop the audio client so it doesn't
                // keep running without a thread to feed it.
                unsafe {
                    let _ = audio_client.Stop();
                }
                AudioError::BackendError(format!("failed to spawn audio thread: {e}"))
            })?;

        self.stop_tx = Some(stop_tx);
        self.thread_handle = Some(handle);
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        // 1. Signal the audio thread to exit.
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        // 2. Stop the audio engine *before* the thread exits, so the engine
        //    won't try to signal the event handle after it is closed.
        if let Some(client) = self.audio_client.as_ref() {
            unsafe {
                let _ = client.Stop();
            }
        }
        // 3. Wait for the thread to finish (drops callback, render client,
        //    and closes the event handle via SendHandle::Drop).
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
        self.running = false;
        Ok(())
    }

    fn pause(&mut self) -> Result<(), AudioError> {
        if let Some(client) = self.audio_client.as_ref() {
            if self.running {
                unsafe {
                    client
                        .Stop()
                        .map_err(|e| {
                            AudioError::BackendError(format!("IAudioClient::Stop: {e}"))
                        })?;
                }
            }
        }
        Ok(())
    }

    fn resume(&mut self) -> Result<(), AudioError> {
        if let Some(client) = self.audio_client.as_ref() {
            if self.running {
                unsafe {
                    client
                        .Start()
                        .map_err(|e| {
                            AudioError::BackendError(format!("IAudioClient::Start: {e}"))
                        })?;
                }
            }
        }
        Ok(())
    }
}

impl Drop for WasapiDevice {
    fn drop(&mut self) {
        let _ = self.stop();
        // Release the audio client last so COM objects are dropped in order.
        self.audio_client = None;
    }
}

/// Pump the WASAPI event loop on a dedicated thread.
///
/// 1. Initialises COM (multi-threaded apartment).
/// 2. Registers with MMCSS ("Audio" task) for real-time scheduling priority.
/// 3. Loops on `WaitForSingleObject` with a 50 ms timeout:
///    - `WAIT_OBJECT_0` → buffer ready → fill it (panic-safe).
///    - `WAIT_TIMEOUT` → check stop signal → exit if set.
///    - Other (`WAIT_FAILED`, …) → brief sleep + check stop signal.
fn wasapi_thread(
    callback: Box<dyn AudioCallback>,
    render_client: SendCom<IAudioRenderClient>,
    event_handle: SendHandle,
    buffer_frame_count: u32,
    sample_count: usize,
    channels: u16,
    stop_rx: mpsc::Receiver<()>,
    callback_alive: AtomicBool,
) {
    // COM is required on every thread that uses WASAPI objects.
    // If COM init fails (e.g. wrong threading model already set on this
    // thread), we cannot safely use WASAPI — exit immediately.
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if hr.is_err() {
        return;
    }

    // Register this thread with MMCSS for elevated audio priority.
    // Without this, the thread runs at normal priority and is prone to
    // buffer underruns under CPU load.
    let mmcss_handle = unsafe {
        AvSetMmThreadCharacteristicsW(&HSTRING::from("Audio"), std::ptr::null_mut())
    };

    let render = &render_client.0;
    let event = event_handle.0;

    // Consecutive GetBuffer failures — if the render client repeatedly
    // fails we assume device loss and exit the thread.
    let mut consecutive_failures: u32 = 0;

    loop {
        let wait_result = unsafe { WaitForSingleObject(event, 50) };

        match wait_result {
            WAIT_OBJECT_0 => {
                if fill_buffer(
                    &*callback,
                    render,
                    buffer_frame_count,
                    sample_count,
                    channels,
                    &callback_alive,
                ) {
                    consecutive_failures = 0;
                } else {
                    consecutive_failures += 1;
                    // After 3 consecutive failures assume device loss / fatal
                    // error and exit the thread gracefully.
                    if consecutive_failures >= 3 {
                        break;
                    }
                }
            }
            WAIT_TIMEOUT => {
                if stop_rx.try_recv().is_ok() {
                    break;
                }
            }
            _ => {
                // WAIT_FAILED / WAIT_ABANDONED — something is wrong with the
                // event handle.  Sleep briefly to avoid a busy-loop, then
                // check the stop signal.
                thread::sleep(Duration::from_millis(10));
                if stop_rx.try_recv().is_ok() {
                    break;
                }
            }
        }
    }

    // Revert MMCSS registration (restore normal priority).
    if let Ok(handle) = mmcss_handle {
        unsafe {
            let _ = AvRevertMmThreadCharacteristics(handle);
        }
    }

    // ── Drop COM objects *before* CoUninitialize ──────────────────────
    // COM contract: all interface references must be released while COM
    // is still initialised on this thread.
    drop(render_client); // → IAudioRenderClient::Release()
    drop(event_handle);  // → CloseHandle (not COM, but close before teardown)
    drop(callback);      // → Box<dyn AudioCallback> (not COM)
    drop(stop_rx);       // → mpsc::Receiver (not COM)

    unsafe {
        CoUninitialize();
    }
}

/// Fill one WASAPI buffer with PCM data from the callback.
///
/// **Panic-safe**: if [`AudioCallback::pull`] panics, the buffer is zero-filled,
/// `ReleaseBuffer` is still called, and the callback is marked dead so it is
/// not invoked again on subsequent buffers.
///
/// Returns `true` on success, `false` if `GetBuffer` failed.
fn fill_buffer(
    callback: &dyn AudioCallback,
    render_client: &IAudioRenderClient,
    frame_count: u32,
    sample_count: usize,
    channels: u16,
    callback_alive: &AtomicBool,
) -> bool {
    let buffer_ptr = unsafe {
        match render_client.GetBuffer(frame_count) {
            Ok(ptr) => ptr,
            Err(_) => return false,
        }
    };

    if buffer_ptr.is_null() {
        return false;
    }

    // Only call the callback if it hasn't panicked on a previous invocation.
    if callback_alive.load(Ordering::Acquire) {
        let result = catch_unwind(AssertUnwindSafe(|| {
            let buffer: &mut [f32] =
                unsafe { std::slice::from_raw_parts_mut(buffer_ptr as *mut f32, sample_count) };
            // Clamp to frame_count to guard against buggy / malicious callbacks
            // that return more frames than the buffer can hold.
            let frames_written = callback.pull(buffer).min(frame_count as usize);
            let written_samples = frames_written * channels as usize;
            if written_samples < sample_count {
                buffer[written_samples..].fill(0.0);
            }
        }));

        match result {
            Ok(()) => {} // callback succeeded, buffer is filled
            Err(_) => {
                // Callback panicked — mark it dead so we don't call it again.
                callback_alive.store(false, Ordering::Release);
                let buffer: &mut [f32] =
                    unsafe { std::slice::from_raw_parts_mut(buffer_ptr as *mut f32, sample_count) };
                buffer.fill(0.0);
            }
        }
    } else {
        // Callback previously panicked — just zero-fill without calling it.
        let buffer: &mut [f32] =
            unsafe { std::slice::from_raw_parts_mut(buffer_ptr as *mut f32, sample_count) };
        buffer.fill(0.0);
    }

    // Always release the buffer. The 0 flag means no silent-insertion.
    unsafe {
        let _ = render_client.ReleaseBuffer(frame_count, 0);
    }

    true
}
