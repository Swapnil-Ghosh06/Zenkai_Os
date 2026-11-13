//! AHCI (SATA) disk driver: read only, polled, one command at a time.
//!
//! `probe` looks at every AHCI controller on the PCI bus and takes the first SATA disk it finds. Reads go
//! through a 64 KiB bounce buffer in DMA memory, so the caller's buffer needs no particular address or
//! alignment. Only IDENTIFY DEVICE and READ DMA EXT are ever sent (see `ahcifis.rs`), so this driver
//! cannot change what is on a disk. Command structures and decoding are pure and host-tested.
//!
//! Global state: `DISK` follows rule A in `docs/design/globals.md`: only boot code (`probe`) and the shell
//! task touch it, never an interrupt handler.

use core::ptr::{copy_nonoverlapping, write_bytes};
use core::sync::atomic::{fence, Ordering};

use crate::ahcifis::{self as fis, HbaCaps, PortKind};
use crate::block::{check_request, BlockDev, BlockError, SECTOR_SIZE};
use crate::global::Global;
use crate::mmio::{dma_frames, dma_page, rd32, wait_until, wr32, PAGE_BYTES};
use crate::{paging, pci};

/// Bounce buffer size in 4 KiB frames, and the most sectors one command moves.
const BOUNCE_FRAMES: usize = 16;
const MAX_SECTORS: usize = BOUNCE_FRAMES * PAGE_BYTES / SECTOR_SIZE;
/// Controllers looked at (the PCI list is longer than any real PC needs).
const MAX_CONTROLLERS: usize = 4;
/// Layout of the one DMA page that holds the command list, the received FIS area and the command table.
const FIS_OFFSET: u64 = 0x400;
const TABLE_OFFSET: u64 = 0x500;
const TABLE_BYTES: usize = 0x90;

pub struct AhciDisk {
    port: u8,
    /// Registers of the port.
    pb: u64,
    sectors: u64,
    model: [u8; 40],
    model_len: usize,
    /// DMA page: command list at 0, received FIS area at 0x400, command table at 0x500.
    page: u64,
    bounce: u64,
}

static DISK: Global<Option<AhciDisk>> = Global::new(None);

/// What `disks` shows about the disk.
pub struct DiskInfo {
    pub port: u8,
    pub sectors: u64,
    pub model: [u8; 40],
    pub model_len: usize,
}

impl DiskInfo {
    pub fn model_str(&self) -> &str {
        core::str::from_utf8(&self.model[..self.model_len]).unwrap_or("?")
    }
}

/// Starts or stops the command engine of a port (the FIS receive engine follows).
fn stop_engine(pb: u64) -> bool {
    let cmd = rd32(pb + fis::PORT_CMD);
    wr32(pb + fis::PORT_CMD, cmd & !(fis::CMD_START | fis::CMD_FIS_RECEIVE));
    wait_until(500, || rd32(pb + fis::PORT_CMD) & (fis::CMD_LIST_RUNNING | fis::CMD_FIS_RUNNING) == 0)
}

fn start_engine(pb: u64) -> bool {
    if !wait_until(1000, || rd32(pb + fis::PORT_TFD) & (fis::TFD_BUSY | fis::TFD_DRQ) == 0) {
        return false;
    }
    let cmd = rd32(pb + fis::PORT_CMD);
    wr32(pb + fis::PORT_CMD, cmd | fis::CMD_FIS_RECEIVE);
    let cmd = rd32(pb + fis::PORT_CMD);
    wr32(pb + fis::PORT_CMD, cmd | fis::CMD_START);
    true
}

