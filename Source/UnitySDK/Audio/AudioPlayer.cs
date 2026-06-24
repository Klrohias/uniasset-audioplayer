using System;
using System.Collections.Generic;
using System.Threading;
using Uniasset.AudioPlayer.Unsafe;
using UnityEngine;

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
    /// </code>
    /// </remarks>
    public sealed class AudioPlayer : IDisposable
    {
        private int _disposedFlag;
        private readonly object _lock = new();
        private readonly List<PlayHandle> _activeHandles = new();

        /// <summary>
        /// The raw unsafe handle. Exposed for advanced use cases.
        /// </summary>
        private UnsafeAudioPlayer UnsafeHandle { get; }

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
        /// Add an audio stream to the player and start playback immediately.
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
            return AddStream(stream, playImmediate: true);
        }

        /// <summary>
        /// Add an audio stream to the player.
        /// </summary>
        /// <param name="stream">
        /// The audio stream source. Its <see cref="IAudioStream.ReadF32"/>,
        /// <see cref="IAudioStream.IsEof"/>, <see cref="IAudioStream.Channels"/>,
        /// and <see cref="IAudioStream.SampleRate"/> members will be called
        /// from the audio thread — they MUST be wait-free.
        /// </param>
        /// <param name="playImmediate">
        /// If true, playback starts immediately; otherwise the stream is
        /// added in a paused state.
        /// </param>
        /// <returns>A <see cref="PlayHandle"/> for controlling playback.</returns>
        /// <exception cref="ArgumentNullException">Thrown if <paramref name="stream"/> is null.</exception>
        /// <exception cref="ObjectDisposedException">Thrown if the player has been disposed.</exception>
        /// <exception cref="NativeException">Thrown if the native stream creation fails.</exception>
        public PlayHandle AddStream(IAudioStream stream, bool playImmediate)
        {
            ThrowIfDisposed();
            if (stream == null)
                throw new ArgumentNullException(nameof(stream));

            // Fast path: InternalAudioStream — pass native handle directly
            if (stream is InternalAudioStream internalStream)
            {
                var playHandlePtr = UnsafeHandle.AddStream(
                    internalStream.UnsafeHandle, playImmediate);

                var playHandle = new PlayHandle(playHandlePtr, default);

                lock (_lock)
                {
                    _activeHandles.Add(playHandle);
                }

                return playHandle;
            }

            // Fallback: wrap managed IAudioStream in C callbacks
            var binding = AudioStreamFactory.CreateBinding(stream);
            try
            {
                var playHandlePtr = UnsafeHandle.AddStream(
                    ref binding.NativeStream, playImmediate);

                var playHandle = new PlayHandle(playHandlePtr, binding);

                lock (_lock)
                {
                    _activeHandles.Add(playHandle);
                }

                return playHandle;
            }
            catch
            {
                binding.Free();
                throw;
            }
        }

        /// <summary>
        /// Add an <see cref="UnityEngine.AudioClip"/> to the player and start
        /// playback immediately. The clip is wrapped in an
        /// <see cref="AudioClipStream"/> internally.
        /// </summary>
        /// <param name="clip">The AudioClip to play.</param>
        /// <returns>A <see cref="PlayHandle"/> for controlling playback.</returns>
        /// <exception cref="ArgumentNullException">Thrown if <paramref name="clip"/> is null.</exception>
        /// <exception cref="ArgumentException">Thrown if the clip has zero channels or zero samples.</exception>
        /// <exception cref="ObjectDisposedException">Thrown if the player has been disposed.</exception>
        public PlayHandle Play(AudioClip clip)
        {
            var stream = new AudioClipStream(clip);
            return Play(stream);
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
        /// </summary>
        /// <exception cref="ObjectDisposedException">Thrown if the player has been disposed.</exception>
        /// <exception cref="NativeException">Thrown if the native call fails.</exception>
        public void Pause()
        {
            ThrowIfDisposed();
            UnsafeHandle.Pause();
        }

        /// <summary>
        /// Resume the audio device after pausing.
        /// </summary>
        /// <exception cref="ObjectDisposedException">Thrown if the player has been disposed.</exception>
        /// <exception cref="NativeException">Thrown if the native call fails.</exception>
        public void Resume()
        {
            ThrowIfDisposed();
            UnsafeHandle.Resume();
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
