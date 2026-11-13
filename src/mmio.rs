//! Small helpers shared by the drivers that talk to memory-mapped controllers and DMA: register reads
//! and writes, a timeout wait, and zeroed DMA pages. Hardware only (UEFI build).

use core::ptr::{read_volatile, write_bytes, write_volatile};

use crate::{arch, log, mm};

pub const PAGE_BYTES: usize = 4096;

pub fn rd32(addr: u64) -> u32 {
    // SAFETY: `addr` is inside a register window that the driver mapped uncached, or DMA memory from the
    // frame allocator; both are plain memory-mapped and 4-byte aligned where this is used.
    unsafe { read_volatile(addr as *const u32) }
}

pub fn wr32(addr: u64, value: u32) {
    // SAFETY: as in `rd32`.
    unsafe { write_volatile(addr as *mut u32, value) }
}

/// 64-bit registers are written as two 32-bit halves, low first, which every controller accepts.
pub fn wr64(addr: u64, value: u64) {
    wr32(addr, value as u32);
    wr32(addr + 4, (value >> 32) as u32);
}

/// Polls `cond` until it is true or `ms` milliseconds have passed. Returns whether it became true.
pub fn wait_until(ms: u64, mut cond: impl FnMut() -> bool) -> bool {
    let t0 = arch::rdtsc();
    let ticks = log::tsc_hz() / 1000 * ms;
    loop {
        if cond() {
            return true;
        }
        if arch::rdtsc().wrapping_sub(t0) >= ticks {
            return false;
        }
        arch::pause();
    }
}

/// `frames` contiguous zeroed 4 KiB frames the device may DMA to. Without 64-bit addressing (`ac64`
/// false) the whole range must be below 4 GiB.
pub fn dma_frames(frames: usize, ac64: bool) -> Result<u64, &'static str> {
    let addr = mm::alloc_frames(frames).ok_or("out of memory")?;
    let bytes = (frames * PAGE_BYTES) as u64;
    if !ac64 && (addr + bytes - 1) >> 32 != 0 {
        mm::free_frames(addr, frames);
        return Err("the controller needs memory below 4 GiB");
    }
    // SAFETY: frames just taken from the allocator: page aligned, identity mapped, used by nobody else.
    unsafe { write_bytes(addr as *mut u8, 0, frames * PAGE_BYTES) };
    Ok(addr)
}

/// One zeroed 4 KiB DMA frame.
pub fn dma_page(ac64: bool) -> Result<u64, &'static str> {
    dma_frames(1, ac64)
}
