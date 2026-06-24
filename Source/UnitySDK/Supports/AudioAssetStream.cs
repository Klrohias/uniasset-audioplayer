using System;
using Uniasset.Audio;

namespace Uniasset.AudioPlayer
{
    /// <summary>
    /// An <see cref="IAudioStream"/> backed by a <see cref="AudioAsset"/>.
    /// </summary>
    /// <remarks>
    /// The <see cref="AudioAsset"/> must be fully loaded (via <c>Load</c> or
    /// <c>LoadIO</c>) before use. All audio-thread methods are wait-free
    /// after the asset is prepared.
    /// </remarks>
    public sealed class AudioAssetStream : IAudioStream
    {
        private readonly AudioAsset _asset;

        public AudioAssetStream(AudioAsset asset)
        {
            if (asset.SampleFormat != SampleFormat.Float)
            {
                throw new NotSupportedException("Non f32 audio asset is not supported");
            }
            _asset = asset ?? throw new ArgumentNullException(nameof(asset));
        }

        // ==================================================================
        // IAudioStream (audio thread — wait-free)
        // ==================================================================

        /// <inheritdoc />
        public int ReadF32(Span<float> buffer)
        {
            var channels = _asset.ChannelCount;
            if (channels == 0)
                return 0;

            var frameCount = buffer.Length / channels;
            if (frameCount == 0)
                return 0;

            var framesRead = _asset.Read<float>(buffer, frameCount);
            return framesRead * channels;
        }

        /// <inheritdoc />
        public bool IsEof => _asset.Tell() >= _asset.FrameCount;

        /// <inheritdoc />
        public ushort Channels => (ushort)_asset.ChannelCount;

        /// <inheritdoc />
        public uint SampleRate => (uint)_asset.SampleRate;

        // ==================================================================
        // IAudioStream (control thread — may block)
        // ==================================================================

        /// <inheritdoc />
        public void SeekFrame(long frame)
        {
            _asset.Seek(frame);
        }
    }
}
