#![allow(non_snake_case)]
use core::{
    ptr::null_mut, sync::atomic::AtomicU8,
};

use core::sync::atomic::Ordering;
#[cfg(debug_assertions)]
use azathoth_core::os::Current::fn_defs::HeapValidate_t;
use azathoth_core::os::Current::{
    fn_defs::{GetProcessHeap_t, HeapAlloc_t, HeapFree_t, HeapReAlloc_t},
    types::{HANDLE, LPVOID},
};
use azathoth_core::os::Current::fn_defs::{VirtualAlloc_t, VirtualFree_t};
use azathoth_core::os::Current::types::{BOOL, DWORD};
use azathoth_libload::{get_proc_address, load_library};
use azathoth_utils::crc32;

pub(crate) struct WinApiFunctions {
    heap_alloc: Option<HeapAlloc_t>,
    heap_realloc: Option<HeapReAlloc_t>,
    get_process_heap: Option<GetProcessHeap_t>,
    heap_free: Option<HeapFree_t>,
    #[cfg(debug_assertions)]
    heap_validate: Option<HeapValidate_t>,
    virtual_alloc: Option<VirtualAlloc_t>,
    virtual_free: Option<VirtualFree_t>
}
#[unsafe(link_section = ".text")]
static INIT: AtomicU8 = AtomicU8::new(0);

impl WinApiFunctions {

    #[inline]
    pub fn ensure_init(&self) {
        if INIT.load(Ordering::Acquire) == 2 { return; }
        if INIT.compare_exchange(0,1,Ordering::AcqRel,Ordering::Acquire).is_ok() {
            unsafe { (*(self as *const _ as *mut Self)).init(); }
            INIT.store(2, Ordering::Release);
        } else {
            while INIT.load(Ordering::Acquire) != 2 { core::hint::spin_loop(); }
        }
    }

    #[unsafe(link_section = ".text")]
    pub(crate) const fn new() -> Self {
        Self {
            heap_alloc: None,
            heap_free: None,
            get_process_heap: None,
            heap_realloc: None,
            #[cfg(debug_assertions)]
            heap_validate: None,
            virtual_free: None,
            virtual_alloc: None
        }
    }

    pub unsafe fn init(&mut self) -> bool {
        let hasher = |name: &str| -> u32 {
            crc32(name)
        };
        let k32 = match unsafe { load_library("KERNEL32.dll", &hasher) } {
            Some(h) => h,
            None => {
                panic!("Failed to load KERNEL32.dll");
            }
        };
        self.get_process_heap = resolve_fn(k32, "GetProcessHeap");
        self.heap_alloc = resolve_fn(k32, "HeapAlloc");
        self.heap_realloc = resolve_fn(k32, "HeapReAlloc");
        self.heap_free = resolve_fn(k32, "HeapFree");
        self.virtual_alloc = resolve_fn(k32, "VirtualAlloc");
        self.virtual_free = resolve_fn(k32, "VirtualFree");
        #[cfg(debug_assertions)]
        {
            self.heap_validate = resolve_fn(k32, "HeapValidate");
        }
        true
    }

    pub fn GetProcessHeap(&self) -> HANDLE {
        let func = match self.get_process_heap {
            Some(g) => g,
            None => return null_mut(),
        };
        unsafe { func() }
    }

    #[unsafe(link_section = ".text")]
    pub fn HeapAlloc(&self, hHeap: HANDLE, dwFlags: u32, dwBytes: usize) -> LPVOID {
        let func = match self.heap_alloc {
            Some(g) => g,
            None => return null_mut(),
        };
        unsafe { func(hHeap, dwFlags, dwBytes) }
    }

    #[unsafe(link_section = ".text")]
    pub unsafe fn HeapReAlloc(
        &self,
        hHeap: HANDLE,
        dwFlags: u32,
        lpMem: LPVOID,
        dwBytes: usize,
    ) -> HANDLE {
        let func = match self.heap_realloc {
            Some(g) => g,
            None => return null_mut(),
        };
        unsafe { func(hHeap, dwFlags, lpMem, dwBytes) }
    }

    #[unsafe(link_section = ".text")]
    pub unsafe fn HeapFree(&self, hHeap: HANDLE, dwFlags: u32, lpMem: HANDLE) -> i32 {
        let func = match self.heap_free {
            Some(g) => g,
            None => return -1,
        };
        unsafe { func(hHeap, dwFlags, lpMem) }
    }

    #[allow(unused)]
    #[cfg(debug_assertions)]
    #[unsafe(link_section = ".text")]
    pub unsafe fn HeapValidate(&self, hHeap: HANDLE, dwFlags: u32, lpMem: *const core::ffi::c_void) -> i32 {
        let func = match self.heap_validate {
            Some(g) => g,
            None => return -1,
        };
        unsafe { func(hHeap, dwFlags, lpMem as _) }
    }

    #[unsafe(link_section = ".text")]
    pub unsafe fn VirtualAlloc(&self, lpAddress: LPVOID, dwSize: usize, flAllocationType: DWORD, flProtect: DWORD) -> LPVOID {
        let func = match self.virtual_alloc {
            Some(g) => g,
            None => return null_mut(),
        };
        unsafe { func(lpAddress, dwSize, flAllocationType, flProtect as _) }
    }

    #[unsafe(link_section = ".text")]
    pub unsafe fn VirtualFree(&self, lpAddress: LPVOID, dwSize: usize, dwFreeType: DWORD) -> BOOL {
        let func = match self.virtual_free {
            Some(g) => g,
            None => return 1,
        };
        unsafe { func(lpAddress, dwSize, dwFreeType) }
    }

}

fn resolve_fn<T>(module: *mut u8, name: &str) -> Option<T> {
    let hasher = |name: &str| -> u32 {
        crc32(name)
    };
    let addr = unsafe {
        match get_proc_address(module, &hasher, name) {
            Some(p) => p,
            None => {
                panic!("resolve_fn")
            }
        }
    };
    let ptr = unsafe { core::mem::transmute_copy::<_, T>(&addr) };
    Some(ptr)
}
