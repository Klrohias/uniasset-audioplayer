using System;
using System.ComponentModel;

namespace Uniasset.AudioPlayer.Unsafe
{
    /// <summary>
    /// Raw handle to a native <c>AudioPlayer</c>. Wraps an opaque <c>void*</c>.
    /// Provides 1:1 mapping to the <c>UAP_AudioPlayer_*</c> C functions.
    /// </summary>
    [EditorBrowsable(EditorBrowsableState.Never)]
    public readonly unsafe struct UnsafeAudioPlayer
    {
        /// <summary>The opaque native handle. Null if creation failed.</summary>
        public readonly void* Instance;

        /// <summary>
        /// Wrap an existing native handle.
        /// </summary>
        public UnsafeAudioPlayer(void* instance)
        {
            Instance = instance;
        }

        /// <summary>
        /// Create a new AudioPlayer, opening the default platform audio device.
        /// Throws <see cref="NativeException"/> on failure.
        /// </summary>
        public static UnsafeAudioPlayer Create()
        {
            var handle = Interop.UAP_AudioPlayer_New();
            NativeException.ThrowIfNeeded();
            if (handle == null)
                throw new NativeException("Failed to create AudioPlayer: native returned null");
            return new UnsafeAudioPlayer(handle);
        }

        /// <summary>
        /// Query the output device format (sample rate and channel count).
        /// </summary>
        public void GetFormat(out uint sampleRate, out ushort channels)
        {
            uint sr = 0;
            ushort ch = 0;
            Interop.UAP_AudioPlayer_Format(Instance, &sr, &ch);
            NativeException.ThrowIfNeeded();
            sampleRate = sr;
            channels = ch;
        }

        /// <summary>
        /// Add an audio stream to the player. <paramref name="stream"/> must point
        /// to a valid <see cref="NativeAudioStream"/> that the caller keeps alive.
        /// Returns a <see cref="UnsafePlayHandle"/>.
        /// Throws <see cref="NativeException"/> on failure.
        /// </summary>
        public UnsafePlayHandle AddStream(NativeAudioStream* stream)
        {
            var result = Interop.UAP_AudioPlayer_AddStream(Instance, stream);
            NativeException.ThrowIfNeeded();
            if (result == null)
                throw new NativeException("Failed to add stream: native returned null");
            return new UnsafePlayHandle(result);
        }

        /// <summary>
        /// Add an audio stream by reference. The struct address is taken internally
        /// so the caller does not need an unsafe context.
        /// </summary>
        public unsafe UnsafePlayHandle AddStream(ref NativeAudioStream stream)
        {
            return AddStream(&stream);
        }

        /// <summary>
        /// Remove all streams that have reached EOF. Call periodically to free resources.
        /// </summary>
        public void CleanupEof()
        {
            Interop.UAP_AudioPlayer_CleanupEof(Instance);
        }

        /// <summary>
        /// Return the number of currently active (non-EOF) streams.
        /// </summary>
        public uint StreamCount()
        {
            var result = Interop.UAP_AudioPlayer_StreamCount(Instance);
            return result;
        }

        /// <summary>Pause the audio device. Returns true on success.</summary>
        public bool Pause()
        {
            var result = Interop.UAP_AudioPlayer_Pause(Instance);
            NativeException.ThrowIfNeeded();
            return result != 0;
        }

        /// <summary>Resume the audio device. Returns true on success.</summary>
        public bool Resume()
        {
            var result = Interop.UAP_AudioPlayer_Resume(Instance);
            NativeException.ThrowIfNeeded();
            return result != 0;
        }

        /// <summary>Stop playback and close the audio device. Returns true on success.</summary>
        public bool Stop()
        {
            var result = Interop.UAP_AudioPlayer_Stop(Instance);
            NativeException.ThrowIfNeeded();
            return result != 0;
        }

        /// <summary>
        /// Destroy the native player handle. Stops playback and releases the device.
        /// Never throws (errors are silently ignored, matching the destroy-no-throw pattern).
        /// </summary>
        public void Destroy()
        {
            if (Instance == null)
                return;
            Interop.UAP_AudioPlayer_Destroy(Instance);
            // Intentionally no ThrowIfNeeded — destroy should never throw.
        }
    }
}
