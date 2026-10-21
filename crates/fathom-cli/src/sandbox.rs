//! Containment: the rule that a run may only write inside the repository it was pointed at.
//!
//! Everything Fathom writes for a repository lives under `<root>/.fathom/` — the store, the object cache,
//! the harness cursor and its quarantine, and the review ledger (ADR-004 §3, §12.1). The paths are built
//! by joining onto `root`, so a plain `..` in a path is already impossible. What a join cannot see is the
//! filesystem underneath it: if `.fathom`, or `.fathom/reviews`, or `.fathom/reviews/ledger.md` is a
//! **symlink**, every write lands wherever that link points, and a clone can put a symlink in the tree
//! (`git` stores symlinks as ordinary content, and `core.symlinks=true` is the default on Linux and
//! macOS). That is the difference between "writes inside the repository" and "writes wherever the
//! repository says", and it is the one hostile-repository write escape this file exists to close
//! ([threat model F-1](../../../docs/security/threat-model.md), `e7.8` / `reco-41a`).
//!
//! # The rule, and what it deliberately is not
//!
//! **A path a run is about to write must resolve, symlinks and all, to somewhere inside the canonicalised
//! repository root.** The check is on resolution rather than on "is a symlink", which keeps the two things
//! people legitimately do working and the one thing a clone can do not working:
//!
//! * `.fathom -> ./.fathom-on-big-disk` inside the repository — allowed, because it resolves inside;
//! * `.fathom -> /home/taylor/scratch` — refused, because a review tool writing to a directory nobody
//!   named is a bug report nobody can reconstruct, even when the tool means well;
//! * `.fathom/reviews -> ~/.ssh` — refused, and this is the case that matters: the write is an *append*
//!   of attacker-shaped text (the entry carries the episode id, the actor and the note), so an escape
//!   here is an arbitrary-file append outside the repository.
//!
//! What this is **not**: a sandbox. It is a containment check on the paths Fathom itself chooses to write,
//! made of `canonicalize` plus a prefix test, and it is documented as such in
//! [the threat model](../../../docs/security/threat-model.md). It does not stop a process that has the
//! user's own rights from doing what the user could do, and it is not a substitute for not running
//! repository-provided programs at all — which is a separate rule, held by [`crate::handoff`].
//!
//! # Where it is called
//!
//! From the verbs that create or append a file: `fathom index` (the store's directory), `fathom review
//! sign|sync` (the ledger, and again per write target), `fathom watch` (the watcher's own store) and
//! `fathom attribution correct|forget` (the store again). Read-only verbs are not gated: refusing to
//! *read* a repository because its `.fathom` is a symlink would be a refusal to answer, and reads are
//! covered by the ledger's own containment rule in
//! [`fathom_analyze::ledger::format::discover`].

use std::path::{Component, Path, PathBuf};

use fathom_core::exit::ExitCode;

use crate::verbs::Pending;

/// The directory every repository-local write lives under.
const STATE_DIR: &str = ".fathom";

/// The canonicalised root, and nothing else.
#[derive(Debug, Clone)]
pub struct Root {
    canonical: PathBuf,
}

impl Root {
    /// Canonicalise `root`, or keep it verbatim when it cannot be canonicalised.
    ///
    /// A root that does not exist fails the verbs long before this point, so the fallback is only for the
    /// odd platform case (a permission-denied ancestor). Falling back to the literal path makes the check
    /// *stricter*, not looser: nothing resolves inside a path that cannot be resolved.
    #[must_use]
    pub fn for_root(root: &Path) -> Self {
        Root {
            canonical: root.canonicalize().unwrap_or_else(|_| root.to_path_buf()),
        }
    }

    /// The canonical repository root, for messages and for tests.
    #[must_use]
    pub fn canonical(&self) -> &Path {
        &self.canonical
    }

    /// Does `path` stay inside this repository — textually *and* once the filesystem answers?
    ///
    /// Two tests, because each catches what the other cannot:
    ///
    /// * the **textual** one refuses `..` before any filesystem state is consulted, so a path written as
    ///   `root/.fathom/../../elsewhere` cannot be talked into looking contained by a missing directory on
    ///   the way (a `..` after a component that does not exist resolves to something nobody wrote);
    /// * the **resolved** one follows the symlinks that do exist, which is the case a clone actually
    ///   ships.
    ///
    /// The tail of the path is allowed not to exist yet — that is the normal case for the file about to
    /// be created — so the nearest existing ancestor is resolved and the rest is appended, exactly as
    /// [`crate::sanitize::Redactor::path`] does for printing.
    #[must_use]
    pub fn contains(&self, path: &Path) -> bool {
        textual(path).is_some_and(|clean| clean.starts_with(&self.canonical))
            && resolve(path).starts_with(&self.canonical)
    }

