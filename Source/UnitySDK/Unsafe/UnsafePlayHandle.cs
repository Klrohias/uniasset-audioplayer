using System;
using System.ComponentModel;

namespace Uniasset.AudioPlayer.Unsafe
{
    /// <summary>
    /// Raw handle to a native <c>PlayHandle</c>. Wraps an opaque <c>void*</c>.
    /// Provides 1:1 mapping to the <c>UAP_PlayHandle_*</c> C functions.
    /// </summary>
    [EditorBrowsable(EditorBrowsableState.Never)]
    public readonly unsafe struct UnsafePlayHandle
    {
        /// <summary>The opaque native handle.</summary>
        public readonly void* Instance;

        public UnsafePlayHandle(void* instance)
        {
            Instance = instance;
        }

        /// <summary>
        /// Destroy this PlayHandle, dropping the C caller's reference.
        /// The mixer holds its own references independently; the stream
        /// continues playing until it reaches EOF or <see cref="Stop"/> is called.
        /// </summary>
        public void Destroy()
        {
            if (Instance == null)
                return;
            Interop.UAP_PlayHandle_Destroy(Instance);
            // Intentionally no ThrowIfNeeded.
        }

        /// <summary>Pause this stream. No-op if the stream is no longer alive.</summary>
        public void Pause()
        {
            if (Instance == null) return;
            Interop.UAP_PlayHandle_Pause(Instance);
        }

        /// <summary>Resume this stream. No-op if the stream is no longer alive.</summary>
        public void Resume()
        {
            if (Instance == null) return;
            Interop.UAP_PlayHandle_Resume(Instance);
        }

        /// <summary>Returns true if this stream is currently paused.</summary>
        public bool IsPaused()
        {
            if (Instance == null) return false;
            return Interop.UAP_PlayHandle_IsPaused(Instance) != 0;
        }

        /// <summary>Returns true if this stream is still active in the mixer.</summary>
        public bool IsAlive()
        {
            if (Instance == null) return false;
            return Interop.UAP_PlayHandle_IsAlive(Instance) != 0;
        }

        /// <summary>
        /// Signal this stream to stop. The mixer removes it from the active
        /// stream set once it observes the stop signal.
        /// No-op if the stream is no longer alive.
        /// </summary>
        public void Stop()
        {
            if (Instance == null) return;
            Interop.UAP_PlayHandle_Stop(Instance);
        }

        /// <summary>
        /// Set volume, clamped to [0.0, 1.0] on the native side.
        /// </summary>
        public void SetVolume(float volume)
        {
            if (Instance == null) return;
            Interop.UAP_PlayHandle_SetVolume(Instance, volume);
        }

        /// <summary>Returns the current volume in [0.0, 1.0].</summary>
        public float GetVolume()
        {
            if (Instance == null) return 0f;
            return Interop.UAP_PlayHandle_GetVolume(Instance);
        }

        /// <summary>
        /// Seek to the given absolute frame position.
        /// Throws <see cref="NativeException"/> on failure.
        /// </summary>
        public void Seek(ulong frame)
        {
            if (Instance == null) return;
            Interop.UAP_PlayHandle_Seek(Instance, frame);
            NativeException.ThrowIfNeeded();
        }

        /// <summary>
        /// Install a pre-mix modifier callback. <paramref name="modifier"/> must
        /// point to a valid <see cref="NativeModifier"/> struct. The callback
        /// runs on the audio thread and must be wait-free.
        /// </summary>
        public void SetModifier(NativeModifier* modifier)
        {
            if (Instance == null) return;
            Interop.UAP_PlayHandle_SetModifier(Instance, modifier);
        }
    }
}
