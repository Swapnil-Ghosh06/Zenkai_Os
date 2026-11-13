//! Read-only FAT16 and FAT32 reader over a [`BlockDev`]. Pure logic, host-tested, written for hostile
//! disks: every size, cluster number and chain comes from the disk, so each is range-checked, chains
//! and directories are walked with hard step limits (a loop ends in an error), and nothing the disk says
//! is used as a pointer or an unbounded allocation.
//!
//! Supported: 512-byte sectors, FAT16 (at least 4085 clusters; FAT12 is refused) and FAT32, long file
//! names (ASCII only; other characters show as `?`), subdirectories. Not supported: writing, exFAT,
//! partition tables (point `open` at the partition's first sector).

use alloc::vec::Vec;

use crate::block::{BlockDev, BlockError, SECTOR_SIZE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FatError {
    /// The boot sector is not a usable FAT boot sector.
    BadBpb,
    /// A valid layout this reader does not handle (FAT12, other sector sizes).
    Unsupported,
    /// A cluster chain or directory is damaged (free or bad cluster, out of range, loop, too short).
    BadChain,
    /// The file is larger than the caller allows.
    TooBig,
    NotFound,
    NotAFile,
    NotADirectory,
    /// Empty component, `.` or `..`.
    BadPath,
    Io(BlockError),
}

impl From<BlockError> for FatError {
    fn from(e: BlockError) -> Self {
        FatError::Io(e)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Fat16,
    Fat32,
}

/// Longest long file name FAT allows.
const MAX_NAME: usize = 255;
/// A directory is never walked past this many entries (a FAT directory holds at most 65536).
const MAX_DIR_ENTRIES: u32 = 65_536;

#[derive(Clone, Copy)]
pub struct DirEntry {
    long: [u8; MAX_NAME],
    long_len: usize,
    short: [u8; 12],
    short_len: usize,
    pub is_dir: bool,
    pub size: u32,
    /// First cluster; 0 means the root directory (for a directory) or an empty file.
    pub cluster: u32,
}

impl DirEntry {
    /// The name to show: the long name if there is one, otherwise the 8.3 name.
    pub fn name(&self) -> &str {
        let b = if self.long_len > 0 { &self.long[..self.long_len] } else { &self.short[..self.short_len] };
        core::str::from_utf8(b).unwrap_or("?")
    }

    fn root() -> DirEntry {
        DirEntry { long: [0; MAX_NAME], long_len: 0, short: [0; 12], short_len: 0, is_dir: true, size: 0, cluster: 0 }
    }

    fn matches(&self, want: &str) -> bool {
        let w = want.as_bytes();
        (self.long_len > 0 && self.long[..self.long_len].eq_ignore_ascii_case(w)) || self.short[..self.short_len].eq_ignore_ascii_case(w)
    }
}

pub struct Volume {
    kind: Kind,
    spc: u32,
    /// Absolute sector numbers.
    fat_start: u64,
    root16_start: u64,
    root16_sectors: u64,
    root_cluster: u32,
    data_start: u64,
    /// Number of data clusters (valid cluster numbers are `2..clusters + 2`).
    clusters: u32,
}

fn le16(b: &[u8], o: usize) -> u32 {
    u32::from(u16::from_le_bytes([b[o], b[o + 1]]))
}

fn le32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

impl Volume {
    /// Opens the volume whose boot sector is at sector `base` of `dev`.
    pub fn open(dev: &mut dyn BlockDev, base: u64) -> Result<Volume, FatError> {
        let mut s = [0u8; SECTOR_SIZE];
        dev.read(base, &mut s)?;
        if s[510] != 0x55 || s[511] != 0xAA {
            return Err(FatError::BadBpb);
        }
        match le16(&s, 11) {
            512 => {}
            1024 | 2048 | 4096 => return Err(FatError::Unsupported),
            _ => return Err(FatError::BadBpb),
        }
        let spc = u32::from(s[13]);
        if spc == 0 || !spc.is_power_of_two() || spc > 128 {
            return Err(FatError::BadBpb);
        }
        let reserved = u64::from(le16(&s, 14));
        let nfats = u64::from(s[16]);
        if reserved == 0 || nfats == 0 || nfats > 4 {
            return Err(FatError::BadBpb);
        }
        let root_entries = u64::from(le16(&s, 17));
        let total = if le16(&s, 19) != 0 { u64::from(le16(&s, 19)) } else { u64::from(le32(&s, 32)) };
        if total == 0 || total > dev.sectors().saturating_sub(base) {
            return Err(FatError::BadBpb); // empty, or claims more than the disk has
        }
        let (kind, fat_sectors) = if le16(&s, 22) != 0 { (Kind::Fat16, u64::from(le16(&s, 22))) } else { (Kind::Fat32, u64::from(le32(&s, 36))) };
        if fat_sectors == 0 {
            return Err(FatError::BadBpb);
        }
        match kind {
            Kind::Fat16 if root_entries == 0 => return Err(FatError::BadBpb),
            Kind::Fat32 if root_entries != 0 => return Err(FatError::BadBpb),
            _ => {}
        }
        let root_sectors = (root_entries * 32).div_ceil(SECTOR_SIZE as u64);
        let data_rel = reserved + nfats * fat_sectors + root_sectors;
        if data_rel >= total {
            return Err(FatError::BadBpb);
        }
        let clusters = (total - data_rel) / u64::from(spc);
        let entry_bytes = if kind == Kind::Fat16 { 2 } else { 4 };
        if clusters == 0 || clusters > 0x0FFF_FFF5 || (clusters + 2) * entry_bytes > fat_sectors * SECTOR_SIZE as u64 {
            return Err(FatError::BadBpb); // the FAT cannot describe this many clusters
        }
        if kind == Kind::Fat16 {
            if clusters < 4085 {
                return Err(FatError::Unsupported); // FAT12 territory
            }
            if clusters > 65_524 {
                return Err(FatError::BadBpb);
            }
        }
        let root_cluster = if kind == Kind::Fat32 { le32(&s, 44) } else { 0 };
        if kind == Kind::Fat32 && !(2..clusters as u32 + 2).contains(&root_cluster) {
            return Err(FatError::BadBpb);
        }
        Ok(Volume {
            kind,
            spc,
            fat_start: base + reserved,
            root16_start: base + reserved + nfats * fat_sectors,
            root16_sectors: root_sectors,
            root_cluster,
            data_start: base + data_rel,
            clusters: clusters as u32,
        })
    }

    /// Opens the first FAT volume on `dev`: the whole disk if sector 0 is a FAT boot sector, otherwise the
    /// first MBR or GPT partition that holds one. Partition tables are checked and bounded like all disk data.
    pub fn open_any(dev: &mut dyn BlockDev) -> Result<Volume, FatError> {
        let first_err = match Volume::open(dev, 0) {
            Ok(v) => return Ok(v),
            Err(e) => e,
        };
        let mut s = [0u8; SECTOR_SIZE];
        dev.read(0, &mut s)?;
        if s[510] != 0x55 || s[511] != 0xAA {
            return Err(first_err);
        }
        // MBR: four entries; a type of 0xEE says the real table is a GPT.
        let mut gpt = false;
        for i in 0..4 {
            let e = &s[446 + 16 * i..446 + 16 * (i + 1)];
            match e[4] {
                0 => {}
                0xEE => gpt = true,
                _ => {
                    if let Ok(v) = Volume::open(dev, u64::from(le32(e, 8))) {
                        return Ok(v);
                    }
                }
            }
        }
        if gpt {
            dev.read(1, &mut s)?;
            if &s[..8] == b"EFI PART" {
                let entries_lba = u64::from_le_bytes(s[72..80].try_into().unwrap_or([0; 8]));
                let count = le32(&s, 80).min(128);
                let size = le32(&s, 84) as usize;
                if (128..=SECTOR_SIZE).contains(&size) && size % 8 == 0 {
                    let per_sector = SECTOR_SIZE / size;
                    for idx in 0..count as usize {
                        let mut sec = [0u8; SECTOR_SIZE];
                        let lba = entries_lba.checked_add((idx / per_sector) as u64).ok_or(FatError::BadBpb)?;
                        dev.read(lba, &mut sec)?;
                        let e = &sec[(idx % per_sector) * size..(idx % per_sector + 1) * size];
                        if e[..16].iter().all(|&b| b == 0) {
                            continue; // unused entry
                        }
                        let start = u64::from_le_bytes(e[32..40].try_into().unwrap_or([0; 8]));
                        if let Ok(v) = Volume::open(dev, start) {
                            return Ok(v);
                        }
                    }
                }
            }
        }
        Err(first_err)
    }

    pub fn is_fat32(&self) -> bool {
        self.kind == Kind::Fat32
    }

    fn cluster_sector(&self, c: u32) -> u64 {
        self.data_start + u64::from(c - 2) * u64::from(self.spc)
    }

    /// The cluster after `c` in its chain, `None` at the end. Errors on free, bad or out-of-range links.
    fn next(&self, dev: &mut dyn BlockDev, cache: &mut FatCache, c: u32) -> Result<Option<u32>, FatError> {
        let width = if self.kind == Kind::Fat16 { 2u64 } else { 4 };
        let off = u64::from(c) * width;
        let lba = self.fat_start + off / SECTOR_SIZE as u64;
        if cache.lba != Some(lba) {
            dev.read(lba, &mut cache.data)?;
            cache.lba = Some(lba);
        }
        let o = (off % SECTOR_SIZE as u64) as usize;
        let (v, eoc, bad) = match self.kind {
            Kind::Fat16 => (le16(&cache.data, o), 0xFFF8, 0xFFF7),
            Kind::Fat32 => (le32(&cache.data, o) & 0x0FFF_FFFF, 0x0FFF_FFF8, 0x0FFF_FFF7),
        };
        if v >= eoc {
            Ok(None)
        } else if v == bad || v < 2 || v > self.clusters + 1 {
            Err(FatError::BadChain)
        } else {
            Ok(Some(v))
        }
    }

    /// Calls `f(sector_lba)` for every sector of the chain starting at `first`, stopping early when `f`
    /// returns `false`. A chain longer than the volume has clusters is a loop and ends in an error.
    fn walk_chain(&self, dev: &mut dyn BlockDev, first: u32, mut f: impl FnMut(&mut dyn BlockDev, u64) -> Result<bool, FatError>) -> Result<(), FatError> {
        let mut cache = FatCache::new();
        let mut c = first;
        // Brent's cycle detection: a chain that revisits a cluster is a loop, however long the file claims to be.
        let (mut tortoise, mut power, mut lam) = (first, 1u32, 1u32);
        for _ in 0..=self.clusters {
            if !(2..self.clusters + 2).contains(&c) {
                return Err(FatError::BadChain);
            }
            let base = self.cluster_sector(c);
            for i in 0..u64::from(self.spc) {
                if !f(dev, base + i)? {
                    return Ok(());
                }
            }
            match self.next(dev, &mut cache, c)? {
                Some(n) => {
                    if n == tortoise {
                        return Err(FatError::BadChain);
                    }
                    if lam == power {
                        tortoise = n;
                        power = power.saturating_mul(2);
                        lam = 0;
                    }
                    lam += 1;
                    c = n;
                }
                None => return Ok(()),
            }
        }
        Err(FatError::BadChain) // more links than clusters: a loop
    }

    /// Walks the directory at `dir` (0 = the root), calling `f` for each real entry until it returns `false`.
    fn scan_dir(&self, dev: &mut dyn BlockDev, dir: u32, f: &mut dyn FnMut(&DirEntry) -> bool) -> Result<(), FatError> {
        let mut st = DirState::new();
        let mut seen = 0u32;
        let mut sector = |dev: &mut dyn BlockDev, lba: u64| -> Result<bool, FatError> {
            let mut buf = [0u8; SECTOR_SIZE];
            dev.read(lba, &mut buf)?;
            for raw in buf.chunks_exact(32) {
                seen += 1;
                if seen > MAX_DIR_ENTRIES {
                    return Err(FatError::BadChain);
                }
                match st.feed(raw) {
                    Feed::End => return Ok(false),
                    Feed::Entry(e) => {
                        if !f(&e) {
                            return Ok(false);
                        }
                    }
                    Feed::Skip => {}
                }
            }
            Ok(true)
        };
        if dir == 0 && self.kind == Kind::Fat16 {
            for i in 0..self.root16_sectors {
                if !sector(dev, self.root16_start + i)? {
                    break;
                }
            }
            return Ok(());
        }
        let first = if dir == 0 { self.root_cluster } else { dir };
        self.walk_chain(dev, first, sector)
    }

    /// Calls `f` for each entry of the directory at `path` ("" or "/" is the root).
    pub fn list(&self, dev: &mut dyn BlockDev, path: &str, mut f: impl FnMut(&DirEntry)) -> Result<(), FatError> {
        let dir = self.find(dev, path)?;
        if !dir.is_dir {
            return Err(FatError::NotADirectory);
        }
        self.scan_dir(dev, dir.cluster, &mut |e| {
            f(e);
            true
        })
    }

    /// Finds the file or directory at `path`, case-insensitively. Components are separated by `/`.
    pub fn find(&self, dev: &mut dyn BlockDev, path: &str) -> Result<DirEntry, FatError> {
        let mut cur = DirEntry::root();
        for comp in path.split('/').filter(|c| !c.is_empty()) {
            if comp == "." || comp == ".." {
                return Err(FatError::BadPath);
            }
            if !cur.is_dir {
                return Err(FatError::NotADirectory);
            }
            let mut found = None;
            self.scan_dir(dev, cur.cluster, &mut |e| {
                if e.matches(comp) {
                    found = Some(*e);
                    false
                } else {
                    true
                }
            })?;
            cur = found.ok_or(FatError::NotFound)?;
        }
        Ok(cur)
    }

    /// Appends the contents of file `e` to `out`. Refuses files larger than `max` bytes.
    pub fn read_file(&self, dev: &mut dyn BlockDev, e: &DirEntry, max: usize, out: &mut Vec<u8>) -> Result<(), FatError> {
        if e.is_dir {
            return Err(FatError::NotAFile);
        }
        let size = e.size as usize;
        if size > max {
            return Err(FatError::TooBig);
        }
        if size == 0 {
            return Ok(());
        }
        out.try_reserve(size).map_err(|_| FatError::TooBig)?;
        let mut remaining = size;
        self.walk_chain(dev, e.cluster, |dev, lba| {
            let mut buf = [0u8; SECTOR_SIZE];
            dev.read(lba, &mut buf)?;
            let n = remaining.min(SECTOR_SIZE);
            out.extend_from_slice(&buf[..n]);
            remaining -= n;
            Ok(remaining > 0)
        })?;
        if remaining > 0 {
            return Err(FatError::BadChain); // the chain ended before the file did
        }
        Ok(())
    }
}

struct FatCache {
    lba: Option<u64>,
    data: [u8; SECTOR_SIZE],
}

impl FatCache {
    fn new() -> Self {
        FatCache { lba: None, data: [0; SECTOR_SIZE] }
    }
}

// `Entry` is large, but entries are produced and consumed one at a time on the stack.
#[allow(clippy::large_enum_variant)]
enum Feed {
    /// The 0x00 marker: no more entries in this directory.
    End,
    Entry(DirEntry),
    Skip,
}

/// Collects long-name pieces until the short entry they belong to arrives.
struct DirState {
    name: [u8; MAX_NAME],
    /// Pieces still expected (counting down); `None` when no long name is being collected.
    expect: Option<u8>,
    checksum: u8,
    pieces: u8,
}

impl DirState {
    fn new() -> Self {
        DirState { name: [0; MAX_NAME], expect: None, checksum: 0, pieces: 0 }
    }

    fn reset(&mut self) {
        self.expect = None;
    }

    fn feed(&mut self, e: &[u8]) -> Feed {
        match e[0] {
            0x00 => return Feed::End,
            0xE5 => {
                self.reset();
                return Feed::Skip;
            }
            _ => {}
        }
        let attr = e[11];
        if attr & 0x3F == 0x0F {
            self.long_piece(e);
            return Feed::Skip;
        }
        let long_ok = self.expect == Some(0) && lfn_checksum(&e[..11]) == self.checksum;
        let long_pieces = self.pieces;
        let long_name = core::mem::replace(&mut self.name, [0; MAX_NAME]);
        self.reset();
        if attr & 0x08 != 0 {
            return Feed::Skip; // volume label
        }
        let mut d = DirEntry::root();
        d.is_dir = attr & 0x10 != 0;
        d.size = le32(e, 28);
        d.cluster = le16(e, 26) | le16(e, 20) << 16;
        d.short_len = short_name(e, &mut d.short);
        if long_ok {
            let span = (usize::from(long_pieces) * 13).min(MAX_NAME);
            let n = long_name[..span].iter().position(|&c| c == 0).unwrap_or(span);
            d.long[..n].copy_from_slice(&long_name[..n]);
            d.long_len = n;
        }
        if d.short_len == 0 || (d.short[..d.short_len] == *b"." || d.short[..d.short_len] == *b"..") {
            return Feed::Skip;
        }
        Feed::Entry(d)
    }

    fn long_piece(&mut self, e: &[u8]) {
        let seq = e[0];
        let idx = seq & 0x3F;
        if seq & 0x40 != 0 {
            if idx == 0 || usize::from(idx) * 13 > MAX_NAME + 13 {
                self.reset();
                return;
            }
            self.name = [0; MAX_NAME];
            self.expect = Some(idx);
            self.pieces = idx;
            self.checksum = e[13];
        }
        match self.expect {
            Some(n) if n == idx && idx > 0 && e[13] == self.checksum => {
                let base = usize::from(idx - 1) * 13;
                for (k, &o) in [1usize, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30].iter().enumerate() {
                    let u = u16::from_le_bytes([e[o], e[o + 1]]);
                    let ch = if u == 0 || u == 0xFFFF { 0 } else if u < 0x80 { u as u8 } else { b'?' };
                    if base + k < MAX_NAME {
                        self.name[base + k] = ch;
                    }
                }
                self.expect = Some(idx - 1);
            }
            _ => self.reset(),
        }
    }
}

fn lfn_checksum(short: &[u8]) -> u8 {
    short.iter().fold(0u8, |sum, &b| (sum >> 1).wrapping_add((sum & 1) << 7).wrapping_add(b))
}

/// Writes the display form of the 8.3 name ("HELLO.TXT") into `out` and returns its length.
fn short_name(e: &[u8], out: &mut [u8; 12]) -> usize {
    let lower_base = e[12] & 0x08 != 0;
    let lower_ext = e[12] & 0x10 != 0;
    fn put(out: &mut [u8; 12], n: &mut usize, b: u8, lower: bool) {
        let b = if b == 0x05 && *n == 0 { 0xE5 } else { b };
        out[*n] = if !(0x20..0x7F).contains(&b) { b'?' } else if lower { b.to_ascii_lowercase() } else { b };
        *n += 1;
    }
    let mut n = 0;
    for &b in e[..8].iter().take_while(|&&b| b != b' ') {
        put(out, &mut n, b, lower_base);
    }
    let ext_len = e[8..11].iter().take_while(|&&b| b != b' ').count();
    if ext_len > 0 {
        out[n] = b'.';
        n += 1;
        for &b in &e[8..8 + ext_len] {
            put(out, &mut n, b, lower_ext);
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::MemDisk;

    /// Builds small FAT images in memory.
    struct Img {
        data: Vec<u8>,
        fat32: bool,
        reserved: usize,
        fat_sectors: usize,
        root_sectors: usize,
        data_start: usize,
        next: u32,
        root: Vec<u8>,
    }

    impl Img {
        fn new(fat32: bool) -> Img {
            let clusters = if fat32 { 200 } else { 4200 };
            let reserved = if fat32 { 4 } else { 1 };
            let root_sectors = if fat32 { 0 } else { 32 };
            let fat_sectors = ((clusters + 2) * if fat32 { 4 } else { 2 } + 511) / 512;
            let data_start = reserved + 2 * fat_sectors + root_sectors;
            let total = data_start + clusters;
            let mut data = vec![0u8; total * 512];
            let mut put = |o: usize, v: &[u8]| data[o..o + v.len()].copy_from_slice(v);
            put(11, &512u16.to_le_bytes());
            put(13, &[1]);
            put(14, &(reserved as u16).to_le_bytes());
            put(16, &[2]);
            put(17, &(if fat32 { 0u16 } else { 512 }).to_le_bytes());
            if total < 65536 {
                put(19, &(total as u16).to_le_bytes());
            } else {
                put(32, &(total as u32).to_le_bytes());
            }
            if fat32 {
                put(36, &(fat_sectors as u32).to_le_bytes());
                put(44, &2u32.to_le_bytes());
            } else {
                put(22, &(fat_sectors as u16).to_le_bytes());
            }
            put(510, &[0x55, 0xAA]);
            let mut img = Img { data, fat32, reserved, fat_sectors, root_sectors, data_start, next: if fat32 { 3 } else { 2 }, root: Vec::new() };
            img.set_fat(0, 0x0FFF_FFF8);
            img.set_fat(1, 0x0FFF_FFFF);
            if fat32 {
                img.set_fat(2, 0x0FFF_FFFF);
            }
            img
        }

        fn set_fat(&mut self, c: u32, v: u32) {
            for k in 0..2 {
                let o = (self.reserved + k * self.fat_sectors) * 512 + c as usize * if self.fat32 { 4 } else { 2 };
                if self.fat32 {
                    self.data[o..o + 4].copy_from_slice(&v.to_le_bytes());
                } else {
                    self.data[o..o + 2].copy_from_slice(&(v as u16).to_le_bytes());
                }
            }
        }

        fn eoc(&self) -> u32 {
            if self.fat32 { 0x0FFF_FFFF } else { 0xFFFF }
        }

        fn chain(&mut self, bytes: &[u8]) -> u32 {
            let n = bytes.len().div_ceil(512).max(1);
            let first = self.next;
            for i in 0..n {
                let c = first + i as u32;
                let v = if i + 1 < n { c + 1 } else { self.eoc() };
                self.set_fat(c, v);
                let o = (self.data_start + (c as usize - 2)) * 512;
                let chunk = &bytes[(i * 512).min(bytes.len())..((i + 1) * 512).min(bytes.len())];
                self.data[o..o + chunk.len()].copy_from_slice(chunk);
            }
            self.next += n as u32;
            first
        }

        fn entry(name: &[u8; 11], attr: u8, cluster: u32, size: u32) -> [u8; 32] {
            let mut e = [0u8; 32];
            e[..11].copy_from_slice(name);
            e[11] = attr;
            e[20..22].copy_from_slice(&((cluster >> 16) as u16).to_le_bytes());
            e[26..28].copy_from_slice(&(cluster as u16).to_le_bytes());
            e[28..32].copy_from_slice(&size.to_le_bytes());
            e
        }

        fn file(&mut self, name: &[u8; 11], bytes: &[u8]) {
            let c = if bytes.is_empty() { 0 } else { self.chain(bytes) };
            self.root.extend_from_slice(&Self::entry(name, 0x20, c, bytes.len() as u32));
        }

        fn raw_entry(&mut self, e: [u8; 32]) {
            self.root.extend_from_slice(&e);
        }

        fn finish(mut self) -> Vec<u8> {
            if self.fat32 {
                let o = (self.data_start) * 512; // cluster 2 is the root directory
                let r = core::mem::take(&mut self.root);
                self.data[o..o + r.len()].copy_from_slice(&r);
            } else {
                let o = (self.reserved + 2 * self.fat_sectors) * 512;
                assert!(self.root.len() <= self.root_sectors * 512);
                let r = core::mem::take(&mut self.root);
                self.data[o..o + r.len()].copy_from_slice(&r);
            }
            self.data
        }
    }

    fn lfn(name: &str, short: &[u8; 11]) -> Vec<[u8; 32]> {
        let chk = lfn_checksum(short);
        let mut units: Vec<u16> = name.chars().map(|c| c as u16).collect();
        units.push(0);
        while units.len() % 13 != 0 {
            units.push(0xFFFF);
        }
        let parts = units.len() / 13;
        (1..=parts)
            .rev()
            .map(|n| {
                let mut e = [0u8; 32];
                e[0] = n as u8 | if n == parts { 0x40 } else { 0 };
                e[11] = 0x0F;
                e[13] = chk;
                for (k, &o) in [1usize, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30].iter().enumerate() {
                    e[o..o + 2].copy_from_slice(&units[(n - 1) * 13 + k].to_le_bytes());
                }
                e
            })
            .collect()
    }

    fn read_all(v: &Volume, d: &mut dyn BlockDev, path: &str) -> Result<Vec<u8>, FatError> {
        let e = v.find(d, path)?;
        let mut out = Vec::new();
        v.read_file(d, &e, 1 << 20, &mut out)?;
        Ok(out)
    }

    fn names(v: &Volume, d: &mut dyn BlockDev, path: &str) -> Vec<String> {
        let mut out = Vec::new();
        v.list(d, path, |e| out.push(e.name().to_string())).unwrap();
        out
    }

    fn sample(fat32: bool) -> Vec<u8> {
        let mut img = Img::new(fat32);
        img.file(b"HELLO   TXT", b"hello from the disk\n");
        let big: Vec<u8> = (0..3000u32).map(|i| (i % 251) as u8).collect();
        img.file(b"BIG     BIN", &big);
        img.file(b"EMPTY   TXT", b"");
        // A sub-directory with one file in it.
        let note = img.chain(b"note\n");
        let mut sub = Vec::new();
        sub.extend_from_slice(&Img::entry(b"NOTE    TXT", 0x20, note, 5));
        let sub_c = img.chain(&sub);
        img.raw_entry(Img::entry(b"DOCS       ", 0x10, sub_c, 0));
        // A long name with its short alias.
        let alias = b"LONGFI~1TXT";
        for e in lfn("Long File Name.txt", alias) {
            img.raw_entry(e);
        }
        let c = img.chain(b"long\n");
        img.raw_entry(Img::entry(alias, 0x20, c, 5));
        img.finish()
    }

    #[test]
    fn lists_reads_and_finds_on_both_types() {
        for fat32 in [false, true] {
            let img = sample(fat32);
            let mut d = MemDisk(&img);
            let v = Volume::open(&mut d, 0).unwrap();
            assert_eq!(v.is_fat32(), fat32);
            assert_eq!(names(&v, &mut d, ""), ["HELLO.TXT", "BIG.BIN", "EMPTY.TXT", "DOCS", "Long File Name.txt"]);
            assert_eq!(read_all(&v, &mut d, "hello.txt").unwrap(), b"hello from the disk\n");
            assert_eq!(read_all(&v, &mut d, "/Hello.Txt").unwrap(), b"hello from the disk\n");
            let big = read_all(&v, &mut d, "BIG.BIN").unwrap();
            assert_eq!(big.len(), 3000);
            assert!(big.iter().enumerate().all(|(i, &b)| b == (i % 251) as u8));
            assert_eq!(read_all(&v, &mut d, "EMPTY.TXT").unwrap(), b"");
            assert_eq!(read_all(&v, &mut d, "docs/note.txt").unwrap(), b"note\n");
            assert_eq!(names(&v, &mut d, "DOCS"), ["NOTE.TXT"]);
            // The long name and the short alias both find the file.
            assert_eq!(read_all(&v, &mut d, "long file name.TXT").unwrap(), b"long\n");
            assert_eq!(read_all(&v, &mut d, "LONGFI~1.TXT").unwrap(), b"long\n");
        }
    }

    #[test]
    fn path_errors() {
        let img = sample(false);
        let mut d = MemDisk(&img);
        let v = Volume::open(&mut d, 0).unwrap();
        assert_eq!(v.find(&mut d, "nosuch").err(), Some(FatError::NotFound));
        assert_eq!(v.find(&mut d, "hello.txt/x").err(), Some(FatError::NotADirectory));
        assert_eq!(v.find(&mut d, "docs/..").err(), Some(FatError::BadPath));
        assert_eq!(v.find(&mut d, ".").err(), Some(FatError::BadPath));
        assert_eq!(v.list(&mut d, "hello.txt", |_| {}).err(), Some(FatError::NotADirectory));
        let docs = v.find(&mut d, "docs").unwrap();
        let mut out = Vec::new();
        assert_eq!(v.read_file(&mut d, &docs, 100, &mut out).err(), Some(FatError::NotAFile));
        let big = v.find(&mut d, "big.bin").unwrap();
        assert_eq!(v.read_file(&mut d, &big, 2999, &mut out).err(), Some(FatError::TooBig));
        assert!(out.is_empty());
    }

    #[test]
    fn deleted_entries_labels_and_bad_long_names() {
        let mut img = Img::new(false);
        let mut gone = Img::entry(b"GONE    TXT", 0x20, 0, 0);
        gone[0] = 0xE5;
        img.raw_entry(gone);
        img.raw_entry(Img::entry(b"MYDISK     ", 0x08, 0, 0));
        // A long name whose checksum does not match: the short name is used.
        for e in lfn("Wrong Checksum.txt", b"OTHERNAMTXT") {
            img.raw_entry(e);
        }
        img.file(b"REAL    TXT", b"x");
        // An orphaned long-name piece with nothing after it.
        for e in lfn("orphan", b"ORPHAN  TXT").into_iter().take(1) {
            img.raw_entry(e);
        }
        img.file(b"LAST    TXT", b"y");
        let data = img.finish();
        let mut d = MemDisk(&data);
        let v = Volume::open(&mut d, 0).unwrap();
        assert_eq!(names(&v, &mut d, ""), ["REAL.TXT", "LAST.TXT"]);
    }

    #[test]
    fn lowercase_flags_and_unusual_bytes() {
        let mut img = Img::new(false);
        let mut e = Img::entry(b"MIXED   TXT", 0x20, 0, 0);
        e[12] = 0x08 | 0x10;
        img.raw_entry(e);
        img.raw_entry(Img::entry(&[0x05, b'B', b'A', b'D', b' ', b' ', b' ', b' ', b'X', b' ', b' '], 0x20, 0, 0));
        img.raw_entry(Img::entry(&[0x80, 0x01, b'Z', b' ', b' ', b' ', b' ', b' ', b' ', b' ', b' '], 0x20, 0, 0));
        let data = img.finish();
        let mut d = MemDisk(&data);
        let v = Volume::open(&mut d, 0).unwrap();
        let n = names(&v, &mut d, "");
        assert_eq!(n[0], "mixed.txt");
        assert_eq!(n[1].len(), 6);
        assert_eq!(n[2], "??Z");
    }

    // ---- hostile images

    fn one_file(fat32: bool, size: u32, cluster: u32, patch: impl FnOnce(&mut Img)) -> Vec<u8> {
        let mut img = Img::new(fat32);
        img.raw_entry(Img::entry(b"F       BIN", 0x20, cluster, size));
        patch(&mut img);
        img.finish()
    }

    fn read_f(data: &[u8]) -> Result<Vec<u8>, FatError> {
        let mut d = MemDisk(data);
        let v = Volume::open(&mut d, 0)?;
        read_all(&v, &mut d, "F.BIN")
    }

    #[test]
    fn chain_loop_ends_in_an_error() {
        for fat32 in [false, true] {
            let first = if fat32 { 3 } else { 2 };
            let data = one_file(fat32, 5000, first, |i| {
                i.set_fat(first, first + 1);
                i.set_fat(first + 1, first); // A -> B -> A
            });
            assert_eq!(read_f(&data).err(), Some(FatError::BadChain));
            let selfloop = one_file(fat32, 5000, first, |i| i.set_fat(first, first));
            assert_eq!(read_f(&selfloop).err(), Some(FatError::BadChain));
        }
    }

    #[test]
    fn bad_links_are_errors() {
        let first = 2;
        let past_end = one_file(false, 2000, first, |i| i.set_fat(first, 60_000)); // beyond the volume
        assert_eq!(read_f(&past_end).err(), Some(FatError::BadChain));
        let to_free = one_file(false, 2000, first, |i| i.set_fat(first, 0)); // links to "free"
        assert_eq!(read_f(&to_free).err(), Some(FatError::BadChain));
        let to_bad = one_file(false, 2000, first, |i| i.set_fat(first, 0xFFF7));
        assert_eq!(read_f(&to_bad).err(), Some(FatError::BadChain));
        let to_reserved = one_file(false, 2000, first, |i| i.set_fat(first, 1));
        assert_eq!(read_f(&to_reserved).err(), Some(FatError::BadChain));
    }

    #[test]
    fn size_larger_than_the_chain_and_cluster_zero() {
        // One cluster of data but the entry claims 2000 bytes.
        let short_chain = one_file(false, 2000, 2, |i| {
            i.set_fat(2, 0xFFFF);
        });
        assert_eq!(read_f(&short_chain).err(), Some(FatError::BadChain));
        // Non-empty file that points at cluster 0.
        assert_eq!(read_f(&one_file(false, 10, 0, |_| {})).err(), Some(FatError::BadChain));
        // Starting cluster beyond the volume.
        assert_eq!(read_f(&one_file(false, 10, 70_000, |_| {})).err(), Some(FatError::BadChain));
        // A chain longer than the file is fine: only `size` bytes come back.
        let long_chain = one_file(false, 100, 2, |i| {
            i.chain(&[7u8; 1500]);
        });
        assert_eq!(read_f(&long_chain).unwrap().len(), 100);
    }

    #[test]
    fn huge_declared_size_is_refused_without_allocating() {
        let data = one_file(false, u32::MAX, 2, |_| {});
        let mut d = MemDisk(&data);
        let v = Volume::open(&mut d, 0).unwrap();
        let e = v.find(&mut d, "F.BIN").unwrap();
        let mut out = Vec::new();
        assert_eq!(v.read_file(&mut d, &e, 1 << 20, &mut out), Err(FatError::TooBig));
        assert!(out.capacity() < 1 << 20);
    }

    #[test]
    fn directory_loop_ends_in_an_error() {
        let mut img = Img::new(true);
        // A directory whose cluster chain loops and never reaches the 0x00 end marker.
        let c = img.chain(&[0xAAu8; 512]); // entries that are not 0x00 or 0xE5 and not long-name pieces
        img.set_fat(c, c);
        img.raw_entry(Img::entry(b"LOOP       ", 0x10, c, 0));
        let data = img.finish();
        let mut d = MemDisk(&data);
        let v = Volume::open(&mut d, 0).unwrap();
        assert_eq!(v.list(&mut d, "LOOP", |_| {}).err(), Some(FatError::BadChain));
    }

    #[test]
    fn broken_boot_sectors() {
        let good = sample(false);
        let mut cases: Vec<(&str, Box<dyn Fn(&mut Vec<u8>)>, FatError)> = Vec::new();
        cases.push(("signature", Box::new(|d| d[510] = 0), FatError::BadBpb));
        cases.push(("sector size 4096", Box::new(|d| d[11..13].copy_from_slice(&4096u16.to_le_bytes())), FatError::Unsupported));
        cases.push(("sector size 777", Box::new(|d| d[11..13].copy_from_slice(&777u16.to_le_bytes())), FatError::BadBpb));
        cases.push(("spc 0", Box::new(|d| d[13] = 0), FatError::BadBpb));
        cases.push(("spc 3", Box::new(|d| d[13] = 3), FatError::BadBpb));
        cases.push(("reserved 0", Box::new(|d| d[14..16].copy_from_slice(&0u16.to_le_bytes())), FatError::BadBpb));
        cases.push(("no FATs", Box::new(|d| d[16] = 0), FatError::BadBpb));
        cases.push(("zero fat size", Box::new(|d| d[22..24].copy_from_slice(&0u16.to_le_bytes())), FatError::BadBpb));
        cases.push(("total beyond the disk", Box::new(|d| d[19..21].copy_from_slice(&60_000u16.to_le_bytes())), FatError::BadBpb));
        cases.push(("zero total", Box::new(|d| {
            d[19..21].copy_from_slice(&0u16.to_le_bytes());
            d[32..36].copy_from_slice(&0u32.to_le_bytes());
        }), FatError::BadBpb));
        cases.push(("no root entries", Box::new(|d| d[17..19].copy_from_slice(&0u16.to_le_bytes())), FatError::BadBpb));
        cases.push(("FAT too small for the clusters", Box::new(|d| d[22..24].copy_from_slice(&1u16.to_le_bytes())), FatError::BadBpb));
        for (name, patch, want) in cases {
            let mut d = good.clone();
            patch(&mut d);
            let mut disk = MemDisk(&d);
            assert_eq!(Volume::open(&mut disk, 0).err(), Some(want), "{name}");
        }
        // FAT12-sized volume (fewer than 4085 clusters) is refused, not misread.
        let mut small = good.clone();
        small[19..21].copy_from_slice(&2000u16.to_le_bytes());
        let mut disk = MemDisk(&small);
        assert_eq!(Volume::open(&mut disk, 0).err(), Some(FatError::Unsupported));
        // FAT32 root cluster out of range.
        let mut f32 = sample(true);
        f32[44..48].copy_from_slice(&9999u32.to_le_bytes());
        let mut disk = MemDisk(&f32);
        assert_eq!(Volume::open(&mut disk, 0).err(), Some(FatError::BadBpb));
    }

    #[test]
    fn truncated_and_tiny_disks() {
        let good = sample(false);
        for len in [0usize, 100, 511, 512, 1024, 100 * 512] {
            let mut d = MemDisk(&good[..len]);
            assert!(Volume::open(&mut d, 0).is_err(), "{len}");
        }
        // Opened at an offset that leaves too little room.
        let mut d = MemDisk(&good);
        assert!(Volume::open(&mut d, 4000).is_err());
        assert!(Volume::open(&mut d, u64::MAX).is_err());
    }

    #[test]
    fn partition_offset_works() {
        let img = sample(false);
        let mut padded = vec![0u8; 63 * 512];
        padded.extend_from_slice(&img);
        let mut d = MemDisk(&padded);
        let v = Volume::open(&mut d, 63).unwrap();
        assert_eq!(read_all(&v, &mut d, "hello.txt").unwrap(), b"hello from the disk\n");
    }

    /// Puts `vol` at sector `at` of a disk with a partition table written by `table`.
    fn with_table(vol: &[u8], at: usize, table: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
        let mut d = vec![0u8; at * 512];
        d.extend_from_slice(vol);
        d.resize(d.len() + 512 * 40, 0);
        table(&mut d);
        d
    }

    #[test]
    fn open_any_finds_volumes_on_the_whole_disk_mbr_and_gpt() {
        let vol = sample(false);
        // Whole disk.
        let mut d = MemDisk(&vol);
        assert!(Volume::open_any(&mut d).is_ok());
        // MBR: entry 2 is a FAT16 partition at sector 63; entry 0 is junk.
        let mbr = with_table(&vol, 63, |d| {
            d[446 + 4] = 0x83;
            d[446 + 8..446 + 12].copy_from_slice(&5u32.to_le_bytes());
            d[446 + 32 + 4] = 0x06;
            d[446 + 32 + 8..446 + 32 + 12].copy_from_slice(&63u32.to_le_bytes());
            d[510] = 0x55;
            d[511] = 0xAA;
        });
        let mut d = MemDisk(&mbr);
        let v = Volume::open_any(&mut d).unwrap();
        assert_eq!(read_all(&v, &mut d, "hello.txt").unwrap(), b"hello from the disk
");
        // GPT: protective MBR, header at sector 1, entries at sector 2, the partition at sector 2048.
        let gpt = with_table(&vol, 2048, |d| {
            d[446 + 4] = 0xEE;
            d[510] = 0x55;
            d[511] = 0xAA;
            d[512..520].copy_from_slice(b"EFI PART");
            d[512 + 72..512 + 80].copy_from_slice(&2u64.to_le_bytes());
            d[512 + 80..512 + 84].copy_from_slice(&4u32.to_le_bytes());
            d[512 + 84..512 + 88].copy_from_slice(&128u32.to_le_bytes());
            // entry 0 unused, entry 1 junk start, entry 2 the real one
            d[1024 + 128 + 1] = 1;
            d[1024 + 128 + 32..1024 + 128 + 40].copy_from_slice(&7u64.to_le_bytes());
            d[1024 + 256 + 1] = 1;
            d[1024 + 256 + 32..1024 + 256 + 40].copy_from_slice(&2048u64.to_le_bytes());
        });
        let mut d = MemDisk(&gpt);
        let v = Volume::open_any(&mut d).unwrap();
        assert!(!v.is_fat32());
        assert_eq!(read_all(&v, &mut d, "docs/note.txt").unwrap(), b"note
");
    }

    #[test]
    fn open_any_on_hostile_tables_gives_the_original_error() {
        let blank = vec![0u8; 512 * 64];
        let mut d = MemDisk(&blank);
        assert_eq!(Volume::open_any(&mut d).err(), Some(FatError::BadBpb));
        // A GPT that claims a huge entry count, a bad entry size and an entry start beyond the disk.
        let mut g = blank.clone();
        g[446 + 4] = 0xEE;
        g[510] = 0x55;
        g[511] = 0xAA;
        g[512..520].copy_from_slice(b"EFI PART");
        g[512 + 72..512 + 80].copy_from_slice(&u64::MAX.to_le_bytes());
        g[512 + 80..512 + 84].copy_from_slice(&u32::MAX.to_le_bytes());
        g[512 + 84..512 + 88].copy_from_slice(&128u32.to_le_bytes());
        let mut d = MemDisk(&g);
        assert!(Volume::open_any(&mut d).is_err());
        g[512 + 84..512 + 88].copy_from_slice(&7u32.to_le_bytes()); // entry size not allowed
        let mut d = MemDisk(&g);
        assert!(Volume::open_any(&mut d).is_err());
        // MBR entries pointing outside the disk.
        let mut m = blank.clone();
        m[446 + 4] = 0x0C;
        m[446 + 8..446 + 12].copy_from_slice(&u32::MAX.to_le_bytes());
        m[510] = 0x55;
        m[511] = 0xAA;
        let mut d = MemDisk(&m);
        assert!(Volume::open_any(&mut d).is_err());
    }

    #[test]
    fn random_garbage_never_panics_or_hangs() {
        // A tiny xorshift so the test needs no dependency.
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let base = sample(false);
        for round in 0..300 {
            let mut d = base.clone();
            // Corrupt the boot sector, the FAT and the root directory with random bytes.
            for _ in 0..(1 + round % 40) {
                let region = next() % 3;
                let (lo, len) = match region {
                    0 => (0, 64),
                    1 => (512, 2 * 512),
                    _ => ((1 + 2 * 17) * 512, 512),
                };
                let at = lo + (next() as usize % len);
                d[at] = next() as u8;
            }
            let mut disk = MemDisk(&d);
            if let Ok(v) = Volume::open(&mut disk, 0) {
                let _ = v.list(&mut disk, "", |_| {});
                for p in ["HELLO.TXT", "BIG.BIN", "DOCS/NOTE.TXT", "x"] {
                    if let Ok(e) = v.find(&mut disk, p) {
                        let mut out = Vec::new();
                        let _ = v.read_file(&mut disk, &e, 1 << 20, &mut out);
                    }
                }
            }
        }
    }
}
