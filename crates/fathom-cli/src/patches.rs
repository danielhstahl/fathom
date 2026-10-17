//! `patches` — the store→diff-pane byte source (`reco-0kw`).
//!
//! The diff surface (`e1.7`) and the decode engine (`e1.8`) were real, and the store holds every
//! fact a diff is made of: each `git_commit_file` row names the two git blobs, the modes, the
//! taxonomy and noise decisions, and the `@@` headers of every hunk. The bytes of each hunk are the
//! pair those blob references name, in the object store, which is exactly what `fathom show HEAD`
//! proves is there. What did not exist was anything that *joined* the two: no build ever constructed
//! a `PatchSource`, so `d` printed "no diff — no episode is in scope / no patch is loaded" while
//! the store sat full. This module is that join, and it lives here because this is the composition
//! root: the shell may not hold a store handle or open a repository (ADR-002 §5.2, lint `E01`), so
//! the root reads and hands the pane a trait object instead (`fathom_tui::diff::PatchSource`'s own
//! doc has been waiting for exactly this implementation).
//!
//! # What an episode's diff is
//!
//! The *net* change across the episode's member commits, oldest first: for each path, the old side
//! is the first member's before-blob and the new side is the last member's after-blob, and the
//! hunk text is re-rendered from that blob pair with `fathom-ingest`'s own renderer — the same
//! algorithm, context and caps that produced the stored `hunk_headers`, so the pane's hunks and
//! the spine's headers are one arithmetic stated twice (ADR-001 §7.10). A path renamed and then
//! edited mid-episode aggregates to a rename plus the net edit; the corners where per-commit
//! history would differ (a path touched, renamed, then touched again under the old name) aggregate
//! to one row — the pane prints what the blob pair actually diffs to, never an invented hunk.
//!
//! # Why bytes are materialised once
//!
//! `PatchReader` is positioned so the viewer can window a huge patch without holding it, and a
//! source that hands over a `Vec<u8>` is explicitly the source's documented cost (source.rs). The
//! alternative — re-opening the store and re-diffing every blob pair on every frame — pays git I/O
//! per keystroke; this pays once per `d` and then clones an `Arc`. Over-budget episodes still
//! degrade honestly: per-file caps print as `X-Fathom-Skip` rows, and the diff index budgets bound
//! what the pane retains whatever the source materialised.

use std::path::{Path, PathBuf};

use fathom_ingest::diff::Rendered;
use fathom_ingest::oid::Oid;
use fathom_ingest::record::{Limits, NotExamined};
use fathom_ingest::repo::Repository;
use fathom_store::spine::Store;
use fathom_tui::diff::{BytesSource, PatchProvider, PatchSource, Unavailable};

/// The one ceiling on a materialised episode diff: past it the files beyond the cap state so in
/// their own rows (`X-Fathom-Skip`) instead of the process swelling. Generously above what the
/// pane's own 16 MiB index budget can retain, so on every repository the pane should be reading,
/// this is inert.
#[cfg(feature = "sqlite-bundled")]
const PATCH_CAP_BYTES: usize = 128 * 1024 * 1024;

/// The provider the shell installs: turns the scope's episode id into its patch, from the index.
///
/// `None` only where there is no backend to read from at all (a binary built without
/// `sqlite-bundled`), which is a fact about the build, and the pane's empty state says that much
/// rather than blaming the repository (`reco-1nu`).
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
#[must_use]
pub fn provider(root: &Path) -> Option<fathom_tui::diff::Provider> {
    Some(fathom_tui::diff::Provider::new(StorePatches {
        root: root.to_path_buf(),
    }))
}

/// A binary built without a SQLite backend has no index to be a byte source for, and the pane's
/// empty state is the one that says which build this is.
#[cfg(all(feature = "tui", not(feature = "sqlite-bundled")))]
#[must_use]
pub fn provider(_root: &Path) -> Option<fathom_tui::diff::Provider> {
    None
}

/// The store-backed patch provider: opens the spine and the repository per request, because a
/// provider outlives frames but holds nothing between them — no handle to go stale, no lock held
/// across a keystroke.
#[cfg(feature = "sqlite-bundled")]
pub struct StorePatches {
    /// The repository under review — the same root the shell was opened in.
    root: PathBuf,
}

