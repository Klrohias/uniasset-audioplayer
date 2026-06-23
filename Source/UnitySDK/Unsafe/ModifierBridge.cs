using System;
using System.Runtime.InteropServices;
using AOT;

namespace Uniasset.AudioPlayer.Unsafe
{
    /// <summary>
    /// <c>#[repr(C)]</c> struct matching the native <c>NativeModifier</c>.
    /// Contains a callback function pointer and opaque user data pointer.
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    public struct NativeModifier
    {
        /// <summary>
        /// Callback: <c>void (*)(float* buffer, uint64_t sample_count, void* user_data)</c>.
        /// Must be wait-free — runs on the audio thread.
        /// </summary>
        public void* callback;

        /// <summary>Opaque user data passed to every invocation.</summary>
        public void* userData;
    }

    /// <summary>
    /// Bridges a managed <see cref="ModifierCallback"/> to the native
    /// <c>NativeModifier</c> struct. Contains all pointer/unsafe code related to
    /// modifier installation so that <see cref="PlayHandle"/> remains safe.
    /// </summary>
    internal static unsafe class ModifierBridge
    {
        // ==================================================================
        // Native callback delegate (matching Rust FFI)
        // ==================================================================

        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate void NativeModifierFn(float* buffer, ulong sampleCount, void* userData);

        // ==================================================================
        // Static singleton delegates + pre-computed function pointers
        // ==================================================================

        private static readonly NativeModifierFn s_delegate = Bridge;
        private static readonly NativeModifierFn s_noopDelegate = Noop;
        private static readonly void* s_ptr;
        private static readonly void* s_noopPtr;

        static ModifierBridge()
        {
            s_ptr = Marshal.GetFunctionPointerForDelegate(s_delegate).ToPointer();
            s_noopPtr = Marshal.GetFunctionPointerForDelegate(s_noopDelegate).ToPointer();
        }

        // ==================================================================
        // Audio-thread callbacks
        // ==================================================================

        /// <summary>
        /// Recovers the managed <see cref="ModifierCallback"/> from the GCHandle,
        /// wraps the native buffer in a <see cref="Span{T}"/>, and invokes the callback.
        /// </summary>
        [MonoPInvokeCallback(typeof(NativeModifierFn))]
        private static void Bridge(float* buffer, ulong sampleCount, void* userData)
        {
            try
            {
                var handle = GCHandle.FromIntPtr(new IntPtr(userData));
                var callback = (ModifierCallback)handle.Target;
                if (callback == null)
                    return;

                var span = new Span<float>(buffer, checked((int)sampleCount));
                callback(span);
            }
            catch
            {
                // Audio thread must never crash.
            }
        }

        /// <summary>
        /// No-op callback used to safely replace a managed modifier before
        /// freeing its GCHandle.
        /// </summary>
        [MonoPInvokeCallback(typeof(NativeModifierFn))]
        private static void Noop(float* buffer, ulong sampleCount, void* userData)
        {
            // Intentionally empty — used to neutralize an installed modifier
            // so the GCHandle can be freed without the audio thread accessing it.
        }

        // ==================================================================
        // Public helper: install or remove a modifier
        // ==================================================================

        /// <summary>
        /// Install a managed <paramref name="callback"/> as the pre-mix modifier
        /// on <paramref name="handle"/>, or remove it if null.
        /// Returns a <see cref="ModifierBinding"/> that the caller must free when
        /// the modifier is no longer needed, or null if no modifier was installed.
        /// </summary>
        public static ModifierBinding? Install(UnsafePlayHandle handle, ModifierCallback? callback)
        {
            if (callback == null)
            {
                // Replace the current modifier with a no-op so the audio thread
                // stops calling the managed callback, then we can safely free the
                // old GCHandle. No need to create a new binding.
                var noopModifier = new NativeModifier
                {
                    callback = s_noopPtr,
                    userData = null,
                };
                handle.SetModifier(&noopModifier);
                return null;
            }

            var gcHandle = GCHandle.Alloc(callback);
            var userData = GCHandle.ToIntPtr(gcHandle).ToPointer();
            var modifier = new NativeModifier
            {
                callback = s_ptr,
                userData = userData,
            };
            handle.SetModifier(&modifier);
            return new ModifierBinding(gcHandle);
        }
    }

    /// <summary>
    /// Owns the <see cref="GCHandle"/> for a modifier callback installed on
    /// a native <c>UAP_PlayHandle</c>. Call <see cref="Free"/> to release.
    /// </summary>
    public struct ModifierBinding
    {
        private GCHandle _gcHandle;

        internal ModifierBinding(GCHandle gcHandle)
        {
            _gcHandle = gcHandle;
        }

        /// <summary>Release the GCHandle that pins the managed callback.</summary>
        public void Free()
        {
            if (_gcHandle.IsAllocated)
                _gcHandle.Free();
        }
    }
}
