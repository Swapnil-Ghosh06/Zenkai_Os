//! Tests for the FAT32 writer: behaviour on small images, safety rules, damaged volumes, a random
//! operation storm checked against a model, and a crash test that makes the disk fail at every write.

use super::*;
use crate::block::{BlockDev, BlockWrite, MemDisk, MemDiskMut, SECTOR_SIZE};
use crate::fat::Volume;
use std::collections::{BTreeMap, BTreeSet};

const AEON: [u8; 11] = *b"AEON       ";
const RESERVED: usize = 4;

/// A FAT32 volume in memory: boot sector, FSInfo, two FATs and an empty root folder in cluster 2.
fn mkimg(label: [u8; 11], clusters: u32, spc: u32) -> Vec<u8> {
    let fat_sectors = ((clusters as usize + 2) * 4).div_ceil(SECTOR_SIZE);
    let total = RESERVED + 2 * fat_sectors + clusters as usize * spc as usize;
    let mut img = vec![0u8; total * SECTOR_SIZE];
    let mut put = |o: usize, v: &[u8]| img[o..o + v.len()].copy_from_slice(v);
    put(0, &[0xEB, 0x58, 0x90]);
    put(3, b"AEONTEST");
    put(11, &512u16.to_le_bytes());
    put(13, &[spc as u8]);
    put(14, &(RESERVED as u16).to_le_bytes());
    put(16, &[2]);
    put(21, &[0xF8]);
    put(32, &(total as u32).to_le_bytes());
    put(36, &(fat_sectors as u32).to_le_bytes());
    put(44, &2u32.to_le_bytes());
    put(48, &1u16.to_le_bytes());
    put(66, &[0x29]);
    put(67, &[1, 2, 3, 4]);
    put(71, &label);
    put(82, b"FAT32   ");
    put(510, &[0x55, 0xAA]);
    // FSInfo
    put(512, b"RRaA");
    put(512 + 484, b"rrAa");
    put(512 + 488, &(clusters - 1).to_le_bytes());
    put(512 + 492, &3u32.to_le_bytes());
    put(512 + 508, &[0, 0, 0x55, 0xAA]);
    for k in 0..2 {
        let fat = (RESERVED + k * fat_sectors) * SECTOR_SIZE;
        put(fat, &0x0FFF_FFF8u32.to_le_bytes());
        put(fat + 4, &0x0FFF_FFFFu32.to_le_bytes());
        put(fat + 8, &0x0FFF_FFFFu32.to_le_bytes());
    }
    img
}

fn fat_sectors_of(img: &[u8]) -> usize {
    u32::from_le_bytes(img[36..40].try_into().unwrap()) as usize
}

fn fat_entry(img: &[u8], c: u32, copy: usize) -> u32 {
    let o = (RESERVED + copy * fat_sectors_of(img)) * SECTOR_SIZE + c as usize * 4;
    u32::from_le_bytes(img[o..o + 4].try_into().unwrap()) & 0x0FFF_FFFF
}

fn set_fat_entry(img: &mut [u8], c: u32, v: u32) {
    for copy in 0..2 {
        let o = (RESERVED + copy * fat_sectors_of(img)) * SECTOR_SIZE + c as usize * 4;
        img[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }
}

fn clean_bit(img: &[u8]) -> bool {
    fat_entry(img, 1, 0) & 0x0800_0000 != 0
}

/// An image plus its writer, with short helpers.
struct Rig {
    img: Vec<u8>,
    w: Fat32Writer,
}

impl Rig {
    fn new(clusters: u32, spc: u32) -> Rig {
        let mut img = mkimg(AEON, clusters, spc);
        let w = Fat32Writer::open(&mut MemDiskMut(&mut img), 0).expect("a fresh AEON volume opens");
        Rig { img, w }
    }

    fn write(&mut self, path: &str, data: &[u8], replace: bool) -> Result<(), WriteError> {
        self.w.write_file(&mut MemDiskMut(&mut self.img), path, data, replace)
    }

    fn mkdir(&mut self, path: &str) -> Result<(), WriteError> {
        self.w.mkdir(&mut MemDiskMut(&mut self.img), path)
    }

    fn rm(&mut self, path: &str) -> Result<(), WriteError> {
        self.w.remove_file(&mut MemDiskMut(&mut self.img), path)
    }

    fn rmdir(&mut self, path: &str) -> Result<(), WriteError> {
        self.w.remove_dir(&mut MemDiskMut(&mut self.img), path)
    }

    fn free(&mut self) -> u32 {
        self.w.free_clusters(&mut MemDiskMut(&mut self.img)).unwrap()
    }

    /// Reads a file back with the independent read-only reader in `fat.rs`.
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let mut d = MemDisk(&self.img);
        let v = Volume::open(&mut d, 0).ok()?;
        let e = v.find(&mut d, path).ok()?;
        if e.is_dir {
            return None;
        }
        let mut out = Vec::new();
        v.read_file(&mut d, &e, 1 << 26, &mut out).ok()?;
        Some(out)
    }

    fn is_dir(&self, path: &str) -> bool {
        let mut d = MemDisk(&self.img);
        Volume::open(&mut d, 0).ok().and_then(|v| v.find(&mut d, path).ok()).is_some_and(|e| e.is_dir)
    }

    fn names(&self, path: &str) -> Vec<String> {
        let mut d = MemDisk(&self.img);
        let v = Volume::open(&mut d, 0).unwrap();
        let mut out = Vec::new();
        v.list(&mut d, path, |e| out.push(e.name().to_string())).unwrap();
        out.sort();
        out
    }
}

/// Walks the whole tree with the writer's own parser and checks the structure: both FATs equal, every
/// chain valid, no cluster owned twice, file chains exactly as long as their sizes need. Returns the
/// number of clusters in use. Leaked clusters (allocated, owned by nobody) are allowed.
fn check(img: &[u8], w: &Fat32Writer) -> Result<usize, String> {
    check_with(img, w, true)
}

