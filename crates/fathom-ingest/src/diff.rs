//! `diff.rs` — the changed-path set between two trees, and the per-file measurement of one blob pair
//! (e1.4 / reco-f28).
//!
//! ADR-004 §6.6 states the requirement this module exists to meet twice, in two halves:
//!
//! * **R1** — "the store exposes `changed_paths(commit)` and per-path `blob_of(path, tree)`, and warm
//!   analysis consumes the changed set";
//! * **R7** — "warm re-index after one commit reads O(changed files) git objects, never O(tree)".
//!
//! Both are claims about *what is read*, which is the only part of a warm index anyone can hold a
//! stranger to. [`Repository::changed_paths`] therefore walks the two trees in parallel and descends
//! only into a subtree whose entry differs, so the objects it reads are the spine down to each change
//! plus the changed leaves — not every tree in the repository. [`Repository::tree_files`] is the
//! opposite: it enumerates everything, is what a cold path and a verification test use, and is the
//! function the incremental one is checked against.
//!
//! # What this is not
//!
//! This is **not** the rename-aware diff that builds spine rows. That is [`crate::normalize`], which asks
//! `gix` for rewrites with a similarity threshold and so must look at every added and deleted blob in
//! the commit — correct for a spine row, where a rename is one row and not a delete plus an add, and
//! wrong for a warm path whose entire purpose is not reading unchanged objects. Rename and copy pairing
//! is `e2.4` (`reco-a0v`) and belongs to the `diff.rename_detect` analyzer, whose declared scope is a
//! commit pair (ADR-004 §5.1); here a rename is what git's raw tree diff says it is, and the two rows
//! say so too.
//!
//! The second half is [`measure_blobs`] and [`measure_bytes`]: line counts and `@@` headers for one
//! (old blob, new blob) pair, which is the per-file unit the analysis layer caches. They are the same
//! code [`crate::normalize`] runs — `normalize` now calls them — so a cached per-file analysis and a
//! spine row cannot disagree about how many lines changed, which is the difference between one reality
//! and two (ADR-001 §7.10). [`measure_bytes`] is public because a dirty working tree is a pair of byte
//! strings with no object written for either side: the provisional path of `e1.4`.

use std::collections::{BTreeMap, BTreeSet};

use crate::error::Error;
use crate::oid::Oid;
use crate::paths::{RepoPath, join};
use crate::record::{Content, Hunk, Limits, NotExamined};
use crate::repo::{BlobRead, Repository, TreeEntry, TreeEntryKind};

/// What happened to one path between two trees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathChangeKind {
    /// Absent before, present after.
    Added,
    /// Present before, absent after.
    Deleted,
    /// Present both sides, different object id.
    Modified,
    /// Present both sides, same object id, different mode — a `chmod`, which is a change with no content
    /// and must not be reported as nothing.
    ModeChanged,
    /// Present both sides with the same content but a different *kind* of entry: a file replaced by a
    /// symlink, a directory replaced by a file.
    TypeChanged,
}

impl PathChangeKind {
    /// The spine's spelling, for a caller that needs to line this up with
    /// [`crate::record::ChangeKind`]. `ModeChanged` and `TypeChanged` are both `modify` there: the
    /// `change_kind` column speaks git's vocabulary, and git has no letter for either.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            PathChangeKind::Added => "add",
            PathChangeKind::Deleted => "delete",
            PathChangeKind::Modified
            | PathChangeKind::ModeChanged
            | PathChangeKind::TypeChanged => "modify",
        }
    }
}

/// One side of a path, as a tree entry names it: which object, in which mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryRef {
    /// The object the entry points at — a blob for a file, a tree for a directory, a commit id for a
    /// gitlink.
    pub oid: Oid,
    /// The raw mode, e.g. `0o100644`.
    pub mode: u32,
    /// What the mode means.
    pub kind: TreeEntryKind,
}

impl EntryRef {
    fn from_entry(entry: &TreeEntry) -> Self {
        EntryRef {
            oid: entry.oid,
            mode: u32::from(entry.mode),
            kind: entry.kind,
        }
    }

    /// Whether this side is a directory.
    #[must_use]
    pub const fn is_tree(&self) -> bool {
        matches!(self.kind, TreeEntryKind::Tree)
    }
}

