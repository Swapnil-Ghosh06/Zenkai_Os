//! Kernel page tables.
//!
//! The kernel runs identity-mapped (virtual == physical) with 2 MiB pages, except where finer
//! control is needed: the kernel image (4 KiB pages with W^X permissions taken from the PE section
//! headers), page 0 (unmapped, so null dereferences fault) and the stack guard page. RAM is mapped
//! no-execute except the firmware's runtime-services code; MMIO is uncached and the framebuffer
//! write-combined.

use crate::bootinfo::BootInfo;
use crate::global::Global;
use crate::memmap::Kind;

const P: u64 = 1;
const RW: u64 = 1 << 1;
const PWT: u64 = 1 << 3;
const PCD: u64 = 1 << 4;
const US: u64 = 1 << 2;
const PS: u64 = 1 << 7;
/// PAT selector bit in a 2 MiB page entry (bit 7 is PS there, so PAT lives at bit 12).
const PAT_LARGE: u64 = 1 << 12;
/// PAT selector bit in a 4 KiB page entry.
const PAT_4K: u64 = 1 << 7;
const NX: u64 = 1 << 63;
const ADDR: u64 = 0x000F_FFFF_FFFF_F000;
const CACHE_MASK: u64 = PWT | PCD | PAT_LARGE;

const SZ_2M: u64 = 2 * 1024 * 1024;
const SZ_4K: u64 = 4096;
const GIB: u64 = 1 << 30;
/// Two PDPTs (PML4 entries 0 and 1) cover 1 TiB, enough for the 64-bit device windows firmware and
/// QEMU place high in the address space.
const MAX_PHYS: u64 = 1024 * GIB;

const N_PD: usize = 64;
const N_PT: usize = 64;

#[repr(C, align(4096))]
struct Table([u64; 512]);

static PML4: Global<Table> = Global::new(Table([0; 512]));
/// Two adjacent tables, so one index `addr / 1 GiB` (below 1024) addresses both.
static PDPT: Global<[Table; 2]> = Global::new([const { Table([0; 512]) }; 2]);
static PDS: Global<[Table; N_PD]> = Global::new([const { Table([0; 512]) }; N_PD]);
static PTS: Global<[Table; N_PT]> = Global::new([const { Table([0; 512]) }; N_PT]);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cache {
    Wb,
    Uc,
    /// Write-combining; needs `arch::program_pat_wc` first, otherwise falls back to UC.
    Wc,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub pd_used: usize,
    pub pt_used: usize,
    /// Pages that could not be mapped (beyond 1 TiB, or the table pool ran out).
    pub skipped: usize,
}

struct PagingState {
    stats: Stats,
    nx: u64,
    wc_ok: bool,
}

static STATE: Global<PagingState> = Global::new(PagingState { stats: Stats { pd_used: 0, pt_used: 0, skipped: 0 }, nx: 0, wc_ok: false });

struct Builder {
    pml4: *mut Table,
    pdpt: *mut Table,
    pds: *mut Table,
    pts: *mut Table,
    nx: u64,
    wc_ok: bool,
    stats: Stats,
}

/// Where a 2 MiB-aligned address lives: the PD entry slot, or `None` if the tables ran out.
type Slot = Option<*mut u64>;

impl Builder {
    fn cache_bits(&self, c: Cache) -> u64 {
        match c {
            Cache::Wb => 0,
            Cache::Uc => PCD | PWT,
            Cache::Wc if self.wc_ok => PAT_LARGE,
            Cache::Wc => PCD | PWT,
        }
    }

    fn pd_slot(&mut self, addr: u64) -> Slot {
        if addr >= MAX_PHYS {
            return None;
        }
        let gb = (addr / GIB) as usize;
        // SAFETY: `gb < 1024` so the index is inside the pair of PDPTs; the table pointers come from statics
        // owned exclusively by this builder.
        unsafe {
            let e = self.pdpt.cast::<u64>().add(gb);
            if *e & P == 0 {
                if self.stats.pd_used >= N_PD {
                    return None;
                }
                let pd = self.pds.add(self.stats.pd_used);
                self.stats.pd_used += 1;
                *e = pd as u64 | P | RW;
            }
            let pd = (*e & ADDR) as *mut u64;
            Some(pd.add(((addr % GIB) / SZ_2M) as usize))
        }
    }

