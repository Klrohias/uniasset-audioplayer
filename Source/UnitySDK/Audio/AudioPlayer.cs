using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Threading;
using Uniasset.AudioPlayer.Unsafe;

namespace Uniasset.AudioPlayer
{
    /// <summary>
    /// A cross-platform audio player. Wraps the native <c>uniasset_audioplayer</c>
    /// library to provide audio device management, PCM stream mixing, and
    /// per-stream playback control.
    /// </summary>
    /// <remarks>
    /// Typical usage:
    /// <code>
    /// using var player = new AudioPlayer();
    /// player.GetFormat(out var sampleRate, out var channels);
    /// var stream = new MyAudioStream { Channels = channels, SampleRate = sampleRate };
    /// var handle = player.Play(stream);
    /// handle.Volume = 0.5f;
    /// // ... periodically call player.CleanupEof() ...
    /// </code>
    /// </remarks>
    public sealed class AudioPlayer : IDisposable
    {
        private int _disposedFlag;
        private readonly object _lock = new();
        private readonly List<PlayHandle> _activeHandles = new();
        private CancellationTokenSource _cts = new();

        /// <summary>
        /// The raw unsafe handle. Exposed for advanced use cases.
        /// </summary>
        public UnsafeAudioPlayer UnsafeHandle { get; }

        /// <summary>
        /// Create a new AudioPlayer, opening the default platform audio device
        /// and starting playback immediately.
        /// </summary>
        /// <exception cref="NativeException">Thrown if the native player could not be created.</exception>
        public AudioPlayer()
        {
            UnsafeHandle = UnsafeAudioPlayer.Create();
        }

        // ==================================================================
        // Device Format
        // ==================================================================

        /// <summary>
        /// Query the output device format.
        /// </summary>
        /// <param name="sampleRate">The sample rate in Hz (e.g. 48000).</param>
        /// <param name="channels">The channel count (e.g. 2 for stereo).</param>
        /// <exception cref="ObjectDisposedException">Thrown if the player has been disposed.</exception>
        public void GetFormat(out uint sampleRate, out ushort channels)
        {
            ThrowIfDisposed();
            UnsafeHandle.GetFormat(out sampleRate, out channels);
        }

        // ==================================================================
        // Stream Management
        // ==================================================================

        /// <summary>
        /// Add an audio stream to the player and start playback.
        /// </summary>
        /// <param name="stream">
        /// The audio stream source. Its <see cref="IAudioStream.ReadF32"/>,
        /// <see cref="IAudioStream.IsEof"/>, <see cref="IAudioStream.Channels"/>,
        /// and <see cref="IAudioStream.SampleRate"/> members will be called
        /// from the audio thread — they MUST be wait-free.
        /// </param>
        /// <returns>A <see cref="PlayHandle"/> for controlling playback.</returns>
        /// <exception cref="ArgumentNullException">Thrown if <paramref name="stream"/> is null.</exception>
        /// <exception cref="ObjectDisposedException">Thrown if the player has been disposed.</exception>
        /// <exception cref="NativeException">Thrown if the native stream creation fails.</exception>
        public PlayHandle Play(IAudioStream stream)
        {
            ThrowIfDisposed();
            if (stream == null)
                throw new ArgumentNullException(nameof(stream));

            var gcHandle = GCHandle.Alloc(stream);
            try
            {
                var unsafeStream = UnsafeAudioStream.Create(GCHandle.ToIntPtr(gcHandle));
                var playHandlePtr = UnsafeHandle.AddStream(unsafeStream);

                var playHandle = new PlayHandle(playHandlePtr, gcHandle, unsafeStream);

                lock (_lock)
                {
                    _activeHandles.Add(playHandle);
                }

                return playHandle;
            }
            catch
            {
                gcHandle.Free();
                throw;
            }
        }

        /// <summary>
        /// Remove all streams that have reached EOF.
        /// Call this periodically (e.g. every frame or on a timer) to free
        /// resources. Also removes disposed handles from the internal list.
        /// </summary>
        /// <exception cref="ObjectDisposedException">Thrown if the player has been disposed.</exception>
        public void CleanupEof()
        {
            ThrowIfDisposed();
            UnsafeHandle.CleanupEof();
        }

        /// <summary>
        /// The number of currently active (non-EOF) streams in the mixer.
        /// </summary>
        /// <exception cref="ObjectDisposedException">Thrown if the player has been disposed.</exception>
        public int StreamCount
        {
            get
            {
                ThrowIfDisposed();
                return (int)UnsafeHandle.StreamCount();
            }
        }

        // ==================================================================
        // Device Control
        // ==================================================================

        /// <summary>
        /// Pause the audio device (silences all output).
        /// Returns true on success.
        /// </summary>
        /// <exception cref="ObjectDisposedException">Thrown if the player has been disposed.</exception>
        /// <exception cref="NativeException">Thrown if the native call fails.</exception>
        public bool Pause()
        {
            ThrowIfDisposed();
            return UnsafeHandle.Pause();
        }

        /// <summary>
        /// Resume the audio device after pausing.
        /// Returns true on success.
        /// </summary>
        /// <exception cref="ObjectDisposedException">Thrown if the player has been disposed.</exception>
        /// <exception cref="NativeException">Thrown if the native call fails.</exception>
        public bool Resume()
        {
            ThrowIfDisposed();
            return UnsafeHandle.Resume();
        }

        /// <summary>
        /// Stop playback and close the audio device.
        /// After this call, the device cannot be resumed.
        /// Returns true on success.
        /// </summary>
        /// <exception cref="ObjectDisposedException">Thrown if the player has been disposed.</exception>
        /// <exception cref="NativeException">Thrown if the native call fails.</exception>
        public bool Stop()
        {
            ThrowIfDisposed();
            return UnsafeHandle.Stop();
        }

        // ==================================================================
        // Disposal
        // ==================================================================

        private void ThrowIfDisposed()
        {
            if (Volatile.Read(ref _disposedFlag) != 0)
                throw new ObjectDisposedException(nameof(AudioPlayer));
        }

        /// <summary>
        /// Dispose the player. Stops all streams, releases the audio device,
        /// and frees all native resources. Safe to call multiple times.
        /// </summary>
        public void Dispose()
        {
            if (Interlocked.CompareExchange(ref _disposedFlag, 1, 0) != 0)
                return;

            _cts.Cancel();
            _cts.Dispose();

            PlayHandle[] handles;
            lock (_lock)
            {
                handles = _activeHandles.ToArray();
                _activeHandles.Clear();
            }

            // Dispose all child handles — each calls Stop() and frees resources.
            foreach (var handle in handles)
            {
                handle.Dispose();
            }

            // Destroy the native player handle last, after all streams are stopped.
            UnsafeHandle.Destroy();

            GC.SuppressFinalize(this);
        }

        /// <summary>
        /// Finalizer fallback — ensures native resources are released if
        /// Dispose was not called.
        /// </summary>
        ~AudioPlayer()
        {
            Dispose();
        }
    }
}
