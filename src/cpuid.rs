//! CPUID feature decoding. The raw `cpuid` instruction lives in `arch`; this module only interprets
//! register values, and refuses to look at a leaf the CPU says it does not implement.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Regs {
    pub eax: u32,
    pub ebx: u32,
    pub ecx: u32,
    pub edx: u32,
}

/// Raw leaves, each `None` when the CPU's maximum leaf is below it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Leaves {
    pub max_basic: u32,
    pub max_ext: u32,
    pub leaf1: Option<Regs>,
    pub leaf7: Option<Regs>,
    pub ext1: Option<Regs>,
    pub ext8: Option<Regs>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Features {
    pub tsc: bool,
    pub fxsr: bool,
    pub sse2: bool,
    pub apic: bool,
    pub rdrand: bool,
    pub hypervisor: bool,
    pub nx: bool,
    pub smep: bool,
    pub smap: bool,
    pub umip: bool,
    pub rdseed: bool,
    /// Control-flow Enforcement Technology: supervisor shadow stacks and indirect-branch tracking.
    pub cet_ss: bool,
    pub cet_ibt: bool,
    pub phys_bits: u8,
    pub virt_bits: u8,
}

fn bit(v: u32, n: u32) -> bool {
    v & (1 << n) != 0
}

/// Missing leaves read as "feature absent". Address widths default to 36/48 bits when leaf
/// `0x8000_0008` is unavailable, as the architecture manual prescribes.
pub fn decode(l: &Leaves) -> Features {
    let f1 = l.leaf1.unwrap_or_default();
    let f7 = l.leaf7.unwrap_or_default();
    let e1 = l.ext1.unwrap_or_default();
    let (phys_bits, virt_bits) = match l.ext8 {
        Some(r) if r.eax != 0 => ((r.eax & 0xFF) as u8, ((r.eax >> 8) & 0xFF) as u8),
        _ => (36, 48),
    };
    Features {
        tsc: bit(f1.edx, 4),
        apic: bit(f1.edx, 9),
        fxsr: bit(f1.edx, 24),
        sse2: bit(f1.edx, 26),
        rdrand: bit(f1.ecx, 30),
        hypervisor: bit(f1.ecx, 31),
        nx: bit(e1.edx, 20),
        smep: bit(f7.ebx, 7),
        smap: bit(f7.ebx, 20),
        umip: bit(f7.ecx, 2),
        rdseed: bit(f7.ebx, 18),
        cet_ss: bit(f7.ecx, 7),
        cet_ibt: bit(f7.edx, 20),
        phys_bits,
        virt_bits,
    }
}

/// 12-byte vendor string from leaf 0 (`ebx, edx, ecx` order).
pub fn vendor(leaf0: Regs) -> [u8; 12] {
    let mut v = [0u8; 12];
    v[0..4].copy_from_slice(&leaf0.ebx.to_le_bytes());
    v[4..8].copy_from_slice(&leaf0.edx.to_le_bytes());
    v[8..12].copy_from_slice(&leaf0.ecx.to_le_bytes());
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regs(eax: u32, ebx: u32, ecx: u32, edx: u32) -> Option<Regs> {
        Some(Regs { eax, ebx, ecx, edx })
    }

    #[test]
    fn old_cpu_without_leaf7_reports_no_smep_smap() {
        // Garbage that would look like SMEP/SMAP if leaf 7 were read blindly.
        let l = Leaves { max_basic: 4, max_ext: 0x8000_0001, leaf1: regs(0, 0, 0, 1 << 4), ..Leaves::default() };
        let f = decode(&l);
        assert!(f.tsc && !f.smep && !f.smap && !f.umip && !f.cet_ss && !f.cet_ibt && !f.rdseed);
        assert_eq!((f.phys_bits, f.virt_bits), (36, 48));
    }

    #[test]
    fn leaf7_bits_are_decoded_when_present() {
        let l = Leaves {
            max_basic: 7,
            max_ext: 0x8000_0008,
            leaf1: regs(0, 0, 1 << 31, 0),
            leaf7: regs(0, (1 << 7) | (1 << 20) | (1 << 18), (1 << 2) | (1 << 7), 1 << 20),
            ext1: regs(0, 0, 0, 1 << 20),
            ext8: regs(39 | (48 << 8), 0, 0, 0),
        };
        let f = decode(&l);
        assert!(f.smep && f.smap && f.umip && f.nx && f.hypervisor);
        assert!(f.rdseed && f.cet_ss && f.cet_ibt);
        assert_eq!((f.phys_bits, f.virt_bits), (39, 48));
    }

    #[test]
    fn vendor_string() {
        let v = vendor(Regs { eax: 0, ebx: u32::from_le_bytes(*b"Genu"), edx: u32::from_le_bytes(*b"ineI"), ecx: u32::from_le_bytes(*b"ntel") });
        assert_eq!(&v, b"GenuineIntel");
    }
}
