//! RAM filesystem with folders: a tree of named byte files with a node-count and byte budget.
//! Pure and host-testable. Paths are absolute and clean (`/`, `/docs`, `/docs/a.txt`); the shell turns
//! what you type into such a path with [`resolve`] (it knows the current folder, `.` and `..`).

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// Longest single name (one file or folder).
pub const MAX_NAME: usize = 32;
/// Longest whole path.
pub const MAX_PATH: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsError {
    /// The name or path is empty, too long, or has a character that is not allowed.
    BadName,
    /// Too many files and folders.
    Full,
    /// Out of bytes.
    TooBig,
    /// The folder that should hold it does not exist.
    NoParent,
    /// The path is a folder, but a file was needed.
    IsDir,
    /// The path is a file, but a folder was needed.
    NotDir,
    /// Something with that name already exists.
    Exists,
    /// The folder still has things in it.
    NotEmpty,
    /// Nothing with that name exists.
    NotFound,
}

struct Node {
    path: String,
    /// `None` for a folder.
    data: Option<Vec<u8>>,
}

pub struct RamFs {
    /// Sorted by `path`. The root `/` is implicit and not stored.
    nodes: Vec<Node>,
    max_files: usize,
    max_bytes: usize,
    used: usize,
}

/// One file or folder name: 1 to `MAX_NAME` bytes, no `/` or `\`, no control characters, not `.` or `..`.
pub fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && name != "."
        && name != ".."
        && !name.bytes().any(|b| b == b'/' || b == b'\\' || b < 0x20 || b == 0x7F)
}

/// Turns what the user typed into a clean absolute path. `cwd` is the current folder (absolute).
/// Handles `.`, `..` (stops at the root), repeated slashes and a trailing slash. `None` when a name in
/// it is not allowed or the path is longer than `MAX_PATH`. An empty `input` means `cwd` itself.
pub fn resolve(cwd: &str, input: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    if !input.starts_with('/') {
        parts.extend(cwd.split('/').filter(|c| !c.is_empty()));
    }
    for c in input.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            c if name_ok(c) => parts.push(c),
            _ => return None,
        }
    }
    let mut out = String::new();
    for p in &parts {
        out.push('/');
        out.push_str(p);
    }
    if out.is_empty() {
        out.push('/');
    }
    (out.len() <= MAX_PATH).then_some(out)
}

/// True for a clean absolute path that is not the root.
fn path_ok(p: &str) -> bool {
    p.len() > 1 && p.len() <= MAX_PATH && p.starts_with('/') && p[1..].split('/').all(name_ok)
}

/// The folder that holds `p` (`/` for a top-level name).
fn parent_of(p: &str) -> &str {
    match p.rfind('/') {
        Some(0) | None => "/",
        Some(i) => &p[..i],
    }
}

