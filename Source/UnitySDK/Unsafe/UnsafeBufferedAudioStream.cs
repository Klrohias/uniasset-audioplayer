using System;
using System.ComponentModel;

namespace Uniasset.AudioPlayer.Unsafe
{
    /// <summary>
    /// Factory for creating native buffered audio streams.
    /// Wraps the <c>UAP_BufferedAudioStream_*</c> C functions.
    /// </summary>
    /// <remarks>
    /// The returned handle is an <see cref="UnsafeInternalAudioStream"/>
    /// (the buffered wrapper is an <c>AudioStreamWrapper</c> internally).
    /// Destroy with <see cref="UnsafeInternalAudioStream.Destroy"/>.
    /// </remarks>
    [EditorBrowsable(EditorBrowsableState.Never)]
    public static unsafe class UnsafeBufferedAudioStream
    {
        /// <summary>
        /// Wrap a native audio stream handle in a buffered stream.
        /// <paramref name="innerHandle"/> is not consumed — the caller
        /// remains responsible for destroying it.
        /// </summary>
        /// <exception cref="NativeException">
        /// Thrown if the native buffered stream could not be created.
        /// </exception>
        public static UnsafeInternalAudioStream Create(void* innerHandle)
        {
            var handle = Interop.UAP_BufferedAudioStream_Create(innerHandle);
            NativeException.ThrowIfNeeded();
            if (handle == null)
                throw new NativeException(
                    "Failed to create BufferedAudioStream: native returned null");
            return new UnsafeInternalAudioStream(handle);
        }

        /// <summary>
        /// Wrap a native audio stream (callbacks struct) in a buffered stream.
        /// <paramref name="stream"/> is copied — the caller retains ownership.
        /// </summary>
        /// <exception cref="NativeException">
        /// Thrown if the native buffered stream could not be created.
        /// </exception>
        public static unsafe UnsafeInternalAudioStream CreateFromNative(ref NativeAudioStream stream)
        {
            fixed (NativeAudioStream* streamPtr = &stream)
            {
                var handle = Interop.UAP_BufferedAudioStream_CreateFromNative(streamPtr);
                NativeException.ThrowIfNeeded();
                if (handle == null)
                    throw new NativeException(
                        "Failed to create BufferedAudioStream: native returned null");
                return new UnsafeInternalAudioStream(handle);
            }
        }
    }
}
