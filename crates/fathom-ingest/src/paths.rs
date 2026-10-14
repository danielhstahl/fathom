//! `paths.rs` — repository-relative paths as the bytes git stores (ADR-002 §4.3).
//!
//! Git stores a path as raw bytes joined by `/`, with no promise that the bytes are valid UTF-8 (they
//! usually are; the ones that are not are exactly the ones that break a tool). Rust's `Path` is the wrong
//! shape here: it is platform-native, so the same commit would carry different bytes on Windows and on
//! Linux, and ADR-001 §7.4 promises the same bytes everywhere. So paths leave the door as [`RepoPath`]:
//! the bytes, plus the honest conversions.
//!
//! Anything that touches the filesystem converts with [`RepoPath::to_native`]; anything that prints
//! converts with `Display`, which is lossy and says so.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A path inside a repository, in git's own representation: `/`-separated bytes, relative to the root.
///
/// # JSON
///
/// Serialized as a **string**, and quoted the way git quotes paths (`core.quotePath`): printable UTF-8
/// passes through, and anything that cannot be a JSON string byte — a control character, a byte that is
/// not valid UTF-8 — becomes a three-digit octal escape (`doc/\303\251.md`). The alternative, a JSON array
/// of numbers, is lossless and unreadable, and paths are the single most-read field in anything this crate
/// emits. It is lossless in the same way: [`unquote`] is the exact inverse of [`quote`], including for a
/// path whose bytes *are* a backslash followed by three octal digits.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RepoPath {
    /// The raw bytes, `/`-separated, never absolute.
    bytes: Vec<u8>,
}

impl Serialize for RepoPath {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&quote(&self.bytes))
    }
}

impl<'de> Deserialize<'de> for RepoPath {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl serde::de::Visitor<'_> for Visitor {
            type Value = RepoPath;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a repository path, git-quoted")
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(RepoPath { bytes: unquote(v) })
            }
        }
        deserializer.deserialize_str(Visitor)
    }
}

/// The JSON spelling of one path's bytes. See [`RepoPath`]'s notes.
#[must_use]
fn quote(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut rest = bytes;
    while !rest.is_empty() {
        match std::str::from_utf8(rest) {
            Ok(text) => {
                escape(text, &mut out);
                break;
            }
            Err(err) => {
                let valid = err.valid_up_to();
                // `valid_up_to` only ever names a valid prefix, so this cannot fail.
                escape(
                    std::str::from_utf8(&rest[..valid]).unwrap_or_default(),
                    &mut out,
                );
                let bad = err.error_len().unwrap_or(1).min(rest.len() - valid);
                for byte in &rest[valid..valid + bad] {
                    out.push_str(&format!("\\{byte:03o}"));
                }
                rest = &rest[valid + bad..];
            }
        }
    }
    out
}

/// Escape the characters that would otherwise make the quoting ambiguous.
fn escape(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            // Doubled, so `\\` in the quoted form is one backslash in the bytes — and so an octal escape
            // can never be invented by a path that happened to contain a backslash.
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                let code = c as u32;
                out.push_str(&format!("\\{code:03o}"));
            }
            c => out.push(c),
        }
    }
}

/// The exact inverse of [`quote`].
#[must_use]
fn unquote(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'\\' {
            out.push(bytes[index]);
            index += 1;
            continue;
        }
        match bytes.get(index + 1) {
            Some(b'\\') => {
                out.push(b'\\');
                index += 2;
            }
            Some(_digit @ b'0'..=b'7') if octal_at(bytes, index + 1).is_some() => {
                out.push(octal_at(bytes, index + 1).unwrap_or(0));
                index += 4;
            }
            // Not one of our escapes. Kept literally rather than dropped: a reader that meets a stray
            // backslash should report the path, not lose a byte of it.
            _ => {
                out.push(b'\\');
                index += 1;
            }
        }
    }
    out
}

/// Decode three octal digits starting at `at`, if there are three and they are octal.
fn octal_at(bytes: &[u8], at: usize) -> Option<u8> {
    let digits = bytes.get(at..at + 3)?;
    if !digits.iter().all(|byte| (b'0'..=b'7').contains(byte)) {
        return None;
    }
    let text = std::str::from_utf8(digits).ok()?;
    u8::from_str_radix(text, 8).ok()
}

