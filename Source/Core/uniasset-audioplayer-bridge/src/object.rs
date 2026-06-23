use std::{error::Error, ffi::c_void, sync::Arc};

use crate::error;

pub type NativeHandle = *const c_void;

pub trait NativeHandleExts {
    fn into_handle(self) -> NativeHandle;
    fn from_handle(handle: NativeHandle) -> Self;
}

impl<T: ?Sized> NativeHandleExts for Box<Arc<T>> {
    fn into_handle(self) -> NativeHandle {
        Box::into_raw(self) as NativeHandle
    }

    fn from_handle(handle: NativeHandle) -> Self {
        unsafe { Box::from_raw(handle as *mut Arc<T>) }
    }
}

pub(crate) fn failible_to_native<T, E: Error>(
    op: impl FnOnce() -> Result<T, E>,
    default: impl FnOnce() -> T,
) -> T {
    match op() {
        Ok(result) => result,
        Err(err) => {
            error::set_error(err);
            default()
        }
    }
}
