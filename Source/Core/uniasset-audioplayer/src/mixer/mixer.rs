use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicPtr, AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;

use crate::hal::AudioCallback;
use crate::mixer::{AudioStream, PlayHandle, StreamState};
use crate::AudioFormat;

/// A stream entry in the control-thread's live list.
struct MixerEntry {
    stream: Arc<dyn AudioStream>,
    state: Arc<StreamState>,
}

/// A frozen snapshot of all streams for lock-free audio thread access.
struct MixerSnapshot {
    entries: Vec<SnapshotEntry>,
}

/// One stream entry inside a snapshot. All per-stream mutable data is
/// behind atomics so the audio thread can read/write without locks.
struct SnapshotEntry {
    stream: Arc<dyn AudioStream>,
    state: Arc<StreamState>,

    /// Cached stream channel count.
    stream_channels: u16,

    /// Resample ratio: `stream_rate / mixer_rate`.
    /// > 1.0 → stream has higher rate; < 1.0 → stream has lower rate.
    ratio: f64,

    /// Current resample position as `f64::to_bits()`.
    /// Only the audio thread writes this.
    position: AtomicU64,
}

/// Scratch buffers reused across audio callbacks to avoid per-call
/// allocation on the hot path.
struct ScratchBuf {
    /// Temporary buffer for reading from one stream.
    stream_buf: Vec<f32>,
    /// Accumulation buffer for mixing all streams together.
    mix_buf: Vec<f32>,
}

/// A lock-free audio mixer that combines multiple [`AudioStream`]s into
/// a single output.
///
/// The mixer implements [`AudioCallback`] so it can be passed directly to
/// [`AudioDevice::start`](crate::hal::AudioDevice::start).
///
/// # Features
///
/// - **Snapshot-based mixing**: audio thread reads a frozen snapshot via
///   atomic pointer swap — zero locks on the hot path.
/// - **Resampling**: streams with different sample rates are resampled
///   (linear interpolation) to match the mixer's target rate.
/// - **Channel adaptation**: extra channels are trimmed; missing channels
///   are filled by repeating the available channels.
/// - **EOF cleanup**: streams that return 0 from `read()` are flagged
///   for removal; call [`cleanup_eof`](Mixer::cleanup_eof) periodically
///   from the control thread.
///
/// # Example
///
/// ```ignore
/// use uniasset::mixer::{Mixer, AudioStream};
///
/// let mixer = Mixer::new(48000, 2);
/// let stream: Arc<dyn AudioStream> = ...;
/// let handle = mixer.add_stream(stream);
/// handle.set_volume(0.5);
/// ```
pub struct Mixer {
    /// Target output format (usually matches hardware).
    format: AudioFormat,

    /// Atomic pointer to the current read-only snapshot.
    snapshot: AtomicPtr<MixerSnapshot>,

    /// Live stream list — only accessed from the control thread.
    /// Protected by a mutex; uncontended (only the control thread writes,
    /// and only during add/remove/cleanup).
    entries: Mutex<Vec<MixerEntry>>,

    /// Scratch buffers for the audio thread.
    /// Wrapped in `UnsafeCell` — only the audio thread accesses this.
    /// Pre-allocated capacity removes allocation from the hot path.
    scratch: UnsafeCell<ScratchBuf>,

    /// Previous snapshot pending free. Freed on the next swap so the audio
    /// thread has time to finish any in-flight `pull()`.
    pending_free: Mutex<Option<*mut MixerSnapshot>>,
}

// Safety: Mixer is used as an AudioCallback, which requires Send + Sync.
// All mutable state is behind atomics, UnsafeCell (single-thread access),
// or Mutex (control-thread-only).
unsafe impl Send for Mixer {}
unsafe impl Sync for Mixer {}

impl Mixer {
    /// Create a new mixer with the given target format.
    ///
    /// `sample_rate` and `channels` should match the hardware format
    /// reported by [`AudioDevice::format`](crate::hal::AudioDevice::format).
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        let format = AudioFormat::new(sample_rate, channels);

        // Create an empty initial snapshot so the audio thread never
        // sees a null pointer.
        let initial = Box::new(MixerSnapshot {
            entries: Vec::new(),
        });

        // Pre-allocate scratch buffers for the audio hot path.
        // 8192 samples for stream_buf covers any realistic callback size
        // (256–1024 frames) with upsampling ratio (e.g. 96 kHz → 44.1 kHz).
        // 4096 samples for mix_buf covers 1024 frames × 4 channels.
        // Growth beyond these is a one-time allocation, not steady-state.
        let stream_buf = Vec::with_capacity(8192);
        let mix_buf = Vec::with_capacity(4096);