/// One path that differs between two trees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathChange {
    /// Repository-relative path, in git's bytes.
    pub path: RepoPath,
    /// What happened.
    pub kind: PathChangeKind,
    /// The old entry, `None` when the path was absent before.
    pub old: Option<EntryRef>,
    /// The new entry, `None` when the path is gone.
    pub new: Option<EntryRef>,
}

impl PathChange {
    /// The blob this change points *at*, preferring the new side (a deletion has only an old one).
    /// `None` for a directory or a gitlink, which have no blob to read.
    #[must_use]
    pub fn blob(&self) -> Option<Oid> {
        let entry = self.new.as_ref().or(self.old.as_ref())?;
        match entry.kind {
            TreeEntryKind::File | TreeEntryKind::Executable | TreeEntryKind::Symlink => {
                Some(entry.oid)
            }
            TreeEntryKind::Tree | TreeEntryKind::Submodule => None,
        }
    }

    /// The kind as the spine spells it.
    #[must_use]
    pub fn kind_text(&self) -> &'static str {
        self.kind.as_str()
    }
}

/// One file in a tree: its path, its blob and its size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeFile {
    /// Repository-relative path.
    pub path: RepoPath,
    /// The blob's id.
    pub oid: Oid,
    /// The raw mode.
    pub mode: u32,
    /// The blob's size, taken from the object header, so nothing was inflated to learn it
    /// ([`Repository::object_header`]).
    pub size: u64,
}

/// A whole-tree enumeration, plus the reason it stopped if it stopped early.
///
/// A partial enumeration is a value and not an error, because "the tree is bigger than you asked for"
/// is an answer (ADR-001 §7.6): the caller keeps the prefix it paid for and says which number it is
/// holding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeListing {
    /// The files, in byte order over paths — git's own collation.
    pub files: Vec<TreeFile>,
    /// How many tree objects were read. This is the number a warm-path test asserts against, because it
    /// grows with the *tree*, which is exactly what a warm path may not do (ADR-004 §6.6 R2). Nothing on
    /// the warm path calls [`Repository::tree_files`].
    pub directories: usize,
    /// Whether [`Self::files`] is the whole tree or a prefix.
    pub truncated: bool,
}

impl TreeListing {
    /// Whether the enumeration reached every path.
    #[must_use]
    pub const fn complete(&self) -> bool {
        !self.truncated
    }

    /// Total bytes of every listed blob.
    #[must_use]
    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|file| file.size).sum()
    }
}

/// One directory's entries, keyed by name.
type Directory = BTreeMap<Vec<u8>, TreeEntry>;

impl Repository {
    /// The paths that differ between `old_tree` and `new_tree`, in byte order over paths.
    ///
    /// Either side may be `None`, and `None` means the empty tree, which is what makes this the whole
    /// diff of a root commit as well as the diff of two ordinary revisions.
    ///
    /// Cost: an entry with an identical id *and* mode on both sides is not descended, so the objects read
    /// are the spine down to each change plus the changed leaves (ADR-004 §6.6 R7). A tree with no
    /// change anywhere costs two tree objects, whatever the repository's size — the property
    /// `warm.reindex.scaling_ratio` is really about.
    ///
    /// # Errors
    ///
    /// [`Error::ObjectUnavailable`] if a tree *on a path to a difference* is not in this clone, plus the
    /// usual read failures. A tree that is never descended is never read, so an object only an unchanged
    /// subtree would have needed may be missing without failing the call — which is what makes this work
    /// on a partial clone for a change outside the omitted set.
    pub fn changed_paths(
        &self,
        old_tree: Option<Oid>,
        new_tree: Option<Oid>,
    ) -> Result<Vec<PathChange>, Error> {
        let mut out: Vec<PathChange> = Vec::new();
        let mut stack: Vec<(Vec<u8>, Option<Oid>, Option<Oid>)> = Vec::with_capacity(64);
        stack.push((Vec::new(), old_tree, new_tree));
        // An explicit stack, not recursion: a repository is hostile input (ADR-004 §7.7), and a tree
        // nested deeper than the call stack is a panic instead of an answer if this recurses.
        while let Some((prefix, old, new)) = stack.pop() {
            let old_entries = old
                .map(|oid| self.directory(oid))
                .transpose()?
                .unwrap_or_default();
            let new_entries = new
                .map(|oid| self.directory(oid))
                .transpose()?
                .unwrap_or_default();
            for name in merged_names(&old_entries, &new_entries) {
                let old_entry = old_entries.get(&name).map(EntryRef::from_entry);
                let new_entry = new_entries.get(&name).map(EntryRef::from_entry);
                let path = if prefix.is_empty() {
                    name
                } else {
                    join(&prefix, &name)
                };
                match classify(old_entry, new_entry, path.clone()) {
                    // Byte-identical entry and mode: nothing below it can differ, and nothing below it
                    // is read. This one case is the whole reason the walk is cheap.
                    Decision::Unchanged => {}
                    Decision::Descend { old, new } => stack.push((path, old, new)),
                    Decision::Report(change) => out.push(change),
                    // A directory on one side and something that is not a directory on the other: git
                    // reports the entry *and* every path inside the directory it replaced, so this
                    // reports the same set — the entry here, the contents by descending with the other
                    // side absent.
                    Decision::ReportAndDescend { change, old, new } => {
                        out.push(change);
                        stack.push((path, old, new));
                    }
                }
            }
        }
        out.sort_by(|left, right| left.path.as_bytes().cmp(right.path.as_bytes()));
        Ok(out)
    }

