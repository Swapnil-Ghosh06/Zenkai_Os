//! Static ELF64 (x86_64, little endian) executable parser. Pure logic, no loading: it checks a file image
//! that may be hostile and says which segments to map where. Everything is bounds-checked with checked
//! arithmetic; a file that is anything but a plain static executable is refused with a reason.
//!
//! Accepted: `ET_EXEC`, machine x86_64, up to [`MAX_SEGMENTS`] `PT_LOAD` segments that are page-congruent,
//! inside the address range the caller allows, not writable and executable at once, not overlapping at page
//! granularity, with the entry point inside an executable segment.
//! Refused: shared objects/PIE, dynamic linking (`PT_INTERP`, `PT_DYNAMIC`), executable stacks.

pub const PAGE: u64 = 4096;
pub const MAX_SEGMENTS: usize = 8;
const MAX_PHDRS: usize = 32;

pub const PF_X: u32 = 1;
pub const PF_W: u32 = 2;
pub const PF_R: u32 = 4;

const EHDR_LEN: usize = 64;
const PHDR_LEN: usize = 56;
const ET_EXEC: u16 = 2;
const EM_X86_64: u16 = 0x3E;
const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const PT_INTERP: u32 = 3;
const PT_GNU_STACK: u32 = 0x6474_e551;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElfError {
    TooShort,
    BadMagic,
    /// Not 64-bit, not little endian, or not the current ELF version.
    BadIdent,
    /// Not `ET_EXEC` (shared objects and PIE need relocation, which Aeon does not do).
    NotExecutable,
    WrongMachine,
    BadHeaderSize,
    /// The program header table does not fit in the file or has an absurd size.
    BadPhdrTable,
    /// `PT_INTERP` or `PT_DYNAMIC`: needs a dynamic linker.
    Dynamic,
    ExecutableStack,
    NoLoadSegments,
    TooManySegments,
    /// File size larger than memory size, or the file bytes lie outside the file.
    SegmentOutOfFile,
    /// Address not congruent to the file offset modulo the page size.
    Misaligned,
    /// Outside the address range the caller allows (or wraps around).
    OutOfRange,
    WritableAndExecutable,
    Overlap,
    BadEntry,
}

/// One `PT_LOAD` segment, checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    pub vaddr: u64,
    pub mem_size: u64,
    pub file_off: usize,
    pub file_size: usize,
    /// `PF_R | PF_W | PF_X` bits.
    pub flags: u32,
}

impl Segment {
    /// First page address and page count the segment occupies.
    pub fn pages(&self) -> (u64, u64) {
        let first = self.vaddr & !(PAGE - 1);
        let end = self.vaddr.saturating_add(self.mem_size);
        (first, (end - first).div_ceil(PAGE))
    }

    /// Page numbers `[first, end)` the segment occupies. Cannot wrap, unlike byte addresses near `u64::MAX`.
    fn page_numbers(&self) -> (u64, u64) {
        (self.vaddr / PAGE, self.vaddr.saturating_add(self.mem_size).div_ceil(PAGE))
    }
    pub fn writable(&self) -> bool {
        self.flags & PF_W != 0
    }
    pub fn executable(&self) -> bool {
        self.flags & PF_X != 0
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Image {
    pub entry: u64,
    pub segments: [Option<Segment>; MAX_SEGMENTS],
}

impl Image {
    pub fn iter(&self) -> impl Iterator<Item = &Segment> {
        self.segments.iter().flatten()
    }
}

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o.checked_add(2)?)?.try_into().ok()?))
}
fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o.checked_add(4)?)?.try_into().ok()?))
}
fn u64_at(b: &[u8], o: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(o..o.checked_add(8)?)?.try_into().ok()?))
}

