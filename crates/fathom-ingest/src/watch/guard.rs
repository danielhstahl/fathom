//! `guard.rs` — what git is busy with, read from the filesystem and never from a subprocess.
//!
//! The watcher has to answer two questions before it indexes, and both of them are about a *different*
//! process being mid-something:
//!
//! * **is a human in the middle of a multi-step operation?** (`rebase -i` stopped for an edit, a merge
//!   with conflicts, a `cherry-pick` sequence). Indexing between every pick of a 40-commit rebase is 40
//!   index runs for a history that is not finished being written, and an interactive rebase can sit on a
//!   conflict for an hour. So the answer is not "wait until it finishes" — that is a stuck spinner for an
//!   hour — it is "index less often, and say why".
//! * **is git rearranging the store under us?** (`gc`, `repack`, a ref transaction holding a lock). Here
//!   the objects themselves are moving, so a read can fail mid-scan; the answer is to wait, bounded.
//!
//! Neither question can be asked of `git` — fathom never runs it (ADR-001 §8.2, asserted by
//! `tests/no_git_subprocess.rs`) — so every signal here is a `stat` of a path git itself maintains. The
//! list of paths is git's own on-disk contract, and each one is commented with the file that writes it.

use std::path::{Path, PathBuf};

/// A multi-step operation the user is inside of.
///
/// `PartialEq` so a caller can tell "still the same rebase" from "a new one started", which is the
/// difference between a slow watcher and one that keeps restarting its settle window forever.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    /// Nothing is in progress.
    None,
    /// `git rebase` is running. `step`/`total` come from git's own counters, and `paused` means it is
    /// waiting for a human (a conflict, or an `edit` stop) rather than making progress.
    Rebase {
        /// `rebase-merge/interactive` exists, so a person is driving it.
        interactive: bool,
        /// `rebase-merge/msgnum` or `rebase-apply/next`.
        step: Option<u32>,
        /// `rebase-merge/end` or `rebase-apply/last`.
        total: Option<u32>,
        /// A conflict or stop means there is a `MERGE_HEAD` or a dirty index alongside the rebase dir.
        paused: bool,
    },
    /// `git merge` stopped on conflicts (`MERGE_HEAD`).
    Merge,
    /// `git cherry-pick` stopped, or a `sequencer/` list is part-way through.
    CherryPick {
        /// How many entries the sequencer still has to apply.
        remaining: usize,
    },
    /// `git revert` stopped (`REVERT_HEAD`).
    Revert,
    /// `git bisect` has been started and not reset (`BISECT_LOG`).
    Bisect,
    /// `git am` is applying a mailbox (`rebase-apply` without a rebase, or `applied`/`previous`).
    ApplyMailbox,
}

impl Operation {
    /// Is anything in progress?
    #[must_use]
    pub fn in_progress(&self) -> bool {
        !matches!(self, Operation::None)
    }

    /// One line for the state machine's notice column, in the shape `fathom watch` prints.
    #[must_use]
    pub fn summary(&self) -> Option<String> {
        match self {
            Operation::None => None,
            Operation::Rebase {
                interactive,
                step,
                total,
                paused,
            } => {
                let mut text = format!(
                    "rebase {}in progress{}",
                    if *interactive { "(interactive) " } else { "" },
                    match (step, total) {
                        (Some(step), Some(total)) => format!(", commit {step} of {total}"),
                        (Some(step), None) => format!(", commit {step}"),
                        (None, _) => String::new(),
                    }
                );
                if *paused {
                    text.push_str(" — stopped for you; showing the commits it has made so far");
                }
                Some(text)
            }
            Operation::Merge => Some("merge in progress: the index has conflicts".to_owned()),
            Operation::CherryPick { remaining } => Some(format!(
                "cherry-pick sequence in progress: {remaining} commit(s) still to apply"
            )),
            Operation::Revert => Some("revert in progress".to_owned()),
            Operation::Bisect => {
                Some("bisect in progress: HEAD is moving underneath us".to_owned())
            }
            Operation::ApplyMailbox => Some("`git am` is applying a patch series".to_owned()),
        }
    }
}

/// git's own bookkeeping that says "somebody is writing".
///
/// Every field is a `stat`, not a read: the watcher pays a syscall, not a decompression, and this runs on
/// every tick including the quiet ones.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Maintenance {
    /// `*.lock` files that exist right now, by name (`index.lock`, `packed-refs.lock`, `refs.lock`,
    /// `HEAD.lock`). A lock is git mid-write: the state on the other side of it is not yet consistent.
    pub locks: Vec<String>,
    /// `.git/gc.lock`, which `git gc` holds for the whole of its run (`builtin/gc.c`).
    pub gc: bool,
    /// `objects/pack/*.pack` files being written (`*.pack.tmp`, `tmp_pack_*`), i.e. a repack in flight.
    pub packs_in_flight: usize,
    /// `.git/objects/incoming-*`, which a `git fetch` stages there before renaming it into place.
    pub incoming: usize,
}

