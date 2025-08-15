use core::marker::PhantomData;
use core::mem::MaybeUninit;
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
    _pad: usize,
}

impl MemBlockHeader {
    #[inline(always)]
    pub const fn empty() -> Self {
        Self {
            prev: null_mut(),
            next: null_mut(),
            size: 0,
            flag: 0,
            _pad: 0,
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

impl<'a> Iterator for MemIter<'a> {
    type Item = (*mut u8, usize); // (ptr, size);
    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.cur.is_null() {
            return None;
        }
        unsafe {
            let hdr = self.cur;
            self.cur = (*hdr).next;
            let user = (hdr as *mut u8).add(size_of::<MemBlockHeader>());
            Some((user, (*hdr).size))
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

#[repr(C)]
pub struct FreedRecord {
    pub ptr: *mut u8,
    pub size: usize,
    pub flags: u32,
    pub seq: u64
}

pub struct FreedBuffer<const N: usize> {
    head: usize,
    len: usize,
    seq: u64,
    buf: [MaybeUninit<FreedRecord>; N],
}

impl<const N: usize> FreedBuffer<N> {
    pub const fn new() -> Self {
        const U: MaybeUninit<FreedRecord> = MaybeUninit::uninit();
        Self {
            head: 0,
            len: 0,
            seq: 0,
            buf: [U; N],
        }
    }

    #[inline]
    pub fn push(&mut self, rec: FreedRecord) {
        let mut r = rec;
        r.seq = self.seq;
        self.seq = self.seq.wrapping_add(1);

        self.buf[self.head].write(r);
        self.head = (self.head + 1) % N;
        if self.len < N { self.len += 1; }
    }

    pub fn iter(&'_ self) -> FreedIter<'_, N> {
        FreedIter {
            freed: self,
            i: 0
        }
    }
}

pub struct FreedIter<'a, const N: usize> {
    freed: &'a FreedBuffer<N>,
    i: usize,
}

impl<'a, const N: usize> Iterator for FreedIter<'a, N> {
    type Item = FreedRecord;
    fn next(&mut self) -> Option<Self::Item> {
        if self.i >= self.freed.len { return None; }
        let idx = (self.freed.head + N - 1 - self.i) % N;
        let r = unsafe { self.freed.buf[idx].assume_init_read() };
        self.i += 1;
        Some(r)
    }
}