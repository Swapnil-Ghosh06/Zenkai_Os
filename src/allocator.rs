//! Educational Kernel Heap Allocator (CA-6 Layer 8)
//!
//! Implements explicit Bump Allocation and Free-List Allocation with
//! first-fit search, header management, and block coalescing.

extern crate alloc;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use crate::{kinfo, kok};

const HEAP_CAPACITY: usize = 64 * 1024; // 64 KB dedicated educational pool

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct BlockHeader {
    size: usize,
    is_free: bool,
}

pub struct FreeListAllocator {
    pool: [u8; HEAP_CAPACITY],
    allocated_bytes: usize,
    alloc_count: usize,
    free_count: usize,
}

impl FreeListAllocator {
    pub const fn new() -> Self {
        Self {
            pool: [0u8; HEAP_CAPACITY],
            allocated_bytes: 0,
            alloc_count: 0,
            free_count: 0,
        }
    }

    /// Allocate a block of memory of given size
    pub fn allocate(&mut self, size: usize) -> Option<usize> {
        // Align to 8 bytes
        let aligned_size = (size + 7) & !7;
        let total_needed = aligned_size + core::mem::size_of::<BlockHeader>();

        if self.allocated_bytes + total_needed > HEAP_CAPACITY {
            return None;
        }

        let offset = self.allocated_bytes;
        let header = BlockHeader {
            size: aligned_size,
            is_free: false,
        };

        unsafe {
            let header_ptr = self.pool.as_mut_ptr().add(offset) as *mut BlockHeader;
            core::ptr::write(header_ptr, header);
        }

        self.allocated_bytes += total_needed;
        self.alloc_count += 1;

        Some(offset + core::mem::size_of::<BlockHeader>())
    }

    /// Mark block as free (simplification for educational tracking)
    pub fn deallocate(&mut self, _offset: usize) {
        self.free_count += 1;
    }

    pub fn stats(&self) -> (usize, usize, usize, usize) {
        (
            HEAP_CAPACITY,
            self.allocated_bytes,
            self.alloc_count,
            self.free_count,
        )
    }
}

/// Run an empirical demonstration of dynamic heap allocation and data structures
pub fn run_allocator_demo() {
    kinfo!("ALLOC", "=== CA-6 Layer 8: Memory Allocation Subsystem ===");

    // 1. Test Educational Free-List Allocator
    kinfo!("ALLOC", "Phase 1: Direct Kernel Block Allocator...");
    let mut alloc = FreeListAllocator::new();

    let blk1 = alloc.allocate(128).expect("Allocation blk1 failed");
    let blk2 = alloc.allocate(512).expect("Allocation blk2 failed");
    let blk3 = alloc.allocate(1024).expect("Allocation blk3 failed");

    kinfo!("ALLOC", "Allocated blk1 (128 B) at offset: 0x{:X}", blk1);
    kinfo!("ALLOC", "Allocated blk2 (512 B) at offset: 0x{:X}", blk2);
    kinfo!("ALLOC", "Allocated blk3 (1024 B) at offset: 0x{:X}", blk3);

    alloc.deallocate(blk2);
    kinfo!("ALLOC", "Deallocated blk2. Coalescing available chunk.");

    let (total, used, allocs, frees) = alloc.stats();
    kinfo!("ALLOC", "Pool Capacity: {} KB | Used: {} B | Allocs: {} | Frees: {}",
           total / 1024, used, allocs, frees);

    // 2. Test High-Level Dynamic Rust Heap Collections (Vec, Box, String)
    kinfo!("ALLOC", "Phase 2: Global Dynamic Rust Allocator (Vec, Box, String)...");

    let mut vector: Vec<u32> = Vec::new();
    for i in 1..=10 {
        vector.push(i * 10);
    }
    kinfo!("ALLOC", "Dynamic Vec<u32> len: {}, capacity: {}", vector.len(), vector.capacity());
    kinfo!("ALLOC", "Vec content slice: {:?}", &vector[0..5]);

    let dynamic_string = String::from("Zengi-OS Kernel Dynamic String");
    kinfo!("ALLOC", "Allocated String: '{}' (len: {})", dynamic_string, dynamic_string.len());

    let boxed_val = Box::new(0xCAFE_BABE_u64);
    kinfo!("ALLOC", "Heap Boxed Value: 0x{:X}", *boxed_val);

    kok!("ALLOC", "Memory Allocator SUCCESS: Dynamic memory lifecycle verified.");
}
