//! One door to "the disk" for the shell: the virtio disk if there is one, then SATA (AHCI),
//! then USB (xHCI). Callers get a `&mut dyn BlockDev` and do not care which kind it is. Hardware only.

use crate::block::BlockDev;
use crate::{ahci, vblk};

/// Runs `f` on the first available disk (virtio first, then AHCI, then USB), or returns `None` when there is none.
/// Do not call it again from inside `f`, and never from an interrupt handler.
pub fn with_disk<R>(f: impl FnOnce(&mut dyn BlockDev) -> R) -> Option<R> {
    // `f` can only run once, so it waits in an Option while the virtio attempt decides.
    let mut f = Some(f);
    let r = vblk::with_disk(|d| (f.take().expect("closure is taken once"))(d));
    if r.is_some() {
        return r;
    }
    let r = ahci::with_disk(|d| (f.take().expect("virtio did not run it"))(d));
    if r.is_some() {
        return r;
    }
    crate::xhci::with_disk(|d| (f.take().expect("ahci did not run it"))(d))
}

/// What `disks` prints: one line per disk Aeon can use, first one is the one the other commands read.
pub fn describe(mut line: impl FnMut(core::fmt::Arguments)) -> usize {
    let mut n = 0;
    if let Some(sectors) = vblk::with_disk(|d| d.sectors()) {
        line(format_args!("vda  virtio-blk  {} sectors  {} KiB", sectors, sectors / 2));
        n += 1;
    }
    if let Some(i) = ahci::disk_info() {
        line(format_args!("sda  sata port {}  {}  {} sectors  {} MiB", i.port, i.model_str(), i.sectors, i.sectors / 2048));
        n += 1;
    }
    if let Some(i) = crate::xhci::disk_info() {
        line(format_args!("uda  usb slot {}  {} {}  {} sectors  {} MiB", i.slot, i.vendor_str(), i.product_str(), i.sectors, i.sectors / 2048));
        n += 1;
    }
    n
}
