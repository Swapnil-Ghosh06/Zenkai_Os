//! Measured-boot wire formats for `EFI_TCG2_PROTOCOL` and the TPM 2.0 `PCR_Read` command.
//! Pure byte building and parsing, so it is unit-tested on the host. The firmware calls live in
//! `main.rs`.

use crate::sha256;

/// PCR that holds the boot command line, the same slot systemd-stub uses for the kernel command line.
pub const PCR_CMDLINE: u32 = 12;
/// PCR that marks the boot phase Aeon reached (systemd uses PCR 11 for kernel and phase measurements).
pub const PCR_PHASE: u32 = 11;
/// The phase string measured just before Aeon leaves the firmware.
pub const PHASE_EXIT_BOOT: &str = "aeon:exit-boot-services";
/// Measured instead of `PHASE_EXIT_BOOT` in recovery mode, so secrets sealed to a normal boot stay shut.
pub const PHASE_RECOVERY: &str = "aeon:recovery";
/// `EV_IPL`: initial program loader data (TCG PC Client platform firmware profile).
pub const EV_IPL: u32 = 0x0000_000D;
/// `EFI_TCG2_EVENT_HEADER.HeaderSize`: 4 + 2 + 4 + 4.
pub const EVENT_HEADER_SIZE: u32 = 14;
/// `EFI_TCG2_EVENT` overhead before the event data: `Size` (4) plus the header.
pub const EVENT_OVERHEAD: usize = 4 + EVENT_HEADER_SIZE as usize;

pub const TPM_ALG_SHA256: u16 = 0x000B;
const TPM_ST_NO_SESSIONS: u16 = 0x8001;
const TPM_CC_PCR_READ: u32 = 0x0000_017E;

/// Writes an `EFI_TCG2_EVENT` for `data` into `out` and returns its length, or `None` if `out` is too small.
pub fn build_event(out: &mut [u8], pcr: u32, event_type: u32, data: &[u8]) -> Option<usize> {
    let total = EVENT_OVERHEAD.checked_add(data.len())?;
    if out.len() < total || u32::try_from(total).is_err() {
        return None;
    }
    out[0..4].copy_from_slice(&(total as u32).to_le_bytes());
    out[4..8].copy_from_slice(&EVENT_HEADER_SIZE.to_le_bytes());
    out[8..10].copy_from_slice(&1u16.to_le_bytes()); // HeaderVersion
    out[10..14].copy_from_slice(&pcr.to_le_bytes());
    out[14..18].copy_from_slice(&event_type.to_le_bytes());
    out[18..total].copy_from_slice(data);
    Some(total)
}

/// ASCII to UTF-16LE with a terminating NUL, the event data for the command line. Returns the byte
/// count, or `None` if `out` is too small. Non-ASCII bytes become `?`.
pub fn utf16le_cmdline(s: &str, out: &mut [u8]) -> Option<usize> {
    let need = (s.len() + 1) * 2;
    if out.len() < need {
        return None;
    }
    for (i, b) in s.bytes().enumerate() {
        let c = if (0x20..0x7F).contains(&b) { b } else { b'?' };
        out[i * 2] = c;
        out[i * 2 + 1] = 0;
    }
    out[s.len() * 2] = 0;
    out[s.len() * 2 + 1] = 0;
    Some(need)
}

/// `TPM2_PCR_Read` for one PCR in the SHA-256 bank. Returns the command length (always 20).
pub fn build_pcr_read(out: &mut [u8; 20], pcr: u32) -> usize {
    out[0..2].copy_from_slice(&TPM_ST_NO_SESSIONS.to_be_bytes());
    out[2..6].copy_from_slice(&20u32.to_be_bytes());
    out[6..10].copy_from_slice(&TPM_CC_PCR_READ.to_be_bytes());
    out[10..14].copy_from_slice(&1u32.to_be_bytes()); // count of selections
    out[14..16].copy_from_slice(&TPM_ALG_SHA256.to_be_bytes());
    out[16] = 3; // sizeofSelect
    out[17] = 0;
    out[18] = 0;
    out[19] = 0;
    let byte = (pcr / 8) as usize;
    if byte < 3 {
        out[17 + byte] = 1 << (pcr % 8);
    }
    20
}

/// Extracts the single SHA-256 digest from a `TPM2_PCR_Read` response, or `None` if it is an error
/// or malformed.
pub fn parse_pcr_read(resp: &[u8]) -> Option<[u8; 32]> {
    // header(10) updateCounter(4) selectionOut(count 4 + alg 2 + size 1 + 3) digestCount(4) size(2) digest(32)
    if resp.len() < 10 || u16::from_be_bytes([resp[0], resp[1]]) != TPM_ST_NO_SESSIONS {
        return None;
    }
    let declared = u32::from_be_bytes(resp[2..6].try_into().ok()?) as usize;
    let rc = u32::from_be_bytes(resp[6..10].try_into().ok()?);
    if rc != 0 || declared > resp.len() {
        return None;
    }
    let mut p = 14; // after updateCounter
    let count = u32::from_be_bytes(resp.get(p..p + 4)?.try_into().ok()?);
    p += 4;
    if count != 1 {
        return None;
    }
    p += 2; // hash alg
    let sel = *resp.get(p)? as usize;
    p += 1 + sel;
    let dcount = u32::from_be_bytes(resp.get(p..p + 4)?.try_into().ok()?);
    p += 4;
    if dcount != 1 {
        return None;
    }
    let dsz = u16::from_be_bytes(resp.get(p..p + 2)?.try_into().ok()?) as usize;
    p += 2;
    if dsz != 32 {
        return None;
    }
    resp.get(p..p + 32)?.try_into().ok()
}

