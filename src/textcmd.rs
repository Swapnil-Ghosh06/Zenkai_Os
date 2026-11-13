//! The text-handling parts of the shell commands `head`, `tail`, `wc`, `grep`, `seq` and `hexdump`, plus the
//! argument splitter they share. Pure logic over byte slices, host-tested; the shell only reads files and
//! prints what these return. Aeon has no pipes, so every command takes file names.

use alloc::string::String;
use alloc::vec::Vec;

/// Splits a command's arguments on spaces; a word in double quotes may hold spaces (`grep "two words" f`).
/// An unterminated quote runs to the end of the line. Empty quotes give an empty word.
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    let mut have = false;
    for ch in s.chars() {
        match ch {
            '"' => {
                in_quote = !in_quote;
                have = true;
            }
            ' ' if !in_quote => {
                if have {
                    out.push(core::mem::take(&mut cur));
                    have = false;
                }
            }
            c => {
                cur.push(c);
                have = true;
            }
        }
    }
    if have {
        out.push(cur);
    }
    out
}

/// The lines of `data`: split at `\n`, no empty last line when the data ends with a newline.
pub fn lines(data: &[u8]) -> impl Iterator<Item = &[u8]> {
    let body = data.strip_suffix(b"\n").unwrap_or(data);
    let none = data.is_empty();
    body.split(|&b| b == b'\n').filter(move |_| !none)
}

/// The first `n` lines (with their newlines), as a prefix of `data`.
pub fn head(data: &[u8], n: usize) -> &[u8] {
    let mut seen = 0;
    for (i, &b) in data.iter().enumerate() {
        if b == b'\n' {
            seen += 1;
            if seen == n {
                return &data[..=i];
            }
        }
    }
    if n == 0 {
        &data[..0]
    } else {
        data
    }
}

/// The last `n` lines, as a suffix of `data`.
pub fn tail(data: &[u8], n: usize) -> &[u8] {
    if n == 0 {
        return &data[data.len()..];
    }
    // A final newline ends the last line instead of starting an empty one.
    let end = if data.ends_with(b"\n") { data.len() - 1 } else { data.len() };
    let mut seen = 0;
    for i in (0..end).rev() {
        if data[i] == b'\n' {
            seen += 1;
            if seen == n {
                return &data[i + 1..];
            }
        }
    }
    data
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Counts {
    pub lines: usize,
    pub words: usize,
    pub bytes: usize,
}

/// Like `wc`: newline count, whitespace-separated words, bytes.
pub fn wc(data: &[u8]) -> Counts {
    let mut words = 0;
    let mut in_word = false;
    for &b in data {
        let space = matches!(b, b' ' | b'\n' | b'\t' | b'\r' | 0x0b | 0x0c);
        if space {
            in_word = false;
        } else if !in_word {
            in_word = true;
            words += 1;
        }
    }
    Counts { lines: data.iter().filter(|&&b| b == b'\n').count(), words, bytes: data.len() }
}

/// Plain substring search (not a regular expression), optionally ignoring ASCII case. An empty needle matches.
pub fn contains(hay: &[u8], needle: &[u8], ignore_case: bool) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > hay.len() {
        return false;
    }
    hay.windows(needle.len()).any(|w| if ignore_case { w.eq_ignore_ascii_case(needle) } else { w == needle })
}

/// One line of `hexdump`: offset, up to 16 bytes in hex, then the printable characters.
pub fn hexdump_line(offset: usize, chunk: &[u8]) -> String {
    use core::fmt::Write;
    let mut s = String::new();
    let _ = write!(s, "{offset:08x} ");
    for i in 0..16 {
        if i == 8 {
            s.push(' ');
        }
        match chunk.get(i) {
            Some(b) => {
                let _ = write!(s, " {b:02x}");
            }
            None => s.push_str("   "),
        }
    }
    s.push_str("  |");
    for &b in chunk.iter().take(16) {
        s.push(if (0x20..0x7f).contains(&b) { b as char } else { '.' });
    }
    s.push('|');
    s
}

/// What `seq` prints: `(first, last)` from one or two numbers. At most `max` numbers, or an error text.
pub fn seq_range(args: &[String], max: u64) -> Result<(i64, i64), &'static str> {
    let num = |s: &String| s.parse::<i64>().map_err(|_| "seq: numbers only");
    let (first, last) = match args {
        [last] => (1, num(last)?),
        [first, last] => (num(first)?, num(last)?),
        _ => return Err("usage: seq [first] <last>"),
    };
    if last >= first && (last as i128 - first as i128) as u128 >= u128::from(max) {
        return Err("seq: too many numbers (at most 10000)");
    }
    Ok((first, last))
}

