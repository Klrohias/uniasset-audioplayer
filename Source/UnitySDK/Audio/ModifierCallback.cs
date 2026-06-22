using System;

namespace Uniasset.AudioPlayer
{
    /// <summary>
    /// A pre-mix DSP modifier callback. Called on the audio thread with a
    /// single stream's interleaved f32 PCM buffer, before it is mixed
    /// into the output. The callback may modify samples in-place.
    /// </summary>
    /// <param name="pcmBuffer">
    /// The interleaved f32 sample buffer for this stream.
    /// <c>pcmBuffer.Length</c> is the total number of f32 values (frames × channels).
    /// </param>
    /// <remarks>
    /// MUST be wait-free: no locks, no allocations, no blocking I/O.
    /// This runs on the real-time audio thread.
    /// </remarks>
    public delegate void ModifierCallback(Span<float> pcmBuffer);
}