impl RepoPath {
    /// Take the bytes as given.
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        RepoPath {
            bytes: bytes.into(),
        }
    }

    /// The raw bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The path in the form this type serializes to: valid UTF-8 as itself, anything else with git's octal
    /// escapes. The store's `new_path` column holds exactly this, so a row and an export line cannot
    /// disagree about what a path is, and neither one has to be lossy.
    #[must_use]
    pub fn quoted(&self) -> String {
        quote(&self.bytes)
    }

    /// Lossy UTF-8, for display and for the lossy analyzers.
    #[must_use]
    pub fn to_string_lossy(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.bytes)
    }

    /// The path as it can be opened on this platform, joined onto `root`.
    ///
    /// On Unix this is byte-exact. On Windows, bytes that are not valid UTF-8 cannot be expressed and
    /// become the replacement character — the file then does not exist, which is reported as a missing
    /// file rather than as a silent mismatch, because a tool that invents a path is worse than one that
    /// admits it cannot name the file.
    #[must_use]
    pub fn to_native(&self, root: &Path) -> PathBuf {
        root.join(native_component(&self.bytes))
    }

    /// The final component, `/`-separated (what a file manager calls the name).
    #[must_use]
    pub fn file_name(&self) -> &[u8] {
        match self.bytes.iter().rposition(|byte| *byte == b'/') {
            Some(index) => &self.bytes[index + 1..],
            None => &self.bytes,
        }
    }

    /// The directory part, or the repository root (empty bytes) when the path is top-level.
    #[must_use]
    pub fn parent(&self) -> &[u8] {
        match self.bytes.iter().rposition(|byte| *byte == b'/') {
            Some(index) => &self.bytes[..index],
            None => b"",
        }
    }

    /// The extension without the dot, if there is one.
    ///
    /// `Makefile` and `.gitignore` have none: a leading dot is a name, not an extension, and the path
    /// taxonomy in `src/recommender/taxonomy/` is written with that rule.
    #[must_use]
    pub fn extension(&self) -> Option<&[u8]> {
        let name = self.file_name();
        let dot = name.iter().rposition(|byte| *byte == b'.')?;
        if dot == 0 || dot + 1 >= name.len() {
            return None;
        }
        Some(&name[dot + 1..])
    }

    /// Is this path inside `ancestor` (bytewise, on a component boundary)?
    #[must_use]
    pub fn starts_with(&self, ancestor: &[u8]) -> bool {
        if ancestor.is_empty() {
            return true;
        }
        self.bytes.starts_with(ancestor)
            && matches!(self.bytes.get(ancestor.len()), None | Some(b'/'))
    }
}

impl std::fmt::Display for RepoPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_string_lossy())
    }
}

impl std::fmt::Debug for RepoPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RepoPath({})", self.to_string_lossy())
    }
}

/// Turn git's `/`-separated bytes into a platform-native path fragment.
#[cfg(unix)]
pub(crate) fn native_component(bytes: &[u8]) -> Cow<'_, Path> {
    use std::os::unix::ffi::OsStrExt as _;
    Cow::Borrowed(Path::new(std::ffi::OsStr::from_bytes(bytes)))
}

/// The non-Unix form: UTF-8 is required, and `/` has to become `\` for the OS's own display; both are
/// accepted by the Win32 file APIs, so only the lossy conversion is worth doing here.
#[cfg(not(unix))]
pub(crate) fn native_component(bytes: &[u8]) -> Cow<'_, Path> {
    Cow::Owned(Path::new(&String::from_utf8_lossy(bytes).replace('/', "\\")).to_path_buf())
}

/// Convert git path bytes to a lossy string; the free function a caller reaches for when it has bytes
/// rather than a [`RepoPath`] (a tree entry, an index entry).
#[must_use]
pub fn to_str_lossy(bytes: &[u8]) -> Cow<'_, str> {
    String::from_utf8_lossy(bytes)
}

