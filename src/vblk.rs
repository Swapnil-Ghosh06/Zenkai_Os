//! virtio-blk driver (virtio 1.0 over PCI), polling, one request at a time.
//!
//! Setup follows the virtio spec's order: reset, acknowledge, driver, features (only VERSION_1), features
//! ok, queue setup, driver ok. A request is three chained descriptors (header, data, status). All memory
//! the device sees comes from the frame allocator, which is identity mapped, so physical = virtual.

use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, AtomicBool, Ordering};

use crate::block::{check_request, BlockDev, BlockError, SECTOR_SIZE};
use crate::global::Global;
use crate::{arch, log, mm, paging, pci, virtio};

// Common configuration register offsets (virtio spec 4.1.4.3).
const DEVICE_FEATURE_SELECT: u64 = 0;
const DEVICE_FEATURE: u64 = 4;
const DRIVER_FEATURE_SELECT: u64 = 8;
const DRIVER_FEATURE: u64 = 12;
const DEVICE_STATUS: u64 = 20;
const QUEUE_SELECT: u64 = 22;
const QUEUE_SIZE: u64 = 24;
const QUEUE_ENABLE: u64 = 28;
const QUEUE_NOTIFY_OFF: u64 = 30;
const QUEUE_DESC: u64 = 32;
const QUEUE_DRIVER: u64 = 40;
const QUEUE_DEVICE: u64 = 48;

const STATUS_ACKNOWLEDGE: u8 = 1;
const STATUS_DRIVER: u8 = 2;
const STATUS_DRIVER_OK: u8 = 4;
const STATUS_FEATURES_OK: u8 = 8;
const STATUS_FAILED: u8 = 128;

const FEATURE_VERSION_1: u32 = 1; // bit 32, seen through feature word 1

const DESC_NEXT: u16 = 1;
const DESC_WRITE: u16 = 2;

const BLK_T_IN: u32 = 0;
/// Sectors per request: the data buffer is one 4 KiB frame.
const MAX_SECTORS: usize = 8;
const TIMEOUT_SECONDS: u64 = 2;

pub struct VirtioBlk {
    common: u64,
    notify: u64,
    sectors: u64,
    qsize: u16,
    ring: u64,
    layout: virtio::RingLayout,
    /// Frame holding the request header and status byte.
    req: u64,
    /// Frame the device writes sector data into.
    data: u64,
    avail_idx: u16,
    last_used: u16,
}

fn rd8(a: u64) -> u8 {
    // SAFETY: `a` is inside a device register window or DMA frame this driver mapped (see `probe`).
    unsafe { read_volatile(a as *const u8) }
}
fn rd16(a: u64) -> u16 {
    // SAFETY: as in `rd8`, 2-byte aligned by the register layout.
    unsafe { read_volatile(a as *const u16) }
}
fn rd32(a: u64) -> u32 {
    // SAFETY: as in `rd8`, 4-byte aligned by the register layout.
    unsafe { read_volatile(a as *const u32) }
}
fn wr8(a: u64, v: u8) {
    // SAFETY: as in `rd8`.
    unsafe { write_volatile(a as *mut u8, v) };
}
fn wr16(a: u64, v: u16) {
    // SAFETY: as in `rd16`.
    unsafe { write_volatile(a as *mut u16, v) };
}
fn wr32(a: u64, v: u32) {
    // SAFETY: as in `rd32`.
    unsafe { write_volatile(a as *mut u32, v) };
}
fn wr64(a: u64, v: u64) {
    wr32(a, v as u32);
    wr32(a + 4, (v >> 32) as u32);
}

/// Address of a BAR (memory, 32 or 64 bit), mapped uncached. `None` for I/O BARs or an unusable one.
fn bar_base(d: &pci::Device, bar: u8) -> Option<u64> {
    pci::memory_bar(d, bar)
}

/// The disk, if there is one. `DISK_BUSY` is the only thing that lets code touch it (see `with_disk`).
///
/// This is a flag and not a `SpinLock` on purpose: a `SpinLock` switches interrupts off, and a disk
/// request polls for up to `TIMEOUT_SECONDS`. That would stop the timer, the scheduler and the
/// keyboard polling for the whole wait. With a flag the timer keeps running during a request.
static DISK: Global<Option<VirtioBlk>> = Global::new(None);
static DISK_BUSY: AtomicBool = AtomicBool::new(false);

/// Takes the disk. If another task is in the middle of a request, sleeps 10 ms and tries again.
/// Never call it from an interrupt handler (it may sleep).
fn take_disk() {
    while DISK_BUSY.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        crate::sched::sleep_ms(10);
    }
}

fn release_disk() {
    DISK_BUSY.store(false, Ordering::Release);
}

