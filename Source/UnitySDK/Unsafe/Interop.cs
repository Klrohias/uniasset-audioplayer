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
        /// Copies the last error message into the caller-provided buffer.
        /// Writes at most <c>buffer_size - 1</c> bytes plus a null terminator.
        /// Returns the number of bytes written (excluding null), or 0 if no error.
        /// The error slot is cleared after this call.
        /// </summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        [return: NativeTypeName("uint32_t")]
        public static extern uint UAP_GetError(
            [NativeTypeName("char *")] sbyte* buffer,
            [NativeTypeName("uint32_t")] uint buffer_size);

        // ==================================================================
        // Player Lifecycle (2 functions)
        // ==================================================================

        /// <summary>
        /// Create a new AudioPlayer and open the platform audio device.
        /// Returns a handle on success, null on failure — check error.
        /// </summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        public static extern void* UAP_AudioPlayer_New();

        /// <summary>
        /// Destroy an AudioPlayer. Stops playback and releases the audio device.
        /// No-op on null handle.
        /// </summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        public static extern void UAP_AudioPlayer_Destroy(void* handle);

        // ==================================================================
        // Device Query (1 function)
        // ==================================================================

        /// <summary>
        /// Query the audio format (sample rate / channel count) of the output device.
        /// No-op if any pointer is null.
        /// </summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        public static extern void UAP_AudioPlayer_Format(
            void* handle,
            [NativeTypeName("uint32_t *")] uint* out_sample_rate,
            [NativeTypeName("uint16_t *")] ushort* out_channels);

        // ==================================================================
        // Stream Management (3 functions)
        // ==================================================================

        /// <summary>
        /// Add an audio stream to the player. Returns a PlayHandle, or null on error.
        /// </summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        public static extern void* UAP_AudioPlayer_AddStream(void* handle, void* stream_handle);

        /// <summary>
        /// Remove all streams that have reached EOF. Call periodically to free resources.
        /// </summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        public static extern void UAP_AudioPlayer_CleanupEof(void* handle);

        /// <summary>
        /// Return the number of currently active streams. Returns 0 for null handles.
        /// </summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        [return: NativeTypeName("uint32_t")]
        public static extern uint UAP_AudioPlayer_StreamCount(void* handle);

        // ==================================================================
        // Device Control (3 functions)
        // ==================================================================

        /// <summary>Pause the audio device. Returns true on success.</summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        [return: NativeTypeName("bool")]
        public static extern byte UAP_AudioPlayer_Pause(void* handle);

        /// <summary>Resume the audio device. Returns true on success.</summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        [return: NativeTypeName("bool")]
        public static extern byte UAP_AudioPlayer_Resume(void* handle);

        /// <summary>Stop playback and close the audio device. Returns true on success.</summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        [return: NativeTypeName("bool")]
        public static extern byte UAP_AudioPlayer_Stop(void* handle);

        // ==================================================================
        // AudioStream (2 functions)
        // ==================================================================

        /// <summary>
        /// Create a new audio stream backed by C callbacks.
        /// All five callbacks must be non-null. Returns null on error.
        /// </summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        public static extern void* UAP_AudioStream_Create(
            void* user_data,
            void* read_fn,
            void* seek_fn,
            void* is_eof_fn,
            void* channels_fn,
            void* sample_rate_fn);

        /// <summary>
        /// Destroy an audio stream handle. Drops the C caller's reference.
        /// The stream remains alive as long as any PlayHandle references exist.
        /// No-op on null handle.
        /// </summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        public static extern void UAP_AudioStream_Destroy(void* handle);

        // ==================================================================
        // PlayHandle (9 functions)
        // ==================================================================

        /// <summary>
        /// Destroy a PlayHandle. Drops the C caller's reference.
        /// The handle remains valid to the mixer until cleanup.
        /// No-op on null handle.
        /// </summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        public static extern void UAP_PlayHandle_Destroy(void* handle);

        /// <summary>Pause playback for this stream. No-op on null handle.</summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        public static extern void UAP_PlayHandle_Pause(void* handle);

        /// <summary>Resume playback for this stream. No-op on null handle.</summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        public static extern void UAP_PlayHandle_Resume(void* handle);

        /// <summary>Returns true if the stream is currently paused. False for null handles.</summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        [return: NativeTypeName("bool")]
        public static extern byte UAP_PlayHandle_IsPaused(void* handle);

        /// <summary>Returns true if the stream is still alive. False for null handles.</summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        [return: NativeTypeName("bool")]
        public static extern byte UAP_PlayHandle_IsAlive(void* handle);

        /// <summary>Signal the stream to stop. No-op on null handle.</summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        public static extern void UAP_PlayHandle_Stop(void* handle);

        /// <summary>Set the volume for this stream, clamped to [0.0, 1.0]. No-op on null handle.</summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        public static extern void UAP_PlayHandle_SetVolume(void* handle, float volume);

        /// <summary>Return the current volume in [0.0, 1.0]. Returns 0.0 for null handles.</summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        public static extern float UAP_PlayHandle_GetVolume(void* handle);

        /// <summary>
        /// Seek the stream to the given frame position.
        /// Returns true on success. On failure check error.
        /// Returns false for null handles.
        /// </summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        [return: NativeTypeName("bool")]
        public static extern byte UAP_PlayHandle_Seek(void* handle, ulong frame);

        /// <summary>
        /// Install or remove a pre-mix modifier callback for this stream.
        /// Pass null callback to remove. user_data must remain valid while modifier is installed.
        /// Returns true on success, false on error (null handle).
        /// </summary>
        [DllImport(LibraryName, CallingConvention = CallingConvention.Cdecl, ExactSpelling = true)]
        [return: NativeTypeName("bool")]
        public static extern byte UAP_PlayHandle_SetModifier(
            void* handle,
            void* callback,
            void* user_data);
    }
}
