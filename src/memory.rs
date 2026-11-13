//! Zenkai OS - Physical Frame Allocator (4KB Pages)
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! CA-6 Layer 8: Memory Subsystem, Frame Accounting & Physical Memory Management.

pub const PAGE_SIZE: usize = 4096;
pub const MAX_FRAMES: usize = 1024; // 4MB managed physical pool for educational demonstration

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysFrame {
    pub frame_number: usize,
}

impl PhysFrame {
    pub fn start_address(&self) -> usize {
        self.frame_number * PAGE_SIZE
    }
}

pub struct FrameAllocator {
    bitmap: [u64; MAX_FRAMES / 64],
    total_frames: usize,
    allocated_count: usize,
}

impl FrameAllocator {
    pub const fn new() -> Self {
        Self {
            bitmap: [0; MAX_FRAMES / 64],
            total_frames: MAX_FRAMES,
            allocated_count: 0,
        }
    }

    pub fn allocate_frame(&mut self) -> Option<PhysFrame> {
        for word_idx in 0..self.bitmap.len() {
            if self.bitmap[word_idx] != u64::MAX {
                for bit_idx in 0..64 {
                    let mask = 1u64 << bit_idx;
                    if (self.bitmap[word_idx] & mask) == 0 {
                        self.bitmap[word_idx] |= mask;
                        self.allocated_count += 1;
                        let frame_number = word_idx * 64 + bit_idx;
                        return Some(PhysFrame { frame_number });
                    }
                }
            }
        }
        None
    }

    pub fn deallocate_frame(&mut self, frame: PhysFrame) {
        if frame.frame_number < self.total_frames {
            let word_idx = frame.frame_number / 64;
            let bit_idx = frame.frame_number % 64;
            let mask = 1u64 << bit_idx;
            if (self.bitmap[word_idx] & mask) != 0 {
                self.bitmap[word_idx] &= !mask;
                self.allocated_count = self.allocated_count.saturating_sub(1);
            }
        }
    }

    pub fn total_frames(&self) -> usize {
        self.total_frames
    }

    pub fn allocated_frames(&self) -> usize {
        self.allocated_count
    }

    pub fn free_frames(&self) -> usize {
        self.total_frames - self.allocated_count
    }
}

