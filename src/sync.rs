use std::sync::{Mutex, MutexGuard};

/// Lock a mutex, recovering from poisoning instead of silently failing.
///
/// Rust poisons a mutex when a holder panics; the guarded data is still
/// intact. A daemon must keep operating when some things are wrong
/// (Hamilton 2 / Akin 2), so recover the guard and continue rather than
/// swallowing the error and going silently deaf.
pub(crate) fn lock_recover<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
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
}
