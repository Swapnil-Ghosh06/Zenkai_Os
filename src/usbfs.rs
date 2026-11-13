//! USB filesystem adapter connecting the shell to the pendrive via [`crate::fat`] and [`crate::fatw`].
//!
//! Pure, host-testable core logic with safety rules:
//! - Only a volume labelled `AEON` is writable (non-AEON labels are strictly read-only);
//! - Files inside `EFI/` and the `EFI/` directory itself cannot be written or removed;
//! - Paths under `/usb` map to the FAT32 volume's root.

use alloc::vec::Vec;

use crate::block::{BlockDev, BlockWrite};
use crate::fat::{DirEntry, FatError, Volume};
use crate::fatw::{Fat32Writer, Reason, WriteError};

pub const MOUNT_NAME: &str = "/usb";

/// Returns true if `path` targets the USB filesystem (`/usb` or `/usb/...`).
pub fn is_usb(path: &str) -> bool {
    path == MOUNT_NAME || path.starts_with("/usb/")
}

/// Returns the relative path on the USB volume for a `/usb` path.
pub fn rel_path(path: &str) -> &str {
    if path == MOUNT_NAME || path == "/usb/" {
        ""
    } else if let Some(rest) = path.strip_prefix("/usb/") {
        rest
    } else {
        path.strip_prefix('/').unwrap_or(path)
    }
}

/// Checks safety rules before any write operation.
pub fn check_write_safety(rel: &str) -> Result<(), &'static str> {
    let clean = rel.trim_matches('/');
    if clean.is_empty() {
        return Err("cannot operate on volume root as a file");
    }
    let first = clean.split('/').next().unwrap_or("");
    if first.eq_ignore_ascii_case("EFI") {
        return Err("writing inside EFI is prohibited");
    }
    Ok(())
}

/// Lists entries in a directory on the USB volume.
pub fn list_dir(dev: &mut dyn BlockDev, rel: &str, mut on_entry: impl FnMut(&DirEntry)) -> Result<(), &'static str> {
    let vol = Volume::open_any(dev).map_err(|_| "cannot open FAT volume")?;
    let path = if rel.is_empty() { "" } else { rel };
    vol.list(dev, path, |e| on_entry(e)).map_err(|e| match e {
        FatError::NotFound => "no such folder",
        FatError::NotADirectory => "not a directory",
        _ => "read failed",
    })
}

/// Checks if a path on the USB volume is an existing directory.
pub fn is_dir(dev: &mut dyn BlockDev, rel: &str) -> bool {
    if rel.is_empty() {
        return Volume::open_any(dev).is_ok();
    }
    let Ok(vol) = Volume::open_any(dev) else { return false };
    vol.find(dev, rel).is_ok_and(|e| e.is_dir)
}

/// Checks if a path on the USB volume is an existing file.
pub fn is_file(dev: &mut dyn BlockDev, rel: &str) -> bool {
    if rel.is_empty() {
        return false;
    }
    let Ok(vol) = Volume::open_any(dev) else { return false };
    vol.find(dev, rel).is_ok_and(|e| !e.is_dir)
}

/// Reads the entire content of a file from the USB volume (up to `limit` bytes).
pub fn read_file(dev: &mut dyn BlockDev, rel: &str, limit: usize) -> Result<Vec<u8>, &'static str> {
    if rel.is_empty() {
        return Err("is a directory");
    }
    let vol = Volume::open_any(dev).map_err(|_| "cannot open FAT volume")?;
    let entry = vol.find(dev, rel).map_err(|e| match e {
        FatError::NotFound => "no such file",
        FatError::NotAFile => "is a directory",
        _ => "lookup failed",
    })?;
    if entry.is_dir {
        return Err("is a directory");
    }
    let mut data = Vec::new();
    vol.read_file(dev, &entry, limit, &mut data).map_err(|e| match e {
        FatError::TooBig => "file too large",
        _ => "read failed",
    })?;
    Ok(data)
}

