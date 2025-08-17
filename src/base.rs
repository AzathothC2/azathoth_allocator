use core::cell::UnsafeCell;
use core::ptr::null_mut;
use crate::{MAX_RECORDS};
use crate::metadata::{FreedBuffer, FreedRecord, MemBlockHeader, MemTracker};

pub(crate) struct BaseAllocator {
    pub tracker: UnsafeCell<MemTracker>,
    pub freed: UnsafeCell<FreedBuffer<{ MAX_RECORDS }>>,
    #[cfg(feature = "multithread")]
    pub(crate) lock: crate::lock::Lock,
}

impl BaseAllocator {
    pub const fn new() -> Self {
        Self {
            tracker: UnsafeCell::new(MemTracker::new()),
            #[cfg(feature = "multithread")]
            lock: crate::lock::Lock::new(),
            freed: UnsafeCell::new(FreedBuffer::new()),
        }
    }
    #[inline(always)]
    pub unsafe fn track_insert(&self, hdr: *mut MemBlockHeader) {
        unsafe {
            // #[cfg(feature = "multithread")]
            // let _g = self.lock.guard();

            let list = &mut *self.tracker.get();
            (*hdr).prev = null_mut();
            (*hdr).next = list.head;
            if !list.head.is_null() {
                (*list.head).prev = hdr;
            }
            list.head = hdr;
        }
    }

    #[inline(always)]
    pub unsafe fn track_remove(&self, hdr: *mut MemBlockHeader) {
        unsafe {
            // #[cfg(feature = "multithread")]
            // let _g = self.lock.guard();

            let list = &mut *self.tracker.get();
            let prev = (*hdr).prev;
            let next = (*hdr).next;
            if !prev.is_null() {
                (*prev).next = next;
            } else {
                list.head = next;
            }
            if !next.is_null() {
                (*next).prev = prev;
            }
            (*hdr).prev = null_mut();
            (*hdr).next = null_mut();

        }
    }

    #[inline(always)]
    pub unsafe fn record_freed(&self, user: *mut u8, hdr: *mut MemBlockHeader) {
        unsafe {
            // #[cfg(feature = "multithread")]
            // let _g = self.lock.guard();
            let rec = FreedRecord {
                ptr: user,
                size: (*hdr).size,
                flags: (*hdr).flag,
                seq: 0,
            };
            (*self.freed.get()).push(rec);

        }
    }
}