use crate::base::BaseAllocator;
use crate::memtrack::{
    header_from_ptr, ptr_from_header, MemBlockHeader, HEADER_SIZE, LARGE_THRESHOLD,
};
use crate::platform::linux::maps::{
    align_up, get_class, mmap, munmap, CLASS_SIZES, SPAN_BYTES,
};
use crate::platform::linux::{write, writenum};
use azathoth_core::os::linux::consts::{
    MAP_ANONYMOUS, MAP_PRIVATE, PROT_READ, PROT_WRITE,
};
use core::alloc::Layout;
use core::cell::UnsafeCell;
use core::ptr::{null_mut, NonNull};

#[repr(C)]
struct Span {
    next: *mut Span,
    base: *mut u8,
    len: usize,
    class_size: usize,
    total_slots: usize,
    free_count: usize,
    free_list: *mut MemBlockHeader,
}

const CLASS_SLOTS: usize = 12;

const ALIGN: usize = 16;

pub struct LinuxAllocator {
    spans: UnsafeCell<[*mut Span; CLASS_SLOTS]>,
    pub(crate) base_alloc: BaseAllocator,
}

impl LinuxAllocator {
    pub const fn new() -> Self {
        Self {
            spans: UnsafeCell::new([null_mut(); CLASS_SLOTS]),
            base_alloc: BaseAllocator::new(),
        }
    }
    unsafe fn alloc_large(&self, need: usize) -> *mut u8 {
        let total = need + HEADER_SIZE;
        unsafe {
            let base = mmap(
                null_mut(),
                total,
                (PROT_READ | PROT_WRITE) as usize,
                (MAP_PRIVATE | MAP_ANONYMOUS) as u32,
                usize::MAX,
                0,
            );
            if base.is_null() {
                write("MMAP WAS NULL");
                return null_mut();
            }
            let hdr = base as *mut MemBlockHeader;
            *hdr = MemBlockHeader::empty();
            (*hdr).size = need;
            (*hdr).set_large();
            self.base_alloc.track_insert(hdr);
            ptr_from_header(hdr)
        }
    }
    #[inline(always)]
    unsafe fn free_large(&self, hdr: *mut MemBlockHeader) {
        unsafe {
            self.base_alloc.track_remove(hdr);
            self.base_alloc.record_freed(ptr_from_header(hdr), hdr);
            (*hdr).poison();
            let total = (*hdr).size + HEADER_SIZE;
            let _ = munmap(hdr as _, total);
        }
    }

    #[inline(always)]
    unsafe fn span_list_mut(&self) -> &mut [*mut Span; CLASS_SLOTS] {
        unsafe { &mut *self.spans.get() }
    }

    unsafe fn span_create(&self, class_size: usize) -> *mut Span {
        unsafe {
            let raw = mmap(
                null_mut(),
                SPAN_BYTES,
                (PROT_READ | PROT_WRITE) as usize,
                (MAP_PRIVATE | MAP_ANONYMOUS) as u32,
                usize::MAX,
                0,
            );
            if raw.is_null() {
                return null_mut();
            }
            let span_hdr_sz = align_up(size_of::<Span>(), ALIGN);
            let arena = raw.add(span_hdr_sz);
            let arena_len = SPAN_BYTES - span_hdr_sz;
            let total_slots = arena_len / class_size;
            if total_slots == 0 {
                let _ = munmap(raw as _, SPAN_BYTES);
                return null_mut();
            }
            let span = raw as *mut Span;
            (*span).next = null_mut();
            (*span).base = arena;
            (*span).len = SPAN_BYTES;
            (*span).class_size = class_size;
            (*span).total_slots = total_slots;
            (*span).free_count = total_slots;
            (*span).free_list = null_mut();

            let mut head: *mut MemBlockHeader = null_mut();
            let mut p = arena;
            for _ in 0..total_slots {
                let hdr = p as *mut MemBlockHeader;
                *hdr = MemBlockHeader::empty();
                (*hdr).next = head;
                head = hdr;
                p = p.add(class_size);
            }
            (*span).free_list = head;
            span
        }
    }
    unsafe fn span_alloc_block(&self, span: *mut Span, need: usize) -> *mut u8 {
        unsafe {
            let hdr = (*span).free_list;
            if hdr.is_null() { return null_mut(); }
            (*span).free_list = (*hdr).next;
            (*span).free_count -= 1;
            (*hdr).prev = null_mut();
            (*hdr).next = null_mut();
            (*hdr).size = need;
            (*hdr).flag = 0;
            self.base_alloc.track_insert(hdr);
            ptr_from_header(hdr)
        }
    }

