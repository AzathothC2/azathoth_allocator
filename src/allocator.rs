use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use crate::metadata::{FreedBuffer, MemTracker};
use crate::platform::InnerAllocator;

pub struct Api {
    inner: InnerAllocator
}
pub struct AzathothAllocator {
    inner: UnsafeCell<Api>
}

impl AzathothAllocator {
    #[unsafe(link_section = ".text")]
    pub const fn new() -> Self {
        Self {
            inner: UnsafeCell::new(Api::new()),
        }
    }

    #[unsafe(link_section = ".text")]
    pub unsafe fn init(&self) -> bool {
        unsafe { (*self.inner.get()).init() }
    }

    #[unsafe(link_section = ".text")]
    pub unsafe fn get_memtracker(&self) -> *mut MemTracker {
        unsafe { self.get_ref().inner.base_alloc.tracker.get() }
    }
    #[unsafe(link_section = ".text")]
    pub unsafe fn get_freed(&self) -> *mut FreedBuffer<{ crate::MAX_RECORDS }> {
        unsafe { self.get_ref().inner.base_alloc.freed.get() }
    }
    #[unsafe(link_section = ".text")]
    pub unsafe fn get_ref(&self) -> &Api {
        unsafe { &*self.inner.get() }
    }
}

unsafe impl Sync for AzathothAllocator {}

impl Api {
    pub unsafe fn init(&mut self) -> bool {
        #[cfg(target_os = "windows")]
        {
            unsafe { self.inner.functions.init() }
        }

        #[cfg(not(target_os = "windows"))]
        {
            true
        }
    }

    #[unsafe(link_section = ".text")]
    pub const fn new() -> Api {
        Self {
            inner: InnerAllocator::new(),
        }
    }

    #[unsafe(link_section = ".text")]
    pub unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { self.inner.inner_alloc(layout).as_ptr() }
    }
    #[unsafe(link_section = ".text")]
    pub unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe {
            self.inner.inner_dealloc(ptr, layout);
        }
    }

    #[unsafe(link_section = ".text")]
    pub unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe { self.inner.inner_realloc(ptr, layout, new_size).as_ptr() }
    }
}

unsafe impl GlobalAlloc for AzathothAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { self.get_ref().alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { self.get_ref().dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe { self.get_ref().realloc(ptr, layout, new_size) }
    }
}

