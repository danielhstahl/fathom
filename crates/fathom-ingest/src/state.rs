//! `state.rs` — the working tree: what is staged, what is unstaged, and what conflicts (e1.1).
//!
//! The ticket's last line is the one that makes this module exist: later tickets have to show uncommitted
//! work, and a review tool that only sees commits is a review tool that misses the agent's last turn.
//!
//! The comparison is git's own, in three places:
//!
//! * **staged** is HEAD's tree against the index — same rule as `git diff --cached`.
//! * **unstaged** is the index against the files on disk, using git's stat shortcut: an entry whose size
//!   and mtime still match the index is *assumed* clean, and only the entries that fail that check are
//!   hashed. `ScanOptions::HashEveryFile` turns the shortcut off, which is what `git status` effectively
//!   does for the handful of racy entries written in the last second.
//! * **conflicts** are index entries at stage 1/2/3, which is the only place a merge's base/ours/theirs
//!   survives after `git mergetool` starts.
//!
//! Untracked files are deliberately **not** reported. Finding them means a full directory walk plus
//! `.gitignore`, `.git/info/exclude` and `core.excludesFile` evaluation, and `gix` gates all of that
//! behind the `dirwalk`/`excludes` features, which are off the local-only allow-list ADR-002 §4.2
//! pins. A `WorkingState` therefore says so in [`StateNotice::UntrackedNotScanned`] rather than looking
//! like it reported everything.

use std::path::{Path, PathBuf};

use crate::error::{Error, WorkingTreeUnavailable};
use crate::oid::Oid;
use crate::paths::RepoPath;
use crate::repo::{Head, LinkedWorktree, Repository, Submodule, SubmoduleState};

/// How carefully to look at the working tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScanMode {
    /// Trust the index's stat data: only files whose size or mtime disagree with the index are hashed.
    /// This is git's rule and it is what makes a status scan of a 100k-file repository take milliseconds.
    #[default]
    StatShortcut,
    /// Hash every tracked file. Slower by definition, and correct in the case the shortcut is not: a
    /// file rewritten with the same length inside the same second, on a filesystem with whole-second
    /// mtimes.
    HashEveryFile,
}

/// How much of the working tree to read.
#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// The stat shortcut, or not.
    pub mode: ScanMode,
    /// Largest file that is hashed rather than reported as oversized. The number is
    /// `benchmarks/budgets.json`'s `ingest.file_size_cap.mib`, and it is a cap on memory, not on
    /// politeness.
    pub max_file_bytes: u64,
    /// Stop after this many tracked entries; `None` means no cap.
    pub max_entries: Option<usize>,
    /// Also read submodule working trees (one open per submodule).
    pub recurse_submodules: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        ScanOptions {
            mode: ScanMode::default(),
            // 8 MiB, from budgets.json — the same cap `e1.2` refuses to slurp past.
            max_file_bytes: 8 * 1024 * 1024,
            max_entries: None,
            recurse_submodules: false,
        }
    }
}

/// Which side of the index a change is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Side {
    /// HEAD against the index: what a `git commit` would record (`git diff --cached`).
    Staged,
    /// The index against the files on disk: what `git add` would take (`git diff`).
    Unstaged,
}

impl Side {
    /// The git diff this corresponds to, for a hint or a `--help` line.
    #[must_use]
    pub fn git_equivalent(self) -> &'static str {
        match self {
            Side::Staged => "git diff --cached",
            Side::Unstaged => "git diff",
        }
    }
}

/// What changed.
///
/// Ordered by severity-of-surprise, which is also a convenient sort key when a status list is grouped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChangeKind {
    /// Present on one side, absent on the other.
    Added,
    /// Present on both, contents differ.
    Modified,
    /// Present on both, only the mode differs (the executable bit is the usual story).
    ModeChanged,
    /// Absent on one side (a delete on the index side, a missing file on the worktree side).
    Deleted,
    /// A file became a directory, a file became a symlink, or vice versa.
    TypeChanged,
    /// Too big to hash under the caller's cap, so it was not compared (see `ingest.file_size_cap.mib`).
    Oversized,
}

impl ChangeKind {
    /// One letter, the way `git status --porcelain` renders it.
    #[must_use]
    pub fn letter(self) -> char {
        match self {
            ChangeKind::Added => 'A',
            ChangeKind::Modified => 'M',
            ChangeKind::ModeChanged => 'M',
            ChangeKind::Deleted => 'D',
            ChangeKind::TypeChanged => 'T',
            ChangeKind::Oversized => '?',
        }
    }
}

