//! The block device interface: a disk is a row of 512-byte sectors that can be read. A virtio, NVMe or
//! AHCI driver implements [`BlockDev`]; the filesystem code only ever sees this trait.

pub const SECTOR_SIZE: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockError {
    /// There is no disk.
    NoDevice,
    /// The request reaches past the end of the disk or the buffer is not a whole number of sectors.
    BadRequest,
    /// The device did not finish in time.
    Timeout,
    /// The device reported a failure.
    Io,
}

pub trait BlockDev {
    /// Size of the disk in sectors.
    fn sectors(&self) -> u64;
    /// Reads `buf.len() / 512` sectors starting at `lba`. `buf.len()` must be a non-zero multiple of 512.
    fn read(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), BlockError>;
}

/// Checks a request against the disk size, so every driver rejects the same bad requests the same way.
pub fn check_request(total_sectors: u64, lba: u64, buf_len: usize) -> Result<usize, BlockError> {
    if buf_len == 0 || buf_len % SECTOR_SIZE != 0 {
        return Err(BlockError::BadRequest);
    }
    let n = (buf_len / SECTOR_SIZE) as u64;
    match lba.checked_add(n) {
        Some(end) if end <= total_sectors => Ok(n as usize),
        _ => Err(BlockError::BadRequest),
    }
}

/// A disk that lives in memory: used by the host tests, and handy for a RAM-backed volume later.
pub struct MemDisk<'a>(pub &'a [u8]);

impl BlockDev for MemDisk<'_> {
    fn sectors(&self) -> u64 {
        (self.0.len() / SECTOR_SIZE) as u64
    }

    fn read(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        check_request(self.sectors(), lba, buf.len())?;
        let start = lba as usize * SECTOR_SIZE;
        buf.copy_from_slice(&self.0[start..start + buf.len()]);
        Ok(())
    }
}

/// A disk that can also be written. Kept apart from [`BlockDev`] so that read-only code (and read-only
/// drivers such as AHCI today) can never be handed something that writes.
pub trait BlockWrite: BlockDev {
    /// Writes `buf.len() / 512` sectors starting at `lba`. `buf.len()` must be a non-zero multiple of 512.
    /// A refused request (out of range, odd length) must change nothing.
    fn write(&mut self, lba: u64, buf: &[u8]) -> Result<(), BlockError>;
}

/// A writable disk in memory, for the host tests of the FAT writer.
pub struct MemDiskMut<'a>(pub &'a mut [u8]);

impl BlockDev for MemDiskMut<'_> {
    fn sectors(&self) -> u64 {
        (self.0.len() / SECTOR_SIZE) as u64
    }

    fn read(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        MemDisk(&*self.0).read(lba, buf)
    }
}

impl BlockWrite for MemDiskMut<'_> {
    fn write(&mut self, lba: u64, buf: &[u8]) -> Result<(), BlockError> {
        check_request(self.sectors(), lba, buf.len())?;
        let start = lba as usize * SECTOR_SIZE;
        self.0[start..start + buf.len()].copy_from_slice(buf);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_checks() {
        assert_eq!(check_request(10, 0, 512), Ok(1));
        assert_eq!(check_request(10, 9, 512), Ok(1));
        assert_eq!(check_request(10, 0, 5120), Ok(10));
        assert_eq!(check_request(10, 10, 512), Err(BlockError::BadRequest));
        assert_eq!(check_request(10, 9, 1024), Err(BlockError::BadRequest));
        assert_eq!(check_request(10, 0, 0), Err(BlockError::BadRequest));
        assert_eq!(check_request(10, 0, 100), Err(BlockError::BadRequest));
        assert_eq!(check_request(10, u64::MAX, 512), Err(BlockError::BadRequest));
        assert_eq!(check_request(0, 0, 512), Err(BlockError::BadRequest));
    }

    #[test]
    fn mem_disk_reads_and_rejects() {
        let data: Vec<u8> = (0..2048u32).map(|i| (i / 512) as u8).collect();
        let mut d = MemDisk(&data);
        assert_eq!(d.sectors(), 4);
        let mut one = [0u8; 512];
        d.read(2, &mut one).unwrap();
        assert!(one.iter().all(|&b| b == 2));
        let mut two = [0u8; 1024];
        d.read(2, &mut two).unwrap();
        assert!(two[..512].iter().all(|&b| b == 2) && two[512..].iter().all(|&b| b == 3));
        assert_eq!(d.read(3, &mut two), Err(BlockError::BadRequest));
        assert_eq!(d.read(4, &mut one), Err(BlockError::BadRequest));
    }

    #[test]
    fn a_disk_smaller_than_a_sector_has_zero_sectors() {
        let tiny = [1u8; 100];
        let mut d = MemDisk(&tiny);
        assert_eq!(d.sectors(), 0);
        let mut one = [0u8; 512];
        assert_eq!(d.read(0, &mut one), Err(BlockError::BadRequest));
    }

    #[test]
    fn mem_disk_mut_writes_and_rejects() {
        let mut data = vec![0u8; 2048];
        let mut d = MemDiskMut(&mut data);
        assert_eq!(d.sectors(), 4);
        d.write(1, &[7u8; 1024]).unwrap();
        let mut back = [0u8; 512];
        d.read(2, &mut back).unwrap();
        assert!(back.iter().all(|&b| b == 7));
        d.read(0, &mut back).unwrap();
        assert!(back.iter().all(|&b| b == 0), "sector 0 must be untouched");
        // Bad requests change nothing.
        assert_eq!(d.write(3, &[9u8; 1024]), Err(BlockError::BadRequest));
        assert_eq!(d.write(0, &[9u8; 100]), Err(BlockError::BadRequest));
        assert_eq!(d.write(u64::MAX, &[9u8; 512]), Err(BlockError::BadRequest));
        assert_eq!(d.write(0, &[]), Err(BlockError::BadRequest));
        d.read(3, &mut back).unwrap();
        assert!(back.iter().all(|&b| b == 0), "a refused write must not change the disk");
    }
}