    /// The entry `tree` holds at `path`, or `None` when the tree has no file there.
    ///
    /// ADR-004 §6.6 R1's `blob_of(path, tree)`. Cost is one tree object per path component. A component
    /// that names a file rather than a directory means the path is not in the tree, which is answered
    /// with `None` rather than with a failure: a caller asking where `src/main.rs/README.md` lives is
    /// asking about a path that does not exist, not about a broken repository.
    ///
    /// # Errors
    ///
    /// As [`changed_paths`](Self::changed_paths).
    pub fn blob_of(&self, tree: Oid, path: &RepoPath) -> Result<Option<EntryRef>, Error> {
        let components: Vec<&[u8]> = path
            .as_bytes()
            .split(|byte| *byte == b'/')
            .filter(|component| !component.is_empty())
            .collect();
        let Some((&last, ancestors)) = components.split_last() else {
            // The empty path names the tree itself, which is not an entry and has no mode.
            return Ok(None);
        };
        let mut current = tree;
        for component in ancestors {
            let entries = self.directory(current)?;
            match entries.get(*component) {
                Some(entry) if entry.kind == TreeEntryKind::Tree => current = entry.oid,
                Some(_) => return Ok(None),
                None => return Ok(None),
            }
        }
        Ok(self
            .directory(current)?
            .get(last)
            .map(EntryRef::from_entry)
            // A directory or a gitlink is not a blob: the question was "which blob", and a prefix is not
            // an answer to it.
            .filter(|entry| !matches!(entry.kind, TreeEntryKind::Tree | TreeEntryKind::Submodule)))
    }

    /// Every file in `tree`, with its blob and size, in byte order over paths.
    ///
    /// This reads every tree object in the tree and one object header per file: it is the cold path's
    /// enumeration and the verification baseline for the incremental one. Nothing on a warm path may
    /// call it (ADR-004 §6.6 R2), which is testable, because `directories` counts what it read.
    ///
    /// `limit` caps the number of files collected; `None` means "everything".
    ///
    /// # Errors
    ///
    /// As [`changed_paths`](Self::changed_paths).
    pub fn tree_files(&self, tree: Oid, limit: Option<usize>) -> Result<TreeListing, Error> {
        let mut files: Vec<TreeFile> = Vec::new();
        let mut directories = 0_usize;
        let mut stack: Vec<(Vec<u8>, Oid)> = vec![(Vec::new(), tree)];
        while let Some((prefix, oid)) = stack.pop() {
            directories += 1;
            for (name, entry) in self.directory(oid)? {
                let path = if prefix.is_empty() {
                    name
                } else {
                    join(&prefix, &name)
                };
                match entry.kind {
                    TreeEntryKind::Tree => stack.push((path, entry.oid)),
                    // A gitlink points at a commit, not a blob: it is not a file, and pretending it is
                    // one would put a size of 0 on a submodule pointer.
                    TreeEntryKind::Submodule => {}
                    _ => {
                        let size = self
                            .object_header(entry.oid)?
                            .map(|header| header.size)
                            .unwrap_or(0);
                        files.push(TreeFile {
                            path: RepoPath::new(path),
                            oid: entry.oid,
                            mode: u32::from(entry.mode),
                            size,
                        });
                        if limit.is_some_and(|cap| files.len() >= cap) {
                            sort_files(&mut files);
                            return Ok(TreeListing {
                                files,
                                directories,
                                truncated: true,
                            });
                        }
                    }
                }
            }
        }
        sort_files(&mut files);
        Ok(TreeListing {
            files,
            directories,
            truncated: false,
        })
    }