    /// Ensures the 2 MiB page at `addr` exists, optionally making it executable and/or changing
    /// its caching. New pages start as no-execute write-back.
    fn set_large(&mut self, addr: u64, exec: bool, cache: Option<Cache>) {
        let addr = addr & !(SZ_2M - 1);
        let Some(slot) = self.pd_slot(addr) else {
            self.stats.skipped += 1;
            return;
        };
        let init_cache = self.cache_bits(cache.unwrap_or(Cache::Wb));
        // SAFETY: `slot` points into a live page-directory table.
        unsafe {
            if *slot & P == 0 {
                *slot = addr | P | RW | PS | self.nx | init_cache;
            } else if *slot & PS != 0 {
                if let Some(c) = cache {
                    *slot = (*slot & !CACHE_MASK) | self.cache_bits(c);
                }
            }
            if exec && *slot & PS != 0 {
                *slot &= !NX;
            }
        }
    }

    fn map_range(&mut self, start: u64, end: u64, exec: bool, cache: Option<Cache>) {
        let mut a = start & !(SZ_2M - 1);
        while a < end {
            self.set_large(a, exec, cache);
            a += SZ_2M;
        }
    }

    /// Splits the 2 MiB page containing `addr` into 512 4 KiB pages with the same attributes and
    /// returns the page table.
    fn split(&mut self, addr: u64) -> Option<*mut u64> {
        let base = addr & !(SZ_2M - 1);
        let slot = self.pd_slot(base)?;
        // SAFETY: `slot` is inside a live PD; `pt` is a pool table exclusively owned by us.
        unsafe {
            if *slot & P == 0 {
                return None;
            }
            if *slot & PS == 0 {
                return Some((*slot & ADDR) as *mut u64);
            }
            if self.stats.pt_used >= N_PT {
                return None;
            }
            let pt = self.pts.add(self.stats.pt_used).cast::<u64>();
            self.stats.pt_used += 1;
            // 4 KiB entries keep NX and the UC/WT bits; the large-page PAT bit moves to the 4 KiB position.
            let attrs = (*slot & (NX | PWT | PCD)) | if *slot & PAT_LARGE != 0 { PAT_4K } else { 0 };
            for i in 0..512u64 {
                *pt.add(i as usize) = (base + i * SZ_4K) | P | RW | attrs;
            }
            // Non-leaf entry: NX stays clear so the leaves decide.
            *slot = pt as u64 | P | RW;
            Some(pt)
        }
    }

    /// Sets one 4 KiB page: `None` unmaps it, `Some((writable, exec))` maps it.
    fn set_page(&mut self, addr: u64, perm: Option<(bool, bool)>) -> bool {
        let Some(pt) = self.split(addr) else {
            return false;
        };
        let idx = ((addr % SZ_2M) / SZ_4K) as usize;
        // SAFETY: `idx < 512`, inside the page table returned by `split`.
        unsafe {
            let e = pt.add(idx);
            let keep = *e & (PWT | PCD | PAT_4K);
            *e = match perm {
                None => 0,
                Some((w, x)) => {
                    (addr & ADDR)
                        | P
                        | keep
                        | if w { RW } else { 0 }
                        | if x { 0 } else { self.nx }
                }
            };
        }
        true
    }
}

impl Builder {
    /// Maps one 4 KiB page for ring 3: user-accessible, with the given write/execute rights. The
    /// entries above the leaf get the user bit too, but every other leaf stays supervisor-only, so
    /// the rest of the kernel is still out of reach.
    fn set_user_page(&mut self, addr: u64, writable: bool, exec: bool) -> bool {
        let Some(pt) = self.split(addr) else {
            return false;
        };
        let idx = ((addr % SZ_2M) / SZ_4K) as usize;
        let Some(pd_slot) = self.pd_slot(addr & !(SZ_2M - 1)) else {
            return false;
        };
        // SAFETY: `idx < 512` inside the page table from `split`; `pd_slot`, the PDPT entry and PML4
        // entry 0 are live table entries owned by this builder (`addr` is below 512 GiB).
        unsafe {
            let e = pt.add(idx);
            let keep = *e & (PWT | PCD | PAT_4K);
            *e = (addr & ADDR) | P | US | keep | if writable { RW } else { 0 } | if exec { 0 } else { self.nx };
            *pd_slot |= US;
            *self.pdpt.cast::<u64>().add((addr / GIB) as usize) |= US;
            *self.pml4.cast::<u64>() |= US;
        }
        true
    }
}

