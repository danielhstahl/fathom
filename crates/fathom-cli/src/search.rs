//! The corpus the one search box reads (`e5.2`, `reco-mkc`).
//!
//! ADR-002 §5.2 says the shell may not open a store, and `/` is the first surface that wants more than a
//! page of rows out of it: episodes, the paths they touched, the dependencies the manifests name and the
//! findings the detectors raised, in one scored list. So this module is the read the shell is not allowed
//! to do — the same shape as [`crate::episodes`] and [`crate::patches`], which exist for exactly the same
//! reason: the composition root pulls, the shell draws what it was handed.
//!
//! Three rules:
//!
//! * **The corpus is built once, at startup.** A search per keystroke is a scoring job over rows already in
//!   memory ([`fathom_analyze::search::Index`]'s budget is what keeps that honest); a *re-read* per keystroke
//!   would be a store query per keystroke, which is a different and worse cost. A repository indexed while
//!   the shell is open is a repository the next `/` sees after a restart — stated in the corpus's own gaps
//!   rather than quietly fresher than it looks.
//! * **A kind the caller did not read is a gap, not an empty result.** The dependency names come from the
//!   manifests this module can see; the findings would come from detectors that need the delta model, which
//!   this build does not run at startup. Each of those is a sentence in [`Lend`]'s notes and a line in the
//!   report's gaps, because "no findings match your search" and "nothing raised findings in this session"
//!   are different answers and only one of them is about the reviewer's query.
//! * **`None` is a real answer with a reason.** No store, an unreadable store, a store built without a
//!   backend: the shell prints which, and `/` says what would make it answer, rather than accepting a query
//!   into a black hole.

use std::path::Path;
use std::sync::Arc;

use fathom_analyze::search::corpus::{self, Dependency};
use fathom_analyze::search::Corpus;
use fathom_analyze::search::Index;
use fathom_tui::index::Facts;

/// The manifests worth searching through, by file name (`fathom_analyze::deps` owns the recognition; this is
/// only the list of names worth *looking* for, at the depths a repository keeps them at).
const MANIFEST_NAMES: &[&str] = &[
    "Cargo.lock",
    "Cargo.toml",
    "package-lock.json",
    "package.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "pyproject.toml",
    "requirements.txt",
    "poetry.lock",
    "go.mod",
    "go.sum",
    "Gemfile.lock",
    "composer.lock",
];

/// How many directories below the root to look for a manifest. Two is enough for a workspace and shallow
/// enough that the walk is not a `find` in disguise.
const MANIFEST_DEPTH: usize = 2;

/// How many manifest files one corpus reads. A monorepo with three hundred `package.json` files is not
/// answering a search box any better than the twenty biggest.
const MANIFEST_MAX: usize = 24;

/// The index the search box will score against, or `None` with the reason the shell should print.
#[cfg(feature = "tui")]
#[must_use]
pub fn index(root: &Path, facts: &Facts) -> Option<Arc<Index>> {
    let corpus = corpus_for(root, facts)?;
    Some(Arc::new(Index::build(&corpus)))
}

/// The corpus itself, which the headless `fathom search` needs for its `--explain` half and the shell needs
/// for the index, so the read is written once.
#[cfg(feature = "tui")]
#[must_use]
pub fn corpus_for(root: &Path, facts: &Facts) -> Option<Corpus> {
    read(root, facts)
}

/// The manifest rows: the dependency names the repository's own lockfiles and manifests declare.
///
/// A lockfile's resolved packages are the ones a reviewer searches for — `ratatui`, `serde` — because those
/// are the names that appear in a diff. A manifest's *declared* requirements are searched too: a dependency
/// added to `Cargo.toml` is searchable before anything has resolved it. The depth and count bounds are the
/// whole policy here; what counts as a manifest, and what can be read out of one, is `fathom_analyze::deps`.
fn manifests(root: &Path) -> (Vec<Dependency>, Vec<String>) {
    use fathom_analyze::deps::Body;

    let mut found: Vec<Dependency> = Vec::new();
    let mut unread: Vec<String> = Vec::new();
    let mut seen: std::collections::BTreeSet<(String, String)> = std::collections::BTreeSet::new();
    for path in candidates(root).into_iter().take(MANIFEST_MAX) {
        let relative = path
            .strip_prefix(root)
            .unwrap_or(path.as_path())
            .to_string_lossy()
            .replace('\\', "/");
        let Ok(text) = std::fs::read_to_string(&path) else {
            // A manifest that is not UTF-8, or is a directory, or is a dangling symlink: not a name to
            // search, and not worth a note. What *looks* readable and is not goes into `unread` below,
            // which is the list a gap sentence is allowed to name.
            continue;
        };
        let Some((format, body)) = fathom_analyze::deps::parse_recognized(&relative, &text) else {
            continue;
        };
        let ecosystem = format.ecosystem().as_str().to_owned();
        let names: Vec<String> = match &body {
            Body::Lock(lock) => lock
                .packages
                .iter()
                .map(|package| package.name.clone())
                .collect(),
            Body::Manifest(manifest) => manifest
                .requirements
                .iter()
                .map(|requirement| requirement.name.clone())
                .collect(),
            // A workspace file names members, not dependencies, and an unparsed one says which; neither is
            // a package, and saying "unparsed" is the corpus's gap sentence's job, not a silent empty list.
            Body::Workspace { .. } => Vec::new(),
            Body::Unparsed(why) => {
                unread.push(format!("{relative}: {:?}", why.reason));
                Vec::new()
            }
        };
        for name in names {
            if found.len() >= DEPENDENCY_LIMIT {
                break;
            }
            if seen.insert((ecosystem.clone(), name.clone())) {
                found.push(Dependency {
                    name,
                    ecosystem: ecosystem.clone(),
                    manifest: relative.clone(),
                });
            }
        }
    }
    unread.sort();
    unread.truncate(3);
    (found, unread)
}