#[cfg(feature = "sqlite-bundled")]
impl PatchProvider for StorePatches {
    fn patch_for(&self, scope: &str) -> Result<Box<dyn PatchSource>, Unavailable> {
        let store = Store::open(&crate::session::store_path(&self.root)).map_err(|err| {
            Unavailable::with_hint(
                format!("the index would not open: {err}"),
                "`fathom index` builds it",
            )
        })?;
        match store.episode(scope) {
            Ok(Some(row)) => {
                let label = format!("episode {} · {} commit(s)", short(scope), row.members.len());
                return build(&store, &self.root, &row.members, &label);
            }
            Ok(None) => {}
            Err(err) => {
                return Err(Unavailable::with_hint(
                    format!("the episode read failed: {err}"),
                    "`fathom doctor` checks the store",
                ));
            }
        }
        // A bare commit oid is also a scope `fathom show` accepts, so `d` answers it too — a
        // reviewer who got here from a citation or a card must not be the only reviewer a `d`
        // cannot serve.
        if is_oid(scope) && store.read_commit(scope).is_ok() {
            let members = vec![scope.to_owned()];
            return build(
                &store,
                &self.root,
                &members,
                &format!("commit {}", short(scope)),
            );
        }
        Err(Unavailable::with_hint(
            format!("the index has no episode `{scope}`"),
            "select an episode in the rail, or run `fathom index`",
        ))
    }
}

/// The seven-character prefix every human-facing line uses, git-style.
#[cfg(feature = "sqlite-bundled")]
fn short(oid: &str) -> String {
    oid.chars().take(7).collect()
}

#[cfg(feature = "sqlite-bundled")]
fn is_oid(text: &str) -> bool {
    text.len() == 40 && text.chars().all(|ch| ch.is_ascii_hexdigit())
}

/// Materialise the members' net diff and wrap it as an in-memory source.
#[cfg(feature = "sqlite-bundled")]
fn build(
    store: &Store,
    root: &Path,
    members: &[String],
    label: &str,
) -> Result<Box<dyn PatchSource>, Unavailable> {
    let repo = Repository::discover(root).map_err(|err| {
        Unavailable::with_hint(
            format!("the object store is not reachable from here: {err}"),
            "open fathom inside the repository, where `fathom index` ran",
        )
    })?;
    let limits = Limits::standard();
    let (aggs, missing) = aggregate(store, members)?;
    let mut out = Vec::new();
    for agg in &aggs {
        emit_file(&mut out, agg, &repo, &limits);
    }
    for oid in &missing {
        // A member the spine has no row for is a stated pseudo-file, not a silent shrink of the
        // episode: the tree prints its row and the reason (ADR-001 §7.2).
        out.extend_from_slice(
            format!(
                "diff --git a/.fathom/missing-commit/{} b/.fathom/missing-commit/{}\n",
                short(oid),
                short(oid)
            )
            .as_bytes(),
        );
        emit_skip(
            &mut out,
            &format!("commit {oid} has no row in the index"),
            Some("`fathom index` writes the spine rows"),
        );
    }
    if aggs.is_empty() && missing.is_empty() {
        return Err(Unavailable::with_hint(
            format!("`{label}` has no file rows in the index"),
            "`fathom index` writes them; `fathom doctor` says why it has not",
        ));
    }
    Ok(Box::new(BytesSource::new(label, out)))
}

/// One path's net state across the episode, in first-touch order.
#[cfg(feature = "sqlite-bundled")]
#[derive(Clone)]
struct Agg {
    /// The path as the pane names it — the last one the episode saw.
    path: String,
    /// The rename/copy origin, if any row named one.
    old_path: Option<String>,
    /// `add` on the first touch: the path was created inside the episode.
    added: bool,
    /// Gone at the end: deleted, and not re-created after.
    deleted: bool,
    /// The first row's before side — the byte source's old half.
    blob_old: Option<String>,
    mode_old: Option<i64>,
    /// The last row's after side — the byte source's new half.
    blob_new: Option<String>,
    mode_new: Option<i64>,
    /// Any side measured binary makes the path binary; the row says why in the pane.
    binary: bool,
    /// The taxonomy and noise decisions as the latest row recorded them (never re-derived here,
    /// ADR-004 §6.6 R3).
    path_class: String,
    noise_class: String,
}