/// Writes `data` to a file on the USB volume.
pub fn write_file(dev: &mut dyn BlockWrite, rel: &str, data: &[u8], replace: bool) -> Result<(), &'static str> {
    check_write_safety(rel)?;
    let (mut writer, _base) = Fat32Writer::open_any(dev).map_err(map_write_err)?;
    writer.write_file(dev, rel, data, replace).map_err(map_write_err)
}

/// Creates a new directory on the USB volume.
pub fn mkdir(dev: &mut dyn BlockWrite, rel: &str) -> Result<(), &'static str> {
    check_write_safety(rel)?;
    let (mut writer, _base) = Fat32Writer::open_any(dev).map_err(map_write_err)?;
    writer.mkdir(dev, rel).map_err(map_write_err)
}

/// Removes a file on the USB volume.
pub fn remove_file(dev: &mut dyn BlockWrite, rel: &str) -> Result<(), &'static str> {
    check_write_safety(rel)?;
    let (mut writer, _base) = Fat32Writer::open_any(dev).map_err(map_write_err)?;
    writer.remove_file(dev, rel).map_err(map_write_err)
}

/// Removes an empty directory on the USB volume.
pub fn remove_dir(dev: &mut dyn BlockWrite, rel: &str) -> Result<(), &'static str> {
    check_write_safety(rel)?;
    let (mut writer, _base) = Fat32Writer::open_any(dev).map_err(map_write_err)?;
    writer.remove_dir(dev, rel).map_err(map_write_err)
}

fn map_write_err(e: WriteError) -> &'static str {
    match e {
        WriteError::NotWritable(Reason::WrongLabel) => "volume label is not ZENKAI or AEON (read only)",
        WriteError::NotWritable(Reason::NotFat32) => "not a FAT32 volume",
        WriteError::NotWritable(Reason::NoMirroring) => "volume mirroring disabled",
        WriteError::NotWritable(Reason::Damaged(_)) => "damaged volume",
        WriteError::Protected => "writing inside EFI is prohibited",
        WriteError::Exists => "already exists",
        WriteError::NotFound => "no such file or folder",
        WriteError::NotADirectory => "not a directory",
        WriteError::IsADirectory => "is a directory",
        WriteError::NotEmpty => "folder is not empty",
        WriteError::NoSpace | WriteError::DirFull => "volume full",
        WriteError::TooBig => "file too large",
        WriteError::BadName => "illegal name",
        WriteError::Corrupt => "corrupted filesystem",
        WriteError::Io(_) => "disk I/O error",
    }
}

// ---- UEFI Hardware Wrappers ----

#[cfg(target_os = "uefi")]
pub fn hw_list(rel: &str, on_entry: impl FnMut(&DirEntry)) -> Result<(), &'static str> {
    crate::xhci::with_disk(|d| list_dir(d, rel, on_entry)).ok_or("no USB drive connected")?
}

#[cfg(target_os = "uefi")]
pub fn hw_is_dir(rel: &str) -> bool {
    crate::xhci::with_disk(|d| is_dir(d, rel)).unwrap_or(false)
}

#[cfg(target_os = "uefi")]
pub fn hw_read(rel: &str, limit: usize) -> Result<Vec<u8>, &'static str> {
    crate::xhci::with_disk(|d| read_file(d, rel, limit)).ok_or("no USB drive connected")?
}

/// Every change to the stick is flushed from the cache before the command returns, so pulling the stick
/// out after a command loses nothing (`sync` is still there for the ones who like to be sure).
#[cfg(target_os = "uefi")]
fn flushed(r: Result<(), &'static str>) -> Result<(), &'static str> {
    r?;
    crate::xhci::sync().map_err(|_| "disk I/O error while saving to the stick")
}

#[cfg(target_os = "uefi")]
pub fn hw_write(rel: &str, data: &[u8], replace: bool) -> Result<(), &'static str> {
    flushed(crate::xhci::with_usb_write(|d| write_file(d, rel, data, replace)).ok_or("no USB drive connected")?)
}

#[cfg(target_os = "uefi")]
pub fn hw_mkdir(rel: &str) -> Result<(), &'static str> {
    flushed(crate::xhci::with_usb_write(|d| mkdir(d, rel)).ok_or("no USB drive connected")?)
}

