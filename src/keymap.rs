//! PS/2 scancode set 1 to characters, with shift/caps/ctrl state. Pure, host-testable.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(u8),
    Enter,
    Backspace,
    Tab,
    Esc,
    /// Ctrl+C
    Interrupt,
    /// Ctrl+L
    ClearScreen,
    /// Plain arrows scroll the screen; Ctrl+Up / Ctrl+Down walk the command history.
    Up,
    Down,
    HistoryPrev,
    HistoryNext,
    PageUp,
    PageDown,
    Home,
    End,
    Left,
    Right,
    Delete,
    /// Ctrl+Home / Ctrl+End: scroll to the oldest line / back to the live bottom.
    ScrollTop,
    ScrollBottom,
    /// Ctrl plus a letter other than C and L (the letter is lower case).
    Ctrl(u8),
}

#[derive(Default)]
pub struct Keyboard {
    shift: bool,
    ctrl: bool,
    caps: bool,
    extended: bool,
}

const LOWER: &[u8; 58] = b"\0\x1b1234567890-=\x08\tqwertyuiop[]\n\0asdfghjkl;'`\0\\zxcvbnm,./\0*\0 ";
const UPPER: &[u8; 58] = b"\0\x1b!@#$%^&*()_+\x08\tQWERTYUIOP{}\n\0ASDFGHJKL:\"~\0|ZXCVBNM<>?\0*\0 ";

impl Keyboard {
    pub const fn new() -> Self {
        Self { shift: false, ctrl: false, caps: false, extended: false }
    }

