use core::sync::atomic::{AtomicBool, Ordering};

pub struct Lock {
    inner: AtomicBool
}

impl Lock {
    pub const fn new() -> Self {
        Self {
            inner: AtomicBool::new(false)
        }
    }

    #[inline]
    pub fn lock(&self) {
        while self.inner.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
            core::hint::spin_loop();
        }
    }
    #[inline] pub fn unlock(&self) { self.inner.store(false, Ordering::Release); }
}

pub struct LockGuard<'a>(&'a Lock);
impl<'a> Drop for LockGuard<'a> { fn drop(&mut self) { self.0.unlock(); } }
#[inline] pub fn guard(lock: &Lock) -> LockGuard<'_> { lock.lock(); LockGuard(lock) }