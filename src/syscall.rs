//! System call numbers. Pure, host-testable. Calling convention: number in `rax`, arguments in `rdi`,
//! `rsi`, `rdx`, result in `rax`, issued with the `syscall` instruction.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u64)]
pub enum Number {
    /// `write(ptr, len)`: print `len` bytes at `ptr` (must be in the user window, at most `MAX_WRITE`).
    Write = 1,
    /// `exit(code)`
    Exit = 2,
    Yield = 3,
    /// microseconds since boot
    Uptime = 4,
    /// the calling task's id
    GetPid = 5,
}

pub const MAX_WRITE: u64 = 256;
/// Returned for bad pointers, bad lengths and unknown calls.
pub const ERR: u64 = u64::MAX;

/// Makes bytes from a user program safe to print: everything except printable ASCII, newline and tab
/// becomes `?`, so a program cannot forge log lines with carriage returns or send terminal escapes.
pub fn sanitize_output(buf: &mut [u8]) {
    for b in buf {
        if !(matches!(*b, b'\n' | b'\t') || (0x20..0x7F).contains(b)) {
            *b = b'?';
        }
    }
}

impl Number {
    /// This call's bit in an allow-mask.
    pub const fn bit(self) -> u32 {
        1 << (self as u64)
    }
}

/// Calls every program may make: without `exit` a program could never end cleanly.
pub const ALWAYS: u32 = Number::Exit.bit();

/// The calls a built-in program is allowed to make (least privilege: `hello` may print, `denied`
/// may only exit). Unknown program names get `exit` only.
pub fn allow_mask(program: &str) -> u32 {
    ALWAYS
        | match program {
            "hello" | "badcall" => Number::Write.bit(),
            "spin" => Number::Yield.bit(),
            _ => 0,
        }
}

/// True if `mask` lets the program make call `n`.
pub const fn permits(mask: u32, n: Number) -> bool {
    mask & n.bit() != 0
}

pub fn decode(n: u64) -> Option<Number> {
    Some(match n {
        1 => Number::Write,
        2 => Number::Exit,
        3 => Number::Yield,
        4 => Number::Uptime,
        5 => Number::GetPid,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_roundtrip() {
        for n in [Number::Write, Number::Exit, Number::Yield, Number::Uptime, Number::GetPid] {
            assert_eq!(decode(n as u64), Some(n));
        }
    }

    #[test]
    fn allow_masks_are_least_privilege() {
        let all = [Number::Write, Number::Exit, Number::Yield, Number::Uptime, Number::GetPid];
        // Every program can exit, and nothing can make a call outside its list.
        for p in ["hello", "fault", "gp", "spin", "badcall", "denied", "whatever"] {
            assert!(permits(allow_mask(p), Number::Exit), "{p} must be able to exit");
            assert!(!permits(allow_mask(p), Number::Uptime), "{p} must not read the clock");
            assert!(!permits(allow_mask(p), Number::GetPid), "{p} must not read its id");
        }
        assert!(permits(allow_mask("hello"), Number::Write));
        assert!(!permits(allow_mask("hello"), Number::Yield));
        assert!(permits(allow_mask("spin"), Number::Yield));
        assert!(!permits(allow_mask("spin"), Number::Write));
        // `denied` and unknown names can only exit.
        for p in ["denied", "whatever", ""] {
            for n in all {
                assert_eq!(permits(allow_mask(p), n), n == Number::Exit, "{p} {n:?}");
            }
        }
    }

    #[test]
    fn number_bits_are_distinct() {
        let all = [Number::Write, Number::Exit, Number::Yield, Number::Uptime, Number::GetPid];
        let mut seen = 0u32;
        for n in all {
            assert_eq!(seen & n.bit(), 0);
            seen |= n.bit();
        }
    }

    #[test]
    fn decode_unknown_is_none() {
        for n in [0, 6, 255, u64::MAX] {
            assert_eq!(decode(n), None);
        }
    }

    #[test]
    fn sanitize_keeps_text_and_replaces_controls() {
        let mut b = *b"ok\tline\n";
        sanitize_output(&mut b);
        assert_eq!(&b, b"ok\tline\n");
        let mut c = *b"a\rb\x1b[2Jc\x00\x7f\xffd";
        sanitize_output(&mut c);
        assert_eq!(&c, b"a?b?[2Jc???d");
    }
}
