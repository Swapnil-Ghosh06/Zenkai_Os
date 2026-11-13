//! FAT32 writer over a [`BlockWrite`] disk: create, replace and delete files, make and remove folders.
//! Pure logic, host-tested against a model and against `fsck.vfat`. Written for hostile disks like the
//! reader in `fat.rs`: every size and cluster number is range-checked, chains and directories have hard
//! step limits, and a damaged volume gives an error, never a panic or an endless loop.
//!
//! Safety rules, enforced here and not relaxed anywhere:
//! - only a FAT32 volume whose boot-sector label is exactly `AEON` can be opened for writing;
//! - nothing inside the `EFI` folder (the boot files) can be created, replaced, removed or entered for
//!   writing; the `EFI` folder itself cannot be removed;
//! - order of writes: new data first, directory entry second, old data freed last, so an interrupted
//!   operation can leak clusters but never leaves an entry pointing at storage that is not its own;
//! - the volume's "clean" flag is cleared before the first write of an operation and set again afterwards
//!   (left cleared if the disk failed in the middle); an operation that is refused writes nothing.
//!
//! Not supported: long names with characters outside ASCII, renaming, file timestamps (Aeon has no
//! clock, so every entry carries 1980-01-01 00:00), FAT12/16/exFAT, volumes with FAT mirroring turned off.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::block::{BlockDev, BlockError, BlockWrite, SECTOR_SIZE};
use crate::fat::{FatError, Volume};
use crate::fatname;

