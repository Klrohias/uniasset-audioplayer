//! Opaque handle pattern and utilities for crossing the C FFI boundary.
//!
//! Follows the same pattern as `uniasset-bridge`: all Rust objects passed
//! to C are wrapped in `Box<Arc<T>>` and leaked to raw pointers.

use std::ffi::c_void;

/// Opaque handle passed across the C FFI boundary.
///
/// Internally a `Box<Arc<T>>` that has been leaked via `Box::into_raw`.
/// The C side treats it as an opaque pointer.
pub type NativeHandle = *const c_void;

/// Extension trait for converting between `Arc<T>` and opaque `NativeHandle`.
pub trait NativeHandleExts: Sized {
    /// Consume self and return an opaque handle.
    fn into_handle(self) -> NativeHandle;

    /// Reconstitute a shared reference from an opaque handle without consuming it.
    ///
    /// # Safety
    ///
    /// `handle` must be a valid `NativeHandle` previously produced by
    /// `into_handle` for this type, and must not have been destroyed.
    unsafe fn from_handle(handle: NativeHandle) -> &'static Self;
}

/// Implements `NativeHandleExts` for `Arc<T>` using `Box<Arc<T>>` encoding.
macro_rules! impl_native_handle {
    ($ty:ty) => {
        impl $crate::object::NativeHandleExts for std::sync::Arc<$ty> {
            fn into_handle(self) -> $crate::object::NativeHandle {
                Box::into_raw(Box::new(self)) as $crate::object::NativeHandle
            }

            unsafe fn from_handle(handle: $crate::object::NativeHandle) -> &'static Self {
                &*(handle as *const std::sync::Arc<$ty>)
            }
        }
    };
}

pub(crate) use impl_native_handle;

/// Runs a fallible operation, storing any error in the thread-local error slot.
///
/// On success, returns the operation's value. On failure, calls
/// [`set_error`](crate::error::set_error) with the Display output of the error
/// and returns `error_value` instead.
pub(crate) fn failible_to_native<T: Copy>(
    result: Result<T, impl std::fmt::Display>,
    error_value: T,
) -> T {
    match result {
        Ok(v) => v,
        Err(e) => {
            crate::error::set_error(&e.to_string());
            error_value
        }
    }
}
