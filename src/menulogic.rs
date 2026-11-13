//! What the Dawn boot menu does, with no screen and no clock of its own: which entry is highlighted, how the
//! countdown behaves and what a key press means. `main.rs` feeds it keys and elapsed milliseconds and draws
//! what it says. Pure logic, host-tested.
//!
//! Rules (the owner's wishes, checked by the tests below):
//! - the menu opens with "Boot Aeon" highlighted and a countdown of `timeout` seconds;
//! - arrow keys move the highlight and restart the countdown (a full `timeout` after your last key);
//! - the countdown keeps running whichever entry is highlighted, so the menu never looks frozen;
//! - when it ends, the highlighted entry runs if it is safe to start by itself (`Boot`, `Recovery`); on any
//!   other entry (firmware shell, debug, firmware setup, restart, power off) Aeon boots normally instead, so
//!   nothing ever restarts, powers off or changes settings without an Enter;
//! - Enter runs the highlighted entry at once; any other key (Escape, a letter) stops the countdown.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    Boot,
    Recovery,
    FirmwareShell,
    Debug,
    FirmwareSetup,
    Restart,
    PowerOff,
}

pub const ENTRIES: [Entry; 7] = [Entry::Boot, Entry::Recovery, Entry::FirmwareShell, Entry::Debug, Entry::FirmwareSetup, Entry::Restart, Entry::PowerOff];

impl Entry {
    pub fn label(self, debug_on: bool) -> &'static str {
        match self {
            Entry::Boot => "Boot Zenkai OS",
            Entry::Recovery => "Recovery mode",
            Entry::FirmwareShell => "Firmware shell",
            Entry::Debug if debug_on => "Debug logging: on",
            Entry::Debug => "Debug logging: off",
            Entry::FirmwareSetup => "Firmware setup",
            Entry::Restart => "Restart",
            Entry::PowerOff => "Power off",
        }
    }

    /// The one line shown under the list while the entry is highlighted.
    pub fn description(self) -> &'static str {
        match self {
            Entry::Boot => "Start Aeon normally.",
            Entry::Recovery => "Start with messages on screen and no scheduler, for repairs.",
            Entry::FirmwareShell => "Open the UEFI command shell.",
            Entry::Debug => "Turn detailed messages during boot on or off.",
            Entry::FirmwareSetup => "Restart into the computer's firmware settings.",
            Entry::Restart => "Restart the computer.",
            Entry::PowerOff => "Shut the computer down.",
        }
    }

    /// Whether the countdown may start this entry when it reaches zero.
    pub fn starts_by_itself(self) -> bool {
        matches!(self, Entry::Boot | Entry::Recovery)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Home,
    End,
    Enter,
    /// Escape or any other key: stops the countdown, does nothing else.
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing to repaint.
    Idle,
    /// Only the countdown line changed.
    Footer,
    /// The highlight (or the countdown line and the description) changed: repaint the list and footer.
    List,
    /// Do this entry now.
    Run(Entry),
}

#[derive(Clone, Copy, Debug)]
pub struct Menu {
    sel: usize,
    timeout_ms: u32,
    elapsed_ms: u32,
    counting: bool,
}

impl Menu {
    pub fn new(timeout_secs: u8) -> Menu {
        Menu { sel: 0, timeout_ms: u32::from(timeout_secs) * 1000, elapsed_ms: 0, counting: timeout_secs > 0 }
    }

    pub fn selected(&self) -> usize {
        self.sel
    }

    pub fn selected_entry(&self) -> Entry {
        ENTRIES[self.sel]
    }

    /// Whole seconds left on the countdown (rounded up, so "1" shows until the very end), if it is running.
    pub fn countdown(&self) -> Option<u8> {
        if !self.counting {
            return None;
        }
        let left = self.timeout_ms.saturating_sub(self.elapsed_ms);
        Some(left.div_ceil(1000) as u8)
    }

    fn move_to(&mut self, sel: usize) -> Outcome {
        self.sel = sel;
        if self.counting {
            self.elapsed_ms = 0; // a full countdown after your last key
        }
        Outcome::List
    }

    /// What the countdown starts when it ends: the highlighted entry when it is safe to start by itself,
    /// otherwise Aeon itself (never a restart, a power off or a settings change).
    pub fn timeout_target(&self) -> Entry {
        if ENTRIES[self.sel].starts_by_itself() {
            ENTRIES[self.sel]
        } else {
            Entry::Boot
        }
    }

    pub fn key(&mut self, key: Key) -> Outcome {
        let n = ENTRIES.len();
        match key {
            Key::Up => self.move_to((self.sel + n - 1) % n),
            Key::Down => self.move_to((self.sel + 1) % n),
            Key::Home => self.move_to(0),
            Key::End => self.move_to(n - 1),
            Key::Enter => Outcome::Run(ENTRIES[self.sel]),
            Key::Other => {
                if self.counting {
                    self.counting = false;
                    Outcome::Footer
                } else {
                    Outcome::Idle
                }
            }
        }
    }

