using System;
using System.ComponentModel;

namespace Uniasset.AudioPlayer.Unsafe
{
    /// <summary>
    /// Raw handle to a native <c>InternalAudioStream</c>. Wraps an opaque <c>void*</c>.
    /// Provides 1:1 mapping to the <c>UAP_InternalAudioStream_*</c> C functions.
    /// </summary>
    [EditorBrowsable(EditorBrowsableState.Never)]
    public readonly unsafe struct UnsafeInternalAudioStream
    {
        /// <summary>The opaque native handle. Null if destroyed / uninitialized.</summary>
        public readonly void* Instance;

        /// <summary>
        /// Wrap an existing native handle.
        /// </summary>
        public UnsafeInternalAudioStream(void* instance)
        {
            Instance = instance;
        }

        // ==================================================================
        // AudioStream trait bindings
        // ==================================================================

        /// <summary>
        /// Read interleaved f32 samples. <paramref name="buffer"/> must be at
        /// least <c>frameCount * channels</c> samples.
        /// Returns the number of <b>samples</b> written, or 0 at EOF.
        /// Must be wait-free — called from the audio thread.
        /// </summary>
        public ulong Read(float* buffer, ulong frameCount)
        {
            if (Instance == null) return 0;
            return Interop.UAP_InternalAudioStream_Read(Instance, buffer, frameCount);
        }

        /// <summary>
        /// Seek to the given absolute frame position.
        /// Returns true on success. On failure, throws <see cref="NativeException"/>.
        /// Called from the control thread — may block.
        /// </summary>
        public bool Seek(ulong frame)
        {
            if (Instance == null) return false;
            var ok = Interop.UAP_InternalAudioStream_Seek(Instance, frame);
            NativeException.ThrowIfNeeded();
            return ok != 0;
        }

        /// <summary>
        /// Returns true if the stream has reached its end.
        /// Must be wait-free — called from the audio thread.
        /// </summary>
        public bool IsEof()
        {
            if (Instance == null) return true;
            return Interop.UAP_InternalAudioStream_IsEof(Instance) != 0;
        }

        /// <summary>
        /// Return the number of channels (1 = mono, 2 = stereo).
        /// Must be wait-free — called from the audio thread.
        /// </summary>
        public ushort Channels()
        {
            if (Instance == null) return 0;
            return Interop.UAP_InternalAudioStream_Channels(Instance);
        }

        /// <summary>
        /// Return the sample rate in Hz (e.g., 44100, 48000).
        /// Must be wait-free — called from the audio thread.
        /// </summary>
        public uint SampleRate()
        {
            if (Instance == null) return 0;
            return Interop.UAP_InternalAudioStream_SampleRate(Instance);
        }

        // ==================================================================
        // Ownership
        // ==================================================================

        /// <summary>
        /// Destroy the native stream handle. Drops the C caller's reference.
        /// Never throws (errors are silently ignored, matching the destroy-no-throw pattern).
        /// </summary>
        public void Destroy()
        {
            if (Instance == null)
                return;
            Interop.UAP_InternalAudioStream_Destroy(Instance);
            // Intentionally no ThrowIfNeeded — destroy should never throw.
        }
    }
}
