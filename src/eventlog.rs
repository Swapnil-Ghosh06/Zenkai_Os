//! TCG crypto-agile event log parser and PCR replay. The firmware records every measurement it makes
//! in this log; replaying it must reproduce the TPM's PCR values. If it does not, the log, the TPM
//! or a measurement is wrong, which is what a verifier looks for. Pure and host-testable.
//!
//! Layout (TCG PC Client Platform Firmware Profile, "Crypto Agile Log Entry Format"):
//! - first record, legacy `TCG_PCR_EVENT`: pcr u32, type u32, sha1 digest [20], data size u32, data
//!   (the Spec ID event, which lists the hash algorithms and their digest sizes);
//! - then `TCG_PCR_EVENT2` records: pcr u32, type u32, digest count u32, per digest an algorithm id
//!   u16 and that algorithm's digest bytes, then data size u32 and the data.

use crate::sha256::Sha256;

pub const PCR_COUNT: usize = 24;
const ALG_SHA256: u16 = 0x000B;
const EV_NO_ACTION: u32 = 0x0000_0003;
const MAX_ALGS: usize = 8;
const MAX_EVENTS: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The log ends in the middle of a record.
    Truncated,
    /// The first record is not a valid Spec ID event.
    BadHeader,
    /// A record names a hash algorithm the Spec ID event did not declare.
    UnknownAlgorithm,
    /// The log has no SHA-256 bank, so it cannot be replayed here.
    NoSha256,
    /// A record names a PCR that does not exist.
    BadPcr,
    /// More records than any real log holds.
    TooMany,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replay {
    /// Replayed SHA-256 PCR values.
    pub pcrs: [[u8; 32]; PCR_COUNT],
    /// Records that were extended (EV_NO_ACTION records are not).
    pub extended: usize,
    /// Records of every kind, the header excluded.
    pub records: usize,
}

struct Reader<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let end = self.p.checked_add(n).ok_or(Error::Truncated)?;
        let s = self.d.get(self.p..end).ok_or(Error::Truncated)?;
        self.p = end;
        Ok(s)
    }
    fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
}

fn extend(pcr: &mut [u8; 32], digest: &[u8]) {
    let mut h = Sha256::new();
    h.update(pcr);
    h.update(digest);
    *pcr = h.finalize();
}