    /// Time passed. Returns `Run(timeout_target)` when the countdown ends.
    pub fn tick(&mut self, ms: u32) -> Outcome {
        if !self.counting {
            return Outcome::Idle;
        }
        let before = self.countdown();
        self.elapsed_ms = self.elapsed_ms.saturating_add(ms);
        if self.elapsed_ms >= self.timeout_ms {
            self.counting = false;
            return Outcome::Run(self.timeout_target());
        }
        if self.countdown() != before {
            Outcome::Footer
        } else {
            Outcome::Idle
        }
    }

    /// The line under the menu: the countdown (naming what will start) or the key help.
    pub fn footer(&self, out: &mut FooterBuf) {
        out.clear();
        use core::fmt::Write;
        let target = self.timeout_target().label(false);
        // When the highlighted entry cannot start by itself, say what the countdown will do instead.
        let pick = self.selected_entry();
        let _ = match (self.countdown(), pick == self.timeout_target()) {
            (Some(1), true) => write!(out, "{target} in 1 second"),
            (Some(n), true) => write!(out, "{target} in {n} seconds"),
            (Some(1), false) => write!(out, "{target} in 1 second, or press Enter for {}", pick.label(false)),
            (Some(n), false) => write!(out, "{target} in {n} seconds, or press Enter for {}", pick.label(false)),
            (None, _) => write!(out, "Use the arrow keys, then press Enter"),
        };
    }
}

/// A small fixed buffer for the footer text, so no allocation is needed while the menu runs.
pub struct FooterBuf {
    buf: [u8; 80],
    len: usize,
}

impl Default for FooterBuf {
    fn default() -> Self {
        Self::new()
    }
}

impl FooterBuf {
    pub const fn new() -> FooterBuf {
        FooterBuf { buf: [0; 80], len: 0 }
    }
    pub fn clear(&mut self) {
        self.len = 0;
    }
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

impl core::fmt::Write for FooterBuf {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let b = s.as_bytes();
        if self.len + b.len() > self.buf.len() {
            return Err(core::fmt::Error);
        }
        self.buf[self.len..self.len + b.len()].copy_from_slice(b);
        self.len += b.len();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn footer(m: &Menu) -> String {
        let mut b = FooterBuf::new();
        m.footer(&mut b);
        b.as_str().to_string()
    }

    #[test]
    fn opens_on_boot_with_the_full_countdown() {
        let m = Menu::new(15);
        assert_eq!(m.selected_entry(), Entry::Boot);
        assert_eq!(m.countdown(), Some(15));
        assert_eq!(footer(&m), "Boot Zenkai OS in 15 seconds");
    }

    #[test]
    fn countdown_ticks_down_and_boots_at_zero() {
        let mut m = Menu::new(15);
        assert_eq!(m.tick(500), Outcome::Idle); // still "15"
        assert_eq!(m.countdown(), Some(15));
        assert_eq!(m.tick(500), Outcome::Footer); // 14 s left
        assert_eq!(m.countdown(), Some(14));
        assert_eq!(m.tick(12_999), Outcome::Footer); // 1.001 s left, shows 2
        assert_eq!(m.countdown(), Some(2));
        assert_eq!(m.tick(1), Outcome::Footer);
        assert_eq!(m.countdown(), Some(1));
        assert_eq!(footer(&m), "Boot Zenkai OS in 1 second");
        assert_eq!(m.tick(999), Outcome::Idle);
        assert_eq!(m.tick(1), Outcome::Run(Entry::Boot));
        assert_eq!(m.tick(1000), Outcome::Idle, "runs only once");
    }

    #[test]
    fn the_highlighted_entry_runs_and_each_move_restarts_the_countdown() {
        let mut m = Menu::new(15);
        m.tick(10_000);
        assert_eq!(m.key(Key::Down), Outcome::List);
        assert_eq!(m.selected_entry(), Entry::Recovery);
        assert_eq!(m.countdown(), Some(15), "full countdown after the last key");
        assert_eq!(footer(&m), "Recovery mode in 15 seconds");
        assert_eq!(m.tick(14_999), Outcome::Footer);
        assert_eq!(m.tick(1), Outcome::Run(Entry::Recovery));
    }

