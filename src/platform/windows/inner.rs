use crate::base::BaseAllocator;
use crate::memtrack::{header_from_ptr, ptr_from_header, MemBlockHeader, LARGE_THRESHOLD};
use crate::platform::windows::mem::WinApiFunctions;
use azathoth_core::os::windows::consts::{MEM_COMMIT, MEM_RESERVE, PAGE_READWRITE};
use azathoth_core::os::windows::types::DWORD;
use azathoth_core::os::Current::types::HANDLE;
use core::alloc::{GlobalAlloc, Layout};
use core::cell::Cell;
use core::ffi::c_void;
use core::ptr::null_mut;

pub struct WinAllocator {
    pub functions: WinApiFunctions,
    heap: Cell<HANDLE>,
    base_alloc: BaseAllocator
}

impl WinAllocator {
    #[inline]
    pub fn heap(&self) -> HANDLE {
        let h = self.heap.get();
        if !h.is_null() {
            return h;
        }
        let h2 = self.functions.GetProcessHeap();
        debug_assert!(!h2.is_null());
        self.heap.set(h2);
        h2
    }

    pub const fn new() -> Self {
        Self {
            functions: WinApiFunctions::new(),
            heap: Cell::new(null_mut()),
            base_alloc: BaseAllocator::new()
        }
    }


    #[unsafe(link_section = ".text")]
    pub unsafe fn inner_alloc(&self, layout: Layout) -> core::ptr::NonNull<u8> {
        let ptr = unsafe { self.inner_alloc_hdr(layout) };
        let ret = match core::ptr::NonNull::new(ptr) {
            Some(ptr) => ptr,
            None => {
                panic!("Invalid ptr")
            }
        };
        ret
    }

    #[unsafe(link_section = ".text")]
    unsafe fn virtual_alloc_rw(&self, size: usize) -> *mut u8 {
        unsafe {
            self.functions
                .VirtualAlloc(null_mut(), size, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE)
                as *mut u8
        }
    }
    #[unsafe(link_section = ".text")]
    unsafe fn virtual_free(&self, ptr: *mut u8) {
        const MEM_RELEASE: DWORD = 0x00008000;
        unsafe {
            self.functions.VirtualFree(
                ptr as *mut c_void,
                0,
                MEM_RELEASE, //TODO: add this to azathoth-core
            );
        }
    }

    #[unsafe(link_section = ".text")]
    unsafe fn inner_alloc_hdr(&self, layout: Layout) -> *mut u8 {
        self.functions.ensure_init();

        let need = layout.size().max(1);
        let total = need + size_of::<MemBlockHeader>();
        if need >= LARGE_THRESHOLD {
            let base = unsafe { self.virtual_alloc_rw(total) };
            if base.is_null() {
                return null_mut();
            }
            let hdr = base as *mut MemBlockHeader;
            unsafe {
                *hdr = MemBlockHeader::empty();
                (*hdr).size = need;
                (*hdr).set_large();
                self.base_alloc.track_insert(hdr);
                return ptr_from_header(hdr);
            }
        }
        let heap = self.heap();
        let base = self.functions.HeapAlloc(heap, 0, total) as *mut u8;
        if base.is_null() {
            return null_mut();
        }
        let hdr = base as *mut MemBlockHeader;
        unsafe {
            *hdr = MemBlockHeader::empty();
            (*hdr).size = need;
            self.base_alloc.track_insert(hdr);
            ptr_from_header(hdr)
        }
    }
    #[unsafe(link_section = ".text")]
    unsafe fn free_with_hdr(&self, ptr: *mut u8) {
        if ptr.is_null() {
            return;
        }
        self.functions.ensure_init();
        unsafe {
            let hdr = header_from_ptr(ptr);
            #[cfg(debug_assertions)]
            if (*hdr).is_poisoned() {
                core::arch::asm!("int3", options(noreturn))
            }
            self.base_alloc.track_remove(hdr);
            self.base_alloc.record_freed(ptr, hdr);
            (*hdr).poison();
            if (*hdr).is_large() {
                self.virtual_free(hdr as *mut u8);
            } else {
                let heap = self.heap();
                let _ = self.functions.HeapFree(heap, 0, hdr as _);
            }
        }
    }

    #[unsafe(link_section = ".text")]
    pub unsafe fn inner_dealloc(&self, ptr: *mut u8, _layout: Layout) {
        unsafe { self.free_with_hdr(ptr) };
    }

    #[unsafe(link_section = ".text")]
    pub unsafe fn inner_realloc(
        &self,
        ptr: *mut u8,
        _layout: Layout,
        new_size: usize,
    ) -> core::ptr::NonNull<u8> {
        unsafe {
            if new_size == 0 {
                self.free_with_hdr(ptr);
                panic!("realloc(0)");
            }
            let hdr = header_from_ptr(ptr);
            if new_size <= (*hdr).size {
                (*hdr).size = new_size;
                return core::ptr::NonNull::new_unchecked(ptr);
            }
            if !(*hdr).is_large() {
                const HEAP_REALLOC_IN_PLACE_ONLY: u32 = 0x10;
                let total = new_size + size_of::<MemBlockHeader>();
                let tried = self.functions.HeapReAlloc(
                    self.heap(),
                    HEAP_REALLOC_IN_PLACE_ONLY,
                    hdr as _,
                    total,
                ) as *mut u8;
                if !tried.is_null() {
                    (*hdr).size = new_size;
                    return core::ptr::NonNull::new_unchecked(ptr);
                }
            }
            let new_layout = Layout::from_size_align(new_size, 16).unwrap();
            let new_ptr = self.inner_alloc(new_layout);
            core::ptr::copy_nonoverlapping(
                ptr,
                new_ptr.as_ptr(),
                core::cmp::min((*hdr).size, new_size),
            );
            self.free_with_hdr(ptr);
            new_ptr
        }
    }
}
