//! USB HID boot-protocol keyboard reports to shell keys. A boot report is 8 bytes: modifier bits, a
//! reserved byte, then up to six key usage codes that are currently held. A key counts as pressed the
//! first time its code shows up. The rules for Ctrl, Shift and Caps Lock match the PS/2 path in
//! `keymap.rs`, so both keyboards behave the same. Pure, host-tested.

use crate::keymap::Key;

const MOD_LCTRL: u8 = 1 << 0;
const MOD_LSHIFT: u8 = 1 << 1;
const MOD_RCTRL: u8 = 1 << 4;
const MOD_RSHIFT: u8 = 1 << 5;

/// Usage codes 0x04..=0x38 (letters, digits, Enter, Esc, Backspace, Tab, Space and punctuation),
/// plain and shifted. A 0 means "no character".
const LOWER: &[u8; 53] = b"abcdefghijklmnopqrstuvwxyz1234567890\n\x1b\x08\t -=[]\\\0;'`,./";
const UPPER: &[u8; 53] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ!@#$%^&*()\n\x1b\x08\t _+{}|\0:\"~<>?";
const FIRST_USAGE: u8 = 0x04;

const CAPS_LOCK: u8 = 0x39;

#[derive(Default)]
pub struct HidKeyboard {
    /// The six key slots of the previous report.
    prev: [u8; 6],
    caps: bool,
}

impl HidKeyboard {
    pub const fn new() -> Self {
        Self { prev: [0; 6], caps: false }
    }

    /// Feeds one boot report and calls `out` once for every key that was pressed since the last report.
    /// Reports shorter than 8 bytes, and error-rollover reports, produce nothing.
    pub fn feed(&mut self, report: &[u8], out: &mut impl FnMut(Key)) {
        let Some(&modifiers) = report.first() else { return };
        let Some(slots) = report.get(2..8) else { return };
        let mut now = [0u8; 6];
        now.copy_from_slice(slots);
        // Usage 1 in every slot is the keyboard saying "too many keys": ignore it, and keep what was held.
        if now.iter().all(|&u| u == 1) {
            return;
        }
        let shift = modifiers & (MOD_LSHIFT | MOD_RSHIFT) != 0;
        let ctrl = modifiers & (MOD_LCTRL | MOD_RCTRL) != 0;
        for &usage in now.iter().filter(|&&u| u > 1) {
            if self.prev.contains(&usage) {
                continue; // still held from the previous report
            }
            if let Some(key) = self.translate(usage, shift, ctrl) {
                out(key);
            }
        }
        self.prev = now;
    }