    /// One tree's entries keyed by name, which is what makes the parallel walk independent of git's
    /// directory-collation subtlety: a subtree sorts as if its name ended in `/`, so two sorted lists
    /// are not comparable entry-by-entry once directories and files interleave.
    fn directory(&self, oid: Oid) -> Result<Directory, Error> {
        Ok(self
            .tree_entries(oid)?
            .into_iter()
            .map(|entry| (entry.name.as_bytes().to_vec(), entry))
            .collect())
    }
}

/// What to do with one name that appears on at least one side of a directory pair.
enum Decision {
    /// Nothing to say, and nothing below to read.
    Unchanged,
    /// The difference is inside a directory pair, or inside a directory one side lost: descend.
    Descend {
        /// The old tree, `None` when this side did not exist.
        old: Option<Oid>,
        /// The new tree, `None` when this side did not exist.
        new: Option<Oid>,
    },
    /// One change, reported at this path.
    Report(PathChange),
    /// A change at this path *and* a subtree whose contents each need their own row.
    ReportAndDescend {
        /// The change for the entry itself.
        change: PathChange,
        /// The subtree to walk, with the side that is not a directory left out.
        old: Option<Oid>,
        /// The subtree to walk.
        new: Option<Oid>,
    },
}

/// The rule, stated once so the walk and the test can cite the same thing.
///
/// It is `git diff --raw`'s rule, measured against git rather than remembered: a subtree on either side
/// is descended into — with the other side treated as absent when it is not a subtree — because git
/// names every path inside a directory that appeared or disappeared, and a warm path that reported only
/// the directory would under-report the work its analyzers have to do. `tests/changed_paths.rs` asserts
/// the agreement on fixtures, including the case that makes this interesting: a directory replaced by a
/// file is the new file plus every path that was inside the old one.
fn classify(old: Option<EntryRef>, new: Option<EntryRef>, path: Vec<u8>) -> Decision {
    if matches!((&old, &new), (Some(o), Some(n)) if o.oid == n.oid && o.mode == n.mode) {
        return Decision::Unchanged;
    }
    let at = |kind: PathChangeKind, old: Option<EntryRef>, new: Option<EntryRef>| PathChange {
        path: RepoPath::new(path.clone()),
        kind,
        old,
        new,
    };
    match (old, new) {
        // Two directories, different: the difference is inside.
        (Some(old), Some(new)) if old.is_tree() && new.is_tree() => Decision::Descend {
            old: Some(old.oid),
            new: Some(new.oid),
        },
        // A directory became a file, or a file became a directory. git names the entry on the side that
        // is now a plain entry — added if it is the new side, deleted if it is the old one — and then
        // every path inside the directory, each with only its own side present.
        (Some(old), Some(new)) if !old.is_tree() && new.is_tree() => Decision::ReportAndDescend {
            change: at(PathChangeKind::Deleted, Some(old), None),
            old: None,
            new: Some(new.oid),
        },
        (Some(old), Some(new)) if old.is_tree() && !new.is_tree() => Decision::ReportAndDescend {
            change: at(PathChangeKind::Added, None, Some(new)),
            old: Some(old.oid),
            new: None,
        },
        // Same kind, different object: a content change, or a mode-only change, which has no content
        // change to report and must still be reported.
        (Some(old), Some(new)) if old.kind == new.kind => Decision::Report(at(
            if old.oid == new.oid {
                PathChangeKind::ModeChanged
            } else {
                PathChangeKind::Modified
            },
            Some(old),
            Some(new),
        )),
        // A file that became a symlink, a symlink that became a gitlink: git spells this `T`.
        (Some(_), Some(_)) => Decision::Report(at(PathChangeKind::TypeChanged, old, new)),
        // One side absent. If the present side is a directory, every path inside it appeared or vanished.
        (None, Some(new)) if new.is_tree() => Decision::Descend {
            old: None,
            new: Some(new.oid),
        },
        (Some(old), None) if old.is_tree() => Decision::Descend {
            old: Some(old.oid),
            new: None,
        },
        (None, Some(entry)) => Decision::Report(at(PathChangeKind::Added, None, Some(entry))),
        (Some(entry), None) => Decision::Report(at(PathChangeKind::Deleted, Some(entry), None)),
        // Unreachable: a name only enters the union by existing on one side.
        (None, None) => Decision::Unchanged,
    }
}

