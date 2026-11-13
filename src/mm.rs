//! Kernel memory management: the physical frame allocator and the kernel heap (`alloc` support).

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::NonNull;

use crate::arch;
use crate::bootinfo::BootInfo;
use crate::frames::{FrameBitmap, FRAME_SIZE};
use crate::global::Global;
use crate::heapalloc::Heap;
use crate::memmap::Kind;

/// 4 Mi frames = 16 GiB of physical memory tracked, 512 KiB of bitmap.
const BITMAP_WORDS: usize = 65536;
const LOW_MEMORY: u64 = 1 << 20;

static BITMAP: Global<[u64; BITMAP_WORDS]> = Global::new([0; BITMAP_WORDS]);
static FRAMES: Global<Option<FrameBitmap<'static>>> = Global::new(None);
static HEAP: Global<Heap> = Global::new(Heap::empty());
static TOTAL_FRAMES: Global<usize> = Global::new(0);

#[derive(Clone, Copy, Debug)]
pub struct FrameStats {
    pub total: usize,
    pub free: usize,
    pub largest_run: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct HeapStats {
    pub capacity: usize,
    pub used: usize,
    pub holes: usize,
}

/// Builds the frame allocator from the final memory map and carves out the kernel image.
///
/// # Safety
/// Boot services must have exited, `info` must describe the real memory map, and this must run once.
pub unsafe fn init_frames(info: &BootInfo) -> FrameStats {
    // SAFETY: single-threaded init; these statics are touched nowhere else until it returns.
    let (bits, frames, total) = unsafe { (BITMAP.get(), FRAMES.get(), TOTAL_FRAMES.get()) };
    let mut fb = FrameBitmap::new(bits);
    let mut tracked = 0usize;
    for r in info.regions() {
        // Loader and boot-services memory is free now that boot services are gone.
        if !matches!(r.kind, Kind::Usable | Kind::Reclaimable) {
            continue;
        }
        let start = r.start.max(LOW_MEMORY);
        if r.end() <= start {
            continue;
        }
        let first = (start / FRAME_SIZE) as usize;
        let count = ((r.end() - start) / FRAME_SIZE) as usize;
        tracked += fb.mark_free(first, count);
    }
    // The kernel image (code, statics, page tables, kernel stack) sits in loader memory: keep it.
    let img_first = (info.image_base / FRAME_SIZE) as usize;
    let img_pages = (info.image_size.div_ceil(FRAME_SIZE)) as usize;
    fb.mark_used(img_first, img_pages);
    // Zero the memory just released by the firmware (like Linux's init_on_free): it can still hold
    // firmware data, and nothing we allocate later should ever read it.
    let (ib, ie) = (info.image_base, info.image_base + info.image_size.next_multiple_of(FRAME_SIZE));
    for r in info.regions().iter().filter(|r| r.kind == Kind::Reclaimable) {
        let (s, e) = (r.start.max(LOW_MEMORY), r.end());
        let zero = |a: u64, b: u64| {
            if b > a {
                // SAFETY: loader/boot-services memory is identity-mapped, writable and unused now
                // that boot services have exited; the kernel image range is excluded by the caller.
                unsafe { core::ptr::write_bytes(a as *mut u8, 0, (b - a) as usize) };
            }
        };
        if e <= ib || s >= ie {
            zero(s, e);
        } else {
            zero(s, ib.min(e));
            zero(ie.max(s), e);
        }
    }
    *total = fb.free_frames();
    let _ = tracked;
    let stats = FrameStats { total: *total, free: fb.free_frames(), largest_run: fb.largest_free_run() };
    *frames = Some(fb);
    stats
}

/// Allocates `n` contiguous physical frames and returns the physical (= virtual) address.
pub fn alloc_frames(n: usize) -> Option<u64> {
    arch::without_interrupts(|| {
        // SAFETY: interrupts are off on a single CPU, so no other reference to the allocator exists.
        let f = unsafe { FRAMES.get_irqs_off() }.as_mut()?;
        f.alloc_contig(n).map(|i| i as u64 * FRAME_SIZE)
    })
}

/// Returns frames obtained from [`alloc_frames`].
pub fn free_frames(addr: u64, n: usize) {
    arch::without_interrupts(|| {
        // SAFETY: as in `alloc_frames`.
        if let Some(f) = unsafe { FRAMES.get_irqs_off() }.as_mut() {
            let first = (addr / FRAME_SIZE) as usize;
            for i in first..first + n {
                f.free(i);
            }
        }
    });
}

pub fn frame_stats() -> FrameStats {
    arch::without_interrupts(|| {
        // SAFETY: as in `alloc_frames`.
        let total = unsafe { *TOTAL_FRAMES.get_irqs_off() };
        // SAFETY: as above; interrupts are off.
        match unsafe { FRAMES.get_irqs_off() }.as_ref() {
            Some(f) => FrameStats { total, free: f.free_frames(), largest_run: f.largest_free_run() },
            None => FrameStats { total: 0, free: 0, largest_run: 0 },
        }
    })
}

/// Carves the kernel heap out of free frames. Returns its size in bytes.
///
/// # Safety
/// `init_frames` must have run; call once, before the first heap allocation.
pub unsafe fn init_heap() -> usize {
    let free = frame_stats().free;
    // Up to 32 MiB, but never more than a quarter of what is free.
    let want = (32usize << 20) / FRAME_SIZE as usize;
    let mut frames = want.min(free / 4).max(64);
    let base = loop {
        if let Some(b) = alloc_frames(frames) {
            break b;
        }
        frames /= 2;
        if frames < 16 {
            return 0;
        }
    };
    let size = frames * FRAME_SIZE as usize;
    // SAFETY: the frames were just allocated exclusively for the heap and are identity-mapped.
    unsafe { HEAP.get().init(base as usize, size) };
    size
}

pub fn heap_stats() -> HeapStats {
    arch::without_interrupts(|| {
        // SAFETY: interrupts off on a single CPU.
        let h = unsafe { HEAP.get_irqs_off() };
        HeapStats { capacity: h.capacity(), used: h.used(), holes: h.hole_count() }
    })
}

pub struct KernelAlloc;

// SAFETY: allocation and deallocation are serialised by disabling interrupts on a single CPU, and
// the heap hands out disjoint, correctly aligned blocks from memory it exclusively owns.
unsafe impl GlobalAlloc for KernelAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: interrupts are disabled inside, so no other reference to HEAP exists.
        arch::without_interrupts(|| unsafe { HEAP.get_irqs_off() }.alloc(layout))
            .map_or(core::ptr::null_mut(), NonNull::as_ptr)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if let Some(p) = NonNull::new(ptr) {
            // Scrub freed memory so one task's data never shows up in another's allocation.
            // SAFETY: the block spans the allocator's rounded size (>= `layout.size()`) and is no longer in use.
            unsafe { core::ptr::write_bytes(ptr, 0, crate::heapalloc::Heap::adjust(layout).0) };
            // SAFETY: the caller guarantees `ptr`/`layout` came from `alloc`; interrupts are off.
            arch::without_interrupts(|| unsafe { HEAP.get_irqs_off().dealloc(p, layout) });
        }
    }
}

#[global_allocator]
static ALLOCATOR: KernelAlloc = KernelAlloc;