impl Maintenance {
    /// Should the watcher hold off? A ref lock or a gc lock means the answer to "what are the refs" is
    /// about to change; a pack being written means a commit the watcher just saw may be in a pack that is
    /// not closed yet.
    #[must_use]
    pub fn busy(&self) -> bool {
        self.gc || !self.locks.is_empty() || self.packs_in_flight > 0 || self.incoming > 0
    }

    /// One line for the notice column, or `None` when nothing is going on.
    #[must_use]
    pub fn summary(&self) -> Option<String> {
        if !self.busy() {
            return None;
        }
        let mut parts: Vec<String> = Vec::new();
        if self.gc {
            parts.push("git gc is running (gc.lock)".to_owned());
        }
        if self.packs_in_flight > 0 {
            parts.push(format!("{} pack(s) being written", self.packs_in_flight));
        }
        if self.incoming > 0 {
            parts.push(format!(
                "{} incoming object directory(ies) from a fetch",
                self.incoming
            ));
        }
        if !self.locks.is_empty() {
            parts.push(format!("locked: {}", self.locks.join(", ")));
        }
        Some(parts.join("; "))
    }
}

/// What git is in the middle of, looking in both the repository's own git dir and the shared one (a linked
/// worktree keeps its refs and most operation state in the common dir, but its `HEAD` and index in its
/// own).
#[must_use]
pub fn operation(git_dir: &Path, common_dir: &Path) -> Operation {
    let dir = |name: &str| {
        if git_dir.join(name).is_dir() {
            Some(git_dir)
        } else if common_dir != git_dir && common_dir.join(name).is_dir() {
            Some(common_dir)
        } else {
            None
        }
    };
    let marker = |name: &str| git_dir.join(name).exists() || common_dir.join(name).exists();

    if let Some(root) = dir("rebase-merge") {
        let interactive = root.join("rebase-merge/interactive").is_file();
        let step = read_small_number(&root.join("rebase-merge/msgnum"));
        let total = read_small_number(&root.join("rebase-merge/end"));
        // A conflicted pick leaves `MERGE_HEAD` behind (rebase applies commits with the merge machinery),
        // which is the only filesystem-visible difference between "working" and "waiting for a human".
        // An `edit` stop is not: it looks exactly like a rebase mid-stride, so it is treated as one, and
        // the indexing rate limit below is what keeps it cheap.
        return Operation::Rebase {
            interactive,
            step,
            total,
            paused: marker("MERGE_HEAD"),
        };
    }
    if let Some(root) = dir("rebase-apply") {
        let step = read_small_number(&root.join("rebase-apply/next"));
        let total = read_small_number(&root.join("rebase-apply/last"));
        // `git am` writes the same directory shape; `applied`/`previous` are its own bookkeeping and are
        // never written by `git rebase`, which is the only way to tell the two apart from the filesystem.
        if root.join("rebase-apply/applied").is_file()
            || root.join("rebase-apply/previous").is_file()
        {
            return Operation::ApplyMailbox;
        }
        return Operation::Rebase {
            interactive: false,
            step,
            total,
            paused: marker("MERGE_HEAD"),
        };
    }
    if marker("MERGE_HEAD") {
        return Operation::Merge;
    }
    if marker("CHERRY_PICK_HEAD") {
        return Operation::CherryPick {
            remaining: remaining_todo(&common_dir.join("sequencer/todo"))
                .max(remaining_todo(&git_dir.join("sequencer/todo"))),
        };
    }
    if marker("REVERT_HEAD") {
        return Operation::Revert;
    }
    if dir("sequencer").is_some() {
        return Operation::CherryPick {
            remaining: remaining_todo(&common_dir.join("sequencer/todo"))
                .max(remaining_todo(&git_dir.join("sequencer/todo"))),
        };
    }
    if marker("BISECT_LOG") {
        return Operation::Bisect;
    }
    Operation::None
}

/// The lock and in-flight-write picture.
#[must_use]
pub fn maintenance(git_dir: &Path, common_dir: &Path, index_path: &Path) -> Maintenance {
    let mut out = Maintenance::default();
    // A lock beside the index is not always beside the git dir: a linked worktree's index lives under
    // `common/worktrees/<name>/`, so the lock is looked for where the index actually is.
    for dir in [git_dir, common_dir] {
        for name in [
            "index.lock",
            "packed-refs.lock",
            "refs.lock",
            "HEAD.lock",
            "config.lock",
        ] {
            if dir.join(name).exists() {
                push_unique(&mut out.locks, name);
            }
        }
    }
    if let Some(parent) = index_path.parent()
        && parent.join("index.lock").exists()
    {
        push_unique(&mut out.locks, "index.lock");
    }
    if git_dir.join("gc.lock").exists() || common_dir.join("gc.lock").exists() {
        out.gc = true;
    }
    let pack_dir = git_dir.join("objects").join("pack");
    if pack_dir.is_dir()
        && let Ok(entries) = std::fs::read_dir(&pack_dir)
    {
        for entry in entries.flatten().take(4096) {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            // git writes a repack as `*.pack.tmp` (or `tmp_pack_XXXXXX` while renaming), and a
            // `*.keep` alongside a pack means somebody just wrote it and may still be writing.
            if name.ends_with(".pack.tmp") || name.starts_with("tmp_pack") {
                out.packs_in_flight += 1;
            }
        }
    }
    let objects = git_dir.join("objects");
    if objects.is_dir()
        && let Ok(entries) = std::fs::read_dir(&objects)
    {
        for entry in entries.flatten().take(256) {
            if entry.file_name().to_string_lossy().starts_with("incoming-") {
                out.incoming += 1;
            }
        }
    }
    out
}

