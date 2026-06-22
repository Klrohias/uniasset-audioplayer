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

            const int maxLen = 512;
            var buffer = stackalloc sbyte[maxLen];
            var len = Interop.UAP_GetError(buffer, maxLen);

            if (len == 0)
                return;

            var message = Marshal.PtrToStringAnsi(new IntPtr(buffer), (int)len);
            if (string.IsNullOrWhiteSpace(message))
                return;

            throw new NativeException(message);
        }
    }
}