fn check_with(img: &[u8], w: &Fat32Writer, mirrored: bool) -> Result<usize, String> {
    let fs = fat_sectors_of(img) * SECTOR_SIZE;
    let a = &img[RESERVED * SECTOR_SIZE..RESERVED * SECTOR_SIZE + fs];
    let b = &img[RESERVED * SECTOR_SIZE + fs..RESERVED * SECTOR_SIZE + 2 * fs];
    if mirrored && a != b {
        return Err("the two FAT copies differ".into());
    }
    let mut owned = BTreeSet::new();
    let mut disk = MemDisk(img);
    let mut todo = vec![w.root_cluster()];
    while let Some(first) = todo.pop() {
        let dir = w.read_dir(&mut disk, first).map_err(|e| format!("read_dir {first}: {e:?}"))?;
        for c in &dir.clusters {
            if !owned.insert(*c) {
                return Err(format!("cluster {c} is owned twice"));
            }
        }
        for it in w.scan(&dir) {
            if it.is_dir() {
                if it.cluster < 2 {
                    return Err(format!("folder {} has no cluster", it.name));
                }
                todo.push(it.cluster);
            } else if it.size == 0 {
                if it.cluster != 0 {
                    return Err(format!("empty file {} owns cluster {}", it.name, it.cluster));
                }
            } else {
                let chain = w.chain(&mut disk, it.cluster).map_err(|e| format!("chain of {}: {e:?}", it.name))?;
                let need = (it.size as usize).div_ceil(w.cluster_bytes());
                if chain.len() != need {
                    return Err(format!("file {} has {} clusters, needs {need}", it.name, chain.len()));
                }
                for c in chain {
                    if !owned.insert(c) {
                        return Err(format!("cluster {c} is owned twice"));
                    }
                }
            }
        }
    }
    Ok(owned.len())
}

// ---------------------------------------------------------------- opening and refusing

#[test]
fn a_volume_labelled_aeon_opens() {
    let mut img = mkimg(AEON, 100, 1);
    assert!(Fat32Writer::open(&mut MemDiskMut(&mut img), 0).is_ok());
}

#[test]
fn any_other_label_is_refused() {
    for label in [*b"NO NAME    ", *b"aeon       ", *b"AEON2      ", *b"AEO        ", *b"           ", *b"AEON      X"] {
        let mut img = mkimg(label, 100, 1);
        assert_eq!(Fat32Writer::open(&mut MemDiskMut(&mut img), 0).err(), Some(WriteError::NotWritable(Reason::WrongLabel)), "{:?}", String::from_utf8_lossy(&label));
    }
}

#[test]
fn a_boot_sector_without_the_extended_signature_is_refused() {
    let mut img = mkimg(AEON, 100, 1);
    img[66] = 0;
    assert!(matches!(Fat32Writer::open(&mut MemDiskMut(&mut img), 0), Err(WriteError::NotWritable(_))));
}

#[test]
fn a_single_active_fat_is_refused() {
    let mut img = mkimg(AEON, 100, 1);
    img[40] |= 0x80;
    assert_eq!(Fat32Writer::open(&mut MemDiskMut(&mut img), 0).err(), Some(WriteError::NotWritable(Reason::NoMirroring)));
}

#[test]
fn garbage_and_non_fat32_are_refused() {
    let mut zeros = vec![0u8; 100 * 512];
    assert!(matches!(Fat32Writer::open(&mut MemDiskMut(&mut zeros), 0), Err(WriteError::NotWritable(_))));
    let mut img = mkimg(AEON, 100, 1);
    // Make it look like FAT16: root entries and a 16-bit FAT size.
    img[17] = 0;
    img[18] = 2;
    img[22] = 1;
    assert!(matches!(Fat32Writer::open(&mut MemDiskMut(&mut img), 0), Err(WriteError::NotWritable(_))));
    let mut tiny = vec![0u8; 512];
    assert!(Fat32Writer::open(&mut MemDiskMut(&mut tiny), 0).is_err());
}

#[test]
fn opening_writes_nothing() {
    let mut img = mkimg(AEON, 100, 1);
    let before = img.clone();
    Fat32Writer::open(&mut MemDiskMut(&mut img), 0).unwrap();
    assert_eq!(img, before);
}

// ---------------------------------------------------------------- files

#[test]
fn a_new_file_reads_back_through_the_independent_reader() {
    let mut r = Rig::new(100, 1);
    r.write("HELLO.TXT", b"hello world", false).unwrap();
    assert_eq!(r.read("HELLO.TXT").as_deref(), Some(&b"hello world"[..]));
    assert_eq!(r.names(""), ["HELLO.TXT"]);
    assert!(check(&r.img, &r.w).is_ok());
}

#[test]
fn long_and_lower_case_names_survive_a_round_trip() {
    let mut r = Rig::new(100, 1);
    for n in ["note.txt", "Long Name Here.txt", "a", "x.y.z", ".hidden", "UPPER.TXT", "mixed.Case"] {
        r.write(n, n.as_bytes(), false).unwrap();
    }
    let mut want: Vec<String> = ["note.txt", "Long Name Here.txt", "a", "x.y.z", ".hidden", "UPPER.TXT", "mixed.Case"].iter().map(|s| s.to_string()).collect();
    want.sort();
    assert_eq!(r.names(""), want);
    assert_eq!(r.read("NOTE.TXT").as_deref(), Some(&b"note.txt"[..]), "lookup ignores case");
    assert_eq!(r.read("long name here.TXT").as_deref(), Some(&b"Long Name Here.txt"[..]));
}

#[test]
fn sizes_around_every_cluster_boundary() {
    for spc in [1u32, 2, 4] {
        let cb = spc as usize * 512;
        for len in [0, 1, 511, 512, 513, cb - 1, cb, cb + 1, 2 * cb, 2 * cb + 1, 5000] {
            let mut r = Rig::new(200, spc);
            let data: Vec<u8> = (0..len).map(|i| (i * 7 + 3) as u8).collect();
            r.write("f.bin", &data, false).unwrap();
            assert_eq!(r.read("f.bin").as_deref(), Some(&data[..]), "spc {spc} len {len}");
            assert_eq!(check(&r.img, &r.w).unwrap(), 1 + len.div_ceil(cb), "spc {spc} len {len}: root + file clusters");
        }
    }
}

#[test]
fn the_unused_end_of_the_last_cluster_is_zero() {
    let mut r = Rig::new(100, 2);
    // Dirty a cluster first, free it again, then reuse it: nothing old may show through.
    r.write("old", &[0xAA; 1000], false).unwrap();
    r.rm("old").unwrap();
    r.write("new", &[0x55; 10], false).unwrap();
    let mut disk = MemDisk(&r.img);
    let it = r.w.scan(&r.w.read_dir(&mut disk, r.w.root_cluster()).unwrap());
    let first = it[0].cluster;
    let start = (RESERVED + 2 * fat_sectors_of(&r.img) + (first as usize - 2) * 2) * 512;
    assert!(r.img[start..start + 10].iter().all(|&b| b == 0x55));
    assert!(r.img[start + 10..start + 1024].iter().all(|&b| b == 0), "the rest of the cluster must be zeroed");
}