/// Parses `[-n N] <rest...>` for `head` and `tail`: the count (default 10) and the remaining words.
pub fn count_option(args: &[String]) -> Result<(usize, &[String]), &'static str> {
    match args {
        [flag, n, rest @ ..] if flag == "-n" => {
            let n = n.parse::<usize>().map_err(|_| "the number after -n must be a whole number")?;
            Ok((n, rest))
        }
        [flag, ..] if flag == "-n" => Err("-n needs a number"),
        _ => Ok((10, args)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| String::from(*s)).collect()
    }

    #[test]
    fn split_args_handles_quotes_and_extra_spaces() {
        assert_eq!(split_args("a  b   c"), v(&["a", "b", "c"]));
        assert_eq!(split_args("\"two words\" file"), v(&["two words", "file"]));
        assert_eq!(split_args("x\"y z\"w"), v(&["xy zw"]));
        assert_eq!(split_args("\"\" f"), v(&["", "f"]));
        assert_eq!(split_args("\"open quote runs on"), v(&["open quote runs on"]));
        assert_eq!(split_args("   "), Vec::<String>::new());
        assert_eq!(split_args(""), Vec::<String>::new());
    }

    #[test]
    fn lines_splits_without_a_phantom_last_line() {
        let l: Vec<&[u8]> = lines(b"a\nb\n").collect();
        assert_eq!(l, vec![&b"a"[..], &b"b"[..]]);
        let l: Vec<&[u8]> = lines(b"a\nb").collect();
        assert_eq!(l, vec![&b"a"[..], &b"b"[..]]);
        assert_eq!(lines(b"").count(), 0);
        assert_eq!(lines(b"\n").count(), 1, "one empty line");
        assert_eq!(lines(b"\n\n").count(), 2);
    }

    #[test]
    fn head_takes_the_first_lines() {
        let d = b"1\n2\n3\n4\n";
        assert_eq!(head(d, 2), b"1\n2\n");
        assert_eq!(head(d, 4), d);
        assert_eq!(head(d, 10), d);
        assert_eq!(head(d, 0), b"");
        assert_eq!(head(b"no newline", 3), b"no newline");
        assert_eq!(head(b"", 3), b"");
    }

    #[test]
    fn tail_takes_the_last_lines() {
        let d = b"1\n2\n3\n4\n";
        assert_eq!(tail(d, 2), b"3\n4\n");
        assert_eq!(tail(d, 1), b"4\n");
        assert_eq!(tail(d, 4), d);
        assert_eq!(tail(d, 99), d);
        assert_eq!(tail(d, 0), b"");
        assert_eq!(tail(b"1\n2\n3", 2), b"2\n3");
        assert_eq!(tail(b"only", 1), b"only");
        assert_eq!(tail(b"", 1), b"");
        assert_eq!(tail(b"\n", 1), b"\n");
    }

    #[test]
    fn wc_counts_lines_words_and_bytes() {
        assert_eq!(wc(b""), Counts { lines: 0, words: 0, bytes: 0 });
        assert_eq!(wc(b"hello world\nfoo\n"), Counts { lines: 2, words: 3, bytes: 16 });
        assert_eq!(wc(b"  spaced   out  "), Counts { lines: 0, words: 2, bytes: 16 });
        assert_eq!(wc(b"tab\tseparated\r\nline"), Counts { lines: 1, words: 3, bytes: 19 });
    }

    #[test]
    fn contains_is_a_plain_search_with_optional_case_folding() {
        assert!(contains(b"Hello World", b"World", false));
        assert!(!contains(b"Hello World", b"world", false));
        assert!(contains(b"Hello World", b"world", true));
        assert!(contains(b"abc", b"", false));
        assert!(!contains(b"ab", b"abc", false));
        assert!(contains(b"a.c", b".", false), "dot is a dot, not a wildcard");
        assert!(!contains(b"abc", b"a.c", false));
    }

    #[test]
    fn hexdump_lines_look_like_xxd() {
        let l = hexdump_line(0, b"Hello, Aeon!\n\x00\x01\x02");
        assert_eq!(l, "00000000  48 65 6c 6c 6f 2c 20 41  65 6f 6e 21 0a 00 01 02  |Hello, Aeon!....|");
        let short = hexdump_line(0x10, b"ab");
        assert_eq!(short, "00000010  61 62                                             |ab|");
        assert_eq!(short.len(), l.len() - 14, "the text column is not padded for a short line");
    }

    #[test]
    fn seq_arguments() {
        assert_eq!(seq_range(&v(&["5"]), 10_000), Ok((1, 5)));
        assert_eq!(seq_range(&v(&["2", "5"]), 10_000), Ok((2, 5)));
        assert_eq!(seq_range(&v(&["-3", "3"]), 10_000), Ok((-3, 3)));
        assert_eq!(seq_range(&v(&["5", "2"]), 10_000), Ok((5, 2)), "empty range is fine, prints nothing");
        assert!(seq_range(&v(&[]), 10_000).is_err());
        assert!(seq_range(&v(&["1", "2", "3"]), 10_000).is_err());
        assert!(seq_range(&v(&["x"]), 10_000).is_err());
        assert!(seq_range(&v(&["1", "10001"]), 10_000).is_err());
        assert!(seq_range(&v(&["1", "10000"]), 10_000).is_ok());
        assert!(seq_range(&v(&["-9223372036854775808", "9223372036854775807"]), 10_000).is_err(), "no overflow");
    }

    #[test]
    fn count_option_parses_dash_n() {
        let a = v(&["-n", "3", "file"]);
        assert_eq!(count_option(&a), Ok((3, &a[2..])));
        let b = v(&["file"]);
        assert_eq!(count_option(&b), Ok((10, &b[..])));
        assert!(count_option(&v(&["-n"])).is_err());
        assert!(count_option(&v(&["-n", "x", "f"])).is_err());
        assert!(count_option(&v(&["-n", "-5", "f"])).is_err());
    }

    #[test]
    fn random_bytes_never_panic() {
        let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for _ in 0..3000 {
            let n = (next() % 200) as usize;
            let data: Vec<u8> = (0..n).map(|_| [b'a', b'\n', b' ', b'z', 0, 0xff][(next() % 6) as usize]).collect();
            let k = (next() % 6) as usize;
            let h = head(&data, k);
            let t = tail(&data, k);
            assert!(data.starts_with(h) && data.ends_with(t));
            assert!(lines(&h).count() <= k);
            let c = wc(&data);
            assert_eq!(c.bytes, data.len());
            let _ = hexdump_line(0, &data[..data.len().min(16)]);
            let _ = contains(&data, &data[..data.len().min(3)], true);
        }
    }
}