/// One recorded side of a comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackedEntry {
    /// The blob's id.
    pub oid: Oid,
    /// The raw mode, e.g. `0o100755`.
    pub mode: u32,
}

/// What was found on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    /// What kind of filesystem object it is.
    pub kind: WorktreeKind,
    /// The blob id it would hash to, or `None` when it was not hashed (the stat shortcut said clean, or
    /// it was oversized).
    pub oid: Option<Oid>,
    /// Size in bytes, as far as it could be determined.
    pub size: u64,
}

/// The kinds of thing a tracked path can be on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeKind {
    /// A regular file.
    File,
    /// A symlink.
    Symlink,
    /// A directory where a file was expected.
    Directory,
    /// A gitlink/submodule checkout.
    Submodule,
}

/// One difference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// Which comparison found it.
    pub side: Side,
    /// What kind of difference it is.
    pub kind: ChangeKind,
    /// Repository-relative path, in git's bytes.
    pub path: RepoPath,
    /// What HEAD had, for a staged change; what the index had, for an unstaged one.
    pub before: Option<TrackedEntry>,
    /// What it is now.
    pub after: Option<TrackedEntry>,
    /// For an unstaged change: what the stat/first-read said about the file.
    pub on_disk: Option<WorktreeEntry>,
}

impl Change {
    /// The path as text, for a line of output.
    #[must_use]
    pub fn path_text(&self) -> std::borrow::Cow<'_, str> {
        self.path.to_string_lossy()
    }
}

/// A conflicted path, with the three stages git keeps in the index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// The path.
    pub path: RepoPath,
    /// Stage 1: the merge base.
    pub base: Option<TrackedEntry>,
    /// Stage 2: ours.
    pub ours: Option<TrackedEntry>,
    /// Stage 3: theirs.
    pub theirs: Option<TrackedEntry>,
}

/// Something the caller should know about a working-tree read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateNotice {
    /// Untracked files were not looked for (see the module docs: it needs a gix feature that is off).
    UntrackedNotScanned,
    /// This many entries were assumed clean by the stat shortcut without being hashed.
    StatAssumedClean {
        /// How many.
        count: usize,
    },
    /// This many files were larger than the cap and were not compared.
    Oversized {
        /// How many.
        count: usize,
    },
    /// The repository has no commits, so every index entry is an addition.
    HeadUnborn,
    /// There is no index: nothing is staged and nothing is tracked.
    NoIndex,
    /// A path could not be examined (permissions, broken symlink, exotic file type).
    UnreadablePath {
        /// Which one.
        path: RepoPath,
        /// Why.
        reason: String,
    },
    /// The scan stopped at the caller's entry cap.
    Capped {
        /// How many entries were examined.
        examined: usize,
    },
    /// A sparse checkout: some tracked paths are deliberately absent from disk.
    Sparse {
        /// How many entries are marked skip-worktree.
        count: usize,
    },
}

/// The working tree's state, as one snapshot.
#[derive(Debug, Clone)]
pub struct WorkingState {
    /// HEAD against the index.
    pub staged: Vec<Change>,
    /// The index against the files.
    pub unstaged: Vec<Change>,
    /// Unmerged paths.
    pub conflicts: Vec<Conflict>,
    /// What the reader wants the caller to know.
    pub notices: Vec<StateNotice>,
    /// What the index looked like.
    pub index: IndexSnapshot,
    /// How many tracked entries were examined.
    pub examined: usize,
    /// How many files were hashed to compare content.
    pub hashed: usize,
}

/// The index, described.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexSnapshot {
    /// Where the index file is, or would be.
    pub path: PathBuf,
    /// Whether the file exists.
    pub present: bool,
    /// The index format version: 2, 3 or 4.
    pub version: Option<u32>,
    /// How many entries, including conflict stages.
    pub entries: usize,
    /// Whether the index has sparse-directory entries.
    pub sparse: bool,
}