impl AhciDisk {
    /// Sends one command whose data (at most one bounce buffer) lands in the bounce buffer.
    fn issue(&mut self, fis_bytes: &[u8; fis::FIS_LEN], bytes: u32) -> Result<(), BlockError> {
        let table = self.page + TABLE_OFFSET;
        let header = fis::command_header(1, table).ok_or(BlockError::Io)?;
        let prd = fis::prd_entry(self.bounce, bytes).ok_or(BlockError::BadRequest)?;
        let pb = self.pb;
        // SAFETY: `page` is a DMA page this driver owns; the command list (slot 0) is at its start and the
        // command table at 0x500, both inside the page. No command is running (CI is 0 between calls).
        unsafe {
            write_bytes(table as *mut u8, 0, TABLE_BYTES);
            copy_nonoverlapping(fis_bytes.as_ptr(), table as *mut u8, fis::FIS_LEN);
            copy_nonoverlapping(prd.as_ptr(), (table + 0x80) as *mut u8, prd.len());
            copy_nonoverlapping(header.as_ptr(), self.page as *mut u8, header.len());
        }
        if !wait_until(1000, || rd32(pb + fis::PORT_TFD) & (fis::TFD_BUSY | fis::TFD_DRQ) == 0) {
            return Err(BlockError::Timeout);
        }
        wr32(pb + fis::PORT_IS, 0xFFFF_FFFF);
        fence(Ordering::SeqCst);
        wr32(pb + fis::PORT_CI, 1);
        let finished = wait_until(5000, || rd32(pb + fis::PORT_CI) & 1 == 0 || rd32(pb + fis::PORT_IS) & fis::IS_TASK_FILE_ERROR != 0);
        if !finished {
            self.recover();
            return Err(BlockError::Timeout);
        }
        if rd32(pb + fis::PORT_IS) & fis::IS_TASK_FILE_ERROR != 0 || rd32(pb + fis::PORT_TFD) & fis::TFD_ERROR != 0 {
            self.recover();
            return Err(BlockError::Io);
        }
        fence(Ordering::Acquire);
        Ok(())
    }

    /// After an error or timeout: stop and restart the engine so the next command starts clean.
    fn recover(&mut self) {
        let pb = self.pb;
        let _ = stop_engine(pb);
        wr32(pb + fis::PORT_SERR, 0xFFFF_FFFF);
        wr32(pb + fis::PORT_IS, 0xFFFF_FFFF);
        let _ = start_engine(pb);
    }

    pub fn info(&self) -> DiskInfo {
        DiskInfo { port: self.port, sectors: self.sectors, model: self.model, model_len: self.model_len }
    }
}

impl BlockDev for AhciDisk {
    fn sectors(&self) -> u64 {
        self.sectors
    }

    fn read(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        check_request(self.sectors, lba, buf.len())?;
        let mut done = 0usize;
        while done < buf.len() {
            let n = ((buf.len() - done) / SECTOR_SIZE).min(MAX_SECTORS);
            let cmd = fis::read_dma_ext_fis(lba + (done / SECTOR_SIZE) as u64, n as u16).ok_or(BlockError::BadRequest)?;
            self.issue(&cmd, (n * SECTOR_SIZE) as u32)?;
            // SAFETY: the command finished, so the controller no longer writes the bounce buffer, which holds
            // `n * 512` valid bytes; the destination slice has at least that many bytes left.
            unsafe { copy_nonoverlapping(self.bounce as *const u8, buf[done..].as_mut_ptr(), n * SECTOR_SIZE) };
            done += n * SECTOR_SIZE;
        }
        Ok(())
    }
}

/// Brings up one SATA port: command list, FIS area, engine, IDENTIFY. `None` when the disk cannot be used.
fn init_port(abar: u64, caps: &HbaCaps, port: u8) -> Result<AhciDisk, &'static str> {
    let pb = fis::port_base(abar, port);
    if !stop_engine(pb) {
        return Err("port engine would not stop");
    }
    let page = dma_page(caps.s64a)?;
    let bounce = dma_frames(BOUNCE_FRAMES, caps.s64a)?;
    wr32(pb + fis::PORT_CLB, page as u32);
    wr32(pb + fis::PORT_CLBU, (page >> 32) as u32);
    let fb = page + FIS_OFFSET;
    wr32(pb + fis::PORT_FB, fb as u32);
    wr32(pb + fis::PORT_FBU, (fb >> 32) as u32);
    wr32(pb + fis::PORT_SERR, 0xFFFF_FFFF);
    wr32(pb + fis::PORT_IS, 0xFFFF_FFFF);
    if !start_engine(pb) {
        return Err("port stayed busy");
    }
    let mut disk = AhciDisk { port, pb, sectors: 0, model: [0; 40], model_len: 0, page, bounce };
    disk.issue(&fis::identify_fis(), 512).map_err(|_| "IDENTIFY DEVICE failed")?;
    let mut data = [0u8; 512];
    // SAFETY: the command finished; the bounce buffer holds the 512 bytes of the answer.
    unsafe { copy_nonoverlapping(bounce as *const u8, data.as_mut_ptr(), 512) };
    let id = fis::parse_identify(&data).map_err(|e| match e {
        fis::IdentifyError::NotAta => "not an ATA disk",
        fis::IdentifyError::NoLba48 => "no 48-bit addressing (a disk under 128 GiB is not supported)",
        fis::IdentifyError::SectorSize(_) => "sector size is not 512 bytes",
        fis::IdentifyError::Empty => "disk reports no sectors",
        fis::IdentifyError::TooShort => "short IDENTIFY answer",
    })?;
    disk.sectors = id.sectors;
    disk.model = id.model;
    disk.model_len = id.model_len;
    Ok(disk)
}

