using System;

namespace Uniasset.AudioPlayer
{
    /// <summary>
    /// A source of interleaved float32 PCM audio samples.
    /// Implement this interface to provide audio data to an <see cref="AudioPlayer"/>.
    /// </summary>
    /// <remarks>
    /// <see cref="ReadF32"/>, <see cref="IsEof"/>,
    /// <see cref="Channels"/>, and <see cref="SampleRate"/> are
    /// called from the real-time audio thread and MUST be <b>wait-free</b>:
    /// no locks, no allocations, no blocking I/O.
    /// <see cref="SeekFrame"/> is called from the control thread and may block.
    /// </remarks>
    public interface IAudioStream
    {
        /// <summary>
        /// Read interleaved f32 samples into <paramref name="buffer"/>.
        /// Returns the number of <b>samples</b> written (frames × channels),
        /// or 0 at EOF.
        /// </summary>
        /// <remarks>Called from the audio thread — MUST be wait-free.</remarks>
        int ReadF32(Span<float> buffer);

        /// <summary>
        /// Seek to the given absolute frame position.
        /// </summary>
        /// <remarks>Called from the control thread — may block.</remarks>
        void SeekFrame(long frame);

        /// <summary>
        /// Returns true if the stream has reached its end.
        /// </summary>
        /// <remarks>Called from the audio thread — MUST be wait-free.</remarks>
        bool IsEof { get; }

        /// <summary>
        /// The number of channels (1 = mono, 2 = stereo).
        /// </summary>
        /// <remarks>Called from the audio thread — MUST be wait-free.</remarks>
        ushort Channels { get; }

        /// <summary>
        /// The sample rate in Hz (e.g. 44100, 48000).
        /// </summary>
        /// <remarks>Called from the audio thread — MUST be wait-free.</remarks>
        uint SampleRate { get; }
    }
}
