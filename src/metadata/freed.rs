use core::mem::MaybeUninit;

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