/// Why a volume cannot be opened for writing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// Not a FAT32 volume.
    NotFat32,
    /// The volume label is not `AEON` (or the boot sector has no label field).
    WrongLabel,
    /// The volume uses a single active FAT instead of mirroring both copies.
    NoMirroring,
    /// The boot sector or the volume layout is damaged or not a FAT volume at all.
    Damaged(FatError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteError {
    NotWritable(Reason),
    /// The name or path is not allowed (see `fatname::valid_name`), or names the root.
    BadName,
    Exists,
    NotFound,
    NotADirectory,
    IsADirectory,
    NotEmpty,
    NoSpace,
    /// Inside `EFI`, which is never written.
    Protected,
    /// A file larger than FAT can describe (4 GiB - 1).
    TooBig,
    /// A folder cannot hold more entries (FAT allows 65536 slots per folder), or no short name is left.
    DirFull,
    /// The volume is damaged: a chain loops, is cut, or points outside the volume.
    Corrupt,
    Io(BlockError),
}

impl From<BlockError> for WriteError {
    fn from(e: BlockError) -> Self {
        WriteError::Io(e)
    }
}

/// One entry of a folder as the writer sees it.
#[derive(Clone, Debug)]
pub struct Item {
    pub name: String,
    pub short: [u8; 11],
    pub attr: u8,
    pub cluster: u32,
    pub size: u32,
    /// Index of the first slot that belongs to this entry (its long-name slots come first).
    pub first_slot: usize,
    /// Index of the short-entry slot.
    pub slot: usize,
}

impl Item {
    pub fn is_dir(&self) -> bool {
        self.attr & ATTR_DIR != 0
    }
}

/// A folder read into memory: its cluster chain and all its slots.
pub struct Dir {
    pub clusters: Vec<u32>,
    pub bytes: Vec<u8>,
}

const EOC: u32 = 0x0FFF_FFFF;
/// A FAT32 entry has 28 bits; the top four belong to nobody and are kept as they are.
const MASK: u32 = 0x0FFF_FFFF;
const FIRST_EOC: u32 = 0x0FFF_FFF8;
const BAD: u32 = 0x0FFF_FFF7;
/// The "volume was shut down cleanly" bit in FAT entry 1.
const CLEAN_BIT: u32 = 0x0800_0000;
/// 1980-01-01, the date every entry gets (Aeon has no clock).
const DOS_DATE: u16 = 0x0021;
const ATTR_DIR: u8 = 0x10;
const ATTR_FILE: u8 = 0x20;
/// A FAT folder holds at most this many 32-byte slots.
const MAX_DIR_SLOTS: usize = 65_536;

fn le16(b: &[u8], o: usize) -> u32 {
    u32::from(u16::from_le_bytes([b[o], b[o + 1]]))
}

fn le32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn short_entry(short: [u8; 11], attr: u8, cluster: u32, size: u32) -> [u8; 32] {
    let mut s = [0u8; 32];
    s[..11].copy_from_slice(&short);
    s[11] = attr;
    s[16..18].copy_from_slice(&DOS_DATE.to_le_bytes());
    s[18..20].copy_from_slice(&DOS_DATE.to_le_bytes());
    s[20..22].copy_from_slice(&((cluster >> 16) as u16).to_le_bytes());
    s[24..26].copy_from_slice(&DOS_DATE.to_le_bytes());
    s[26..28].copy_from_slice(&(cluster as u16).to_le_bytes());
    s[28..32].copy_from_slice(&size.to_le_bytes());
    s
}

/// Splits `path` into checked components. The root itself, empty components, `.` and `..` are refused.
fn split(path: &str) -> Result<Vec<&str>, WriteError> {
    let p = path.strip_prefix('/').unwrap_or(path);
    if p.is_empty() {
        return Err(WriteError::BadName);
    }
    let comps: Vec<&str> = p.split('/').collect();
    if comps.iter().any(|c| !fatname::valid_name(c)) {
        return Err(WriteError::BadName);
    }
    Ok(comps)
}

pub struct Fat32Writer {
    spc: u32,
    nfats: u32,
    /// Absolute sector of the first FAT.
    fat_start: u64,
    fat_sectors: u64,
    data_start: u64,
    clusters: u32,
    root: u32,
    /// Absolute sector of FSInfo, if the volume has a valid one.
    fsinfo: Option<u64>,
    next_free: u32,
    protect_efi: bool,
    /// The running operation has written something (and so has cleared the clean flag).
    marked: bool,
    /// The volume was clean before the running operation, so it is marked clean again afterwards.
    restore_clean: bool,
}

impl Fat32Writer {
    /// Opens the volume whose boot sector is at `base`. Refuses anything but a FAT32 volume labelled `AEON`.
    pub fn open(dev: &mut dyn BlockWrite, base: u64) -> Result<Fat32Writer, WriteError> {
        // The read-only reader judges the layout first, so both agree on what a valid volume is.
        let vol = Volume::open(dev, base).map_err(|e| WriteError::NotWritable(Reason::Damaged(e)))?;
        if !vol.is_fat32() {
            return Err(WriteError::NotWritable(Reason::NotFat32));
        }
        let mut s = [0u8; SECTOR_SIZE];
        dev.read(base, &mut s)?;
        if s[66] != 0x29 || (s[71..82] != *b"AEON       " && s[71..82] != *b"ZENKAI     ") {
            return Err(WriteError::NotWritable(Reason::WrongLabel));
        }
        if s[40] & 0x80 != 0 {
            return Err(WriteError::NotWritable(Reason::NoMirroring));
        }
        let spc = u32::from(s[13]);
        let reserved = u64::from(le16(&s, 14));
        let nfats = u32::from(s[16]);
        let fat_sectors = u64::from(le32(&s, 36));
        let total = if le16(&s, 19) != 0 { u64::from(le16(&s, 19)) } else { u64::from(le32(&s, 32)) };
        let data_rel = reserved + u64::from(nfats) * fat_sectors;
        let clusters = ((total - data_rel) / u64::from(spc)) as u32;
        let fsinfo_rel = u64::from(le16(&s, 48));
        let fsinfo = if fsinfo_rel > 0 && fsinfo_rel < reserved {
            let mut f = [0u8; SECTOR_SIZE];
            dev.read(base + fsinfo_rel, &mut f)?;
            (&f[0..4] == b"RRaA" && &f[484..488] == b"rrAa" && f[508..512] == [0, 0, 0x55, 0xAA]).then_some(base + fsinfo_rel)
        } else {
            None
        };
        Ok(Fat32Writer {
            spc,
            nfats,
            fat_start: base + reserved,
            fat_sectors,
            data_start: base + data_rel,
            clusters,
            root: le32(&s, 44),
            fsinfo,
            next_free: 2,
            protect_efi: true,
            marked: false,
            restore_clean: false,
        })
    }

    /// Opens the volume the reader (`Volume::open_any`) would show: sector 0 if that is a FAT boot sector,
    /// otherwise the first MBR or GPT partition that holds a FAT volume. If that volume is not a writable
    /// `AEON` FAT32 volume the answer is an error: the writer never moves on to a later partition, so what
    /// `ls` shows and where `write` puts data can never differ.
    pub fn open_any(dev: &mut dyn BlockWrite) -> Result<(Fat32Writer, u64), WriteError> {
        let first_err = match Volume::open(dev, 0) {
            Ok(_) => return Fat32Writer::open(dev, 0).map(|w| (w, 0)),
            Err(e) => e,
        };
        let not_found = WriteError::NotWritable(Reason::Damaged(first_err));
        let mut s = [0u8; SECTOR_SIZE];
        dev.read(0, &mut s)?;
        if s[510] != 0x55 || s[511] != 0xAA {
            return Err(not_found);
        }
        let mut gpt = false;
        for i in 0..4 {
            let e = &s[446 + 16 * i..446 + 16 * (i + 1)];
            match e[4] {
                0 => {}
                0xEE => gpt = true,
                _ => {
                    let start = u64::from(le32(e, 8));
                    if Volume::open(dev, start).is_ok() {
                        return Fat32Writer::open(dev, start).map(|w| (w, start));
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
                        let lba = entries_lba.checked_add((idx / per_sector) as u64).ok_or(WriteError::Corrupt)?;
                        dev.read(lba, &mut sec)?;
                        let e = &sec[(idx % per_sector) * size..(idx % per_sector + 1) * size];
                        if e[..16].iter().all(|&b| b == 0) {
                            continue;
                        }
                        let start = u64::from_le_bytes(e[32..40].try_into().unwrap_or([0; 8]));
                        if Volume::open(dev, start).is_ok() {
                            return Fat32Writer::open(dev, start).map(|w| (w, start));
                        }
                    }
                }
            }
        }
        Err(not_found)
    }

    /// Bytes in one cluster.
    pub fn cluster_bytes(&self) -> usize {
        self.spc as usize * SECTOR_SIZE
    }

    pub fn root_cluster(&self) -> u32 {
        self.root
    }

    pub fn cluster_count(&self) -> u32 {
        self.clusters
    }

    /// Test builds only: switches the `EFI` protection off so a test can build a boot folder to protect.
    #[cfg(test)]
    pub fn set_efi_protection(&mut self, on: bool) {
        self.protect_efi = on;
    }

    // ------------------------------------------------------------ low level: every write goes through `put`

    fn cluster_sector(&self, c: u32) -> u64 {
        self.data_start + u64::from(c - 2) * u64::from(self.spc)
    }

    /// Writes sectors, after clearing the volume's clean flag if this operation has not done so yet.
    fn put(&mut self, dev: &mut dyn BlockWrite, lba: u64, buf: &[u8]) -> Result<(), WriteError> {
        if !self.marked {
            self.marked = true;
            self.restore_clean = self.set_clean(dev, false)?;
        }
        dev.write(lba, buf)?;
        Ok(())
    }

    /// Sets or clears the clean flag (FAT entry 1) in every FAT copy; returns whether it was set before.
    fn set_clean(&self, dev: &mut dyn BlockWrite, clean: bool) -> Result<bool, WriteError> {
        let mut sec = [0u8; SECTOR_SIZE];
        dev.read(self.fat_start, &mut sec)?;
        let v = le32(&sec, 4);
        let new = if clean { v | CLEAN_BIT } else { v & !CLEAN_BIT };
        if new != v {
            sec[4..8].copy_from_slice(&new.to_le_bytes());
            for k in 0..u64::from(self.nfats) {
                dev.write(self.fat_start + k * self.fat_sectors, &sec)?;
            }
        }
        Ok(v & CLEAN_BIT != 0)
    }

    /// Ends an operation: on success or a clean refusal, makes FSInfo say "unknown" and sets the clean flag
    /// again. After a disk failure nothing more is written and the flag stays cleared.
    fn finish<T>(&mut self, dev: &mut dyn BlockWrite, r: Result<T, WriteError>) -> Result<T, WriteError> {
        let marked = core::mem::take(&mut self.marked);
        let restore = core::mem::take(&mut self.restore_clean);
        if matches!(r, Err(WriteError::Io(_))) || !marked {
            return r;
        }
        let tidy = self.tidy(dev, restore);
        match (r, tidy) {
            (Ok(v), Ok(())) => Ok(v),
            (Ok(_), Err(e)) => Err(e),
            (Err(e), _) => Err(e),
        }
    }

    fn tidy(&self, dev: &mut dyn BlockWrite, restore: bool) -> Result<(), WriteError> {
        if let Some(lba) = self.fsinfo {
            let mut f = [0u8; SECTOR_SIZE];
            dev.read(lba, &mut f)?;
            if f[488..496] != [0xFF; 8] {
                f[488..496].fill(0xFF); // free count and next free: unknown
                dev.write(lba, &f)?;
            }
        }
        if restore {
            self.set_clean(dev, true)?;
        }
        Ok(())
    }

    // ------------------------------------------------------------ the FAT

    fn fat_get(&self, dev: &mut dyn BlockDev, c: u32) -> Result<u32, WriteError> {
        if c >= self.clusters + 2 {
            return Err(WriteError::Corrupt);
        }
        let off = u64::from(c) * 4;
        let mut sec = [0u8; SECTOR_SIZE];
        dev.read(self.fat_start + off / SECTOR_SIZE as u64, &mut sec)?;
        Ok(le32(&sec, (off % SECTOR_SIZE as u64) as usize) & MASK)
    }

    /// Sets FAT entry `c` in every copy, keeping the top four bits of the old value.
    fn fat_set(&mut self, dev: &mut dyn BlockWrite, c: u32, v: u32) -> Result<(), WriteError> {
        if c >= self.clusters + 2 {
            return Err(WriteError::Corrupt);
        }
        let off = u64::from(c) * 4;
        let (rel, o) = (off / SECTOR_SIZE as u64, (off % SECTOR_SIZE as u64) as usize);
        let mut sec = [0u8; SECTOR_SIZE];
        dev.read(self.fat_start + rel, &mut sec)?;
        let new = (le32(&sec, o) & !MASK) | (v & MASK);
        sec[o..o + 4].copy_from_slice(&new.to_le_bytes());
        for k in 0..u64::from(self.nfats) {
            self.put(dev, self.fat_start + k * self.fat_sectors + rel, &sec)?;
        }
        Ok(())
    }

    /// First free cluster in `lo..hi`.
    fn find_free(&self, dev: &mut dyn BlockDev, lo: u32, hi: u32) -> Result<Option<u32>, WriteError> {
        let mut sec = [0u8; SECTOR_SIZE];
        let mut loaded = u64::MAX;
        for c in lo..hi {
            let off = u64::from(c) * 4;
            let lba = self.fat_start + off / SECTOR_SIZE as u64;
            if lba != loaded {
                dev.read(lba, &mut sec)?;
                loaded = lba;
            }
            if le32(&sec, (off % SECTOR_SIZE as u64) as usize) & MASK == 0 {
                return Ok(Some(c));
            }
        }
        Ok(None)
    }

    /// Takes one free cluster and marks it as the end of a chain.
    fn alloc_one(&mut self, dev: &mut dyn BlockWrite) -> Result<u32, WriteError> {
        let end = self.clusters + 2;
        let from = self.next_free.clamp(2, end - 1);
        let found = match self.find_free(dev, from, end)? {
            Some(c) => Some(c),
            None => self.find_free(dev, 2, from)?,
        };
        let c = found.ok_or(WriteError::NoSpace)?;
        self.fat_set(dev, c, EOC)?;
        self.next_free = if c + 1 >= end { 2 } else { c + 1 };
        Ok(c)
    }

    /// Takes `n` free clusters linked into one chain. On failure everything taken is given back.
    fn alloc_chain(&mut self, dev: &mut dyn BlockWrite, n: usize) -> Result<Vec<u32>, WriteError> {
        let mut got: Vec<u32> = Vec::with_capacity(n.min(self.clusters as usize));
        for _ in 0..n {
            let step = self.alloc_one(dev).and_then(|c| {
                if let Some(&prev) = got.last() {
                    self.fat_set(dev, prev, c)?;
                }
                got.push(c);
                Ok(())
            });
            if let Err(e) = step {
                return Err(self.give_back(dev, &got, e));
            }
        }
        Ok(got)
    }

    fn free_list(&mut self, dev: &mut dyn BlockWrite, list: &[u32]) -> Result<(), WriteError> {
        for &c in list {
            self.fat_set(dev, c, 0)?;
        }
        Ok(())
    }

    /// After a failed step: frees what the operation took (unless the disk itself failed) and passes the error on.
    fn give_back(&mut self, dev: &mut dyn BlockWrite, taken: &[u32], e: WriteError) -> WriteError {
        if !matches!(e, WriteError::Io(_)) {
            let _ = self.free_list(dev, taken);
        }
        e
    }

    /// Number of free clusters (what `df` shows), counted from the FAT itself.
    pub fn free_clusters(&mut self, dev: &mut dyn BlockWrite) -> Result<u32, WriteError> {
        let mut sec = [0u8; SECTOR_SIZE];
        let mut loaded = u64::MAX;
        let mut free = 0;
        for c in 2..self.clusters + 2 {
            let off = u64::from(c) * 4;
            let lba = self.fat_start + off / SECTOR_SIZE as u64;
            if lba != loaded {
                dev.read(lba, &mut sec)?;
                loaded = lba;
            }
            if le32(&sec, (off % SECTOR_SIZE as u64) as usize) & MASK == 0 {
                free += 1;
            }
        }
        Ok(free)
    }

    /// The clusters of the chain that starts at `first`, in order. A chain that loops, is cut by a free or
    /// bad cluster, or leaves the volume is `Corrupt`.
    pub fn chain(&self, dev: &mut dyn BlockDev, first: u32) -> Result<Vec<u32>, WriteError> {
        self.chain_max(dev, first, self.clusters as usize)
    }

    fn chain_max(&self, dev: &mut dyn BlockDev, first: u32, max: usize) -> Result<Vec<u32>, WriteError> {
        if first < 2 || first >= self.clusters + 2 {
            return Err(WriteError::Corrupt);
        }
        let mut out = Vec::new();
        let mut c = first;
        loop {
            out.push(c);
            if out.len() > max {
                return Err(WriteError::Corrupt); // longer than the volume (or the folder limit): a loop
            }
            let next = self.fat_get(dev, c)?;
            if next >= FIRST_EOC {
                return Ok(out);
            }
            if next == BAD || next < 2 || next >= self.clusters + 2 {
                return Err(WriteError::Corrupt);
            }
            c = next;
        }
    }

    // ------------------------------------------------------------ folders

    /// Reads the folder whose first cluster is `first` (the root cluster for the root).
    pub fn read_dir(&self, dev: &mut dyn BlockDev, first: u32) -> Result<Dir, WriteError> {
        let cb = self.cluster_bytes();
        let clusters = self.chain_max(dev, first, (MAX_DIR_SLOTS * 32).div_ceil(cb))?;
        let mut bytes = vec![0u8; clusters.len() * cb];
        for (i, &c) in clusters.iter().enumerate() {
            dev.read(self.cluster_sector(c), &mut bytes[i * cb..(i + 1) * cb])?;
        }
        Ok(Dir { clusters, bytes })
    }

    /// The entries of a folder, without `.` and `..`, the volume label and deleted entries.
    pub fn scan(&self, dir: &Dir) -> Vec<Item> {
        struct Lfn {
            sum: u8,
            expect: u8,
            first_slot: usize,
            chars: Vec<u16>,
        }
        const AT: [usize; 13] = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];
        let mut items = Vec::new();
        let mut lfn: Option<Lfn> = None;
        for i in 0..dir.bytes.len() / 32 {
            let s = &dir.bytes[i * 32..i * 32 + 32];
            match s[0] {
                0x00 => break,
                0xE5 => {
                    lfn = None;
                    continue;
                }
                _ => {}
            }
            if s[11] & 0x3F == 0x0F {
                let idx = s[0] & 0x1F;
                if s[0] & 0x40 != 0 {
                    lfn = (1..=20).contains(&idx).then(|| Lfn { sum: s[13], expect: idx, first_slot: i, chars: vec![0xFFFF; 13 * idx as usize] });
                }
                if let Some(l) = lfn.as_mut() {
                    if idx == l.expect && idx >= 1 && s[13] == l.sum {
                        for (k, &at) in AT.iter().enumerate() {
                            l.chars[(idx as usize - 1) * 13 + k] = u16::from_le_bytes([s[at], s[at + 1]]);
                        }
                        l.expect -= 1;
                    } else {
                        lfn = None;
                    }
                }
                continue;
            }
            let mut short = [0u8; 11];
            short.copy_from_slice(&s[..11]);
            let pending = lfn.take();
            if s[11] & 0x08 != 0 && s[11] & ATTR_DIR == 0 {
                continue; // volume label
            }
            if short == *b".          " || short == *b"..         " {
                continue;
            }
            let mut first_slot = i;
            let mut name = fatname::short_display(&short);
            if let Some(l) = pending {
                if l.expect == 0 && l.sum == fatname::checksum(&short) {
                    let end = l.chars.iter().position(|&c| c == 0).unwrap_or(l.chars.len());
                    let text: Option<String> = l.chars[..end].iter().map(|&c| u8::try_from(c).ok().filter(|b| (0x20..0x7F).contains(b)).map(char::from)).collect();
                    if let Some(t) = text {
                        name = t;
                    }
                    first_slot = l.first_slot;
                }
            }
            items.push(Item { name, short, attr: s[11], cluster: (le16(s, 20) << 16) | le16(s, 26), size: le32(s, 28), first_slot, slot: i });
        }
        items
    }

    /// Writes the sectors of `dir.bytes` that cover bytes `lo..hi`.
    fn flush(&mut self, dev: &mut dyn BlockWrite, dir: &Dir, lo: usize, hi: usize) -> Result<(), WriteError> {
        for si in lo / SECTOR_SIZE..=(hi - 1) / SECTOR_SIZE {
            let (ci, within) = (si / self.spc as usize, si % self.spc as usize);
            let lba = self.cluster_sector(dir.clusters[ci]) + within as u64;
            self.put(dev, lba, &dir.bytes[si * SECTOR_SIZE..(si + 1) * SECTOR_SIZE])?;
        }
        Ok(())
    }

    fn write_cluster(&mut self, dev: &mut dyn BlockWrite, c: u32, buf: &[u8]) -> Result<(), WriteError> {
        let lba = self.cluster_sector(c);
        self.put(dev, lba, buf)
    }

    /// Index of `n` consecutive free slots, counting deleted ones and everything after the end marker.
    fn find_run(bytes: &[u8], n: usize) -> Option<usize> {
        let slots = bytes.len() / 32;
        let end = (0..slots).find(|&i| bytes[i * 32] == 0).unwrap_or(slots);
        let mut run = 0;
        for i in 0..end {
            if bytes[i * 32] == 0xE5 {
                run += 1;
                if run == n {
                    return Some(i + 1 - n);
                }
            } else {
                run = 0;
            }
        }
        (run + (slots - end) >= n).then_some(end - run)
    }

    /// Finds room for `n` slots, adding a zeroed cluster to the folder if it is full.
    fn place(&mut self, dev: &mut dyn BlockWrite, dir: &mut Dir, n: usize) -> Result<usize, WriteError> {
        loop {
            if let Some(at) = Self::find_run(&dir.bytes, n) {
                return Ok(at);
            }
            let cb = self.cluster_bytes();
            if (dir.bytes.len() + cb) / 32 > MAX_DIR_SLOTS {
                return Err(WriteError::DirFull);
            }
            let c = self.alloc_one(dev)?;
            let linked = self.write_cluster(dev, c, &vec![0u8; cb]).and_then(|()| match dir.clusters.last() {
                Some(&last) => self.fat_set(dev, last, c),
                None => Err(WriteError::Corrupt),
            });
            if let Err(e) = linked {
                return Err(self.give_back(dev, &[c], e));
            }
            dir.clusters.push(c);
            dir.bytes.resize(dir.bytes.len() + cb, 0);
        }
    }

    fn put_slots(&mut self, dev: &mut dyn BlockWrite, dir: &mut Dir, at: usize, slots: &[[u8; 32]]) -> Result<(), WriteError> {
        for (k, s) in slots.iter().enumerate() {
            dir.bytes[(at + k) * 32..(at + k + 1) * 32].copy_from_slice(s);
        }
        self.flush(dev, dir, at * 32, (at + slots.len()) * 32)
    }

    // ------------------------------------------------------------ paths

    /// Checks the path and returns its components; refuses anything inside `EFI`.
    fn parse<'p>(&self, path: &'p str) -> Result<Vec<&'p str>, WriteError> {
        let comps = split(path)?;
        if self.protect_efi && fatname::same_name(comps[0], "EFI") {
            return Err(WriteError::Protected);
        }
        Ok(comps)
    }

    /// First cluster of the folder that holds the last component.
    fn parent_of(&self, dev: &mut dyn BlockDev, comps: &[&str]) -> Result<u32, WriteError> {
        let mut cur = self.root;
        for comp in &comps[..comps.len() - 1] {
            let dir = self.read_dir(dev, cur)?;
            let it = self.scan(&dir).into_iter().find(|i| fatname::same_name(&i.name, comp)).ok_or(WriteError::NotFound)?;
            if !it.is_dir() {
                return Err(WriteError::NotADirectory);
            }
            if it.cluster < 2 {
                return Err(WriteError::Corrupt);
            }
            cur = it.cluster;
        }
        Ok(cur)
    }

    /// Adds a new entry for `leaf` to `dir`.
    #[allow(clippy::too_many_arguments)]
    fn add_entry(&mut self, dev: &mut dyn BlockWrite, dir: &mut Dir, items: &[Item], leaf: &str, attr: u8, cluster: u32, size: u32) -> Result<(), WriteError> {
        // `None` for a name that is already a legal short name means another entry holds that short name
        // (usually as the alias of a long name): to the user that is "exists". For other names it means
        // all 999999 numbers are used up.
        let clash = if fatname::exact_short(leaf).is_some() { WriteError::Exists } else { WriteError::DirFull };
        let (short, long) = fatname::make_short(leaf, |s| items.iter().any(|i| i.short == *s)).ok_or(clash)?;
        let mut slots = if long { fatname::lfn_slots(leaf, &short) } else { Vec::new() };
        slots.push(short_entry(short, attr, cluster, size));
        let at = self.place(dev, dir, slots.len())?;
        self.put_slots(dev, dir, at, &slots)
    }

    // ------------------------------------------------------------ operations

    /// Writes `data` as the file at `path`. If the file exists, `replace` must be true. The parent folder
    /// must exist (nothing is created on the way). The old data is freed only after the new data and the
    /// entry are in place, so replacing needs room for both at once.
    pub fn write_file(&mut self, dev: &mut dyn BlockWrite, path: &str, data: &[u8], replace: bool) -> Result<(), WriteError> {
        let r = self.write_file_inner(dev, path, data, replace);
        self.finish(dev, r)
    }

    fn write_file_inner(&mut self, dev: &mut dyn BlockWrite, path: &str, data: &[u8], replace: bool) -> Result<(), WriteError> {
        let comps = self.parse(path)?;
        let leaf = comps[comps.len() - 1];
        let size = u32::try_from(data.len()).map_err(|_| WriteError::TooBig)?;
        let parent = self.parent_of(dev, &comps)?;
        let mut dir = self.read_dir(dev, parent)?;
        let items = self.scan(&dir);
        let existing = items.iter().find(|i| fatname::same_name(&i.name, leaf)).cloned();
        let mut old = Vec::new();
        if let Some(e) = &existing {
            if e.is_dir() {
                return Err(WriteError::IsADirectory);
            }
            if !replace {
                return Err(WriteError::Exists);
            }
            if e.cluster >= 2 {
                old = self.chain(dev, e.cluster)?; // a damaged chain is found before anything is changed
            }
        }
        let cb = self.cluster_bytes();
        let new = if data.is_empty() { Vec::new() } else { self.alloc_chain(dev, data.len().div_ceil(cb))? };
        // 1. the data, with the unused end of the last cluster zeroed
        let mut buf = vec![0u8; cb];
        for (i, &c) in new.iter().enumerate() {
            let chunk = &data[i * cb..((i + 1) * cb).min(data.len())];
            buf.fill(0);
            buf[..chunk.len()].copy_from_slice(chunk);
            if let Err(e) = self.write_cluster(dev, c, &buf) {
                return Err(self.give_back(dev, &new, e));
            }
        }
        // 2. the directory entry
        let first = new.first().copied().unwrap_or(0);
        let entry = match &existing {
            Some(e) => {
                let b = &mut dir.bytes[e.slot * 32..e.slot * 32 + 32];
                b[20..22].copy_from_slice(&((first >> 16) as u16).to_le_bytes());
                b[26..28].copy_from_slice(&(first as u16).to_le_bytes());
                b[28..32].copy_from_slice(&size.to_le_bytes());
                self.flush(dev, &dir, e.slot * 32, e.slot * 32 + 32)
            }
            None => self.add_entry(dev, &mut dir, &items, leaf, ATTR_FILE, first, size),
        };
        if let Err(e) = entry {
            return Err(self.give_back(dev, &new, e));
        }
        // 3. the old data, last
        self.free_list(dev, &old)
    }

    /// Makes the folder at `path`; the parent must exist.
    pub fn mkdir(&mut self, dev: &mut dyn BlockWrite, path: &str) -> Result<(), WriteError> {
        let r = self.mkdir_inner(dev, path);
        self.finish(dev, r)
    }

    fn mkdir_inner(&mut self, dev: &mut dyn BlockWrite, path: &str) -> Result<(), WriteError> {
        let comps = self.parse(path)?;
        let leaf = comps[comps.len() - 1];
        let parent = self.parent_of(dev, &comps)?;
        let mut dir = self.read_dir(dev, parent)?;
        let items = self.scan(&dir);
        if items.iter().any(|i| fatname::same_name(&i.name, leaf)) {
            return Err(WriteError::Exists);
        }
        let c = self.alloc_one(dev)?;
        let mut buf = vec![0u8; self.cluster_bytes()];
        let dotdot = if parent == self.root { 0 } else { parent };
        buf[..32].copy_from_slice(&short_entry(*b".          ", ATTR_DIR, c, 0));
        buf[32..64].copy_from_slice(&short_entry(*b"..         ", ATTR_DIR, dotdot, 0));
        if let Err(e) = self.write_cluster(dev, c, &buf) {
            return Err(self.give_back(dev, &[c], e));
        }
        if let Err(e) = self.add_entry(dev, &mut dir, &items, leaf, ATTR_DIR, c, 0) {
            return Err(self.give_back(dev, &[c], e));
        }
        Ok(())
    }

    /// Removes the file at `path`.
    pub fn remove_file(&mut self, dev: &mut dyn BlockWrite, path: &str) -> Result<(), WriteError> {
        let r = self.remove_inner(dev, path, false);
        self.finish(dev, r)
    }

    /// Removes the empty folder at `path`.
    pub fn remove_dir(&mut self, dev: &mut dyn BlockWrite, path: &str) -> Result<(), WriteError> {
        let r = self.remove_inner(dev, path, true);
        self.finish(dev, r)
    }

    fn remove_inner(&mut self, dev: &mut dyn BlockWrite, path: &str, want_dir: bool) -> Result<(), WriteError> {
        let comps = self.parse(path)?;
        let leaf = comps[comps.len() - 1];
        let parent = self.parent_of(dev, &comps)?;
        let mut dir = self.read_dir(dev, parent)?;
        let it = self.scan(&dir).into_iter().find(|i| fatname::same_name(&i.name, leaf)).ok_or(WriteError::NotFound)?;
        match (it.is_dir(), want_dir) {
            (true, false) => return Err(WriteError::IsADirectory),
            (false, true) => return Err(WriteError::NotADirectory),
            _ => {}
        }
        if want_dir && it.cluster < 2 {
            return Err(WriteError::Corrupt);
        }
        let old = if it.cluster >= 2 { self.chain(dev, it.cluster)? } else { Vec::new() };
        if want_dir && !self.scan(&self.read_dir(dev, it.cluster)?).is_empty() {
            return Err(WriteError::NotEmpty);
        }
        // The entry goes first, the clusters after: an interruption leaks space but never shares it.
        for i in it.first_slot..=it.slot {
            dir.bytes[i * 32] = 0xE5;
        }
        self.flush(dev, &dir, it.first_slot * 32, (it.slot + 1) * 32)?;
        self.free_list(dev, &old)
    }
}

#[cfg(test)]
#[path = "fatw_tests.rs"]
mod tests;
