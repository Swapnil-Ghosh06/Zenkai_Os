//! FAT file names for the writer: which names are allowed, the 8.3 short name every entry needs, the
//! `~1` numbering when two long names would share one, the checksum, and the long-name directory slots.
//! Pure logic, host-tested. ASCII only (a name with any other character is refused, not mangled).

use alloc::string::String;
use alloc::vec::Vec;

/// Size of one directory slot.
pub const SLOT: usize = 32;
/// Attribute byte that marks a long-name slot.
pub const ATTR_LFN: u8 = 0x0F;
/// Longest long name FAT allows.
pub const MAX_NAME: usize = 255;

/// The checksum a long name stores to say which short entry it belongs to.
pub fn checksum(short: &[u8; 11]) -> u8 {
    let mut s = 0u8;
    for &c in short {
        s = ((s & 1) << 7).wrapping_add(s >> 1).wrapping_add(c);
    }
    s
}

/// True if `name` may be stored: 1 to 255 printable ASCII characters, none of `\ / : * ? " < > |`, not `.`
/// or `..`, no leading space and no trailing space or dot.
pub fn valid_name(name: &str) -> bool {
    let b = name.as_bytes();
    if b.is_empty() || b.len() > MAX_NAME || name == "." || name == ".." {
        return false;
    }
    if b[0] == b' ' || b[b.len() - 1] == b' ' || b[b.len() - 1] == b'.' {
        return false;
    }
    b.iter().all(|&c| (0x20..0x7F).contains(&c) && !b"\\/:*?\"<>|".contains(&c))
}

/// Characters a short name may hold besides A to Z and 0 to 9.
fn short_symbol(c: u8) -> bool {
    b"$%'-_@~!(){}^#&".contains(&c)
}

/// The short name (8 + 3, space padded) if `name` already is one: upper-case letters, digits and the
/// allowed symbols, base up to 8, extension up to 3, so it needs no long-name slots.
pub fn exact_short(name: &str) -> Option<[u8; 11]> {
    if !valid_name(name) {
        return None;
    }
    let (base, ext) = match name.split_once('.') {
        Some((b, e)) => (b, e),
        None => (name, ""),
    };
    let legal = |p: &str| p.bytes().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || short_symbol(c));
    if base.is_empty() || base.len() > 8 || ext.len() > 3 || !legal(base) || !legal(ext) {
        return None; // (an extension holding a second dot fails `legal`)
    }
    let mut out = [b' '; 11];
    out[..base.len()].copy_from_slice(base.as_bytes());
    out[8..8 + ext.len()].copy_from_slice(ext.as_bytes());
    Some(out)
}

/// The short name to store for `name`, and whether long-name slots are needed. `taken` says whether a
/// short name is already used in the directory. `None` if `name` is invalid or no free `~N` is left.
pub fn make_short(name: &str, taken: impl Fn(&[u8; 11]) -> bool) -> Option<([u8; 11], bool)> {
    if !valid_name(name) {
        return None;
    }
    if let Some(short) = exact_short(name) {
        return if taken(&short) { None } else { Some((short, false)) };
    }
    // Leading dots go; the last dot starts the extension; spaces and the other dots are dropped;
    // characters a short name cannot hold become `_`.
    let name = name.trim_start_matches('.');
    let (base, ext) = match name.rsplit_once('.') {
        Some((b, e)) => (b, e),
        None => (name, ""),
    };
    let clean = |p: &str, max: usize| -> Vec<u8> {
        p.bytes()
            .filter(|&c| c != b' ' && c != b'.')
            .map(|c| c.to_ascii_uppercase())
            .map(|c| if c.is_ascii_uppercase() || c.is_ascii_digit() || short_symbol(c) { c } else { b'_' })
            .take(max)
            .collect()
    };
    let base = clean(base, 8);
    let ext = clean(ext, 3);
    for n in 1..=999_999u32 {
        let tail = alloc::format!("~{n}");
        let keep = base.len().min(8 - tail.len());
        let mut out = [b' '; 11];
        out[..keep].copy_from_slice(&base[..keep]);
        out[keep..keep + tail.len()].copy_from_slice(tail.as_bytes());
        out[8..8 + ext.len()].copy_from_slice(&ext);
        if !taken(&out) {
            return Some((out, true));
        }
    }
    None
}

