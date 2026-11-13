//! Write-back block cache over a [`BlockWrite`] disk with an explicit `sync`.
//!
//! Pure, host-tested logic:
//! - 512-byte sector caching with LRU replacement;
//! - Writes stay in cache and mark slots dirty until `sync()` or eviction;
//! - Reads hit dirty cache before touching the disk;
//! - Tested against [`MemDiskMut`] on the host.

use crate::block::{check_request, BlockDev, BlockError, BlockWrite, SECTOR_SIZE};

pub const CACHE_SLOTS: usize = 64;

#[derive(Clone, Copy)]
pub struct CacheSlot {
    pub lba: u64,
    pub valid: bool,
    pub dirty: bool,
    pub last_used: u64,
    pub data: [u8; SECTOR_SIZE],
}

impl Default for CacheSlot {
    fn default() -> Self {
        Self {
            lba: 0,
            valid: false,
            dirty: false,
            last_used: 0,
            data: [0u8; SECTOR_SIZE],
        }
    }
}

pub struct BlockCache<D: BlockWrite, const N: usize = CACHE_SLOTS> {
    pub disk: D,
    pub slots: [CacheSlot; N],
    pub clock: u64,
}

impl<D: BlockWrite, const N: usize> BlockCache<D, N> {
    pub fn new(disk: D) -> Self {
        Self {
            disk,
            slots: [CacheSlot::default(); N],
            clock: 0,
        }
    }

    fn tick(&mut self) -> u64 {
        self.clock = self.clock.wrapping_add(1);
        self.clock
    }

    fn find_slot(&self, lba: u64) -> Option<usize> {
        self.slots.iter().position(|s| s.valid && s.lba == lba)
    }

    fn pick_slot_for(&mut self, lba: u64) -> Result<usize, BlockError> {
        if let Some(idx) = self.find_slot(lba) {
            return Ok(idx);
        }
        if let Some(idx) = self.slots.iter().position(|s| !s.valid) {
            return Ok(idx);
        }
        let mut oldest_idx = 0;
        let mut oldest_time = u64::MAX;
        for (i, s) in self.slots.iter().enumerate() {
            if s.last_used < oldest_time {
                oldest_time = s.last_used;
                oldest_idx = i;
            }
        }
        if self.slots[oldest_idx].valid && self.slots[oldest_idx].dirty {
            let slot_lba = self.slots[oldest_idx].lba;
            self.disk.write(slot_lba, &self.slots[oldest_idx].data)?;
            self.slots[oldest_idx].dirty = false;
        }
        self.slots[oldest_idx].valid = false;
        Ok(oldest_idx)
    }

    pub fn sync(&mut self) -> Result<(), BlockError> {
        for s in &mut self.slots {
            if s.valid && s.dirty {
                self.disk.write(s.lba, &s.data)?;
                s.dirty = false;
            }
        }
        Ok(())
    }

    pub fn has_dirty(&self) -> bool {
        self.slots.iter().any(|s| s.valid && s.dirty)
    }
}

impl<D: BlockWrite, const N: usize> BlockDev for BlockCache<D, N> {
    fn sectors(&self) -> u64 {
        self.disk.sectors()
    }

    fn read(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        check_request(self.sectors(), lba, buf.len())?;
        let count = buf.len() / SECTOR_SIZE;
        for i in 0..count {
            let sec_lba = lba + i as u64;
            let time = self.tick();
            let idx = self.pick_slot_for(sec_lba)?;
            if !self.slots[idx].valid {
                self.disk.read(sec_lba, &mut self.slots[idx].data)?;
                self.slots[idx].lba = sec_lba;
                self.slots[idx].valid = true;
                self.slots[idx].dirty = false;
            }
            self.slots[idx].last_used = time;
            let offset = i * SECTOR_SIZE;
            buf[offset..offset + SECTOR_SIZE].copy_from_slice(&self.slots[idx].data);
        }
        Ok(())
    }
}

