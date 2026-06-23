using System;
using System.Runtime.InteropServices;

namespace Uniasset.AudioPlayer.Unsafe
{
    /// <summary>
    /// Exception thrown when a native library call produces an error.
    /// The error message is retrieved from the native side via <c>UAP_GetError</c>.
    /// </summary>
    public class NativeException : Exception
    {
        public NativeException(string message) : base(message)
        {
        }

        /// <summary>
        /// Checks the thread-local error slot and throws a <see cref="NativeException"/>
        /// if an error is pending.
        /// </summary>
        public static unsafe void ThrowIfNeeded()
        {
            if (Interop.UAP_HasError() == 0)
                return;

            var ptr = Interop.UAP_GetError();
            if (ptr == null)
                return;

            var message = Marshal.PtrToStringAnsi((IntPtr)ptr);
            if (string.IsNullOrWhiteSpace(message))
                return;

            throw new NativeException(message);
        }
    }
}
