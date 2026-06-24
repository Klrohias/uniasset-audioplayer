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
        /// Add an audio stream backed by C function pointers to the player.
        /// <paramref name="stream"/> must point to a valid
        /// <see cref="NativeAudioStream"/> that the caller keeps alive.
        /// If <paramref name="playImmediate"/> is true, playback starts immediately;
        /// otherwise the stream is added in a paused state.
        /// Returns a <see cref="UnsafePlayHandle"/>.
        /// Throws <see cref="NativeException"/> on failure.
        /// </summary>
        public UnsafePlayHandle AddStream(NativeAudioStream* stream, bool playImmediate = true)
        {
            var result = Interop.UAP_AudioPlayer_AddNativeStream(
                Instance, stream, playImmediate ? (byte)1 : (byte)0);
            NativeException.ThrowIfNeeded();
            if (result == null)
                throw new NativeException("Failed to add stream: native returned null");
            return new UnsafePlayHandle(result);
        }

        /// <summary>
        /// Add an audio stream by reference. The struct address is taken internally
        /// so the caller does not need an unsafe context.
        /// </summary>
        public unsafe UnsafePlayHandle AddStream(ref NativeAudioStream stream, bool playImmediate = true)
        {
            fixed (NativeAudioStream* structPtr = &stream)
            {
                return AddStream(structPtr, playImmediate);
            }
        }

        /// <summary>
        /// Add a pre-constructed native audio stream handle to the player.
        /// <paramref name="streamHandle"/> must be a valid native handle encoding a
        /// <c>Box&lt;Arc&lt;dyn AudioStream&gt;&gt;</c>. The handle is <b>not</b>
        /// consumed — the caller remains responsible for destroying it.
        /// If <paramref name="playImmediate"/> is true, playback starts immediately;
        /// otherwise the stream is added in a paused state.
        /// Returns a <see cref="UnsafePlayHandle"/>.
        /// Throws <see cref="NativeException"/> on failure.
        /// </summary>
        public UnsafePlayHandle AddStream(UnsafeInternalAudioStream stream, bool playImmediate = true)
        {
            var result = Interop.UAP_AudioPlayer_AddStream(
                Instance, stream.Instance, playImmediate ? (byte)1 : (byte)0);
            NativeException.ThrowIfNeeded();
            if (result == null)
                throw new NativeException("Failed to add stream: native returned null");
            return new UnsafePlayHandle(result);
        }

        /// <summary>
        /// Return the number of currently active (non-EOF) streams.
        /// </summary>
        public uint StreamCount()
        {
            var result = Interop.UAP_AudioPlayer_StreamCount(Instance);
            return result;
        }

        /// <summary>
        /// Pause the audio device (silences all output).
        /// Throws <see cref="NativeException"/> on failure.
        /// </summary>
        public void Pause()
        {
            Interop.UAP_AudioPlayer_Pause(Instance);
            NativeException.ThrowIfNeeded();
        }

        /// <summary>
        /// Resume the audio device after pausing.
        /// Throws <see cref="NativeException"/> on failure.
        /// </summary>
        public void Resume()
        {
            Interop.UAP_AudioPlayer_Resume(Instance);
            NativeException.ThrowIfNeeded();
        }

        /// <summary>
        /// Destroy the native player handle. Drops the C caller's reference.
        /// When the last reference is dropped, playback stops and the device
        /// is released.
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
