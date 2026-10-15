//! `observe.rs` — answering "did anything a reviewer cares about just happen?" cheaply, and then naming it.
//!
//! Two passes, in that order, because the two questions have different costs and the cheap one has to be
//! asked on every tick of a watcher that is supposed to be invisible next to a running agent:
//!
//! 1. **the fingerprint** ([`Fingerprint::read`]) is a `stat` of a dozen paths plus a walk of `refs/`. It
//!    reads no file *content*, opens no pack and inflates no object: a quiet repository costs a handful of
//!    syscalls per tick, which is what makes the "idle CPU under 1 percent" acceptance criterion a
//!    consequence of the design rather than an aspiration. The fold is over `(identity, size, mtime)` —
//!    and the identity is the *inode*, because git writes every ref, every `HEAD` move and its own index by
//!    `lock → write → rename`, which produces a **new inode** even when the filesystem's mtime granularity
//!    is a whole second. A watcher that compared mtimes alone would silently miss a commit made in the same
//!    second as the last one; that bug is the reason this file exists in a filesystem with 1 s timestamps.
//! 2. **the snapshot** ([`Snapshot::read`]) names what moved: `HEAD` and every ref's target, read through
//!    [`Repository::refs`] — which the file's own doc calls a snapshot for exactly this caller ("a
//!    long-lived fathom watching a repository re-reads it"). It costs a ref-store walk, so it runs only
//!    when the fingerprint says something moved, or when the loose-ref walk had to stop early.
//!
//! The diff of two snapshots ([`diff`]) is what the rest of the crate consumes: names, not paths. The
//! timeline shows commits, and a commit reaches the timeline because a ref moved to it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::repo::{Head, Ref, RefFilter, RefTarget, Repository};

/// The visible identity of one file: enough to tell "rewritten" from "untouched" without reading it.
///
/// On unix `identity` is `st_ino`; anywhere else it is `0` and the comparison falls back to size and mtime,
/// which is weaker — so a build on a platform without inode numbers is a build where the watcher can be
/// fooled by a same-second rewrite. That asymmetry is stated rather than hidden because it is a real
/// difference in what the guarantee is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stat {
    /// Does the path exist?
    pub present: bool,
    /// `st_ino` on unix, `0` elsewhere.
    pub identity: u64,
    /// Byte size, or `0` when absent.
    pub size: u64,
    /// `st_mtime_ns`, or `0` when absent. Nanoseconds because ext4 and APFS both store them, and a
    /// granularity the filesystem has is a granularity we are allowed to use.
    pub mtime_ns: i128,
}

impl Stat {
    /// The absent stat, which compares unequal to every real one — so a file disappearing is a change.
    pub const ABSENT: Stat = Stat {
        present: false,
        identity: 0,
        size: 0,
        mtime_ns: 0,
    };

    /// `stat` one path. A failed `stat` is `ABSENT` rather than an error: for a watcher, "cannot see it"
    /// and "not there" produce the same action (look again, and let the caller that actually needs the
    /// content report the failure).
    #[must_use]
    pub fn of(path: &Path) -> Stat {
        match std::fs::metadata(path) {
            Ok(meta) => Stat {
                present: true,
                identity: inode_of(&meta),
                size: meta.len(),
                mtime_ns: mtime_ns(&meta),
            },
            Err(_) => Stat::ABSENT,
        }
    }

    /// Fold into a running hash.
    fn mix(&self, hash: &mut Fnv) {
        hash.add(self.present as u64);
        hash.add(self.identity);
        hash.add(self.size);
        hash.add(self.mtime_ns as u64);
    }
}

/// The identity of a *directory*, which is how a repository being swapped out from under the watcher is
/// caught. `st_ino` (plus the device, on unix) changes when the path is deleted and recreated, when it is
/// replaced by a symlink to another clone, and when a container restarts with a different bind mount —
/// the three cases this ticket has to survive without a crash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirId {
    /// `st_dev`, or `0` where the platform does not say.
    pub dev: u64,
    /// `st_ino`, or `0` where the platform does not say.
    pub ino: u64,
    /// Was the directory there at all?
    pub present: bool,
}

