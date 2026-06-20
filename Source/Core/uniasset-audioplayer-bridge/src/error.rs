//! Thread-local error slot for C FFI error reporting.
//!
//! C callers call a function, check `UAP_HasError()`, and if true,
//! retrieve the message via `UAP_GetError()`.

use std::cell::RefCell;
use std::os::raw::c_char;

thread_local! {
    static ERROR_INFO: RefCell<Option<String>> = RefCell::new(None);
}

/// Clear the thread-local error slot. Called at the start of every
/// fallible `extern "C"` function.
pub(crate) fn clear_error() {
    ERROR_INFO.with(|e| *e.borrow_mut() = None);
}

/// Store an error message in the thread-local error slot.
pub(crate) fn set_error(msg: &str) {
    ERROR_INFO.with(|e| *e.borrow_mut() = Some(msg.to_string()));
}

// ---------------------------------------------------------------------------
// Exported C API
// ---------------------------------------------------------------------------

/// Returns true if the last FFI call on this thread produced an error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_HasError() -> bool {
    ERROR_INFO.with(|e| e.borrow().is_some())
}

/// Copies the last error message into the caller-provided `buffer`.
///
/// Writes at most `buffer_size - 1` bytes followed by a null terminator.
/// Returns the number of bytes written (excluding null terminator), or 0
/// if there is no error.
///
/// The error slot is **cleared** after this call.
///
/// # Safety
///
/// `buffer` must be a valid pointer to at least `buffer_size` bytes of
/// writable memory. `buffer_size` must be accurate.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn UAP_GetError(buffer: *mut c_char, buffer_size: u32) -> u32 {
    if buffer.is_null() || buffer_size == 0 {
        // Still clear the error even if buffer is invalid.
        ERROR_INFO.with(|e| *e.borrow_mut() = None);
        return 0;
    }

    ERROR_INFO.with(|e| {
        let mut slot = e.borrow_mut();
        match slot.as_ref() {
            Some(msg) => {
                let buf = unsafe {
                    std::slice::from_raw_parts_mut(buffer as *mut u8, buffer_size as usize)
                };
                let max_len = buffer_size as usize - 1;
                let src = msg.as_bytes();
                let len = src.len().min(max_len);
                buf[..len].copy_from_slice(&src[..len]);
                buf[len] = 0; // null terminator
                *slot = None; // clear after reading
                len as u32
            }
            None => 0,
        }
    })
}