/// Turns the typed escapes `\n`, `\t` and `\\` into real bytes; any other backslash stays as typed.
/// The shell line is one row long, so this is how a one-line command writes a many-line file.
pub fn unescape(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            let r = match b[i + 1] {
                b'n' => Some(b'\n'),
                b't' => Some(b'\t'),
                b'\\' => Some(b'\\'),
                _ => None,
            };
            if let Some(r) = r {
                out.push(r);
                i += 2;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

impl RamFs {
    pub const fn new(max_files: usize, max_bytes: usize) -> RamFs {
        RamFs { nodes: Vec::new(), max_files, max_bytes, used: 0 }
    }

    fn find(&self, path: &str) -> Result<usize, usize> {
        self.nodes.binary_search_by(|n| n.path.as_str().cmp(path))
    }

    /// True for the root and for any existing folder.
    pub fn is_dir(&self, path: &str) -> bool {
        path == "/" || self.find(path).is_ok_and(|i| self.nodes[i].data.is_none())
    }

    /// Checks that the folder holding `path` exists.
    fn check_parent(&self, path: &str) -> Result<(), FsError> {
        let parent = parent_of(path);
        if self.is_dir(parent) {
            Ok(())
        } else if self.find(parent).is_ok() {
            Err(FsError::NotDir)
        } else {
            Err(FsError::NoParent)
        }
    }

    /// Says whether a file could be written at `path` (ignoring size): a good name, not a folder, and
    /// its folder exists. For `edit`, so a typo is caught before the text is typed.
    pub fn check_writable(&self, path: &str) -> Result<(), FsError> {
        if !path_ok(path) {
            return Err(FsError::BadName);
        }
        match self.find(path) {
            Ok(i) if self.nodes[i].data.is_none() => Err(FsError::IsDir),
            Ok(_) => Ok(()),
            Err(_) => self.check_parent(path),
        }
    }

    /// Creates or replaces a file. On error nothing changes, an existing file keeps its old content.
    pub fn write(&mut self, path: &str, data: &[u8]) -> Result<(), FsError> {
        if !path_ok(path) {
            return Err(FsError::BadName);
        }
        match self.find(path) {
            Ok(i) => {
                let Some(old) = self.nodes[i].data.as_ref().map(Vec::len) else {
                    return Err(FsError::IsDir);
                };
                if self.used - old + data.len() > self.max_bytes {
                    return Err(FsError::TooBig);
                }
                self.used = self.used - old + data.len();
                self.nodes[i].data = Some(data.to_vec());
            }
            Err(at) => {
                self.check_parent(path)?;
                if self.nodes.len() >= self.max_files {
                    return Err(FsError::Full);
                }
                if self.used + data.len() > self.max_bytes {
                    return Err(FsError::TooBig);
                }
                self.used += data.len();
                self.nodes.insert(at, Node { path: String::from(path), data: Some(data.to_vec()) });
            }
        }
        Ok(())
    }

    /// Adds `data` to the end of a file, creating it when missing. On error nothing changes.
    pub fn append(&mut self, path: &str, data: &[u8]) -> Result<(), FsError> {
        if !path_ok(path) {
            return Err(FsError::BadName);
        }
        match self.find(path) {
            Ok(i) => {
                if self.nodes[i].data.is_none() {
                    return Err(FsError::IsDir);
                }
                if self.used + data.len() > self.max_bytes {
                    return Err(FsError::TooBig);
                }
                self.used += data.len();
                if let Some(d) = self.nodes[i].data.as_mut() {
                    d.extend_from_slice(data);
                }
                Ok(())
            }
            Err(_) => self.write(path, data),
        }
    }

    /// The content of a file (`None` for a folder or a missing path).
    pub fn read(&self, path: &str) -> Option<&[u8]> {
        self.find(path).ok().and_then(|i| self.nodes[i].data.as_deref())
    }

    /// Deletes a file. Returns false when there is no such file (a folder needs `rmdir`).
    pub fn remove(&mut self, path: &str) -> bool {
        match self.find(path) {
            Ok(i) if self.nodes[i].data.is_some() => {
                self.used -= self.nodes.remove(i).data.map_or(0, |d| d.len());
                true
            }
            _ => false,
        }
    }

    /// Creates an empty folder. The folder that holds it must exist.
    pub fn mkdir(&mut self, path: &str) -> Result<(), FsError> {
        if !path_ok(path) {
            return Err(FsError::BadName);
        }
        match self.find(path) {
            Ok(_) => Err(FsError::Exists),
            Err(at) => {
                self.check_parent(path)?;
                if self.nodes.len() >= self.max_files {
                    return Err(FsError::Full);
                }
                self.nodes.insert(at, Node { path: String::from(path), data: None });
                Ok(())
            }
        }
    }

    /// Deletes an empty folder.
    pub fn rmdir(&mut self, path: &str) -> Result<(), FsError> {
        if !path_ok(path) {
            return Err(FsError::BadName);
        }
        let Ok(i) = self.find(path) else { return Err(FsError::NotFound) };
        if self.nodes[i].data.is_some() {
            return Err(FsError::NotDir);
        }
        let prefix = format!("{path}/");
        if self.nodes.iter().any(|n| n.path.starts_with(prefix.as_str())) {
            return Err(FsError::NotEmpty);
        }
        self.nodes.remove(i);
        Ok(())
    }

    /// What is directly inside folder `dir`: name and size (`None` for a folder), sorted by name.
    pub fn list<'a>(&'a self, dir: &str) -> impl Iterator<Item = (&'a str, Option<usize>)> + 'a + use<'a> {
        let prefix = if dir == "/" { String::from("/") } else { format!("{dir}/") };
        self.nodes.iter().filter_map(move |n| {
            let rest = n.path.strip_prefix(prefix.as_str())?;
            (!rest.is_empty() && !rest.contains('/')).then(|| (rest, n.data.as_ref().map(Vec::len)))
        })
    }

    pub fn used_bytes(&self) -> usize {
        self.used
    }

    /// Number of files (folders not counted).
    pub fn count(&self) -> usize {
        self.nodes.iter().filter(|n| n.data.is_some()).count()
    }

    /// Number of folders (the root not counted).
    pub fn dir_count(&self) -> usize {
        self.nodes.iter().filter(|n| n.data.is_none()).count()
    }

    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bad_names() {
        let mut fs = RamFs::new(4, 100);
        let long = format!("/{}", "x".repeat(MAX_NAME + 1));
        for n in ["", "/", "a", long.as_str(), "/a\\b", "/a\0b", "/.", "/..", "/a\nb", "/a//b", "/a/"] {
            assert_eq!(fs.write(n, b"x"), Err(FsError::BadName), "{n:?}");
        }
        assert_eq!(fs.count(), 0);
        assert!(fs.write(&format!("/{}", "y".repeat(MAX_NAME)), b"x").is_ok());
    }

    #[test]
    fn write_read_roundtrip() {
        let mut fs = RamFs::new(4, 100);
        fs.write("/a", b"hello").unwrap();
        assert_eq!(fs.read("/a"), Some(&b"hello"[..]));
        assert_eq!(fs.read("/b"), None);
        assert_eq!(fs.used_bytes(), 5);
    }

    #[test]
    fn overwrite_replaces_and_frees_old_bytes() {
        let mut fs = RamFs::new(4, 10);
        fs.write("/a", b"0123456789").unwrap();
        fs.write("/a", b"xy").unwrap();
        assert_eq!(fs.used_bytes(), 2);
        assert_eq!(fs.read("/a"), Some(&b"xy"[..]));
    }

    #[test]
    fn full_by_count_keeps_existing() {
        let mut fs = RamFs::new(2, 100);
        fs.write("/a", b"1").unwrap();
        fs.write("/b", b"2").unwrap();
        assert_eq!(fs.write("/c", b"3"), Err(FsError::Full));
        assert_eq!(fs.mkdir("/d"), Err(FsError::Full));
        assert_eq!(fs.count(), 2);
        assert!(fs.write("/a", b"9").is_ok(), "overwrite still allowed when the count is full");
    }

    #[test]
    fn full_by_bytes_keeps_existing() {
        let mut fs = RamFs::new(4, 10);
        fs.write("/a", b"12345").unwrap();
        assert_eq!(fs.write("/a", &[0u8; 11]), Err(FsError::TooBig));
        assert_eq!(fs.read("/a"), Some(&b"12345"[..]));
        assert_eq!(fs.write("/b", &[0u8; 6]), Err(FsError::TooBig));
        assert_eq!(fs.used_bytes(), 5);
    }

    #[test]
    fn remove_frees_space() {
        let mut fs = RamFs::new(4, 10);
        fs.write("/a", b"12345").unwrap();
        assert!(fs.remove("/a"));
        assert!(!fs.remove("/a"));
        assert_eq!(fs.used_bytes(), 0);
        assert_eq!(fs.count(), 0);
    }

    #[test]
    fn list_is_sorted_and_only_direct_children() {
        let mut fs = RamFs::new(16, 100);
        fs.mkdir("/docs").unwrap();
        fs.mkdir("/docs/old").unwrap();
        for p in ["/m", "/b", "/z", "/a", "/docs/x", "/docs/old/deep"] {
            fs.write(p, b"-").unwrap();
        }
        let top: Vec<_> = fs.list("/").collect();
        assert_eq!(top, [("a", Some(1)), ("b", Some(1)), ("docs", None), ("m", Some(1)), ("z", Some(1))]);
        let d: Vec<_> = fs.list("/docs").collect();
        assert_eq!(d, [("old", None), ("x", Some(1))]);
        assert_eq!(fs.list("/docs/old").count(), 1);
        assert_eq!(fs.list("/nothing").count(), 0);
    }

    #[test]
    fn sibling_with_a_dash_does_not_leak_into_a_folder() {
        // "/a-b" sorts between "/a" and "/a/x": list must still return the right children.
        let mut fs = RamFs::new(8, 100);
        fs.mkdir("/a").unwrap();
        fs.write("/a-b", b"1").unwrap();
        fs.write("/a/x", b"2").unwrap();
        assert_eq!(fs.list("/a").collect::<Vec<_>>(), [("x", Some(1))]);
        assert_eq!(fs.list("/").collect::<Vec<_>>(), [("a", None), ("a-b", Some(1))]);
    }

    #[test]
    fn files_need_an_existing_folder() {
        let mut fs = RamFs::new(8, 100);
        assert_eq!(fs.write("/nope/a", b"x"), Err(FsError::NoParent));
        assert_eq!(fs.mkdir("/nope/sub"), Err(FsError::NoParent));
        fs.write("/f", b"x").unwrap();
        assert_eq!(fs.write("/f/a", b"x"), Err(FsError::NotDir), "a file is not a folder");
        assert_eq!(fs.mkdir("/f/sub"), Err(FsError::NotDir));
        fs.mkdir("/d").unwrap();
        assert!(fs.write("/d/a", b"x").is_ok());
        assert_eq!(fs.count(), 2);
        assert_eq!(fs.dir_count(), 1);
    }

    #[test]
    fn mkdir_rmdir_rules() {
        let mut fs = RamFs::new(8, 100);
        fs.mkdir("/d").unwrap();
        assert_eq!(fs.mkdir("/d"), Err(FsError::Exists));
        fs.write("/f", b"x").unwrap();
        assert_eq!(fs.mkdir("/f"), Err(FsError::Exists));
        assert_eq!(fs.mkdir("/"), Err(FsError::BadName));
        fs.write("/d/a", b"x").unwrap();
        assert_eq!(fs.rmdir("/d"), Err(FsError::NotEmpty));
        assert!(fs.remove("/d/a"));
        assert_eq!(fs.rmdir("/f"), Err(FsError::NotDir));
        assert_eq!(fs.rmdir("/none"), Err(FsError::NotFound));
        assert_eq!(fs.rmdir("/"), Err(FsError::BadName));
        assert_eq!(fs.rmdir("/d"), Ok(()));
        assert!(!fs.is_dir("/d"));
        assert!(fs.is_dir("/"));
    }

    #[test]
    fn a_folder_is_not_a_file() {
        let mut fs = RamFs::new(8, 100);
        fs.mkdir("/d").unwrap();
        assert_eq!(fs.write("/d", b"x"), Err(FsError::IsDir));
        assert_eq!(fs.append("/d", b"x"), Err(FsError::IsDir));
        assert_eq!(fs.read("/d"), None);
        assert!(!fs.remove("/d"), "rm does not delete folders");
        assert!(fs.is_dir("/d"));
    }

    #[test]
    fn resolve_handles_relative_dots_and_slashes() {
        assert_eq!(resolve("/", "a").as_deref(), Some("/a"));
        assert_eq!(resolve("/docs", "a/b").as_deref(), Some("/docs/a/b"));
        assert_eq!(resolve("/docs", "/x").as_deref(), Some("/x"));
        assert_eq!(resolve("/docs/old", "..").as_deref(), Some("/docs"));
        assert_eq!(resolve("/docs/old", "../../x").as_deref(), Some("/x"));
        assert_eq!(resolve("/docs", "../../../x").as_deref(), Some("/x"), "stops at the root");
        assert_eq!(resolve("/docs", "").as_deref(), Some("/docs"));
        assert_eq!(resolve("/docs", ".").as_deref(), Some("/docs"));
        assert_eq!(resolve("/", "a//b/./c/").as_deref(), Some("/a/b/c"));
        assert_eq!(resolve("/a", "/").as_deref(), Some("/"));
        assert_eq!(resolve("/", "a\\b"), None);
        assert_eq!(resolve("/", "a\tb"), None);
        assert_eq!(resolve("/", &"x".repeat(MAX_NAME + 1)), None);
        let deep = "abcdefghij/".repeat(13);
        assert_eq!(resolve("/", &deep), None, "path longer than MAX_PATH");
    }

    #[test]
    fn unescape_makes_newlines_and_tabs() {
        assert_eq!(unescape("one\\ntwo"), b"one\ntwo");
        assert_eq!(unescape("a\\tb"), b"a\tb");
        assert_eq!(unescape("back\\\\slash"), b"back\\slash");
        assert_eq!(unescape("keep \\q as is"), b"keep \\q as is");
        assert_eq!(unescape("trailing\\"), b"trailing\\");
        assert_eq!(unescape(""), b"");
    }

    #[test]
    fn append_adds_to_the_end_and_creates() {
        let mut fs = RamFs::new(4, 20);
        fs.append("/a", b"one\n").unwrap();
        fs.append("/a", b"two\n").unwrap();
        assert_eq!(fs.read("/a"), Some(&b"one\ntwo\n"[..]));
        assert_eq!(fs.used_bytes(), 8);
        assert_eq!(fs.append("/a", &[0u8; 13]), Err(FsError::TooBig));
        assert_eq!(fs.read("/a"), Some(&b"one\ntwo\n"[..]), "a failed append changes nothing");
        assert_eq!(fs.append("/bad\\name", b"x"), Err(FsError::BadName));
        assert_eq!(fs.append("/nofolder/x", b"x"), Err(FsError::NoParent));
    }

    /// Random operations never panic and never break the byte and count books.
    #[test]
    fn random_operations_keep_the_books_straight() {
        let mut fs = RamFs::new(12, 64);
        let names = ["/a", "/b", "/a/x", "/a/y", "/b/x", "/a/x/z", "/c", "/a-b", "/", "/bad\\"];
        let mut seed = 0x1234_5678u32;
        for _ in 0..5000 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let p = names[(seed >> 8) as usize % names.len()];
            let n = (seed >> 16) as usize % 20;
            let _ = match (seed >> 4) % 6 {
                0 => fs.write(p, &vec![1u8; n]),
                1 => fs.append(p, &vec![2u8; n]),
                2 => fs.mkdir(p),
                3 => fs.rmdir(p),
                4 => {
                    fs.remove(p);
                    Ok(())
                }
                _ => Ok(()),
            };
            let total: usize = fs.nodes.iter().filter_map(|n| n.data.as_ref().map(Vec::len)).sum();
            assert_eq!(total, fs.used_bytes());
            assert!(fs.used_bytes() <= 64 && fs.nodes.len() <= 12);
            assert!(fs.nodes.windows(2).all(|w| w[0].path < w[1].path), "sorted and unique");
            for n in &fs.nodes {
                assert!(path_ok(&n.path) && fs.is_dir(parent_of(&n.path)), "{} has no parent folder", n.path);
            }
        }
    }
}