impl WorkingState {
    /// Nothing staged, nothing unstaged, nothing conflicted.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.staged.is_empty() && self.unstaged.is_empty() && self.conflicts.is_empty()
    }

    /// `+2 ~1 -0` style counts, staged first — the summary line a status pane wants.
    #[must_use]
    pub fn summary(&self) -> String {
        let count = |changes: &[Change], kind: ChangeKind| {
            changes.iter().filter(|change| change.kind == kind).count()
        };
        format!(
            "staged: {} added, {} modified, {} deleted | unstaged: {} modified, {} deleted | conflicts: {}",
            count(&self.staged, ChangeKind::Added),
            self.staged
                .iter()
                .filter(|c| matches!(c.kind, ChangeKind::Modified | ChangeKind::ModeChanged))
                .count(),
            count(&self.staged, ChangeKind::Deleted),
            self.unstaged
                .iter()
                .filter(|c| matches!(c.kind, ChangeKind::Modified | ChangeKind::ModeChanged))
                .count(),
            count(&self.unstaged, ChangeKind::Deleted),
            self.conflicts.len()
        )
    }
}

/// Read the working tree.
pub(crate) fn working_state(
    repo: &Repository,
    options: &ScanOptions,
) -> Result<WorkingState, Error> {
    let work_dir = match repo.identity().work_dir.as_ref() {
        None => {
            return Err(Error::WorkingTreeUnavailable {
                root: repo.identity().display_name.clone(),
                why: WorkingTreeUnavailable::Bare,
            });
        }
        Some(path) if !path.is_dir() => {
            return Err(Error::WorkingTreeUnavailable {
                root: repo.identity().display_name.clone(),
                why: WorkingTreeUnavailable::WorktreeMissing {
                    expected: path.display().to_string(),
                },
            });
        }
        Some(path) => path.clone(),
    };

    let index_path = repo.gix().index_path();
    let index = if index_path.is_file() {
        repo.gix()
            .open_index()
            .map_err(|err| Error::WorkingTreeUnavailable {
                root: repo.identity().display_name.clone(),
                why: WorkingTreeUnavailable::UnreadableIndex {
                    reason: err.to_string(),
                },
            })?
    } else {
        // No index is a state, not a failure: `git init` and no `git add` yet. Everything in HEAD looks
        // deleted, everything is unstaged-by-absence, and the notice says why the answer is empty.
        return Ok(WorkingState {
            staged: Vec::new(),
            unstaged: Vec::new(),
            conflicts: Vec::new(),
            notices: vec![StateNotice::NoIndex, StateNotice::UntrackedNotScanned],
            index: IndexSnapshot {
                path: index_path,
                present: false,
                version: None,
                entries: 0,
                sparse: false,
            },
            examined: 0,
            hashed: 0,
        });
    };

    let mut notices = vec![StateNotice::UntrackedNotScanned];
    let head_entries = match repo.head()?.oid() {
        Some(commit) => {
            let header = repo.commit(commit)?;
            match header {
                Some(header) => flatten_tree(repo, header.tree)?,
                None => Vec::new(),
            }
        }
        None => {
            notices.push(StateNotice::HeadUnborn);
            Vec::new()
        }
    };

    // Index entries, split into the normal (stage 0) set and the conflict stages, both in index order,
    // which is byte order over paths — the same order the flattened tree is sorted into.
    let mut normal: Vec<(&gix::index::Entry, Vec<u8>)> = Vec::with_capacity(index.entries().len());
    let mut conflicts: Vec<Conflict> = Vec::new();
    let mut conflict_stages: Vec<(Vec<u8>, Conflict)> = Vec::new();
    let mut sparse = 0usize;
    for entry in index.entries() {
        let path = entry.path(&index).to_vec();
        match entry.stage() {
            gix::index::entry::Stage::Unconflicted => {
                if entry
                    .flags
                    .contains(gix::index::entry::Flags::SKIP_WORKTREE)
                {
                    sparse += 1;
                    continue;
                }
                if entry.flags.contains(gix::index::entry::Flags::ASSUME_VALID) {
                    // `--assume-unchanged`: the owner told us not to look. Respect it, like git does.
                    continue;
                }
                normal.push((entry, path));
            }
            stage => {
                let tracked = TrackedEntry {
                    oid: crate::repo::from_gix(entry.id.as_ref()),
                    mode: entry.mode.bits(),
                };
                if let Some((_, existing)) = conflict_stages
                    .iter_mut()
                    .find(|(existing_path, _)| *existing_path == path)
                {
                    match stage {
                        gix::index::entry::Stage::Base => existing.base = Some(tracked),
                        gix::index::entry::Stage::Ours => existing.ours = Some(tracked),
                        gix::index::entry::Stage::Theirs => existing.theirs = Some(tracked),
                        gix::index::entry::Stage::Unconflicted => {}
                    }
                } else {
                    let mut conflict = Conflict {
                        path: RepoPath::new(path.clone()),
                        base: None,
                        ours: None,
                        theirs: None,
                    };
                    match stage {
                        gix::index::entry::Stage::Base => conflict.base = Some(tracked),
                        gix::index::entry::Stage::Ours => conflict.ours = Some(tracked),
                        gix::index::entry::Stage::Theirs => conflict.theirs = Some(tracked),
                        gix::index::entry::Stage::Unconflicted => {}
                    }
                    conflict_stages.push((path, conflict));
                }
            }
        }
    }
    conflicts.extend(conflict_stages.into_iter().map(|(_, conflict)| conflict));
    if sparse > 0 {
        notices.push(StateNotice::Sparse { count: sparse });
    }

    let staged = staged_changes(&head_entries, &normal);
    let unstaged_result = unstaged_changes(repo, &work_dir, &normal, options, &mut notices)?;
    let mut examined = normal.len();
    if let Some(cap) = options.max_entries
        && examined > cap
    {
        examined = cap;
        notices.push(StateNotice::Capped { examined });
    }
    Ok(WorkingState {
        staged,
        unstaged: unstaged_result.changes,
        conflicts,
        notices,
        index: IndexSnapshot {
            path: index_path,
            present: true,
            version: Some(match index.version() {
                gix::index::Version::V2 => 2,
                gix::index::Version::V3 => 3,
                gix::index::Version::V4 => 4,
            }),
            entries: index.entries().len(),
            sparse: sparse > 0,
        },
        examined,
        hashed: unstaged_result.hashed,
    })
}

