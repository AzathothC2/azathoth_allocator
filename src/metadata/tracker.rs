use core::ffi::c_void;
use core::marker::PhantomData;
use core::ptr::null_mut;

pub const FLG_IS_LARGE: u32 = 0x01;
pub const FLG_POISONED: u32 = 0x02;
pub const LARGE_THRESHOLD: usize = 65536;
pub const HEADER_SIZE: usize = size_of::<MemBlockHeader>();


#[repr(C, align(16))]
pub struct MemBlockHeader {
    pub(crate) prev: *mut MemBlockHeader,
    pub(crate) next: *mut MemBlockHeader,
    pub(crate) size: usize,
    pub(crate) flag: u32,
    pub(crate) owner: *mut c_void,
    pub(crate) map_len: usize,
}

impl MemBlockHeader {
    #[inline(always)]
    pub const fn empty() -> Self {
        Self {
            prev: null_mut(),
            next: null_mut(),
            owner: null_mut(),
            size: 0,
            flag: 0,
            map_len: 0,
        }
    }
    #[inline(always)]
    pub fn set_large(&mut self) {
        self.flag |= FLG_IS_LARGE;
    }
    #[inline(always)]
    pub fn is_large(&self) -> bool {
        (self.flag & FLG_IS_LARGE) != 0
    }

    #[inline(always)]
    pub fn poison(&mut self) {
        self.flag |= FLG_POISONED;
    }
    #[inline(always)]
    pub fn is_poisoned(&self) -> bool {
        (self.flag & FLG_POISONED) != 0
    }
}

pub struct MemIter<'a> {
    cur: *mut MemBlockHeader,
    _lt: PhantomData<&'a MemBlockHeader>,
}

pub struct MemItem {
    pub ptr: *mut u8,
    pub size: usize,
}

impl<'a> Iterator for MemIter<'a> {
    type Item = MemItem;
    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.cur.is_null() {
            return None;
        }
        unsafe {
            let hdr = self.cur;
            self.cur = (*hdr).next;
            let ptr = (hdr as *mut u8).add(size_of::<MemBlockHeader>());
            Some(MemItem {
                ptr,
                size: (*hdr).size
            })
        }
    }
}
#[inline(always)]
pub unsafe fn header_from_ptr(p: *mut u8) -> *mut MemBlockHeader {
    unsafe { p.sub(size_of::<MemBlockHeader>()) as *mut MemBlockHeader }
}

#[inline(always)]
pub unsafe fn ptr_from_header(h: *mut MemBlockHeader) -> *mut u8 {
    unsafe { (h as *mut u8).add(size_of::<MemBlockHeader>()) }
}
#[repr(C)]
pub struct MemTracker {
    pub head: *mut MemBlockHeader,
}

impl MemTracker {
    pub const fn new() -> Self {
        Self { head: null_mut() }
    }

    #[inline]
    pub unsafe fn iter(&self) -> MemIter<'_> {
        MemIter {
            cur: self.head,
            _lt: PhantomData,
        }
    }
}