//! Physical Memory Management & Frame Tracking (CA-6 Layer 8)
//!
//! Tracks 4KB physical page frames and provides system memory accounting.

use crate::kinfo;

pub const PAGE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy)]
pub struct MemoryStats {
    pub total_memory_bytes: usize,
    pub total_frames: usize,
    pub allocated_frames: usize,
    pub free_frames: usize,
}

pub struct PhysicalFrameAllocator {
    total_frames: usize,
    allocated_frames: usize,
}

impl PhysicalFrameAllocator {
    pub const fn new(total_frames: usize) -> Self {
        Self {
            total_frames,
            allocated_frames: 0,
        }
    }

    /// Allocate a 4KB physical frame
    pub fn allocate_frame(&mut self) -> Option<usize> {
        if self.allocated_frames < self.total_frames {
            let frame_idx = self.allocated_frames;
            self.allocated_frames += 1;
            Some(frame_idx)
        } else {
            None
        }
    }

    /// Free an allocated frame
    pub fn deallocate_frame(&mut self) {
        if self.allocated_frames > 0 {
            self.allocated_frames -= 1;
        }
    }

    pub fn stats(&self) -> MemoryStats {
        MemoryStats {
            total_memory_bytes: self.total_frames * PAGE_SIZE,
            total_frames: self.total_frames,
            allocated_frames: self.allocated_frames,
            free_frames: self.total_frames.saturating_sub(self.allocated_frames),
        }
    }
}

pub fn print_memory_overview(stats: &MemoryStats) {
    kinfo!("MEM", "=== Physical Memory Subsystem ===");
    kinfo!("MEM", "Page Frame Size     : {} bytes (4 KB)", PAGE_SIZE);
    kinfo!("MEM", "Total Physical RAM  : {} MB ({} frames)", stats.total_memory_bytes / (1024 * 1024), stats.total_frames);
    kinfo!("MEM", "Allocated Frames    : {} ({} KB)", stats.allocated_frames, (stats.allocated_frames * PAGE_SIZE) / 1024);
    kinfo!("MEM", "Available Frames    : {} ({} KB)", stats.free_frames, (stats.free_frames * PAGE_SIZE) / 1024);
}

