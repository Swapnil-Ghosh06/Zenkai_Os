//! AHCI (SATA) command structures and decoding: the Register Host-to-Device FIS, command headers, PRDT
//! entries, controller capability and port status decoding, and the IDENTIFY DEVICE answer. Pure and
//! host-tested; the register access is in `ahci.rs`. Only two commands are ever built: IDENTIFY DEVICE
//! and READ DMA EXT, so nothing here can write to a disk.

pub const CMD_IDENTIFY: u8 = 0xEC;
pub const CMD_READ_DMA_EXT: u8 = 0x25;

/// Length of a Register H2D FIS in bytes (5 dwords).
pub const FIS_LEN: usize = 20;
const FIS_TYPE_REG_H2D: u8 = 0x27;
/// Bit 7 of FIS byte 1: this FIS carries a command (not device control).
const FIS_COMMAND_BIT: u8 = 0x80;
/// Device register value that selects LBA addressing.
const DEVICE_LBA: u8 = 0x40;

/// Most bytes one PRDT entry can describe.
pub const PRD_MAX_BYTES: u32 = 4 * 1024 * 1024;

// ---- register offsets ----

pub const HBA_CAP: u64 = 0x00;
pub const HBA_GHC: u64 = 0x04;
pub const HBA_PI: u64 = 0x0C;
pub const HBA_BOHC: u64 = 0x28;
pub const GHC_AHCI_ENABLE: u32 = 1 << 31;
pub const BOHC_BIOS_OWNED: u32 = 1 << 0;
pub const BOHC_OS_OWNED: u32 = 1 << 1;

pub const PORT_CLB: u64 = 0x00;
pub const PORT_CLBU: u64 = 0x04;
pub const PORT_FB: u64 = 0x08;
pub const PORT_FBU: u64 = 0x0C;
pub const PORT_IS: u64 = 0x10;
pub const PORT_CMD: u64 = 0x18;
pub const PORT_TFD: u64 = 0x20;
pub const PORT_SIG: u64 = 0x24;
pub const PORT_SSTS: u64 = 0x28;
pub const PORT_SERR: u64 = 0x30;
pub const PORT_CI: u64 = 0x38;

pub const CMD_START: u32 = 1 << 0;
pub const CMD_SPIN_UP: u32 = 1 << 1;
pub const CMD_FIS_RECEIVE: u32 = 1 << 4;
pub const CMD_FIS_RUNNING: u32 = 1 << 14;
pub const CMD_LIST_RUNNING: u32 = 1 << 15;
pub const TFD_ERROR: u32 = 1 << 0;
pub const TFD_DRQ: u32 = 1 << 3;
pub const TFD_BUSY: u32 = 1 << 7;
/// Port interrupt status: task file error.
pub const IS_TASK_FILE_ERROR: u32 = 1 << 30;

/// Address of the registers of `port` given the HBA base (ABAR).
pub fn port_base(abar: u64, port: u8) -> u64 {
    abar + 0x100 + 0x80 * u64::from(port)
}

// ---- controller and port decoding ----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HbaCaps {
    /// Number of ports the controller has (not all need be implemented).
    pub ports: u8,
    /// Command slots per port.
    pub slots: u8,
    /// 64-bit DMA addressing supported.
    pub s64a: bool,
    /// Bit `n` set: port `n` is implemented.
    pub implemented: u32,
}

impl HbaCaps {
    /// `cap` is the CAP register, `pi` the ports-implemented register.
    pub fn decode(cap: u32, pi: u32) -> HbaCaps {
        HbaCaps { ports: (cap & 0x1F) as u8 + 1, slots: ((cap >> 8) & 0x1F) as u8 + 1, s64a: cap & (1 << 31) != 0, implemented: pi }
    }