    /// One newly pressed usage code to a key, or `None` for keys the shell has no use for.
    fn translate(&mut self, usage: u8, shift: bool, ctrl: bool) -> Option<Key> {
        match usage {
            CAPS_LOCK => {
                self.caps = !self.caps;
                return None;
            }
            0x49 => return None, // Insert
            0x4A => return Some(if ctrl { Key::ScrollTop } else { Key::Home }),
            0x4B => return Some(Key::PageUp),
            0x4C => return Some(Key::Delete),
            0x4D => return Some(if ctrl { Key::ScrollBottom } else { Key::End }),
            0x4E => return Some(Key::PageDown),
            0x4F => return Some(Key::Right),
            0x50 => return Some(Key::Left),
            0x51 => return Some(if ctrl { Key::HistoryNext } else { Key::Down }),
            0x52 => return Some(if ctrl { Key::HistoryPrev } else { Key::Up }),
            _ => {}
        }
        let table = if shift { UPPER } else { LOWER };
        let mut c = *table.get(usize::from(usage.checked_sub(FIRST_USAGE)?))?;
        if self.caps && c.is_ascii_alphabetic() {
            c = if shift { c.to_ascii_lowercase() } else { c.to_ascii_uppercase() };
        }
        if ctrl {
            match c.to_ascii_lowercase() {
                b'c' => return Some(Key::Interrupt),
                b'l' => return Some(Key::ClearScreen),
                l if l.is_ascii_lowercase() => return Some(Key::Ctrl(l)),
                _ => {}
            }
        }
        match c {
            0 => None,
            b'\n' => Some(Key::Enter),
            0x08 => Some(Key::Backspace),
            b'\t' => Some(Key::Tab),
            0x1b => Some(Key::Esc),
            c => Some(Key::Char(c)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHIFT: u8 = MOD_LSHIFT;
    const CTRL: u8 = MOD_LCTRL;

    fn rep(modifiers: u8, keys: &[u8]) -> [u8; 8] {
        let mut r = [0u8; 8];
        r[0] = modifiers;
        r[2..2 + keys.len()].copy_from_slice(keys);
        r
    }

    fn feed(k: &mut HidKeyboard, r: &[u8]) -> Vec<Key> {
        let mut v = Vec::new();
        k.feed(r, &mut |key| v.push(key));
        v
    }

    #[test]
    fn letters_digits_and_shift() {
        let mut k = HidKeyboard::new();
        assert_eq!(feed(&mut k, &rep(0, &[0x04])), [Key::Char(b'a')]);
        assert_eq!(feed(&mut k, &rep(0, &[])), []);
        assert_eq!(feed(&mut k, &rep(SHIFT, &[0x04])), [Key::Char(b'A')]);
        assert_eq!(feed(&mut k, &rep(0, &[])), []);
        assert_eq!(feed(&mut k, &rep(0, &[0x1E])), [Key::Char(b'1')]);
        assert_eq!(feed(&mut k, &rep(0, &[])), []);
        assert_eq!(feed(&mut k, &rep(MOD_RSHIFT, &[0x1E])), [Key::Char(b'!')]);
        assert_eq!(feed(&mut k, &rep(0, &[])), []);
        assert_eq!(feed(&mut k, &rep(0, &[0x27])), [Key::Char(b'0')]);
    }

    #[test]
    fn punctuation_and_space() {
        let mut k = HidKeyboard::new();
        for (usage, plain, shifted) in [(0x2C, b' ', b' '), (0x2D, b'-', b'_'), (0x2E, b'=', b'+'), (0x33, b';', b':'), (0x34, b'\'', b'"'), (0x35, b'`', b'~'), (0x36, b',', b'<'), (0x37, b'.', b'>'), (0x38, b'/', b'?')] {
            assert_eq!(feed(&mut k, &rep(0, &[usage])), [Key::Char(plain)], "usage {usage:#x}");
            feed(&mut k, &rep(0, &[]));
            assert_eq!(feed(&mut k, &rep(SHIFT, &[usage])), [Key::Char(shifted)], "shifted usage {usage:#x}");
            feed(&mut k, &rep(0, &[]));
        }
    }

    #[test]
    fn editing_and_cursor_keys() {
        let mut k = HidKeyboard::new();
        for (usage, key) in [
            (0x28, Key::Enter),
            (0x29, Key::Esc),
            (0x2A, Key::Backspace),
            (0x2B, Key::Tab),
            (0x4A, Key::Home),
            (0x4B, Key::PageUp),
            (0x4C, Key::Delete),
            (0x4D, Key::End),
            (0x4E, Key::PageDown),
            (0x4F, Key::Right),
            (0x50, Key::Left),
            (0x51, Key::Down),
            (0x52, Key::Up),
        ] {
            assert_eq!(feed(&mut k, &rep(0, &[usage])), [key], "usage {usage:#x}");
            feed(&mut k, &rep(0, &[]));
        }
    }

    #[test]
    fn ctrl_combinations_match_the_ps2_keyboard() {
        let mut k = HidKeyboard::new();
        assert_eq!(feed(&mut k, &rep(CTRL, &[0x06])), [Key::Interrupt], "Ctrl+C");
        feed(&mut k, &rep(0, &[]));
        assert_eq!(feed(&mut k, &rep(MOD_RCTRL, &[0x0F])), [Key::ClearScreen], "Ctrl+L");
        feed(&mut k, &rep(0, &[]));
        assert_eq!(feed(&mut k, &rep(CTRL, &[0x04])), [Key::Ctrl(b'a')]);
        feed(&mut k, &rep(0, &[]));
        assert_eq!(feed(&mut k, &rep(CTRL, &[0x52])), [Key::HistoryPrev], "Ctrl+Up");
        feed(&mut k, &rep(0, &[]));
        assert_eq!(feed(&mut k, &rep(CTRL, &[0x51])), [Key::HistoryNext], "Ctrl+Down");
        feed(&mut k, &rep(0, &[]));
        assert_eq!(feed(&mut k, &rep(CTRL, &[0x4A])), [Key::ScrollTop], "Ctrl+Home");
        feed(&mut k, &rep(0, &[]));
        assert_eq!(feed(&mut k, &rep(CTRL, &[0x4D])), [Key::ScrollBottom], "Ctrl+End");
    }

    #[test]
    fn caps_lock_inverts_letters_and_shift_inverts_back() {
        let mut k = HidKeyboard::new();
        assert_eq!(feed(&mut k, &rep(0, &[CAPS_LOCK])), []);
        feed(&mut k, &rep(0, &[]));
        assert_eq!(feed(&mut k, &rep(0, &[0x14])), [Key::Char(b'Q')]);
        feed(&mut k, &rep(0, &[]));
        assert_eq!(feed(&mut k, &rep(SHIFT, &[0x14])), [Key::Char(b'q')]);
        feed(&mut k, &rep(0, &[]));
        assert_eq!(feed(&mut k, &rep(0, &[0x1E])), [Key::Char(b'1')], "caps does not touch digits");
    }

    #[test]
    fn a_held_key_gives_one_event_and_a_second_key_gives_one_more() {
        let mut k = HidKeyboard::new();
        assert_eq!(feed(&mut k, &rep(0, &[0x04])), [Key::Char(b'a')]);
        assert_eq!(feed(&mut k, &rep(0, &[0x04])), [], "same report again: still held");
        assert_eq!(feed(&mut k, &rep(0, &[0x04, 0x05])), [Key::Char(b'b')]);
        assert_eq!(feed(&mut k, &rep(0, &[0x05])), [], "a released, b still held");
        assert_eq!(feed(&mut k, &rep(0, &[0x04, 0x05])), [Key::Char(b'a')], "a pressed again");
    }

    #[test]
    fn two_keys_in_one_report_come_out_in_order() {
        let mut k = HidKeyboard::new();
        assert_eq!(feed(&mut k, &rep(0, &[0x08, 0x09])), [Key::Char(b'e'), Key::Char(b'f')]);
    }

    #[test]
    fn error_rollover_gives_nothing_and_keeps_the_held_keys() {
        let mut k = HidKeyboard::new();
        assert_eq!(feed(&mut k, &rep(0, &[0x04])), [Key::Char(b'a')]);
        assert_eq!(feed(&mut k, &rep(0, &[1, 1, 1, 1, 1, 1])), []);
        assert_eq!(feed(&mut k, &rep(0, &[0x04])), [], "a was still held, so it is not pressed again");
    }

    #[test]
    fn short_and_odd_reports_are_ignored() {
        let mut k = HidKeyboard::new();
        assert_eq!(feed(&mut k, &[0, 0, 4]), []);
        assert_eq!(feed(&mut k, &[]), []);
        // Unknown usage codes (function keys, keypad, media) give nothing and do not panic.
        assert_eq!(feed(&mut k, &rep(0, &[0x3A, 0x53, 0xE8, 0xFF])), []);
    }

    #[test]
    fn random_reports_never_panic() {
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut k = HidKeyboard::new();
        for _ in 0..20_000 {
            let r = next().to_le_bytes();
            let len = (next() % 12) as usize;
            let mut v = r.to_vec();
            v.extend_from_slice(&next().to_le_bytes());
            v.truncate(len);
            k.feed(&v, &mut |_| {});
        }
    }
}
