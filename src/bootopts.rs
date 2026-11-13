//! Boot options from the image's command line (UEFI load options), in the spirit of the Linux kernel
//! command line and Windows BCD switches. Pure, host-testable.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootOpts {
    /// Only warnings and errors.
    pub quiet: bool,
    /// Show trace-level messages.
    pub debug: bool,
    /// Seconds to wait for a key before booting (0 skips the boot menu).
    pub menu_timeout: u8,
    /// Show the log on screen during boot instead of the logo and progress bar.
    pub verbose: bool,
    /// Boot in recovery mode: log on screen, menu waits for a key, no timer or scheduler.
    pub recovery: bool,
    /// Do not count boots (kill switch for firmware whose variable writes misbehave).
    pub nocount: bool,
    /// Keep the legacy PIC/PIT timer even when a local APIC is available.
    pub nolapic: bool,
    /// Requested screen mode `(width, height)`; ignored when the firmware has no such mode.
    pub mode: Option<(u32, u32)>,
}

/// Seconds the menu waits before it starts the highlighted entry. Long enough to read and decide.
pub const DEFAULT_TIMEOUT: u8 = 15;
pub const MAX_TIMEOUT: u8 = 30;

impl Default for BootOpts {
    fn default() -> Self {
        Self { quiet: false, debug: false, menu_timeout: DEFAULT_TIMEOUT, verbose: false, recovery: false, nocount: false, nolapic: false, mode: None }
    }
}

/// Parses space-separated words: `quiet`, `debug`, `nomenu`, `timeout=N`. Unknown words are ignored.
pub fn parse(cmdline: &str) -> BootOpts {
    let mut o = BootOpts::default();
    for w in cmdline.split_whitespace() {
        match w {
            "quiet" => o.quiet = true,
            "debug" => o.debug = true,
            "nomenu" => o.menu_timeout = 0,
            "verbose" => o.verbose = true,
            "recovery" => o.recovery = true,
            "nocount" => o.nocount = true,
            "nolapic" => o.nolapic = true,
            _ => {
                if let Some(n) = w.strip_prefix("timeout=").and_then(|n| n.parse::<u8>().ok()) {
                    o.menu_timeout = n.min(MAX_TIMEOUT);
                }
                if let Some((w, h)) = w.strip_prefix("mode=").and_then(parse_mode) {
                    o.mode = Some((w, h));
                }
            }
        }
    }
    if o.quiet && o.debug {
        o.quiet = false; // contradictory: the more informative option wins
    }
    if o.debug {
        o.verbose = true; // debug output is for reading, so the splash would hide it
    }
    o
}

/// `1920x1080` to `(1920, 1080)`; sizes outside 320..=7680 by 200..=4320 are rejected.
fn parse_mode(s: &str) -> Option<(u32, u32)> {
    let (w, h) = s.split_once('x')?;
    let (w, h) = (w.parse::<u32>().ok()?, h.parse::<u32>().ok()?);
    ((320..=7680).contains(&w) && (200..=4320).contains(&h)).then_some((w, h))
}

/// Turns the text of a config file into one command-line string in `out`: lines starting with `#`
/// are comments, line breaks become spaces, anything outside printable ASCII is dropped. Returns
/// the number of bytes written (at most `out.len()`).
pub fn flatten_config(text: &[u8], out: &mut [u8]) -> usize {
    let mut n = 0;
    let mut cut = false;
    for line in text.split(|&b| b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.first() == Some(&b'#') {
            continue;
        }
        let mut wrote = false;
        for &b in line {
            if (0x20..0x7F).contains(&b) {
                if n < out.len() {
                    out[n] = b;
                    n += 1;
                    wrote = true;
                } else {
                    cut = true;
                }
            }
        }
        if wrote && n < out.len() {
            out[n] = b' ';
            n += 1;
        }
    }
    if cut {
        // The last token did not fit: drop it entirely rather than pass on a different option.
        while n > 0 && out[n - 1] != b' ' {
            n -= 1;
        }
    }
    while n > 0 && out[n - 1] == b' ' {
        n -= 1;
    }
    n
}

/// Hostile or garbage command lines must never panic the parser.
#[cfg(test)]
mod fuzz {
    use super::*;

