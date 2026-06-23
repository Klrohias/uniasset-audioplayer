using System.Runtime.InteropServices;

namespace Uniasset.AudioPlayer.Unsafe
{
    public static unsafe partial class Interop
    {
        // ==================================================================
        // Error (2 functions)
        // ==================================================================

        /// <summary>Returns true if the last FFI call on this thread produced an error.</summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        [return: NativeTypeName("bool")]
        public static extern byte UAP_HasError();

        /// <summary>
        /// Returns a pointer to a null-terminated error message string, or null if
        /// there is no error. The pointer is valid until the next FFI call on this
        /// thread (most FFI functions clear the error slot on entry). Calling this
        /// function does <b>not</b> clear the error.
        /// </summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        [return: NativeTypeName("const char *")]
        public static extern byte* UAP_GetError();

        // ==================================================================
        // Player Lifecycle (2 functions)
        // ==================================================================

        /// <summary>
        /// Create a new AudioPlayer and open the platform audio device.
        /// Returns a handle on success, or null on failure.
        /// </summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        public static extern void* UAP_AudioPlayer_New();

        /// <summary>
        /// Destroy an AudioPlayer. Drops the C caller's reference.
        /// When the last reference is dropped, playback stops and the audio device
        /// is released.
        /// </summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        public static extern void UAP_AudioPlayer_Destroy(void* handle);

        // ==================================================================
        // Device Query (1 function)
        // ==================================================================

        /// <summary>
        /// Query the audio format (sample rate / channel count) of the output device.
        /// </summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        public static extern void UAP_AudioPlayer_Format(
            void* handle,
            [NativeTypeName("uint32_t *")] uint* out_sample_rate,
            [NativeTypeName("uint16_t *")] ushort* out_channels);

        // ==================================================================
        // Stream Management (2 functions)
        // ==================================================================

        /// <summary>
        /// Add an audio stream to the player. <c>stream</c> must point to a valid
        /// <see cref="NativeAudioStream"/> struct that the caller keeps alive.
        /// If <c>playImmediate</c> is non-zero, the stream begins playing
        /// immediately; otherwise it is added in a paused state.
        /// Returns a <see cref="UnsafePlayHandle"/>.
        /// </summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        public static extern void* UAP_AudioPlayer_AddStream(
            void* handle,
            void* stream,
            [NativeTypeName("uint8_t")] byte playImmediate);

        /// <summary>
        /// Return the number of currently active streams.
        /// </summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        [return: NativeTypeName("uint32_t")]
        public static extern uint UAP_AudioPlayer_StreamCount(void* handle);

        // ==================================================================
        // Device Control (2 functions)
        // ==================================================================

        /// <summary>
        /// Pause playback on the audio device.
        /// On failure, the error is reported via <see cref="UAP_HasError"/> /
        /// <see cref="UAP_GetError"/>.
        /// </summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        public static extern void UAP_AudioPlayer_Pause(void* handle);

        /// <summary>
        /// Resume playback on the audio device.
        /// On failure, the error is reported via <see cref="UAP_HasError"/> /
        /// <see cref="UAP_GetError"/>.
        /// </summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        public static extern void UAP_AudioPlayer_Resume(void* handle);

        // ==================================================================
        // PlayHandle (9 functions)
        // ==================================================================

        /// <summary>
        /// Destroy a PlayHandle. Drops the C caller's reference.
        /// The mixer holds its own references independently; the underlying
        /// stream continues playing until it reaches EOF or
        /// <see cref="UAP_PlayHandle_Stop"/> is called.
        /// </summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        public static extern void UAP_PlayHandle_Destroy(void* handle);

        /// <summary>Pause playback for this stream. No-op if the stream is no longer alive.</summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        public static extern void UAP_PlayHandle_Pause(void* handle);

        /// <summary>Resume playback for this stream. No-op if the stream is no longer alive.</summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        public static extern void UAP_PlayHandle_Resume(void* handle);

        /// <summary>Returns true if the stream is currently paused.</summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        [return: NativeTypeName("bool")]
        public static extern byte UAP_PlayHandle_IsPaused(void* handle);

        /// <summary>Returns true if the stream is still active in the mixer.</summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        [return: NativeTypeName("bool")]
        public static extern byte UAP_PlayHandle_IsAlive(void* handle);

        /// <summary>
        /// Signal the stream to stop. The mixer will remove it from the
        /// active stream set once it observes the stop signal.
        /// No-op if the stream is no longer alive.
        /// </summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        public static extern void UAP_PlayHandle_Stop(void* handle);

        /// <summary>Set the volume for this stream. <c>volume</c> is clamped to [0.0, 1.0].</summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        public static extern void UAP_PlayHandle_SetVolume(void* handle, float volume);

        /// <summary>Return the current volume in [0.0, 1.0].</summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        public static extern float UAP_PlayHandle_GetVolume(void* handle);

        /// <summary>
        /// Seek the stream to the given frame position.
        /// On failure, the error is reported via <see cref="UAP_HasError"/> /
        /// <see cref="UAP_GetError"/>.
        /// </summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        public static extern void UAP_PlayHandle_Seek(void* handle, ulong frame);

        /// <summary>
        /// Install a pre-mix modifier callback for this stream.
        /// <c>modifier</c> points to a <see cref="NativeModifier"/> containing
        /// the callback function pointer and opaque user data. The callback is
        /// called on the audio thread with the interleaved PCM buffer for this
        /// stream before it is mixed into the output.
        /// The callback must be wait-free. <c>user_data</c> must remain valid
        /// for as long as the modifier is installed.
        /// </summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        public static extern void UAP_PlayHandle_SetModifier(
            void* handle,
            [NativeTypeName("const NativeModifier *")] NativeModifier* modifier);
    }
}
