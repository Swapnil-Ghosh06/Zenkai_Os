//! UEFI memory-map parsing and accounting. Pure code: it works on byte slices, so descriptor
//! alignment and the firmware-reported stride are handled without raw pointer casts.

pub const PAGE_SIZE: u64 = 4096;

/// Smallest valid `EFI_MEMORY_DESCRIPTOR` (type, pad, phys, virt, pages, attribute).
pub const MIN_DESC_SIZE: usize = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct RawDesc {
    pub mem_type: u32,
    pub phys_start: u64,
    pub pages: u64,
    pub attribute: u64,
}

fn le_u32(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn le_u64(b: &[u8]) -> u64 {
    u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

/// Returns descriptor `index` of a map whose entries are `desc_size` bytes apart. The stride may be
/// larger than [`MIN_DESC_SIZE`]; firmware is allowed to append fields.
pub fn read_desc(raw: &[u8], index: usize, desc_size: usize) -> Option<RawDesc> {
    if desc_size < MIN_DESC_SIZE {
        return None;
    }
    let start = index.checked_mul(desc_size)?;
    let end = start.checked_add(MIN_DESC_SIZE)?;
    let d = raw.get(start..end)?;
    Some(RawDesc {
        mem_type: le_u32(&d[0..4]),
        phys_start: le_u64(&d[8..16]),
        pages: le_u64(&d[24..32]),
        attribute: le_u64(&d[32..40]),
    })
}

/// How the kernel treats a region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    /// EfiConventionalMemory: free now.
    Usable,
    /// Loader and boot-services code/data: free once boot services have exited.
    Reclaimable,
    /// EfiACPIReclaimMemory: free once the ACPI tables have been parsed.
    AcpiReclaim,
    /// EfiACPIMemoryNVS.
    AcpiNvs,
    /// EfiRuntimeServicesCode/Data: must stay intact.
    Runtime,
    /// Memory-mapped I/O and port space.
    Mmio,
    /// Persistent memory (NVDIMM).
    Persistent,
    /// Unusable, reserved or unknown.
    Reserved,
}

pub fn classify(mem_type: u32) -> Kind {
    match mem_type {
        7 => Kind::Usable,
        1..=4 => Kind::Reclaimable,
        5 | 6 => Kind::Runtime,
        9 => Kind::AcpiReclaim,
        10 => Kind::AcpiNvs,
        11 | 12 => Kind::Mmio,
        14 => Kind::Persistent,
        _ => Kind::Reserved,
    }
}

impl Kind {
    /// True when the region is backed by ordinary RAM (as opposed to MMIO or holes).
    pub fn is_ram(self) -> bool {
        !matches!(self, Kind::Mmio | Kind::Reserved)
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Usable => "usable",
            Kind::Reclaimable => "reclaim",
            Kind::AcpiReclaim => "acpi-rec",
            Kind::AcpiNvs => "acpi-nvs",
            Kind::Runtime => "runtime",
            Kind::Mmio => "mmio",
            Kind::Persistent => "pmem",
            Kind::Reserved => "reserved",
        }
    }
}

/// Compact region stored in `BootInfo`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub start: u64,
    pub pages: u64,
    pub kind: Kind,
    pub mem_type: u32,
}

impl Region {
    pub const EMPTY: Region = Region { start: 0, pages: 0, kind: Kind::Reserved, mem_type: 0 };