    unsafe fn span_free_block(&self, span: *mut Span, hdr: *mut MemBlockHeader) {
        unsafe {
            self.base_alloc.track_remove(hdr);
            let ptr = ptr_from_header(hdr);
            self.base_alloc.record_freed(ptr, hdr);
            (*hdr).poison();
            (*hdr).next = (*span).free_list;
            (*span).free_list = hdr;
            (*span).free_count += 1;

            if (*span).free_count == (*span).total_slots {
                self.span_unlink(span);
            }
        }
    }
    unsafe fn span_unlink(&self, dead: *mut Span) {
        unsafe {
            let class_size = (*dead).class_size;
            let idx = CLASS_SIZES.iter().position(|&s| s == class_size).unwrap();
            let lists = self.span_list_mut();

            #[cfg(feature = "multithread")]
            let _g = crate::lock::guard(&self.base_alloc.lock);

            let mut cur = lists[idx];
            let mut prev: *mut Span = null_mut();
            while !cur.is_null() {
                if cur == dead {
                    if prev.is_null() {
                        lists[idx] = (*cur).next;
                    } else {
                        (*prev).next = (*cur).next;
                    }
                    let _ = munmap(cur as _, (*cur).len);
                    return;
                }
                prev = cur;
                cur = (*cur).next;
            }
        }
    }
    unsafe fn find_create_span(&self, idx: usize) -> *mut Span {
        unsafe {
            let lists = self.span_list_mut();
            let mut cur = lists[idx];
            while !cur.is_null() {
                if (*cur).free_count > 0 {
                    return cur;
                }
                cur = (*cur).next;
            }
            let class_size = CLASS_SIZES[idx];
            let span = self.span_create(class_size);
            if span.is_null() {
                return null_mut();
            }
            (*span).next = lists[idx];
            lists[idx] = span;
            span
        }
    }

    unsafe fn alloc_small(&self, layout: Layout) -> *mut u8 {
        let need = core::cmp::max(1, layout.size());
        let total = align_up(HEADER_SIZE + need, ALIGN);
        let idx = match get_class(total) {
            Some(i) => CLASS_SIZES.iter().position(|&s| s == i).unwrap(),
            None => return {
                write("could not get class size of: ");
                writenum(total as u32);
                write("\n");
                null_mut()
            },
        };
        #[cfg(feature = "multithread")]
        let _g = crate::lock::guard(&self.base_alloc.lock);
        let span = unsafe { self.find_create_span(idx) };
        if span.is_null() {
            write("span is null\n");
            return null_mut();
        }
        unsafe { self.span_alloc_block(span, need) }
    }

    pub unsafe fn inner_alloc(&self, layout: Layout) -> NonNull<u8> {
        unsafe {
            let need = core::cmp::max(1, layout.size());
            let ptr = if need >= LARGE_THRESHOLD {
                self.alloc_large(need)
            } else {
                self.alloc_small(layout)
            };
            NonNull::new(ptr).unwrap_or_else(|| {
                write("failed to create new NonNull pointer\n");
                core::arch::asm!("int3", options(noreturn))
            })
        }
    }

    pub unsafe fn inner_dealloc(&self, ptr: *mut u8, _layout: Layout) {
        unsafe {
            if ptr.is_null() {
                return;
            }
            let hdr = header_from_ptr(ptr);
            if (*hdr).is_poisoned() {
                write("Use After Free\n");
                // UAF. just abort.
                core::arch::asm!("int3", options(noreturn))
            }
            if (*hdr).is_large() {
                self.free_large(hdr);
                return;
            }

            let span_base = (hdr as usize) & !(SPAN_BYTES - 1);
            let span = span_base as *mut Span;

            #[cfg(feature = "multithread")]
            let _g = crate::lock::guard(&self.base_alloc.lock);
            self.span_free_block(span, hdr);
        }
    }
    pub unsafe fn inner_realloc(
        &self,
        ptr: *mut u8,
        layout: Layout,
        new_size: usize,
    ) -> NonNull<u8> {
        unsafe {
            if new_size == 0 {
                self.inner_dealloc(ptr, layout);
                write("realloc with new_size == 0\n");
                core::arch::asm!("int3", options(noreturn));
            }
            let hdr = header_from_ptr(ptr);
            let current_size = (*hdr).size;

            let new_layout = Layout::from_size_align_unchecked(new_size, ALIGN);
            let new_ptr = self.inner_alloc(new_layout);
            core::ptr::copy_nonoverlapping(
                ptr,
                new_ptr.as_ptr(),
                core::cmp::min(current_size, new_size),
            );
            self.inner_dealloc(ptr, layout);
            new_ptr
        }
    }
}