#[test]
fn the_end_of_a_multi_cluster_file_is_zero_not_the_previous_cluster() {
    let mut r = Rig::new(100, 1);
    // 2 full clusters of 0xAA and 10 bytes of 0x55: the last cluster must not keep any 0xAA.
    let mut data = vec![0xAAu8; 1024];
    data.extend_from_slice(&[0x55; 10]);
    r.write("f", &data, false).unwrap();
    let mut disk = MemDisk(&r.img);
    let it = r.w.scan(&r.w.read_dir(&mut disk, r.w.root_cluster()).unwrap());
    let chain = r.w.chain(&mut disk, it[0].cluster).unwrap();
    assert_eq!(chain.len(), 3);
    let start = (RESERVED + 2 * fat_sectors_of(&r.img) + (chain[2] as usize - 2)) * 512;
    assert!(r.img[start..start + 10].iter().all(|&b| b == 0x55));
    assert!(r.img[start + 10..start + 512].iter().all(|&b| b == 0), "the tail of the last cluster must be zeroed");
}

#[test]
fn an_existing_file_is_not_replaced_without_permission() {
    let mut r = Rig::new(100, 1);
    r.write("a.txt", b"first", false).unwrap();
    assert_eq!(r.write("a.txt", b"second", false), Err(WriteError::Exists));
    assert_eq!(r.write("A.TXT", b"second", false), Err(WriteError::Exists), "names are case-insensitive");
    assert_eq!(r.read("a.txt").as_deref(), Some(&b"first"[..]));
}

#[test]
fn replacing_changes_the_content_and_gives_back_the_old_clusters() {
    let mut r = Rig::new(100, 1);
    let free0 = r.free();
    r.write("a.txt", &[1u8; 3000], false).unwrap();
    let used_big = free0 - r.free();
    assert_eq!(used_big, 6);
    r.write("a.txt", b"tiny", true).unwrap();
    assert_eq!(r.read("a.txt").as_deref(), Some(&b"tiny"[..]));
    assert_eq!(free0 - r.free(), 1);
    r.write("a.txt", &[2u8; 1500], true).unwrap();
    assert_eq!(r.read("a.txt").as_deref(), Some(&[2u8; 1500][..]));
    assert_eq!(free0 - r.free(), 3);
    r.write("a.txt", b"", true).unwrap();
    assert_eq!(r.read("a.txt").as_deref(), Some(&b""[..]));
    assert_eq!(free0 - r.free(), 0);
    assert!(check(&r.img, &r.w).is_ok());
}

#[test]
fn replace_keeps_the_long_name_and_does_not_add_entries() {
    let mut r = Rig::new(100, 1);
    r.write("Some Long Name.txt", b"v1", false).unwrap();
    r.write("Some Long Name.txt", b"v2", true).unwrap();
    assert_eq!(r.names(""), ["Some Long Name.txt"]);
}

#[test]
fn removing_a_file_frees_its_clusters_and_name() {
    let mut r = Rig::new(100, 1);
    let free0 = r.free();
    r.write("a.txt", &[9u8; 2000], false).unwrap();
    r.rm("A.txt").unwrap();
    assert_eq!(r.free(), free0);
    assert!(r.names("").is_empty());
    assert_eq!(r.rm("a.txt"), Err(WriteError::NotFound));
    r.write("a.txt", b"again", false).unwrap();
    assert_eq!(r.read("a.txt").as_deref(), Some(&b"again"[..]));
}

#[test]
fn short_name_collisions_get_distinct_numbers() {
    let mut r = Rig::new(200, 1);
    for i in 0..12 {
        r.write(&format!("same prefix {i}.txt"), format!("{i}").as_bytes(), false).unwrap();
    }
    for i in 0..12 {
        assert_eq!(r.read(&format!("same prefix {i}.txt")).unwrap(), format!("{i}").into_bytes());
    }
    assert!(check(&r.img, &r.w).is_ok());
}

// ---------------------------------------------------------------- folders

#[test]
fn folders_nest_and_hold_files() {
    let mut r = Rig::new(100, 1);
    r.mkdir("a").unwrap();
    r.mkdir("a/b").unwrap();
    r.mkdir("a/b/deep").unwrap();
    r.write("a/b/deep/note.txt", b"nested", false).unwrap();
    r.write("a/top.txt", b"top", false).unwrap();
    assert_eq!(r.read("a/b/deep/note.txt").as_deref(), Some(&b"nested"[..]));
    assert!(r.is_dir("a/b"));
    assert_eq!(r.names("a"), ["b", "top.txt"]);
    assert_eq!(check(&r.img, &r.w).unwrap(), 1 + 3 + 2);
}

#[test]
fn a_new_folder_has_dot_and_dotdot_pointing_the_right_way() {
    let mut r = Rig::new(100, 1);
    r.mkdir("a").unwrap();
    r.mkdir("a/b").unwrap();
    let mut d = MemDisk(&r.img);
    let root = r.w.read_dir(&mut d, r.w.root_cluster()).unwrap();
    let a = r.w.scan(&root)[0].cluster;
    let adir = r.w.read_dir(&mut d, a).unwrap();
    let b = r.w.scan(&adir)[0].cluster;
    let slot = |dir: &Dir, i: usize| dir.bytes[i * 32..i * 32 + 32].to_vec();
    let cluster_of = |s: &[u8]| (u32::from(u16::from_le_bytes([s[20], s[21]])) << 16) | u32::from(u16::from_le_bytes([s[26], s[27]]));
    let (dot, dotdot) = (slot(&adir, 0), slot(&adir, 1));
    assert_eq!(&dot[..11], b".          ");
    assert_eq!(&dotdot[..11], b"..         ");
    assert_eq!(cluster_of(&dot), a);
    assert_eq!(cluster_of(&dotdot), 0, "`..` of a folder in the root is cluster 0");
    let bdir = r.w.read_dir(&mut d, b).unwrap();
    assert_eq!(cluster_of(&slot(&bdir, 0)), b);
    assert_eq!(cluster_of(&slot(&bdir, 1)), a);
}