    pub fn end(&self) -> u64 {
        self.start.saturating_add(self.pages.saturating_mul(PAGE_SIZE))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub regions: usize,
    pub usable_pages: u64,
    pub reclaimable_pages: u64,
    pub acpi_pages: u64,
    pub runtime_pages: u64,
    pub mmio_pages: u64,
    pub reserved_pages: u64,
    /// All RAM-backed pages: usable + reclaimable + ACPI + runtime + persistent.
    pub ram_pages: u64,
    pub largest_usable_pages: u64,
    /// Highest end address of any RAM region. MMIO does not count.
    pub top_of_ram: u64,
}

pub fn pages_to_mb(pages: u64) -> u64 {
    pages.saturating_mul(PAGE_SIZE) / (1024 * 1024)
}

/// Totals over `regions` (saturating: a hostile page count must not overflow and panic). Adjacent usable regions are merged before looking for the largest block.
pub fn summarize(regions: &[Region]) -> Summary {
    let mut s = Summary { regions: regions.len(), ..Summary::default() };
    let mut run_end = 0u64;
    let mut run_pages = 0u64;
    for r in regions {
        match r.kind {
            Kind::Usable => s.usable_pages = s.usable_pages.saturating_add(r.pages),
            Kind::Reclaimable => s.reclaimable_pages = s.reclaimable_pages.saturating_add(r.pages),
            Kind::AcpiReclaim | Kind::AcpiNvs => s.acpi_pages = s.acpi_pages.saturating_add(r.pages),
            Kind::Runtime => s.runtime_pages = s.runtime_pages.saturating_add(r.pages),
            Kind::Mmio => s.mmio_pages = s.mmio_pages.saturating_add(r.pages),
            Kind::Persistent => {}
            Kind::Reserved => s.reserved_pages = s.reserved_pages.saturating_add(r.pages),
        }
        if r.kind.is_ram() {
            s.ram_pages = s.ram_pages.saturating_add(r.pages);
            s.top_of_ram = s.top_of_ram.max(r.end());
        }
        if r.kind == Kind::Usable {
            if run_pages != 0 && r.start == run_end {
                run_pages = run_pages.saturating_add(r.pages);
            } else {
                run_pages = r.pages;
            }
            run_end = r.end();
            s.largest_usable_pages = s.largest_usable_pages.max(run_pages);
        } else {
            run_pages = 0;
        }
    }
    s
}

/// Parses `raw` into `out`, merging a descriptor into its predecessor when both are contiguous and
/// of the same type. Returns `(stored, dropped)`: `dropped` counts descriptors that did not fit.
pub fn collect_regions(raw: &[u8], desc_size: usize, out: &mut [Region]) -> (usize, usize) {
    let mut n = 0usize;
    let mut dropped = 0usize;
    let mut i = 0usize;
    while let Some(d) = read_desc(raw, i, desc_size) {
        i += 1;
        if d.pages == 0 {
            continue;
        }
        let kind = classify(d.mem_type);
        if n > 0 {
            let last = &mut out[n - 1];
            if last.mem_type == d.mem_type && last.end() == d.phys_start {
                last.pages = last.pages.saturating_add(d.pages);
                continue;
            }
        }
        if n < out.len() {
            out[n] = Region { start: d.phys_start, pages: d.pages, kind, mem_type: d.mem_type };
            n += 1;
        } else {
            dropped += 1;
        }
    }
    (n, dropped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_desc(buf: &mut std::vec::Vec<u8>, stride: usize, ty: u32, start: u64, pages: u64) {
        let mut d = std::vec![0xEEu8; stride]; // firmware padding must be ignored
        d[0..4].copy_from_slice(&ty.to_le_bytes());
        d[4..8].copy_from_slice(&0u32.to_le_bytes());
        d[8..16].copy_from_slice(&start.to_le_bytes());
        d[16..24].copy_from_slice(&0u64.to_le_bytes());
        d[24..32].copy_from_slice(&pages.to_le_bytes());
        d[32..40].copy_from_slice(&0xFu64.to_le_bytes());
        buf.extend_from_slice(&d);
    }

    #[test]
    fn oversized_stride_is_honoured() {
        for stride in [40usize, 48, 64] {
            let mut raw = std::vec::Vec::new();
            push_desc(&mut raw, stride, 7, 0x10_0000, 256);
            push_desc(&mut raw, stride, 11, 0xFEE0_0000, 1);
            let a = read_desc(&raw, 0, stride).unwrap();
            let b = read_desc(&raw, 1, stride).unwrap();
            assert_eq!((a.mem_type, a.phys_start, a.pages), (7, 0x10_0000, 256));
            assert_eq!((b.mem_type, b.phys_start, b.pages), (11, 0xFEE0_0000, 1));
            assert!(read_desc(&raw, 2, stride).is_none());
        }
    }

    #[test]
    fn stride_below_minimum_is_rejected() {
        let raw = [0u8; 128];
        assert!(read_desc(&raw, 0, 32).is_none());
    }

    #[test]
    fn truncated_map_does_not_panic() {
        let raw = [0u8; 50];
        assert!(read_desc(&raw, 0, 48).is_some());
        assert!(read_desc(&raw, 1, 48).is_none());
    }

    #[test]
    fn mmio_and_reserved_do_not_count_as_ram() {
        let regions = [
            Region { start: 0, pages: 256, kind: Kind::Usable, mem_type: 7 },
            Region { start: 0x10_0000, pages: 16, kind: Kind::Reclaimable, mem_type: 3 },
            Region { start: 0xFEE0_0000, pages: 1, kind: Kind::Mmio, mem_type: 11 },
            Region { start: 0x10_0000_0000, pages: 1, kind: Kind::Mmio, mem_type: 11 },
        ];
        let s = summarize(&regions);
        assert_eq!(s.ram_pages, 272);
        assert_eq!(s.mmio_pages, 2);
        assert_eq!(s.top_of_ram, 0x10_0000 + 16 * 4096);
    }

    #[test]
    fn adjacent_usable_regions_form_one_block() {
        let regions = [
            Region { start: 0x1000, pages: 4, kind: Kind::Usable, mem_type: 7 },
            Region { start: 0x5000, pages: 6, kind: Kind::Usable, mem_type: 7 },
            Region { start: 0x20000, pages: 3, kind: Kind::Reserved, mem_type: 0 },
            Region { start: 0x30000, pages: 8, kind: Kind::Usable, mem_type: 7 },
        ];
        assert_eq!(summarize(&regions).largest_usable_pages, 10);
    }

    #[test]
    fn collect_merges_contiguous_same_type() {
        let mut raw = std::vec::Vec::new();
        push_desc(&mut raw, 48, 7, 0x0, 16);
        push_desc(&mut raw, 48, 7, 0x10000, 16);
        push_desc(&mut raw, 48, 4, 0x20000, 1);
        push_desc(&mut raw, 48, 7, 0x30000, 0); // zero-length: skipped
        let mut out = [Region::EMPTY; 8];
        let (n, dropped) = collect_regions(&raw, 48, &mut out);
        assert_eq!((n, dropped), (2, 0));
        assert_eq!(out[0].pages, 32);
        assert_eq!(out[1].kind, Kind::Reclaimable);
    }

    #[test]
    fn collect_reports_overflow() {
        let mut raw = std::vec::Vec::new();
        for i in 0..5u64 {
            push_desc(&mut raw, 40, if i % 2 == 0 { 7 } else { 0 }, i * 0x100000, 1);
        }
        let mut out = [Region::EMPTY; 3];
        assert_eq!(collect_regions(&raw, 40, &mut out), (3, 2));
    }
}

/// A hostile memory map must never panic or overrun the output table.
#[cfg(test)]
mod fuzz {
    use super::*;

    fn xorshift(s: &mut u64) -> u64 {
        *s ^= *s << 13;
        *s ^= *s >> 7;
        *s ^= *s << 17;
        *s
    }

    #[test]
    fn random_maps_never_panic_or_overflow() {
        let mut s = 0x1234_5678_9ABC_DEF1u64;
        let mut out = [Region::EMPTY; 16];
        for _ in 0..20_000 {
            let len = (xorshift(&mut s) % 600) as usize;
            let raw: Vec<u8> = (0..len).map(|_| xorshift(&mut s) as u8).collect();
            let desc = [0usize, 1, 16, 40, 48, 64, 4096, usize::MAX][(xorshift(&mut s) % 8) as usize];
            let (n, _dropped) = collect_regions(&raw, desc, &mut out);
            assert!(n <= out.len());
            let sm = summarize(&out[..n]);
            assert!(sm.regions == n);
        }
    }
}