/// Looks for a SATA disk behind one controller.
fn scan_controller(d: &pci::Device) -> Result<Option<AhciDisk>, &'static str> {
    // Memory space and bus mastering on.
    let command = pci::config_read(d.bus, d.dev, d.func, 4);
    pci::config_write(d.bus, d.dev, d.func, 4, (command & 0xFFFF) | 0x6);
    let abar = pci::memory_bar(d, 5).ok_or("BAR 5 is not a memory BAR")?;
    if !(paging::map_mmio_uc(abar) && paging::map_mmio_uc(abar + 0x1FFF)) {
        return Err("cannot map the registers");
    }
    // Take the controller over from the firmware when it supports the handoff (CAP2 bit 0).
    if rd32(abar + 0x24) & 1 != 0 {
        let bohc = rd32(abar + fis::HBA_BOHC);
        if bohc & fis::BOHC_BIOS_OWNED != 0 {
            wr32(abar + fis::HBA_BOHC, bohc | fis::BOHC_OS_OWNED);
            if !wait_until(2000, || rd32(abar + fis::HBA_BOHC) & fis::BOHC_BIOS_OWNED == 0) {
                warn!("ahci", "firmware did not release the controller, continuing");
            }
        }
    }
    wr32(abar + fis::HBA_GHC, rd32(abar + fis::HBA_GHC) | fis::GHC_AHCI_ENABLE);
    let caps = HbaCaps::decode(rd32(abar + fis::HBA_CAP), rd32(abar + fis::HBA_PI));
    for port in 0..32u8 {
        if !caps.port_implemented(port) {
            continue;
        }
        let pb = fis::port_base(abar, port);
        match fis::classify(rd32(pb + fis::PORT_SSTS), rd32(pb + fis::PORT_SIG)) {
            PortKind::Sata => match init_port(abar, &caps, port) {
                Ok(disk) => return Ok(Some(disk)),
                Err(why) => warn!("ahci", "port {}: SATA disk not used: {}", port, why),
            },
            PortKind::NoDevice => {}
            other => info!("ahci", "port {}: {:?} is not touched", port, other),
        }
    }
    Ok(None)
}

/// Finds the first SATA disk on any AHCI controller (PCI class 01, subclass 06, interface 01). Called
/// once at boot, after the PCI scan.
pub fn probe() {
    let mut found = [None; MAX_CONTROLLERS];
    let mut n = 0;
    pci::for_each(|d| {
        let h = &d.header;
        if n < MAX_CONTROLLERS && h.class == 0x01 && h.subclass == 0x06 && h.prog_if == 0x01 {
            found[n] = Some(*d);
            n += 1;
        }
    });
    if n == 0 {
        info!("ahci", "no AHCI controller on the PCI bus");
        return;
    }
    for d in found.iter().flatten() {
        match scan_controller(d) {
            Ok(Some(disk)) => {
                let i = disk.info();
                info!("ahci", "disk on port {}: {}, {} sectors ({} MiB)", i.port, i.model_str(), i.sectors, i.sectors / 2048);
                // SAFETY: rule A: boot code, before the shell task exists; no interrupt handler touches DISK.
                *unsafe { DISK.get() } = Some(disk);
                return;
            }
            Ok(None) => {}
            Err(why) => warn!("ahci", "controller at {:02x}:{:02x}.{} not used: {}", d.bus, d.dev, d.func, why),
        }
    }
    info!("ahci", "no SATA disk found on {} controller(s)", n);
}

/// Runs `f` on the AHCI disk, if there is one.
pub fn with_disk<R>(f: impl FnOnce(&mut AhciDisk) -> R) -> Option<R> {
    // SAFETY: rule A: only the shell task and boot code reach DISK.
    let d = unsafe { DISK.get() }.as_mut()?;
    Some(f(d))
}

pub fn disk_info() -> Option<DiskInfo> {
    with_disk(|d| d.info())
}
