using System;
using System.Runtime.InteropServices;
using AOT;
using Uniasset.AudioPlayer;

namespace Uniasset.AudioPlayer.Unsafe
{
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
        // Static singleton delegate + pre-computed function pointer
        // ==================================================================

        private static readonly NativeModifierFn s_delegate = Bridge;
        private static readonly void* s_ptr;

        static ModifierBridge()
        {
            s_ptr = Marshal.GetFunctionPointerForDelegate(s_delegate).ToPointer();
        }

        // ==================================================================
        // Audio-thread callback
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

        // ==================================================================
        // Public helper: install or remove a modifier
        // ==================================================================

        /// <summary>
        /// Install a managed <paramref name="callback"/> as the pre-mix modifier
        /// on <paramref name="handle"/>, or remove it if null.
        /// Returns a <see cref="ModifierBinding"/> that the caller must dispose when
        /// the modifier is no longer needed, or null if no modifier was installed.
        /// </summary>
        public static ModifierBinding? Install(UnsafePlayHandle handle, ModifierCallback callback)
        {
            if (callback == null)
            {
                // Pass null to the native side — it sets the modifier to None.
                handle.SetModifier(null);
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
}
