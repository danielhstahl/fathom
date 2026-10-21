//! The corpus the one search box reads, and the headless form of the box (`e5.2`, `reco-mkc`).
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
//!
//! # `fathom search`
//!
//! The same corpus and the same scorer, asked from a shell that never took a screen. It exists because a
//! ranking only the interactive shell can ask is a ranking a script cannot use — a pre-submit hook, a
//! notebook of "what did I look at", an agent asking where a name lives. `/` and this verb cannot
//! disagree, because one [`Index`], one query grammar and one [`Corpus`] builder sit between them.

use std::path::Path;
use std::sync::Arc;

// The corpus, the index and the store reads behind them exist only where there is a store: a `minimal`
// build compiles this module down to its own refusal (ADR-002 §6.2).
#[cfg(feature = "sqlite-bundled")]
use fathom_analyze::search::Corpus;
#[cfg(feature = "sqlite-bundled")]
use fathom_analyze::search::Index;
#[cfg(feature = "sqlite-bundled")]
use fathom_analyze::search::corpus::{self, Dependency};
#[cfg(feature = "sqlite-bundled")]
use fathom_analyze::search::history;
#[cfg(feature = "sqlite-bundled")]
use fathom_analyze::search::query::Query;
use fathom_core::exit::ExitCode;
#[cfg(feature = "sqlite-bundled")]
use fathom_core::time::Clock as _;

#[cfg(feature = "sqlite-bundled")]
use crate::output;
#[cfg(feature = "sqlite-bundled")]
use crate::verbs::Pending;
use crate::verbs::SearchArgs;

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
#[must_use]
#[cfg(feature = "sqlite-bundled")]
pub fn index(root: &Path) -> Option<Arc<Index>> {
    let corpus = corpus_for(root)?;
    Some(Arc::new(Index::build(&corpus)))
}

/// The corpus itself, which the headless `fathom search` needs for its `--explain` half and the shell needs
/// for the index, so the read is written once.
#[must_use]
#[cfg(feature = "sqlite-bundled")]
pub fn corpus_for(root: &Path) -> Option<Corpus> {
    read(root)
}

/// The manifest rows: the dependency names the repository's own lockfiles and manifests declare.
///
/// A lockfile's resolved packages are the ones a reviewer searches for — `ratatui`, `serde` — because those
/// are the names that appear in a diff. A manifest's *declared* requirements are searched too: a dependency
/// added to `Cargo.toml` is searchable before anything has resolved it. The depth and count bounds are the
/// whole policy here; what counts as a manifest, and what can be read out of one, is `fathom_analyze::deps`.
#[cfg(feature = "sqlite-bundled")]
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