/// Parses and checks `file`. Every segment must lie in `[lo, hi)`, `max_total` bounds the sum of the
/// segments' memory sizes (so a tiny file cannot ask for gigabytes).
pub fn parse(file: &[u8], lo: u64, hi: u64, max_total: u64) -> Result<Image, ElfError> {
    // A page-aligned range means "end <= hi" also holds for the segment's last page, and nothing can wrap.
    if lo % PAGE != 0 || hi % PAGE != 0 || lo >= hi {
        return Err(ElfError::OutOfRange);
    }
    if file.len() < EHDR_LEN {
        return Err(ElfError::TooShort);
    }
    if file[0..4] != [0x7f, b'E', b'L', b'F'] {
        return Err(ElfError::BadMagic);
    }
    if file[4] != 2 || file[5] != 1 || file[6] != 1 {
        return Err(ElfError::BadIdent);
    }
    // The reads below cannot fail (the header is complete); `ok_or` keeps the code free of unwraps.
    let e_type = u16_at(file, 16).ok_or(ElfError::TooShort)?;
    let machine = u16_at(file, 18).ok_or(ElfError::TooShort)?;
    let entry = u64_at(file, 24).ok_or(ElfError::TooShort)?;
    let phoff = u64_at(file, 32).ok_or(ElfError::TooShort)?;
    let ehsize = u16_at(file, 52).ok_or(ElfError::TooShort)?;
    let phentsize = u16_at(file, 54).ok_or(ElfError::TooShort)?;
    let phnum = u16_at(file, 56).ok_or(ElfError::TooShort)? as usize;
    if e_type != ET_EXEC {
        return Err(ElfError::NotExecutable);
    }
    if machine != EM_X86_64 {
        return Err(ElfError::WrongMachine);
    }
    if ehsize as usize != EHDR_LEN || phentsize as usize != PHDR_LEN {
        return Err(ElfError::BadHeaderSize);
    }
    if phnum == 0 || phnum > MAX_PHDRS {
        return Err(ElfError::BadPhdrTable);
    }
    let phoff = usize::try_from(phoff).map_err(|_| ElfError::BadPhdrTable)?;
    let table_end = phnum.checked_mul(PHDR_LEN).and_then(|n| n.checked_add(phoff)).ok_or(ElfError::BadPhdrTable)?;
    if table_end > file.len() {
        return Err(ElfError::BadPhdrTable);
    }

    let mut img = Image { entry, segments: [None; MAX_SEGMENTS] };
    let mut count = 0;
    let mut total: u64 = 0;
    for i in 0..phnum {
        let o = phoff + i * PHDR_LEN;
        let p_type = u32_at(file, o).ok_or(ElfError::BadPhdrTable)?;
        let flags = u32_at(file, o + 4).ok_or(ElfError::BadPhdrTable)?;
        match p_type {
            PT_INTERP | PT_DYNAMIC => return Err(ElfError::Dynamic),
            PT_GNU_STACK => {
                if flags & PF_X != 0 {
                    return Err(ElfError::ExecutableStack);
                }
                continue;
            }
            PT_LOAD => {}
            _ => continue,
        }
        let off = u64_at(file, o + 8).ok_or(ElfError::BadPhdrTable)?;
        let vaddr = u64_at(file, o + 16).ok_or(ElfError::BadPhdrTable)?;
        let filesz = u64_at(file, o + 32).ok_or(ElfError::BadPhdrTable)?;
        let memsz = u64_at(file, o + 40).ok_or(ElfError::BadPhdrTable)?;
        if memsz == 0 {
            continue;
        }
        if count == MAX_SEGMENTS {
            return Err(ElfError::TooManySegments);
        }
        if filesz > memsz {
            return Err(ElfError::SegmentOutOfFile);
        }
        let file_end = off.checked_add(filesz).ok_or(ElfError::SegmentOutOfFile)?;
        if file_end > file.len() as u64 {
            return Err(ElfError::SegmentOutOfFile);
        }
        if vaddr % PAGE != off % PAGE {
            return Err(ElfError::Misaligned);
        }
        let end = vaddr.checked_add(memsz).ok_or(ElfError::OutOfRange)?;
        if vaddr < lo || end > hi {
            return Err(ElfError::OutOfRange);
        }
        if flags & PF_W != 0 && flags & PF_X != 0 {
            return Err(ElfError::WritableAndExecutable);
        }
        total = total.checked_add(memsz).ok_or(ElfError::OutOfRange)?;
        if total > max_total {
            return Err(ElfError::OutOfRange);
        }
        let seg = Segment { vaddr, mem_size: memsz, file_off: off as usize, file_size: filesz as usize, flags: flags & (PF_R | PF_W | PF_X) };
        // Page-granular overlap: two segments cannot share a page, they would need two permissions on it.
        let (p0, p1) = seg.page_numbers();
        for other in img.segments.iter().flatten() {
            let (q0, q1) = other.page_numbers();
            if p0 < q1 && q0 < p1 {
                return Err(ElfError::Overlap);
            }
        }
        img.segments[count] = Some(seg);
        count += 1;
    }
    if count == 0 {
        return Err(ElfError::NoLoadSegments);
    }
    let entry_ok = img.iter().any(|s| s.executable() && entry >= s.vaddr && entry < s.vaddr + s.mem_size);
    if !entry_ok {
        return Err(ElfError::BadEntry);
    }
    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LO: u64 = 0x40_0000;
    const HI: u64 = 0x1000_0000;
    const MAX: u64 = 1 << 20;

    struct Seg {
        typ: u32,
        flags: u32,
        off: u64,
        vaddr: u64,
        filesz: u64,
        memsz: u64,
    }

    fn seg(flags: u32, off: u64, vaddr: u64, filesz: u64, memsz: u64) -> Seg {
        Seg { typ: PT_LOAD, flags, off, vaddr, filesz, memsz }
    }

    /// Builds a file: header, program headers, then zero padding up to `len`.
    fn build(entry: u64, segs: &[Seg], len: usize) -> Vec<u8> {
        let mut f = vec![0u8; len.max(EHDR_LEN + segs.len() * PHDR_LEN)];
        f[0..4].copy_from_slice(&[0x7f, b'E', b'L', b'F']);
        f[4] = 2;
        f[5] = 1;
        f[6] = 1;
        f[16..18].copy_from_slice(&ET_EXEC.to_le_bytes());
        f[18..20].copy_from_slice(&EM_X86_64.to_le_bytes());
        f[24..32].copy_from_slice(&entry.to_le_bytes());
        f[32..40].copy_from_slice(&(EHDR_LEN as u64).to_le_bytes());
        f[52..54].copy_from_slice(&(EHDR_LEN as u16).to_le_bytes());
        f[54..56].copy_from_slice(&(PHDR_LEN as u16).to_le_bytes());
        f[56..58].copy_from_slice(&(segs.len() as u16).to_le_bytes());
        for (i, s) in segs.iter().enumerate() {
            let o = EHDR_LEN + i * PHDR_LEN;
            f[o..o + 4].copy_from_slice(&s.typ.to_le_bytes());
            f[o + 4..o + 8].copy_from_slice(&s.flags.to_le_bytes());
            f[o + 8..o + 16].copy_from_slice(&s.off.to_le_bytes());
            f[o + 16..o + 24].copy_from_slice(&s.vaddr.to_le_bytes());
            f[o + 24..o + 32].copy_from_slice(&s.vaddr.to_le_bytes());
            f[o + 32..o + 40].copy_from_slice(&s.filesz.to_le_bytes());
            f[o + 40..o + 48].copy_from_slice(&s.memsz.to_le_bytes());
            f[o + 48..o + 56].copy_from_slice(&PAGE.to_le_bytes());
        }
        f
    }

    fn good() -> Vec<u8> {
        // text at 0x400000 (file offset 0x1000), data+bss at 0x402000 (file offset 0x2000)
        build(0x40_1000, &[seg(PF_R | PF_X, 0x1000, 0x40_1000, 0x100, 0x100), seg(PF_R | PF_W, 0x2000, 0x40_2000, 0x80, 0x1000)], 0x3000)
    }

    #[test]
    fn accepts_a_plain_static_executable() {
        let img = parse(&good(), LO, HI, MAX).unwrap();
        assert_eq!(img.entry, 0x40_1000);
        let segs: Vec<_> = img.iter().collect();
        assert_eq!(segs.len(), 2);
        assert!(segs[0].executable() && !segs[0].writable());
        assert!(segs[1].writable() && !segs[1].executable());
        assert_eq!(segs[1].pages(), (0x40_2000, 1));
        assert_eq!(segs[1].file_size, 0x80);
    }

    #[test]
    fn pages_covers_an_unaligned_tail() {
        let s = Segment { vaddr: 0x40_1ff0, mem_size: 0x20, file_off: 0xff0, file_size: 0x20, flags: PF_R };
        assert_eq!(s.pages(), (0x40_1000, 2));
    }

    #[test]
    fn short_files_and_bad_magic() {
        assert_eq!(parse(&[], LO, HI, MAX), Err(ElfError::TooShort));
        assert_eq!(parse(&good()[..63], LO, HI, MAX), Err(ElfError::TooShort));
        let mut f = good();
        f[1] = b'X';
        assert_eq!(parse(&f, LO, HI, MAX), Err(ElfError::BadMagic));
    }

    #[test]
    fn header_fields_are_checked() {
        let mut f = good();
        f[4] = 1; // 32-bit
        assert_eq!(parse(&f, LO, HI, MAX), Err(ElfError::BadIdent));
        let mut f = good();
        f[5] = 2; // big endian
        assert_eq!(parse(&f, LO, HI, MAX), Err(ElfError::BadIdent));
        let mut f = good();
        f[16] = 3; // ET_DYN
        assert_eq!(parse(&f, LO, HI, MAX), Err(ElfError::NotExecutable));
        let mut f = good();
        f[18] = 0xB7; // aarch64
        assert_eq!(parse(&f, LO, HI, MAX), Err(ElfError::WrongMachine));
        let mut f = good();
        f[54] = 32; // wrong phentsize
        assert_eq!(parse(&f, LO, HI, MAX), Err(ElfError::BadHeaderSize));
    }

    #[test]
    fn program_header_table_must_fit() {
        let mut f = good();
        f[56] = 0; // no headers
        assert_eq!(parse(&f, LO, HI, MAX), Err(ElfError::BadPhdrTable));
        let mut f = good();
        f[56] = 33; // too many
        assert_eq!(parse(&f, LO, HI, MAX), Err(ElfError::BadPhdrTable));
        let mut f = good();
        f[32..40].copy_from_slice(&u64::MAX.to_le_bytes()); // offset wraps
        assert_eq!(parse(&f, LO, HI, MAX), Err(ElfError::BadPhdrTable));
        let mut f = good();
        f[32..40].copy_from_slice(&0x2fffu64.to_le_bytes()); // table runs past the end
        assert_eq!(parse(&f, LO, HI, MAX), Err(ElfError::BadPhdrTable));
    }

    #[test]
    fn dynamic_and_executable_stack_are_refused() {
        let f = build(0x40_1000, &[seg(PF_R | PF_X, 0x1000, 0x40_1000, 0x10, 0x10), Seg { typ: PT_INTERP, flags: PF_R, off: 0, vaddr: 0, filesz: 0, memsz: 0 }], 0x2000);
        assert_eq!(parse(&f, LO, HI, MAX), Err(ElfError::Dynamic));
        let f = build(0x40_1000, &[seg(PF_R | PF_X, 0x1000, 0x40_1000, 0x10, 0x10), Seg { typ: PT_DYNAMIC, flags: PF_R, off: 0, vaddr: 0, filesz: 0, memsz: 0 }], 0x2000);
        assert_eq!(parse(&f, LO, HI, MAX), Err(ElfError::Dynamic));
        let f = build(0x40_1000, &[seg(PF_R | PF_X, 0x1000, 0x40_1000, 0x10, 0x10), Seg { typ: PT_GNU_STACK, flags: PF_R | PF_W | PF_X, off: 0, vaddr: 0, filesz: 0, memsz: 0 }], 0x2000);
        assert_eq!(parse(&f, LO, HI, MAX), Err(ElfError::ExecutableStack));
        let f = build(0x40_1000, &[seg(PF_R | PF_X, 0x1000, 0x40_1000, 0x10, 0x10), Seg { typ: PT_GNU_STACK, flags: PF_R | PF_W, off: 0, vaddr: 0, filesz: 0, memsz: 0 }], 0x2000);
        assert!(parse(&f, LO, HI, MAX).is_ok());
    }

    #[test]
    fn segment_checks() {
        let rx = |off, vaddr, filesz, memsz| build(0x40_1000, &[seg(PF_R | PF_X, off, vaddr, filesz, memsz)], 0x2000);
        // file bytes beyond the file
        assert_eq!(parse(&rx(0x1000, 0x40_1000, 0x2000, 0x2000), LO, HI, MAX), Err(ElfError::SegmentOutOfFile));
        // offset + size wraps
        assert_eq!(parse(&rx(u64::MAX - 4, 0x40_1000 | 0xffb, 0x10, 0x10), LO, HI, MAX), Err(ElfError::SegmentOutOfFile));
        // filesz > memsz
        assert_eq!(parse(&rx(0x1000, 0x40_1000, 0x20, 0x10), LO, HI, MAX), Err(ElfError::SegmentOutOfFile));
        // not congruent
        assert_eq!(parse(&rx(0x1000, 0x40_1008, 0x10, 0x10), LO, HI, MAX), Err(ElfError::Misaligned));
        // below the allowed range, above it, wrapping
        assert_eq!(parse(&rx(0x1000, 0x1000, 0x10, 0x10), LO, HI, MAX), Err(ElfError::OutOfRange));
        assert_eq!(parse(&rx(0x1000, 0x2000_0000, 0x10, 0x10), LO, HI, MAX), Err(ElfError::OutOfRange));
        assert_eq!(parse(&rx(0x1000, 0xffff_ffff_ffff_f000, 0x10, 0x2000), 0, 0xffff_ffff_ffff_f000, MAX), Err(ElfError::OutOfRange));
        // kernel half of the address space
        assert_eq!(parse(&rx(0x1000, 0xffff_8000_0000_0000, 0x10, 0x10), LO, HI, MAX), Err(ElfError::OutOfRange));
        // asks for far more memory than the budget
        assert_eq!(parse(&rx(0x1000, 0x40_1000, 0x10, 0x800_0000), LO, HI, MAX), Err(ElfError::OutOfRange));
    }

    #[test]
    fn the_allowed_range_must_be_page_aligned() {
        assert_eq!(parse(&good(), LO + 1, HI, MAX), Err(ElfError::OutOfRange));
        assert_eq!(parse(&good(), LO, HI - 1, MAX), Err(ElfError::OutOfRange));
        assert_eq!(parse(&good(), HI, LO, MAX), Err(ElfError::OutOfRange));
    }

    #[test]
    fn segments_at_the_very_top_cannot_wrap_the_overlap_check() {
        let top = u64::MAX & !(PAGE - 1); // last page boundary; the range is [0, top)
        // ends exactly on the limit: fine on its own
        let one = build(top - PAGE, &[seg(PF_R | PF_X, 0x1000, top - PAGE, 0x10, PAGE)], 0x2000);
        assert!(parse(&one, 0, top, MAX).is_ok());
        // a second segment in the same top page must still be seen as overlapping
        let two = build(top - PAGE, &[seg(PF_R | PF_X, 0x1000, top - PAGE, 0x10, 0x10), seg(PF_R | PF_W, 0x1800, top - PAGE + 0x800, 0x10, 0x10)], 0x2000);
        assert_eq!(parse(&two, 0, top, MAX), Err(ElfError::Overlap));
        // one byte past the limit is out of range
        let past = build(top - PAGE, &[seg(PF_R | PF_X, 0x1000, top - PAGE, 0x10, PAGE + 1)], 0x2000);
        assert_eq!(parse(&past, 0, top, MAX), Err(ElfError::OutOfRange));
    }

    #[test]
    fn writable_and_executable_is_refused() {
        let f = build(0x40_1000, &[seg(PF_R | PF_W | PF_X, 0x1000, 0x40_1000, 0x10, 0x10)], 0x2000);
        assert_eq!(parse(&f, LO, HI, MAX), Err(ElfError::WritableAndExecutable));
    }

    #[test]
    fn segments_sharing_a_page_are_refused() {
        let f = build(0x40_1000, &[seg(PF_R | PF_X, 0x1000, 0x40_1000, 0x10, 0x10), seg(PF_R | PF_W, 0x1800, 0x40_1800, 0x10, 0x10)], 0x2000);
        assert_eq!(parse(&f, LO, HI, MAX), Err(ElfError::Overlap));
        // adjacent pages are fine
        let f = build(0x40_1000, &[seg(PF_R | PF_X, 0x1000, 0x40_1000, 0x10, 0x1000), seg(PF_R | PF_W, 0x2000, 0x40_2000, 0x10, 0x10)], 0x3000);
        assert!(parse(&f, LO, HI, MAX).is_ok());
    }

    #[test]
    fn entry_must_be_inside_an_executable_segment() {
        let segs = [seg(PF_R | PF_X, 0x1000, 0x40_1000, 0x10, 0x10), seg(PF_R | PF_W, 0x2000, 0x40_2000, 0x10, 0x10)];
        assert_eq!(parse(&build(0x40_2000, &segs, 0x3000), LO, HI, MAX), Err(ElfError::BadEntry)); // in data
        assert_eq!(parse(&build(0x50_0000, &segs, 0x3000), LO, HI, MAX), Err(ElfError::BadEntry)); // nowhere
        assert_eq!(parse(&build(0x40_1010, &segs, 0x3000), LO, HI, MAX), Err(ElfError::BadEntry)); // one past the end
        assert_eq!(parse(&build(0x40_100f, &segs, 0x3000), LO, HI, MAX).map(|i| i.entry), Ok(0x40_100f));
    }

    #[test]
    fn no_load_segments_and_too_many() {
        let f = build(0x40_1000, &[Seg { typ: 4, flags: PF_R, off: 0, vaddr: 0, filesz: 0, memsz: 0 }], 0x1000);
        assert_eq!(parse(&f, LO, HI, MAX), Err(ElfError::NoLoadSegments));
        // nine distinct pages: the ninth is one too many (offset 0x1000 is congruent to every page start)
        let segs: Vec<Seg> = (0..9).map(|i| seg(PF_R | PF_X, 0x1000, 0x40_0000 + i * 0x1000, 0, 0x10)).collect();
        assert_eq!(parse(&build(0x40_0000, &segs, 0x2000), LO, HI, MAX), Err(ElfError::TooManySegments));
    }

    #[test]
    fn random_garbage_never_panics() {
        let mut x: u64 = 0x1234_5678_9abc_def0;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let base = good();
        for round in 0..20_000 {
            let mut f = base.clone();
            // flip a few bytes, sometimes truncate
            for _ in 0..(1 + next() % 6) {
                let i = (next() % f.len() as u64) as usize;
                f[i] = next() as u8;
            }
            if round % 5 == 0 {
                let n = (next() % f.len() as u64) as usize;
                f.truncate(n);
            }
            let _ = parse(&f, LO, HI, MAX);
        }
        for _ in 0..2000 {
            let n = (next() % 200) as usize;
            let f: Vec<u8> = (0..n).map(|_| next() as u8).collect();
            let _ = parse(&f, LO, HI, MAX);
        }
    }
}
