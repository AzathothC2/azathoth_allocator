use azathoth_core::os::Current::consts::{SYS_MMAP, SYS_MUNMAP};
use azathoth_core::os::Current::syscalls::{syscall2, syscall6};

pub const SPAN_BYTES: usize = 256 * 1024;

pub const CLASS_SIZES: &[usize] = &[
    32, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384, 32768, 65536,
];

#[inline(always)]
pub unsafe fn mmap(
    addr: *mut u8,
    len: usize,
    prot: usize,
    flags: u32,
    fd: usize,
    offset: usize,
) -> *mut u8 {
    syscall6(
        SYS_MMAP,
        addr as _,
        len as _,
        prot as _,
        flags as _,
        fd as _,
        offset as _,
    ) as *mut u8
}


#[inline(always)]
pub unsafe fn munmap(addr: *mut core::ffi::c_void, len: usize) -> i32 {
    syscall2(SYS_MUNMAP, addr as usize, len) as _
}

#[inline(always)]
pub fn get_class(total_needed: usize) -> Option<usize> {
    for &cls in CLASS_SIZES {
        if total_needed <= cls {
            return Some(cls);
        }
    }
    None
}
#[inline(always)]
pub fn align_up(x: usize, a: usize) -> usize {
    (x + (a - 1)) & !(a - 1)
}