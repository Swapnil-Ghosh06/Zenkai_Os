//! virtio 1.0 over PCI: the parts that are pure arithmetic (finding the device's register windows in the
//! PCI capability list, sizing the virtqueue). Host-tested; the driver itself is `vblk`.

pub const VENDOR: u16 = 0x1AF4;
/// Transitional virtio-blk (also has the modern registers) and modern-only virtio-blk.
pub const DEVICE_BLK_TRANSITIONAL: u16 = 0x1001;
pub const DEVICE_BLK_MODERN: u16 = 0x1042;

/// A register window: which BAR, where in it, how long.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapLoc {
    pub bar: u8,
    pub offset: u32,
    pub length: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Caps {
    pub common: Option<CapLoc>,
    pub notify: Option<CapLoc>,
    /// Multiplier for the per-queue notify address (from the notify capability).
    pub notify_mult: u32,
    pub device: Option<CapLoc>,
}

const CAP_ID_VENDOR: u8 = 0x09;
const CFG_COMMON: u8 = 1;
const CFG_NOTIFY: u8 = 2;
const CFG_DEVICE: u8 = 4;
/// A configuration space holds at most 48 capabilities; stopping there ends any loop in a hostile list.
const MAX_CAPS: usize = 48;

/// Walks the PCI capability list and picks out the virtio register windows. `read(off)` returns the
/// 32-bit configuration register at byte offset `off`. Bounded and tolerant: a missing or looping list
/// gives fewer (or no) windows, never a hang.
pub fn find_caps(mut read: impl FnMut(u8) -> u32) -> Caps {
    let mut caps = Caps::default();
    if (read(0x04) >> 16) & 0x10 == 0 {
        return caps; // no capability list
    }
    let mut ptr = (read(0x34) as u8) & 0xFC;
    for _ in 0..MAX_CAPS {
        if ptr < 0x40 {
            break;
        }
        let w0 = read(ptr);
        let (id, next, len, cfg_type) = (w0 as u8, ((w0 >> 8) as u8) & 0xFC, (w0 >> 16) as u8, (w0 >> 24) as u8);
        // The structure must lie inside the 256-byte configuration space.
        if id == CAP_ID_VENDOR && len >= 16 && usize::from(ptr) + usize::from(len) <= 256 {
            let loc = CapLoc { bar: read(ptr + 4) as u8, offset: read(ptr + 8), length: read(ptr + 12) };
            match cfg_type {
                CFG_COMMON if caps.common.is_none() => caps.common = Some(loc),
                CFG_NOTIFY if caps.notify.is_none() && len >= 20 => {
                    caps.notify = Some(loc);
                    caps.notify_mult = read(ptr + 16);
                }
                CFG_DEVICE if caps.device.is_none() => caps.device = Some(loc),
                _ => {}
            }
        }
        ptr = next;
    }
    caps
}

/// Largest power of two that is at most `device_max` and at most 8 (a small queue is plenty for one
/// request at a time). `None` when the device offers no queue.
pub fn queue_size(device_max: u16) -> Option<u16> {
    let m = device_max.min(8);
    if m == 0 {
        return None;
    }
    Some(1 << (15 - m.leading_zeros()))
}

/// Byte offsets inside the queue memory for a queue of `q` entries: descriptors at 0, then the
/// available ring, then the used ring (4-byte aligned), and the total size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RingLayout {
    pub avail: usize,
    pub used: usize,
    pub total: usize,
}