/// How many dependency names one corpus carries. Mirrors `search::corpus`'s own cap: the point of the
/// number is that a `Cargo.lock` is not allowed to become two thirds of a searchable corpus.
const DEPENDENCY_LIMIT: usize = 1_000;

/// The manifest paths worth opening, shallowest first, so a root `Cargo.lock` wins over a vendored one.
fn candidates(root: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut frontier = vec![root.to_path_buf()];
    for depth in 0..=MANIFEST_DEPTH {
        let mut next = Vec::new();
        for dir in frontier {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let file_type = match entry.file_type() {
                    Ok(file_type) => file_type,
                    Err(_) => continue,
                };
                if file_type.is_dir() {
                    let name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    let spelled: &str = &name;
                    // The directories that are never worth a manifest lookup, and the hidden ones that
                    // are someone else's working state.
                    if name.starts_with('.')
                        || matches!(
                            spelled,
                            "node_modules" | "target" | "build" | "dist" | "vendor" | ".git"
                        )
                    {
                        continue;
                    }
                    if depth < MANIFEST_DEPTH {
                        next.push(path);
                    }
                    continue;
                }
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                if MANIFEST_NAMES.contains(&name.as_str()) {
                    out.push(path);
                }
            }
        }
        frontier = next;
    }
    out.sort();
    out
}

#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
fn read(root: &Path, facts: &Facts) -> Option<Corpus> {
    if !facts.is_present() {
        return None;
    }
    let store = match fathom_store::spine::Store::open(&crate::session::store_path(root)) {
        Ok(store) => store,
        Err(why) => {
            // The caller read `facts` seconds ago, so this is a fact about the store and not about the
            // build; the shell has no better sentence than the one it already prints for an unreadable
            // index, so returning `None` here is the honest compression rather than a swallowed error.
            eprintln!("fathom: the search corpus could not be read from the store: {why}");
            return None;
        }
    };
    let (dependencies, unread) = manifests(root);
    let empty = dependencies.is_empty();
    let lend = corpus::Lend {
        dependencies,
        findings: Vec::new(),
        dependency_note: if empty {
            Some(
                "no dependency manifest was readable from the working tree, so no dependency name is \
                 searchable — `fathom deps` says what it can read"
                    .to_owned(),
            )
        } else if unread.is_empty() {
            None
        } else {
            Some(
                "some manifests could not be read, so the dependency list is shorter than the \
                 repository's"
                    .to_owned(),
            )
        },
        // `e4.3`'s detectors are not run at startup: they need the delta model and a scan of the diff, and
        // `/` is not allowed to become a minute of work before it draws a frame. Findings are searchable
        // when a caller hands them over; this caller does not, and says so.
        finding_note: Some(
            "no detector ran in this session, so findings are not searchable yet — `fathom analyze` \
             raises them and `fathom why` explains one"
                .to_owned(),
        ),
    };
    match corpus::from_store(&store, &lend) {
        Ok(corpus) => Some(corpus),
        Err(why) => {
            // Half a corpus is worse than none: a box that searches the episodes and quietly not the
            // dependencies would still be believed. Say what failed, and hand over nothing.
            eprintln!("fathom: the search corpus could not be built: {why}");
            None
        }
    }
}

#[cfg(all(feature = "tui", not(feature = "sqlite-bundled")))]
fn read(_root: &Path, _facts: &Facts) -> Option<Corpus> {
    // A binary with no backend has no index to search. `fathom_tui::index` already owns the sentence for
    // that state; this module's job is to decline without inventing a second one.
    None
}
