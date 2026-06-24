using System.Runtime.InteropServices;

namespace Uniasset.AudioPlayer.Unsafe
{
    /// <summary>
    /// <c>#[repr(C)]</c> struct matching the native <c>NativeModifier</c>.
    /// Contains a callback function pointer and opaque user data pointer.
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    public unsafe struct NativeModifier
    {
        /// <summary>
        /// Callback: <c>void (*)(float* buffer, uint64_t sample_count, void* user_data)</c>.
        /// Must be wait-free — runs on the audio thread.
        /// </summary>
        public void* callback;

        /// <summary>Opaque user data passed to every invocation.</summary>
        public void* userData;
    }
}
