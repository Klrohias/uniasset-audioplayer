//! COM initialization guard for WASAPI.
//!
//! `ComGuard` is a RAII struct that calls `CoInitializeEx` on creation
//! and `CoUninitialize` on drop. A `thread_local!` ensures exactly one
//! `ComGuard` exists per thread — it is never moved across threads.

use std::cell::RefCell;

use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

thread_local! {
    /// One `ComGuard` per thread, lazily initialised on first COM call.
    static COM_GUARD: RefCell<Option<ComGuard>> = const { RefCell::new(None) };
}

/// RAII guard that initialises COM on the current thread.
///
/// Created once per thread via [`ensure_com_initialized`]. When the
/// thread exits, `thread_local!` destructors run, dropping the guard
/// and calling `CoUninitialize`.
///
/// COM reference-counts per thread: every successful `CoInitializeEx`
/// (`S_OK` or `S_FALSE`) must be paired with a `CoUninitialize`.
/// We only skip `CoUninitialize` if the call failed entirely (e.g.
/// `RPC_E_CHANGED_MODE`).
///
/// **`!Send`** — tied to the creating thread via [`thread_local!`].
struct ComGuard {
    /// `true` when `CoInitializeEx` succeeded (`S_OK` or `S_FALSE`).
    should_uninit: bool,
}

impl ComGuard {
    fn new() -> Self {
        // SAFETY: called from the thread that will use COM.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        // Both S_OK and S_FALSE are success and require a matching CoUninitialize.
        Self {
            should_uninit: hr.is_ok(),
        }
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.should_uninit {
            // SAFETY: paired with our own successful CoInitializeEx, on the same thread.
            unsafe {
                CoUninitialize();
            }
        }
    }
}

/// Ensure COM is initialised on the calling thread.
///
/// Idempotent — the first call on each thread creates a `ComGuard`
/// that lives for the remainder of the thread's lifetime.
///
/// If `CoInitializeEx` fails (e.g. `RPC_E_CHANGED_MODE`), the guard
/// is still stored so we don't retry, but subsequent COM operations
/// will fail as they should.
///
/// Uses `try_borrow_mut` to avoid a `RefCell` panic in the unlikely
/// event of re-entrancy (e.g. `ComGuard::new()` somehow triggering
/// another call to this function). If the guard is already borrowed,
/// we assume the outer call will finish initialisation.
pub(crate) fn ensure_com_initialized() {
    COM_GUARD.with(|guard| {
        if guard.borrow().is_none() {
            if let Ok(mut g) = guard.try_borrow_mut() {
                *g = Some(ComGuard::new());
            }
        }
    });
}
