using System;
using System.Runtime.InteropServices;

namespace Uniasset.AudioPlayer.Unsafe
{
    /// <summary>
    /// Owns the <see cref="GCHandle"/> for a modifier callback installed on
    /// a native <c>UAP_PlayHandle</c>. Disposing releases the handle.
    /// </summary>
    public struct ModifierBinding : IDisposable
    {
        private GCHandle _gcHandle;

        internal ModifierBinding(GCHandle gcHandle)
        {
            _gcHandle = gcHandle;
        }

        /// <summary>Release the GCHandle that pins the managed callback.</summary>
        public void Dispose()
        {
            if (_gcHandle.IsAllocated)
                _gcHandle.Free();
        }
    }
}