/// One tree, flattened to `(full path, mode, oid)` sorted by path.
///
/// Sorting is not a nicety: git sorts entries *within* a tree, and a subtree named `a` sorts as if it
/// were `a/`, which does not agree with a byte sort of the full paths (`a.txt` before `a/b`?). The index
/// is sorted by full path, so the merge below needs the tree side sorted the same way; flattening and
/// sorting once is cheaper and much easier to reason about than a two-iterator merge over two different
/// orders.
pub(crate) fn flatten_tree(
    repo: &Repository,
    root: Oid,
) -> Result<Vec<(Vec<u8>, u32, Oid)>, Error> {
    let mut out: Vec<(Vec<u8>, u32, Oid)> = Vec::new();
    let mut stack: Vec<(Oid, Vec<u8>)> = vec![(root, Vec::new())];
    while let Some((tree, prefix)) = stack.pop() {
        for entry in raw_tree_entries(repo, tree)? {
            let (name, mode, oid) = entry;
            let full = crate::paths::join(&prefix, &name);
            if crate::repo::TreeEntryKind::from_mode_value(mode) == crate::repo::TreeEntryKind::Tree
            {
                stack.push((oid, full));
            } else {
                out.push((full, mode, oid));
            }
        }
    }
    out.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(out)
}

/// A tree's entries with the name bytes kept whole (a lossy name would break the path comparison below).
pub(crate) fn raw_tree_entries(
    repo: &Repository,
    tree: Oid,
) -> Result<Vec<(Vec<u8>, u32, Oid)>, Error> {
    let id = crate::repo::to_gix(tree, repo.gix().object_hash());
    let object = repo
        .gix()
        .try_find_object(id)
        .map_err(|err| Error::RepositoryUnreadable {
            root: repo.identity().display_name.clone(),
            reason: format!("reading a tree: {err}"),
        })?
        .ok_or_else(|| Error::ObjectUnavailable {
            oid: tree,
            why: repo.why_unavailable(tree),
        })?;
    let tree = object
        .try_into_tree()
        .map_err(|err| Error::RepositoryUnreadable {
            root: repo.identity().display_name.clone(),
            reason: format!("decoding a tree: {err}"),
        })?;
    let mut out = Vec::new();
    for entry in tree.iter() {
        let entry = entry.map_err(|err| Error::RepositoryUnreadable {
            root: repo.identity().display_name.clone(),
            reason: format!("decoding a tree entry: {err}"),
        })?;
        out.push((
            entry.filename().to_vec(),
            u32::from(entry.mode().value()),
            crate::repo::from_gix(entry.object_id().as_ref()),
        ));
    }
    Ok(out)
}

struct UnstagedOutcome {
    changes: Vec<Change>,
    hashed: usize,
}