impl<D: BlockWrite, const N: usize> BlockWrite for BlockCache<D, N> {
    fn write(&mut self, lba: u64, buf: &[u8]) -> Result<(), BlockError> {
        check_request(self.sectors(), lba, buf.len())?;
        let count = buf.len() / SECTOR_SIZE;
        for i in 0..count {
            let sec_lba = lba + i as u64;
            let time = self.tick();
            let idx = self.pick_slot_for(sec_lba)?;
            let offset = i * SECTOR_SIZE;
            self.slots[idx].data.copy_from_slice(&buf[offset..offset + SECTOR_SIZE]);
            self.slots[idx].lba = sec_lba;
            self.slots[idx].valid = true;
            self.slots[idx].dirty = true;
            self.slots[idx].last_used = time;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::MemDiskMut;

    #[test]
    fn write_caches_and_sync_flushes() {
        let mut storage = [0u8; 4096];
        let mut disk = BlockCache::<_, 4>::new(MemDiskMut(&mut storage));
        let data = [0xABu8; 512];
        disk.write(0, &data).unwrap();
        assert!(disk.has_dirty());

        // The underlying storage must NOT have been written yet.
        assert_eq!(disk.disk.0[..512], [0u8; 512]);

        // A read from the cache returns the dirty written data.
        let mut read_back = [0u8; 512];
        disk.read(0, &mut read_back).unwrap();
        assert_eq!(read_back, data);

        // Sync flushes to underlying storage.
        disk.sync().unwrap();
        assert!(!disk.has_dirty());
        assert_eq!(disk.disk.0[..512], data);
    }

    #[test]
    fn read_miss_caches_and_hits_cache_subsequently() {
        let mut storage = [0u8; 4096];
        storage[512..1024].copy_from_slice(&[0x42u8; 512]);
        let mut disk = BlockCache::<_, 4>::new(MemDiskMut(&mut storage));

        let mut buf = [0u8; 512];
        disk.read(1, &mut buf).unwrap();
        assert_eq!(buf, [0x42u8; 512]);

        // Change underlying storage directly behind the cache's back
        disk.disk.0[512..1024].fill(0);

        // Subsequent read hits the cache, returning original data
        let mut buf2 = [0u8; 512];
        disk.read(1, &mut buf2).unwrap();
        assert_eq!(buf2, [0x42u8; 512]);
    }

    #[test]
    fn lru_eviction_flushes_dirty_slot() {
        let mut storage = [0u8; 4096];
        let mut disk = BlockCache::<_, 2>::new(MemDiskMut(&mut storage));

        // Fill cache with sectors 0 and 1
        disk.write(0, &[0x11u8; 512]).unwrap();
        disk.write(1, &[0x22u8; 512]).unwrap();
        assert_eq!(disk.disk.0[..512], [0u8; 512]);
        assert_eq!(disk.disk.0[512..1024], [0u8; 512]);

        // Writing sector 2 must evict sector 0 (least recently used) and flush it!
        disk.write(2, &[0x33u8; 512]).unwrap();
        assert_eq!(disk.disk.0[..512], [0x11u8; 512], "evicted sector 0 was flushed");
        assert_eq!(disk.disk.0[512..1024], [0u8; 512], "sector 1 still cached");

        // Sync flushes remaining dirty sectors (1 and 2)
        disk.sync().unwrap();
        assert_eq!(disk.disk.0[512..1024], [0x22u8; 512]);
        assert_eq!(disk.disk.0[1024..1536], [0x33u8; 512]);
    }

    #[test]
    fn multi_sector_reads_and_writes() {
        let mut storage = [0u8; 4096];
        let mut disk = BlockCache::<_, 4>::new(MemDiskMut(&mut storage));

        let mut multi = [0u8; 1024];
        multi[..512].fill(0xAA);
        multi[512..].fill(0xBB);
        disk.write(1, &multi).unwrap();

        let mut read_multi = [0u8; 1024];
        disk.read(1, &mut read_multi).unwrap();
        assert_eq!(read_multi, multi);

        disk.sync().unwrap();
        assert_eq!(disk.disk.0[512..1536], multi);
    }

    #[test]
    fn bad_requests_are_refused() {
        let mut storage = [0u8; 2048]; // 4 sectors
        let mut disk = BlockCache::<_, 4>::new(MemDiskMut(&mut storage));

        let mut buf = [0u8; 512];
        assert_eq!(disk.read(4, &mut buf), Err(BlockError::BadRequest));
        assert_eq!(disk.write(4, &buf), Err(BlockError::BadRequest));
        assert_eq!(disk.read(0, &mut buf[..100]), Err(BlockError::BadRequest));
        assert_eq!(disk.write(0, &buf[..100]), Err(BlockError::BadRequest));
    }
}
