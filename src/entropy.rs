//! Boot random seed: several weak or untrusted sources are hashed together, so the seed is only as
//! predictable as the best source. Pure and host-testable; the hardware reads live in `main.rs`.

use crate::sha256::{Sha256, DIGEST_LEN};

pub const SEED_LEN: usize = DIGEST_LEN;

/// Accumulates entropy inputs. Each input is length-prefixed so `("ab","c")` and `("a","bc")`
/// differ.
pub struct Pool {
    h: Sha256,
    inputs: u32,
}

impl Default for Pool {
    fn default() -> Self {
        Self::new()
    }
}

impl Pool {
    pub fn new() -> Self {
        let mut h = Sha256::new();
        h.update(b"aeon-boot-seed-v1");
        Self { h, inputs: 0 }
    }

    pub fn add(&mut self, data: &[u8]) {
        self.h.update(&(data.len() as u32).to_le_bytes());
        self.h.update(data);
        self.inputs += 1;
    }

    pub fn add_u64(&mut self, v: u64) {
        self.add(&v.to_le_bytes());
    }

    pub fn inputs(&self) -> u32 {
        self.inputs
    }

    pub fn finish(self) -> [u8; SEED_LEN] {
        self.h.finalize()
    }
}

/// A non-zero 64-bit canary value derived from the seed.
pub fn canary_from(seed: &[u8; SEED_LEN]) -> u64 {
    let v = u64::from_le_bytes(seed[..8].try_into().unwrap());
    if v == 0 {
        0xAE0A_5AFE_C0DE_D00D
    } else {
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_inputs_same_seed_different_inputs_differ() {
        let mk = |x: &[u8]| {
            let mut p = Pool::new();
            p.add(x);
            p.finish()
        };
        assert_eq!(mk(b"a"), mk(b"a"));
        assert_ne!(mk(b"a"), mk(b"b"));
    }

    #[test]
    fn input_boundaries_matter() {
        let mut p = Pool::new();
        p.add(b"ab");
        p.add(b"c");
        let mut q = Pool::new();
        q.add(b"a");
        q.add(b"bc");
        assert_ne!(p.finish(), q.finish());
    }

    #[test]
    fn canary_never_zero() {
        assert_ne!(canary_from(&[0; SEED_LEN]), 0);
        assert_eq!(canary_from(&[1; SEED_LEN]), 0x0101_0101_0101_0101);
    }
}
