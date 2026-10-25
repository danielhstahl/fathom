//! The working tree as files: which ones are there, and what is in them (`reco-p41`).
//!
//! # Why the outside door has a directory walk
//!
//! ADR-002 §3 makes `fathom-analyze` IO-free on purpose. An analysis that reaches for `std::fs` is an
//! analysis whose golden test depends on the machine it ran on and whose ADR-004 §5 cache key is a
//! checksum of somebody's filesystem, and `lint W1` refuses it on every commit. The rule does not delete
//! the read, it moves it — here. [`crate::deps::scan`] already reads dependency files out of *git* through
//! [`crate::repo`]; the read that has no git object behind it is the working tree as it sits on disk right
//! now, including the edits nobody has staged or committed, and the only thing that wants it is
//! `fathom_analyze::search::symbols`, which fills the one search box's symbol section (`reco-6sw`) and
//! had been doing the walking itself until this module existed.
//!
//! # The line this module holds
//!
//! What lives here is the mechanical half of looking at a directory: how deep to go, which directories a
//! build or a vendor drop left behind, which order to hand files over in, what one file's `stat` said, and
//! what its bytes are. What does **not** live here is any opinion about why a file is interesting. Which
//! extensions are worth opening, which caps the byte budget should be spent in, and what a declaration is,
//! all belong to the caller — the analysis, not the door. So [`find`] takes a `wants` predicate rather than
//! a language table, and hands back [`Entry`] rows: a path, a size, a last touch, and nothing derived.
//!
//! Two properties hold across the whole module, because a caller that is trying to be a pure function of
//! (input, config) has to be able to rely on them:
//!
//! * **A walk never fails.** An unreadable directory is a directory with nothing in it, a file that
//!   disappears between the walk and the read is a file the caller counts and mentions, and the only
//!   failure this module reports is [`read`]'s own I/O error, which is the caller's fact about one file
//!   rather than this module's verdict about the tree.
//! * **A walk is repeatable.** Children are sorted at every level and the result is sorted by
//!   repository-relative path, so two walks of one tree hand the caller one order — which is what lets a
//!   caller spend a bounded budget in that order twice and get the same files both times.
//!
//! Paths come back repository-relative with `/` separators, the spelling every other row in this
//! workspace uses (ADR-002 §3's path taxonomy), and absolute in [`Entry::absolute`] for the read. The
//! relative form is what a caller compares against an ignore list; the absolute form is what it opens.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The directories a working-tree walk does not enter, whatever the caller adds.
///
/// These are the names a build or a vendor drop leaves behind — compiled output, an unpacked dependency
/// tree, a lockfile's expanded cache — where a declaration is somebody else's and a byte is not this
/// repository's. A caller adds its own on top through [`WalkPolicy::skip_dirs`]; both lists are matched
/// one path segment at a time, case-insensitively.
///
/// Hidden directories are skipped unconditionally rather than through this list: `.git` is git's and
/// `.venv` is a machine's, and a caller that wanted either of them should ask git rather than walk it.
pub const DEFAULT_SKIPPED_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    "vendor",
    "third_party",
    "bower_components",
    "__pycache__",
    "venv",
    "htmlcov",
    "coverage",
    "Pods",
];

/// How deep [`find`] goes when the caller has no opinion.
///
/// Six is deep enough for a monorepo's `packages/x/src/lib/y.rs` and shallow enough that a tree cannot
/// turn a walk into a crawl. Deeper trees lose their leaves, and the caller — which knows what it was
/// looking for — is the one that has to say so.
pub const DEFAULT_MAX_DEPTH: usize = 6;

/// What one [`find`] may touch.
#[derive(Debug, Clone)]
pub struct WalkPolicy {
    /// How many directories below `root` the walk descends. [`DEFAULT_MAX_DEPTH`] when built by
    /// [`Default`].
    pub max_depth: usize,
    /// Directory names to skip in addition to [`DEFAULT_SKIPPED_DIRS`], matched case-insensitively
    /// against one path segment at a time.
    ///
    /// This is where a caller puts the ignore list it already has rather than inventing a second one:
    /// `fathom_analyze::search::symbols` feeds it `graph.ignored_dirs`, so the architecture view's opinion
    /// of which directories are not this repository is the search box's opinion too.
    pub skip_dirs: Vec<String>,
}

impl Default for WalkPolicy {
    fn default() -> Self {
        WalkPolicy {
            max_depth: DEFAULT_MAX_DEPTH,
            skip_dirs: Vec::new(),
        }
    }
}