    /// Feeds one raw byte from port 0x60; returns a key on a key press.
    pub fn feed(&mut self, code: u8) -> Option<Key> {
        if code == 0xE0 {
            self.extended = true;
            return None;
        }
        let ext = core::mem::take(&mut self.extended);
        let release = code & 0x80 != 0;
        let make = code & 0x7F;
        match make {
            0x2A | 0x36 if !ext => {
                self.shift = !release;
                return None;
            }
            0x1D => {
                self.ctrl = !release;
                return None;
            }
            0x3A if !release => {
                self.caps = !self.caps;
                return None;
            }
            _ => {}
        }
        if ext {
            if release {
                return None;
            }
            // The dedicated cursor block sends E0 + these codes (Num Lock and Shift add fake E0 2A / E0 AA
            // prefixes, which fall through the shift arm above because of `!ext`).
            return match make {
                0x48 => Some(if self.ctrl { Key::HistoryPrev } else { Key::Up }),
                0x50 => Some(if self.ctrl { Key::HistoryNext } else { Key::Down }),
                0x49 => Some(Key::PageUp),
                0x51 => Some(Key::PageDown),
                0x47 => Some(if self.ctrl { Key::ScrollTop } else { Key::Home }),
                0x4F => Some(if self.ctrl { Key::ScrollBottom } else { Key::End }),
                0x4B => Some(Key::Left),
                0x4D => Some(Key::Right),
                0x53 => Some(Key::Delete),
                _ => None,
            };
        }
        if release {
            return None;
        }
        let table = if self.shift { UPPER } else { LOWER };
        let mut c = *table.get(make as usize)?;
        if self.caps && c.is_ascii_alphabetic() {
            c = if self.shift { c.to_ascii_lowercase() } else { c.to_ascii_uppercase() };
        }
        if self.ctrl {
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

    #[test]
    fn plain_and_shifted_letters() {
        let mut k = Keyboard::new();
        assert_eq!(k.feed(0x1E), Some(Key::Char(b'a')));
        assert_eq!(k.feed(0x9E), None); // release
        k.feed(0x2A); // shift down
        assert_eq!(k.feed(0x1E), Some(Key::Char(b'A')));
        assert_eq!(k.feed(0x02), Some(Key::Char(b'!')));
        k.feed(0xAA); // shift up
        assert_eq!(k.feed(0x02), Some(Key::Char(b'1')));
    }

    #[test]
    fn caps_lock_inverts_with_shift() {
        let mut k = Keyboard::new();
        k.feed(0x3A);
        assert_eq!(k.feed(0x10), Some(Key::Char(b'Q')));
        k.feed(0x2A);
        assert_eq!(k.feed(0x10), Some(Key::Char(b'q')));
    }

    #[test]
    fn control_keys() {
        let mut k = Keyboard::new();
        assert_eq!(k.feed(0x1C), Some(Key::Enter));
        assert_eq!(k.feed(0x0E), Some(Key::Backspace));
        k.feed(0x1D);
        assert_eq!(k.feed(0x2E), Some(Key::Interrupt));
        assert_eq!(k.feed(0x26), Some(Key::ClearScreen));
    }

    #[test]
    fn extended_and_unknown_codes_are_ignored() {
        let mut k = Keyboard::new();
        assert_eq!(k.feed(0xE0), None);
        assert_eq!(k.feed(0x5F), None); // unknown extended key
        assert_eq!(k.feed(0x5F), None); // beyond the table
        assert_eq!(k.feed(0x1E), Some(Key::Char(b'a')));
    }

    #[test]
    fn cursor_block_keys() {
        let mut k = Keyboard::new();
        for (code, key) in [(0x48, Key::Up), (0x50, Key::Down), (0x49, Key::PageUp), (0x51, Key::PageDown), (0x47, Key::Home), (0x4F, Key::End)] {
            assert_eq!(k.feed(0xE0), None);
            assert_eq!(k.feed(code), Some(key));
            assert_eq!(k.feed(0xE0), None);
            assert_eq!(k.feed(code | 0x80), None, "release gives nothing");
        }
    }

    #[test]
    fn fake_shift_prefix_does_not_stick_or_swallow_the_key() {
        // With Num Lock on the keyboard sends E0 2A before E0 49 and E0 B7 after the release.
        let mut k = Keyboard::new();
        assert_eq!(k.feed(0xE0), None);
        assert_eq!(k.feed(0x2A), None);
        assert_eq!(k.feed(0xE0), None);
        assert_eq!(k.feed(0x49), Some(Key::PageUp));
        assert_eq!(k.feed(0xE0), None);
        assert_eq!(k.feed(0xC9), None);
        assert_eq!(k.feed(0xE0), None);
        assert_eq!(k.feed(0xAA), None);
        assert_eq!(k.feed(0x1E), Some(Key::Char(b'a')), "shift is not left on");
    }

    #[test]
    fn ctrl_arrows_are_history() {
        let mut k = Keyboard::new();
        k.feed(0x1D); // Ctrl down
        k.feed(0xE0);
        assert_eq!(k.feed(0x48), Some(Key::HistoryPrev));
        k.feed(0xE0);
        assert_eq!(k.feed(0x50), Some(Key::HistoryNext));
        k.feed(0x9D); // Ctrl up
        k.feed(0xE0);
        assert_eq!(k.feed(0x48), Some(Key::Up));
    }

    #[test]
    fn editing_keys_and_ctrl_letters() {
        let mut k = Keyboard::new();
        for (code, key) in [(0x4B, Key::Left), (0x4D, Key::Right), (0x53, Key::Delete)] {
            k.feed(0xE0);
            assert_eq!(k.feed(code), Some(key));
        }
        // Ctrl+A is a control key, not the letter a.
        k.feed(0x1D);
        assert_eq!(k.feed(0x1E), Some(Key::Ctrl(b'a')));
        assert_eq!(k.feed(0x12), Some(Key::Ctrl(b'e')));
        // Ctrl+Home / Ctrl+End scroll; without Ctrl they edit.
        k.feed(0xE0);
        assert_eq!(k.feed(0x47), Some(Key::ScrollTop));
        k.feed(0xE0);
        assert_eq!(k.feed(0x4F), Some(Key::ScrollBottom));
        k.feed(0x9D); // Ctrl up
        k.feed(0xE0);
        assert_eq!(k.feed(0x47), Some(Key::Home));
        assert_eq!(k.feed(0x1E), Some(Key::Char(b'a')));
    }

    #[test]
    fn keypad_digits_are_not_arrows_when_not_extended() {
        let mut k = Keyboard::new();
        assert_eq!(k.feed(0x48), None); // keypad 8 without the E0 prefix
    }
}
