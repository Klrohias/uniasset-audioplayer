using System;
using System.Runtime.InteropServices;
using System.Threading;
using AOT;
using Uniasset.AudioPlayer.Unsafe;

namespace Uniasset.AudioPlayer
{
    /// <summary>
    /// Controls playback for a single audio stream added to an <see cref="AudioPlayer"/>.
    /// Provides pause/resume, volume control, seeking, and DSP modifier installation.
    /// Must be disposed to release native resources.
    /// </summary>
    public sealed unsafe class PlayHandle : IDisposable
    {
        private int _disposedFlag;
        private GCHandle _streamGcHandle;
        private GCHandle _modifierGcHandle;
        private readonly UnsafeAudioStream _streamHandle;

        internal UnsafePlayHandle UnsafeHandle { get; private set; }

        // ==================================================================
        // Modifier callback bridge (static — one delegate shared by all handles)
        // ==================================================================

        /// <summary>C callback signature for the pre-mix modifier.</summary>
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate void NativeModifierFn(float* buffer, ulong sampleCount, void* userData);

        private static readonly NativeModifierFn s_modifierDelegate = ModifierBridge;
        private static readonly void* s_modifierPtr;

        static PlayHandle()
        {
            s_modifierPtr = Marshal.GetFunctionPointerForDelegate(s_modifierDelegate).ToPointer();
        }

        /// <summary>
        /// Audio-thread modifier bridge. Recovers the managed <see cref="ModifierCallback"/>
        /// from the GCHandle and invokes it with a <see cref="Span{T}"/> over the native buffer.
        /// </summary>
        [MonoPInvokeCallback(typeof(NativeModifierFn))]
        private static unsafe void ModifierBridge(float* buffer, ulong sampleCount, void* userData)
        {
            try
            {
                var handle = GCHandle.FromIntPtr(new IntPtr(userData));
                var callback = (ModifierCallback)handle.Target;
                if (callback == null)
                    return;

                var span = new Span<float>(buffer, checked((int)sampleCount));
                callback(span);
            }
            catch
            {
                // Audio thread must never crash.
            }
        }

        // ==================================================================
        // Construction
        // ==================================================================

        internal PlayHandle(
            UnsafePlayHandle unsafeHandle,
            GCHandle streamGcHandle,
            UnsafeAudioStream streamHandle)
        {
            UnsafeHandle = unsafeHandle;
            _streamGcHandle = streamGcHandle;
            _streamHandle = streamHandle;
        }

        // ==================================================================
        // Public API
        // ==================================================================

        /// <summary>Returns true if this stream is currently paused.</summary>
        public bool IsPaused => UnsafeHandle.IsPaused();

        /// <summary>
        /// Returns true if this stream is still alive in the mixer
        /// (has not been cleaned up via <see cref="AudioPlayer.CleanupEof"/>).
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
        /// Signal this stream to stop. The mixer will remove it on the
        /// next <see cref="AudioPlayer.CleanupEof"/> call.
        /// </summary>
        public void Stop() => UnsafeHandle.Stop();

        /// <summary>
        /// Seek to the given absolute frame position.
        /// Returns true on success.
        /// </summary>
        /// <exception cref="NativeException">Thrown if the native seek fails.</exception>
        public bool Seek(ulong frame)
        {
            return UnsafeHandle.Seek(frame);
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
        /// <returns>True on success.</returns>
        /// <exception cref="NativeException">Thrown if the native call fails.</exception>
        public bool SetModifier(ModifierCallback callback)
        {
            // Remove existing modifier first — unregister from native side
            // before freeing the GCHandle, so the audio thread stops using it.
            if (_modifierGcHandle.IsAllocated)
            {
                UnsafeHandle.SetModifier(null, null);
                _modifierGcHandle.Free();
            }

            if (callback == null)
                return UnsafeHandle.SetModifier(null, null);

            var gcHandle = GCHandle.Alloc(callback);
            try
            {
                var userData = GCHandle.ToIntPtr(gcHandle).ToPointer();
                var result = UnsafeHandle.SetModifier(s_modifierPtr, userData);
                if (result)
                {
                    _modifierGcHandle = gcHandle;
                }
                else
                {
                    gcHandle.Free();
                }
                return result;
            }
            catch
            {
                gcHandle.Free();
                throw;
            }
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

            // Remove modifier first — audio thread must stop using the GCHandle
            // before we free it.
            if (_modifierGcHandle.IsAllocated)
            {
                UnsafeHandle.SetModifier(null, null);
                _modifierGcHandle.Free();
            }

            // Signal EOF to the native side so the mixer stops calling our callbacks.
            UnsafeHandle.Stop();

            // Drop our C references. The mixer retains internal Arc refs until
            // CleanupEof is called on the player.
            UnsafeHandle.Destroy();
            _streamHandle.Destroy();

            // Free the stream GCHandle. The callback try/catch handles the edge
            // case where the audio thread is mid-callback (Target returns null).
            if (_streamGcHandle.IsAllocated)
                _streamGcHandle.Free();

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
