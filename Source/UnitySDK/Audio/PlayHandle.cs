using System;
using System.Threading;
using Uniasset.AudioPlayer.Unsafe;

namespace Uniasset.AudioPlayer
{
    /// <summary>
    /// Controls playback for a single audio stream added to an <see cref="AudioPlayer"/>.
    /// Provides pause/resume, volume control, seeking, and DSP modifier installation.
    /// Must be disposed to release native resources.
    /// </summary>
    public sealed class PlayHandle : IDisposable
    {
        private int _disposedFlag;
        private StreamBinding _streamBinding;
        private ModifierBinding? _modifierBinding;

        internal UnsafePlayHandle UnsafeHandle { get; private set; }

        // ==================================================================
        // Construction
        // ==================================================================

        internal PlayHandle(
            UnsafePlayHandle unsafeHandle,
            StreamBinding streamBinding)
        {
            UnsafeHandle = unsafeHandle;
            _streamBinding = streamBinding;
        }

        // ==================================================================
        // Public API
        // ==================================================================

        /// <summary>Returns true if this stream is currently paused.</summary>
        public bool IsPaused => UnsafeHandle.IsPaused();

        /// <summary>
        /// Returns true if this stream is still active in the mixer.
        /// </summary>
        public bool IsAlive => UnsafeHandle.IsAlive();

        /// <summary>
        /// Gets or sets the stream volume in the range [0.0, 1.0].
        /// Values outside this range are clamped on the native side.
        /// </summary>
        public float Volume
        {
            get => UnsafeHandle.GetVolume();
            set => UnsafeHandle.SetVolume(value);
        }

        /// <summary>Pause this stream.</summary>
        public void Pause() => UnsafeHandle.Pause();

        /// <summary>Resume this stream if paused.</summary>
        public void Resume() => UnsafeHandle.Resume();

        /// <summary>
        /// Signal this stream to stop. The mixer removes it from the active
        /// stream set once it observes the stop signal.
        /// </summary>
        public void Stop() => UnsafeHandle.Stop();

        /// <summary>
        /// Seek to the given absolute frame position.
        /// </summary>
        /// <exception cref="NativeException">Thrown if the native seek fails.</exception>
        public void Seek(ulong frame)
        {
            UnsafeHandle.Seek(frame);
        }

        /// <summary>
        /// Install or remove a pre-mix DSP modifier callback.
        /// Pass null to remove any previously installed modifier.
        /// </summary>
        /// <param name="callback">
        /// The modifier callback, or null to remove. If non-null, the callback
        /// MUST be wait-free (no locks, no allocations, no blocking I/O) —
        /// it runs on the real-time audio thread.
        /// </param>
        public void SetModifier(ModifierCallback? callback)
        {
            // Remove existing modifier first — replace with no-op on the native
            // side so the audio thread stops using the old callback, then free
            // the GCHandle safely.
            if (_modifierBinding.HasValue)
            {
                // Install a no-op to neutralize the old modifier.
                ModifierBridge.Install(UnsafeHandle, null);
                _modifierBinding.Value.Free();
                _modifierBinding = null;
            }

            var binding = ModifierBridge.Install(UnsafeHandle, callback);
            _modifierBinding = binding;
        }

        // ==================================================================
        // Disposal
        // ==================================================================

        /// <summary>
        /// Dispose this PlayHandle. Signals the stream to stop and releases
        /// all native resources. Safe to call multiple times.
        /// </summary>
        public void Dispose()
        {
            if (Interlocked.CompareExchange(ref _disposedFlag, 1, 0) != 0)
                return;

            // Remove modifier first — replace with no-op so the audio thread
            // stops using the managed callback before we free the GCHandle.
            if (_modifierBinding.HasValue)
            {
                ModifierBridge.Install(UnsafeHandle, null);
                _modifierBinding.Value.Free();
                _modifierBinding = null;
            }

            // Signal EOF to the native side so the mixer stops calling our callbacks.
            UnsafeHandle.Stop();

            // Drop our C reference. The mixer retains internal Arc refs until
            // the stream is removed from the mixer.
            UnsafeHandle.Destroy();

            // Free the stream GCHandle. The callback try/catch handles the edge
            // case where the audio thread is mid-callback (Target returns null).
            _streamBinding.Free();

            GC.SuppressFinalize(this);
        }

        /// <summary>
        /// Finalizer fallback — ensures native resources are released if
        /// Dispose was not called.
        /// </summary>
        ~PlayHandle()
        {
            Dispose();
        }
    }
}