pub fn ring_layout(q: u16) -> RingLayout {
    let q = usize::from(q);
    let avail = 16 * q;
    let used = (avail + 6 + 2 * q + 3) & !3;
    RingLayout { avail, used, total: used + 6 + 8 * q }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake configuration space of 64 dwords.
    fn space(f: impl FnOnce(&mut [u32; 64])) -> impl FnMut(u8) -> u32 {
        let mut s = [0u32; 64];
        f(&mut s);
        move |off| s[usize::from(off / 4) % 64]
    }

    fn put_cap(s: &mut [u32; 64], at: usize, next: u8, len: u8, cfg: u8, bar: u8, off: u32, length: u32, mult: u32) {
        s[at / 4] = 0x09 | u32::from(next) << 8 | u32::from(len) << 16 | u32::from(cfg) << 24;
        s[at / 4 + 1] = u32::from(bar);
        s[at / 4 + 2] = off;
        s[at / 4 + 3] = length;
        if at / 4 + 4 < 64 {
            s[at / 4 + 4] = mult;
        }
    }

    #[test]
    fn finds_the_three_windows() {
        let caps = find_caps(space(|s| {
            s[1] = 0x0010_0000; // status: capability list present
            s[0x34 / 4] = 0x40;
            put_cap(s, 0x40, 0x60, 16, CFG_COMMON, 4, 0x0, 0x38, 0);
            put_cap(s, 0x60, 0x80, 20, CFG_NOTIFY, 4, 0x3000, 0x1000, 4);
            put_cap(s, 0x80, 0x00, 16, CFG_DEVICE, 4, 0x2000, 0x100, 0);
        }));
        assert_eq!(caps.common, Some(CapLoc { bar: 4, offset: 0, length: 0x38 }));
        assert_eq!(caps.notify, Some(CapLoc { bar: 4, offset: 0x3000, length: 0x1000 }));
        assert_eq!(caps.notify_mult, 4);
        assert_eq!(caps.device, Some(CapLoc { bar: 4, offset: 0x2000, length: 0x100 }));
    }

    #[test]
    fn no_capability_list_gives_nothing() {
        assert_eq!(find_caps(space(|s| s[0x34 / 4] = 0x40)), Caps::default());
    }

    #[test]
    fn a_looping_list_terminates() {
        let caps = find_caps(space(|s| {
            s[1] = 0x0010_0000;
            s[0x34 / 4] = 0x40;
            put_cap(s, 0x40, 0x60, 16, 9, 0, 0, 0, 0); // unknown type, points on
            put_cap(s, 0x60, 0x40, 16, 9, 0, 0, 0, 0); // points back
        }));
        assert_eq!(caps, Caps::default());
    }

    #[test]
    fn short_or_misplaced_capabilities_are_ignored() {
        // Too short for a virtio capability, then one that would run past the end of config space.
        let caps = find_caps(space(|s| {
            s[1] = 0x0010_0000;
            s[0x34 / 4] = 0x40;
            put_cap(s, 0x40, 0xF0, 8, CFG_COMMON, 0, 0, 0, 0);
            put_cap(s, 0xF0, 0, 32, CFG_COMMON, 0, 0, 0, 0);
        }));
        assert_eq!(caps.common, None);
        // A notify capability without the multiplier field is not usable.
        let caps = find_caps(space(|s| {
            s[1] = 0x0010_0000;
            s[0x34 / 4] = 0x40;
            put_cap(s, 0x40, 0, 16, CFG_NOTIFY, 0, 0, 0, 7);
        }));
        assert_eq!(caps.notify, None);
    }

    #[test]
    fn pointer_into_the_header_is_rejected() {
        assert_eq!(find_caps(space(|s| {
            s[1] = 0x0010_0000;
            s[0x34 / 4] = 0x10;
        })), Caps::default());
    }

    #[test]
    fn queue_sizes() {
        assert_eq!(queue_size(0), None);
        assert_eq!(queue_size(1), Some(1));
        assert_eq!(queue_size(3), Some(2));
        assert_eq!(queue_size(8), Some(8));
        assert_eq!(queue_size(256), Some(8));
        assert_eq!(queue_size(u16::MAX), Some(8));
    }

    #[test]
    fn ring_layout_is_aligned_and_fits_a_page() {
        let l = ring_layout(8);
        assert_eq!(l.avail, 128);
        assert_eq!(l.used, 152);
        assert_eq!(l.total, 222);
        for q in [1u16, 2, 4, 8] {
            let l = ring_layout(q);
            assert_eq!(l.used % 4, 0);
            assert!(l.used >= l.avail + 6 + 2 * usize::from(q));
            assert!(l.total <= 4096);
        }
    }
}
