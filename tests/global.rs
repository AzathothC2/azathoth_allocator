#[cfg(test)]
mod global_tests {
    use std::alloc::{GlobalAlloc, Layout};

    #[test]
    fn ansi_prefix_is_intact_no_nuls_inserted() {
        static ALLOC: azathoth_allocator::allocator::AzathothAllocator =  azathoth_allocator::allocator::AzathothAllocator::new();
        let s = b"\x1b[38;5;34mWe reached inner_run!\x1b[0m";
        let layout = Layout::from_size_align(s.len(), 1).unwrap();
        unsafe {
            let p = ALLOC.alloc(layout);
            core::ptr::copy_nonoverlapping(s.as_ptr(), p, s.len());
            assert_eq!(*p, 0x1b);
            assert_eq!(*p.add(1), b'[');
            assert_ne!(*p.add(1), 0, "embedded NUL after ESC (corruption)");
            println!("String: {}", core::str::from_utf8(s.as_slice()).unwrap());
            ALLOC.dealloc(p, layout);
        }
    }

}