    pub fn port_implemented(&self, port: u8) -> bool {
        port < 32 && self.implemented & (1 << port) != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortKind {
    /// Nothing plugged in, or the link is not up.
    NoDevice,
    /// An ATA disk (signature 0x00000101).
    Sata,
    Atapi,
    EnclosureBridge,
    PortMultiplier,
    Unknown,
}

/// What is on a port, from its SStatus and signature registers.
pub fn classify(ssts: u32, sig: u32) -> PortKind {
    // DET 3: device present and communication established; IPM 1: interface active.
    if ssts & 0xF != 3 || (ssts >> 8) & 0xF != 1 {
        return PortKind::NoDevice;
    }
    match sig {
        0x0000_0101 => PortKind::Sata,
        0xEB14_0101 => PortKind::Atapi,
        0xC33C_0101 => PortKind::EnclosureBridge,
        0x9669_0101 => PortKind::PortMultiplier,
        _ => PortKind::Unknown,
    }
}

// ---- building commands ----

fn h2d(command: u8, lba: u64, count: u16) -> [u8; FIS_LEN] {
    let mut f = [0u8; FIS_LEN];
    f[0] = FIS_TYPE_REG_H2D;
    f[1] = FIS_COMMAND_BIT;
    f[2] = command;
    f[4] = lba as u8;
    f[5] = (lba >> 8) as u8;
    f[6] = (lba >> 16) as u8;
    f[7] = DEVICE_LBA;
    f[8] = (lba >> 24) as u8;
    f[9] = (lba >> 32) as u8;
    f[10] = (lba >> 40) as u8;
    f[12] = count as u8;
    f[13] = (count >> 8) as u8;
    f
}

/// FIS for IDENTIFY DEVICE (the device answers with 512 bytes of data).
pub fn identify_fis() -> [u8; FIS_LEN] {
    let mut f = h2d(CMD_IDENTIFY, 0, 0);
    f[7] = 0; // IDENTIFY wants a zero device register
    f
}

/// FIS for READ DMA EXT of `sectors` (1 to 65535) from `lba` (48 bits). `None` for a request outside that.
pub fn read_dma_ext_fis(lba: u64, sectors: u16) -> Option<[u8; FIS_LEN]> {
    if sectors == 0 || lba >> 48 != 0 {
        return None;
    }
    Some(h2d(CMD_READ_DMA_EXT, lba, sectors))
}

/// Command list entry (32 bytes) for a read: FIS of 5 dwords, `prdt_entries` entries, and the physical
/// address of the command table, which must be 128-byte aligned.
pub fn command_header(prdt_entries: u16, table: u64) -> Option<[u8; 32]> {
    if table & 0x7F != 0 {
        return None;
    }
    let dw0 = (FIS_LEN / 4) as u32 | u32::from(prdt_entries) << 16;
    let mut h = [0u8; 32];
    h[0..4].copy_from_slice(&dw0.to_le_bytes());
    h[8..12].copy_from_slice(&(table as u32).to_le_bytes());
    h[12..16].copy_from_slice(&((table >> 32) as u32).to_le_bytes());
    Some(h)
}

/// Physical region descriptor (16 bytes): `bytes` from `buf`. Counts must be even and at most 4 MiB, and
/// the address even.
pub fn prd_entry(buf: u64, bytes: u32) -> Option<[u8; 16]> {
    if bytes == 0 || bytes > PRD_MAX_BYTES || bytes % 2 != 0 || buf % 2 != 0 {
        return None;
    }
    let mut e = [0u8; 16];
    e[0..4].copy_from_slice(&(buf as u32).to_le_bytes());
    e[4..8].copy_from_slice(&((buf >> 32) as u32).to_le_bytes());
    e[12..16].copy_from_slice(&(bytes - 1).to_le_bytes());
    Some(e)
}

// ---- IDENTIFY DEVICE ----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identify {
    pub sectors: u64,
    pub model: [u8; 40],
    pub model_len: usize,
}

impl Identify {
    pub fn model_str(&self) -> &str {
        core::str::from_utf8(&self.model[..self.model_len]).unwrap_or("?")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentifyError {
    /// Fewer than 512 bytes.
    TooShort,
    /// The device says it is not an ATA disk (ATAPI).
    NotAta,
    /// No 48-bit LBA: disks below 128 GiB only; not supported.
    NoLba48,
    /// A logical sector size other than 512 bytes.
    SectorSize(u32),
    /// Reports zero sectors.
    Empty,
}

fn word(d: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([d[2 * i], d[2 * i + 1]])
}

/// Decodes the 512-byte IDENTIFY DEVICE answer.
pub fn parse_identify(d: &[u8]) -> Result<Identify, IdentifyError> {
    if d.len() < 512 {
        return Err(IdentifyError::TooShort);
    }
    if word(d, 0) & (1 << 15) != 0 {
        return Err(IdentifyError::NotAta);
    }
    // Word 83: bit 15 clear and bit 14 set mean the word is valid; bit 10 is "48-bit address feature set".
    let w83 = word(d, 83);
    if w83 >> 14 != 1 || w83 & (1 << 10) == 0 {
        return Err(IdentifyError::NoLba48);
    }
    // Word 106: bit 14 set and bit 15 clear mean valid; bit 12 says the logical sector is longer than
    // 256 words, and then words 117 and 118 give its size in words.
    let w106 = word(d, 106);
    if w106 >> 14 == 1 && w106 & (1 << 12) != 0 {
        let bytes = (u32::from(word(d, 117)) | u32::from(word(d, 118)) << 16).saturating_mul(2);
        if bytes != 512 {
            return Err(IdentifyError::SectorSize(bytes));
        }
    }
    let sectors = u64::from(word(d, 100)) | u64::from(word(d, 101)) << 16 | u64::from(word(d, 102)) << 32 | u64::from(word(d, 103)) << 48;
    if sectors == 0 {
        return Err(IdentifyError::Empty);
    }
    // Words 27 to 46: the model, two characters per word, the first character in the high byte.
    let mut raw = [0u8; 40];
    for i in 0..20 {
        raw[2 * i] = d[2 * (27 + i) + 1];
        raw[2 * i + 1] = d[2 * (27 + i)];
    }
    let printable = |b: u8| (0x21..0x7F).contains(&b);
    let start = raw.iter().position(|&b| printable(b)).unwrap_or(0);
    let end = raw.iter().rposition(|&b| printable(b)).map_or(0, |i| i + 1);
    let mut model = [0u8; 40];
    let len = end.saturating_sub(start);
    model[..len].copy_from_slice(&raw[start..start + len]);
    Ok(Identify { sectors, model, model_len: len })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_dma_ext_fis_layout() {
        // LBA 0x0000_0102_0304_0506, 8 sectors.
        let f = read_dma_ext_fis(0x0000_0102_0304_0506, 8).unwrap();
        assert_eq!(f[0], 0x27, "register host to device FIS");
        assert_eq!(f[1], 0x80, "command bit");
        assert_eq!(f[2], 0x25, "READ DMA EXT");
        assert_eq!([f[4], f[5], f[6]], [0x06, 0x05, 0x04], "LBA bits 0 to 23");
        assert_eq!(f[7], 0x40, "device register: LBA mode");
        assert_eq!([f[8], f[9], f[10]], [0x03, 0x02, 0x01], "LBA bits 24 to 47");
        assert_eq!([f[12], f[13]], [8, 0], "sector count");
        assert_eq!([f[3], f[11], f[14], f[15], f[16], f[17], f[18], f[19]], [0; 8]);
    }

    #[test]
    fn read_requests_outside_the_command_are_refused() {
        assert!(read_dma_ext_fis(0, 0).is_none(), "0 sectors would mean 65536");
        assert!(read_dma_ext_fis(1 << 48, 1).is_none(), "LBA does not fit 48 bits");
        assert!(read_dma_ext_fis((1 << 48) - 1, 65535).is_some());
        let f = read_dma_ext_fis(0, 0x1234).unwrap();
        assert_eq!([f[12], f[13]], [0x34, 0x12]);
    }

    #[test]
    fn identify_fis_layout() {
        let f = identify_fis();
        assert_eq!((f[0], f[1], f[2], f[7]), (0x27, 0x80, 0xEC, 0));
        assert_eq!(f[4..7], [0, 0, 0]);
    }

    #[test]
    fn no_write_command_exists() {
        // The only command bytes this module can produce.
        assert_eq!(identify_fis()[2], CMD_IDENTIFY);
        assert_eq!(read_dma_ext_fis(0, 1).unwrap()[2], CMD_READ_DMA_EXT);
        assert_ne!(CMD_READ_DMA_EXT, 0x35, "WRITE DMA EXT");
    }

    #[test]
    fn command_header_layout() {
        let h = command_header(1, 0x0000_0001_2345_6780).unwrap();
        assert_eq!(u32::from_le_bytes(h[0..4].try_into().unwrap()), 5 | 1 << 16, "FIS length 5 dwords, one PRDT entry, a read");
        assert_eq!(u32::from_le_bytes(h[8..12].try_into().unwrap()), 0x2345_6780);
        assert_eq!(u32::from_le_bytes(h[12..16].try_into().unwrap()), 1);
        assert_eq!(h[16..], [0u8; 16]);
        assert!(command_header(1, 0x1040).is_none(), "the table must be 128-byte aligned");
    }

    #[test]
    fn prd_entry_layout_and_limits() {
        let e = prd_entry(0x0000_0002_0000_1000, 4096).unwrap();
        assert_eq!(u32::from_le_bytes(e[0..4].try_into().unwrap()), 0x1000);
        assert_eq!(u32::from_le_bytes(e[4..8].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(e[12..16].try_into().unwrap()), 4095, "byte count minus one, no interrupt bit");
        assert!(prd_entry(0x1000, 0).is_none());
        assert!(prd_entry(0x1000, 3).is_none(), "odd counts are not allowed");
        assert!(prd_entry(0x1001, 512).is_none(), "odd addresses are not allowed");
        assert!(prd_entry(0x1000, PRD_MAX_BYTES + 2).is_none());
        assert!(prd_entry(0x1000, PRD_MAX_BYTES).is_some());
    }

    #[test]
    fn controller_capabilities() {
        // 6 ports (NP = 5), 32 command slots (NCS = 31), 64-bit addressing; ports 0, 1 and 5 implemented.
        let c = HbaCaps::decode(1 << 31 | 31 << 8 | 5, 0b100011);
        assert_eq!((c.ports, c.slots, c.s64a), (6, 32, true));
        assert!(c.port_implemented(0) && c.port_implemented(1) && c.port_implemented(5));
        assert!(!c.port_implemented(2) && !c.port_implemented(31) && !c.port_implemented(40));
        let c = HbaCaps::decode(0, 1);
        assert_eq!((c.ports, c.slots, c.s64a), (1, 1, false));
    }

    #[test]
    fn port_classification() {
        let up = 3 | 1 << 8; // DET 3, IPM 1
        assert_eq!(classify(up, 0x0000_0101), PortKind::Sata);
        assert_eq!(classify(up, 0xEB14_0101), PortKind::Atapi);
        assert_eq!(classify(up, 0xC33C_0101), PortKind::EnclosureBridge);
        assert_eq!(classify(up, 0x9669_0101), PortKind::PortMultiplier);
        assert_eq!(classify(up, 0xFFFF_FFFF), PortKind::Unknown);
        assert_eq!(classify(0, 0x0000_0101), PortKind::NoDevice, "no link");
        assert_eq!(classify(1, 0x0000_0101), PortKind::NoDevice, "device seen but no communication");
        assert_eq!(classify(3 | 2 << 8, 0x0000_0101), PortKind::NoDevice, "interface in partial power state");
    }

    /// An IDENTIFY answer for a 1 GiB disk called "QEMU HARDDISK" (name padded with spaces).
    fn identify_block() -> [u8; 512] {
        let mut d = [0u8; 512];
        let mut set = |i: usize, v: u16| d[2 * i..2 * i + 2].copy_from_slice(&v.to_le_bytes());
        set(83, 1 << 14 | 1 << 10);
        set(100, 0x0000);
        set(101, 0x0020); // 0x0020_0000 sectors = 1 GiB
        let name = b"QEMU HARDDISK                           ";
        for i in 0..20 {
            set(27 + i, u16::from(name[2 * i]) << 8 | u16::from(name[2 * i + 1]));
        }
        d
    }

    #[test]
    fn identify_of_a_normal_disk() {
        let id = parse_identify(&identify_block()).unwrap();
        assert_eq!(id.sectors, 0x0020_0000);
        assert_eq!(id.model_str(), "QEMU HARDDISK");
    }

    #[test]
    fn identify_reads_the_sector_count_from_four_words() {
        let mut d = identify_block();
        d[200..208].copy_from_slice(&0x0001_0002_0003_0004u64.to_le_bytes());
        assert_eq!(parse_identify(&d).unwrap().sectors, 0x0001_0002_0003_0004);
    }

    #[test]
    fn identify_refuses_what_it_cannot_read() {
        assert_eq!(parse_identify(&[0u8; 100]), Err(IdentifyError::TooShort));
        let mut d = identify_block();
        d[1] = 0x80; // word 0 bit 15
        assert_eq!(parse_identify(&d), Err(IdentifyError::NotAta));
        let mut d = identify_block();
        d[166..168].copy_from_slice(&(1u16 << 14).to_le_bytes()); // word 83 without the 48-bit bit
        assert_eq!(parse_identify(&d), Err(IdentifyError::NoLba48));
        let mut d = identify_block();
        d[166..168].copy_from_slice(&0u16.to_le_bytes()); // word 83 not valid
        assert_eq!(parse_identify(&d), Err(IdentifyError::NoLba48));
        let mut d = identify_block();
        d[202..204].copy_from_slice(&0u16.to_le_bytes());
        assert_eq!(parse_identify(&d), Err(IdentifyError::Empty));
    }

    #[test]
    fn identify_refuses_4k_sectors_but_accepts_the_explicit_512() {
        let mut d = identify_block();
        d[212..214].copy_from_slice(&(1u16 << 14 | 1 << 12).to_le_bytes()); // word 106: long logical sectors
        d[234..236].copy_from_slice(&2048u16.to_le_bytes()); // word 117: 2048 words = 4096 bytes
        assert_eq!(parse_identify(&d), Err(IdentifyError::SectorSize(4096)));
        d[234..236].copy_from_slice(&256u16.to_le_bytes()); // 256 words = 512 bytes
        assert!(parse_identify(&d).is_ok());
    }

    #[test]
    fn model_with_no_printable_characters_is_empty_not_a_panic() {
        let mut d = identify_block();
        d[54..94].fill(0);
        let id = parse_identify(&d).unwrap();
        assert_eq!(id.model_str(), "");
    }

    #[test]
    fn port_register_addresses() {
        assert_eq!(port_base(0x1000, 0), 0x1100);
        assert_eq!(port_base(0x1000, 3), 0x1000 + 0x100 + 3 * 0x80);
    }
}
