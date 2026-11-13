//! PCI bus scan. The decoding is pure and host-tested; reading the configuration space and keeping the
//! device list is UEFI-only (legacy port mechanism 1, which every x86 PC and QEMU provide).

/// What the first four configuration words say about a function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub vendor: u16,
    pub device: u16,
    pub class: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub revision: u8,
    /// Header type without the multifunction bit: 0 = device, 1 = PCI-to-PCI bridge.
    pub header_type: u8,
    pub multifunction: bool,
}

/// Decodes configuration words 0..4. Returns `None` when no function answers (vendor 0xFFFF).
pub fn parse_header(w: &[u32; 4]) -> Option<Header> {
    let vendor = w[0] as u16;
    if vendor == 0xFFFF {
        return None;
    }
    let ht = (w[3] >> 16) as u8;
    Some(Header {
        vendor,
        device: (w[0] >> 16) as u16,
        class: (w[2] >> 24) as u8,
        subclass: (w[2] >> 16) as u8,
        prog_if: (w[2] >> 8) as u8,
        revision: w[2] as u8,
        header_type: ht & 0x7F,
        multifunction: ht & 0x80 != 0,
    })
}

/// A short name for a class and subclass pair.
pub fn class_name(class: u8, subclass: u8) -> &'static str {
    match (class, subclass) {
        (0x00, _) => "Unclassified device",
        (0x01, 0x01) => "IDE controller",
        (0x01, 0x06) => "SATA controller",
        (0x01, 0x08) => "NVMe controller",
        (0x01, _) => "Storage controller",
        (0x02, 0x00) => "Ethernet controller",
        (0x02, _) => "Network controller",
        (0x03, 0x00) => "VGA controller",
        (0x03, _) => "Display controller",
        (0x04, _) => "Multimedia controller",
        (0x06, 0x00) => "Host bridge",
        (0x06, 0x01) => "ISA bridge",
        (0x06, 0x04) => "PCI bridge",
        (0x06, _) => "Bridge",
        (0x07, _) => "Communication controller",
        (0x08, _) => "System peripheral",
        (0x09, _) => "Input device",
        (0x0C, 0x03) => "USB controller",
        (0x0C, 0x05) => "SMBus controller",
        (0x0C, _) => "Serial bus controller",
        _ => "Unknown device",
    }
}

/// Where a function lives on the bus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Device {
    pub bus: u8,
    pub dev: u8,
    pub func: u8,
    pub header: Header,
}

/// Most functions the list keeps. A scan that finds more stops adding (and says so).
pub const MAX_DEVICES: usize = 64;

#[cfg(target_os = "uefi")]
mod hw {
    use super::{class_name, parse_header, Device, MAX_DEVICES};
    use crate::arch::{self, inl, outl};
    use crate::spin::SpinLock;
    use crate::{info, warn};

    const CONFIG_ADDRESS: u16 = 0xCF8;
    const CONFIG_DATA: u16 = 0xCFC;

    /// Reads a 32-bit configuration register (`off` is rounded down to a multiple of 4).
    pub fn config_read(bus: u8, dev: u8, func: u8, off: u8) -> u32 {
        let addr = 1u32 << 31 | u32::from(bus) << 16 | u32::from(dev & 31) << 11 | u32::from(func & 7) << 8 | u32::from(off & 0xFC);
        arch::without_interrupts(|| {
            // SAFETY: 0xCF8/0xCFC are the PCI configuration mechanism #1 ports; reading a register has no side effect.
            unsafe {
                outl(CONFIG_ADDRESS, addr);
                inl(CONFIG_DATA)
            }
        })
    }

    /// Writes a 32-bit configuration register (`off` is rounded down to a multiple of 4).
    pub fn config_write(bus: u8, dev: u8, func: u8, off: u8, val: u32) {
        let addr = 1u32 << 31 | u32::from(bus) << 16 | u32::from(dev & 31) << 11 | u32::from(func & 7) << 8 | u32::from(off & 0xFC);
        arch::without_interrupts(|| {
            // SAFETY: 0xCF8/0xCFC are the PCI configuration mechanism #1 ports. Callers write only
            // registers they own (the command register of the device they drive).
            unsafe {
                outl(CONFIG_ADDRESS, addr);
                outl(CONFIG_DATA, val);
            }
        })
    }

    struct List {
        items: [Option<Device>; MAX_DEVICES],
        n: usize,
    }

    static DEVICES: SpinLock<List> = SpinLock::new(List { items: [None; MAX_DEVICES], n: 0 });

    fn read_header(bus: u8, dev: u8, func: u8) -> Option<super::Header> {
        let w = [config_read(bus, dev, func, 0), config_read(bus, dev, func, 4), config_read(bus, dev, func, 8), config_read(bus, dev, func, 12)];
        parse_header(&w)
    }

