using System;
using System.Threading;
using Uniasset.AudioPlayer.Unsafe;

namespace Uniasset.AudioPlayer
{
    /// <summary>
    /// A managed wrapper around a native <c>InternalAudioStream</c> handle.
    /// Implements <see cref="IAudioStream"/> by delegating to the native
    /// <c>UAP_InternalAudioStream_*</c> functions.
    /// </summary>
    /// <remarks>
    /// Disposing this object calls <c>UAP_InternalAudioStream_Destroy</c> to
    /// drop the C caller's reference. The underlying stream continues to
    /// live in the mixer independently.
    /// </remarks>
    public class InternalAudioStream : IAudioStream, IDisposable
    {
        private int _disposedFlag;

        /// <summary>
        /// The raw unsafe handle. Exposed for advanced use and for
        /// subclasses that construct the handle themselves.
        /// </summary>
        internal UnsafeInternalAudioStream UnsafeHandle { get; private set; }

        /// <summary>
        /// Wrap an existing native handle.
        /// </summary>
        /// <param name="handle">
        /// A valid <see cref="UnsafeInternalAudioStream"/> from a native stream factory
        /// (e.g. <c>UAP_BufferedAudioStream_Create</c>).
        /// </param>
        public InternalAudioStream(UnsafeInternalAudioStream handle)
        {
            UnsafeHandle = handle;
        }

        /// <summary>
        /// Parameterless constructor for subclasses that set the handle
        /// via <see cref="SetHandle"/> after construction.
        /// </summary>
        private protected InternalAudioStream() { }

        /// <summary>
        /// Set the native handle after construction. For use by subclasses
        /// that create the handle in their own constructor.
        /// </summary>
        private protected void SetHandle(UnsafeInternalAudioStream handle)
        {
            UnsafeHandle = handle;
        }

        // ==================================================================
        // IAudioStream
        // ==================================================================

        /// <inheritdoc />
        public unsafe int ReadF32(Span<float> buffer)
        {
            ThrowIfDisposed();
            fixed (float* ptr = buffer)
            {
                var channels = Channels;
                var frameCount = (ulong)(buffer.Length / channels);
                return (int)UnsafeHandle.Read(ptr, frameCount);
            }
        }

        /// <inheritdoc />
        public void SeekFrame(long frame)
        {
            ThrowIfDisposed();
            UnsafeHandle.Seek((ulong)frame);
        }

        /// <inheritdoc />
        public bool IsEof
        {
            get
            {
                ThrowIfDisposed();
                return UnsafeHandle.IsEof();
            }
        }

        /// <inheritdoc />
        public ushort Channels
        {
            get
            {
                ThrowIfDisposed();
                return UnsafeHandle.Channels();
            }
        }

        /// <inheritdoc />
        public uint SampleRate
        {
            get
            {
                ThrowIfDisposed();
                return UnsafeHandle.SampleRate();
            }
        }

        // ==================================================================
        // Disposal
        // ==================================================================

        /// <summary>
        /// Check whether this stream has been disposed.
        /// </summary>
        protected void ThrowIfDisposed()
        {
            if (Volatile.Read(ref _disposedFlag) != 0)
                throw new ObjectDisposedException(nameof(InternalAudioStream));
        }

        /// <summary>
        /// Dispose this stream. Drops the C caller's reference.
        /// The underlying stream continues to live in the mixer independently.
        /// Safe to call multiple times.
        /// </summary>
        public void Dispose()
        {
            if (Interlocked.CompareExchange(ref _disposedFlag, 1, 0) != 0)
                return;

            UnsafeHandle.Destroy();

            GC.SuppressFinalize(this);
        }

        /// <summary>
        /// Finalizer fallback — ensures native resources are released if
        /// Dispose was not called.
        /// </summary>
        ~InternalAudioStream()
        {
            Dispose();
        }
    }
}
