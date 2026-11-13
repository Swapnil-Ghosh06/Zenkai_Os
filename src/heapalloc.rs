//! First-fit free-list heap with coalescing. Pure logic over a caller-supplied arena so it can be
//! tested on the host; the kernel wraps it in `GlobalAlloc` (see `mm`).

use core::alloc::Layout;
use core::ptr::{self, NonNull};

/// Free block header, stored in the free memory itself. 16 bytes, so every block is a multiple of 16.
#[repr(C)]
struct Hole {
    size: usize,
    next: *mut Hole,
}

const MIN: usize = core::mem::size_of::<Hole>();
const ALIGN: usize = 16;

fn round_up(v: usize, a: usize) -> usize {
    (v + a - 1) & !(a - 1)
}

pub struct Heap {
    /// Address-ordered list of free holes.
    head: *mut Hole,
    start: usize,
    end: usize,
    used: usize,
}

// SAFETY: the heap owns its arena exclusively; callers serialise access (single CPU, interrupts off).
unsafe impl Send for Heap {}

impl Heap {
    pub const fn empty() -> Self {
        Self { head: ptr::null_mut(), start: 0, end: 0, used: 0 }
    }

    /// Gives the heap its arena.
    ///
    /// # Safety
    /// `[start, start + size)` must be valid, writable, unused memory that lives as long as the heap.
    pub unsafe fn init(&mut self, start: usize, size: usize) {
        let s = round_up(start, ALIGN);
        let e = (start + size) & !(ALIGN - 1);
        if e <= s + MIN {
            return;
        }
        self.start = s;
        self.end = e;
        self.used = 0;
        let h = s as *mut Hole;
        // SAFETY: the arena is at least one hole long and exclusively ours (contract above).
        unsafe { h.write(Hole { size: e - s, next: ptr::null_mut() }) };
        self.head = h;
    }

    pub fn used(&self) -> usize {
        self.used
    }

    pub fn capacity(&self) -> usize {
        self.end - self.start
    }

    pub(crate) fn adjust(layout: Layout) -> (usize, usize) {
        (round_up(layout.size().max(1), ALIGN).max(MIN), layout.align().max(ALIGN))
    }

    pub fn alloc(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        let (size, align) = Self::adjust(layout);
        let mut prev: *mut Hole = ptr::null_mut();
        let mut cur = self.head;
        while !cur.is_null() {
            // SAFETY: every node on the list is a live Hole written by this module.
            let (hsize, hnext) = unsafe { ((*cur).size, (*cur).next) };
            let a = cur as usize;
            let aligned = round_up(a, align);
            let front = aligned - a;
            if hsize >= front + size {
                let back = hsize - front - size;
                // Both gaps are multiples of 16, so each is either 0 or large enough for a Hole.
                let mut link = hnext;
                if back != 0 {
                    let b = (aligned + size) as *mut Hole;
                    // SAFETY: `b` lies inside the hole being split.
                    unsafe { b.write(Hole { size: back, next: hnext }) };
                    link = b;
                }
                if front != 0 {
                    // SAFETY: shrinking the existing hole header in place.
                    unsafe {
                        (*cur).size = front;
                        (*cur).next = link;
                    }
                } else if prev.is_null() {
                    self.head = link;
                } else {
                    // SAFETY: `prev` is a live node.
                    unsafe { (*prev).next = link };
                }
                self.used += size;
                return NonNull::new(aligned as *mut u8);
            }
            prev = cur;
            cur = hnext;
        }
        None
    }

    /// Returns a block to the heap.
    ///
    /// # Safety
    /// `p` must have been returned by `alloc` on this heap with the same `layout`, and not freed since.
    pub unsafe fn dealloc(&mut self, p: NonNull<u8>, layout: Layout) {
        let (size, _) = Self::adjust(layout);
        let addr = p.as_ptr() as usize;
        debug_assert!(addr >= self.start && addr + size <= self.end);
        self.used = self.used.saturating_sub(size);

        // Find the insertion point in address order.
        let mut prev: *mut Hole = ptr::null_mut();
        let mut cur = self.head;
        // SAFETY: list nodes are live Holes.
        unsafe {
            while !cur.is_null() && (cur as usize) < addr {
                prev = cur;
                cur = (*cur).next;
            }
            let node = addr as *mut Hole;
            node.write(Hole { size, next: cur });
            if prev.is_null() {
                self.head = node;
            } else {
                (*prev).next = node;
            }
            // Merge with the following hole, then with the preceding one.
            if !cur.is_null() && addr + (*node).size == cur as usize {
                (*node).size += (*cur).size;
                (*node).next = (*cur).next;
            }
            if !prev.is_null() && prev as usize + (*prev).size == addr {
                (*prev).size += (*node).size;
                (*prev).next = (*node).next;
            }
        }
    }

