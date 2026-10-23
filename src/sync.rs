use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Lock a mutex, recovering from poisoning instead of silently failing.
///
/// Rust poisons a mutex when a holder panics; the guarded data is still
/// intact. A daemon must keep operating when some things are wrong
/// (Hamilton 2 / Akin 2), so recover the guard and continue rather than
/// swallowing the error and going silently deaf.
pub(crate) fn lock_recover<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Read-lock an RwLock, recovering from poisoning (see [`lock_recover`]).
/// Used for the shared config: a panicking holder must not make the daemon
/// or the GUI silently deaf.
pub(crate) fn read_recover<T>(m: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    m.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Write-lock an RwLock, recovering from poisoning (see [`lock_recover`]).
pub(crate) fn write_recover<T>(m: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    m.write().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovers_after_poison() {
        let m = Mutex::new(42i32);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = m.lock().unwrap();
            panic!("poison");
        }));
        let g = lock_recover(&m);
        assert_eq!(*g, 42);
    }

    #[test]
    fn rwlock_recovers_after_poisoned_write() {
        // Arrange: poison via a panicking writer.
        let m = RwLock::new(7i32);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = m.write().unwrap();
            panic!("poison");
        }));
        // Act: recover the read guard after the poisoned write.
        let r = read_recover(&m);
        // Assert.
        assert_eq!(*r, 7);
        drop(r);
        // Act: write through the recovered write guard; drop it BEFORE
        // re-reading (a read while a write guard is alive is a deadlock).
        {
            let mut w = write_recover(&m);
            *w = 8;
        }
        assert_eq!(*read_recover(&m), 8);
    }
}