impl WalkPolicy {
    /// Whether a directory name is one this walk will not enter.
    #[must_use]
    pub fn skips(&self, name: &str) -> bool {
        name.starts_with('.')
            || DEFAULT_SKIPPED_DIRS.contains(&name)
            || self
                .skip_dirs
                .iter()
                .any(|skipped| skipped.eq_ignore_ascii_case(name))
    }
}

/// One file [`find`] found, as the tree reported it — a fact about a file and nothing else.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Repository-relative and `/`-separated, the spelling every other row in this workspace uses.
    pub path: String,
    /// The same file as opened; [`read`] takes this.
    pub absolute: PathBuf,
    /// Bytes, as the `stat` said. A file edited between the walk and the read has already been paid for by
    /// whoever decided the caps from this number, which is why [`read`] returns what it actually got.
    pub size: u64,
    /// Whole seconds since the epoch, or `0` when the filesystem would not say.
    ///
    /// Whole seconds on purpose: this is a staleness signal, not a timestamp, and a nanosecond field would
    /// be read as one. A caller that folds it into a digest wants "did the tree move", and sub-second
    /// resolution only makes the same tree stamp differently twice.
    pub modified: i64,
}

impl Entry {
    /// The file's bytes.
    ///
    /// # Errors
    ///
    /// Whatever `std::io::Error` the read gave: a file deleted since the walk, a permission the process
    /// does not have, a dangling symlink that passed the `stat`. The caller counts these and says how many
    /// it saw; this module does not decide that a missing file is uninteresting.
    pub fn read(&self) -> std::io::Result<Vec<u8>> {
        read(&self.absolute)
    }
}

/// The files under `root` that `wants` asked for, in path order, without reading a single body.
///
/// `wants` is called with each candidate's repository-relative path. A path has to be `stat`ed before the
/// door can say it is a file at all, so the predicate runs after that and before the row is built: a cheap
/// one — an extension test, an ignore list — is what keeps the returned list, and the caller's budget for
/// reading it, small. The walk is breadth-first up to [`WalkPolicy::max_depth`], never enters a directory
/// [`WalkPolicy::skips`], never follows a symlink (a symlink is neither the file nor the directory it
/// points at, and following one into a tree the caller did not name is how a walk becomes unbounded), and
/// never fails: a directory it cannot read is a directory with nothing in it.
///
/// The order is the contract worth pinning. Children are sorted at every level and the result is sorted by
/// [`Entry::path`], so a caller with a byte budget spends it on the same files twice, which is the
/// difference between a narrowing that can be reproduced and one that cannot.
#[must_use]
pub fn find(root: &Path, policy: &WalkPolicy, wants: &dyn Fn(&str) -> bool) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    let mut frontier: Vec<PathBuf> = vec![root.to_path_buf()];
    for depth in 0..=policy.max_depth {
        let mut next: Vec<PathBuf> = Vec::new();
        for dir in &frontier {
            let Ok(children_here) = std::fs::read_dir(dir) else {
                continue;
            };
            let mut children: Vec<PathBuf> =
                children_here.flatten().map(|child| child.path()).collect();
            // Sorted at every level, so the walk is one order rather than whatever readdir handed back.
            children.sort();
            for path in children {
                let Ok(meta) = path.symlink_metadata() else {
                    continue;
                };
                if meta.is_dir() {
                    if depth >= policy.max_depth {
                        continue;
                    }
                    let name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    if policy.skips(&name) {
                        continue;
                    }
                    next.push(path);
                    continue;
                }
                if !meta.is_file() {
                    continue;
                }
                let relative = path
                    .strip_prefix(root)
                    .unwrap_or(path.as_path())
                    .to_string_lossy()
                    .replace('\\', "/");
                if !wants(&relative) {
                    continue;
                }
                out.push(Entry {
                    path: relative,
                    absolute: path,
                    size: meta.len(),
                    modified: modified_secs(&meta),
                });
            }
        }
        frontier = next;
    }
    out.sort_by(|left, right| left.path.cmp(&right.path));
    out
}

/// One file's bytes.
///
/// The one read this module does, and the whole reason it exists: a caller above the door asks for bytes by
/// path and gets an `io::Result` rather than a silent empty vector, because "no file" and "could not look"
/// are different answers and only one of them is about the repository (ADR-001 §7.2).
///
/// # Errors
///
/// Whatever `std::fs::read` reports.
pub fn read(path: &Path) -> std::io::Result<Vec<u8>> {
    std::fs::read(path)
}

/// Last touch in whole seconds, or `0`.
///
/// The same rule [`crate::state`] uses for the working tree: a filesystem that will not say is reported as
/// unknown rather than guessed at, because a digest over the wrong answer is worse than a digest over a
/// stated zero.
fn modified_secs(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|when| when.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |since| i64::try_from(since.as_secs()).unwrap_or(0))
}
