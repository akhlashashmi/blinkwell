use std::sync::{Mutex, MutexGuard};

/// Take a lock, recovering instead of panicking if a thread panicked while
/// holding it. The guarded values in this app are plain data (settings,
/// status snapshots), so a recovered guard is safe to keep using - and one
/// bad tick should never escalate into a crash loop.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