/// The paths worth a `stat` on every tick, as far as the guards are concerned. `observe` folds these into
/// its fingerprint so a rebase starting is *detected* even though it moves no ref.
#[must_use]
pub fn marker_paths(git_dir: &Path, common_dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for dir in [git_dir, common_dir] {
        for name in [
            "MERGE_HEAD",
            "CHERRY_PICK_HEAD",
            "REVERT_HEAD",
            "BISECT_LOG",
            "REBASE_HEAD",
            "gc.lock",
            "index.lock",
            "packed-refs.lock",
            "refs.lock",
            "HEAD.lock",
        ] {
            out.push(dir.join(name));
        }
        for name in [
            "rebase-merge",
            "rebase-apply",
            "sequencer",
            "rebase-merge/git-rebase-todo",
        ] {
            out.push(dir.join(name));
        }
    }
    out
}

/// How many lines of a sequencer/rebase todo file are still to do. Comment lines and blank lines are not
/// work; git itself says so (`git status` prints the same count).
fn remaining_todo(path: &Path) -> usize {
    let Ok(text) = std::fs::read_to_string(path) else {
        return 0;
    };
    text.lines()
        .filter(|line| {
            let line = line.trim();
            !line.is_empty() && !line.starts_with('#')
        })
        .count()
}

/// Read a one-line counter (`msgnum`, `end`, `next`, `last`). Unreadable is `None`, not 0: "git did not
/// tell me" and "commit zero" are different sentences.
fn read_small_number(path: &Path) -> Option<u32> {
    let text = std::fs::read_to_string(path).ok()?;
    text.trim().parse::<u32>().ok()
}

fn push_unique(list: &mut Vec<String>, name: &str) {
    if !list.iter().any(|have| have == name) {
        list.push(name.to_owned());
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        path.push(format!(
            "fathom-watch-guard-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("create the guard fixture");
        let _ = name;
        path
    }

    #[test]
    fn a_quiet_repository_is_in_no_operation_and_holds_no_locks() {
        let root = dir("quiet");
        assert_eq!(operation(&root, &root), Operation::None);
        assert!(!maintenance(&root, &root, &root.join("index")).busy());
        std::fs::remove_dir_all(&root).expect("clean the fixture");
    }

    #[test]
    fn a_rebase_reports_its_step_and_total() {
        let root = dir("rebase");
        std::fs::create_dir_all(root.join("rebase-merge")).expect("rebase-merge");
        std::fs::write(root.join("rebase-merge/interactive"), b"").expect("interactive");
        std::fs::write(root.join("rebase-merge/msgnum"), b"3\n").expect("msgnum");
        std::fs::write(root.join("rebase-merge/end"), b"7\n").expect("end");
        std::fs::write(
            root.join("rebase-merge/git-rebase-todo"),
            b"pick aaaaaaa x\npick bbbbbbb y\n",
        )
        .expect("todo");
        assert_eq!(
            operation(&root, &root),
            Operation::Rebase {
                interactive: true,
                step: Some(3),
                total: Some(7),
                paused: false,
            }
        );
        // The notice has to be something a human can act on, not "operation in progress".
        let text = operation(&root, &root).summary().expect("a summary");
        assert!(
            text.contains("interactive") && text.contains("3 of 7"),
            "{text}"
        );

        // Stop for a conflict and the notice changes, because the advice does.
        std::fs::write(root.join("MERGE_HEAD"), b"aaaaaaa\n").expect("MERGE_HEAD");
        assert!(matches!(
            operation(&root, &root),
            Operation::Rebase { paused: true, .. }
        ));
        std::fs::remove_dir_all(&root).expect("clean the fixture");
    }

    #[test]
    fn a_gc_storm_and_a_ref_lock_are_both_busy() {
        let root = dir("gc");
        std::fs::create_dir_all(root.join("objects/pack")).expect("pack dir");
        std::fs::write(root.join("gc.lock"), b"").expect("gc.lock");
        std::fs::write(root.join("objects/pack/pack-abc.pack.tmp"), b"").expect("tmp pack");
        let seen = maintenance(&root, &root, &root.join("index"));
        assert!(seen.gc && seen.packs_in_flight == 1);
        assert!(seen.summary().unwrap().contains("git gc"));

        std::fs::remove_file(root.join("gc.lock")).expect("release gc.lock");
        std::fs::remove_file(root.join("objects/pack/pack-abc.pack.tmp")).expect("finish the pack");
        assert!(!maintenance(&root, &root, &root.join("index")).busy());
        std::fs::write(root.join("packed-refs.lock"), b"").expect("lock");
        assert!(maintenance(&root, &root, &root.join("index")).busy());
        std::fs::remove_dir_all(&root).expect("clean the fixture");
    }
}
