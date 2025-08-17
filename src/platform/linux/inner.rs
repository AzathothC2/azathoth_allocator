use crate::base::BaseAllocator;
use crate::metadata::{
    HEADER_SIZE, LARGE_THRESHOLD, MemBlockHeader, header_from_ptr, ptr_from_header,
};
use crate::platform::linux::maps::{CLASS_SIZES, SPAN_BYTES, align_up, get_class, mmap, munmap};
use crate::platform::linux::{write, writenum};
use azathoth_core::os::linux::consts::{MAP_ANONYMOUS, MAP_PRIVATE, PROT_READ, PROT_WRITE};
use core::alloc::Layout;
use core::cell::UnsafeCell;
use core::ffi::c_void;
use core::ptr::{NonNull, null_mut};

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
                (*hdr).owner = span as _;
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
            if hdr.is_null() {
                write("self.span_alloc_block(): Header was null!\n");
                return null_mut();
            }
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
                write("self.find_create_span(): span is null\n");
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
            None => {
                return {
                    write("could not get class size of: ");
                    writenum(total as u32);
                    write("\n");
                    null_mut()
                };
            }
        };
        let span = unsafe { self.find_create_span(idx) };
        if span.is_null() {
            write("span is null\n");
            return null_mut();
        }
        unsafe { self.span_alloc_block(span, need) }
    }

    unsafe fn alloc_large_aligned(&self, need: usize, align: usize) -> *mut u8 {
        unsafe {
            if !align.is_power_of_two() {
                write("alloc_large_aligned: alignment must be power of two\n");
                core::arch::asm!("int3", options(noreturn))
            }
            let required = need.saturating_add(HEADER_SIZE);
            let over = required.saturating_add(align);

            let raw = mmap(
                null_mut(),
                over,
                (PROT_READ | PROT_WRITE) as usize,
                (MAP_PRIVATE | MAP_ANONYMOUS) as u32,
                usize::MAX,
                0,
            );
            if raw.is_null() {
                write("alloc_large_aligned(): mmap failed\n");
                return null_mut();
            }

            let raw_usize = raw as usize;
            let user_usize = align_up(raw_usize.saturating_add(HEADER_SIZE), align);
            let hdr_usize = user_usize.saturating_sub(HEADER_SIZE);
            let raw_end = raw_usize.saturating_add(over);

            if hdr_usize < raw_usize || user_usize.saturating_add(need) > raw_end {
                write("alloc_large_aligned(): bad align window, unmapping\n");
                let _ = munmap(raw as _, over);
                return null_mut();
            }

            let hdr = hdr_usize as *mut MemBlockHeader;
            let user = user_usize as *mut u8;

            *hdr = MemBlockHeader::empty();
            (*hdr).size = need;
            (*hdr).set_large();
            (*hdr).owner = raw as *mut c_void;
            (*hdr).map_len = over;

            self.base_alloc.track_insert(hdr);
            user
        }
    }

    #[inline(always)]
    unsafe fn free_large(&self, hdr: *mut MemBlockHeader) {
        unsafe {
            self.base_alloc.track_remove(hdr);
            self.base_alloc.record_freed(ptr_from_header(hdr), hdr);
            (*hdr).poison();

            let base = (*hdr).owner;
            let len = (*hdr).map_len;
            if !base.is_null() && len != 0 {
                let _ = munmap(base, len);
            } else {
                let total = (*hdr).size + HEADER_SIZE;
                let _ = munmap(hdr as _, total);
            }
        }
    }
    pub unsafe fn inner_alloc(&self, layout: Layout) -> NonNull<u8> {
        #[cfg(feature = "multithread")]
        let _g = self.base_alloc.lock.guard();
        unsafe { self.do_alloc(layout) }

    }

    unsafe fn do_alloc(&self, layout: Layout) -> NonNull<u8> {
        unsafe {
            let need = core::cmp::max(1, layout.size());
            let align = layout.align();
            let ptr = if need >= LARGE_THRESHOLD || align > ALIGN {
                self.alloc_large_aligned(need, align)
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
        #[cfg(feature = "multithread")]
        let _g = self.base_alloc.lock.guard();
        unsafe { self.do_dealloc(ptr, _layout) }
    }

    unsafe fn do_dealloc(&self, ptr: *mut u8, _layout: Layout) {
        unsafe {
            if ptr.is_null() {
                return;
            }
            let hdr = header_from_ptr(ptr);
            if (*hdr).is_poisoned() {
                write("Use After Free\n");
                core::arch::asm!("int3", options(noreturn))
            }
            if (*hdr).is_large() {
                self.free_large(hdr);
                return;
            }

            let span = (*hdr).owner as *mut Span;
            if !((*hdr).owner as *mut Span == span) {
                write("hdr.owner != span\n");
                core::arch::asm!("int3", options(noreturn))
            }
            if !hdr_in_span(span, hdr) {
                write("hdr not in span!\n");
                writenum((hdr as usize) as u32);
                write(" hdr\n");
                writenum((*span).base as usize as u32);
                write(" base\n");
                writenum(((*span).base as usize + (*span).total_slots * (*span).class_size) as u32);
                write(" end\n");
                core::arch::asm!("int3", options(noreturn))
            }
            self.span_free_block(span, hdr);
        }
    }
    unsafe fn do_realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> NonNull<u8> {
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
    pub unsafe fn inner_realloc(
        &self,
        ptr: *mut u8,
        layout: Layout,
        new_size: usize,
    ) -> NonNull<u8> {
        unsafe {
            #[cfg(feature = "multithread")]
            let _g = self.base_alloc.lock.guard();
            self.do_realloc(ptr, layout, new_size)
        }
    }
}

unsafe fn hdr_in_span(span: *const Span, hdr: *const MemBlockHeader) -> bool {
    unsafe {
        if span.is_null() || hdr.is_null() {
            return false;
        }
        let base = (*span).base as usize;
        let end = base + (*span).total_slots * (*span).class_size;
        let h = hdr as usize;
        h >= base && (h + HEADER_SIZE) <= end && ((h - base) % (*span).class_size == 0)
    }
}