impl DirId {
    /// `stat` a directory.
    #[must_use]
    pub fn of(path: &Path) -> DirId {
        let Ok(meta) = std::fs::metadata(path) else {
            return DirId {
                dev: 0,
                ino: 0,
                present: false,
            };
        };
        DirId {
            dev: dev_of(&meta),
            ino: meta_identity(&meta),
            present: true,
        }
    }

    /// Did the directory become a *different directory*, or stop being one at all?
    ///
    /// A presence flip counts: `.git` vanishing is the first half of "the repository was replaced" as often
    /// as not (`rm -rf` then `git clone`, or a container restarting onto a different mount), and the watcher
    /// has to come back on the other side of it rather than stay wedged comparing an absent stat forever.
    /// On platforms without inode numbers, two present directories compare equal and this guard is inert —
    /// the honest answer, and the callers that would have acted on it fail at their next read instead.
    #[must_use]
    pub fn replaced(&self, other: &DirId) -> bool {
        if self.present != other.present {
            return true;
        }
        (self.ino != 0 || self.dev != 0) && (other.ino != 0 || other.dev != 0) && self != other
    }
}

/// The cheap answer to "has anything moved".
///
/// `PartialEq` is the whole contract: unequal means *look properly*, equal means *nothing to do*. It is
/// deliberately allowed to have false positives (a `gc` that rewrote a pack touches nothing a reviewer
/// will see, but moves `objects/pack`'s listing) and is designed to have no false negatives for refs and
/// `HEAD`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    /// The git dir's own identity.
    pub git_dir: DirId,
    /// The common dir's identity — for a linked worktree, where the refs actually live.
    pub common_dir: DirId,
    /// `.git/HEAD`.
    pub head: Stat,
    /// `.git/packed-refs`.
    pub packed_refs: Stat,
    /// The worktree index: it moves when the human or the agent stages something.
    pub index: Stat,
    /// `.git/logs/HEAD`, which git writes for a ref move even when the ref itself is packed.
    pub head_reflog: Stat,
    /// Fold over every loose ref file reached by the walk.
    pub loose_refs: u64,
    /// How many loose ref files were folded.
    pub loose_refs_walked: usize,
    /// Did the walk hit its cap? Then `loose_refs` is a partial fold and the watcher must take the full
    /// snapshot on every poll instead of trusting it — stated, not assumed.
    pub loose_refs_capped: bool,
    /// Fold over the directories that matter (`refs/**`, `objects/pack`, `logs`).
    pub directories: u64,
    /// Fold over the guard markers: rebase dirs, operation heads, lock files.
    pub markers: u64,
}

impl Fingerprint {
    /// Read it. Nothing here can fail: every sub-read degrades to `Stat::ABSENT`.
    #[must_use]
    pub fn read(repo: &Repository, loose_ref_cap: usize) -> Fingerprint {
        let git_dir = repo.git_dir();
        let common_dir = repo.common_dir();
        let index = repo.index_path();

        let mut hash = Fnv::default();
        for (which, dir) in [(0u64, git_dir), (1u64, common_dir)] {
            for name in [
                "refs",
                "refs/heads",
                "refs/tags",
                "refs/remotes",
                "logs",
                "objects",
                "objects/pack",
            ] {
                let path = dir.join(name);
                let stat = Stat::of(&path);
                if !stat.present {
                    continue;
                }
                hash.add(which);
                hash.mix_path(name.as_bytes());
                stat.mix(&mut hash);
            }
        }

        let mut markers = Fnv::default();
        for path in crate::watch::guard::marker_paths(git_dir, common_dir) {
            Stat::of(&path).mix(&mut markers);
            // A directory's entry count moves as a rebase or a sequencer works through its list, and a
            // `read_dir` is a syscall, not a read of contents.
            if let Ok(entries) = std::fs::read_dir(&path) {
                markers.add(entries.count().min(u16::MAX as usize) as u64 + 1);
            }
        }

        let (loose_refs, walked, capped) = walk_refs(common_dir, loose_ref_cap);
        Fingerprint {
            git_dir: DirId::of(git_dir),
            common_dir: DirId::of(common_dir),
            head: Stat::of(&git_dir.join("HEAD")),
            packed_refs: Stat::of(&common_dir.join("packed-refs")),
            index: Stat::of(&index),
            head_reflog: Stat::of(&git_dir.join("logs").join("HEAD")),
            loose_refs,
            loose_refs_walked: walked,
            loose_refs_capped: capped,
            directories: hash.finish(),
            markers: markers.finish(),
        }
    }

