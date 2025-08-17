#[cfg(test)]
mod test_utils {
    use std::alloc::Layout;
    use std::time::{Duration, Instant};

    pub fn cfg_usize(key: &str, default: usize) -> usize {
        std::env::var(key)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    }

    pub struct Budget(Instant, Duration);
    impl Budget {
        pub fn new_ms(ms: u64) -> Self {
            Self(Instant::now(), Duration::from_millis(ms))
        }
        pub fn hit(&self) -> bool {
            self.0.elapsed() >= self.1
        }
    }

    #[inline]
    pub fn is_aligned(p: *mut u8, align: usize) -> bool {
        (p as usize) & (align - 1) == 0
    }

    #[inline]
    pub unsafe fn fill_pattern(ptr: *mut u8, len: usize, seed: u32) {
        let mut s = seed ^ (len as u32).wrapping_mul(0x9E37_79B9);
        for i in 0..len {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            unsafe {
                ptr.add(i).write((s as u8).wrapping_add(i as u8));
            }
        }
    }

    #[inline]
    pub unsafe fn verify_pattern(ptr: *mut u8, len: usize, seed: u32) {
        let mut s = seed ^ (len as u32).wrapping_mul(0x9E37_79B9);
        for i in 0..len {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            let expected = (s as u8).wrapping_add(i as u8);
            let got = unsafe { ptr.add(i).read() };
            assert_eq!(got, expected, "pattern mismatch at byte {}", i);
        }
    }

    #[derive(Clone)]
    pub struct Lcg(u64);
    impl Lcg {
        pub fn new(seed: u64) -> Self {
            Self(seed | 1)
        }
        #[inline]
        pub fn next_u32(&mut self) -> u32 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
            (self.0 >> 32) as u32
        }
        #[inline]
        pub fn gen_range(&mut self, lo: u32, hi_excl: u32) -> u32 {
            let r = self.next_u32();
            lo + r % (hi_excl - lo).max(1)
        }
    }

    pub fn layout_checked(size: usize, align: usize) -> Layout {
        Layout::from_size_align(size, align.max(1)).expect("valid layout")
    }

    pub fn choose_align(r: &mut Lcg) -> usize {
        // Powers of two up to 64 (adjust as needed)
        const A: &[usize] = &[1, 2, 4, 8, 16, 32, 64];
        A[r.gen_range(0, A.len() as u32) as usize]
    }
}

#[cfg(test)]
mod global_tests {
    use super::test_utils::*;
    use std::alloc::{GlobalAlloc, Layout};
    static ALLOC: azathoth_allocator::allocator::AzathothAllocator =
        azathoth_allocator::allocator::AzathothAllocator::new();

    #[test]
    fn smoke_small_alloc_free() {
        unsafe {
            ALLOC.init();
        }
        let s = b"\x1b[38;5;34mWe reached inner_run!\x1b[0m";
        let layout = Layout::from_size_align(s.len(), 1).unwrap();
        unsafe {
            let p = ALLOC.alloc(layout);
            assert!(!p.is_null());
            core::ptr::copy_nonoverlapping(s.as_ptr(), p, s.len());
            assert_eq!(*p, 0x1b);
            assert_eq!(*p.add(1), b'[');
            assert_ne!(*p.add(1), 0);
            ALLOC.dealloc(p, layout);
        }
    }

    #[test]
    fn alignment_is_respected() {
        unsafe {
            ALLOC.init();
        }
        for &align in &[1, 2, 4, 8, 16, 32, 64, 128] {
            let layout = layout_checked(257, align);
            unsafe {
                let p = ALLOC.alloc(layout);
                assert!(!p.is_null(), "null for align={}", align);
                assert!(is_aligned(p, align), "ptr not aligned to {}", align);
                ALLOC.dealloc(p, layout);
            }
        }
    }
}

#[cfg(test)]
mod st_stress {
    use super::test_utils::*;
    use std::alloc::{GlobalAlloc, Layout};

    static ALLOC: azathoth_allocator::allocator::AzathothAllocator =
        azathoth_allocator::allocator::AzathothAllocator::new();

    struct Block {
        ptr: *mut u8,
        layout: Layout,
        seed: u32,
    }

    #[test]
    fn stress_mixed_sizes_fast() {
        unsafe {
            ALLOC.init();
        }

        // Fast defaults; override via env when needed
        let iters = cfg_usize("ALLOC_TEST_ITERS", 800);
        let keep = cfg_usize("ALLOC_TEST_LIVE", 128);
        let time = cfg_usize("ALLOC_TEST_MS", 1800); // bail after ~1.8s
        let budget = Budget::new_ms(time as u64);

        let mut rng = Lcg::new(0xA5A5_1234);
        let mut live: Vec<Block> = Vec::with_capacity(keep);

        for i in 0..iters {
            // Sizes biased to small/medium; rare large to tick that path.
            let bucket = rng.gen_range(0, 100);
            let size = if bucket < 70 {
                rng.gen_range(8, 1024) as usize
            } else if bucket < 98 {
                rng.gen_range(1024, 16 * 1024) as usize
            } else {
                rng.gen_range(64 * 1024, 256 * 1024) as usize
            };
            let align = choose_align(&mut rng);
            let layout = layout_checked(size, align);

            unsafe {
                let p = ALLOC.alloc(layout);
                assert!(!p.is_null());
                assert!(is_aligned(p, align));
                let seed = rng.next_u32();
                // Touch sparsely for larger blocks to avoid full page commits.
                if size <= 4096 {
                    fill_pattern(p, size, seed);
                }
                live.push(Block {
                    ptr: p,
                    layout,
                    seed,
                });

                // Keep working set bounded
                if live.len() > keep {
                    let b = live.swap_remove(
                        (rng.gen_range(0, live.len() as u32) as usize).min(live.len() - 1),
                    );
                    if b.layout.size() <= 4096 {
                        verify_pattern(b.ptr, b.layout.size(), b.seed);
                    }
                    ALLOC.dealloc(b.ptr, b.layout);
                }
            }

            if i % 128 == 0 && budget.hit() {
                break;
            }
        }

        unsafe {
            for b in live.drain(..) {
                if b.layout.size() <= 4096 {
                    verify_pattern(b.ptr, b.layout.size(), b.seed);
                }
                ALLOC.dealloc(b.ptr, b.layout);
            }
        }
    }
}

