//! Pure log formatting and buffering. No hardware access, so it runs under `cargo test` on the host.

use core::fmt::{self, Write};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Level {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
    Trace = 4,
}

impl Level {
    /// Fixed-width (5) tag so columns line up in the serial log.
    pub const fn tag(self) -> &'static str {
        match self {
            Level::Error => "ERROR",
            Level::Warn => "WARN ",
            Level::Info => "INFO ",
            Level::Debug => "DEBUG",
            Level::Trace => "TRACE",
        }
    }
}

/// Converts a TSC delta to microseconds. `hz == 0` (not calibrated yet) yields 0.
pub fn ticks_to_us(ticks: u64, hz: u64) -> u64 {
    if hz == 0 {
        return 0;
    }
    ((ticks as u128 * 1_000_000) / hz as u128) as u64
}

/// Writes `[ssss.uuuuuu] LEVEL target ` (no newline).
pub fn write_prefix(w: &mut dyn Write, us: u64, level: Level, target: &str) -> fmt::Result {
    write!(w, "[{:4}.{:06}] {} {} ", us / 1_000_000, us % 1_000_000, level.tag(), target)
}

/// Fixed-capacity line buffer. Output beyond the capacity is cut and ends with `~`.
pub struct LineBuf<const N: usize> {
    buf: [u8; N],
    len: usize,
    cut: bool,
}

impl<const N: usize> LineBuf<N> {
    pub const fn new() -> Self {
        Self { buf: [0; N], len: 0, cut: false }
    }

    pub fn clear(&mut self) {
        self.len = 0;
        self.cut = false;
    }

    /// Terminates the line with `\n` (replacing the last byte when the buffer was full).
    pub fn finish(&mut self) -> &[u8] {
        if self.cut && N >= 2 {
            self.buf[N - 2] = b'~';
            self.len = N - 1;
        }
        if self.len < N {
            self.buf[self.len] = b'\n';
            self.len += 1;
        } else {
            self.buf[N - 1] = b'\n';
        }
        &self.buf[..self.len]
    }
}

impl<const N: usize> Default for LineBuf<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Write for LineBuf<N> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for &b in s.as_bytes() {
            // Keep one byte spare for the final '\n'.
            if self.len + 1 >= N {
                self.cut = true;
                break;
            }
            // Keep the log 7-bit clean and single-line.
            self.buf[self.len] = if (0x20..0x7F).contains(&b) { b } else { b'?' };
            self.len += 1;
        }
        Ok(())
    }
}

/// Overwriting byte ring: keeps the newest `N` bytes.
pub struct Ring<const N: usize> {
    buf: [u8; N],
    head: usize,
    len: usize,
}

impl<const N: usize> Ring<N> {
    pub const fn new() -> Self {
        Self { buf: [0; N], head: 0, len: 0 }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn push(&mut self, data: &[u8]) {
        for &b in data {
            let tail = (self.head + self.len) % N;
            self.buf[tail] = b;
            if self.len < N {
                self.len += 1;
            } else {
                self.head = (self.head + 1) % N;
            }
        }
    }

    /// Contents oldest-first as two slices (the second is empty unless the ring has wrapped).
    pub fn slices(&self) -> (&[u8], &[u8]) {
        if self.head + self.len <= N {
            (&self.buf[self.head..self.head + self.len], &[])
        } else {
            (&self.buf[self.head..], &self.buf[..(self.head + self.len) % N])
        }
    }
}

impl<const N: usize> Default for Ring<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_format_is_stable() {
        let mut b = LineBuf::<128>::new();
        write_prefix(&mut b, 12_345, Level::Info, "boot.mem").unwrap();
        write!(b, "usable_mb={}", 978).unwrap();
        assert_eq!(b.finish(), b"[   0.012345] INFO  boot.mem usable_mb=978\n");
    }

    #[test]
    fn prefix_handles_large_uptime() {
        let mut b = LineBuf::<128>::new();
        write_prefix(&mut b, 3_725_000_042, Level::Error, "x").unwrap();
        assert_eq!(b.finish(), b"[3725.000042] ERROR x \n");
    }

    #[test]
    fn long_lines_are_cut_with_marker() {
        let mut b = LineBuf::<8>::new();
        write!(b, "0123456789").unwrap();
        assert_eq!(b.finish(), b"012345~\n");
    }

    #[test]
    fn non_ascii_and_newlines_are_sanitised() {
        let mut b = LineBuf::<32>::new();
        write!(b, "a\nb\u{2588}c").unwrap();
        assert_eq!(b.finish(), b"a?b???c\n");
    }

    #[test]
    fn ticks_to_us_does_not_overflow() {
        assert_eq!(ticks_to_us(5_000_000_000, 2_500_000_000), 2_000_000);
        assert!(ticks_to_us(u64::MAX, 1_000_000_000) > 0);
        assert_eq!(ticks_to_us(123, 0), 0);
    }

    #[test]
    fn ring_keeps_newest_bytes_in_order() {
        let mut r = Ring::<4>::new();
        r.push(b"ab");
        assert_eq!(r.slices(), (&b"ab"[..], &b""[..]));
        r.push(b"cdef");
        let (a, b) = r.slices();
        let mut all = [0u8; 4];
        all[..a.len()].copy_from_slice(a);
        all[a.len()..a.len() + b.len()].copy_from_slice(b);
        assert_eq!(&all, b"cdef");
        assert_eq!(r.len(), 4);
    }
}

/// The log ring and line buffer take arbitrary bytes from anywhere in the kernel.
#[cfg(test)]
mod fuzz {
    use super::*;
    use core::fmt::Write;

    fn xorshift(s: &mut u64) -> u64 {
        *s ^= *s << 13;
        *s ^= *s >> 7;
        *s ^= *s << 17;
        *s
    }

    #[test]
    fn ring_keeps_exactly_the_newest_bytes() {
        let mut s = 7u64;
        let mut ring = Ring::<64>::new();
        let mut all: Vec<u8> = Vec::new();
        for _ in 0..2_000 {
            let n = (xorshift(&mut s) % 100) as usize;
            let chunk: Vec<u8> = (0..n).map(|_| xorshift(&mut s) as u8).collect();
            ring.push(&chunk);
            all.extend_from_slice(&chunk);
            let (a, b) = ring.slices();
            let got: Vec<u8> = a.iter().chain(b).copied().collect();
            let want = &all[all.len().saturating_sub(64)..];
            assert_eq!(got, want);
        }
    }

    #[test]
    fn line_buffer_is_always_single_line_ascii() {
        let mut s = 99u64;
        for _ in 0..5_000 {
            let mut lb = LineBuf::<32>::new();
            let n = (xorshift(&mut s) % 80) as usize;
            let text: String = (0..n).map(|_| char::from_u32((xorshift(&mut s) % 0x300) as u32).unwrap_or('?')).collect();
            let _ = lb.write_str(&text);
            let out = lb.finish();
            assert!(out.len() <= 32);
            assert_eq!(out.last(), Some(&b'\n'));
            assert!(out[..out.len() - 1].iter().all(|b| (0x20..0x7F).contains(b) && *b != b'\n'));
        }
    }
}