    /// Which part moved, for the log line and for the "was it a ref?" decision.
    #[must_use]
    pub fn what_moved(&self, before: &Fingerprint) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.git_dir != before.git_dir || self.common_dir != before.common_dir {
            out.push("git-dir identity");
        }
        if self.head != before.head {
            out.push("HEAD");
        }
        if self.packed_refs != before.packed_refs {
            out.push("packed-refs");
        }
        if self.index != before.index {
            out.push("index");
        }
        if self.loose_refs != before.loose_refs || self.head_reflog != before.head_reflog {
            out.push("refs");
        }
        if self.markers != before.markers {
            out.push("git's own markers");
        }
        if self.directories != before.directories {
            out.push("directory listing");
        }
        out
    }
}

/// Fold helper: FNV-1a over the numbers that matter. Not a security hash — a change detector that must not
/// allocate.
#[derive(Default, Debug)]
struct Fnv(u64);

impl Fnv {
    fn add(&mut self, value: u64) {
        self.0 = (self.0 ^ value).wrapping_mul(0x0000_0100_0000_01b3);
    }

    fn mix_path(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.add(u64::from(*byte));
        }
        self.add(0xff);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

/// Fold every loose ref under `refs/`, capped so a 50 000-tag repository cannot turn a poll tick into a
/// 50 000-syscall event. Hitting the cap is reported, and the watcher then snapshots the ref store instead
/// of trusting a partial fold.
fn walk_refs(root: &Path, cap: usize) -> (u64, usize, bool) {
    let mut hash = Fnv::default();
    let mut walked = 0_usize;
    let mut capped = false;
    let base = root.join("refs");
    if !base.is_dir() {
        return (hash.finish(), walked, capped);
    }
    let mut stack = vec![base];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if walked >= cap {
                capped = true;
                break;
            }
            let path = entry.path();
            let is_dir = entry
                .file_type()
                .map(|file_type| file_type.is_dir())
                .unwrap_or(false);
            if is_dir {
                stack.push(path);
                continue;
            }
            // Fold the name relative to `refs/` so a rename is a change and a moved-with-its-parent tree
            // is not (the base is not in the hash).
            if let Ok(relative) = path.strip_prefix(root) {
                hash.mix_path(relative.to_string_lossy().as_bytes());
            }
            Stat::of(&path).mix(&mut hash);
            walked += 1;
        }
        if capped {
            break;
        }
    }
    (hash.finish(), walked, capped)
}

/// What the ref store said at one instant: `HEAD` plus every ref's target, in the shape a diff needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// `HEAD`, rendered: `main@a1b2c3d`, `detached@a1b2c3d`, `main (no commits yet)`.
    pub head: String,
    /// The commit `HEAD` resolves to, `None` while the repository has no commits.
    pub head_oid: Option<String>,
    /// Every ref, by full name. The value is the target's oid, or `sym:<name>` for a symbolic ref.
    pub refs: BTreeMap<String, String>,
}

impl Snapshot {
    /// Read the ref store. `Err` only when the store cannot be opened at all, which for a watcher means
    /// "the repository moved or went away" and is the trigger for a reopen.
    pub fn read(repo: &Repository) -> Result<Snapshot, crate::error::Error> {
        let head = repo.head()?;
        let refs = repo.refs(RefFilter::All)?;
        Ok(Snapshot {
            head: head.summary(),
            head_oid: head.oid().map(|oid| oid.hex()),
            refs: refs
                .iter()
                .map(|reference| (reference.name.clone(), target_of(reference)))
                .collect(),
        })
    }

    /// How many refs it saw — the number behind "N refs" in a status line.
    #[must_use]
    pub fn ref_count(&self) -> usize {
        self.refs.len()
    }
}

fn target_of(reference: &Ref) -> String {
    match &reference.target {
        RefTarget::Direct { oid } => oid.hex(),
        RefTarget::Symbolic { to } => format!("sym:{to}"),
    }
}

