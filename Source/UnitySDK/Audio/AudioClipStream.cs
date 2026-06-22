using System;
using System.Threading;
using UnityEngine;

namespace Uniasset.AudioPlayer
{
    /// <summary>
    /// Wraps a <see cref="UnityEngine.AudioClip"/> as an <see cref="IAudioStream"/>.
    /// </summary>
    /// <remarks>
    /// All Unity API calls (GetData, channels, frequency, etc.) happen in the
    /// constructor on the main thread. The audio data is pre-read into a managed
    /// array so that <see cref="ReadF32"/>, which runs on the native WASAPI audio
    /// thread, only performs safe array copies — no Unity API access.
    ///
    /// Validates the clip on construction. If the clip is destroyed mid-playback
    /// (Unity's <c>== null</c> override detects it), <see cref="ReadF32"/> returns
    /// 0 and marks the stream as EOF.
    /// </remarks>
    public sealed class AudioClipStream : IAudioStream
    {
        private readonly AudioClip _clip;      // kept for the Unity == null override check
        private readonly float[] _audioData;   // complete interleaved f32 PCM, read on main thread
        private readonly long _totalFloats;    // pre-computed to avoid int overflow
        private readonly ushort _channels;     // cached: audio thread cannot access clip.channels
        private readonly uint _sampleRate;     // cached: audio thread cannot access clip.frequency
        private int _position;                 // float index in the interleaved sample buffer
        private int _eof;                      // 0 = alive, 1 = eof

        /// <summary>
        /// Create a stream from an AudioClip.
        /// </summary>
        /// <remarks>
        /// All Unity API access happens here (main thread). The entire clip is
        /// decoded into a managed <c>float[]</c> so the audio thread never
        /// touches Unity objects.
        /// </remarks>
        /// <exception cref="ArgumentNullException">Thrown if <paramref name="clip"/> is null.</exception>
        /// <exception cref="ArgumentException">Thrown if the clip has zero channels or zero samples.</exception>
        /// <exception cref="InvalidOperationException">Thrown if the clip's load type is not
        /// <see cref="AudioClipLoadType.DecompressOnLoad"/> (required by GetData), or if
        /// GetData fails.</exception>
        public AudioClipStream(AudioClip clip)
        {
            if (clip == null)
                throw new ArgumentNullException(nameof(clip));
            if (clip.channels == 0)
                throw new ArgumentException("AudioClip has no channels.", nameof(clip));
            if (clip.samples == 0)
                throw new ArgumentException("AudioClip has no samples.", nameof(clip));

            // GetData only works with DecompressOnLoad — compressed/streaming
            // clips return false and fill the buffer with zeros.
            if (clip.loadType != AudioClipLoadType.DecompressOnLoad)
                throw new InvalidOperationException(
                    $"AudioClip '{clip.name}' must be set to 'Decompress On Load'. "
                    + $"Current load type: {clip.loadType}. "
                    + "Select the clip in the Project view and change 'Load Type' in the Inspector.");

            _clip = clip;

            // Cache these now so the audio thread never touches Unity API.
            _channels = (ushort)clip.channels;
            _sampleRate = (uint)clip.frequency;

            // Read the entire clip into a managed array on the main thread.
            // From this point on, the audio thread only does array copies.
            var dataLength = clip.samples * clip.channels;
            _audioData = new float[dataLength];
            if (!clip.GetData(_audioData, 0))
            {
                throw new InvalidOperationException(
                    $"AudioClip '{clip.name}': GetData failed. "
                    + "Ensure the clip is set to 'Decompress On Load' in the Inspector.");
            }

            _totalFloats = dataLength;
        }

        /// <inheritdoc/>
        public ushort Channels => _channels;

        /// <inheritdoc/>
        public uint SampleRate => _sampleRate;

        /// <inheritdoc/>
        public bool IsEof => Volatile.Read(ref _eof) != 0;

        /// <inheritdoc/>
        public int ReadF32(Span<float> buffer)
        {
            // If the AudioClip was destroyed by Unity, treat as EOF.
            if (_clip == null)
            {
                Volatile.Write(ref _eof, 1);
                return 0;
            }

            var remaining = _totalFloats - _position;

            if (remaining <= 0)
            {
                Volatile.Write(ref _eof, 1);
                return 0;
            }

            var count = (int)Math.Min(buffer.Length, remaining);

            // Copy from pre-read audio data — no Unity API calls on the audio thread.
            _audioData.AsSpan(_position, count).CopyTo(buffer);
            _position += count;

            return count;
        }

        /// <inheritdoc/>
        public void SeekFrame(long frame)
        {
            if (_clip == null)
                return;

            var newPosition = (long)(frame * _clip.channels);
            _position = (int)Math.Clamp(newPosition, 0, _totalFloats);
            Volatile.Write(ref _eof, 0);
        }
    }
}