/// The union of two directories' names, in byte order.
///
/// A `BTreeSet` rather than a hand-rolled merge: the two halves are already sorted, but the set makes the
/// union and the ordering the same operation, which is one less thing to get subtly wrong at the point
/// where a missed duplicate would silently duplicate every change under it.
fn merged_names(old: &Directory, new: &Directory) -> Vec<Vec<u8>> {
    old.keys()
        .chain(new.keys())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn sort_files(files: &mut [TreeFile]) {
    files.sort_by(|left, right| left.path.as_bytes().cmp(right.path.as_bytes()));
}

/// What was measured about one pair of blobs' content, and what was not.
///
/// The same shape the spine records per file (`FileRecord`'s content fields), for the same reason: an
/// absence is a state with a reason and never a zero ([`Content`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Measurement {
    /// Text, binary, or not examined with the reason why not.
    pub content: Content,
    /// Lines added.
    pub added: u32,
    /// Lines removed.
    pub removed: u32,
    /// The `@@` headers, capped at [`Limits::hunk_headers_per_file`].
    pub hunks: Vec<Hunk>,
    /// How many hunks there really were, which is what the cap is measured against.
    pub hunk_total: usize,
}

impl Measurement {
    /// Nothing was measured, with the reason.
    #[must_use]
    pub fn unmeasured(content: Content) -> Self {
        Measurement {
            content,
            added: 0,
            removed: 0,
            hunks: Vec::new(),
            hunk_total: 0,
        }
    }

    /// Whether anything at all was measured.
    #[must_use]
    pub fn measured(&self) -> bool {
        matches!(self.content, Content::Text) && self.hunk_total > 0
    }
}

/// Measure one (old blob, new blob) pair by id, reading each side under [`Limits::blob_memory_cap`].
///
/// # Errors
///
/// [`Error::ObjectUnavailable`] if a side is named and missing, plus the usual read failures. A blob
/// *over the cap* is not an error: it is [`Content::NotExamined`] with [`NotExamined::Oversized`],
/// because a file too big to diff is a fact a reviewer needs and a failure is not.
pub fn measure_blobs(
    repo: &Repository,
    old: Option<Oid>,
    new: Option<Oid>,
    limits: &Limits,
) -> Result<Measurement, Error> {
    let before = read_side(repo, old, limits)?;
    let after = read_side(repo, new, limits)?;
    Ok(match (before, after) {
        (Side::Skipped(reason), _) | (_, Side::Skipped(reason)) => {
            Measurement::unmeasured(Content::NotExamined { reason })
        }
        (before, after) => measure_bytes(
            before.bytes().unwrap_or_default(),
            after.bytes().unwrap_or_default(),
            limits,
        ),
    })
}

/// git's binary rule: a NUL in the first 8 KiB (`FIRST_FEW_BYTES` in git's `xdiff-interface.c`).
///
/// Lives here because this module is the only place content is compared; `crate::normalize` re-exports it
/// so the name stayed where `e1.2` published it.
#[must_use]
pub fn is_binary(bytes: &[u8]) -> bool {
    const FIRST_FEW_BYTES: usize = 8 * 1024;
    bytes
        .iter()
        .take(FIRST_FEW_BYTES)
        .any(|byte| *byte == b'\0')
}