/// One ref or `HEAD` that is not where it was.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Change {
    /// `HEAD`, or the full ref name (`refs/heads/main`).
    pub name: String,
    /// What happened to it.
    pub kind: ChangeKind,
    /// Where it was. `None` when it did not exist.
    pub from: Option<String>,
    /// Where it is. `None` when it is gone.
    pub to: Option<String>,
    /// What kind of name it is (`branch`, `tag`, `remote`, `stash`, …) — `HEAD` sorts as `head`.
    pub kind_of_ref: &'static str,
}

impl Change {
    /// Is this the one change that puts a commit in the timeline?
    #[must_use]
    pub fn is_head(&self) -> bool {
        self.name == "HEAD"
    }

    /// Did a commit-shaped ref move, as opposed to a tag annotation or a stash entry?
    #[must_use]
    pub fn is_branch(&self) -> bool {
        self.kind_of_ref == "branch"
    }

    /// The line a human reads.
    #[must_use]
    pub fn summary(&self) -> String {
        let short = self
            .name
            .strip_prefix("refs/heads/")
            .or_else(|| self.name.strip_prefix("refs/remotes/"))
            .or_else(|| self.name.strip_prefix("refs/tags/"))
            .unwrap_or(&self.name);
        let (from, to) = (
            self.from.as_deref().map(short_target).unwrap_or_default(),
            self.to.as_deref().map(short_target).unwrap_or_default(),
        );
        match self.kind {
            ChangeKind::Added => format!("{short} (new) → {to}"),
            ChangeKind::Removed => format!("{short} deleted (was {from})"),
            ChangeKind::Moved => format!("{short} {from} → {to}"),
        }
    }
}

fn short_target(target: &str) -> String {
    if let Some(symbolic) = target.strip_prefix("sym:") {
        return symbolic.to_owned();
    }
    target.chars().take(10).collect()
}

/// What happened to a ref.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    /// It did not exist before.
    Added,
    /// It does not exist now.
    Removed,
    /// Its target moved.
    Moved,
}

/// The diff of two snapshots: everything that moved, `HEAD` first.
///
/// `HEAD` is included as a change in its own right, because it is the one ref the timeline follows — a
/// `git checkout` of a different branch moves `HEAD` and every already-indexed commit's *visibility*, and
/// a detached `HEAD` commit is a state a reviewer needs to see happen.
#[must_use]
pub fn diff(before: &Snapshot, after: &Snapshot) -> Vec<Change> {
    let mut out = Vec::new();
    if before.head_oid != after.head_oid || before.head != after.head {
        out.push(Change {
            name: "HEAD".to_owned(),
            kind: match (&before.head_oid, &after.head_oid) {
                (None, Some(_)) => ChangeKind::Added,
                (Some(_), None) => ChangeKind::Removed,
                _ => ChangeKind::Moved,
            },
            from: before.head_oid.clone(),
            to: after.head_oid.clone(),
            kind_of_ref: "head",
        });
    }
    for (name, target) in &after.refs {
        if *name == "HEAD" {
            // `HEAD` is reported above from `Head`'s own rendering; a second row for the same move would
            // be the same fact twice, with a different spelling of the target.
            continue;
        }
        match before.refs.get(name) {
            None => out.push(Change {
                name: name.clone(),
                kind: ChangeKind::Added,
                from: None,
                to: Some(target.clone()),
                kind_of_ref: classify_ref(name),
            }),
            Some(old) if old != target => out.push(Change {
                name: name.clone(),
                kind: ChangeKind::Moved,
                from: Some(old.clone()),
                to: Some(target.clone()),
                kind_of_ref: classify_ref(name),
            }),
            Some(_) => {}
        }
    }
    for (name, target) in &before.refs {
        if *name == "HEAD" || after.refs.contains_key(name) {
            continue;
        }
        out.push(Change {
            name: name.clone(),
            kind: ChangeKind::Removed,
            from: Some(target.clone()),
            to: None,
            kind_of_ref: classify_ref(name),
        });
    }
    out
}