/// Replays the log in `log`, processing the records that start at or before `last_offset` (the
/// firmware reports where the last entry begins).
pub fn replay(log: &[u8], last_offset: usize) -> Result<Replay, Error> {
    let mut r = Reader { d: log, p: 0 };

    // Header: legacy event carrying the Spec ID event.
    let (pcr0, ty) = (r.u32()?, r.u32()?);
    r.take(20)?;
    let size = r.u32()? as usize;
    if pcr0 != 0 || ty != EV_NO_ACTION {
        return Err(Error::BadHeader);
    }
    let mut spec = Reader { d: r.take(size)?, p: 0 };
    if spec.take(16)? != b"Spec ID Event03\0" {
        return Err(Error::BadHeader);
    }
    spec.take(4 + 4)?; // platform class, minor/major/errata/uintn size
    let nalg = spec.u32()? as usize;
    if nalg == 0 || nalg > MAX_ALGS {
        return Err(Error::BadHeader);
    }
    let mut algs = [(0u16, 0usize); MAX_ALGS];
    for a in algs.iter_mut().take(nalg) {
        *a = (spec.u16()?, spec.u16()? as usize);
    }
    let algs = &algs[..nalg];
    if !algs.iter().any(|a| a.0 == ALG_SHA256 && a.1 == 32) {
        return Err(Error::NoSha256);
    }

    let mut out = Replay { pcrs: [[0; 32]; PCR_COUNT], extended: 0, records: 0 };
    while r.p <= last_offset && r.p < log.len() {
        let pcr = r.u32()? as usize;
        let ty = r.u32()?;
        let count = r.u32()? as usize;
        if count > MAX_ALGS {
            return Err(Error::UnknownAlgorithm);
        }
        let mut sha256: Option<&[u8]> = None;
        for _ in 0..count {
            let alg = r.u16()?;
            let size = algs.iter().find(|a| a.0 == alg).ok_or(Error::UnknownAlgorithm)?.1;
            let d = r.take(size)?;
            if alg == ALG_SHA256 {
                sha256 = Some(d);
            }
        }
        let dsize = r.u32()? as usize;
        let data = r.take(dsize)?;
        out.records += 1;
        if out.records > MAX_EVENTS {
            return Err(Error::TooMany);
        }
        if pcr >= PCR_COUNT {
            return Err(Error::BadPcr);
        }
        if ty == EV_NO_ACTION {
            // A TPM started at locality 3 begins PCR 0 at 0...03 instead of zero.
            if data.len() >= 17 && &data[..16] == b"StartupLocality\0" && data[16] == 3 {
                out.pcrs[0][31] = 3;
            }
            continue;
        }
        extend(&mut out.pcrs[pcr], sha256.ok_or(Error::NoSha256)?);
        out.extended += 1;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sha256::digest;

    fn header(algs: &[(u16, u16)]) -> Vec<u8> {
        let mut spec = b"Spec ID Event03\0".to_vec();
        spec.extend_from_slice(&0u32.to_le_bytes()); // platform class
        spec.extend_from_slice(&[0, 2, 0, 2]); // minor, major, errata, uintn size
        spec.extend_from_slice(&(algs.len() as u32).to_le_bytes());
        for (a, s) in algs {
            spec.extend_from_slice(&a.to_le_bytes());
            spec.extend_from_slice(&s.to_le_bytes());
        }
        spec.push(0); // vendor info size
        let mut h = Vec::new();
        h.extend_from_slice(&0u32.to_le_bytes());
        h.extend_from_slice(&3u32.to_le_bytes());
        h.extend_from_slice(&[0; 20]);
        h.extend_from_slice(&(spec.len() as u32).to_le_bytes());
        h.extend_from_slice(&spec);
        h
    }

    fn record(pcr: u32, ty: u32, digests: &[(u16, Vec<u8>)], data: &[u8]) -> Vec<u8> {
        let mut r = Vec::new();
        r.extend_from_slice(&pcr.to_le_bytes());
        r.extend_from_slice(&ty.to_le_bytes());
        r.extend_from_slice(&(digests.len() as u32).to_le_bytes());
        for (a, d) in digests {
            r.extend_from_slice(&a.to_le_bytes());
            r.extend_from_slice(d);
        }
        r.extend_from_slice(&(data.len() as u32).to_le_bytes());
        r.extend_from_slice(data);
        r
    }

    fn two_banks(data: &[u8]) -> Vec<(u16, Vec<u8>)> {
        vec![(4, vec![9; 20]), (ALG_SHA256, digest(data).to_vec())]
    }

    #[test]
    fn replays_extends_in_order() {
        let mut log = header(&[(4, 20), (ALG_SHA256, 32)]);
        let first = log.len();
        log.extend(record(4, 0x80000001, &two_banks(b"a"), b"a"));
        let last = log.len();
        log.extend(record(4, 0x80000001, &two_banks(b"b"), b"b"));
        let rp = replay(&log, last).unwrap();
        let mut want = [0u8; 32];
        extend(&mut want, &digest(b"a"));
        extend(&mut want, &digest(b"b"));
        assert_eq!(rp.pcrs[4], want);
        assert_eq!((rp.extended, rp.records), (2, 2));
        assert_eq!(rp.pcrs[5], [0; 32]);
        // Stopping at the first record leaves the second out.
        let rp1 = replay(&log, first).unwrap();
        let mut one = [0u8; 32];
        extend(&mut one, &digest(b"a"));
        assert_eq!(rp1.pcrs[4], one);
    }

    #[test]
    fn no_action_events_are_not_extended() {
        let mut log = header(&[(ALG_SHA256, 32)]);
        let at = log.len();
        log.extend(record(0, 3, &[(ALG_SHA256, vec![0; 32])], b"noise"));
        let rp = replay(&log, at).unwrap();
        assert_eq!((rp.extended, rp.records), (0, 1));
        assert_eq!(rp.pcrs[0], [0; 32]);
    }

    #[test]
    fn startup_locality_3_sets_pcr0_start() {
        let mut log = header(&[(ALG_SHA256, 32)]);
        let at = log.len();
        let mut data = b"StartupLocality\0".to_vec();
        data.push(3);
        log.extend(record(0, 3, &[(ALG_SHA256, vec![0; 32])], &data));
        let rp = replay(&log, at).unwrap();
        assert_eq!(rp.pcrs[0][31], 3);
    }

    #[test]
    fn rejects_bad_headers_and_banks() {
        assert_eq!(replay(&[], 0), Err(Error::Truncated));
        let mut bad = header(&[(ALG_SHA256, 32)]);
        bad[4] = 2; // header is not EV_NO_ACTION
        assert_eq!(replay(&bad, 0), Err(Error::BadHeader));
        let mut sig = header(&[(ALG_SHA256, 32)]);
        sig[32] ^= 1; // corrupt "Spec ID Event03"
        assert_eq!(replay(&sig, 0), Err(Error::BadHeader));
        assert_eq!(replay(&header(&[(4, 20)]), 0), Err(Error::NoSha256));
        assert_eq!(replay(&header(&[]), 0), Err(Error::BadHeader));
    }

    #[test]
    fn rejects_unknown_algorithms_and_bad_pcrs() {
        let mut log = header(&[(ALG_SHA256, 32)]);
        let at = log.len();
        log.extend(record(1, 5, &[(0x1234, vec![0; 32])], b""));
        assert_eq!(replay(&log, at), Err(Error::UnknownAlgorithm));
        let mut log = header(&[(ALG_SHA256, 32)]);
        let at = log.len();
        log.extend(record(99, 5, &[(ALG_SHA256, vec![0; 32])], b""));
        assert_eq!(replay(&log, at), Err(Error::BadPcr));
    }

    #[test]
    fn every_truncation_is_an_error_not_a_panic() {
        let mut log = header(&[(4, 20), (ALG_SHA256, 32)]);
        let at = log.len();
        log.extend(record(4, 5, &two_banks(b"x"), b"payload"));
        for cut in 0..log.len() {
            let _ = replay(&log[..cut], at); // may fail, must not panic
        }
        assert!(replay(&log, at).is_ok());
    }

    #[test]
    fn random_logs_never_panic() {
        let mut s = 0xC0FF_EE00_1234_5678u64;
        let mut next = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        let base = {
            let mut l = header(&[(4, 20), (ALG_SHA256, 32)]);
            l.extend(record(4, 5, &two_banks(b"x"), b"payload"));
            l
        };
        for _ in 0..30_000 {
            let mut l = base.clone();
            for _ in 0..(next() % 6) {
                let i = (next() as usize) % l.len();
                l[i] = next() as u8;
            }
            let len = (next() as usize) % (l.len() + 1);
            let _ = replay(&l[..len], (next() as usize) % 400);
        }
        for _ in 0..20_000 {
            let len = (next() % 300) as usize;
            let l: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            let _ = replay(&l, (next() as usize) % 300);
        }
    }
}

/// A real log captured from OVMF + swtpm (see `tests/capture_eventlog.py`): replaying it must give
/// exactly the PCR values the TPM reported in the same run.
#[cfg(test)]
mod fixture {
    use super::*;

    const LOG: &[u8] = include_bytes!("../tests/fixtures/ovmf-swtpm-eventlog.bin");
    const META: &str = include_str!("../tests/fixtures/ovmf-swtpm-eventlog.json");

    fn last_offset() -> usize {
        let at = META.find("\"last_offset\":").unwrap() + "\"last_offset\":".len();
        META[at..].split(',').next().unwrap().trim().parse().unwrap()
    }

    /// `(pcr, hex)` pairs from the `"pcrs"` object.
    fn pcrs() -> Vec<(usize, String)> {
        let body = &META[META.find("\"pcrs\"").unwrap()..];
        body.lines()
            .filter_map(|l| {
                let (k, v) = l.trim().trim_end_matches(',').split_once(':')?;
                let k = k.trim().trim_matches('"').parse().ok()?;
                Some((k, v.trim().trim_matches('"').to_string()))
            })
            .collect()
    }

    fn hex(d: &[u8; 32]) -> String {
        d.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn replay_reproduces_every_pcr_the_tpm_reported() {
        let rp = replay(LOG, last_offset()).unwrap();
        let expected = pcrs();
        assert_eq!(expected.len(), 10);
        for (pcr, want) in expected {
            assert_eq!(hex(&rp.pcrs[pcr]), want, "PCR {pcr}");
        }
        assert!(rp.extended > 20, "a real boot log has dozens of measurements, got {}", rp.extended);
    }

    #[test]
    fn tampering_with_one_digest_changes_the_replayed_pcr() {
        let good = replay(LOG, last_offset()).unwrap();
        // Flip a bit in every position in turn inside the record area; replay must either fail
        // cleanly or give different PCRs, never the same ones.
        let mut changed = 0;
        for i in (200..LOG.len().min(8600)).step_by(7) {
            let mut bad = LOG.to_vec();
            bad[i] ^= 0x01;
            match replay(&bad, last_offset()) {
                Ok(r) if r.pcrs == good.pcrs => {}
                _ => changed += 1,
            }
        }
        // Only the SHA-256 digests (and structure fields) feed the PCRs; event text and the SHA-1
        // bank do not, so only part of the bytes matter. A real log has plenty that do.
        assert!(changed > 100, "single-bit changes in digests must be visible, saw {changed}");
    }

    #[test]
    fn dropping_the_last_record_changes_the_result() {
        let all = replay(LOG, last_offset()).unwrap();
        let fewer = replay(LOG, last_offset() - 1).unwrap();
        assert_eq!(fewer.records + 1, all.records);
        assert_ne!(fewer.pcrs, all.pcrs);
    }
}
