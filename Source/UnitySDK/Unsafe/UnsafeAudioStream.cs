using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using AOT;

namespace Uniasset.AudioPlayer.Unsafe
{
    /// <summary>
    /// <c>#[repr(C)]</c> struct matching the native <c>NativeAudioStream</c>.
    /// Populate and pass a pointer to <c>UAP_AudioPlayer_AddStream</c>.
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct NativeAudioStream
    {
        /// <summary>Opaque user data passed to every callback.</summary>
        public IntPtr userData;

        /// <summary>ReadFn: interleaved f32 → sample count. Audio thread, wait-free.</summary>
        public IntPtr readFn;

        /// <summary>SeekFn: seek to frame → bool. Control thread, may block.</summary>
        public IntPtr seekFn;

        /// <summary>IsEofFn: → bool. Audio thread, wait-free.</summary>
        public IntPtr isEofFn;

        /// <summary>ChannelsFn: → u16. Audio thread, wait-free.</summary>
        public IntPtr channelsFn;

        /// <summary>SampleRateFn: → u32. Audio thread, wait-free.</summary>
        public IntPtr sampleRateFn;
    }

    /// <summary>
    /// Static holders for the callback delegates and their function pointers.
    /// Provides <see cref="CreateStream"/> to build a <see cref="NativeAudioStream"/>
    /// from a managed <see cref="IAudioStream"/> (via GCHandle).
    /// </summary>
    [EditorBrowsable(EditorBrowsableState.Never)]
    public static unsafe class AudioStreamFactory
    {
        // ==================================================================
        // C callback delegate types (matching Rust FFI signatures exactly)
        // ==================================================================

        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        public delegate ulong NativeReadFn(void* userData, float* buffer, ulong frameCount);

        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        [return: MarshalAs(UnmanagedType.I1)]
        public delegate bool NativeSeekFn(void* userData, ulong frame);

        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        [return: MarshalAs(UnmanagedType.I1)]
        public delegate bool NativeIsEofFn(void* userData);

        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        public delegate ushort NativeChannelsFn(void* userData);

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

        // Pre-computed function pointers.
        private static readonly IntPtr s_readFnPtr;
        private static readonly IntPtr s_seekFnPtr;
        private static readonly IntPtr s_isEofFnPtr;
        private static readonly IntPtr s_channelsFnPtr;
        private static readonly IntPtr s_sampleRateFnPtr;

        static AudioStreamFactory()
        {
            s_readFnPtr = Marshal.GetFunctionPointerForDelegate(s_readDelegate);
            s_seekFnPtr = Marshal.GetFunctionPointerForDelegate(s_seekDelegate);
            s_isEofFnPtr = Marshal.GetFunctionPointerForDelegate(s_isEofDelegate);
            s_channelsFnPtr = Marshal.GetFunctionPointerForDelegate(s_channelsDelegate);
            s_sampleRateFnPtr = Marshal.GetFunctionPointerForDelegate(s_sampleRateDelegate);
        }

        // ==================================================================
        // Factory
        // ==================================================================

        /// <summary>
        /// Build a <see cref="NativeAudioStream"/> that bridges the given
        /// <c>userData</c> (a GCHandle IntPtr) to the static callbacks.
        /// </summary>
        public static NativeAudioStream CreateStream(IntPtr userData)
        {
            return new NativeAudioStream
            {
                userData = userData,
                readFn = s_readFnPtr,
                seekFn = s_seekFnPtr,
                isEofFn = s_isEofFnPtr,
                channelsFn = s_channelsFnPtr,
                sampleRateFn = s_sampleRateFnPtr,
            };
        }

        // ==================================================================
        // Callback implementations
        // ==================================================================

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
                return 0;
            }
        }

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
    }
}