fn staged_changes(
    head_entries: &[(Vec<u8>, u32, Oid)],
    index_entries: &[(&gix::index::Entry, Vec<u8>)],
) -> Vec<Change> {
    let mut out = Vec::new();
    let mut head_index = 0usize;
    let mut index_index = 0usize;
    while head_index < head_entries.len() && index_index < index_entries.len() {
        let (head_path, head_mode, head_oid) = &head_entries[head_index];
        let (entry, path) = &index_entries[index_index];
        let index_oid = crate::repo::from_gix(entry.id.as_ref());
        let index_mode = entry.mode.bits();
        match head_path.as_slice().cmp(path.as_slice()) {
            std::cmp::Ordering::Equal => {
                if index_oid != *head_oid {
                    out.push(Change {
                        side: Side::Staged,
                        kind: ChangeKind::Modified,
                        path: RepoPath::new(path.clone()),
                        before: Some(TrackedEntry {
                            oid: *head_oid,
                            mode: *head_mode,
                        }),
                        after: Some(TrackedEntry {
                            oid: index_oid,
                            mode: index_mode,
                        }),
                        on_disk: None,
                    });
                } else if index_mode != *head_mode {
                    out.push(Change {
                        side: Side::Staged,
                        kind: ChangeKind::ModeChanged,
                        path: RepoPath::new(path.clone()),
                        before: Some(TrackedEntry {
                            oid: *head_oid,
                            mode: *head_mode,
                        }),
                        after: Some(TrackedEntry {
                            oid: index_oid,
                            mode: index_mode,
                        }),
                        on_disk: None,
                    });
                }
                head_index += 1;
                index_index += 1;
            }
            std::cmp::Ordering::Less => {
                out.push(Change {
                    side: Side::Staged,
                    kind: ChangeKind::Deleted,
                    path: RepoPath::new(head_path.clone()),
                    before: Some(TrackedEntry {
                        oid: *head_oid,
                        mode: *head_mode,
                    }),
                    after: None,
                    on_disk: None,
                });
                head_index += 1;
            }
            std::cmp::Ordering::Greater => {
                out.push(Change {
                    side: Side::Staged,
                    kind: ChangeKind::Added,
                    path: RepoPath::new(path.clone()),
                    before: None,
                    after: Some(TrackedEntry {
                        oid: index_oid,
                        mode: index_mode,
                    }),
                    on_disk: None,
                });
                index_index += 1;
            }
        }
    }
    for (head_path, head_mode, head_oid) in &head_entries[head_index..] {
        out.push(Change {
            side: Side::Staged,
            kind: ChangeKind::Deleted,
            path: RepoPath::new(head_path.clone()),
            before: Some(TrackedEntry {
                oid: *head_oid,
                mode: *head_mode,
            }),
            after: None,
            on_disk: None,
        });
    }
    for (entry, path) in &index_entries[index_index..] {
        out.push(Change {
            side: Side::Staged,
            kind: ChangeKind::Added,
            path: RepoPath::new(path.clone()),
            before: None,
            after: Some(TrackedEntry {
                oid: crate::repo::from_gix(entry.id.as_ref()),
                mode: entry.mode.bits(),
            }),
            on_disk: None,
        });
    }
    out
}