#[cfg(all(test, feature = "multithread"))]
mod mt_tests {
    use super::test_utils::*;
    use azathoth_allocator::lock::Lock;
    use std::alloc::{GlobalAlloc, Layout};
    use std::sync::{Arc, Barrier, Once};
    use std::thread;
    use std::time::{Duration, Instant};

    static ALLOC: azathoth_allocator::allocator::AzathothAllocator =
        azathoth_allocator::allocator::AzathothAllocator::new();

    static INIT: Once = Once::new();
    fn init_alloc() {
        INIT.call_once(|| unsafe {
            ALLOC.init();
        });
    }

    #[test]
    fn lock_test() {
        let lock = Lock::new();
        let start = Instant::now();
        lock.lock();
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "lock acquisition stuck"
        );
        lock.unlock();
    }
    #[test]
    fn mt_smoke_fast() {
        init_alloc();

        let max_threads = cfg_usize("ALLOC_TEST_THREADS", 2);
        let iters_per_thread = cfg_usize("ALLOC_TEST_ITERS", 400);

        let threads = max_threads.max(1).min(4);
        let barrier = Arc::new(Barrier::new(threads));
        let mut handles = Vec::with_capacity(threads);

        for t in 0..threads {
            let b = barrier.clone();
            handles.push(thread::spawn(move || {
                let mut rng = Lcg::new(0xC0FFEE ^ t as u64);
                let mut live: Vec<(*mut u8, Layout, u32)> = Vec::with_capacity(64);
                b.wait();

                for _ in 0..iters_per_thread {
                    let size = rng.gen_range(8, 4096) as usize;
                    let align = choose_align(&mut rng);
                    let layout = layout_checked(size, align);
                    unsafe {
                        let p = ALLOC.alloc(layout);
                        assert!(!p.is_null());
                        let seed = rng.next_u32();
                        fill_pattern(p, size, seed);
                        live.push((p, layout, seed));
                        if live.len() > 64 {
                            let (pp, ll, ss) = live.pop().unwrap();
                            verify_pattern(pp, ll.size(), ss);
                            ALLOC.dealloc(pp, ll);
                        }
                    }
                }

                unsafe {
                    for (p, l, s) in live.drain(..) {
                        verify_pattern(p, l.size(), s);
                        ALLOC.dealloc(p, l);
                    }
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
    }

    #[test]
    fn mt_stress_contention() {
        init_alloc();

        let n_cpus = thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let threads = n_cpus.clamp(4, 12);
        let iters_per_thread = 4_000usize;

        let barrier = Arc::new(Barrier::new(threads));
        let mut handles = Vec::with_capacity(threads);

        for t in 0..threads {
            let b = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                let mut rng = Lcg::new(0xC0FFEE_D00D ^ (t as u64));

                b.wait();

                let mut live: Vec<(*mut u8, Layout, u32)> = Vec::with_capacity(256);

                for _ in 0..iters_per_thread {
                    let bucket = rng.gen_range(0, 100);
                    let size = if bucket < 70 {
                        rng.gen_range(8, 1024) as usize
                    } else if bucket < 98 {
                        rng.gen_range(1024, 64 * 1024) as usize
                    } else {
                        rng.gen_range(256 * 1024, 2 * 1024 * 1024) as usize
                    };
                    println!("Allocating size: {}", size);
                    let align = choose_align(&mut rng);
                    let layout = layout_checked(size, align);

                    unsafe {
                        let p = ALLOC.alloc(layout);
                        assert!(!p.is_null(), "null ptr in thread {t}");
                        assert!(is_aligned(p, align));

                        let seed = rng.next_u32();
                        fill_pattern(p, size, seed);
                        live.push((p, layout, seed));

                        if !live.is_empty() && (rng.gen_range(0, 3) == 0) {
                            let idx =
                                (rng.gen_range(0, live.len() as u32) as usize).min(live.len() - 1);
                            let (pp, ll, ss) = live.swap_remove(idx);
                            verify_pattern(pp, ll.size(), ss);
                            ALLOC.dealloc(pp, ll);
                        }
                    }
                }
                unsafe {
                    for (p, l, s) in live.drain(..) {
                        verify_pattern(p, l.size(), s);
                        ALLOC.dealloc(p, l);
                    }
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }
    }
}
