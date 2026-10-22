//! Single-instance guard: only one copy of rapcoding may run at a time.
//! Two instances would fight over the same global hotkeys and inject text
//! twice. The guard is released automatically when the process exits, even
//! on a crash (Windows: the OS owns the mutex; elsewhere: best-effort file).

use crate::error::{Error, Result};

/// Held for the lifetime of the process.
pub struct Singleton {
    #[cfg(target_os = "windows")]
    handle: windows_sys::Win32::Foundation::HANDLE,
    #[cfg(not(target_os = "windows"))]
    lock_path: std::path::PathBuf,
}

/// Try to become the single instance. `Ok(None)` = another instance runs.
pub fn acquire() -> Result<Option<Singleton>> {
    acquire_impl("rapcoding-single-instance")
}

fn acquire_impl(name: &str) -> Result<Option<Singleton>> {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
        use windows_sys::Win32::System::Threading::CreateMutexW;

        let wide: Vec<u16> = format!("Local\\{name}")
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        // bInitialOwner = 0: the guard is the handle we hold, not mutex
        // ownership, so it releases cleanly when the handle is closed and a
        // crashed process never leaves an "owned" (abandoned) mutex behind.
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, wide.as_ptr()) };
        if handle.is_null() {
            return Err(Error::Io(std::io::Error::last_os_error()));
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            // Another instance holds the mutex; close our duplicate handle.
            unsafe { CloseHandle(handle) };
            return Ok(None);
        }
        Ok(Some(Singleton { handle }))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let path = std::env::temp_dir().join(format!("{name}.lock"));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(_) => {
                let _ = std::fs::write(&path, std::process::id().to_string());
                Ok(Some(Singleton { lock_path: path }))
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(None),
            Err(e) => Err(Error::Io(e)),
        }
    }
}

impl Drop for Singleton {
    fn drop(&mut self) {
        #[cfg(target_os = "windows")]
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
        #[cfg(not(target_os = "windows"))]
        {
            // Best-effort: a stale lock after a hard kill is acceptable on
            // non-Windows targets (the primary target holds the mutex in the OS).
            let _ = std::fs::remove_file(&self.lock_path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_acquire_succeeds_second_is_rejected() {
        let name = format!("rapcoding-test-{}", std::process::id());
        let first = acquire_impl(&name).expect("first acquire must succeed");
        assert!(first.is_some(), "no other instance holds this name");
        let second = acquire_impl(&name).expect("second acquire must not error");
        assert!(
            second.is_none(),
            "a second instance must be rejected while the first is alive"
        );
        drop(first);
        // After release a new acquire must succeed again.
        let third = acquire_impl(&name).expect("re-acquire after drop");
        assert!(third.is_some());
    }
}