/// Measure one pair of byte strings: the binary rule, then the diff, then the hunk cap.
///
/// Public because the working tree is a pair of byte strings with no object written for either side:
/// the provisional per-file analysis of `e1.4` has HEAD's bytes and the file's bytes in hand and no
/// blob id for one of them.
#[must_use]
pub fn measure_bytes(old: &[u8], new: &[u8], limits: &Limits) -> Measurement {
    if is_binary(old) || is_binary(new) {
        return Measurement::unmeasured(Content::Binary);
    }
    if old.is_empty() && new.is_empty() {
        // A mode-only change on an empty file, or a rename of one: nothing to diff, and `Text` is still
        // the truth about the content.
        return Measurement::unmeasured(Content::Text);
    }
    let input = gix::diff::blob::InternedInput::new(old, new);
    let diff =
        gix::diff::blob::diff_with_slider_heuristics(gix::diff::blob::Algorithm::Histogram, &input);
    let mut hunks = HunkCollector::default();
    // The hunk list comes from `gix`'s unified-diff writer rather than from the raw change ranges, so
    // the `@@` lines carry the caller's context and merge when their contexts overlap — git's headers,
    // not imara's context-free ones. `e1.2`'s comment in `normalize.rs` says why that is a correctness
    // distinction and not a cosmetic one.
    let rendered = gix::diff::blob::UnifiedDiff::new(
        &diff,
        &input,
        &mut hunks,
        gix::diff::blob::unified_diff::ContextSize::symmetrical(limits.hunk_context_lines),
    )
    .consume();
    let mut hunks = hunks.0;
    if rendered.is_err() {
        // The writer's only failure mode is an I/O error and the sink above cannot produce one. Were it
        // ever to, the counts are still true and the headers are simply absent, so half an answer is
        // returned with its headers empty rather than no answer at all.
        return Measurement {
            content: Content::Text,
            added: diff.count_additions(),
            removed: diff.count_removals(),
            hunks: Vec::new(),
            hunk_total: 0,
        };
    }
    let hunk_total = hunks.len();
    hunks.truncate(limits.hunk_headers_per_file.max(1));
    Measurement {
        content: Content::Text,
        added: diff.count_additions(),
        removed: diff.count_removals(),
        hunks,
        hunk_total,
    }
}

/// One side of a content comparison.
enum Side {
    /// There was no entry on this side.
    Absent,
    /// The bytes, already known to be within the cap.
    Bytes(Vec<u8>),
    /// Not read, with the reason.
    Skipped(NotExamined),
}

impl Side {
    fn bytes(&self) -> Option<&[u8]> {
        match self {
            Side::Absent => None,
            Side::Bytes(bytes) => Some(bytes),
            Side::Skipped(_) => None,
        }
    }
}

fn read_side(repo: &Repository, oid: Option<Oid>, limits: &Limits) -> Result<Side, Error> {
    let Some(oid) = oid else {
        return Ok(Side::Absent);
    };
    let cap = usize::try_from(limits.blob_memory_cap).unwrap_or(usize::MAX);
    match repo.blob(oid, cap)? {
        BlobRead::Present { bytes, .. } => Ok(Side::Bytes(bytes)),
        BlobRead::Oversized { size } => Ok(Side::Skipped(NotExamined::Oversized {
            size: u64::try_from(size).unwrap_or(u64::MAX),
        })),
        BlobRead::WrongKind { kind } => Ok(Side::Skipped(NotExamined::NotABlob {
            found: kind.as_str().to_owned(),
        })),
    }
}

/// Collects `@@` headers out of `gix`'s unified-diff writer — the same collector `crate::normalize`
/// used before this code was factored out, and the reason a cached measurement and a spine row agree.
#[derive(Default)]
struct HunkCollector(Vec<Hunk>);

impl gix::diff::blob::unified_diff::ConsumeHunk for &mut HunkCollector {
    type Out = ();