fn unstaged_changes(
    repo: &Repository,
    work_dir: &Path,
    index_entries: &[(&gix::index::Entry, Vec<u8>)],
    options: &ScanOptions,
    notices: &mut Vec<StateNotice>,
) -> Result<UnstagedOutcome, Error> {
    let mut out = Vec::new();
    let mut hashed = 0usize;
    let mut assumed_clean = 0usize;
    let mut oversized = 0usize;
    for (entry, path) in index_entries {
        if let Some(cap) = options.max_entries
            && out.len() + assumed_clean >= cap
        {
            break;
        }
        let mode = entry.mode.bits();
        if mode & 0o170000 == 0o160000 {
            // A gitlink: its contents belong to the submodule's own repository, and `submodules()`
            // reports its state. Recursing here would double-report a changed submodule as a modified
            // *file*, which is a category error.
            continue;
        }
        let native = RepoPath::new(path.clone()).to_native(work_dir);
        let expected_oid = crate::repo::from_gix(entry.id.as_ref());
        let metadata = match std::fs::symlink_metadata(&native) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                out.push(Change {
                    side: Side::Unstaged,
                    kind: ChangeKind::Deleted,
                    path: RepoPath::new(path.clone()),
                    before: Some(TrackedEntry {
                        oid: expected_oid,
                        mode,
                    }),
                    after: None,
                    on_disk: Some(WorktreeEntry {
                        kind: WorktreeKind::File,
                        oid: None,
                        size: 0,
                    }),
                });
                continue;
            }
            Err(err) => {
                notices.push(StateNotice::UnreadablePath {
                    path: RepoPath::new(path.clone()),
                    reason: err.to_string(),
                });
                continue;
            }
        };
        let on_disk_kind = if metadata.is_dir() {
            WorktreeKind::Directory
        } else if metadata.file_type().is_symlink() {
            WorktreeKind::Symlink
        } else {
            // A fifo, a socket or a door is not a blob and not a directory: calling it a file is what makes
            // the next step (the attempt to hash it, which will fail and be reported) the right one.
            WorktreeKind::File
        };
        let index_says_symlink = mode & 0o170000 == 0o120000;
        if on_disk_kind == WorktreeKind::Directory
            || (index_says_symlink && on_disk_kind != WorktreeKind::Symlink)
            || (!index_says_symlink && on_disk_kind == WorktreeKind::Symlink)
        {
            // A type change still has a *new* object as far as git is concerned — a symlink's blob is
            // its target text — and a caller comparing ids needs it, so hash what is hashable and leave
            // directories (nothing to hash) without one.
            let hashable = matches!(on_disk_kind, WorktreeKind::File | WorktreeKind::Symlink);
            let actual = hashable
                .then(|| {
                    hash_worktree_file(&native, on_disk_kind, repo.object_hash_kind())
                        .ok()
                        .inspect(|_| hashed += 1)
                })
                .flatten();
            out.push(Change {
                side: Side::Unstaged,
                kind: ChangeKind::TypeChanged,
                path: RepoPath::new(path.clone()),
                before: Some(TrackedEntry {
                    oid: expected_oid,
                    mode,
                }),
                after: actual.map(|oid| TrackedEntry {
                    oid,
                    mode: if on_disk_kind == WorktreeKind::Symlink {
                        0o120000
                    } else {
                        mode
                    },
                }),
                on_disk: Some(WorktreeEntry {
                    kind: on_disk_kind,
                    oid: actual,
                    size: metadata.len(),
                }),
            });
            continue;
        }
        let size = metadata.len();
        let executable = is_executable(&metadata);
        let index_executable = mode & 0o000111 != 0;
        let stat_matches = size == u64::from(entry.stat.size)
            && u32::try_from(metadata_modified_secs(&metadata)).unwrap_or(u32::MAX)
                == entry.stat.mtime.secs
            && metadata_modified_nanos(&metadata) == entry.stat.mtime.nsecs;
        if options.mode == ScanMode::StatShortcut && stat_matches && executable == index_executable
        {
            // The shortcut's whole point: a clean 100k-file tree costs 100k `stat`s and no hashing.
            assumed_clean += 1;
            continue;
        }
        if size > options.max_file_bytes {
            oversized += 1;
            out.push(Change {
                side: Side::Unstaged,
                kind: ChangeKind::Oversized,
                path: RepoPath::new(path.clone()),
                before: Some(TrackedEntry {
                    oid: expected_oid,
                    mode,
                }),
                after: None,
                on_disk: Some(WorktreeEntry {
                    kind: on_disk_kind,
                    oid: None,
                    size,
                }),
            });
            continue;
        }
        let actual = match hash_worktree_file(&native, on_disk_kind, repo.object_hash_kind()) {
            Ok(oid) => oid,
            Err(reason) => {
                notices.push(StateNotice::UnreadablePath {
                    path: RepoPath::new(path.clone()),
                    reason,
                });
                continue;
            }
        };
        hashed += 1;
        let tracked = TrackedEntry {
            oid: actual,
            mode: if executable { 0o100755 } else { 0o100644 },
        };
        if actual != expected_oid {
            out.push(Change {
                side: Side::Unstaged,
                kind: ChangeKind::Modified,
                path: RepoPath::new(path.clone()),
                before: Some(TrackedEntry {
                    oid: expected_oid,
                    mode,
                }),
                after: Some(tracked),
                on_disk: Some(WorktreeEntry {
                    kind: on_disk_kind,
                    oid: Some(actual),
                    size,
                }),
            });
        } else if executable != index_executable {
            out.push(Change {
                side: Side::Unstaged,
                kind: ChangeKind::ModeChanged,
                path: RepoPath::new(path.clone()),
                before: Some(TrackedEntry {
                    oid: expected_oid,
                    mode,
                }),
                after: Some(tracked),
                on_disk: Some(WorktreeEntry {
                    kind: on_disk_kind,
                    oid: Some(actual),
                    size,
                }),
            });
        }
        // Content identical and mode identical: the stat data was merely stale (a touch, or a re-write
        // with the same bytes). Git refreshes its index here; fathom reads, so it leaves it alone and
        // reports nothing, which is the same answer.
    }
    if assumed_clean > 0 {
        notices.push(StateNotice::StatAssumedClean {
            count: assumed_clean,
        });
    }
    if oversized > 0 {
        notices.push(StateNotice::Oversized { count: oversized });
    }
    Ok(UnstagedOutcome {
        changes: out,
        hashed,
    })
}