    fn xorshift(s: &mut u64) -> u64 {
        *s ^= *s << 13;
        *s ^= *s >> 7;
        *s ^= *s << 17;
        *s
    }

    #[test]
    fn random_command_lines_do_not_panic() {
        let words = ["quiet", "debug", "nomenu", "timeout=", "timeout=5", "verbose", "=", "\u{0}", "\u{ff}", "é"];
        let mut s = 0x9E37_79B9_7F4A_7C15u64;
        for _ in 0..20_000 {
            let mut line = String::new();
            for _ in 0..(xorshift(&mut s) % 8) {
                line.push_str(words[(xorshift(&mut s) % words.len() as u64) as usize]);
                if xorshift(&mut s) % 3 != 0 {
                    line.push(' ');
                }
                if xorshift(&mut s) % 5 == 0 {
                    line.push_str(&(xorshift(&mut s) % 100_000).to_string());
                }
            }
            let o = parse(&line);
            assert!(o.menu_timeout <= MAX_TIMEOUT);
            assert!(!(o.quiet && o.debug));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        assert_eq!(parse(""), BootOpts::default());
    }

    #[test]
    fn words_and_timeout() {
        let o = parse("quiet timeout=7 junk");
        assert!(o.quiet && !o.debug);
        assert_eq!(o.menu_timeout, 7);
        assert_eq!(parse("nomenu").menu_timeout, 0);
    }

    #[test]
    fn timeout_is_clamped_and_garbage_ignored() {
        assert_eq!(parse("timeout=200").menu_timeout, 30);
        assert_eq!(parse("timeout=abc").menu_timeout, DEFAULT_TIMEOUT);
        assert_eq!(parse("timeout=999999").menu_timeout, DEFAULT_TIMEOUT);
    }

    #[test]
    fn verbose_and_debug_show_the_log() {
        assert!(!parse("").verbose);
        assert!(parse("verbose").verbose);
        assert!(parse("debug").verbose);
    }

    #[test]
    fn quiet_and_debug_conflict() {
        let o = parse("quiet debug");
        assert!(o.debug && !o.quiet);
    }
}

#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn mode_parses_and_rejects_nonsense() {
        assert_eq!(parse("mode=1920x1080").mode, Some((1920, 1080)));
        for bad in ["mode=", "mode=1920", "mode=axb", "mode=10x10", "mode=99999x99999", "mode=1920x", "mode=-1x-1"] {
            assert_eq!(parse(bad).mode, None, "{bad}");
        }
    }

    #[test]
    fn flatten_drops_a_token_that_does_not_fit() {
        let mut out = [0u8; 12];
        let n = flatten_config(b"verbose\ntimeout=10\n", &mut out);
        assert_eq!(&out[..n], b"verbose");
        let mut big = [0u8; 20];
        let n = flatten_config(b"verbose\ntimeout=10\n", &mut big);
        assert_eq!(&big[..n], b"verbose timeout=10");
    }

    #[test]
    fn nolapic_word() {
        assert!(parse("nolapic").nolapic);
        assert!(!parse("").nolapic);
        assert!(!parse("nolapic2 lapic").nolapic);
    }

    #[test]
    fn recovery_and_nocount_words() {
        let o = parse("recovery nocount");
        assert!(o.recovery && o.nocount);
        assert!(!parse("").recovery && !parse("").nocount);
    }

    #[test]
    fn config_is_flattened() {
        let mut out = [0u8; 64];
        let n = flatten_config(b"# comment\nquiet\r\n\ntimeout=5\n  # not a comment start\n", &mut out);
        assert_eq!(&out[..n], b"quiet timeout=5   # not a comment start");
        assert_eq!(flatten_config(b"", &mut out), 0);
        assert_eq!(flatten_config(b"\x00\x01\xff\n", &mut out), 0);
    }

    #[test]
    fn config_never_overruns_the_buffer() {
        let mut out = [0u8; 8];
        let n = flatten_config(b"aaaaaaaaaaaaaaaa\nbbbb\n", &mut out);
        assert!(n <= 8);
        let mut empty: [u8; 0] = [];
        assert_eq!(flatten_config(b"abc", &mut empty), 0);
    }
}
