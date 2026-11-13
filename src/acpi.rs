//! ACPI RSDP validation on byte slices (pure, host-testable).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rsdp {
    /// Signature or checksum is wrong; do not trust the pointer.
    Invalid,
    /// ACPI 1.0 (revision 0): only the RSDT address is meaningful.
    V1 { rsdt: u32 },
    /// ACPI 2.0+: the extended checksum covers the length stated in the table.
    V2 { xsdt: u64, revision: u8 },
}

pub const V1_LEN: usize = 20;
pub const V2_MIN_LEN: usize = 36;
/// Upper bound on the `length` field we accept before summing, so a corrupt table cannot make us
/// read an arbitrary amount of memory.
pub const V2_MAX_LEN: usize = 4096;

fn checksum(b: &[u8]) -> u8 {
    b.iter().fold(0u8, |a, &x| a.wrapping_add(x))
}

/// Header fields needed to decide how many bytes may be read: `(revision, length)`.
/// `bytes` must hold at least the first 24 bytes of the RSDP.
pub fn peek_header(bytes: &[u8]) -> Option<(u8, usize)> {
    if bytes.len() < 24 || &bytes[0..8] != b"RSD PTR " {
        return None;
    }
    let revision = bytes[15];
    let len = u32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]) as usize;
    Some((revision, len))
}

/// Validates the RSDP in `bytes` (which must cover the whole table).
pub fn validate(bytes: &[u8]) -> Rsdp {
    let Some((revision, len)) = peek_header(bytes) else {
        return Rsdp::Invalid;
    };
    if bytes.len() < V1_LEN || checksum(&bytes[..V1_LEN]) != 0 {
        return Rsdp::Invalid;
    }
    if revision == 0 {
        let rsdt = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        return Rsdp::V1 { rsdt };
    }
    if !(V2_MIN_LEN..=V2_MAX_LEN).contains(&len) || bytes.len() < len || checksum(&bytes[..len]) != 0 {
        return Rsdp::Invalid;
    }
    let mut x = [0u8; 8];
    x.copy_from_slice(&bytes[24..32]);
    Rsdp::V2 { xsdt: u64::from_le_bytes(x), revision }
}

/// ACPI system description table header size.
pub const SDT_HEADER: usize = 36;
/// Largest table we will read (a DSDT can be a few hundred KiB).
pub const SDT_MAX_LEN: usize = 1 << 20;

/// Table length from an SDT header, if it is plausible.
pub fn sdt_len(header: &[u8]) -> Option<usize> {
    let len = u32::from_le_bytes(header.get(4..8)?.try_into().ok()?) as usize;
    (SDT_HEADER..=SDT_MAX_LEN).contains(&len).then_some(len)
}

pub fn sdt_checksum_ok(table: &[u8]) -> bool {
    checksum(table) == 0
}

/// Physical addresses of the tables listed by an RSDT (4-byte entries) or XSDT (8-byte entries).
pub fn sdt_entries(table: &[u8], xsdt: bool) -> impl Iterator<Item = u64> + '_ {
    let w = if xsdt { 8 } else { 4 };
    table.get(SDT_HEADER..).unwrap_or(&[]).chunks_exact(w).map(move |c| {
        let mut b = [0u8; 8];
        b[..w].copy_from_slice(c);
        u64::from_le_bytes(b)
    })
}

/// What the FADT tells us about powering off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fadt {
    pub dsdt: u64,
    pub pm1a_cnt: u16,
    pub pm1b_cnt: u16,
}