/// Finds a virtio-blk device and brings it up. Logs the result; keeps the device for `with_disk`.
pub fn probe() {
    let Some(dev) = pci::find(virtio::VENDOR, virtio::DEVICE_BLK_MODERN).or_else(|| pci::find(virtio::VENDOR, virtio::DEVICE_BLK_TRANSITIONAL)) else {
        info!("vblk", "no virtio-blk device");
        return;
    };
    match VirtioBlk::init(&dev) {
        Ok(d) => {
            info!("vblk", "virtio-blk at {:02x}:{:02x}.{}: {} sectors ({} KiB), queue size {}", dev.bus, dev.dev, dev.func, d.sectors, d.sectors / 2, d.qsize);
            take_disk();
            // SAFETY: `DISK_BUSY` is held, so no other reference to the disk exists.
            unsafe { *DISK.get() = Some(d) };
            release_disk();
        }
        Err(e) => warn!("vblk", "virtio-blk at {:02x}:{:02x}.{} not usable: {}", dev.bus, dev.dev, dev.func, e),
    }
}

/// Runs `f` on the disk, or returns `None` when there is none. Interrupts stay on while `f` runs, so
/// `f` may be slow (a request can poll for seconds); another task asking for the disk waits its turn.
/// Do not call it again from inside `f`, and never from an interrupt handler.
pub fn with_disk<R>(f: impl FnOnce(&mut VirtioBlk) -> R) -> Option<R> {
    take_disk();
    // SAFETY: `DISK_BUSY` is held until `release_disk`, so this is the only reference to the disk.
    let r = unsafe { DISK.get() }.as_mut().map(f);
    release_disk();
    r
}

impl VirtioBlk {
    fn init(d: &pci::Device) -> Result<VirtioBlk, &'static str> {
        let caps = virtio::find_caps(|off| pci::config_read(d.bus, d.dev, d.func, off));
        let (Some(common), Some(notify), Some(devcfg)) = (caps.common, caps.notify, caps.device) else {
            return Err("missing virtio capabilities");
        };
        // Allow the device to answer memory reads and to write main memory.
        let cmd = pci::config_read(d.bus, d.dev, d.func, 4);
        pci::config_write(d.bus, d.dev, d.func, 4, (cmd & 0xFFFF) | 0x6);

        let window = |loc: virtio::CapLoc| -> Option<u64> {
            let Some(bar) = bar_base(d, loc.bar) else {
                warn!("vblk", "BAR {} is not a usable memory BAR", loc.bar);
                return None;
            };
            let base = bar.checked_add(u64::from(loc.offset))?;
            let last = base.checked_add(u64::from(loc.length.max(1)) - 1)?;
            let ok = paging::map_mmio_uc(base) && paging::map_mmio_uc(last);
            if !ok {
                warn!("vblk", "cannot map {:#x}..{:#x} (BAR {} at {:#x})", base, last, loc.bar, bar);
            }
            ok.then_some(base)
        };
        let (Some(common_a), Some(notify_a), Some(dev_a)) = (window(common), window(notify), window(devcfg)) else {
            return Err("cannot map the device registers");
        };
        if common.length < 56 || devcfg.length < 8 {
            return Err("register windows too small");
        }

        let wait_us = |us: u64| {
            let t0 = arch::rdtsc();
            let ticks = log::tsc_hz() / 1_000_000 * us;
            while arch::rdtsc().wrapping_sub(t0) < ticks {
                arch::pause();
            }
        };

        // Reset, then the handshake.
        wr8(common_a + DEVICE_STATUS, 0);
        let mut spins = 0;
        while rd8(common_a + DEVICE_STATUS) != 0 {
            spins += 1;
            if spins > 1000 {
                return Err("device did not reset");
            }
            wait_us(1000);
        }
        wr8(common_a + DEVICE_STATUS, STATUS_ACKNOWLEDGE);
        wr8(common_a + DEVICE_STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER);
        wr32(common_a + DEVICE_FEATURE_SELECT, 1);
        if rd32(common_a + DEVICE_FEATURE) & FEATURE_VERSION_1 == 0 {
            wr8(common_a + DEVICE_STATUS, STATUS_FAILED);
            return Err("device does not offer virtio 1.0");
        }
        wr32(common_a + DRIVER_FEATURE_SELECT, 0);
        wr32(common_a + DRIVER_FEATURE, 0);
        wr32(common_a + DRIVER_FEATURE_SELECT, 1);
        wr32(common_a + DRIVER_FEATURE, FEATURE_VERSION_1);
        wr8(common_a + DEVICE_STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK);
        if rd8(common_a + DEVICE_STATUS) & STATUS_FEATURES_OK == 0 {
            wr8(common_a + DEVICE_STATUS, STATUS_FAILED);
            return Err("device rejected the features");
        }

        wr16(common_a + QUEUE_SELECT, 0);
        let Some(qsize) = virtio::queue_size(rd16(common_a + QUEUE_SIZE)) else {
            wr8(common_a + DEVICE_STATUS, STATUS_FAILED);
            return Err("queue 0 is not available");
        };
        let layout = virtio::ring_layout(qsize);
        let Some(mem) = mm::alloc_frames(3) else {
            wr8(common_a + DEVICE_STATUS, STATUS_FAILED);
            return Err("out of memory");
        };
        // SAFETY: three freshly allocated, identity-mapped, writable frames that nothing else uses.
        unsafe { core::ptr::write_bytes(mem as *mut u8, 0, 3 * 4096) };
        let (ring, req, data) = (mem, mem + 4096, mem + 2 * 4096);

