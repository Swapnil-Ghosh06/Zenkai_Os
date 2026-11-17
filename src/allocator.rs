//! Zenkai OS - FreeList Dynamic Heap Allocator
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! CA-6 Layer 8: Dynamic Memory Allocation, First-Fit Strategy & Block Coalescing.

pub const HEAP_SIZE: usize = 64 * 1024; // 64 KB heap pool for kernel structures

static mut KERNEL_HEAP_POOL: [u8; HEAP_SIZE] = [0; HEAP_SIZE];

#[repr(C)]
pub struct BlockHeader {
    pub size: usize,
    pub is_free: bool,
    pub next: Option<*mut BlockHeader>,
}

pub struct FreeListAllocator {
    head: Option<*mut BlockHeader>,
    pub total_allocated: usize,
    pub peak_allocated: usize,
}

impl FreeListAllocator {
    pub const fn new() -> Self {
        Self {
            head: None,
            total_allocated: 0,
            peak_allocated: 0,
        }
    }

    pub fn init(&mut self) {
        let base_ptr = core::ptr::addr_of_mut!(KERNEL_HEAP_POOL) as *mut BlockHeader;
        let header_size = core::mem::size_of::<BlockHeader>();
        let initial_capacity = HEAP_SIZE - header_size;

        unsafe {
            base_ptr.write(BlockHeader {
                size: initial_capacity,
                is_free: true,
                next: None,
            });
        }
        self.head = Some(base_ptr);
    }

    pub fn allocate(&mut self, size: usize) -> Option<*mut u8> {
        let align = core::mem::align_of::<usize>();
        let aligned_size = (size + align - 1) & !(align - 1);
        let header_size = core::mem::size_of::<BlockHeader>();

        let mut current = self.head;
        while let Some(block_ptr) = current {
            let block = unsafe { &mut *block_ptr };
            if block.is_free && block.size >= aligned_size {
                if block.size >= aligned_size + header_size + 16 {
                    // Split the block
                    let remaining_size = block.size - aligned_size - header_size;
                    let next_block_ptr = unsafe {
                        (block_ptr as *mut u8).add(header_size + aligned_size) as *mut BlockHeader
                    };

                    unsafe {
                        next_block_ptr.write(BlockHeader {
                            size: remaining_size,
                            is_free: true,
                            next: block.next,
                        });
                    }

                    block.size = aligned_size;
                    block.is_free = false;
                    block.next = Some(next_block_ptr);
                } else {
                    block.is_free = false;
                }

                self.total_allocated += block.size;
                if self.total_allocated > self.peak_allocated {
                    self.peak_allocated = self.total_allocated;
                }

                let payload_ptr = unsafe { (block_ptr as *mut u8).add(header_size) };
                return Some(payload_ptr);
            }
            current = block.next;
        }
        None
    }

    pub fn deallocate(&mut self, ptr: *mut u8) {
        if ptr.is_null() {
            return;
        }

        let header_size = core::mem::size_of::<BlockHeader>();
        let block_ptr = unsafe { ptr.sub(header_size) as *mut BlockHeader };
        let block = unsafe { &mut *block_ptr };
        block.is_free = true;
        self.total_allocated = self.total_allocated.saturating_sub(block.size);

        // Coalesce adjacent free blocks
        self.coalesce();
    }

    fn coalesce(&mut self) {
        let mut current = self.head;
        let header_size = core::mem::size_of::<BlockHeader>();

        while let Some(curr_ptr) = current {
            let curr = unsafe { &mut *curr_ptr };
            if let Some(next_ptr) = curr.next {
                let next = unsafe { &mut *next_ptr };
                if curr.is_free && next.is_free {
                    curr.size += header_size + next.size;
                    curr.next = next.next;
                    continue; // Re-check this block with the new next
                }
            }
            current = curr.next;
        }
    }

    pub fn block_stats(&self) -> (usize, usize, usize) {
        let mut total_blocks = 0;
        let mut free_blocks = 0;
        let mut max_free = 0;
        let mut current = self.head;
        while let Some(curr_ptr) = current {
            let curr = unsafe { &*curr_ptr };
            total_blocks += 1;
            if curr.is_free {
                free_blocks += 1;
                if curr.size > max_free {
                    max_free = curr.size;
                }
            }
            current = curr.next;
        }
        (total_blocks, free_blocks, max_free)
    }
}