    /// Number of free holes (a fragmentation indicator).
    pub fn hole_count(&self) -> usize {
        let (mut n, mut cur) = (0, self.head);
        while !cur.is_null() {
            n += 1;
            // SAFETY: list nodes are live Holes.
            cur = unsafe { (*cur).next };
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arena(bytes: usize) -> (std::vec::Vec<u128>, usize) {
        let v = std::vec![0u128; bytes / 16];
        let p = v.as_ptr() as usize;
        (v, p)
    }

    fn heap_of(bytes: usize) -> (Heap, std::vec::Vec<u128>) {
        let (v, p) = arena(bytes);
        let mut h = Heap::empty();
        // SAFETY: `v` outlives the heap in each test and is not touched otherwise.
        unsafe { h.init(p, bytes) };
        (h, v)
    }

    #[test]
    fn allocations_are_aligned_and_disjoint() {
        let (mut h, _v) = heap_of(4096);
        let a = h.alloc(Layout::from_size_align(24, 8).unwrap()).unwrap().as_ptr() as usize;
        let b = h.alloc(Layout::from_size_align(100, 64).unwrap()).unwrap().as_ptr() as usize;
        assert_eq!(a % 16, 0);
        assert_eq!(b % 64, 0);
        assert!(b >= a + 32 || a >= b + 112);
    }

    #[test]
    fn free_coalesces_back_to_one_hole() {
        let (mut h, _v) = heap_of(4096);
        let l = Layout::from_size_align(64, 16).unwrap();
        let p: std::vec::Vec<_> = (0..10).map(|_| h.alloc(l).unwrap()).collect();
        for x in p.iter().rev().step_by(2) {
            // SAFETY: allocated above with the same layout.
            unsafe { h.dealloc(*x, l) };
        }
        assert!(h.hole_count() > 1);
        for x in p.iter().rev().skip(1).step_by(2) {
            // SAFETY: allocated above with the same layout.
            unsafe { h.dealloc(*x, l) };
        }
        assert_eq!(h.hole_count(), 1);
        assert_eq!(h.used(), 0);
        assert!(h.alloc(Layout::from_size_align(4000, 16).unwrap()).is_some());
    }

    #[test]
    fn exhaustion_returns_none_and_recovers() {
        let (mut h, _v) = heap_of(1024);
        let l = Layout::from_size_align(512, 16).unwrap();
        let a = h.alloc(l).unwrap();
        assert!(h.alloc(Layout::from_size_align(600, 16).unwrap()).is_none());
        // SAFETY: allocated above with this layout.
        unsafe { h.dealloc(a, l) };
        assert!(h.alloc(Layout::from_size_align(600, 16).unwrap()).is_some());
    }

    #[test]
    fn large_alignment_leaves_a_usable_front_hole() {
        let (mut h, _v) = heap_of(8192);
        let big = Layout::from_size_align(16, 1024).unwrap();
        let p = h.alloc(big).unwrap();
        assert_eq!(p.as_ptr() as usize % 1024, 0);
        // SAFETY: allocated above with this layout.
        unsafe { h.dealloc(p, big) };
        assert_eq!(h.hole_count(), 1);
    }

    #[test]
    fn memory_is_writable_without_corrupting_neighbours() {
        let (mut h, _v) = heap_of(4096);
        let l = Layout::from_size_align(48, 16).unwrap();
        let a = h.alloc(l).unwrap();
        let b = h.alloc(l).unwrap();
        // SAFETY: both blocks are live and 48 bytes long.
        unsafe {
            ptr::write_bytes(a.as_ptr(), 0xAA, 48);
            ptr::write_bytes(b.as_ptr(), 0xBB, 48);
            assert_eq!(*a.as_ptr().add(47), 0xAA);
            h.dealloc(a, l);
            assert_eq!(*b.as_ptr(), 0xBB);
            h.dealloc(b, l);
        }
    }
}
