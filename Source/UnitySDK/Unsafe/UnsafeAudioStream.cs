using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using AOT;

namespace Uniasset.AudioPlayer.Unsafe
{
    /// <summary>
    /// Raw handle to a native <c>NativeAudioStream</c>. Wraps an opaque <c>void*</c>.
    /// Contains the static callback delegates that bridge managed <see cref="IAudioStream"/>
    /// instances to the C function pointers expected by the native audio thread.
    /// </summary>
    /// <remarks>
    /// All callbacks except <c>SeekCallback</c> are invoked from the real-time audio thread
    /// and MUST be wait-free: no locks, no allocations, no blocking I/O.
    /// <c>SeekCallback</c> is called from the control thread and may block.
    /// </remarks>
    [EditorBrowsable(EditorBrowsableState.Never)]
    public readonly unsafe struct UnsafeAudioStream
    {
        /// <summary>The opaque native handle. Null if creation failed.</summary>
        public readonly void* Instance;

        // ==================================================================
        // C callback delegate types (matching Rust FFI signatures exactly)
        // ==================================================================

        /// <summary>
        /// ReadFn: reads up to <c>frame_count</c> frames into <c>buffer</c> (interleaved f32).
        /// Returns the number of <b>samples</b> written (frames × channels), or 0 at EOF.
        /// Called from the audio thread — MUST be wait-free.
        /// </summary>
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        public delegate ulong NativeReadFn(void* userData, float* buffer, ulong frameCount);

        /// <summary>
        /// SeekFn: seek to the given absolute frame position. Returns true on success.
        /// Called from the control thread — may block.
        /// </summary>
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        [return: MarshalAs(UnmanagedType.I1)]
        public delegate bool NativeSeekFn(void* userData, ulong frame);

        /// <summary>
        /// IsEofFn: returns true if the stream has reached its end.
        /// Called from the audio thread — MUST be wait-free.
        /// </summary>
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        [return: MarshalAs(UnmanagedType.I1)]
        public delegate bool NativeIsEofFn(void* userData);

        /// <summary>
        /// ChannelsFn: returns the number of channels (1 = mono, 2 = stereo).
        /// Called from the audio thread — MUST be wait-free.
        /// </summary>
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        public delegate ushort NativeChannelsFn(void* userData);

        /// <summary>
        /// SampleRateFn: returns the sample rate in Hz (e.g. 44100, 48000).
        /// Called from the audio thread — MUST be wait-free.
        /// </summary>
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        public delegate uint NativeSampleRateFn(void* userData);

        // ==================================================================
        // Static delegate instances (allocated once, never GC'd)
        // ==================================================================

        private static readonly NativeReadFn s_readDelegate = ReadCallback;
        private static readonly NativeSeekFn s_seekDelegate = SeekCallback;
        private static readonly NativeIsEofFn s_isEofDelegate = IsEofCallback;
        private static readonly NativeChannelsFn s_channelsDelegate = ChannelsCallback;
        private static readonly NativeSampleRateFn s_sampleRateDelegate = SampleRateCallback;

        // Pre-computed function pointers for passing to native code.
        private static readonly void* s_readFnPtr;
        private static readonly void* s_seekFnPtr;
        private static readonly void* s_isEofFnPtr;
        private static readonly void* s_channelsFnPtr;
        private static readonly void* s_sampleRateFnPtr;

        static UnsafeAudioStream()
        {
            s_readFnPtr = Marshal.GetFunctionPointerForDelegate(s_readDelegate).ToPointer();
            s_seekFnPtr = Marshal.GetFunctionPointerForDelegate(s_seekDelegate).ToPointer();
            s_isEofFnPtr = Marshal.GetFunctionPointerForDelegate(s_isEofDelegate).ToPointer();
            s_channelsFnPtr = Marshal.GetFunctionPointerForDelegate(s_channelsDelegate).ToPointer();
            s_sampleRateFnPtr = Marshal.GetFunctionPointerForDelegate(s_sampleRateDelegate).ToPointer();
        }

        // ==================================================================
        // Callback implementations
        // ==================================================================

        /// <summary>
        /// Audio-thread read callback. Recovers the <see cref="IAudioStream"/> from the
        /// GCHandle stored in <c>userData</c> and delegates to <c>ReadF32</c>.
        /// Returns 0 on any error (treated as EOF by the mixer).
        /// </summary>
        [MonoPInvokeCallback(typeof(NativeReadFn))]
        private static ulong ReadCallback(void* userData, float* buffer, ulong frameCount)
        {
            try
            {
                var handle = GCHandle.FromIntPtr(new IntPtr(userData));
                var stream = (IAudioStream)handle.Target;
                if (stream == null)
                    return 0;

                var channels = stream.Channels;
                var sampleCount = checked((int)(frameCount * channels));
                var span = new Span<float>(buffer, sampleCount);
                return (ulong)stream.ReadF32(span);
            }
            catch
            {
                // Audio thread must never crash. Return 0 → EOF.
                return 0;
            }
        }

        /// <summary>
        /// Control-thread seek callback. Recovers the <see cref="IAudioStream"/> and
        /// delegates to <c>SeekFrame</c>. This path may block.
        /// </summary>
        [MonoPInvokeCallback(typeof(NativeSeekFn))]
        private static bool SeekCallback(void* userData, ulong frame)
        {
            try
            {
                var handle = GCHandle.FromIntPtr(new IntPtr(userData));
                var stream = (IAudioStream)handle.Target;
                if (stream == null)
                    return false;

                stream.SeekFrame((long)frame);
                return true;
            }
            catch
            {
                return false;
            }
        }

        /// <summary>
        /// Audio-thread EOF check. Returns true if the stream has ended.
        /// Defaults to true (EOF) on any error.
        /// </summary>
        [MonoPInvokeCallback(typeof(NativeIsEofFn))]
        private static bool IsEofCallback(void* userData)
        {
            try
            {
                var handle = GCHandle.FromIntPtr(new IntPtr(userData));
                var stream = (IAudioStream)handle.Target;
                if (stream == null)
                    return true;

                return stream.IsEof;
            }
            catch
            {
                return true;
            }
        }

        /// <summary>
        /// Audio-thread channel count query. Returns the number of channels.
        /// Defaults to 2 (stereo) on any error.
        /// </summary>
        [MonoPInvokeCallback(typeof(NativeChannelsFn))]
        private static ushort ChannelsCallback(void* userData)
        {
            try
            {
                var handle = GCHandle.FromIntPtr(new IntPtr(userData));
                var stream = (IAudioStream)handle.Target;
                if (stream == null)
                    return 2;

                return stream.Channels;
            }
            catch
            {
                return 2;
            }
        }

        /// <summary>
        /// Audio-thread sample rate query. Returns the sample rate in Hz.
        /// Defaults to 44100 on any error.
        /// </summary>
        [MonoPInvokeCallback(typeof(NativeSampleRateFn))]
        private static uint SampleRateCallback(void* userData)
        {
            try
            {
                var handle = GCHandle.FromIntPtr(new IntPtr(userData));
                var stream = (IAudioStream)handle.Target;
                if (stream == null)
                    return 44100;

                return stream.SampleRate;
            }
            catch
            {
                return 44100;
            }
        }

        // ==================================================================
        // Construction
        // ==================================================================

        private UnsafeAudioStream(void* instance)
        {
            Instance = instance;
        }

        /// <summary>
        /// Create a native audio stream backed by the managed <see cref="IAudioStream"/>
        /// identified by the given GCHandle (passed as <c>userData</c> to all callbacks).
        /// Throws <see cref="NativeException"/> on failure.
        /// </summary>
        public static UnsafeAudioStream Create(IntPtr userData)
        {
            var result = Interop.UAP_AudioStream_Create(
                userData.ToPointer(),
                s_readFnPtr,
                s_seekFnPtr,
                s_isEofFnPtr,
                s_channelsFnPtr,
                s_sampleRateFnPtr);

            NativeException.ThrowIfNeeded();
            if (result == null)
                throw new NativeException("Failed to create AudioStream: native returned null");

            return new UnsafeAudioStream(result);
        }

        /// <summary>
        /// Destroy the native stream handle, dropping the C caller's reference.
        /// The stream remains alive as long as any PlayHandle references exist.
        /// Never throws.
        /// </summary>
        public void Destroy()
        {
            if (Instance == null)
                return;
            Interop.UAP_AudioStream_Destroy(Instance);
            // Intentionally no ThrowIfNeeded.
        }
    }
}