#[test]
fn folder_errors() {
    let mut r = Rig::new(100, 1);
    r.mkdir("a").unwrap();
    r.write("f.txt", b"x", false).unwrap();
    assert_eq!(r.mkdir("a"), Err(WriteError::Exists));
    assert_eq!(r.mkdir("A"), Err(WriteError::Exists));
    assert_eq!(r.mkdir("f.txt"), Err(WriteError::Exists));
    assert_eq!(r.mkdir("x/y"), Err(WriteError::NotFound), "no folders are made on the way");
    assert_eq!(r.mkdir("f.txt/y"), Err(WriteError::NotADirectory));
    assert_eq!(r.write("x/y.txt", b"", false), Err(WriteError::NotFound));
    assert_eq!(r.write("f.txt/y.txt", b"", false), Err(WriteError::NotADirectory));
    assert_eq!(r.write("a", b"data", true), Err(WriteError::IsADirectory));
    assert_eq!(r.rm("a"), Err(WriteError::IsADirectory));
    assert_eq!(r.rmdir("f.txt"), Err(WriteError::NotADirectory));
    assert_eq!(r.rmdir("nothere"), Err(WriteError::NotFound));
    r.write("a/inside", b"1", false).unwrap();
    assert_eq!(r.rmdir("a"), Err(WriteError::NotEmpty));
    r.rm("a/inside").unwrap();
    r.rmdir("a").unwrap();
    assert!(!r.is_dir("a"));
}

#[test]
fn removing_a_folder_gives_back_its_cluster() {
    let mut r = Rig::new(100, 1);
    let free0 = r.free();
    r.mkdir("a").unwrap();
    assert_eq!(free0 - r.free(), 1);
    r.rmdir("a").unwrap();
    assert_eq!(r.free(), free0);
}

#[test]
fn a_folder_grows_when_it_runs_out_of_slots() {
    let mut r = Rig::new(400, 1);
    r.mkdir("d").unwrap();
    for i in 0..40 {
        r.write(&format!("d/file number {i:02}.txt"), format!("{i}").as_bytes(), false).unwrap();
    }
    for i in 0..40 {
        assert_eq!(r.read(&format!("d/file number {i:02}.txt")).unwrap(), format!("{i}").into_bytes());
    }
    assert_eq!(r.names("d").len(), 40);
    let mut disk = MemDisk(&r.img);
    let root = r.w.read_dir(&mut disk, r.w.root_cluster()).unwrap();
    let d = r.w.scan(&root)[0].cluster;
    assert!(r.w.chain(&mut disk, d).unwrap().len() >= 7, "40 entries of 3 slots need at least 8 clusters of 16 slots");
    assert!(check(&r.img, &r.w).is_ok());
    // Remove them all: the slots are reused by the next ones.
    for i in 0..40 {
        r.rm(&format!("d/file number {i:02}.txt")).unwrap();
    }
    let before = MemDisk(&r.img).0.len();
    r.write("d/again.txt", b"x", false).unwrap();
    assert_eq!(MemDisk(&r.img).0.len(), before);
    assert!(check(&r.img, &r.w).is_ok());
}

#[test]
fn the_root_cannot_be_made_or_removed_and_empty_names_are_refused() {
    let mut r = Rig::new(100, 1);
    for p in ["", "/", "//", "a//b", "a/", "."] {
        assert_eq!(r.mkdir(p), Err(WriteError::BadName), "mkdir {p:?}");
        assert_eq!(r.rmdir(p), Err(WriteError::BadName), "rmdir {p:?}");
        assert_eq!(r.rm(p), Err(WriteError::BadName), "rm {p:?}");
        assert_eq!(r.write(p, b"x", true), Err(WriteError::BadName), "write {p:?}");
    }
    for p in ["..", "a/../b", "a/./b", "bad:name", "q?", "tab\there", "caf\u{e9}"] {
        assert_eq!(r.write(p, b"x", false), Err(WriteError::BadName), "write {p:?}");
    }
}

// ---------------------------------------------------------------- the EFI folder is never written

/// A volume that already holds EFI/BOOT/BOOTX64.EFI (built with the protection switched off, which only
/// exists in test builds), plus an ordinary file next to it.
fn efi_rig() -> Rig {
    let mut r = Rig::new(100, 1);
    r.w.set_efi_protection(false);
    r.mkdir("EFI").unwrap();
    r.mkdir("EFI/BOOT").unwrap();
    r.write("EFI/BOOT/BOOTX64.EFI", &[7u8; 700], false).unwrap();
    r.w.set_efi_protection(true);
    r.write("ok.txt", b"x", false).unwrap();
    r
}

#[test]
fn nothing_can_be_written_inside_efi() {
    let mut r = efi_rig();
    let before = r.img.clone();
    for p in ["EFI", "efi", "Efi", "EFI/BOOT", "efi/boot/BOOTX64.EFI", "EFI/x.txt", "EFI/BOOT/new"] {
        assert_eq!(r.mkdir(p), Err(WriteError::Protected), "mkdir {p}");
        assert_eq!(r.write(p, b"x", true), Err(WriteError::Protected), "write {p}");
        assert_eq!(r.write(p, b"x", false), Err(WriteError::Protected), "write {p}");
        assert_eq!(r.rm(p), Err(WriteError::Protected), "rm {p}");
        assert_eq!(r.rmdir(p), Err(WriteError::Protected), "rmdir {p}");
    }
    assert_eq!(r.img, before, "refused operations change not one byte");
    assert_eq!(r.read("EFI/BOOT/BOOTX64.EFI").as_deref(), Some(&[7u8; 700][..]), "the boot file can still be read");
}

#[test]
fn ordinary_work_leaves_the_efi_folder_alone() {
    let mut r = efi_rig();
    let mut d = MemDisk(&r.img);
    let efi_cluster = |r: &Rig, d: &mut MemDisk| r.w.scan(&r.w.read_dir(d, 2).unwrap()).into_iter().find(|i| i.name == "EFI").unwrap().cluster;
    let c = efi_cluster(&r, &mut d);
    let dir_before = r.w.read_dir(&mut d, c).unwrap().bytes;
    for i in 0..30 {
        r.write(&format!("work {i}.txt"), &[i as u8; 900], false).unwrap();
    }
    r.mkdir("docs").unwrap();
    r.write("docs/a.txt", b"a", false).unwrap();
    for i in 0..30 {
        r.rm(&format!("work {i}.txt")).unwrap();
    }
    let mut d = MemDisk(&r.img);
    assert_eq!(r.w.read_dir(&mut d, c).unwrap().bytes, dir_before);
    assert_eq!(r.read("EFI/BOOT/BOOTX64.EFI").as_deref(), Some(&[7u8; 700][..]));
    assert!(check(&r.img, &r.w).is_ok());
}