/// Hash a working-tree file the way git hashes a blob: `blob <len>\0` then the bytes, streamed.
fn hash_worktree_file(
    path: &Path,
    kind: WorktreeKind,
    hash: gix::hash::Kind,
) -> Result<Oid, String> {
    use std::io::Read as _;
    let kind_name = "blob";
    let mut hasher = gix::hash::hasher(hash);
    let payload: Vec<u8> = if kind == WorktreeKind::Symlink {
        std::fs::read_link(path)
            .map(|target| target.to_string_lossy().into_owned().into_bytes())
            .map_err(|err| format!("reading the symlink: {err}"))?
    } else {
        let mut file = std::fs::File::open(path).map_err(|err| format!("opening: {err}"))?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)
            .map_err(|err| format!("reading: {err}"))?;
        buffer
    };
    hasher.update(format!("{kind_name} {}\0", payload.len()).as_bytes());
    hasher.update(&payload);
    let id = hasher
        .try_finalize()
        .map_err(|err| format!("hashing: {err}"))?;
    Ok(crate::repo::from_gix(id.as_ref()))
}

fn metadata_modified_secs(metadata: &std::fs::Metadata) -> i64 {
    metadata
        .modified()
        .map(|time| {
            time.duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_secs() as i64)
                .unwrap_or(0)
        })
        .unwrap_or(0)
}

fn metadata_modified_nanos(metadata: &std::fs::Metadata) -> u32 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|since| since.subsec_nanos())
        .unwrap_or(0)
}

#[cfg(unix)]
fn is_executable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    metadata.permissions().mode() & 0o000111 != 0
}

#[cfg(not(unix))]
fn is_executable(_metadata: &std::fs::Metadata) -> bool {
    // Windows has no executable bit; git's `core.fileMode` is false there by default, and an index
    // written on Windows does not record one either, so "never executable" agrees with the index.
    false
}

/// The submodules declared in `.gitmodules`, with the state of each checkout.
pub(crate) fn submodules(repo: &Repository) -> Result<Vec<Submodule>, Error> {
    let mut declared: Vec<(String, Vec<u8>, Option<String>)> = Vec::new();
    let modules_file = repo.work_dir().map(|work_dir| work_dir.join(".gitmodules"));
    let contents = match &modules_file {
        Some(path) => std::fs::read(path).ok(),
        None => None,
    };
    if let Some(contents) = contents {
        let Ok(file) = gix::config::File::try_from(gix::bstr::BStr::new(&contents)) else {
            // A `.gitmodules` that does not parse is git's problem to fix; report no submodules rather
            // than failing a working-tree read over it, and let `fathom doctor` be the tool that says
            // *why* the file is bad.
            return Ok(Vec::new());
        };
        for section in file.sections_by_name("submodule").into_iter().flatten() {
            let Some(name) = section.header().subsection_name() else {
                continue;
            };
            let name = crate::paths::to_str_lossy(name).into_owned();
            let path = section
                .value("path")
                .map(|value| value.to_vec())
                .unwrap_or_default();
            let url = section
                .value("url")
                .map(|value| crate::paths::to_str_lossy(value.as_ref()).into_owned());
            declared.push((name, path, url));
        }
    }
    if declared.is_empty() {
        return Ok(Vec::new());
    }
    // The gitlink each submodule *should* be at, straight out of HEAD's tree.
    let recorded: Vec<(Vec<u8>, Oid)> = match repo.head()?.oid() {
        Some(commit) => repo
            .commit(commit)?
            .map(|header| {
                flatten_tree(repo, header.tree).map(|entries| {
                    entries
                        .into_iter()
                        .filter(|(_, mode, _)| {
                            crate::repo::TreeEntryKind::from_mode_value(*mode)
                                == crate::repo::TreeEntryKind::Submodule
                        })
                        .map(|(path, _, oid)| (path, oid))
                        .collect()
                })
            })
            .transpose()?
            .unwrap_or_default(),
        None => Vec::new(),
    };
    let mut out = Vec::new();
    for (name, path, url) in declared {
        let active = repo
            .config_bool(&format!("submodule.{name}.active"))
            .unwrap_or(false);
        let recorded_oid = recorded
            .iter()
            .find(|(recorded_path, _)| *recorded_path == path)
            .map(|(_, oid)| *oid);
        let state = submodule_state(repo, &name, &path, recorded_oid);
        out.push(Submodule {
            name,
            path: RepoPath::new(path),
            url,
            active,
            recorded: recorded_oid,
            state,
        });
    }
    Ok(out)
}

