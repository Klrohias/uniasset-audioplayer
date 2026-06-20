//! Dummy audio device for unsupported platforms.
//!
//! This device accepts a callback and calls `pull()` in a dedicated thread
//! at approximately the requested sample rate, discarding the output. It
//! is useful for testing the pipeline on platforms without a real audio
//! backend.

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::error::AudioError;
use crate::hal::AudioCallback;
use crate::hal::AudioDevice;
use crate::types::AudioFormat;

/// How often the dummy device calls `pull()` (every ~10 ms = 100 Hz).
const PULL_INTERVAL_MS: u64 = 10;

/// A no-op audio device that pulls samples on a timer thread.
pub struct DummyDevice {
    format: AudioFormat,
    stop_tx: Option<mpsc::Sender<()>>,
    thread_handle: Option<thread::JoinHandle<()>>,
    running: bool,
}

unsafe impl Send for DummyDevice {}

impl DummyDevice {
    /// Create a new dummy device.
    pub fn new(format: AudioFormat) -> Self {
        Self {
            format,
            stop_tx: None,
            thread_handle: None,
            running: false,
        }
    }
}

impl AudioDevice for DummyDevice {
    fn start(&mut self, mut callback: Box<dyn AudioCallback>) -> Result<(), AudioError> {
        if self.running {
            return Ok(());
        }

        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let format = self.format;
        let interval = Duration::from_millis(PULL_INTERVAL_MS);

        // Compute how many frames to pull per tick to match the sample rate.
        let frames_per_tick =
            (format.sample_rate as u64 * PULL_INTERVAL_MS / 1000) as usize;
        let samples_per_tick = frames_per_tick * format.channels as usize;

        let handle = thread::Builder::new()
            .name("uniasset-dummy".into())
            .spawn(move || {
                let mut buf = vec![0.0f32; samples_per_tick];
                let mut next_tick = Instant::now();

                loop {
                    // Check for stop signal.
                    if stop_rx.try_recv().is_ok() {
                        break;
                    }

                    // Pull samples from the callback (&self — lock-free).
                    callback.pull(&mut buf);

                    // Sleep to maintain the target pull rate.
                    next_tick += interval;
                    let now = Instant::now();
                    if next_tick > now {
                        thread::sleep(next_tick - now);
                    } else {
                        // We fell behind; reset the tick.
                        next_tick = now + interval;
                    }
                }
            })
            .map_err(|e| AudioError::BackendError(format!("failed to spawn dummy thread: {e}")))?;

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
        // Pause by stopping the pull thread.
        self.stop()
    }

    fn resume(&mut self) -> Result<(), AudioError> {
        // Resume is not meaningful for the dummy device since the callback
        // is moved into the thread. Consumers should call start() again.
        Ok(())
    }
}

impl Drop for DummyDevice {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