/// Join two `/`-separated byte paths.
#[must_use]
pub fn join(parent: &[u8], child: &[u8]) -> Vec<u8> {
    if parent.is_empty() {
        return child.to_vec();
    }
    if child.is_empty() {
        return parent.to_vec();
    }
    let mut out = Vec::with_capacity(parent.len() + 1 + child.len());
    out.extend_from_slice(parent);
    out.push(b'/');
    out.extend_from_slice(child);
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::RepoPath;

    #[test]
    fn splits_a_path_the_way_a_reviewer_reads_it() {
        let path = RepoPath::new("src/recommender/taxonomy/__init__.py");
        assert_eq!(path.file_name(), b"__init__.py");
        assert_eq!(path.extension(), Some(b"py".as_slice()));
        assert_eq!(path.parent(), b"src/recommender/taxonomy");

        let dotfile = RepoPath::new(".github/workflows/ci.yml");
        assert_eq!(dotfile.file_name(), b"ci.yml");
        assert_eq!(dotfile.parent(), b".github/workflows");

        let bare = RepoPath::new("Makefile");
        assert_eq!(bare.extension(), None);
        assert_eq!(bare.parent(), b"");
        assert_eq!(RepoPath::new(".gitignore").extension(), None);
    }

    #[test]
    fn keeps_non_utf8_bytes_intact_and_prints_them_lossily() {
        let path = RepoPath::new(vec![b'd', b'o', b'c', b'/', 0xff, b'.', b'm', b'd']);
        assert_eq!(path.as_bytes()[4], 0xff);
        assert!(path.to_string_lossy().contains('\u{fffd}'));
        // The JSON form is the file's bytes, not the lossy rendering: a fixture may round-trip them.
        let json = serde_json::to_string(&path).expect("serialisable");
        let back: RepoPath = serde_json::from_str(&json).expect("deserialisable");
        assert_eq!(back.as_bytes(), path.as_bytes());
    }

    #[test]
    fn json_is_a_string_with_git_style_octal_escapes() {
        let plain = RepoPath::new("src/lib.rs");
        assert_eq!(serde_json::to_string(&plain).unwrap(), "\"src/lib.rs\"");

        // "café.md" where the é really is those two bytes: printable text passes through as
        // itself, because a golden file a human cannot read is a worse contract, not a better one.
        let accented = RepoPath::new(vec![b'c', b'a', b'f', 0xc3, 0xa9, b'.', b'm', b'd']);
        assert_eq!(
            serde_json::to_string(&accented).unwrap(),
            "\"café.md\"",
            "valid UTF-8 passes through untouched"
        );

        let broken = RepoPath::new(vec![b'd', b'o', b'c', b'/', 0xff, b'.', b'm', b'd']);
        let text = serde_json::to_string(&broken).unwrap();
        assert_eq!(text, "\"doc/\\\\377.md\"");
        assert_eq!(serde_json::from_str::<RepoPath>(&text).unwrap(), broken);

        // A backslash in the bytes must not be confusable with an escape: it is doubled on the way out and
        // single on the way back, and the round trip is the proof. Four backslashes in the file is not a
        // mistake: the quoting doubles the one real backslash, and JSON then doubles each of those two.
        let backslash = RepoPath::new("a\\b".to_owned());
        let text = serde_json::to_string(&backslash).unwrap();
        assert_eq!(text, r#""a\\\\b""#);
        assert_eq!(
            serde_json::from_str::<RepoPath>(&serde_json::to_string(&backslash).unwrap()).unwrap(),
            backslash
        );

        // A path whose bytes *look* like an escape survives too, because a real backslash is doubled.
        let tricky = RepoPath::new("a\\377b".to_owned());
        assert_eq!(
            serde_json::from_str::<RepoPath>(&serde_json::to_string(&tricky).unwrap()).unwrap(),
            tricky
        );
    }

    #[test]
    fn containment_is_on_component_boundaries() {
        let path = RepoPath::new("src/lib.rs");
        assert!(path.starts_with(b"src"));
        assert!(path.starts_with(b"src/lib.rs"));
        assert!(!path.starts_with(b"sr"));
        assert!(path.starts_with(b""));
    }
}