fn submodule_state(
    repo: &Repository,
    name: &str,
    path: &[u8],
    recorded: Option<Oid>,
) -> SubmoduleState {
    let checkout = match repo.work_dir() {
        Some(work_dir) => RepoPath::new(path.to_vec()).to_native(work_dir),
        None => return SubmoduleState::Uninitialized,
    };
    let git_pointer = checkout.join(".git");
    if !checkout.is_dir() {
        return SubmoduleState::Uninitialized;
    }
    if !git_pointer.exists() {
        return if std::fs::read_dir(&checkout).is_ok_and(|mut entries| entries.next().is_none()) {
            SubmoduleState::Empty
        } else {
            SubmoduleState::Uninitialized
        };
    }
    let options = crate::repo::DiscoverOptions::default();
    match Repository::open_at(&checkout, &options) {
        Ok(sub) => match sub.head().ok().and_then(|head| head.oid()) {
            Some(actual) => match recorded {
                Some(recorded) if recorded == actual => SubmoduleState::AtRecorded { at: actual },
                Some(recorded) => SubmoduleState::AtDifference { recorded, actual },
                None => SubmoduleState::AtRecorded { at: actual },
            },
            None => SubmoduleState::Empty,
        },
        Err(err) => SubmoduleState::Unreadable {
            reason: format!("submodule {name}: {err}"),
        },
    }
}

/// The worktrees of this repository, main first — `git worktree list`.
pub(crate) fn linked_worktrees(repo: &Repository) -> Result<Vec<LinkedWorktree>, Error> {
    let mut out = Vec::new();
    if let Some(work_dir) = repo.work_dir() {
        out.push(LinkedWorktree {
            name: work_dir
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| ".".to_owned()),
            path: work_dir.to_path_buf(),
            head: Some(repo.head()?),
            is_main: true,
            missing: !work_dir.is_dir(),
        });
    }
    let worktrees_dir = repo.common_dir().join("worktrees");
    let Ok(entries) = std::fs::read_dir(&worktrees_dir) else {
        return Ok(out);
    };
    let options = crate::repo::DiscoverOptions::default();
    for entry in entries.flatten() {
        let admin = entry.path();
        if !admin.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        // The `gitdir` file names the worktree's `.git` pointer file; its parent is the worktree.
        let pointer = std::fs::read(admin.join("gitdir")).ok();
        let path = pointer
            .and_then(|bytes| {
                let text = String::from_utf8_lossy(&bytes);
                PathBuf::from(text.trim_end())
                    .parent()
                    .map(Path::to_path_buf)
            })
            .unwrap_or_else(|| PathBuf::from(&name));
        let head = if path.is_dir() {
            Repository::open_at(&path, &options)
                .ok()
                .and_then(|worktree| worktree.head().ok())
        } else {
            // The directory is gone (prunable). `git worktree list` still shows it, so we do, and say
            // what it points at if the admin `HEAD` still answers.
            read_admin_head(&admin).map(|name| match name {
                AdminHead::Branch(name) => Head::Unborn { name },
                AdminHead::Detached(oid) => Head::Detached { oid },
            })
        };
        out.push(LinkedWorktree {
            name,
            path: path.clone(),
            head,
            is_main: false,
            missing: !path.is_dir(),
        });
    }
    out.sort_by(|left, right| {
        right
            .is_main
            .cmp(&left.is_main)
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(out)
}

enum AdminHead {
    Branch(String),
    Detached(Oid),
}

fn read_admin_head(admin: &Path) -> Option<AdminHead> {
    let contents = std::fs::read(admin.join("HEAD")).ok()?;
    let text = String::from_utf8_lossy(&contents).trim().to_owned();
    if let Some(name) = text.strip_prefix("ref: ") {
        return Some(AdminHead::Branch(
            name.strip_prefix("refs/heads/").unwrap_or(name).to_owned(),
        ));
    }
    text.parse::<Oid>().ok().map(AdminHead::Detached)
}