/// Fold every member's file rows into one row per path: old side from the first touch, new side
/// from the last, rename-aware because a rename row moves the aggregation to the new name.
/// Members the spine cannot answer for come back in `missing` — a fact for the caller, never a
/// silently shorter episode.
#[cfg(feature = "sqlite-bundled")]
fn aggregate(store: &Store, members: &[String]) -> Result<(Vec<Agg>, Vec<String>), Unavailable> {
    let mut order: Vec<usize> = Vec::new();
    let mut aggs: Vec<Agg> = Vec::new();
    // Keyed by the path's current name; a rename moves the entry.
    let mut index: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut missing: Vec<String> = Vec::new();
    for member in members {
        let row = match store.read_commit(member) {
            Ok(row) => row,
            Err(_) => {
                missing.push(member.clone());
                continue;
            }
        };
        for file in &row.files {
            let known = index.get(&file.new_path).copied().or_else(|| {
                file.old_path
                    .as_ref()
                    .and_then(|old| index.get(old).copied())
            });
            let slot = match known {
                Some(slot) => {
                    // A rename of a path this episode already touched moves the aggregation to the
                    // new name; the pane sees one row for the file, not two.
                    if index.contains_key(&file.new_path) {
                        slot
                    } else if let Some(old) = &file.old_path {
                        index.remove(old);
                        index.insert(file.new_path.clone(), slot);
                        slot
                    } else {
                        slot
                    }
                }
                None => {
                    aggs.push(Agg {
                        path: file.new_path.clone(),
                        old_path: file.old_path.clone(),
                        added: file.change_kind == "add",
                        deleted: false,
                        blob_old: file.blob_old.clone(),
                        mode_old: file.mode_old,
                        blob_new: file.blob_new.clone(),
                        mode_new: file.mode_new,
                        binary: file.is_binary,
                        path_class: file.path_class.clone(),
                        noise_class: file.noise_class.clone(),
                    });
                    let slot = aggs.len() - 1;
                    index.insert(file.new_path.clone(), slot);
                    order.push(slot);
                    slot
                }
            };
            let agg = &mut aggs[slot];
            agg.path = file.new_path.clone();
            // The old side is a first-touch fact and the new side a last-touch one; a creation makes
            // the first row's before side meaningless (the path never existed before the episode),
            // and a deletion empties the last row's after side.
            if agg.added {
                agg.blob_old = None;
                agg.mode_old = None;
            }
            if file.change_kind == "delete" {
                agg.deleted = true;
                agg.blob_new = None;
                agg.mode_new = None;
            } else {
                agg.deleted = false;
                agg.blob_new = file.blob_new.clone();
                agg.mode_new = file.mode_new;
            }
            agg.binary |= file.is_binary;
            agg.path_class = file.path_class.clone();
            agg.noise_class = file.noise_class.clone();
            if let Some(old) = &file.old_path {
                // A rename row: the pane prints the *first* name in the chain as the origin.
                agg.old_path = Some(agg.old_path.take().unwrap_or_else(|| old.clone()));
            }
        }
    }
    Ok((
        order.into_iter().map(|slot| aggs[slot].clone()).collect(),
        missing,
    ))
}

/// Emit one aggregated path as a file section of the patch.
#[cfg(feature = "sqlite-bundled")]
fn emit_file(out: &mut Vec<u8>, agg: &Agg, repo: &Repository, limits: &Limits) {
    if agg.added && agg.deleted {
        // Created and removed inside the episode: net zero, and a row for it would be the tree
        // listing a file no reviewer will ever find in either tree.
        return;
    }
    if out.len() > PATCH_CAP_BYTES {
        // Past the materialisation cap the file rows still exist; only their text is withheld, and
        // each row says so, because a silently short list is the one tree shape that lies.
        out.extend_from_slice(b"diff --git a/.fathom/truncated b/.fathom/truncated\n");
        emit_skip(
            out,
            "episode diff over the materialisation cap",
            Some("narrow the episode, or read `fathom show <episode>`"),
        );
        return;
    }
    let old_name = agg.old_path.clone().unwrap_or_else(|| agg.path.clone());
    let new_name = agg.path.clone();
    out.extend_from_slice(
        format!("diff --git a/{} b/{}\n", quote(&old_name), quote(&new_name)).as_bytes(),
    );
    if agg.old_path.is_some() {
        out.extend_from_slice(
            format!(
                "rename from {}\nrename to {}\n",
                quote(&old_name),
                quote(&new_name)
            )
            .as_bytes(),
        );
    }
    if agg.added {
        out.extend_from_slice(format!("new file mode {}\n", mode(agg.mode_new)).as_bytes());
    } else if agg.deleted {
        out.extend_from_slice(format!("deleted file mode {}\n", mode(agg.mode_old)).as_bytes());
    } else if let (Some(old), Some(new)) = (agg.mode_old, agg.mode_new)
        && old != new
    {
        out.extend_from_slice(
            format!(
                "old mode {}\nnew mode {}\n",
                mode(Some(old)),
                mode(Some(new))
            )
            .as_bytes(),
        );
    }
    // The spine's decisions ride in the patch so the viewer classifies without a store handle —
    // the grammar docs/concepts/diff-viewer.md §3 defines and `scan.rs` already parses.
    if agg.path_class != "unknown" {
        out.extend_from_slice(format!("X-Fathom-Class: {}\n", agg.path_class).as_bytes());
    }
    if agg.noise_class != "unknown" && agg.noise_class != "informative" {
        out.extend_from_slice(
            format!(
                "X-Fathom-Noise: {} rule=\"{} as the index recorded it\"\n",
                agg.noise_class, agg.noise_class
            )
            .as_bytes(),
        );
    }
    if agg.binary {
        emit_binary(out, &old_name, &new_name);
        return;
    }
    let old_oid = match oid(&agg.blob_old) {
        Ok(oid) => oid,
        Err(why) => return emit_skip(out, &why, None),
    };
    let new_oid = match oid(&agg.blob_new) {
        Ok(oid) => oid,
        Err(why) => return emit_skip(out, &why, None),
    };
    match fathom_ingest::diff::render_blobs(repo, old_oid, new_oid, limits) {
        Ok(Rendered::Text { hunks }) => {
            out.extend_from_slice(format!("--- a/{}\n", quote(&old_name)).as_bytes());
            out.extend_from_slice(format!("+++ b/{}\n", quote(&new_name)).as_bytes());
            out.extend_from_slice(&hunks);
        }
        Ok(Rendered::Binary) => emit_binary(out, &old_name, &new_name),
        Ok(Rendered::NotExamined { reason }) => match reason {
            NotExamined::Oversized { size } => {
                out.extend_from_slice(
                    format!(
                        "X-Fathom-Skip: oversize bytes={} cap={}\n",
                        size, limits.blob_memory_cap
                    )
                    .as_bytes(),
                );
            }
            NotExamined::Unavailable { why } => {
                emit_skip(out, &format!("the blob is {why}"), Some(why.hint()))
            }
            NotExamined::ExternalDiffDriver { driver } => emit_skip(
                out,
                &format!(
                    "a `.gitattributes` diff driver (`{driver}`) owns this file's text; fathom does not run programs the repository names"
                ),
                None,
            ),
            NotExamined::NotABlob { found } => emit_skip(
                out,
                &format!("the stored object is a {found}, not a file"),
                None,
            ),
        },
        // The object the store named cannot be had. That is a stated per-file absence in the pane,
        // not a failed keystroke: the rest of the episode still shows.
        Err(err) => emit_skip(
            out,
            &format!("{err}"),
            Some("`fathom doctor` classifies missing objects"),
        ),
    }
}