/// The long-name slots for `name`, in the order they are stored on disk (last part first).
pub fn lfn_slots(name: &str, short: &[u8; 11]) -> Vec<[u8; SLOT]> {
    const AT: [usize; 13] = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];
    let chars = name.as_bytes();
    let parts = chars.len().div_ceil(13);
    let sum = checksum(short);
    let mut out = Vec::with_capacity(parts);
    for i in (0..parts).rev() {
        let mut s = [0u8; SLOT];
        s[0] = (i as u8 + 1) | if i + 1 == parts { 0x40 } else { 0 };
        s[11] = ATTR_LFN;
        s[13] = sum;
        for (k, &at) in AT.iter().enumerate() {
            let idx = i * 13 + k;
            let v: u16 = match idx.cmp(&chars.len()) {
                core::cmp::Ordering::Less => u16::from(chars[idx]),
                core::cmp::Ordering::Equal => 0,
                core::cmp::Ordering::Greater => 0xFFFF,
            };
            s[at..at + 2].copy_from_slice(&v.to_le_bytes());
        }
        out.push(s);
    }
    out
}

/// `README.TXT` for the stored `README  TXT`.
pub fn short_display(short: &[u8; 11]) -> String {
    let part = |b: &[u8]| -> String { b.iter().take_while(|&&c| c != b' ').map(|&c| char::from(c)).collect() };
    let (base, ext) = (part(&short[..8]), part(&short[8..]));
    if ext.is_empty() { base } else { alloc::format!("{base}.{ext}") }
}

