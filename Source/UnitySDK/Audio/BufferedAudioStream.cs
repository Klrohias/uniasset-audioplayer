using Uniasset.AudioPlayer.Unsafe;

namespace Uniasset.AudioPlayer
{
    /// <summary>
    /// An <see cref="InternalAudioStream"/> that wraps an inner stream in a
    /// native buffered stream for smooth playback.
    /// </summary>
    /// <remarks>
    /// Construct with an existing native handle. The constructor calls
    /// <c>UAP_BufferedAudioStream_Create</c> to wrap the inner stream in a
    /// 4-second ring buffer. The resulting buffered handle is managed by
    /// <see cref="InternalAudioStream"/> and destroyed on <see cref="IDisposable.Dispose"/>.
    /// </remarks>
    public sealed class BufferedAudioStream : InternalAudioStream
    {
        /// <summary>
        /// Wrap <paramref name="innerHandle"/> in a native buffered stream.
        /// </summary>
        /// <param name="innerHandle">
        /// A valid native handle encoding a <c>Box&lt;Arc&lt;dyn AudioStream&gt;&gt;</c>.
        /// The handle is <b>not</b> consumed.
        /// </param>
        /// <exception cref="NativeException">
        /// Thrown if the native buffered stream could not be created.
        /// </exception>
        public unsafe BufferedAudioStream(void* innerHandle)
        {
            var handle = Interop.UAP_BufferedAudioStream_Create(innerHandle);
            NativeException.ThrowIfNeeded();
            if (handle == null)
                throw new NativeException("Failed to create BufferedAudioStream: native returned null");
            SetHandle(new UnsafeInternalAudioStream(handle));
        }
    }
}
