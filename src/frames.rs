//! Physical frame allocator: one bit per 4 KiB frame (1 = in use), next-fit search. Pure logic over a
//! caller-supplied bitmap so it can be unit-tested on the host.

pub const FRAME_SIZE: u64 = 4096;

pub struct FrameBitmap<'a> {
    bits: &'a mut [u64],
    /// Next-fit cursor (frame index) so repeated allocations do not rescan from zero.
    cursor: usize,
    free: usize,
}

impl<'a> FrameBitmap<'a> {
    /// All frames start as used; call [`mark_free`](Self::mark_free) for usable ranges.
    pub fn new(bits: &'a mut [u64]) -> Self {
        bits.fill(u64::MAX);
        Self { bits, cursor: 0, free: 0 }
    }

    pub fn capacity(&self) -> usize {
        self.bits.len() * 64
    }

    pub fn free_frames(&self) -> usize {
        self.free
    }

    fn is_used(&self, f: usize) -> bool {
        self.bits[f / 64] & (1 << (f % 64)) != 0
    }

    fn set(&mut self, f: usize, used: bool) {
        let (w, b) = (f / 64, 1u64 << (f % 64));
        let was_used = self.bits[w] & b != 0;
        if used && !was_used {
            self.bits[w] |= b;
            self.free -= 1;
        } else if !used && was_used {
            self.bits[w] &= !b;
            self.free += 1;
        }
    }

    /// Frees `count` frames starting at frame number `first`; frames beyond capacity are ignored.
    /// Returns how many frames were actually inside the bitmap.
    pub fn mark_free(&mut self, first: usize, count: usize) -> usize {
        let end = first.saturating_add(count).min(self.capacity());
        for f in first.min(end)..end {
            self.set(f, false);
        }
        end.saturating_sub(first)
    }

    /// Marks a range used (for carving out kernel memory from a free range).
    pub fn mark_used(&mut self, first: usize, count: usize) {
        let end = first.saturating_add(count).min(self.capacity());
        for f in first.min(end)..end {
            self.set(f, true);
        }
    }

    pub fn alloc(&mut self) -> Option<usize> {
        self.alloc_contig(1)
    }

    /// Allocates `n` physically contiguous frames; returns the first frame number.
    pub fn alloc_contig(&mut self, n: usize) -> Option<usize> {
        if n == 0 || n > self.free {
            return None;
        }
        let cap = self.capacity();
        // Two passes: from the cursor to the end, then from 0 up to the cursor.
        for (lo, hi) in [(self.cursor, cap), (0, self.cursor.saturating_add(n - 1).min(cap))] {
            let mut run = 0usize;
            let mut f = lo;
            while f < hi {
                if self.is_used(f) {
                    run = 0;
                    // Skip a fully used word quickly.
                    if f % 64 == 0 && self.bits[f / 64] == u64::MAX {
                        f += 64;
                        continue;
                    }
                } else {
                    run += 1;
                    if run == n {
                        let start = f + 1 - n;
                        for g in start..=f {
                            self.set(g, true);
                        }
                        self.cursor = f + 1;
                        return Some(start);
                    }
                }
                f += 1;
            }
        }
        None
    }

    /// Returns a frame to the pool. Double frees are ignored.
    pub fn free(&mut self, f: usize) {
        if f < self.capacity() {
            self.set(f, false);
        }
    }

    /// Length of the longest run of free frames (for fragmentation reporting).
    pub fn largest_free_run(&self) -> usize {
        let (mut best, mut run) = (0, 0);
        for f in 0..self.capacity() {
            if self.is_used(f) {
                run = 0;
            } else {
                run += 1;
                best = best.max(run);
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_fully_used() {
        let mut bits = [0u64; 4];
        let mut fb = FrameBitmap::new(&mut bits);
        assert_eq!(fb.free_frames(), 0);
        assert_eq!(fb.alloc(), None);
    }

    #[test]
    fn alloc_and_free_round_trip() {
        let mut bits = [0u64; 4];
        let mut fb = FrameBitmap::new(&mut bits);
        assert_eq!(fb.mark_free(10, 20), 20);
        assert_eq!(fb.free_frames(), 20);
        let a = fb.alloc().unwrap();
        assert_eq!(a, 10);
        fb.free(a);
        fb.free(a); // double free is harmless
        assert_eq!(fb.free_frames(), 20);
    }

    #[test]
    fn contiguous_allocation_skips_holes() {
        let mut bits = [0u64; 4];
        let mut fb = FrameBitmap::new(&mut bits);
        fb.mark_free(0, 4);
        fb.mark_free(16, 8);
        fb.mark_used(20, 1); // runs are now 0..4, 16..20 and 21..24
        assert_eq!(fb.alloc_contig(5), None);
        assert_eq!(fb.alloc_contig(4), Some(0));
        assert_eq!(fb.alloc_contig(4), Some(16));
        assert_eq!(fb.alloc_contig(3), Some(21));
        assert_eq!(fb.free_frames(), 0);
    }

    #[test]
    fn wraps_around_to_find_earlier_frames() {
        let mut bits = [0u64; 2];
        let mut fb = FrameBitmap::new(&mut bits);
        fb.mark_free(0, 128);
        assert_eq!(fb.alloc_contig(100), Some(0));
        fb.free(3);
        fb.free(4);
        assert_eq!(fb.alloc_contig(28), Some(100)); // cursor is now at the end of the bitmap
        assert_eq!(fb.alloc_contig(2), Some(3)); // found by wrapping to the start
        assert_eq!(fb.free_frames(), 0);
    }

    #[test]
    fn out_of_range_is_ignored() {
        let mut bits = [0u64; 1];
        let mut fb = FrameBitmap::new(&mut bits);
        assert_eq!(fb.mark_free(60, 100), 4);
        assert_eq!(fb.free_frames(), 4);
        assert_eq!(fb.mark_free(500, 5), 0);
    }

    #[test]
    fn largest_run() {
        let mut bits = [0u64; 2];
        let mut fb = FrameBitmap::new(&mut bits);
        fb.mark_free(2, 3);
        fb.mark_free(10, 7);
        assert_eq!(fb.largest_free_run(), 7);
    }
}