/// Parses a FADT ("FACP"). Port numbers above 0xFFFF (memory-mapped registers) are not supported
/// and yield `None`.
pub fn parse_fadt(t: &[u8]) -> Option<Fadt> {
    if t.get(0..4)? != b"FACP" || t.len() < 72 {
        return None;
    }
    let rd32 = |o: usize| -> Option<u32> { Some(u32::from_le_bytes(t.get(o..o + 4)?.try_into().ok()?)) };
    let mut dsdt = rd32(40)? as u64;
    if t.len() >= 148 {
        let x = u64::from_le_bytes(t.get(140..148)?.try_into().ok()?);
        if x != 0 {
            dsdt = x;
        }
    }
    let pm1a = rd32(64)?;
    let pm1b = rd32(68)?;
    if pm1a == 0 || pm1a > 0xFFFF || pm1b > 0xFFFF {
        return None;
    }
    Some(Fadt { dsdt, pm1a_cnt: pm1a as u16, pm1b_cnt: pm1b as u16 })
}

/// Finds the `_S5_` sleep-type values `(SLP_TYPa, SLP_TYPb)` in a DSDT by locating the
/// `Name(_S5_, Package(){...})` object, the same shortcut other small kernels use instead of a
/// full AML interpreter.
pub fn find_s5(dsdt: &[u8]) -> Option<(u8, u8)> {
    let pos = dsdt.windows(4).position(|w| w == b"_S5_")?;
    // Name(_S5_ ...) is `08 5F 53 35 5F 12 <PkgLength> <NumElements> <elements>`; the name may also
    // be prefixed with `\` (0x5C).
    if pos == 0 || !matches!(dsdt[pos - 1], 0x08 | 0x5C) {
        return None;
    }
    let mut i = pos + 4;
    if *dsdt.get(i)? != 0x12 {
        return None;
    }
    i += 1;
    let extra = (*dsdt.get(i)? >> 6) as usize; // PkgLength: top two bits count the extra bytes
    i += 1 + extra;
    i += 1; // NumElements
    let mut vals = [0u8; 2];
    for v in vals.iter_mut() {
        if *dsdt.get(i)? == 0x0A {
            i += 1; // BytePrefix
        }
        *v = *dsdt.get(i)?;
        i += 1;
    }
    Some((vals[0], vals[1]))
}

/// What the MADT tells us about the interrupt hardware.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Madt {
    /// Physical address of the local APIC registers (same for every CPU).
    pub lapic_base: u32,
    /// Usable CPUs: entries marked enabled or online-capable.
    pub cpus: u8,
    /// Address of the first I/O APIC, if any.
    pub ioapic_base: Option<u32>,
    /// First global system interrupt (GSI) number that the first I/O APIC handles.
    pub ioapic_gsi_base: u32,
    /// Interrupt source overrides for the 16 legacy ISA IRQs (index = ISA IRQ).
    overrides: [Option<IsaRoute>; 16],
}

/// Where a legacy ISA interrupt arrives on the I/O APIC, and how its line is wired.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IsaRoute {
    /// Global system interrupt: the I/O APIC input to program.
    pub gsi: u32,
    pub active_low: bool,
    pub level: bool,
}

impl Madt {
    /// The route for ISA IRQ `irq`: the firmware's override when the MADT has one, otherwise the same
    /// number as the IRQ, active high and edge triggered (how the ISA bus is wired).
    pub fn isa_route(&self, irq: u8) -> IsaRoute {
        self.overrides.get(usize::from(irq)).copied().flatten().unwrap_or(IsaRoute { gsi: u32::from(irq), active_low: false, level: false })
    }
}

/// MPS INTI flags of an interrupt source override: bits 0-1 polarity, bits 2-3 trigger mode. The value
/// 0 means "follow the bus" (ISA: active high, edge); the reserved value 2 is treated the same way.
fn inti_flags(flags: u16) -> (bool, bool) {
    (flags & 0b11 == 0b11, (flags >> 2) & 0b11 == 0b11)
}

