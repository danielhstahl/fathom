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
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RepoPath {
    /// The raw bytes, `/`-separated, never absolute.
    bytes: Vec<u8>,
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
    fn containment_is_on_component_boundaries() {
        let path = RepoPath::new("src/lib.rs");
        assert!(path.starts_with(b"src"));
        assert!(path.starts_with(b"src/lib.rs"));
        assert!(!path.starts_with(b"sr"));
        assert!(path.starts_with(b""));
    }
}