/// The namespace is the type — git's rule, reused rather than re-derived.
fn classify_ref(name: &str) -> &'static str {
    if name.starts_with("refs/heads/") {
        "branch"
    } else if name.starts_with("refs/tags/") {
        "tag"
    } else if name.starts_with("refs/remotes/") {
        "remote"
    } else if name.starts_with("refs/notes/") {
        "note"
    } else if name == "refs/stash" {
        "stash"
    } else {
        "other"
    }
}

/// The state a change-driven refresh carries: what changed, and whether the worktree should be re-read.
#[derive(Debug, Clone, Default)]
pub struct Observation {
    /// The named moves, `HEAD` first.
    pub changes: Vec<Change>,
    /// `HEAD` as it reads now.
    pub head: String,
    /// Did the index move? Uncommitted work is only re-indexed when the watcher was told to look at it.
    pub index_touched: bool,
    /// Was the git directory a different directory than last time?
    pub replaced: bool,
    /// Which cheap signals fired, for the log and for the tests that assert *why* a refresh happened.
    pub signals: Vec<&'static str>,
    /// Did the loose-ref walk have to stop early? Then this observation is only as good as the snapshot,
    /// which it is — but the fact is worth surfacing, because it changes what a poll tick costs.
    pub coarse: bool,
}

impl Observation {
    /// Is there anything worth a refresh?
    #[must_use]
    pub fn has_changes(&self) -> bool {
        !self.changes.is_empty()
    }

    /// Every cheap signal as one line.
    #[must_use]
    pub fn signal_line(&self) -> String {
        if self.signals.is_empty() {
            "nothing".to_owned()
        } else {
            self.signals.join(", ")
        }
    }
}

/// `HEAD` as a string, for a status line when no change happened.
#[must_use]
pub fn head_summary(repo: &Repository) -> String {
    match repo.head() {
        Ok(head) => head.summary(),
        Err(_) => "unreadable HEAD".to_owned(),
    }
}

/// The platform half of [`Stat`] and [`DirId`]: unix has inode numbers, everything else does not.
#[cfg(unix)]
fn inode_of(meta: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.ino()
}

#[cfg(not(unix))]
fn inode_of(_meta: &std::fs::Metadata) -> u64 {
    0
}

#[cfg(unix)]
fn meta_identity(meta: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.ino()
}

#[cfg(not(unix))]
fn meta_identity(_meta: &std::fs::Metadata) -> u64 {
    0
}

#[cfg(unix)]
fn dev_of(meta: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.dev()
}

#[cfg(not(unix))]
fn dev_of(_meta: &std::fs::Metadata) -> u64 {
    0
}

#[cfg(unix)]
fn mtime_ns(meta: &std::fs::Metadata) -> i128 {
    use std::os::unix::fs::MetadataExt;
    i128::from(meta.mtime()) * 1_000_000_000 + i128::from(meta.mtime_nsec())
}

#[cfg(not(unix))]
fn mtime_ns(meta: &std::fs::Metadata) -> i128 {
    meta.modified()
        .ok()
        .and_then(|when| when.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|since| i128::try_from(since.as_nanos()).unwrap_or(0))
        .unwrap_or(0)
}

/// Where a linked worktree's refs and its `HEAD` live, for the paths a watcher must subscribe.
///
/// Exposed because the subscription list and the fingerprint have to agree, and disagreeing about that is
/// how a watcher silently watches the wrong worktree.
#[must_use]
pub fn watch_roots(repo: &Repository) -> Vec<PathBuf> {
    let mut out = vec![repo.git_dir().to_path_buf()];
    let common = repo.common_dir();
    if common != repo.git_dir() {
        out.push(common.to_path_buf());
    }
    // A linked worktree keeps its own `HEAD` under `common/worktrees/<name>`, and git's ref-store
    // bookkeeping for it is spread between the two, so subscribe to the worktrees directory too.
    let worktrees = common.join("worktrees");
    if worktrees.is_dir() {
        out.push(worktrees);
    }
    out
}

/// `HEAD`'s branch name, when there is one — for the ambient status line a later ticket renders.
#[must_use]
pub fn head_branch(repo: &Repository) -> Option<String> {
    match repo.head().ok()? {
        Head::Branch { name, .. } | Head::Unborn { name } => Some(name),
        Head::Detached { .. } => None,
    }
}