    fn consume_hunk(
        &mut self,
        header: gix::diff::blob::unified_diff::HunkHeader,
        _lines: &[(gix::diff::blob::unified_diff::DiffLineKind, &[u8])],
    ) -> std::io::Result<()> {
        self.0.push(Hunk::new(
            header.before_hunk_start,
            header.before_hunk_len,
            header.after_hunk_start,
            header.after_hunk_len,
        ));
        Ok(())
    }

    fn finish(self) {}
}

/// What a working-tree path yielded when it was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkingRead {
    /// The bytes.
    Present {
        /// The content.
        bytes: Vec<u8>,
    },
    /// Larger than the caller's cap, so nothing was read — the same outcome a blob over the cap gets
    /// ([`NotExamined::Oversized`]), because an uncommitted file is not allowed to be the one case that
    /// inflates memory.
    Oversized {
        /// The size `metadata` reported.
        size: u64,
    },
    /// The file is gone: a deletion in the worktree, or a path git knows that the filesystem no longer has.
    Absent,
    /// It could not be opened, with the reason.
    Unreadable {
        /// The OS's words.
        reason: String,
    },
}

/// Read a working-tree file, under the same cap as a blob.
///
/// This lives here rather than in `fathom-analyze` for the reason ADR-002 §3 gives for the whole crate
/// boundary: reading the filesystem is ingestion's job, so the size cap, the "is it gone" answer and the
/// error wording are the same ones every other reader gets. Analysis calls [`measure_working`] instead of
/// touching a path itself.
///
/// # Errors
///
/// Only the repository-side ones: a bare repository has no work tree to read
/// ([`Error::WorkingTreeUnavailable`]). A file that will not open is a [`WorkingRead::Unreadable`], because
/// "someone chmod-ed 000 the fixture" is an answer and not a failed run.
pub fn read_working(repo: &Repository, path: &RepoPath, cap: u64) -> Result<WorkingRead, Error> {
    let root = repo
        .work_dir()
        .ok_or_else(|| Error::WorkingTreeUnavailable {
            root: repo.git_dir().display().to_string(),
            why: crate::error::WorkingTreeUnavailable::Bare,
        })?;
    let native = path.to_native(root);
    match std::fs::metadata(&native) {
        Ok(metadata) if !metadata.is_file() => Ok(WorkingRead::Unreadable {
            reason: format!("`{}` is not a regular file", native.display()),
        }),
        Ok(metadata) if metadata.len() > cap => Ok(WorkingRead::Oversized {
            size: metadata.len(),
        }),
        Ok(_) => match std::fs::read(&native) {
            Ok(bytes) => Ok(WorkingRead::Present { bytes }),
            Err(err) => Ok(WorkingRead::Unreadable {
                reason: format!("`{}`: {err}", native.display()),
            }),
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(WorkingRead::Absent),
        Err(err) => Ok(WorkingRead::Unreadable {
            reason: format!("`{}`: {err}", native.display()),
        }),
    }
}

/// Measure a working-tree file against the committed side it would replace.
///
/// The point of the function is that the answer is *identical* to what `measure_blobs` will say once the
/// file is committed: the bytes are the same bytes, the cap is the same cap, and the diff is the same diff.
/// That is what ADR-004 §6.4 R6 means by "reused if it happens to be right" — the provisional number and
/// the committed number are the same number, so a panel that showed an uncommitted diff and then showed the
/// committed one cannot tell a different story in between.
///
/// # Errors
///
/// As [`read_working`], plus the object-side errors of [`measure_blobs`].
pub fn measure_working(
    repo: &Repository,
    path: &RepoPath,
    old: Option<Oid>,
    limits: &Limits,
) -> Result<Measurement, Error> {
    let cap = usize::try_from(limits.blob_memory_cap).unwrap_or(usize::MAX);
    let before = read_side(repo, old, limits)?;
    let after = match read_working(repo, path, limits.blob_memory_cap)? {
        WorkingRead::Present { bytes } => {
            // The cap was checked against `metadata`, and the file may have changed in between; the same
            // refusal applies to what was actually read, so this cannot be a path around the cap.
            if bytes.len() > cap {
                Side::Skipped(NotExamined::Oversized {
                    size: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
                })
            } else {
                Side::Bytes(bytes)
            }
        }
        WorkingRead::Oversized { size } => Side::Skipped(NotExamined::Oversized { size }),
        WorkingRead::Absent => Side::Absent,
        // Not a `NotExamined` variant: `record.rs`'s enum is the published set of reasons a *committed*
        // object went unexamined, and "the file on disk would not open" is a working-tree fact that the
        // caller reports as its own stated absence rather than burying in a measurement.
        WorkingRead::Unreadable { reason } => {
            return Err(Error::PathUnreadable {
                path: path.quoted(),
                reason,
            });
        }
    };
    Ok(match (before, after) {
        (Side::Skipped(reason), _) | (_, Side::Skipped(reason)) => {
            Measurement::unmeasured(Content::NotExamined { reason })
        }
        (before, after) => measure_bytes(
            before.bytes().unwrap_or_default(),
            after.bytes().unwrap_or_default(),
            limits,
        ),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn names(items: &[&str]) -> Vec<Vec<u8>> {
        items.iter().map(|item| item.as_bytes().to_vec()).collect()
    }

    fn directory(items: &[&str]) -> Directory {
        items
            .iter()
            .map(|name| {
                (
                    name.as_bytes().to_vec(),
                    TreeEntry {
                        name: RepoPath::new(*name),
                        mode: 0o100644,
                        kind: TreeEntryKind::File,
                        oid: Oid::ZERO,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn merged_names_unions_two_sorted_key_lists_without_duplicates() {
        let old = directory(&["a", "b", "d"]);
        let new = directory(&["b", "c", "e"]);
        assert_eq!(
            merged_names(&old, &new),
            names(&["a", "b", "c", "d", "e"]),
            "the merge walks both sides once and names each path exactly once"
        );
        assert!(merged_names(&Directory::new(), &Directory::new()).is_empty());
    }

    #[test]
    fn merged_names_holds_when_git_sorts_a_subtree_before_a_deeper_file() {
        // `src` (a directory) sorts as `src/`, which is after `src-2` and before `src2`. Byte order on
        // the raw names would disagree, and this is the trap the keyed merge exists to avoid.
        let old = directory(&["src-2"]);
        let new = directory(&["src2"]);
        assert_eq!(merged_names(&old, &new), names(&["src-2", "src2"]));
    }

    #[test]
    fn a_mode_only_change_is_a_change_and_a_directory_has_no_blob() {
        let mode_only = PathChange {
            path: RepoPath::new("script.sh"),
            kind: PathChangeKind::ModeChanged,
            old: Some(EntryRef {
                oid: Oid::ZERO,
                mode: 0o100644,
                kind: TreeEntryKind::File,
            }),
            new: Some(EntryRef {
                oid: Oid::ZERO,
                mode: 0o100755,
                kind: TreeEntryKind::Executable,
            }),
        };
        assert_eq!(mode_only.kind_text(), "modify");
        assert_eq!(mode_only.blob(), Some(Oid::ZERO));

        let directory_gone = PathChange {
            path: RepoPath::new("src"),
            kind: PathChangeKind::TypeChanged,
            old: Some(EntryRef {
                oid: Oid::ZERO,
                mode: 0o040000,
                kind: TreeEntryKind::Tree,
            }),
            new: None,
        };
        assert_eq!(directory_gone.blob(), None);
        assert_eq!(directory_gone.kind_text(), "modify");
    }

    #[test]
    fn measurement_of_nothing_says_so_rather_than_reporting_zero_lines() {
        let limits = Limits::default();
        let empty = measure_bytes(b"", b"", &limits);
        assert_eq!(empty.hunk_total, 0);
        assert!(!empty.measured());
        let binary = measure_bytes(b"a\0b", b"a\nb", &limits);
        assert!(binary.content.is_binary());
        assert!(!binary.measured());
    }

    #[test]
    fn one_line_changed_is_one_hunk_and_two_lines_of_count() {
        let limits = Limits::default();
        let before = b"one\ntwo\nthree\n";
        let after = b"one\nTWO\nthree\n";
        let measured = measure_bytes(before, after, &limits);
        assert_eq!((measured.added, measured.removed), (1, 1));
        assert_eq!(measured.hunk_total, 1);
        assert!(measured.measured());
    }
}
