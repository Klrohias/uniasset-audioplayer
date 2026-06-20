//! WASAPI backend for Windows.
//!
//! Uses `IAudioClient` in shared mode with event-driven buffer filling.
//! A dedicated thread waits on the buffer-event handle and calls
//! [`AudioCallback::pull`] to fill each buffer.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::error::AudioError;
use crate::hal::AudioCallback;
use crate::hal::AudioDevice;
use crate::types::AudioFormat;

/// An audio output device backed by WASAPI (`IAudioClient`).
pub struct WasapiDevice {
    format: AudioFormat,
    /// Handle to stop the audio thread.
    stop_tx: Option<mpsc::Sender<()>>,
    /// Join handle for the audio thread.
    thread_handle: Option<thread::JoinHandle<()>>,
    running: bool,
}

// Safety: The WASAPI thread handle is Send.
unsafe impl Send for WasapiDevice {}

impl WasapiDevice {
    /// Create a new WASAPI output device for the requested format.
    pub fn new(format: AudioFormat) -> Result<Self, AudioError> {
        // TODO: Initialize COM (`CoInitializeEx`), create `IMMDeviceEnumerator`,
        // get the default audio render endpoint, activate `IAudioClient` in
        // shared mode, query buffer size, and create the event handle.

        Ok(Self {
            format,
            stop_tx: None,
            thread_handle: None,
            running: false,
        })
    }
}

impl AudioDevice for WasapiDevice {
    fn start(&mut self, callback: Box<dyn AudioCallback>) -> Result<(), AudioError> {
        if self.running {
            return Ok(());
        }

        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let format = self.format;

        // Spawn the audio thread that waits on the WASAPI buffer event.
        let handle = thread::Builder::new()
            .name("uniasset-wasapi".into())
            .spawn(move || {
                wasapi_thread(callback, format, stop_rx);
            })
            .map_err(|e| AudioError::BackendError(format!("failed to spawn audio thread: {e}")))?;

        self.stop_tx = Some(stop_tx);
        self.thread_handle = Some(handle);
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
        self.running = false;
        Ok(())
    }

    fn pause(&mut self) -> Result<(), AudioError> {
        // TODO: Call IAudioClient::Stop().
        Ok(())
    }

    fn resume(&mut self) -> Result<(), AudioError> {
        // TODO: Call IAudioClient::Start().
        Ok(())
    }
}

impl Drop for WasapiDevice {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// The audio thread function.
///
/// TODO (full WASAPI impl):
/// 1. CoInitializeEx
/// 2. IMMDeviceEnumerator::GetDefaultAudioEndpoint
/// 3. IMMDevice::Activate(IAudioClient)
/// 4. IAudioClient::Initialize(AUDCLNT_SHAREMODE_SHARED, ...)
/// 5. IAudioClient::GetBufferSize
/// 6. IAudioClient::SetEventHandle
/// 7. IAudioRenderClient::GetService
/// 8. IAudioClient::Start
/// 9. Loop: WaitForSingleObject → IAudioRenderClient::GetBuffer →
///    callback.pull() → IAudioRenderClient::ReleaseBuffer
/// 10. IAudioClient::Stop on exit
fn wasapi_thread(
    _callback: Box<dyn AudioCallback>,
    _format: AudioFormat,
    stop_rx: mpsc::Receiver<()>,
) {
    // Skeleton: wait for stop signal.
    // In a real implementation, we'd loop with a timeout on the event handle
    // and check stop_rx periodically.
    loop {
        if stop_rx.recv_timeout(Duration::from_millis(10)).is_ok() {
            break;
        }
    }
}