#[cfg(feature = "sqlite-bundled")]
fn read(root: &Path) -> Option<Corpus> {
    let path = fathom_analyze::index::default_store_path(root);
    if !path.is_file() {
        return None;
    }
    let store = match fathom_store::spine::Store::open(&path) {
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

// ---------------------------------------------------------------------------
// `fathom search` — the same box, asked from a shell that took no screen
// ---------------------------------------------------------------------------

/// The contract id a machine-readable search carries, beside `fathom.compare/1` and the delta's own.
pub const CONTRACT: &str = "fathom.search/1";

/// `fathom search <query> [--json] [--save NAME] [--list]`.
///
/// # Errors
///
/// Exit `12` when the repository has no index to search (ADR-002 §7.3 — the same code every other
/// store-backed verb uses for "run `fathom index`"), exit `2` for an empty query.
pub fn run(args: &SearchArgs) -> eyre::Result<ExitCode> {
    let query_text = args.query.join(" ").trim().to_owned();

    #[cfg(not(feature = "sqlite-bundled"))]
    {
        let _ = &query_text;
        return Err(crate::verbs::feature_absent(
            "fathom search",
            "sqlite-bundled",
        ));
    }

    #[cfg(feature = "sqlite-bundled")]
    {
        use fathom_store::spine::Store;

        let root = Path::new(&args.root);
        let path = fathom_analyze::index::default_store_path(root);
        // The command line is judged before the repository is opened: `fathom search` with nothing to
        // search is a usage error whether or not there is an index, and telling the reviewer to go and
        // build one so that their empty query can then be refused is the wrong order of advice.
        if !args.list && query_text.is_empty() {
            return Err(Pending::new(
                ExitCode::Usage,
                "an empty query searches for nothing",
                Some(
                    "the same grammar `/` takes: a word, or a filter — `author:dana since:7d risk>60`; \
                     `fathom search --list` prints the saved searches"
                        .to_owned(),
                ),
            )
            .into());
        }
        if !path.is_file() {
            return Err(Pending::new(
                ExitCode::IndexDegraded,
                format!(
                    "no index at {} yet, so there is nothing to search",
                    crate::sanitize::Redactor::for_root(root).path(&path)
                ),
                Some(crate::state::REMEDY_INDEX.to_owned()),
            )
            .into());
        }
        let mut store = Store::open(&path).map_err(crate::verbs::failed)?;
        if args.list {
            return list(&store, args.json);
        }
        if let Some(name) = args.save.as_deref() {
            let (key, value) = history::saved_entry(name, &query_text);
            store
                .set_ui_state(&key, Some(&value), crate::clock::now_millis())
                .map_err(crate::verbs::failed)?;
        }
        let Some(corpus) = read(root) else {
            // `read` already printed why the corpus could not be built; the exit code is the part a script
            // reads, and "no corpus" is the index's gap, not the reviewer's typo.
            return Ok(ExitCode::IndexDegraded);
        };
        let index = Index::build(&corpus);
        let now = crate::clock::SystemClock.now();
        let query = Query::parse(&query_text, now);
        let report = index.search(&query, args.limit.max(1));
        if args.json {
            output::line(format_args!("{}", payload(&report)));
        } else {
            for line in render(&report) {
                output::line(format_args!("{line}"));
            }
            if let Some(name) = args.save.as_deref() {
                output::line(format_args!(
                    "\nsaved as `{}` — `fathom search --list` prints the names, and `,` offers them \
                     in the shell",
                    name.trim().replace(' ', "-")
                ));
            }
        }
        Ok(ExitCode::Ok)
    }
}

/// The saved searches, in the order the palette prints them.
#[cfg(feature = "sqlite-bundled")]
fn list(store: &fathom_store::spine::Store, as_json: bool) -> eyre::Result<ExitCode> {
    let rows = store
        .ui_state_rows(history::SAVED_PREFIX)
        .map_err(crate::verbs::failed)?;
    let entries: Vec<(String, String)> = rows
        .iter()
        .map(|(key, value, _)| (key.clone(), value.clone()))
        .collect();
    let saved = history::saved(&entries);
    if as_json {
        let payload = serde_json::json!({
            "contract": CONTRACT,
            "action": "list",
            "saved": saved
                .iter()
                .map(|item| serde_json::json!({ "name": item.name, "query": item.query }))
                .collect::<Vec<_>>(),
        });
        output::line(format_args!("{payload}"));
        return Ok(ExitCode::Ok);
    }
    if saved.is_empty() {
        output::line(format_args!(
            "no saved searches yet — `fathom search --save risky \"risk>60\"` makes one"
        ));
        return Ok(ExitCode::Ok);
    }
    for item in &saved {
        output::line(format_args!("{:<20} {}", item.name, item.query));
    }
    output::line(format_args!(
        "\n{} saved; `,` lists them in the shell",
        saved.len()
    ));
    Ok(ExitCode::Ok)
}

/// The text report: what was asked, what answered, and what the answer could not cover.
///
/// Printed in the order a reader uses it — the counts first, so "3 of 41" is seen before the three rows
/// and a thin answer is a thin answer at a glance — then the hits grouped by kind with their per-token
/// explanation, then the gaps and notes, which are the part that stops an empty list being read as an
/// empty repository.
#[cfg(feature = "sqlite-bundled")]
fn render(report: &fathom_analyze::search::Report) -> Vec<String> {
    let mut out = vec![
        format!("search  {}", report.query),
        format!(
            "{} of {} candidates in a corpus of {} rows · {} scored · {} µs",
            report.hits.len(),
            report.candidates,
            report.documents,
            report.scored,
            report.micros
        ),
    ];
    if !report.filters.is_empty() {
        out.push(format!("filters  {}", report.filters.join(" ")));
    }
    let mut group: Option<fathom_analyze::search::Kind> = None;
    for hit in &report.hits {
        if group != Some(hit.kind) {
            group = Some(hit.kind);
            out.push(String::new());
            out.push(format!("{}s", hit.kind.as_str()));
        }
        let mut line = format!("  {}", hit.label);
        if !hit.detail.is_empty() {
            line.push_str(&format!("  ·  {}", hit.detail));
        }
        out.push(line);
        if !hit.why.is_empty() {
            out.push(format!("     why: {}", hit.why.join(" · ")));
        }
        if let Some(episode) = &hit.episode {
            out.push(format!("     episode {}", short(episode)));
        }
    }
    if report.hits.is_empty() {
        out.push(String::new());
        out.push("nothing matched".to_owned());
    }
    for note in &report.notes {
        out.push(format!("! {note}"));
    }
    for gap in &report.gaps {
        out.push(format!("! {}: {}", gap.kind.as_str(), gap.because));
    }
    if report.truncated {
        out.push(format!(
            "! the limit cut the answer short — {} matched and only {} are printed",
            report.scored,
            report.hits.len()
        ));
    }
    out
}

/// The machine report, one value, contract first.
#[cfg(feature = "sqlite-bundled")]
fn payload(report: &fathom_analyze::search::Report) -> String {
    use fathom_analyze::search::Destination;

    let hits: Vec<serde_json::Value> = report
        .hits
        .iter()
        .map(|hit| {
            let goes = match hit.destination() {
                Destination::Timeline { episode } => {
                    serde_json::json!({ "view": "timeline", "episode": episode })
                }
                Destination::Diff { episode, path } => {
                    serde_json::json!({ "view": "diff", "episode": episode, "path": path })
                }
                Destination::Deps { episode, name } => {
                    serde_json::json!({ "view": "deps", "episode": episode, "name": name })
                }
                Destination::None { because } => {
                    serde_json::json!({ "view": "none", "because": because })
                }
            };
            serde_json::json!({
                "kind": hit.kind.as_str(),
                "id": hit.id,
                "label": hit.label,
                "detail": hit.detail,
                "episode": hit.episode,
                "risk": hit.risk,
                "score": hit.score,
                "matched_on": hit.matched_on,
                "why": hit.why,
                "snippet": hit.snippet,
                "goes_to": goes,
            })
        })
        .collect();
    let payload = serde_json::json!({
        "contract": CONTRACT,
        "query": report.query,
        "tokens": report.tokens,
        "filters": report.filters,
        "unknown_filters": report.unknown,
        "notes": report.notes,
        "documents": report.documents,
        "candidates": report.candidates,
        "scored": report.scored,
        "scanned": report.scanned,
        "truncated": report.truncated,
        "micros": report.micros,
        "groups": report.groups.iter().map(|(kind, count)| serde_json::json!({
            "kind": kind.as_str(), "count": count })).collect::<Vec<_>>(),
        "gaps": report.gaps.iter().map(|gap| serde_json::json!({
            "kind": gap.kind.as_str(), "state": gap.state, "because": gap.because,
            "owner": gap.owner })).collect::<Vec<_>>(),
        "hits": hits,
    });
    payload.to_string()
}

/// The first eight characters of an id, which is how every surface in this product shortens one.
#[cfg(feature = "sqlite-bundled")]
fn short(id: &str) -> String {
    id.chars().take(8).collect()
}
