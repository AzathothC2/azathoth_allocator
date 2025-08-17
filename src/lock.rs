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
    pub fn try_lock(&self) -> bool {
        self.inner
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    }
    #[inline]
    pub fn lock(&self) {
        let mut spins = 0u32;
        while self.inner.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            spins += 1;
            core::hint::spin_loop();
            if spins >= 64 {
                spins = 0;
            }
        }
    }

    pub fn guard(&self) -> LockGuard<'_> {
        self.lock();
        LockGuard(self)
    }

    #[inline] pub fn unlock(&self) { self.inner.store(false, Ordering::Release); }
}

pub struct LockGuard<'a>(&'a Lock);
impl<'a> Drop for LockGuard<'a> { fn drop(&mut self) { self.0.unlock(); } }
#[inline] pub fn guard(lock: &Lock) -> LockGuard<'_> { lock.lock(); LockGuard(lock) }