/// The value a SHA-256 PCR holds after extending `data` into a PCR that held `before`:
/// `SHA256(before || SHA256(data))`.
pub fn expected_extend(before: &[u8; 32], data: &[u8]) -> [u8; 32] {
    let mut h = sha256::Sha256::new();
    h.update(before);
    h.update(&sha256::digest(data));
    h.finalize()
}

/// Lowercase hex of a digest, for the log.
pub fn hex32(d: &[u8; 32], out: &mut [u8; 64]) {
    const H: &[u8; 16] = b"0123456789abcdef";
    for (i, b) in d.iter().enumerate() {
        out[i * 2] = H[(b >> 4) as usize];
        out[i * 2 + 1] = H[(b & 15) as usize];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_layout_matches_spec() {
        let mut buf = [0u8; 64];
        let n = build_event(&mut buf, 12, EV_IPL, b"ab").unwrap();
        assert_eq!(n, 20);
        assert_eq!(&buf[0..4], &20u32.to_le_bytes());
        assert_eq!(&buf[4..8], &14u32.to_le_bytes());
        assert_eq!(&buf[8..10], &1u16.to_le_bytes());
        assert_eq!(&buf[10..14], &12u32.to_le_bytes());
        assert_eq!(&buf[14..18], &0xDu32.to_le_bytes());
        assert_eq!(&buf[18..20], b"ab");
    }

    #[test]
    fn event_rejects_small_buffer() {
        let mut buf = [0u8; 19];
        assert!(build_event(&mut buf, 12, EV_IPL, b"ab").is_none());
    }

    #[test]
    fn cmdline_is_utf16le_with_nul() {
        let mut buf = [0xFFu8; 16];
        let n = utf16le_cmdline("a\u{e9}", &mut buf).unwrap(); // 'a' then a non-ASCII char (2 bytes in UTF-8)
        assert_eq!(&buf[..n], &[b'a', 0, b'?', 0, b'?', 0, 0, 0]);
        assert!(utf16le_cmdline("abcdefgh", &mut buf).is_none());
    }

    #[test]
    fn pcr_read_command_selects_the_bit() {
        let mut c = [0u8; 20];
        build_pcr_read(&mut c, 12);
        assert_eq!(&c[0..10], &[0x80, 0x01, 0, 0, 0, 20, 0, 0, 0x01, 0x7E]);
        assert_eq!(&c[14..16], &[0x00, 0x0B]);
        assert_eq!(&c[16..20], &[3, 0, 0x10, 0]);
        build_pcr_read(&mut c, 2);
        assert_eq!(&c[16..20], &[3, 0x04, 0, 0]);
    }

    fn response(rc: u32, digest: &[u8]) -> Vec<u8> {
        let mut r = vec![0x80, 0x01, 0, 0, 0, 0, 0, 0, 0, 0];
        r[6..10].copy_from_slice(&rc.to_be_bytes());
        r.extend_from_slice(&1u32.to_be_bytes()); // updateCounter
        r.extend_from_slice(&1u32.to_be_bytes()); // selection count
        r.extend_from_slice(&[0, 0x0B, 3, 0, 0, 0x10]);
        r.extend_from_slice(&1u32.to_be_bytes()); // digest count
        r.extend_from_slice(&(digest.len() as u16).to_be_bytes());
        r.extend_from_slice(digest);
        let n = r.len() as u32;
        r[2..6].copy_from_slice(&n.to_be_bytes());
        r
    }

    #[test]
    fn parses_good_response_and_rejects_bad() {
        let d = [7u8; 32];
        assert_eq!(parse_pcr_read(&response(0, &d)), Some(d));
        assert_eq!(parse_pcr_read(&response(0x101, &d)), None);
        assert_eq!(parse_pcr_read(&response(0, &d[..16])), None);
        let r = response(0, &d);
        for cut in 0..r.len() {
            assert_eq!(parse_pcr_read(&r[..cut]), None, "truncated at {cut}");
        }
    }

    #[test]
    fn expected_extend_value() {
        // PCR_new = SHA256(before || SHA256(data)).
        let v = expected_extend(&[0; 32], b"");
        let mut input = [0u8; 64];
        input[32..].copy_from_slice(&sha256::digest(b""));
        assert_eq!(v, sha256::digest(&input));
        assert_ne!(expected_extend(&[1; 32], b""), v);
    }

    #[test]
    fn hex_is_lowercase_and_fixed_width() {
        let mut out = [0u8; 64];
        let mut d = [0u8; 32];
        d[0] = 0xAB;
        d[31] = 0x0F;
        hex32(&d, &mut out);
        assert_eq!(&out[..2], b"ab");
        assert_eq!(&out[62..], b"0f");
    }
}
