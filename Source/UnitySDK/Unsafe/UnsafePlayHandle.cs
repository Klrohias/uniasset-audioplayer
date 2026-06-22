using System;
using System.ComponentModel;

namespace Uniasset.AudioPlayer.Unsafe
{
    /// <summary>
    /// Raw handle to a native <c>PlayHandle</c>. Wraps an opaque <c>void*</c>.
    /// Provides 1:1 mapping to the <c>UAP_PlayHandle_*</c> C functions.
    /// All methods are no-ops on null handles.
    /// </summary>
    [EditorBrowsable(EditorBrowsableState.Never)]
    public readonly unsafe struct UnsafePlayHandle
    {
        /// <summary>The opaque native handle. May be null.</summary>
        public readonly void* Instance;

        public UnsafePlayHandle(void* instance)
        {
            Instance = instance;
        }

        /// <summary>
        /// Destroy this PlayHandle, dropping the C caller's reference.
        /// The handle remains valid to the mixer until EOF cleanup.
        /// </summary>
        public void Destroy()
        {
            if (Instance == null)
                return;
            Interop.UAP_PlayHandle_Destroy(Instance);
            // Intentionally no ThrowIfNeeded.
        }

        /// <summary>Pause this stream. No-op on null handle.</summary>
        public void Pause()
        {
            if (Instance == null) return;
            Interop.UAP_PlayHandle_Pause(Instance);
        }

        /// <summary>Resume this stream. No-op on null handle.</summary>
        public void Resume()
        {
            if (Instance == null) return;
            Interop.UAP_PlayHandle_Resume(Instance);
        }

        /// <summary>Returns true if this stream is paused. False for null handles.</summary>
        public bool IsPaused()
        {
            if (Instance == null) return false;
            return Interop.UAP_PlayHandle_IsPaused(Instance) != 0;
        }

        /// <summary>Returns true if this stream is still alive. False for null handles.</summary>
        public bool IsAlive()
        {
            if (Instance == null) return false;
            return Interop.UAP_PlayHandle_IsAlive(Instance) != 0;
        }

        /// <summary>Signal this stream to stop. No-op on null handle.</summary>
        public void Stop()
        {
            if (Instance == null) return;
            Interop.UAP_PlayHandle_Stop(Instance);
        }

        /// <summary>
        /// Set volume, clamped to [0.0, 1.0] on the native side.
        /// No-op on null handle.
        /// </summary>
        public void SetVolume(float volume)
        {
            if (Instance == null) return;
            Interop.UAP_PlayHandle_SetVolume(Instance, volume);
        }

        /// <summary>Returns the current volume in [0.0, 1.0]. Returns 0.0 for null handles.</summary>
        public float GetVolume()
        {
            if (Instance == null) return 0f;
            return Interop.UAP_PlayHandle_GetVolume(Instance);
        }

        /// <summary>
        /// Seek to the given absolute frame position.
        /// Returns true on success. On failure, check <see cref="NativeException.ThrowIfNeeded"/>.
        /// Returns false for null handles.
        /// </summary>
        public bool Seek(ulong frame)
        {
            if (Instance == null) return false;
            var result = Interop.UAP_PlayHandle_Seek(Instance, frame);
            NativeException.ThrowIfNeeded();
            return result != 0;
        }

        /// <summary>
        /// Install or remove a pre-mix modifier callback.
        /// Pass null callback and null userData to remove.
        /// Returns true on success. On failure, check <see cref="NativeException.ThrowIfNeeded"/>.
        /// Returns false for null handles.
        /// </summary>
        public bool SetModifier(void* callback, void* userData)
        {
            if (Instance == null)
            {
                // Set error for consistency with native behavior
                return false;
            }
            var result = Interop.UAP_PlayHandle_SetModifier(Instance, callback, userData);
            NativeException.ThrowIfNeeded();
            return result != 0;
        }
    }
}