    /// Scans bus 0 and every bus behind a bridge, and remembers what it finds.
    pub fn scan() {
        let mut visited = [false; 256];
        let mut stack = [0u8; 64];
        let mut top = 1;
        visited[0] = true;
        let mut list = DEVICES.lock();
        list.n = 0;
        while top > 0 {
            top -= 1;
            let bus = stack[top];
            for dev in 0..32u8 {
                let Some(first) = read_header(bus, dev, 0) else { continue };
                let funcs = if first.multifunction { 8 } else { 1 };
                for func in 0..funcs {
                    let Some(h) = (if func == 0 { Some(first) } else { read_header(bus, dev, func) }) else { continue };
                    if list.n < MAX_DEVICES {
                        let n = list.n;
                        list.items[n] = Some(Device { bus, dev, func, header: h });
                        list.n += 1;
                    } else {
                        warn!("pci", "device list full ({} entries), ignoring the rest", MAX_DEVICES);
                    }
                    if h.header_type == 1 && h.class == 6 && h.subclass == 4 {
                        let secondary = (config_read(bus, dev, func, 0x18) >> 8) as u8;
                        if secondary != 0 && !visited[secondary as usize] && top < stack.len() {
                            visited[secondary as usize] = true;
                            stack[top] = secondary;
                            top += 1;
                        }
                    }
                }
            }
        }
        for d in list.items[..list.n].iter().flatten() {
            info!(
                "pci",
                "{:02x}:{:02x}.{} {:04x}:{:04x} {}",
                d.bus,
                d.dev,
                d.func,
                d.header.vendor,
                d.header.device,
                class_name(d.header.class, d.header.subclass)
            );
        }
    }

    /// Address of a memory BAR (32 or 64 bit). `None` for I/O BARs or an unusable one.
    pub fn memory_bar(d: &Device, bar: u8) -> Option<u64> {
        if bar > 5 {
            return None;
        }
        let off = 0x10 + 4 * bar;
        let lo = config_read(d.bus, d.dev, d.func, off);
        if lo & 1 != 0 {
            return None; // I/O space
        }
        let base = match (lo >> 1) & 3 {
            0 => u64::from(lo & !0xF),
            2 if bar < 5 => u64::from(lo & !0xF) | u64::from(config_read(d.bus, d.dev, d.func, off + 4)) << 32,
            _ => return None,
        };
        (base != 0).then_some(base)
    }

    /// Calls `f` for every function the scan found, in bus order.
    pub fn for_each(mut f: impl FnMut(&Device)) {
        let list = DEVICES.lock();
        for d in list.items[..list.n].iter().flatten() {
            f(d);
        }
    }

    /// The first function with this vendor and device id.
    pub fn find(vendor: u16, device: u16) -> Option<Device> {
        let list = DEVICES.lock();
        list.items[..list.n].iter().flatten().find(|d| d.header.vendor == vendor && d.header.device == device).copied()
    }
}

#[cfg(target_os = "uefi")]
pub use hw::{config_read, config_write, find, for_each, memory_bar, scan};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_slot_is_none() {
        assert_eq!(parse_header(&[0xFFFF_FFFF, 0, 0, 0]), None);
        assert_eq!(parse_header(&[0x1234_FFFF, 0, 0, 0]), None);
    }

    #[test]
    fn decodes_a_host_bridge() {
        let h = parse_header(&[0x29C0_8086, 0x0000_0000, 0x0600_0000, 0x0000_0000]).unwrap();
        assert_eq!((h.vendor, h.device), (0x8086, 0x29C0));
        assert_eq!((h.class, h.subclass, h.prog_if, h.revision), (6, 0, 0, 0));
        assert_eq!(class_name(h.class, h.subclass), "Host bridge");
        assert!(!h.multifunction);
    }

    #[test]
    fn header_type_masks_the_multifunction_bit() {
        let h = parse_header(&[0x0001_1AF4, 0, 0x0100_0000, 0x0081_0000]).unwrap();
        assert_eq!(h.header_type, 1);
        assert!(h.multifunction);
        let g = parse_header(&[0x0001_1AF4, 0, 0, 0x0000_0000]).unwrap();
        assert_eq!(g.header_type, 0);
        assert!(!g.multifunction);
    }

    #[test]
    fn class_names_known_and_unknown() {
        assert_eq!(class_name(1, 8), "NVMe controller");
        assert_eq!(class_name(1, 0x99), "Storage controller");
        assert_eq!(class_name(0x0C, 3), "USB controller");
        assert_eq!(class_name(3, 0), "VGA controller");
        assert_eq!(class_name(0xEE, 0), "Unknown device");
        for c in 0..=255u8 {
            for s in [0u8, 1, 4, 255] {
                assert!(!class_name(c, s).is_empty());
            }
        }
    }
}