/// Names are compared without regard to case, as FAT does.
pub fn same_name(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &[u8; 11]) -> bool {
        false
    }

    #[test]
    fn checksum_matches_independent_values() {
        // Computed with a separate Python implementation of the algorithm in the FAT specification.
        assert_eq!(checksum(b"README  TXT"), 0x73);
        assert_eq!(checksum(b"FOO     BAR"), 0x53);
        assert_eq!(checksum(b"AEON       "), 0x8d);
        assert_eq!(checksum(b"LONGNA~1TXT"), 0xf4);
    }

    #[test]
    fn valid_and_invalid_names() {
        for ok in ["a", "note.txt", "My File.txt", "a.b.c", ".hidden", "x".repeat(255).as_str(), "UPPER", "semi;colon", "a+b=c", "(x)", "#1"] {
            assert!(valid_name(ok), "{ok:?} should be accepted");
        }
        let long = "x".repeat(256);
        for bad in ["", ".", "..", " lead", "trail ", "trail.", "a/b", "a\\b", "a:b", "a*b", "a?b", "a\"b", "a<b", "a>b", "a|b", "tab\there", "nul\0", "caf\u{e9}", "\u{1F600}", long.as_str()] {
            assert!(!valid_name(bad), "{bad:?} should be refused");
        }
    }

    #[test]
    fn exact_short_names() {
        assert_eq!(exact_short("README.TXT"), Some(*b"README  TXT"));
        assert_eq!(exact_short("AEON"), Some(*b"AEON       "));
        assert_eq!(exact_short("A.B"), Some(*b"A       B  "));
        assert_eq!(exact_short("EIGHTCHR.EXT"), Some(*b"EIGHTCHREXT"));
        for no in ["readme.txt", "Readme.TXT", "NINECHARS.TXT", "A.LONG", "A.B.C", "HAS SPACE.TXT", ".HIDDEN", "A+B.TXT", "SEMI;.TXT", "", "CAF\u{c9}"] {
            assert_eq!(exact_short(no), None, "{no:?}");
        }
    }

    #[test]
    fn short_names_are_generated_with_a_tilde_number() {
        assert_eq!(make_short("readme.txt", none), Some((*b"README~1TXT", true)));
        assert_eq!(make_short("a long name.markdown", none), Some((*b"ALONGN~1MAR", true)));
        assert_eq!(make_short("a b.txt", none), Some((*b"AB~1    TXT", true)));
        assert_eq!(make_short("a.b.c", none), Some((*b"AB~1    C  ", true)));
        assert_eq!(make_short(".hidden", none), Some((*b"HIDDEN~1   ", true)));
        assert_eq!(make_short("noext-but-lower", none), Some((*b"NOEXT-~1   ", true)));
        assert_eq!(make_short("a+b;c.txt", none), Some((*b"A_B_C~1 TXT", true)));
    }

    #[test]
    fn an_exact_short_name_needs_no_long_slots() {
        assert_eq!(make_short("README.TXT", none), Some((*b"README  TXT", false)));
        // ...but not if that short name is already used (by the short name of some other long name).
        assert_eq!(make_short("README.TXT", |s| s == b"README  TXT"), None);
    }

    #[test]
    fn collisions_count_up_and_the_base_shrinks_to_make_room() {
        let used: Vec<[u8; 11]> = vec![*b"README~1TXT", *b"README~2TXT"];
        assert_eq!(make_short("readme.txt", |s| used.contains(s)), Some((*b"README~3TXT", true)));
        // ~1 to ~9 taken: the first free is ~10, which needs one more digit, so the base loses a character.
        let nine = |s: &[u8; 11]| s[..8].starts_with(b"LONGNA~") && (b'1'..=b'9').contains(&s[7]);
        assert_eq!(make_short("longname.txt", nine), Some((*b"LONGN~10TXT", true)));
    }

    #[test]
    fn running_out_of_numbers_is_an_error_not_a_loop() {
        assert_eq!(make_short("readme.txt", |_| true), None);
    }

    #[test]
    fn invalid_names_get_no_short_name() {
        for bad in ["", ".", "..", "a/b", "caf\u{e9}"] {
            assert_eq!(make_short(bad, none), None, "{bad:?}");
        }
    }

    #[test]
    fn one_long_slot_for_a_short_long_name() {
        let slots = lfn_slots("readme.txt", b"README~1TXT");
        assert_eq!(slots.len(), 1);
        let s = &slots[0];
        assert_eq!(s[0], 0x41, "first and last part");
        assert_eq!(s[11], ATTR_LFN);
        assert_eq!(s[12], 0);
        assert_eq!(s[13], checksum(b"README~1TXT"));
        assert_eq!(&s[26..28], &[0, 0], "cluster field is zero");
        let chars: Vec<u16> = [1usize, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30].iter().map(|&o| u16::from_le_bytes([s[o], s[o + 1]])).collect();
        let want: Vec<u16> = "readme.txt".encode_utf16().chain([0, 0xFFFF, 0xFFFF]).collect();
        assert_eq!(chars, want);
    }

    #[test]
    fn long_names_are_split_in_thirteens_last_part_first() {
        let name = "abcdefghijklmn"; // 14 characters: 13 + 1
        let slots = lfn_slots(name, b"ABCDEF~1   ");
        assert_eq!(slots.len(), 2);
        assert_eq!(slots[0][0], 0x42, "the stored-first slot is the last part");
        assert_eq!(slots[1][0], 0x01);
        assert_eq!(u16::from_le_bytes([slots[0][1], slots[0][2]]), u16::from(b'n'));
        assert_eq!(u16::from_le_bytes([slots[0][3], slots[0][4]]), 0, "terminator right after the name");
        assert_eq!(u16::from_le_bytes([slots[0][5], slots[0][6]]), 0xFFFF);
        assert_eq!(u16::from_le_bytes([slots[1][1], slots[1][2]]), u16::from(b'a'));
        assert!(slots.iter().all(|s| s[13] == checksum(b"ABCDEF~1   ")));
    }

    #[test]
    fn exactly_thirteen_characters_have_no_terminator() {
        let slots = lfn_slots("abcdefghijklm", b"ABCDEF~1   ");
        assert_eq!(slots.len(), 1);
        assert_eq!(u16::from_le_bytes([slots[0][28], slots[0][29]]), u16::from(b'l'));
        assert_eq!(u16::from_le_bytes([slots[0][30], slots[0][31]]), u16::from(b'm'));
    }

    #[test]
    fn the_longest_name_needs_twenty_slots() {
        let name = "x".repeat(255);
        let slots = lfn_slots(&name, b"XXXXXX~1   ");
        assert_eq!(slots.len(), 20);
        assert_eq!(slots[0][0], 0x40 | 20);
        assert_eq!(slots[19][0], 1);
    }

    #[test]
    fn short_names_display_like_a_person_writes_them() {
        assert_eq!(short_display(b"README  TXT"), "README.TXT");
        assert_eq!(short_display(b"AEON       "), "AEON");
        assert_eq!(short_display(b"A       B  "), "A.B");
    }

    #[test]
    fn names_compare_without_case() {
        assert!(same_name("Note.TXT", "note.txt"));
        assert!(!same_name("note", "notes"));
    }

    #[test]
    fn random_strings_never_panic() {
        let mut x = 0x2545_F491_4F6C_DD1Du64;
        for _ in 0..2000 {
            let mut s = String::new();
            for _ in 0..(x % 20) {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                s.push(char::from_u32((x % 0x300) as u32).unwrap_or('?'));
            }
            if let Some((short, _)) = make_short(&s, none) {
                assert!(valid_name(&s));
                assert!(short.iter().all(|&b| (0x20..0x7F).contains(&b)));
            }
        }
    }
}
