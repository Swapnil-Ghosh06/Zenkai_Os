//! USB Mass Storage Class (MSC) Bulk-Only Transport (BOT) and SCSI command logic.
//!
//! Pure, host-tested logic for USB mass-storage storage devices:
//! - Command Block Wrapper (CBW) and Command Status Wrapper (CSW) encode/decode;
//! - SCSI commands: INQUIRY (0x12), TEST UNIT READY (0x00), REQUEST SENSE (0x03),
//!   READ CAPACITY(10) (0x25), READ(10) (0x28), and WRITE(10) (0x2A);
//! - Sense data decoding (Sense Key, ASC, ASCQ);
//! - Capacity math and validation;
//! - Hostile device checks (signature, tag, truncation, zero block size, absurd capacity).
//!
//! No `unsafe`, no hardware registers.

pub const CBW_SIGNATURE: u32 = 0x43425355; // "USBC" in little-endian
pub const CBW_LEN: usize = 31;
pub const CSW_SIGNATURE: u32 = 0x53425355; // "USBS" in little-endian
pub const CSW_LEN: usize = 13;

pub const SCSI_TEST_UNIT_READY: u8 = 0x00;
pub const SCSI_REQUEST_SENSE: u8 = 0x03;
pub const SCSI_INQUIRY: u8 = 0x12;
pub const SCSI_READ_CAPACITY_10: u8 = 0x25;
pub const SCSI_READ_10: u8 = 0x28;
pub const SCSI_WRITE_10: u8 = 0x2A;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Out = 0x00,
    In = 0x80,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CswStatus {
    Passed = 0x00,
    Failed = 0x01,
    PhaseError = 0x02,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MsdError {
    ShortData,
    BadSignature,
    TagMismatch { expected: u32, actual: u32 },
    BadStatus(u8),
    BadBlockSize(u32),
    AbsurdCapacity,
    UnsupportedDeviceType(u8),
    BadResponseCode(u8),
    InvalidCdbLength(usize),
    Overflow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cbw {
    pub tag: u32,
    pub data_len: u32,
    pub direction: Direction,
    pub lun: u8,
    pub cdb_len: u8,
    pub cdb: [u8; 16],
}

impl Cbw {
    pub fn new(tag: u32, data_len: u32, direction: Direction, lun: u8, cdb: &[u8]) -> Result<Self, MsdError> {
        if cdb.is_empty() || cdb.len() > 16 {
            return Err(MsdError::InvalidCdbLength(cdb.len()));
        }
        let mut cdb_arr = [0u8; 16];
        cdb_arr[..cdb.len()].copy_from_slice(cdb);
        Ok(Self {
            tag,
            data_len,
            direction,
            lun: lun & 0x0F,
            cdb_len: cdb.len() as u8,
            cdb: cdb_arr,
        })
    }

    pub fn encode(&self) -> [u8; CBW_LEN] {
        let mut buf = [0u8; CBW_LEN];
        buf[0..4].copy_from_slice(&CBW_SIGNATURE.to_le_bytes());
        buf[4..8].copy_from_slice(&self.tag.to_le_bytes());
        buf[8..12].copy_from_slice(&self.data_len.to_le_bytes());
        buf[12] = self.direction as u8;
        buf[13] = self.lun & 0x0F;
        buf[14] = self.cdb_len & 0x1F;
        buf[15..31].copy_from_slice(&self.cdb);
        buf
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Csw {
    pub tag: u32,
    pub data_residue: u32,
    pub status: CswStatus,
}

impl Csw {
    pub fn decode(bytes: &[u8], expected_tag: u32) -> Result<Self, MsdError> {
        if bytes.len() < CSW_LEN {
            return Err(MsdError::ShortData);
        }
        let sig = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if sig != CSW_SIGNATURE {
            return Err(MsdError::BadSignature);
        }
        let tag = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        if tag != expected_tag {
            return Err(MsdError::TagMismatch { expected: expected_tag, actual: tag });
        }
        let data_residue = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        let status = match bytes[12] {
            0x00 => CswStatus::Passed,
            0x01 => CswStatus::Failed,
            0x02 => CswStatus::PhaseError,
            other => return Err(MsdError::BadStatus(other)),
        };
        Ok(Self { tag, data_residue, status })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InquiryData {
    pub peripheral_device_type: u8,
    pub removable: bool,
    pub vendor: [u8; 8],
    pub product: [u8; 16],
    pub revision: [u8; 4],
}

impl InquiryData {
    pub fn decode(bytes: &[u8]) -> Result<Self, MsdError> {
        if bytes.len() < 36 {
            return Err(MsdError::ShortData);
        }
        let pdt = bytes[0] & 0x1F;
        // Direct access block devices (e.g. magnetic disks, flash drives) have device type 0x00.
        if pdt != 0x00 {
            return Err(MsdError::UnsupportedDeviceType(pdt));
        }
        let removable = (bytes[1] & 0x80) != 0;
        let mut vendor = [0u8; 8];
        vendor.copy_from_slice(&bytes[8..16]);
        let mut product = [0u8; 16];
        product.copy_from_slice(&bytes[16..32]);
        let mut revision = [0u8; 4];
        revision.copy_from_slice(&bytes[32..36]);
        Ok(Self { peripheral_device_type: pdt, removable, vendor, product, revision })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SenseData {
    pub response_code: u8,
    pub sense_key: u8,
    pub asc: u8,
    pub ascq: u8,
}

impl SenseData {
    pub fn decode(bytes: &[u8]) -> Result<Self, MsdError> {
        if bytes.len() < 18 {
            return Err(MsdError::ShortData);
        }
        let code = bytes[0] & 0x7F;
        if !(0x70..=0x73).contains(&code) {
            return Err(MsdError::BadResponseCode(bytes[0]));
        }
        let sense_key = bytes[2] & 0x0F;
        let asc = bytes[12];
        let ascq = bytes[13];
        Ok(Self { response_code: bytes[0], sense_key, asc, ascq })
    }

    pub fn is_no_sense(&self) -> bool {
        self.sense_key == 0x00
    }

    pub fn is_not_ready(&self) -> bool {
        self.sense_key == 0x02
    }

    pub fn is_unit_attention(&self) -> bool {
        self.sense_key == 0x06
    }

    pub fn sense_key_name(&self) -> &'static str {
        match self.sense_key {
            0x00 => "No Sense",
            0x01 => "Recovered Error",
            0x02 => "Not Ready",
            0x03 => "Medium Error",
            0x04 => "Hardware Error",
            0x05 => "Illegal Request",
            0x06 => "Unit Attention",
            0x07 => "Data Protect",
            0x08 => "Blank Check",
            0x09 => "Vendor Specific",
            0x0A => "Copy Aborted",
            0x0B => "Aborted Command",
            0x0E => "Miscompare",
            _ => "Unknown Sense",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capacity {
    pub last_lba: u32,
    pub block_size: u32,
}

impl Capacity {
    pub fn decode(bytes: &[u8]) -> Result<Self, MsdError> {
        if bytes.len() < 8 {
            return Err(MsdError::ShortData);
        }
        let last_lba = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let block_size = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        if last_lba == 0xFFFF_FFFF {
            return Err(MsdError::AbsurdCapacity);
        }
        if block_size != 512 {
            return Err(MsdError::BadBlockSize(block_size));
        }
        Ok(Self { last_lba, block_size })
    }

    pub fn total_sectors(&self) -> u64 {
        u64::from(self.last_lba) + 1
    }

    pub fn total_bytes(&self) -> Result<u64, MsdError> {
        self.total_sectors()
            .checked_mul(u64::from(self.block_size))
            .ok_or(MsdError::Overflow)
    }
}

pub fn scsi_test_unit_ready(tag: u32, lun: u8) -> Cbw {
    let cdb = [SCSI_TEST_UNIT_READY, 0, 0, 0, 0, 0];
    Cbw::new(tag, 0, Direction::Out, lun, &cdb).expect("valid 6-byte CDB")
}

pub fn scsi_request_sense(tag: u32, lun: u8, alloc_len: u8) -> Cbw {
    let cdb = [SCSI_REQUEST_SENSE, 0, 0, 0, alloc_len, 0];
    Cbw::new(tag, u32::from(alloc_len), Direction::In, lun, &cdb).expect("valid 6-byte CDB")
}

pub fn scsi_inquiry(tag: u32, lun: u8, alloc_len: u8) -> Cbw {
    let cdb = [SCSI_INQUIRY, 0, 0, 0, alloc_len, 0];
    Cbw::new(tag, u32::from(alloc_len), Direction::In, lun, &cdb).expect("valid 6-byte CDB")
}

pub fn scsi_read_capacity_10(tag: u32, lun: u8) -> Cbw {
    let cdb = [SCSI_READ_CAPACITY_10, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    Cbw::new(tag, 8, Direction::In, lun, &cdb).expect("valid 10-byte CDB")
}

pub fn scsi_read_10(tag: u32, lun: u8, lba: u32, sectors: u16, block_size: u32) -> Result<Cbw, MsdError> {
    if sectors == 0 {
        return Err(MsdError::ShortData);
    }
    let data_len = u32::from(sectors).checked_mul(block_size).ok_or(MsdError::Overflow)?;
    let [l3, l2, l1, l0] = lba.to_be_bytes();
    let [s1, s0] = sectors.to_be_bytes();
    let cdb = [SCSI_READ_10, 0, l3, l2, l1, l0, 0, s1, s0, 0];
    Cbw::new(tag, data_len, Direction::In, lun, &cdb)
}

pub fn scsi_write_10(tag: u32, lun: u8, lba: u32, sectors: u16, block_size: u32) -> Result<Cbw, MsdError> {
    if sectors == 0 {
        return Err(MsdError::ShortData);
    }
    let data_len = u32::from(sectors).checked_mul(block_size).ok_or(MsdError::Overflow)?;
    let [l3, l2, l1, l0] = lba.to_be_bytes();
    let [s1, s0] = sectors.to_be_bytes();
    let cdb = [SCSI_WRITE_10, 0, l3, l2, l1, l0, 0, s1, s0, 0];
    Cbw::new(tag, data_len, Direction::Out, lun, &cdb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cbw_encode_roundtrip() {
        let cbw = Cbw::new(0x1234_5678, 512, Direction::In, 0, &[SCSI_READ_10, 0, 0, 0, 0, 0, 0, 0, 1, 0]).unwrap();
        let bytes = cbw.encode();
        assert_eq!(&bytes[0..4], b"USBC");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 0x1234_5678);
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 512);
        assert_eq!(bytes[12], 0x80); // In
        assert_eq!(bytes[13], 0);    // LUN 0
        assert_eq!(bytes[14], 10);   // CDB len
        assert_eq!(bytes[15], SCSI_READ_10);
    }

    #[test]
    fn cbw_rejects_empty_or_too_long_cdb() {
        assert_eq!(Cbw::new(1, 0, Direction::Out, 0, &[]), Err(MsdError::InvalidCdbLength(0)));
        assert_eq!(Cbw::new(1, 0, Direction::Out, 0, &[0; 17]), Err(MsdError::InvalidCdbLength(17)));
    }

    #[test]
    fn csw_decode_success() {
        let mut raw = [0u8; 13];
        raw[0..4].copy_from_slice(b"USBS");
        raw[4..8].copy_from_slice(&0xCAFE_BABE_u32.to_le_bytes());
        raw[8..12].copy_from_slice(&0_u32.to_le_bytes()); // residue 0
        raw[12] = 0x00; // Passed
        let csw = Csw::decode(&raw, 0xCAFE_BABE).unwrap();
        assert_eq!(csw.tag, 0xCAFE_BABE);
        assert_eq!(csw.data_residue, 0);
        assert_eq!(csw.status, CswStatus::Passed);
    }

    #[test]
    fn csw_decode_failed_and_phase_error() {
        let mut raw = [0u8; 13];
        raw[0..4].copy_from_slice(b"USBS");
        raw[4..8].copy_from_slice(&1234u32.to_le_bytes());
        raw[8..12].copy_from_slice(&512u32.to_le_bytes()); // residue 512
        raw[12] = 0x01; // Failed
        let csw = Csw::decode(&raw, 1234).unwrap();
        assert_eq!(csw.status, CswStatus::Failed);
        assert_eq!(csw.data_residue, 512);

        raw[12] = 0x02; // Phase Error
        let csw2 = Csw::decode(&raw, 1234).unwrap();
        assert_eq!(csw2.status, CswStatus::PhaseError);
    }

    #[test]
    fn csw_hostile_checks() {
        let mut raw = [0u8; 13];
        raw[0..4].copy_from_slice(b"USBS");
        raw[4..8].copy_from_slice(&100u32.to_le_bytes());
        raw[12] = 0;

        // Short data
        assert_eq!(Csw::decode(&raw[..12], 100), Err(MsdError::ShortData));
        assert_eq!(Csw::decode(&[], 100), Err(MsdError::ShortData));

        // Wrong signature
        let mut bad_sig = raw;
        bad_sig[0..4].copy_from_slice(b"USBC");
        assert_eq!(Csw::decode(&bad_sig, 100), Err(MsdError::BadSignature));

        // Tag mismatch
        assert_eq!(Csw::decode(&raw, 999), Err(MsdError::TagMismatch { expected: 999, actual: 100 }));

        // Bad status (> 2)
        let mut bad_status = raw;
        bad_status[12] = 0x03;
        assert_eq!(Csw::decode(&bad_status, 100), Err(MsdError::BadStatus(3)));
        bad_status[12] = 0xFF;
        assert_eq!(Csw::decode(&bad_status, 100), Err(MsdError::BadStatus(0xFF)));
    }

    #[test]
    fn scsi_test_unit_ready_layout() {
        let cbw = scsi_test_unit_ready(42, 0);
        assert_eq!(cbw.tag, 42);
        assert_eq!(cbw.data_len, 0);
        assert_eq!(cbw.direction, Direction::Out);
        assert_eq!(cbw.cdb_len, 6);
        assert_eq!(&cbw.cdb[..6], &[0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn scsi_request_sense_and_decoding() {
        let cbw = scsi_request_sense(10, 0, 18);
        assert_eq!(cbw.tag, 10);
        assert_eq!(cbw.data_len, 18);
        assert_eq!(cbw.direction, Direction::In);
        assert_eq!(cbw.cdb_len, 6);
        assert_eq!(&cbw.cdb[..6], &[0x03, 0, 0, 0, 18, 0]);

        // Decode valid sense response: Not Ready (0x02), Medium Not Present (0x3A, 0x00)
        let mut sense_buf = [0u8; 18];
        sense_buf[0] = 0x70; // Current errors, fixed format
        sense_buf[2] = 0x02; // Sense key: Not Ready
        sense_buf[12] = 0x3A; // ASC
        sense_buf[13] = 0x00; // ASCQ
        let sense = SenseData::decode(&sense_buf).unwrap();
        assert_eq!(sense.response_code, 0x70);
        assert_eq!(sense.sense_key, 0x02);
        assert!(sense.is_not_ready());
        assert!(!sense.is_no_sense());
        assert_eq!(sense.asc, 0x3A);
        assert_eq!(sense.ascq, 0x00);
        assert_eq!(sense.sense_key_name(), "Not Ready");

        // Hostile / short sense data
        assert_eq!(SenseData::decode(&sense_buf[..17]), Err(MsdError::ShortData));
        // Bad response code
        let mut bad_code = sense_buf;
        bad_code[0] = 0x00;
        assert_eq!(SenseData::decode(&bad_code), Err(MsdError::BadResponseCode(0x00)));
    }

    #[test]
    fn scsi_inquiry_and_decoding() {
        let cbw = scsi_inquiry(77, 0, 36);
        assert_eq!(cbw.tag, 77);
        assert_eq!(cbw.data_len, 36);
        assert_eq!(cbw.direction, Direction::In);
        assert_eq!(cbw.cdb_len, 6);
        assert_eq!(&cbw.cdb[..6], &[0x12, 0, 0, 0, 36, 0]);

        let mut inq_buf = [0u8; 36];
        inq_buf[0] = 0x00; // Direct access block device
        inq_buf[1] = 0x80; // Removable media
        inq_buf[8..16].copy_from_slice(b"SanDisk ");
        inq_buf[16..32].copy_from_slice(b"Ultra USB 3.0   ");
        inq_buf[32..36].copy_from_slice(b"1.00");
        let inq = InquiryData::decode(&inq_buf).unwrap();
        assert_eq!(inq.peripheral_device_type, 0x00);
        assert!(inq.removable);
        assert_eq!(&inq.vendor, b"SanDisk ");
        assert_eq!(&inq.product, b"Ultra USB 3.0   ");
        assert_eq!(&inq.revision, b"1.00");

        // Hostile: short inquiry data
        assert_eq!(InquiryData::decode(&inq_buf[..35]), Err(MsdError::ShortData));

        // Hostile: device type not direct-access block device (e.g. CD-ROM 0x05)
        let mut bad_type = inq_buf;
        bad_type[0] = 0x05;
        assert_eq!(InquiryData::decode(&bad_type), Err(MsdError::UnsupportedDeviceType(0x05)));
    }

    #[test]
    fn scsi_read_capacity_10_and_decoding() {
        let cbw = scsi_read_capacity_10(99, 0);
        assert_eq!(cbw.tag, 99);
        assert_eq!(cbw.data_len, 8);
        assert_eq!(cbw.direction, Direction::In);
        assert_eq!(cbw.cdb_len, 10);
        assert_eq!(&cbw.cdb[..10], &[0x25, 0, 0, 0, 0, 0, 0, 0, 0, 0]);

        // 16 GB drive: 31,266,815 last LBA (31,266,816 sectors), 512 bytes/sector
        let mut cap_buf = [0u8; 8];
        cap_buf[0..4].copy_from_slice(&31_266_815_u32.to_be_bytes());
        cap_buf[4..8].copy_from_slice(&512_u32.to_be_bytes());
        let cap = Capacity::decode(&cap_buf).unwrap();
        assert_eq!(cap.last_lba, 31_266_815);
        assert_eq!(cap.block_size, 512);
        assert_eq!(cap.total_sectors(), 31_266_816);
        assert_eq!(cap.total_bytes(), Ok(31_266_816 * 512));

        // Hostile checks:
        // 1. Short data
        assert_eq!(Capacity::decode(&cap_buf[..7]), Err(MsdError::ShortData));
        // 2. Zero block size
        let mut zero_bs = cap_buf;
        zero_bs[4..8].copy_from_slice(&0_u32.to_be_bytes());
        assert_eq!(Capacity::decode(&zero_bs), Err(MsdError::BadBlockSize(0)));
        // 3. Non-512 block size (e.g. 2048 or 4096)
        let mut odd_bs = cap_buf;
        odd_bs[4..8].copy_from_slice(&4096_u32.to_be_bytes());
        assert_eq!(Capacity::decode(&odd_bs), Err(MsdError::BadBlockSize(4096)));
        // 4. Absurd capacity (0xFFFFFFFF LBA)
        let mut max_lba = cap_buf;
        max_lba[0..4].copy_from_slice(&0xFFFF_FFFF_u32.to_be_bytes());
        assert_eq!(Capacity::decode(&max_lba), Err(MsdError::AbsurdCapacity));
    }

    #[test]
    fn scsi_read_10_and_write_10_encoding() {
        // Read 8 sectors starting at LBA 0x0012_3456
        let cbw = scsi_read_10(1, 0, 0x0012_3456, 8, 512).unwrap();
        assert_eq!(cbw.data_len, 8 * 512);
        assert_eq!(cbw.direction, Direction::In);
        assert_eq!(cbw.cdb_len, 10);
        assert_eq!(cbw.cdb[0], SCSI_READ_10);
        assert_eq!(cbw.cdb[1], 0);
        assert_eq!(&cbw.cdb[2..6], &[0x00, 0x12, 0x34, 0x56]); // Big endian LBA
        assert_eq!(cbw.cdb[6], 0);
        assert_eq!(&cbw.cdb[7..9], &[0x00, 0x08]); // Big endian sectors
        assert_eq!(cbw.cdb[9], 0);

        // Write 4 sectors starting at LBA 100
        let cbw_w = scsi_write_10(2, 0, 100, 4, 512).unwrap();
        assert_eq!(cbw_w.data_len, 4 * 512);
        assert_eq!(cbw_w.direction, Direction::Out);
        assert_eq!(cbw_w.cdb_len, 10);
        assert_eq!(cbw_w.cdb[0], SCSI_WRITE_10);
        assert_eq!(&cbw_w.cdb[2..6], &[0x00, 0x00, 0x00, 0x64]);
        assert_eq!(&cbw_w.cdb[7..9], &[0x00, 0x04]);

        // Zero sectors is rejected
        assert_eq!(scsi_read_10(3, 0, 0, 0, 512), Err(MsdError::ShortData));
        assert_eq!(scsi_write_10(4, 0, 0, 0, 512), Err(MsdError::ShortData));
    }

    #[test]
    fn random_garbage_never_panics() {
        let mut seed = 0xDEAD_BEEF_CAFE_0123u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..5000 {
            let len = (next() % 64) as usize;
            let buf: alloc::vec::Vec<u8> = (0..len).map(|_| next() as u8).collect();
            let _ = Csw::decode(&buf, 0x1234);
            let _ = InquiryData::decode(&buf);
            let _ = SenseData::decode(&buf);
            let _ = Capacity::decode(&buf);
        }
    }
}