#[cfg(target_os = "uefi")]
pub fn hw_remove_file(rel: &str) -> Result<(), &'static str> {
    flushed(crate::xhci::with_usb_write(|d| remove_file(d, rel)).ok_or("no USB drive connected")?)
}

#[cfg(target_os = "uefi")]
pub fn hw_remove_dir(rel: &str) -> Result<(), &'static str> {
    flushed(crate::xhci::with_usb_write(|d| remove_dir(d, rel)).ok_or("no USB drive connected")?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{MemDiskMut, SECTOR_SIZE};

    const AEON: [u8; 11] = *b"AEON       ";
    const OTHER: [u8; 11] = *b"NOAEON     ";
    const RESERVED: usize = 4;

    fn make_test_vol(label: [u8; 11]) -> Vec<u8> {
        let clusters = 100u32;
        let spc = 1u32;
        let fat_sectors = ((clusters as usize + 2) * 4).div_ceil(SECTOR_SIZE);
        let total = RESERVED + 2 * fat_sectors + clusters as usize * spc as usize;
        let mut img = alloc::vec![0u8; total * SECTOR_SIZE];
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

    #[test]
    fn path_helpers() {
        assert!(is_usb("/usb"));
        assert!(is_usb("/usb/"));
        assert!(is_usb("/usb/file.txt"));
        assert!(!is_usb("/"));
        assert!(!is_usb("/docs"));
        assert!(!is_usb("/usbx"));

        assert_eq!(rel_path("/usb"), "");
        assert_eq!(rel_path("/usb/"), "");
        assert_eq!(rel_path("/usb/hello.txt"), "hello.txt");
        assert_eq!(rel_path("/usb/sub/note.txt"), "sub/note.txt");
    }

    #[test]
    fn efi_protection() {
        assert_eq!(check_write_safety("EFI/boot.efi"), Err("writing inside EFI is prohibited"));
        assert_eq!(check_write_safety("efi/boot.efi"), Err("writing inside EFI is prohibited"));
        assert_eq!(check_write_safety("/efi/boot.efi"), Err("writing inside EFI is prohibited"));
        assert_eq!(check_write_safety("//EFI/boot.efi"), Err("writing inside EFI is prohibited"));
        assert_eq!(check_write_safety("EFI"), Err("writing inside EFI is prohibited"));
        assert_eq!(check_write_safety("/EFI"), Err("writing inside EFI is prohibited"));
        assert_eq!(check_write_safety("EFI/"), Err("writing inside EFI is prohibited"));
        assert_eq!(check_write_safety(""), Err("cannot operate on volume root as a file"));
        assert_eq!(check_write_safety("/"), Err("cannot operate on volume root as a file"));
        assert!(check_write_safety("docs/note.txt").is_ok());
        assert!(check_write_safety("/docs/note.txt").is_ok());
    }

    #[test]
    fn non_aeon_label_refuses_write_but_allows_read() {
        let mut storage = make_test_vol(OTHER);
        let mut dev = MemDiskMut(&mut storage);

        // Read works (empty volume list succeeds)
        assert!(list_dir(&mut dev, "", |_| {}).is_ok());

        // Write is refused due to wrong label
        assert_eq!(write_file(&mut dev, "test.txt", b"hello", false), Err("volume label is not AEON (read only)"));
    }

    #[test]
    fn write_and_read_roundtrip_on_aeon_volume() {
        let mut storage = make_test_vol(AEON);
        let mut dev = MemDiskMut(&mut storage);

        write_file(&mut dev, "note.txt", b"hello pendrive", false).unwrap();
        assert!(is_file(&mut dev, "note.txt"));
        assert!(!is_dir(&mut dev, "note.txt"));

        let content = read_file(&mut dev, "note.txt", 1024).unwrap();
        assert_eq!(content, b"hello pendrive");

        // Mkdir and remove
        mkdir(&mut dev, "docs").unwrap();
        assert!(is_dir(&mut dev, "docs"));
        remove_dir(&mut dev, "docs").unwrap();
        assert!(!is_dir(&mut dev, "docs"));

        remove_file(&mut dev, "note.txt").unwrap();
        assert!(!is_file(&mut dev, "note.txt"));
    }
}