#[test]
fn a_file_called_efi_deeper_down_is_not_protected() {
    let mut r = Rig::new(100, 1);
    r.mkdir("docs").unwrap();
    r.write("docs/EFI", b"just a file", false).unwrap();
    r.mkdir("docs/efi2").unwrap();
    assert_eq!(r.read("docs/EFI").as_deref(), Some(&b"just a file"[..]));
}

// ---------------------------------------------------------------- space

#[test]
fn a_full_volume_refuses_cleanly_and_changes_nothing() {
    let mut r = Rig::new(20, 1);
    r.write("a", &[1u8; 10 * 512], false).unwrap();
    let before = r.img.clone();
    assert_eq!(r.write("b", &[2u8; 20 * 512], false), Err(WriteError::NoSpace));
    assert_eq!(r.img, before, "a refused write must leave the volume exactly as it was");
    assert_eq!(r.mkdir("zzz").err(), None, "there is still room for a folder");
    // Fill it up completely, then everything that needs a cluster is refused.
    let free = r.free();
    r.write("c", &vec![3u8; free as usize * 512], false).unwrap();
    assert_eq!(r.free(), 0);
    let full = r.img.clone();
    assert_eq!(r.write("d", b"x", false), Err(WriteError::NoSpace));
    assert_eq!(r.mkdir("e"), Err(WriteError::NoSpace));
    assert_eq!(r.img, full);
    // Removing something makes room again.
    r.rm("c").unwrap();
    r.write("d", b"x", false).unwrap();
    assert!(check(&r.img, &r.w).is_ok());
}

#[test]
fn replacing_on_a_nearly_full_volume_keeps_the_old_file() {
    let mut r = Rig::new(20, 1);
    r.write("big", &[1u8; 12 * 512], false).unwrap();
    let before = r.img.clone();
    // Needs old + new at the same time (the old one is only freed once the new one is safe).
    assert_eq!(r.write("big", &[2u8; 12 * 512], true), Err(WriteError::NoSpace));
    assert_eq!(r.img, before);
    assert_eq!(r.read("big").as_deref(), Some(&[1u8; 12 * 512][..]));
}

// ---------------------------------------------------------------- which sectors get written

struct Spy<'a> {
    inner: MemDiskMut<'a>,
    written: BTreeSet<u64>,
    count: usize,
    fail_at: Option<usize>,
}

impl BlockDev for Spy<'_> {
    fn sectors(&self) -> u64 {
        self.inner.sectors()
    }
    fn read(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        self.inner.read(lba, buf)
    }
}

impl BlockWrite for Spy<'_> {
    fn write(&mut self, lba: u64, buf: &[u8]) -> Result<(), BlockError> {
        if self.fail_at.is_some_and(|n| self.count >= n) {
            // Like a pulled cable: this write and every later one fail.
            self.count += 1;
            return Err(BlockError::Io);
        }
        self.count += 1;
        for i in 0..(buf.len() / 512) as u64 {
            self.written.insert(lba + i);
        }
        self.inner.write(lba, buf)
    }
}

#[test]
fn the_boot_sector_and_reserved_area_are_never_written() {
    let mut img = mkimg(AEON, 200, 1);
    let mut w = Fat32Writer::open(&mut MemDiskMut(&mut img), 0).unwrap();
    let fs = fat_sectors_of(&img) as u64;
    let mut spy = Spy { inner: MemDiskMut(&mut img), written: BTreeSet::new(), count: 0, fail_at: None };
    w.mkdir(&mut spy, "d").unwrap();
    w.write_file(&mut spy, "d/some long file name.txt", &[5u8; 3000], false).unwrap();
    w.write_file(&mut spy, "d/some long file name.txt", b"x", true).unwrap();
    w.remove_file(&mut spy, "d/some long file name.txt").unwrap();
    w.remove_dir(&mut spy, "d").unwrap();
    assert!(!spy.written.contains(&0), "sector 0 (boot sector) must never be written");
    for lba in &spy.written {
        let in_fat = (RESERVED as u64..RESERVED as u64 + 2 * fs).contains(lba);
        let in_data = *lba >= RESERVED as u64 + 2 * fs;
        assert!(in_fat || in_data || *lba == 1, "sector {lba} is outside FAT, data and FSInfo");
    }
}

// ---------------------------------------------------------------- clean flag and FSInfo

#[test]
fn the_clean_flag_is_set_after_success_and_after_a_clean_refusal() {
    let mut r = Rig::new(100, 1);
    assert!(clean_bit(&r.img));
    r.write("a", b"x", false).unwrap();
    assert!(clean_bit(&r.img));
    assert_eq!(r.write("a", b"y", false), Err(WriteError::Exists));
    assert!(clean_bit(&r.img));
    assert_eq!(r.rm("nothere"), Err(WriteError::NotFound));
    assert!(clean_bit(&r.img));
}

#[test]
fn the_clean_flag_stays_cleared_if_the_disk_fails_midway() {
    let mut img = mkimg(AEON, 100, 1);
    let mut w = Fat32Writer::open(&mut MemDiskMut(&mut img), 0).unwrap();
    // Write number 0 is the "dirty" mark; fail the second write.
    let mut spy = Spy { inner: MemDiskMut(&mut img), written: BTreeSet::new(), count: 0, fail_at: Some(1) };
    assert_eq!(w.write_file(&mut spy, "a", &[1u8; 2000], false), Err(WriteError::Io(BlockError::Io)));
    drop(spy);
    assert!(!clean_bit(&img), "an interrupted operation must leave the volume marked as not cleanly shut down");
}

#[test]
fn fsinfo_never_claims_a_wrong_free_count() {
    let mut r = Rig::new(100, 1);
    r.write("a", &[1u8; 5000], false).unwrap();
    let count = u32::from_le_bytes(r.img[512 + 488..512 + 492].try_into().unwrap());
    let free = r.free();
    assert!(count == 0xFFFF_FFFF || count == free, "FSInfo says {count}, the FAT says {free}");
    assert_eq!(&r.img[512..516], b"RRaA", "FSInfo signatures stay intact");
}

// ---------------------------------------------------------------- damaged volumes