        Self {
            format,
            snapshot: AtomicPtr::new(Box::into_raw(initial)),
            entries: Mutex::new(Vec::new()),
            scratch: UnsafeCell::new(ScratchBuf {
                stream_buf,
                mix_buf,
            }),
            pending_free: Mutex::new(None),
        }
    }

    /// Return the target audio format.
    pub fn format(&self) -> AudioFormat {
        self.format
    }

    /// Number of streams currently in the mixer.
    pub fn stream_count(&self) -> usize {
        self.entries.lock().len()
    }

    /// Add an audio stream to the mixer.
    ///
    /// Returns a [`PlayHandle`] that can be used to control playback
    /// (volume, pause, seek, modifier).
    pub fn add_stream(&self, stream: Arc<dyn AudioStream>) -> PlayHandle {
        let state = Arc::new(StreamState::new());
        let handle = PlayHandle {
            state: Arc::clone(&state),
            stream: Arc::clone(&stream),
        };

        self.entries.lock().push(MixerEntry { stream, state });

        self.rebuild_snapshot();
        handle
    }

    /// Remove streams that have reached EOF.
    ///
    /// Call this periodically from the control thread (e.g., on a timer or
    /// at application idle). The audio thread only sets the `eof` flag; it
    /// never deallocates.
    pub fn cleanup_eof(&self) {
        let mut entries = self.entries.lock();
        let prev_len = entries.len();
        entries.retain(|entry| !entry.state.eof.load(Ordering::Relaxed));
        if entries.len() != prev_len {
            drop(entries);
            self.rebuild_snapshot();
        }
    }

    /// Rebuild the snapshot from the current live entry list and swap it
    /// into the atomic pointer.
    fn rebuild_snapshot(&self) {
        let mixer_rate = self.format.sample_rate as f64;

        let entries_guard = self.entries.lock();

        // Drain any pending modifier frees from the control thread.
        // Safe: the audio thread will switch to the new snapshot after
        // the swap below, so old modifier pointers are unreachable.
        for entry in entries_guard.iter() {
            let pending = entry
                .state
                .modifier_pending_free
                .swap(std::ptr::null_mut(), Ordering::AcqRel);
            if !pending.is_null() {
                unsafe { drop(Box::from_raw(pending)); }
            }
        }

        let entries: Vec<SnapshotEntry> = entries_guard
            .iter()
            .map(|entry| {
                let stream_rate = entry.stream.sample_rate();
                let ratio = stream_rate as f64 / mixer_rate;
                SnapshotEntry {
                    stream: Arc::clone(&entry.stream),
                    state: Arc::clone(&entry.state),
                    stream_channels: entry.stream.channels(),
                    ratio,
                    position: AtomicU64::new(0u64),
                }
            })
            .collect();

        drop(entries_guard);

        let new_snapshot = Box::new(MixerSnapshot { entries });
        let new_ptr = Box::into_raw(new_snapshot);

        // Swap: audio thread now sees the new snapshot.
        let old_ptr = self.snapshot.swap(new_ptr, Ordering::AcqRel);

        // Free the previous pending snapshot — enough time has passed that
        // any in-flight audio callback has completed.
        let mut pending = self.pending_free.lock();
        if let Some(ptr) = pending.take() {
            unsafe {
                drop(Box::from_raw(ptr));
            }
        }
        *pending = Some(old_ptr);
    }
}

