use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};

use crate::mixer::AudioStream;
use crate::AudioError;

/// A pre-mix modifier callback that can transform a stream's PCM buffer
/// before it is mixed into the output.
///
/// Called on the audio thread with the interleaved `f32` buffer for a
/// single stream. The callback may modify the samples in-place (e.g., for
/// filtering, panning, or effects).
pub type ModifierFn = Box<dyn Fn(&mut [f32]) + Send + Sync>;

/// Per-stream atomic state shared between [`Mixer`] and [`PlayHandle`].
///
/// All fields use atomic or lock-free access patterns so the audio thread
/// can read state without blocking.
pub(crate) struct StreamState {
    /// Whether this stream is still active in the mixer.
    pub alive: AtomicBool,

    /// Whether playback is paused.
    pub paused: AtomicBool,

    /// Volume multiplier (0.0 .. 1.0) stored as `f32::to_bits()`.
    pub volume: AtomicU32,

    /// Set to `true` by the audio thread when `AudioStream::read()` returns 0
    /// and `AudioStream::is_eof()` returns true.
    /// The control thread checks this flag and removes the stream.
    pub eof: AtomicBool,

    /// Optional pre-mix modifier callback — lock-free via `AtomicPtr`.
    /// The audio thread loads with `Acquire` and calls through it; the
    /// control thread swaps with `AcqRel` via `set_modifier()`.
    /// `null` means no modifier is set.
    pub modifier: AtomicPtr<ModifierFn>,

    /// Previous modifier pointer awaiting deferred free.
    /// The control thread frees this pointer in `set_modifier()` (next
    /// modifier change) or `rebuild_snapshot()` — both guarantee the
    /// audio thread has moved past the old value.
    pub modifier_pending_free: AtomicPtr<ModifierFn>,
}

impl StreamState {
    pub fn new() -> Self {
        Self {
            alive: AtomicBool::new(true),
            paused: AtomicBool::new(false),
            volume: AtomicU32::new(f32::to_bits(1.0)),
            eof: AtomicBool::new(false),
            modifier: AtomicPtr::new(std::ptr::null_mut()),
            modifier_pending_free: AtomicPtr::new(std::ptr::null_mut()),
        }
    }
}

impl Drop for StreamState {
    fn drop(&mut self) {
        // Free the live modifier pointer (if any).
        let ptr = *self.modifier.get_mut();
        if !ptr.is_null() {
            unsafe { drop(Box::from_raw(ptr)); }
        }
        // Free the pending-free modifier pointer (if any).
        let ptr = *self.modifier_pending_free.get_mut();
        if !ptr.is_null() {
            unsafe { drop(Box::from_raw(ptr)); }
        }
    }
}

/// Controls playback for a single stream in the [`Mixer`].
///
/// Created by [`Mixer::add_stream`]. The handle can be freely sent and
/// shared across threads. All methods that would panic if the stream has
/// been removed from the mixer instead become no-ops.
///
/// Cloning a `PlayHandle` is cheap — it only increments two atomic
/// reference counts.
pub struct PlayHandle {
    /// Shared atomic state with the mixer.
    pub(crate) state: Arc<StreamState>,
    /// Reference to the underlying audio stream (kept for `seek()`).
    pub(crate) stream: Arc<dyn AudioStream>,
}

// Safety: all mutable state is behind atomics (including AtomicPtr for modifier).
unsafe impl Send for PlayHandle {}
unsafe impl Sync for PlayHandle {}

impl Clone for PlayHandle {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            stream: Arc::clone(&self.stream),
        }
    }
}

impl PlayHandle {
    /// Pause playback. If the stream has been removed from the mixer,
    /// this is a no-op.
    pub fn pause(&self) {
        if self.state.alive.load(Ordering::Relaxed) {
            self.state.paused.store(true, Ordering::Release);
        }
    }

    /// Resume playback. If the stream has been removed from the mixer,
    /// this is a no-op.
    pub fn resume(&self) {
        if self.state.alive.load(Ordering::Relaxed) {
            self.state.paused.store(false, Ordering::Release);
        }
    }

    /// Returns `true` if the stream is currently paused.
    pub fn is_paused(&self) -> bool {
        self.state.paused.load(Ordering::Relaxed)
    }

    /// Returns `true` if the stream is still active in the mixer.
    pub fn is_alive(&self) -> bool {
        self.state.alive.load(Ordering::Relaxed)
    }

    /// Stop playback and mark this stream for removal from the mixer.
    ///
    /// Sets the EOF flag so that the next call to
    /// [`Mixer::cleanup_eof`](crate::mixer::Mixer::cleanup_eof) (or
    /// [`AudioPlayer::cleanup_eof`](crate::player::AudioPlayer::cleanup_eof))
    /// will remove this stream from the mixer's snapshot.
    ///
    /// If the stream has already been removed from the mixer, this is a no-op.
    pub fn stop(&self) {
        if self.state.alive.load(Ordering::Relaxed) {
            self.state.eof.store(true, Ordering::Release);
        }
    }

    /// Set the volume for this stream.
    ///
    /// Values are clamped to `[0.0, 1.0]`. The volume is applied on the
    /// audio thread before mixing.
    pub fn set_volume(&self, vol: f32) {
        let clamped = vol.clamp(0.0, 1.0);
        self.state
            .volume
            .store(f32::to_bits(clamped), Ordering::Release);
    }

    /// Get the current volume (0.0 .. 1.0).
    pub fn get_volume(&self) -> f32 {
        f32::from_bits(self.state.volume.load(Ordering::Relaxed))
    }

    /// Seek the underlying stream to the given frame position.
    ///
    /// If the stream has been removed from the mixer, this is a no-op
    /// and returns `Ok(())`.
    pub fn seek(&self, frame: u64) -> Result<(), AudioError> {
        if self.state.alive.load(Ordering::Relaxed) {
            self.stream.seek(frame)?;
        }
        Ok(())
    }

    /// Set an optional pre-mix modifier callback.
    ///
    /// The modifier is called on the audio thread with the interleaved
    /// `f32` buffer for this stream before it is mixed. Pass `None` to
    /// remove a previously set modifier.
    ///
    /// This can be used to implement per-stream effects such as filtering
    /// or panning.
    ///
    /// The modifier takes effect immediately — the next audio callback
    /// will see it. The old modifier is freed via deferred-free (on the
    /// next modifier change or snapshot rebuild) so the audio thread
    /// never sees a dangling pointer.
    pub fn set_modifier(&self, modifier: Option<ModifierFn>) {
        // Box the new modifier (if any) into a raw pointer.
        let new_ptr = match modifier {
            Some(f) => Box::into_raw(Box::new(f)),
            None => std::ptr::null_mut(),
        };

        // Atomically swap: audio thread sees the new modifier immediately.
        let old_ptr = self.state.modifier.swap(new_ptr, Ordering::AcqRel);

        // Deferred-free the old pointer: stash it in pending_free.
        // If there's already a pending pointer, free it now — it has
        // survived at least one full pull() cycle since the last swap.
        if !old_ptr.is_null() {
            let prev_pending = self
                .state
                .modifier_pending_free
                .swap(old_ptr, Ordering::AcqRel);
            if !prev_pending.is_null() {
                // Safety: prev_pending was the live modifier at least two
                // swaps ago. The audio thread has moved past it.
                unsafe { drop(Box::from_raw(prev_pending)); }
            }
        }
    }
}