        wr16(common_a + QUEUE_SIZE, qsize);
        wr64(common_a + QUEUE_DESC, ring);
        wr64(common_a + QUEUE_DRIVER, ring + layout.avail as u64);
        wr64(common_a + QUEUE_DEVICE, ring + layout.used as u64);
        wr16(common_a + QUEUE_ENABLE, 1);
        let notify_off = u64::from(rd16(common_a + QUEUE_NOTIFY_OFF));
        let notify_addr = notify_a + notify_off * u64::from(caps.notify_mult);
        if notify_addr + 2 > window_end(notify, notify_a) {
            wr8(common_a + DEVICE_STATUS, STATUS_FAILED);
            return Err("notify address outside its window");
        }
        wr8(common_a + DEVICE_STATUS, STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK | STATUS_DRIVER_OK);

        let sectors = u64::from(rd32(dev_a)) | u64::from(rd32(dev_a + 4)) << 32;
        Ok(VirtioBlk { common: common_a, notify: notify_addr, sectors, qsize, ring, layout, req, data, avail_idx: 0, last_used: 0 })
    }

    /// Reads up to `MAX_SECTORS` sectors (the buffer is a multiple of 512 and already checked).
    fn read_chunk(&mut self, lba: u64, out: &mut [u8]) -> Result<(), BlockError> {
        let n = out.len();
        let (hdr, status) = (self.req, self.req + 16);
        // SAFETY: `req` and `data` are this driver's frames; `ring` holds the descriptor table and rings
        // laid out by `ring_layout`. Everything written lies inside those frames.
        unsafe {
            write_volatile(hdr as *mut u32, BLK_T_IN);
            write_volatile((hdr + 4) as *mut u32, 0);
            write_volatile((hdr + 8) as *mut u64, lba);
            write_volatile(status as *mut u8, 0xFF);
            let desc = self.ring as *mut u8;
            let d = |i: usize, addr: u64, len: u32, flags: u16, next: u16| {
                let p = desc.add(16 * i);
                write_volatile(p.cast::<u64>(), addr);
                write_volatile(p.add(8).cast::<u32>(), len);
                write_volatile(p.add(12).cast::<u16>(), flags);
                write_volatile(p.add(14).cast::<u16>(), next);
            };
            d(0, hdr, 16, DESC_NEXT, 1);
            d(1, self.data, n as u32, DESC_NEXT | DESC_WRITE, 2);
            d(2, status, 1, DESC_WRITE, 0);
            let avail = (self.ring + self.layout.avail as u64) as *mut u16;
            write_volatile(avail.add(2 + usize::from(self.avail_idx % self.qsize)), 0); // ring[i] = head descriptor 0
            fence(Ordering::SeqCst);
            self.avail_idx = self.avail_idx.wrapping_add(1);
            write_volatile(avail.add(1), self.avail_idx);
            fence(Ordering::SeqCst);
        }
        wr16(self.notify, 0);

        let used_idx = (self.ring + self.layout.used as u64 + 2) as *const u16;
        let t0 = arch::rdtsc();
        let limit = log::tsc_hz().saturating_mul(TIMEOUT_SECONDS);
        // SAFETY: `used_idx` is inside the used ring of this driver's queue memory.
        while unsafe { read_volatile(used_idx) } == self.last_used {
            if arch::rdtsc().wrapping_sub(t0) > limit {
                return Err(BlockError::Timeout);
            }
            arch::pause();
        }
        fence(Ordering::SeqCst);
        self.last_used = self.last_used.wrapping_add(1);
        if rd8(status) != 0 {
            return Err(BlockError::Io);
        }
        // SAFETY: the device wrote `n` bytes into `data`, a frame of 4096 bytes (`n <= 8 * 512`).
        unsafe { core::ptr::copy_nonoverlapping(self.data as *const u8, out.as_mut_ptr(), n) };
        Ok(())
    }

    /// Resets the device so it stops using the queue memory. Called if a request ever times out.
    pub fn reset(&mut self) {
        wr8(self.common + DEVICE_STATUS, 0);
        self.sectors = 0; // every later request is refused at once instead of timing out again
    }
}

fn window_end(loc: virtio::CapLoc, base: u64) -> u64 {
    base.saturating_add(u64::from(loc.length))
}

impl BlockDev for VirtioBlk {
    fn sectors(&self) -> u64 {
        self.sectors
    }

    fn read(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        check_request(self.sectors, lba, buf.len())?;
        for (i, chunk) in buf.chunks_mut(MAX_SECTORS * SECTOR_SIZE).enumerate() {
            let at = lba + (i * MAX_SECTORS) as u64;
            if let Err(e) = self.read_chunk(at, chunk) {
                if e == BlockError::Timeout {
                    // A device that stops answering may still own the queue memory: take it away.
                    self.reset();
                }
                return Err(e);
            }
        }
        Ok(())
    }
}
