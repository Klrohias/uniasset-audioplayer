use std::{io, sync::Arc};

use crate::{
    mixer::AudioStream,
    stream::buffered_stream::worker::{acquire_worker_handle, AudioBuffer, WorkerHandle},
    AudioError,
};

use super::BUFFER_WATERMARK;

/// A stream wrapper that maintains a 4-second ring buffer filled ahead of
/// time by a background worker thread.
///
/// The audio thread reads from the ring buffer (wait-free) while the worker
/// thread keeps the buffer above [`BUFFER_WATERMARK`] by reading ahead from
/// `inner`.
pub struct BufferedAudioStream {
    inner: Arc<dyn AudioStream>,
    worker_handle: WorkerHandle,
    buffer: Arc<AudioBuffer>,
}

impl BufferedAudioStream {
    /// Wrap `inner` with a 4-second ring buffer and register it with the
    /// background worker.
    pub fn new(inner: Arc<dyn AudioStream>) -> Result<Self, io::Error> {
        let worker_handle = acquire_worker_handle()?;

        // 12-second ring buffer: sample_rate × channels × 12
        let one_second_samples = inner.sample_rate() * inner.channels() as u32;
        let buffer: Arc<AudioBuffer> = AudioBuffer::new(one_second_samples as usize * 12).into();

        worker_handle.add_buffer_group(inner.clone(), buffer.clone());
        worker_handle.notify();

        Ok(Self {
            inner,
            worker_handle,
            buffer,
        })
    }
}

impl Drop for BufferedAudioStream {
    fn drop(&mut self) {
        self.worker_handle.remove_buffer_group(&self.inner);
    }
}

impl AudioStream for BufferedAudioStream {
    /// Read interleaved `f32` samples from the ring buffer.
    ///
    /// Samples that cannot be satisfied from the buffer are filled with
    /// silence (zeros). If after reading the fill level drops below
    /// [`BUFFER_WATERMARK`], the worker thread is notified to top up.
    ///
    /// Always writes exactly `buffer.len()` samples — wait-free.
    fn read(&self, buffer: &mut [f32], _frame_count: u64) -> usize {
        let n = self.buffer.read(buffer);

        // Zero-fill any shortfall (buffer underrun = silence).
        if n < buffer.len() {
            buffer[n..].fill(0.0);
        }

        // Wake the worker if we dipped below the watermark.
        if self.buffer.fill_level() < BUFFER_WATERMARK {
            self.worker_handle.notify();
        }

        buffer.len()
    }

    /// Seek the inner stream to `frame` and discard buffered data so the
    /// worker refills from the new position.
    fn seek(&self, frame: u64) -> Result<(), AudioError> {
        self.inner.seek(frame)?;
        self.buffer.reset();
        self.worker_handle.notify();
        Ok(())
    }

    /// Returns `true` when the inner stream has ended **and** the ring buffer
    /// has been fully consumed (no more data to deliver).
    fn is_eof(&self) -> bool {
        self.inner.is_eof() && self.buffer.available() == 0
    }

    fn channels(&self) -> u16 {
        self.inner.channels()
    }

    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }
}