#[test]
fn a_looping_chain_is_an_error_not_a_hang() {
    let mut r = Rig::new(100, 1);
    r.write("f", &[1u8; 2000], false).unwrap();
    // Folder chain loop: root cluster 2 points to itself.
    set_fat_entry(&mut r.img, 2, 2);
    assert_eq!(r.w.read_dir(&mut MemDisk(&r.img), 2).err(), Some(WriteError::Corrupt));
    assert!(r.write("g", b"x", false).is_err());
    assert!(r.rm("f").is_err());
    assert!(r.mkdir("d").is_err());
}

#[test]
fn bad_free_and_out_of_range_links_are_errors() {
    for link in [0u32, 1, 0x0FFF_FFF7, 0x0FFF_FFF0, 0x0000_0FFF] {
        let mut r = Rig::new(100, 1);
        r.write("f", &[1u8; 2000], false).unwrap();
        let mut d = MemDisk(&r.img);
        let f = r.w.scan(&r.w.read_dir(&mut d, 2).unwrap())[0].cluster;
        set_fat_entry(&mut r.img, f, link);
        assert_eq!(r.w.chain(&mut MemDisk(&r.img), f).err(), Some(WriteError::Corrupt), "link {link:#x}");
        let before = r.img.clone();
        assert!(r.rm("f").is_err(), "removing a file whose chain is damaged is refused");
        assert_eq!(r.img, before, "link {link:#x}: and changes nothing");
    }
}

#[test]
fn random_garbage_in_a_folder_never_panics() {
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    for round in 0..300 {
        let mut r = Rig::new(60, 1);
        r.write("seed.txt", b"seed", false).unwrap();
        r.mkdir("sub").unwrap();
        let root = (RESERVED + 2 * fat_sectors_of(&r.img)) * 512;
        for i in 0..512 {
            if next() % 3 == 0 {
                r.img[root + i] = next() as u8;
            }
        }
        if round % 5 == 0 {
            let fat = RESERVED * 512;
            for _ in 0..8 {
                let o = fat + 8 + (next() as usize % 60) * 4;
                r.img[o..o + 4].copy_from_slice(&(next() as u32).to_le_bytes());
            }
        }
        let _ = r.write("new.txt", b"x", false);
        let _ = r.write("seed.txt", b"y", true);
        let _ = r.mkdir("other");
        let _ = r.rm("seed.txt");
        let _ = r.rmdir("sub");
        let _ = r.write("sub/in.txt", b"z", false);
        let _ = r.free();
    }
}

// ---------------------------------------------------------------- model-based storm

#[derive(Clone, Debug, PartialEq)]
enum Node {
    File(Vec<u8>),
    Dir,
}

struct Model(BTreeMap<String, Node>);

fn key(p: &str) -> String {
    p.to_ascii_lowercase()
}

impl Model {
    fn parent_state(&self, path: &str) -> Result<(), WriteError> {
        match path.rsplit_once('/') {
            None => Ok(()),
            Some((parent, _)) => {
                // Every ancestor must exist and be a folder.
                let mut acc = String::new();
                for c in parent.split('/') {
                    if !acc.is_empty() {
                        acc.push('/');
                    }
                    acc.push_str(c);
                    match self.0.get(&key(&acc)) {
                        None => return Err(WriteError::NotFound),
                        Some(Node::File(_)) => return Err(WriteError::NotADirectory),
                        Some(Node::Dir) => {}
                    }
                }
                Ok(())
            }
        }
    }

    fn has_children(&self, path: &str) -> bool {
        let prefix = format!("{}/", key(path));
        self.0.keys().any(|k| k.starts_with(&prefix))
    }
}

#[test]
fn a_random_storm_of_operations_matches_a_model() {
    let leaf = ["a", "B", "c.txt", "Long Name Here.txt", "d", "e.md", "file with spaces", "UPPER.TXT", "x.y.z"];
    let mut x = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    for spc in [1u32, 2] {
        let mut r = Rig::new(600, spc);
        let mut m = Model(BTreeMap::new());
        let mut refused_for_space = 0;
        for step in 0..700 {
            let depth = 1 + next() % 3;
            let path: Vec<&str> = (0..depth).map(|_| leaf[(next() % leaf.len() as u64) as usize]).collect();
            let path = path.join("/");
            let len = (next() % 2600) as usize;
            let data: Vec<u8> = (0..len).map(|i| (i as u64 ^ next()) as u8).collect();
            let op = next() % 10;
            let ctx = format!("spc {spc} step {step} op {op} path {path:?} len {len}");
            match op {
                0..=3 => {
                    let replace = next() % 2 == 0;
                    let want = m.parent_state(&path).and_then(|_| match m.0.get(&key(&path)) {
                        Some(Node::Dir) => Err(WriteError::IsADirectory),
                        Some(Node::File(_)) if !replace => Err(WriteError::Exists),
                        _ => Ok(()),
                    });
                    let got = r.write(&path, &data, replace);
                    if got == Err(WriteError::NoSpace) && want.is_ok() {
                        refused_for_space += 1;
                    } else {
                        assert_eq!(got, want, "{ctx}");
                        if want.is_ok() {
                            // A replace keeps the old name's spelling; the model only needs the content.
                            m.0.insert(key(&path), Node::File(data));
                        }
                    }
                }
                4 | 5 => {
                    let want = m.parent_state(&path).and_then(|_| if m.0.contains_key(&key(&path)) { Err(WriteError::Exists) } else { Ok(()) });
                    let got = r.mkdir(&path);
                    if got == Err(WriteError::NoSpace) && want.is_ok() {
                        refused_for_space += 1;
                    } else {
                        assert_eq!(got, want, "{ctx}");
                        if want.is_ok() {
                            m.0.insert(key(&path), Node::Dir);
                        }
                    }
                }
                6 | 7 => {
                    let want = m.parent_state(&path).and_then(|_| match m.0.get(&key(&path)) {
                        None => Err(WriteError::NotFound),
                        Some(Node::Dir) => Err(WriteError::IsADirectory),
                        Some(Node::File(_)) => Ok(()),
                    });
                    assert_eq!(r.rm(&path), want, "{ctx}");
                    if want.is_ok() {
                        m.0.remove(&key(&path));
                    }
                }
                _ => {
                    let want = m.parent_state(&path).and_then(|_| match m.0.get(&key(&path)) {
                        None => Err(WriteError::NotFound),
                        Some(Node::File(_)) => Err(WriteError::NotADirectory),
                        Some(Node::Dir) if m.has_children(&path) => Err(WriteError::NotEmpty),
                        Some(Node::Dir) => Ok(()),
                    });
                    assert_eq!(r.rmdir(&path), want, "{ctx}");
                    if want.is_ok() {
                        m.0.remove(&key(&path));
                    }
                }
            }
            // The whole tree must equal the model, and the structure must be sound.
            let used = check(&r.img, &r.w).unwrap_or_else(|e| panic!("{ctx}: {e}"));
            assert_eq!(r.free() as usize + used, r.w.cluster_count() as usize, "{ctx}: every cluster is either free or owned (no leak, none lost)");
            if step % 25 == 0 || step == 699 {
                for (k, node) in &m.0 {
                    match node {
                        Node::File(d) => assert_eq!(r.read(k).as_deref(), Some(&d[..]), "{ctx}: file {k}"),
                        Node::Dir => assert!(r.is_dir(k), "{ctx}: folder {k}"),
                    }
                }
                let on_disk = tree_names(&r);
                let in_model: BTreeSet<String> = m.0.keys().cloned().collect();
                assert_eq!(on_disk, in_model, "{ctx}");
            }
        }
        assert!(m.0.len() > 3, "the storm should have built something (spc {spc}, refused for space {refused_for_space})");
    }
}