/// Parses a MADT ("APIC"). Returns `None` for a wrong signature, no usable CPU, or any entry that
/// is truncated or has a length below 2 (which would otherwise loop forever).
pub fn parse_madt(t: &[u8]) -> Option<Madt> {
    if t.get(0..4)? != b"APIC" || t.len() < 44 {
        return None;
    }
    let mut lapic_base = u32::from_le_bytes(t.get(36..40)?.try_into().ok()?);
    let (mut cpus, mut ioapic_base, mut ioapic_gsi_base) = (0u8, None, 0u32);
    let mut overrides = [None; 16];
    let mut i = 44;
    while i < t.len() {
        let (kind, len) = (*t.get(i)?, *t.get(i + 1)? as usize);
        let e = t.get(i..i.checked_add(len)?)?;
        if len < 2 {
            return None;
        }
        match kind {
            0 if len >= 8 => {
                let flags = u32::from_le_bytes(e[4..8].try_into().ok()?);
                if flags & 0b11 != 0 {
                    cpus = cpus.saturating_add(1);
                }
            }
            1 if len >= 12 && ioapic_base.is_none() => {
                ioapic_base = Some(u32::from_le_bytes(e[4..8].try_into().ok()?));
                ioapic_gsi_base = u32::from_le_bytes(e[8..12].try_into().ok()?);
            }
            // Interrupt source override: only the ISA bus (0) and the 16 legacy IRQs matter here.
            2 if len >= 10 && e[2] == 0 && e[3] < 16 => {
                let (active_low, level) = inti_flags(u16::from_le_bytes(e[8..10].try_into().ok()?));
                overrides[usize::from(e[3])] = Some(IsaRoute { gsi: u32::from_le_bytes(e[4..8].try_into().ok()?), active_low, level });
            }
            5 if len >= 12 => {
                if let Ok(a) = u32::try_from(u64::from_le_bytes(e[4..12].try_into().ok()?)) {
                    lapic_base = a;
                }
            }
            _ => {}
        }
        i += len;
    }
    (cpus > 0).then_some(Madt { lapic_base, cpus, ioapic_base, ioapic_gsi_base, overrides })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fix(buf: &mut [u8], at: usize, upto: usize) {
        buf[at] = 0;
        let s = checksum(&buf[..upto]);
        buf[at] = 0u8.wrapping_sub(s);
    }

    fn v2(len: u32) -> std::vec::Vec<u8> {
        let mut b = std::vec![0u8; len as usize];
        b[0..8].copy_from_slice(b"RSD PTR ");
        b[15] = 2;
        b[20..24].copy_from_slice(&len.to_le_bytes());
        b[24..32].copy_from_slice(&0x3ED7_D0E8u64.to_le_bytes());
        fix(&mut b, 8, 20);
        fix(&mut b, 32, len as usize);
        b
    }


    /// A MADT with the given entries after the 44-byte fixed part.
    fn madt(lapic: u32, entries: &[&[u8]]) -> std::vec::Vec<u8> {
        let mut t = std::vec![0u8; 44];
        t[0..4].copy_from_slice(b"APIC");
        t[36..40].copy_from_slice(&lapic.to_le_bytes());
        for e in entries {
            t.extend_from_slice(e);
        }
        let len = t.len() as u32;
        t[4..8].copy_from_slice(&len.to_le_bytes());
        t
    }
    fn cpu(id: u8, flags: u32) -> [u8; 8] {
        let mut e = [0u8; 8];
        e[0] = 0;
        e[1] = 8;
        e[3] = id;
        e[4..8].copy_from_slice(&flags.to_le_bytes());
        e
    }
    fn ioapic(addr: u32) -> [u8; 12] {
        let mut e = [0u8; 12];
        e[0] = 1;
        e[1] = 12;
        e[4..8].copy_from_slice(&addr.to_le_bytes());
        e
    }

    #[test]
    fn madt_two_cpus_one_ioapic() {
        let t = madt(0xFEE0_0000, &[&cpu(0, 1), &cpu(1, 1), &ioapic(0xFEC0_0000)]);
        let m = parse_madt(&t).unwrap();
        assert_eq!((m.lapic_base, m.cpus, m.ioapic_base), (0xFEE0_0000, 2, Some(0xFEC0_0000)));
    }

    fn ioapic_at(addr: u32, gsi_base: u32) -> [u8; 12] {
        let mut e = ioapic(addr);
        e[8..12].copy_from_slice(&gsi_base.to_le_bytes());
        e
    }

    /// An interrupt source override: ISA bus, `source` IRQ delivered as `gsi`, with MPS INTI `flags`.
    fn iso(bus: u8, source: u8, gsi: u32, flags: u16) -> [u8; 10] {
        let mut e = [0u8; 10];
        e[0] = 2;
        e[1] = 10;
        e[2] = bus;
        e[3] = source;
        e[4..8].copy_from_slice(&gsi.to_le_bytes());
        e[8..10].copy_from_slice(&flags.to_le_bytes());
        e
    }

    #[test]
    fn madt_ioapic_gsi_base_is_kept() {
        let t = madt(0xFEE0_0000, &[&cpu(0, 1), &ioapic_at(0xFEC0_0000, 24)]);
        assert_eq!(parse_madt(&t).unwrap().ioapic_gsi_base, 24);
    }

    #[test]
    fn isa_irqs_map_to_the_same_gsi_when_nothing_overrides_them() {
        let t = madt(0xFEE0_0000, &[&cpu(0, 1), &ioapic(0xFEC0_0000)]);
        let m = parse_madt(&t).unwrap();
        for irq in 0..16u8 {
            assert_eq!(m.isa_route(irq), IsaRoute { gsi: u32::from(irq), active_low: false, level: false });
        }
    }

    #[test]
    fn the_usual_timer_override_moves_irq0_to_gsi2() {
        let t = madt(0xFEE0_0000, &[&cpu(0, 1), &ioapic(0xFEC0_0000), &iso(0, 0, 2, 0)]);
        let m = parse_madt(&t).unwrap();
        assert_eq!(m.isa_route(0), IsaRoute { gsi: 2, active_low: false, level: false });
        assert_eq!(m.isa_route(1).gsi, 1, "the keyboard IRQ stays where it is");
    }

    #[test]
    fn override_flags_give_polarity_and_trigger() {
        // 0b1111 = active low, level triggered (the usual PCI-style wiring on an ISA IRQ).
        let t = madt(0xFEE0_0000, &[&cpu(0, 1), &iso(0, 9, 9, 0b1111), &iso(0, 5, 5, 0b0101), &iso(0, 6, 6, 0b1010)]);
        let m = parse_madt(&t).unwrap();
        assert_eq!(m.isa_route(9), IsaRoute { gsi: 9, active_low: true, level: true });
        assert_eq!(m.isa_route(5), IsaRoute { gsi: 5, active_low: false, level: false }, "1 = active high, 1 = edge");
        assert_eq!(m.isa_route(6), IsaRoute { gsi: 6, active_low: false, level: false }, "reserved value 2 means: follow the bus");
    }

    #[test]
    fn bad_overrides_are_ignored_not_fatal() {
        let short = [2u8, 8, 0, 1, 7, 0, 0, 0]; // type 2 but only 8 bytes long
        let t = madt(0xFEE0_0000, &[&cpu(0, 1), &iso(1, 1, 99, 0), &iso(0, 16, 99, 0), &short]);
        let m = parse_madt(&t).unwrap();
        assert_eq!(m.isa_route(1).gsi, 1, "a non-ISA bus override is not for the ISA keyboard");
        assert_eq!(m.isa_route(15).gsi, 15);
        assert_eq!(m.isa_route(200), IsaRoute { gsi: 200, active_low: false, level: false }, "out of range: identity");
    }

    #[test]
    fn the_last_override_for_an_irq_wins() {
        let t = madt(0xFEE0_0000, &[&cpu(0, 1), &iso(0, 3, 10, 0), &iso(0, 3, 11, 0)]);
        assert_eq!(parse_madt(&t).unwrap().isa_route(3).gsi, 11);
    }

    #[test]
    fn madt_disabled_cpu_not_counted() {
        let t = madt(0xFEE0_0000, &[&cpu(0, 1), &cpu(1, 0)]);
        assert_eq!(parse_madt(&t).unwrap().cpus, 1);
        let online_capable = madt(0xFEE0_0000, &[&cpu(0, 2)]);
        assert_eq!(parse_madt(&online_capable).unwrap().cpus, 1);
    }

    #[test]
    fn madt_zero_length_entry_is_none() {
        let t = madt(0xFEE0_0000, &[&cpu(0, 1), &[0u8, 0u8]]);
        assert_eq!(parse_madt(&t), None);
    }

    #[test]
    fn madt_truncated_is_none() {
        let mut t = madt(0xFEE0_0000, &[&cpu(0, 1)]);
        t.truncate(t.len() - 3); // entry claims 8 bytes, only 5 remain
        assert_eq!(parse_madt(&t), None);
        assert_eq!(parse_madt(&t[..30]), None);
    }

    #[test]
    fn madt_wrong_signature_or_no_cpu_is_none() {
        let mut t = madt(0xFEE0_0000, &[&cpu(0, 1)]);
        t[0] = b'X';
        assert_eq!(parse_madt(&t), None);
        assert_eq!(parse_madt(&madt(0xFEE0_0000, &[])), None);
    }

    #[test]
    fn madt_lapic_override_is_used_when_it_fits() {
        let mut o = [0u8; 12];
        o[0] = 5;
        o[1] = 12;
        o[4..12].copy_from_slice(&0xFEE1_0000u64.to_le_bytes());
        let t = madt(0xFEE0_0000, &[&cpu(0, 1), &o]);
        assert_eq!(parse_madt(&t).unwrap().lapic_base, 0xFEE1_0000);
        o[4..12].copy_from_slice(&0x1_0000_0000u64.to_le_bytes());
        let t = madt(0xFEE0_0000, &[&cpu(0, 1), &o]);
        assert_eq!(parse_madt(&t).unwrap().lapic_base, 0xFEE0_0000, "an override above 4 GiB is ignored");
    }

    #[test]
    fn valid_v2_is_accepted() {
        assert_eq!(validate(&v2(36)), Rsdp::V2 { xsdt: 0x3ED7_D0E8, revision: 2 });
    }

    #[test]
    fn extended_checksum_covers_declared_length() {
        let mut b = v2(48);
        b[44] ^= 1; // beyond byte 36: the old 36-byte check would have missed this
        assert_eq!(validate(&b), Rsdp::Invalid);
    }

    #[test]
    fn bad_first_checksum_is_rejected() {
        let mut b = v2(36);
        b[9] ^= 0x10;
        assert_eq!(validate(&b), Rsdp::Invalid);
    }

    #[test]
    fn wrong_signature_is_rejected() {
        let mut b = v2(36);
        b[0] = b'X';
        assert_eq!(validate(&b), Rsdp::Invalid);
    }

    #[test]
    fn absurd_length_is_rejected_without_reading() {
        let mut b = v2(36);
        b[20..24].copy_from_slice(&0x00FF_FFFFu32.to_le_bytes());
        assert_eq!(validate(&b), Rsdp::Invalid);
    }

    #[test]
    fn v1_table() {
        let mut b = std::vec![0u8; 20];
        b[0..8].copy_from_slice(b"RSD PTR ");
        b[16..20].copy_from_slice(&0x1234_5000u32.to_le_bytes());
        // peek_header wants 24 bytes
        b.resize(24, 0);
        fix(&mut b, 8, 20);
        assert_eq!(validate(&b), Rsdp::V1 { rsdt: 0x1234_5000 });
    }

    #[test]
    fn fadt_fields() {
        let mut t = std::vec![0u8; 244];
        t[0..4].copy_from_slice(b"FACP");
        t[4..8].copy_from_slice(&244u32.to_le_bytes());
        t[40..44].copy_from_slice(&0x3EDF_0000u32.to_le_bytes());
        t[64..68].copy_from_slice(&0x4004u32.to_le_bytes());
        assert_eq!(parse_fadt(&t), Some(Fadt { dsdt: 0x3EDF_0000, pm1a_cnt: 0x4004, pm1b_cnt: 0 }));
        t[140..148].copy_from_slice(&0x1_0000_0000u64.to_le_bytes());
        assert_eq!(parse_fadt(&t).unwrap().dsdt, 0x1_0000_0000);
        t[64..68].copy_from_slice(&0x1_0000u32.to_le_bytes()); // memory-mapped: unsupported
        assert_eq!(parse_fadt(&t), None);
    }

    #[test]
    fn s5_object_is_found() {
        // ...Name(_S5_, Package(4){ 0x05, 0x05, 0, 0 })...
        let aml = [0x10, 0x20, 0x08, b'_', b'S', b'5', b'_', 0x12, 0x06, 0x04, 0x0A, 0x05, 0x0A, 0x05, 0x00, 0x00];
        assert_eq!(find_s5(&aml), Some((5, 5)));
        // Zero/One ops instead of BytePrefix constants.
        let aml2 = [0x08, b'_', b'S', b'5', b'_', 0x12, 0x04, 0x02, 0x00, 0x01];
        assert_eq!(find_s5(&aml2), Some((0, 1)));
        assert_eq!(find_s5(b"no sleep object here"), None);
        assert_eq!(find_s5(&[0x08, b'_', b'S', b'5', b'_', 0x12]), None);
    }

    #[test]
    fn sdt_length_is_bounded() {
        let mut h = [0u8; 36];
        h[4..8].copy_from_slice(&100u32.to_le_bytes());
        assert_eq!(sdt_len(&h), Some(100));
        h[4..8].copy_from_slice(&10u32.to_le_bytes());
        assert_eq!(sdt_len(&h), None);
        h[4..8].copy_from_slice(&0x4000_0000u32.to_le_bytes());
        assert_eq!(sdt_len(&h), None);
    }

    #[test]
    fn sdt_entry_widths() {
        let mut t = std::vec![0u8; 36];
        t.extend_from_slice(&0x1111u64.to_le_bytes());
        t.extend_from_slice(&0x2222u64.to_le_bytes());
        assert_eq!(sdt_entries(&t, true).collect::<std::vec::Vec<_>>(), [0x1111, 0x2222]);
        assert_eq!(sdt_entries(&t, false).count(), 4);
    }
}