    /// Refuse a `.fathom` that does not resolve inside the repository.
    ///
    /// Checked once per run, before anything is created, so the remedy is printed before a half-written
    /// store exists rather than after.
    ///
    /// # Errors
    ///
    /// A [`Pending`] naming the link's target and the two ways to fix it.
    pub fn ensure_state_dir(&self) -> Result<(), Pending> {
        let state = self.canonical.join(STATE_DIR);
        if state.symlink_metadata().is_ok() && !self.contains(&state) {
            return Err(violation(&state, &resolve(&state), "nothing was written"));
        }
        Ok(())
    }

    /// Refuse one concrete write target that does not resolve inside the repository.
    ///
    /// # Errors
    ///
    /// A [`Pending`] naming the target and where it really goes.
    pub fn ensure_write(&self, path: &Path) -> Result<(), Pending> {
        if self.contains(path) {
            return Ok(());
        }
        Err(violation(path, &resolve(path), "nothing was written"))
    }
}

/// The refusal, in the shape every verb prints: what was refused, where it would really have gone, and
/// what to do about it.
fn violation(path: &Path, target: &Path, outcome: &str) -> Pending {
    Pending::new(
        ExitCode::Error,
        format!(
            "{} resolves to {}, outside the repository, so Fathom will not write through it",
            path.display(),
            target.display()
        ),
        Some(format!(
            "a clone can put that symlink in the tree; replace it with a real directory (or move the \
             store with a path inside the repository) and run again — {outcome}"
        )),
    )
}

/// The absolute, `..`-collapsed spelling of `path`, computed without asking the filesystem anything.
///
/// `None` means the `..` walked above the filesystem root, which is outside every repository by definition.
#[must_use]
pub fn textual(path: &Path) -> Option<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    strip_dots(&absolute)
}

/// Resolve what exists and re-attach the rest, so a path that is about to be created is still an
/// absolute path with its symlinks followed.
///
/// `Path::canonicalize` needs the whole path to exist, which is exactly false for the file a verb is
/// about to create; resolving the nearest existing ancestor and pushing the remainder back on is the
/// same compromise `crate::sanitize` makes for the same reason.
#[must_use]
pub fn resolve(path: &Path) -> PathBuf {
    if let Ok(absolute) = path.canonicalize() {
        return absolute;
    }
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    let mut cursor = path;
    while !cursor.exists() {
        if let Some(name) = cursor.file_name() {
            tail.push(name.to_os_string());
        }
        let Some(parent) = cursor.parent() else { break };
        cursor = parent;
    }
    let mut base = match cursor.canonicalize() {
        Ok(absolute) => absolute,
        Err(_) => absolute_fallback(cursor),
    };
    for name in tail.iter().rev() {
        base.push(name);
    }
    base
}

/// The literal path, made absolute, when even the ancestor will not canonicalise.
fn absolute_fallback(path: &Path) -> PathBuf {
    if path.is_absolute() {
        strip_dots(path).unwrap_or_else(|| path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .map(|joined| strip_dots(&joined).unwrap_or(joined))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

/// A purely textual `..` collapse, used only when the filesystem will not answer.
fn strip_dots(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    Some(out)
}

/// The one-call form for a verb that only needs the `.fathom` question answered.
///
/// # Errors
///
/// As [`Root::ensure_state_dir`].
pub fn ensure_state_dir(root: &Path) -> Result<(), Pending> {
    Root::for_root(root).ensure_state_dir()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn a_path_that_does_not_exist_yet_is_still_contained() {
        let dir = std::env::temp_dir().join(format!("fathom-sandbox-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create the fixture");
        let root = Root::for_root(&dir);
        assert!(root.contains(&dir.join(".fathom").join("reviews").join("ledger.md")));
        assert!(!root.contains(Path::new("/etc/passwd")));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_dot_dot_escape_is_refused_without_the_directory_existing() {
        let dir =
            std::env::temp_dir().join(format!("fathom-sandbox-escape-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create the fixture");
        let root = Root::for_root(&dir);
        let sneaky = dir.join(".fathom").join("..").join("..").join("elsewhere");
        assert!(
            !root.contains(&sneaky),
            "a purely textual escape must be refused even with no filesystem behind it"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_symlink_out_of_the_repository_is_refused() {
        let base = std::env::temp_dir().join(format!("fathom-sandbox-link-{}", std::process::id()));
        let dir = base.join("repo");
        let outside = base.join("outside");
        std::fs::remove_dir_all(&base).ok();
        std::fs::create_dir_all(&dir).expect("create the repository");
        std::fs::create_dir_all(&outside).expect("create the outside directory");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, dir.join(".fathom")).expect("symlink .fathom");
        #[cfg(not(unix))]
        std::os::windows::fs::symlink_dir(&outside, dir.join(".fathom")).expect("link .fathom");

        let root = Root::for_root(&dir);
        assert!(
            !root.ensure_state_dir().is_ok(),
            "a clone's symlinked .fathom must be refused, not written through"
        );
        assert!(
            root.contains(&dir.join("README.md")),
            "an ordinary path still passes"
        );
        std::fs::remove_dir_all(&base).ok();
    }
}