impl AudioCallback for Mixer {
    fn pull(&self, buffer: &mut [f32]) -> usize {
        let mixer_channels = self.format.channels as usize;
        let frame_count = buffer.len() / mixer_channels;

        // Zero the output — we'll add (mix) into it.
        buffer.fill(0.0);

        // ── Load snapshot (lock-free) ──────────────────────────────────
        let snapshot_ptr = self.snapshot.load(Ordering::Acquire);
        // Safety: snapshot_ptr is never null (initialized with empty snapshot).
        let snapshot = unsafe { &*snapshot_ptr };

        if snapshot.entries.is_empty() {
            return frame_count;
        }

        // ── Acquire scratch buffers (lock-free) ──────────────────────────
        // Safety: Only the audio thread calls pull(). Calls are serialized
        // by the OS audio callback, so there is never concurrent access.
        let scratch = unsafe { &mut *self.scratch.get() };

        // Ensure the mix accumulation buffer is large enough and zeroed.
        scratch.mix_buf.resize(buffer.len(), 0.0);

        for entry in &snapshot.entries {
            // ── Skip paused / dead streams ─────────────────────────────
            if !entry.state.alive.load(Ordering::Relaxed) {
                continue;
            }
            if entry.state.paused.load(Ordering::Relaxed) {
                continue;
            }

            let stream_channels = entry.stream_channels as usize;
            let ratio = entry.ratio;

            // ── Calculate how many input frames we need ────────────────
            // Start from the current resample position.
            let pos_bits = entry.position.load(Ordering::Relaxed);
            let pos = f64::from_bits(pos_bits);

            // Start frame (floor) and fractional offset.
            let start_frame = pos as u64;
            let frac = pos - (start_frame as f64);

            // End position after processing frame_count mixer frames.
            let end_pos = pos + (frame_count as f64) * ratio;
            let end_frame = (end_pos.ceil() as u64).max(1);

            // Total input frames needed from this stream.
            let need_frames = (end_frame - start_frame) as usize;
            let need_samples = need_frames * stream_channels;

            // ── Read from stream ───────────────────────────────────────
            scratch.stream_buf.resize(need_samples, 0.0);
            let samples_read = entry
                .stream
                .read(&mut scratch.stream_buf, need_frames as u64);

            if samples_read == 0 {
                if entry.stream.is_eof() {
                    // Stream has reached its end — flag for cleanup.
                    entry.state.eof.store(true, Ordering::Release);
                    // Update position for next call.
                    entry
                        .position
                        .store(f64::to_bits(end_pos), Ordering::Relaxed);
                }
                // If not eof, just skip this round (no data available yet).
                continue;
            }

            let read_frames = samples_read / stream_channels;
            let read_samples = read_frames * stream_channels;

            // If we read fewer frames than needed, pad with zeros.
            let effective_frames = read_frames.max(need_frames);
            let effective_samples = effective_frames * stream_channels;
            scratch.stream_buf.resize(effective_samples, 0.0);

            // ── Apply modifier if set (lock-free, before mixing!) ──────────
            let volume = f32::from_bits(entry.state.volume.load(Ordering::Relaxed));

            let modifier_ptr = entry.state.modifier.load(Ordering::Acquire);
            if !modifier_ptr.is_null() {
                // Safety: The pointer was created via Box::into_raw in
                // set_modifier(). Deferred-free guarantees the pointer
                // is not freed while any audio callback might observe it.
                unsafe {
                    (*modifier_ptr)(&mut scratch.stream_buf[..read_samples]);
                }
            }

            // ── Resample + channel-convert + mix ───────────────────────
            if (ratio - 1.0).abs() < 1e-9 && stream_channels == mixer_channels {
                // ── Fast path: no resampling, no channel conversion ────
                mix_direct(
                    &scratch.stream_buf,
                    &mut scratch.mix_buf,
                    read_samples,
                    mixer_channels,
                    volume,
                );
            } else {
                // ── Generic path: linear interpolation resampling ──────
                mix_resampled(
                    &scratch.stream_buf,
                    &mut scratch.mix_buf,
                    stream_channels,
                    mixer_channels,
                    ratio,
                    frame_count,
                    start_frame,
                    frac,
                    effective_frames,
                    volume,
                );
            }

            // ── Update position ────────────────────────────────────────
            entry
                .position
                .store(f64::to_bits(end_pos), Ordering::Relaxed);
        }

        // ── Copy accumulated mix_buf to output and zero mix_buf ────────
        for (out, &mix) in buffer.iter_mut().zip(scratch.mix_buf.iter()) {
            *out = mix;
        }
        scratch.mix_buf.fill(0.0);

        frame_count
    }
}

/// Fast path: no resampling, no channel conversion. Just apply volume
/// and add into the output.
fn mix_direct(src: &[f32], dst: &mut [f32], src_len: usize, _channels: usize, volume: f32) {
    let samples = src_len.min(dst.len());
    for i in 0..samples {
        dst[i] += src[i] * volume;
    }
}

/// Mix a stream with optional resampling and channel conversion.
///
/// Uses linear interpolation for resampling. Handles channel count
/// mismatch: extra channels are trimmed, missing channels are filled
/// by repeating the available channels.
fn mix_resampled(
    src: &[f32],
    dst: &mut [f32],
    src_channels: usize,
    dst_channels: usize,
    ratio: f64,
    frame_count: usize,
    start_frame: u64,
    _frac: f64,
    src_frames: usize,
    volume: f32,
) {
    for out_frame in 0..frame_count {
        // Position in the source stream (as f64 for sub-frame accuracy).
        let src_pos = (start_frame as f64) + (out_frame as f64) * ratio;
        let src_pos_clamped = src_pos.min((src_frames - 1) as f64).max(0.0);

        let idx0 = src_pos_clamped as usize;
        let idx1 = (idx0 + 1).min(src_frames - 1);
        let frac = src_pos_clamped - (idx0 as f64);

        let out_offset = out_frame * dst_channels;

        // For each output channel, interpolate from source channels.
        for ch in 0..dst_channels {
            // Map output channel to source channel:
            // - If src has more channels than dst, use the first dst_channels.
            // - If src has fewer, repeat the last available channel.
            let src_ch = if ch < src_channels {
                ch
            } else {
                src_channels - 1
            };

            let s0 = src[idx0 * src_channels + src_ch];
            let s1 = src[idx1 * src_channels + src_ch];
            let sample = s0 + (s1 - s0) * frac as f32;

            dst[out_offset + ch] += sample * volume;
        }
    }
}

impl Drop for Mixer {
    fn drop(&mut self) {
        // Free the current snapshot.
        let ptr = self.snapshot.load(Ordering::Relaxed);
        if !ptr.is_null() {
            unsafe {
                drop(Box::from_raw(ptr));
            }
        }

        // Free any pending snapshot.
        if let Some(ptr) = self.pending_free.lock().take() {
            unsafe {
                drop(Box::from_raw(ptr));
            }
        }
    }
}