/// All paths on the disk, lower-cased, found with the independent reader.
fn tree_names(r: &Rig) -> BTreeSet<String> {
    fn walk(r: &Rig, dir: &str, out: &mut BTreeSet<String>) {
        for n in r.names(dir) {
            let p = if dir.is_empty() { n.clone() } else { format!("{dir}/{n}") };
            out.insert(key(&p));
            if r.is_dir(&p) {
                walk(r, &p, out);
            }
        }
    }
    let mut out = BTreeSet::new();
    walk(r, "", &mut out);
    out
}

// ---------------------------------------------------------------- the disk fails at every possible write

#[test]
fn a_failure_at_any_write_never_corrupts_the_volume() {
    // The same scenario, with the disk failing at write number 0, 1, 2, ... Afterwards the volume must
    // still be a sound tree (leaked clusters are fine, shared or dangling ones are not).
    fn scenario(w: &mut Fat32Writer, d: &mut dyn BlockWrite) -> Result<(), WriteError> {
        w.mkdir(d, "dir")?;
        w.write_file(d, "dir/some long file name.txt", &[1u8; 1700], false)?;
        w.write_file(d, "dir/some long file name.txt", &[2u8; 600], true)?;
        for i in 0..20 {
            w.write_file(d, &format!("dir/grow {i}.txt"), b"x", false)?; // forces the folder to grow
        }
        w.write_file(d, "top.bin", &[3u8; 3000], false)?;
        w.remove_file(d, "dir/some long file name.txt")?;
        w.remove_file(d, "top.bin")?;
        for i in 0..20 {
            w.remove_file(d, &format!("dir/grow {i}.txt"))?;
        }
        w.remove_dir(d, "dir")
    }
    let base = mkimg(AEON, 80, 1);
    // First a clean run to count the writes.
    let total = {
        let mut img = base.clone();
        let mut w = Fat32Writer::open(&mut MemDiskMut(&mut img), 0).unwrap();
        let mut spy = Spy { inner: MemDiskMut(&mut img), written: BTreeSet::new(), count: 0, fail_at: None };
        scenario(&mut w, &mut spy).unwrap();
        spy.count
    };
    assert!(total > 100, "the scenario should do plenty of writes, did {total}");
    for n in 0..total {
        let mut img = base.clone();
        let mut w = Fat32Writer::open(&mut MemDiskMut(&mut img), 0).unwrap();
        let outcome = {
            let mut spy = Spy { inner: MemDiskMut(&mut img), written: BTreeSet::new(), count: 0, fail_at: Some(n) };
            scenario(&mut w, &mut spy)
        };
        assert_eq!(outcome, Err(WriteError::Io(BlockError::Io)), "failure at write {n} must be reported");
        if let Err(e) = check_with(&img, &w, false) {
            panic!("after a disk failure at write {n} the volume is unsound: {e}");
        }
    }
}

// ---------------------------------------------------------------- real images, checked by fsck.vfat

/// Applies a mixed set of operations to every `base-*.img` in `$FATW_DIR` (made by `mkfs.fat` through
/// `tools/fatw-check.sh`), reads everything back with the independent reader, and saves `out-*.img` for
/// `fsck.vfat` plus `expect-*.txt` with the paths that must exist. Run by the script, not by `cargo test`.
#[test]
#[ignore = "needs the images made by tools/fatw-check.sh"]
fn scenarios_on_real_mkfs_images() {
    let dir = std::env::var("FATW_DIR").expect("FATW_DIR is set by tools/fatw-check.sh");
    let mut found = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let Some(tag) = name.strip_prefix("base-").and_then(|n| n.strip_suffix(".img")) else { continue };
        found += 1;
        let mut img = std::fs::read(&path).unwrap();
        let mut w = Fat32Writer::open(&mut MemDiskMut(&mut img), 0).expect("mkfs.fat -n AEON gives a writable volume");
        let mut want: BTreeMap<String, Option<Vec<u8>>> = BTreeMap::new();
        {
            let mut d = MemDiskMut(&mut img);
            let pattern = |n: usize, salt: u8| -> Vec<u8> { (0..n).map(|i| (i % 251) as u8 ^ salt).collect() };
            fn put_in(want: &mut BTreeMap<String, Option<Vec<u8>>>, w: &mut Fat32Writer, d: &mut MemDiskMut, p: &str, data: Vec<u8>, replace: bool) {
                w.write_file(d, p, &data, replace).unwrap_or_else(|e| panic!("write {p}: {e:?}"));
                want.insert(p.to_string(), Some(data));
            }
            w.mkdir(&mut d, "docs").unwrap();
            w.mkdir(&mut d, "docs/sub").unwrap();
            want.insert("docs".into(), None);
            want.insert("docs/sub".into(), None);
            put_in(&mut want, &mut w, &mut d, "README.TXT", pattern(300, 1), false);
            put_in(&mut want, &mut w, &mut d, "lower case.txt", pattern(1500, 2), false);
            put_in(&mut want, &mut w, &mut d, "empty", Vec::new(), false);
            put_in(&mut want, &mut w, &mut d, "docs/Long File Name Number One.txt", pattern(3000, 3), false);
            put_in(&mut want, &mut w, &mut d, "docs/sub/deep.bin", pattern(100_000, 4), false);
            for i in 0..40 {
                put_in(&mut want, &mut w, &mut d, &format!("docs/item {i:02}.dat"), pattern(10 + i * 37, i as u8), false);
            }
            put_in(&mut want, &mut w, &mut d, "lower case.txt", pattern(9000, 5), true);
            put_in(&mut want, &mut w, &mut d, "docs/sub/deep.bin", pattern(10, 6), true);
            for i in (0..40).step_by(4) {
                let p = format!("docs/item {i:02}.dat");
                w.remove_file(&mut d, &p).unwrap();
                want.remove(&p);
            }
            w.mkdir(&mut d, "tmpdir").unwrap();
            w.mkdir(&mut d, "tmpdir/inner").unwrap();
            w.remove_dir(&mut d, "tmpdir/inner").unwrap();
            w.remove_dir(&mut d, "tmpdir").unwrap();
            put_in(&mut want, &mut w, &mut d, "again.txt", pattern(777, 7), false);
        }
        // Read everything back with the other reader.
        {
            let mut d = MemDisk(&img);
            let v = Volume::open(&mut d, 0).unwrap();
            for (p, content) in &want {
                let e = v.find(&mut d, p).unwrap_or_else(|e| panic!("{tag}: {p} not found: {e:?}"));
                match content {
                    None => assert!(e.is_dir, "{p}"),
                    Some(c) => {
                        let mut out = Vec::new();
                        v.read_file(&mut d, &e, 1 << 24, &mut out).unwrap();
                        assert_eq!(&out, c, "{tag}: content of {p}");
                    }
                }
            }
        }
        std::fs::write(format!("{dir}/out-{tag}.img"), &img).unwrap();
        let listing: String = want.keys().map(|k| format!("/{k}\n")).collect();
        std::fs::write(format!("{dir}/expect-{tag}.txt"), listing).unwrap();
    }
    assert!(found > 0, "no base-*.img in {dir}");
}