    #[test]
    fn the_countdown_keeps_running_on_every_entry_and_falls_back_to_boot() {
        let mut m = Menu::new(15);
        m.key(Key::Down);
        m.key(Key::Down);
        assert_eq!(m.selected_entry(), Entry::FirmwareShell);
        assert_eq!(m.countdown(), Some(15), "not frozen: a full countdown after the last key");
        assert_eq!(footer(&m), "Boot Zenkai OS in 15 seconds, or press Enter for Firmware shell");
        assert_eq!(m.tick(1000), Outcome::Footer);
        assert_eq!(footer(&m), "Boot Zenkai OS in 14 seconds, or press Enter for Firmware shell");
        assert_eq!(m.tick(13_000), Outcome::Footer);
        assert_eq!(footer(&m), "Boot Zenkai OS in 1 second, or press Enter for Firmware shell");
        assert_eq!(m.tick(1000), Outcome::Run(Entry::Boot), "the shell is not started; Aeon boots");
        // moving on every entry keeps restarting the full countdown
        let mut m = Menu::new(15);
        for _ in 0..6 {
            m.tick(5000);
            m.key(Key::Down);
            assert_eq!(m.countdown(), Some(15));
        }
        assert_eq!(m.selected_entry(), Entry::PowerOff);
    }

    #[test]
    fn nothing_dangerous_ever_runs_from_the_countdown() {
        for (i, e) in ENTRIES.iter().enumerate() {
            let mut m = Menu::new(5);
            m.sel = i; // as if reached some other way
            let out = m.tick(u32::MAX);
            if e.starts_by_itself() {
                assert_eq!(out, Outcome::Run(*e));
            } else {
                assert_eq!(out, Outcome::Run(Entry::Boot), "{e:?} must never start by itself: Aeon boots instead");
            }
        }
        assert!(!Entry::Restart.starts_by_itself());
        assert!(!Entry::PowerOff.starts_by_itself());
        assert!(!Entry::FirmwareSetup.starts_by_itself());
    }

    #[test]
    fn enter_runs_the_highlighted_entry_at_once() {
        let mut m = Menu::new(15);
        assert_eq!(m.key(Key::Enter), Outcome::Run(Entry::Boot));
        m.key(Key::End);
        assert_eq!(m.selected_entry(), Entry::PowerOff);
        assert_eq!(m.key(Key::Enter), Outcome::Run(Entry::PowerOff));
    }

    #[test]
    fn any_other_key_stops_the_countdown() {
        let mut m = Menu::new(15);
        assert_eq!(m.key(Key::Other), Outcome::Footer);
        assert_eq!(m.countdown(), None);
        assert_eq!(m.tick(60_000), Outcome::Idle);
        assert_eq!(m.key(Key::Other), Outcome::Idle);
        assert_eq!(m.selected_entry(), Entry::Boot);
    }

    #[test]
    fn up_and_down_wrap_and_home_end_jump() {
        let mut m = Menu::new(15);
        m.key(Key::Up);
        assert_eq!(m.selected_entry(), Entry::PowerOff);
        m.key(Key::Down);
        assert_eq!(m.selected_entry(), Entry::Boot);
        m.key(Key::End);
        assert_eq!(m.selected_entry(), Entry::PowerOff);
        m.key(Key::Home);
        assert_eq!(m.selected_entry(), Entry::Boot);
        assert_eq!(m.selected(), 0);
    }

    #[test]
    fn timeout_zero_never_counts() {
        let mut m = Menu::new(0);
        assert_eq!(m.countdown(), None);
        assert_eq!(m.tick(100_000), Outcome::Idle);
    }

    #[test]
    fn huge_time_steps_do_not_overflow() {
        let mut m = Menu::new(30);
        m.tick(u32::MAX);
        let mut m = Menu::new(30);
        m.tick(u32::MAX - 5);
        m.tick(u32::MAX);
        assert_eq!(m.tick(u32::MAX), Outcome::Idle);
    }

    #[test]
    fn labels_and_descriptions_are_present_and_plain() {
        assert_eq!(ENTRIES.len(), 7);
        for e in ENTRIES {
            assert!(!e.label(false).is_empty() && !e.label(true).is_empty());
            assert!(e.description().len() < 70, "{e:?} description must fit on one line");
            assert!(e.description().ends_with('.'));
        }
        assert_eq!(Entry::Debug.label(true), "Debug logging: on");
        assert_eq!(Entry::Debug.label(false), "Debug logging: off");
    }

    #[test]
    fn random_key_and_time_sequences_keep_the_rules() {
        let mut x: u64 = 0xfeed_beef_1234_5678;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for _ in 0..2000 {
            let mut m = Menu::new((next() % 31) as u8);
            for _ in 0..200 {
                let out = match next() % 8 {
                    0 => m.key(Key::Up),
                    1 => m.key(Key::Down),
                    2 => m.key(Key::Home),
                    3 => m.key(Key::End),
                    4 => m.key(Key::Other),
                    5 => m.key(Key::Enter),
                    _ => m.tick((next() % 3000) as u32),
                };
                assert!(m.selected() < ENTRIES.len());
                if let Outcome::Run(e) = out {
                    // Enter runs the highlighted entry; the countdown only ever runs a safe one.
                    assert!(e == m.selected_entry() || e == Entry::Boot);
                }
            }
        }
    }
}