/// Hostile firmware tables must be rejected, never crash the parser.
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
    fn random_and_mutated_tables_never_panic() {
        let mut s = 0xDEAD_BEEF_CAFE_F00Du64;
        let mut valid = [0u8; 36];
        valid[..8].copy_from_slice(b"RSD PTR ");
        valid[15] = 2;
        valid[20..24].copy_from_slice(&36u32.to_le_bytes());
        for _ in 0..50_000 {
            let len = (xorshift(&mut s) % 80) as usize;
            let mut buf: Vec<u8> = (0..len).map(|_| xorshift(&mut s) as u8).collect();
            if xorshift(&mut s) % 2 == 0 && len >= 8 {
                buf[..8].copy_from_slice(b"RSD PTR "); // get past the signature check
            }
            let _ = peek_header(&buf);
            let _ = validate(&buf);
            let mut m = valid.to_vec();
            let i = (xorshift(&mut s) % 36) as usize;
            m[i] ^= 1 << (xorshift(&mut s) % 8);
            let _ = validate(&m); // single bit flips must be caught or handled, never panic
        }
    }

    #[test]
    fn random_and_mutated_madts_never_panic() {
        let mut s = 0x1234_5678_9ABC_DEF1u64;
        for _ in 0..50_000 {
            let len = 44 + (xorshift(&mut s) % 60) as usize;
            let mut t: Vec<u8> = (0..len).map(|_| xorshift(&mut s) as u8).collect();
            t[..4].copy_from_slice(b"APIC");
            if xorshift(&mut s) % 3 == 0 {
                t.truncate(30 + (xorshift(&mut s) % 40) as usize);
            }
            let _ = parse_madt(&t); // must return, whatever the entry lengths say
        }
    }
}
