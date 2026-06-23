//! Thread-local error slot for C FFI error reporting.
//!
//! C callers call a function, check `UAP_HasError()`, and if true,
//! retrieve the message via `UAP_GetError()`.

use std::cell::RefCell;
use std::ffi::CString;
use std::fmt::Display;
use std::os::raw::c_char;
use std::ptr::null;

thread_local! {
    static ERROR_INFO: RefCell<Option<ErrorInfo>> = RefCell::new(None);
}

#[derive(Clone)]
pub struct ErrorInfo {
    pub msg: CString,
}

pub fn has_error() -> bool {
    ERROR_INFO.with_borrow(|it| it.is_some())
}

pub fn with_error<T>(f: impl FnOnce(&Option<ErrorInfo>) -> T) -> T {
    ERROR_INFO.with_borrow(|it| f(it))
}

pub fn set_error(error: impl Display) {
    let message = CString::new(format!("{error}")).unwrap();
    ERROR_INFO.replace(Some(ErrorInfo { msg: message }));
}

pub fn clear_error() {
    ERROR_INFO.set(None);
}

// ---------------------------------------------------------------------------
// Exported C API
// ---------------------------------------------------------------------------

/// Returns true if the last FFI call on this thread produced an error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_HasError() -> bool {
    ERROR_INFO.with(|e| e.borrow().is_some())
}

/// Returns a pointer to a null-terminated error message string, or null if
/// there is no error.
///
/// The returned pointer is valid until the next FFI call on this thread
/// (most FFI functions clear the error slot on entry). Calling this function
/// does **not** clear the error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_GetError() -> *const c_char {
    if !has_error() {
        return null();
    }

    with_error(|it| {
        if let Some(error_info) = it {
            error_info.msg.as_ptr()
        } else {
            null()
        }
    })
}