#[test]
fn a_name_equal_to_another_files_short_alias_counts_as_existing() {
    let mut r = Rig::new(100, 1);
    r.write("long name file.txt", b"x", false).unwrap(); // stored with the alias LONGNA~1.TXT
    assert_eq!(r.write("LONGNA~1.TXT", b"y", false), Err(WriteError::Exists));
    assert_eq!(r.mkdir("LONGNA~1.TXT"), Err(WriteError::Exists));
    assert_eq!(r.names(""), ["long name file.txt"]);
}

#[test]
fn open_any_finds_volume_at_root_or_in_mbr() {
    let vol = mkimg(AEON, 100, 1);
    let mut d = MemDiskMut(&mut vol.clone());
    let (w, base) = Fat32Writer::open_any(&mut d).unwrap();
    assert_eq!(base, 0);
    assert_eq!(w.root_cluster(), 2);

    // MBR partition at sector 63
    let mut mbr = vec![0u8; 63 * SECTOR_SIZE];
    mbr.extend_from_slice(&vol);
    mbr[446 + 4] = 0x0C; // FAT32 LBA
    mbr[446 + 8..446 + 12].copy_from_slice(&63u32.to_le_bytes());
    mbr[510] = 0x55;
    mbr[511] = 0xAA;
    let mut d = MemDiskMut(&mut mbr);
    let (w, base) = Fat32Writer::open_any(&mut d).unwrap();
    assert_eq!(base, 63);
    assert_eq!(w.root_cluster(), 2);
}

#[test]
fn open_any_preserves_wrong_label_error_on_partitioned_disk() {
    const OTHER: [u8; 11] = *b"OTHER      ";
    let vol = mkimg(OTHER, 100, 1);
    let mut mbr = vec![0u8; 63 * SECTOR_SIZE];
    mbr.extend_from_slice(&vol);
    mbr[446 + 4] = 0x0C; // FAT32 LBA
    mbr[446 + 8..446 + 12].copy_from_slice(&63u32.to_le_bytes());
    mbr[510] = 0x55;
    mbr[511] = 0xAA;
    let mut d = MemDiskMut(&mut mbr);
    assert_eq!(Fat32Writer::open_any(&mut d).err(), Some(WriteError::NotWritable(Reason::WrongLabel)));
}

/// A disk with an MBR and two FAT32 partitions: first `a`, then `b`.
fn two_partitions(a: [u8; 11], b: [u8; 11]) -> (Vec<u8>, u64, u64) {
    let (va, vb) = (mkimg(a, 100, 1), mkimg(b, 100, 1));
    let start_a = 1u64;
    let start_b = start_a + (va.len() / SECTOR_SIZE) as u64;
    let mut disk = vec![0u8; SECTOR_SIZE];
    disk.extend_from_slice(&va);
    disk.extend_from_slice(&vb);
    for (i, start) in [start_a, start_b].into_iter().enumerate() {
        disk[446 + 16 * i + 4] = 0x0C;
        disk[446 + 16 * i + 8..446 + 16 * i + 12].copy_from_slice(&(start as u32).to_le_bytes());
    }
    disk[510] = 0x55;
    disk[511] = 0xAA;
    (disk, start_a, start_b)
}

#[test]
fn the_writer_uses_the_same_partition_as_the_reader_never_a_later_aeon_one() {
    // First partition is someone else's (read only), second is labelled AEON. The reader (`ls /usb`) shows the
    // first one, so a write must not silently go to the second one: it must be refused.
    let (mut disk, _, start_b) = two_partitions(*b"OTHER      ", AEON);
    let before = disk.clone();
    assert_eq!(Fat32Writer::open_any(&mut MemDiskMut(&mut disk)).err(), Some(WriteError::NotWritable(Reason::WrongLabel)));
    assert_eq!(disk, before);
    assert!(start_b > 1);
}

#[test]
fn with_an_aeon_first_partition_that_one_is_used() {
    let (mut disk, start_a, _) = two_partitions(AEON, *b"OTHER      ");
    let (_, base) = Fat32Writer::open_any(&mut MemDiskMut(&mut disk)).unwrap();
    assert_eq!(base, start_a);
}

#[test]
fn a_partition_the_reader_cannot_open_is_skipped_by_both() {
    // First partition is not a FAT volume at all; both reader and writer move on to the second.
    let (mut disk, start_a, start_b) = two_partitions(AEON, AEON);
    disk[start_a as usize * SECTOR_SIZE + 510] = 0; // break the first boot sector signature
    let (_, base) = Fat32Writer::open_any(&mut MemDiskMut(&mut disk)).unwrap();
    assert_eq!(base, start_b);
    assert!(Volume::open_any(&mut MemDisk(&disk)).is_ok());
}