fn rd16(b: &[u8], o: usize) -> Option<usize> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?) as usize)
}
fn rd32(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

const SCN_EXEC: u32 = 0x2000_0000;
const SCN_WRITE: u32 = 0x8000_0000;

/// Sections of a loaded PE image: `(rva, size, characteristics)`.
fn pe_sections(hdr: &[u8], mut each: impl FnMut(u32, u32, u32)) -> Option<()> {
    if hdr.get(0..2)? != b"MZ" {
        return None;
    }
    let pe = rd32(hdr, 0x3C)? as usize;
    if hdr.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    let nsec = rd16(hdr, pe + 6)?;
    let opt = rd16(hdr, pe + 20)?;
    let table = pe + 24 + opt;
    for i in 0..nsec {
        let s = table + i * 40;
        let vsize = rd32(hdr, s + 8)?;
        let va = rd32(hdr, s + 12)?;
        let raw = rd32(hdr, s + 16)?;
        let ch = rd32(hdr, s + 36)?;
        each(va, vsize.max(raw), ch);
    }
    Some(())
}

/// Builds the kernel page tables and returns what was used.
///
/// `guard_page` is the 4 KiB page to leave unmapped at the bottom of the kernel stack.
///
/// # Safety
/// Must run once, single-threaded, after the memory map in `info` is final. `info.image_base`
/// must be the address of the loaded PE image. Nothing may be using the paging statics.
pub unsafe fn build(info: &BootInfo, nx: bool, wc_ok: bool, guard_page: u64) -> Stats {
    let mut b = Builder {
        pml4: PML4.as_ptr(),
        pdpt: PDPT.as_ptr().cast::<Table>(),
        pds: PDS.as_ptr().cast::<Table>(),
        pts: PTS.as_ptr().cast::<Table>(),
        nx: if nx { NX } else { 0 },
        wc_ok,
        stats: Stats::default(),
    };
    // SAFETY: PML4 is a static, 4 KiB-aligned table; entry 0 covers the first 512 GiB and entry 1 the
    // next 512 GiB, through the second table of the adjacent pair.
    unsafe {
        *b.pml4.cast::<u64>() = b.pdpt as u64 | P | RW;
        *b.pml4.cast::<u64>().add(1) = (b.pdpt as u64 + 4096) | P | RW;
    }

    // Low 4 GiB first (legacy MMIO such as the APIC lives here), then everything the firmware
    // described.
    b.map_range(0, 4 * GIB, false, None);
    for r in info.regions() {
        match r.kind {
            Kind::Mmio => b.map_range(r.start, r.end(), false, Some(Cache::Uc)),
            _ => b.map_range(r.start, r.end(), r.mem_type == 5, None),
        }
    }
    if let Some(fb) = &info.fb {
        b.map_range(fb.base, fb.base + fb.size as u64, false, Some(Cache::Wc));
    }

    // W^X for the kernel image, from the PE section headers.
    let base = info.image_base;
    let size = info.image_size;
    if base % SZ_4K == 0 && size > 0 {
        let end = (base + size + SZ_4K - 1) & !(SZ_4K - 1);
        let mut a = base;
        while a < end {
            b.set_page(a, Some((false, false))); // headers and gaps: read-only, no-execute
            a += SZ_4K;
        }
        // SAFETY: the image was loaded at `base` and is at least one page long; we read only the
        // first page, which holds the PE headers.
        let hdr = unsafe { core::slice::from_raw_parts(base as *const u8, (size.min(SZ_4K)) as usize) };
        let _ = pe_sections(hdr, |va, vsize, ch| {
            let exec = ch & SCN_EXEC != 0;
            let write = ch & SCN_WRITE != 0 && !exec;
            let first = base + (va as u64 & !(SZ_4K - 1));
            let last = base + ((va as u64 + vsize as u64 + SZ_4K - 1) & !(SZ_4K - 1));
            let mut p = first;
            while p < last.min(end) {
                b.set_page(p, Some((write, exec)));
                p += SZ_4K;
            }
        });
    }

    // Both must really be unmapped: a silent failure would leave null dereferences or stack overflows unchecked.
    let guards_ok = b.set_page(0, None) & b.set_page(guard_page, None);
    assert!(guards_ok, "page-table pool exhausted while unmapping the null page or the stack guard");
    // SAFETY: single-threaded init; remember the pool state so pages can be remapped later.
    unsafe { *STATE.get() = PagingState { stats: b.stats, nx: b.nx, wc_ok: b.wc_ok } };
    b.stats
}

/// Runs `f` on a builder that continues from the state `build` left behind.
fn with_builder<R>(f: impl FnOnce(&mut Builder) -> R) -> R {
    crate::arch::without_interrupts(|| {
        // SAFETY: interrupts are off on a single CPU, so nothing else touches the tables or STATE.
        let st = unsafe { STATE.get() };
        let mut b = Builder {
            pml4: PML4.as_ptr(),
            pdpt: PDPT.as_ptr().cast::<Table>(),
            pds: PDS.as_ptr().cast::<Table>(),
            pts: PTS.as_ptr().cast::<Table>(),
            nx: st.nx,
            wc_ok: st.wc_ok,
            stats: st.stats,
        };
        let r = f(&mut b);
        st.stats = b.stats;
        r
    })
}

fn invlpg(addr: u64) {
    // SAFETY: invalidating a TLB entry has no effect on memory contents.
    unsafe { core::arch::asm!("invlpg [{}]", in(reg) addr, options(nostack, preserves_flags)) };
}

/// Unmaps one 4 KiB page so that any access faults (used as a stack guard). Returns false if the
/// page-table pool is exhausted, in which case the page stays mapped.
pub fn guard_page(addr: u64) -> bool {
    let ok = with_builder(|b| b.set_page(addr, None));
    invlpg(addr);
    ok
}

/// Maps a page previously unmapped with [`guard_page`] back as read-write, no-execute.
pub fn restore_page(addr: u64) -> bool {
    let ok = with_builder(|b| b.set_page(addr, Some((true, false))));
    invlpg(addr);
    ok
}

/// Maps one 4 KiB page so that ring 3 can use it (`writable` and `exec` are never both true).
pub fn user_page(addr: u64, writable: bool, exec: bool) -> bool {
    if writable && exec {
        return false; // never both: user pages are W^X in release builds too
    }
    let ok = with_builder(|b| b.set_user_page(addr, writable, exec));
    invlpg(addr);
    ok
}

/// Makes sure the 2 MiB page holding `addr` is mapped uncached and no-execute (device registers).
/// Returns false if that page is not a plain large page afterwards, so callers must not use it.
pub fn map_mmio_uc(addr: u64) -> bool {
    let ok = with_builder(|b| {
        b.set_large(addr, false, Some(Cache::Uc));
        let Some(slot) = b.pd_slot(addr & !(SZ_2M - 1)) else {
            return false;
        };
        // SAFETY: `slot` points into a live page-directory table.
        let e = unsafe { *slot };
        e & P != 0 && e & PS != 0 && e & (PCD | PWT) == (PCD | PWT)
    });
    invlpg(addr);
    ok
}

/// Loads the page tables built by [`build`].
///
/// # Safety
/// `build` must have run, and every address executed or accessed afterwards (kernel image, stacks,
/// IDT/GDT, framebuffer, `BootInfo`) must be mapped by it.
pub unsafe fn activate() {
    // SAFETY: contract above; the PML4 is identity-mapped so its virtual address is its physical one.
    unsafe { crate::arch::write_cr3(PML4.as_ptr() as u64) };
}