#[cfg(feature = "sqlite-bundled")]
fn emit_binary(out: &mut Vec<u8>, old_name: &str, new_name: &str) {
    out.extend_from_slice(
        format!(
            "Binary files a/{} and b/{} differ\n",
            quote(old_name),
            quote(new_name)
        )
        .as_bytes(),
    );
}

/// `X-Fathom-Skip: unavailable reason="…" [hint="…"]`, quoted so a sentence stays one field.
#[cfg(feature = "sqlite-bundled")]
fn emit_skip(out: &mut Vec<u8>, reason: &str, hint: Option<&str>) {
    let reason = reason.replace('"', "'");
    match hint {
        Some(hint) => out.extend_from_slice(
            format!(
                "X-Fathom-Skip: unavailable reason=\"{reason}\" hint=\"{}\"\n",
                hint.replace('"', "'")
            )
            .as_bytes(),
        ),
        None => {
            out.extend_from_slice(
                format!("X-Fathom-Skip: unavailable reason=\"{reason}\"\n").as_bytes(),
            );
        }
    }
}

/// The stored hex as a git oid; an unreadable column is a stated skip, never a panic.
#[cfg(feature = "sqlite-bundled")]
fn oid(hex: &Option<String>) -> Result<Option<Oid>, String> {
    match hex {
        None => Ok(None),
        Some(hex) => hex
            .parse::<Oid>()
            .map(Some)
            .map_err(|_| format!("the index names blob `{hex}`, which is not an oid")),
    }
}

/// The mode as git prints it: octal, no `0o`.
#[cfg(feature = "sqlite-bundled")]
fn mode(value: Option<i64>) -> String {
    value.map_or_else(|| "0".to_owned(), |value| format!("{value:o}"))
}

/// Quote a path the way git quotes it in a patch header — which the scanner's `unquote` undoes.
/// Only the cases that actually happen: a space, a quote, a backslash, a control character, or any
/// non-ASCII byte (git quotes those too, and the scanner's `unquote` passes the rest through).
#[cfg(feature = "sqlite-bundled")]
fn quote(path: &str) -> String {
    let needs = path
        .chars()
        .any(|ch| !ch.is_ascii() || ch == ' ' || ch == '"' || ch == '\\' || ch.is_control());
    if !needs {
        return path.to_owned();
    }
    let mut out = String::with_capacity(path.len() + 2);
    out.push('"');
    for ch in path.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}
