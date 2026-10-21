//! The store→frame read: the episodes the shell opens with (`reco-wzt`).
//!
//! Before this module `fathom-cli` built every frame with `episodes: None`, so no pane ever received an
//! episode and a reviewer on a repository whose index held 500 of them was told, in all six
//! episode-shaped panes, that nothing was available. The only read anyone had written for the purpose
//! was [`fathom_analyze::insight::episodes`], which is the *delta model's* read — `e2.11`, an Epic 2
//! ticket Epic 1 does not depend on — and Epic 1's exit criterion ("a human reviews a 500-commit agent
//! repo entirely inside the TUI") was therefore unsatisfiable by construction.
//!
//! A rail row does not need the delta model. Identity, authorship, scale, paths, session/turn and
//! review state are rows the spine already writes (ADR-003 §2, ADR-004 App. A §4.3), and
//! [`fathom_tui::timeline::EpisodeSource`]'s own doc says the store-backed source is `fathom-cli`'s to
//! implement. This module is that read: it fills a [`fathom_tui::timeline::Batch`] from the spine and
//! hands it to the shell in [`crate::tui`], which is the composition root's job precisely because the
//! shell may not open a store (ADR-002 §5.2, lint `E01`).
//!
//! What the store does not know stays `None` and prints `?` — the card's `delta` row stays the stated
//! not-in-this-build — so opening the data path does not quietly become a claim that `e2.11` landed.
//! [`fathom_analyze::insight::status`]'s `views_ready` is still false, and this module does not change
//! it. The one column that used to be `?` and is not any more is risk (`reco-c05`): the spine cannot
//! rank by findings it has not computed, but it can rank by size, surfaces, deletions and breadth, and
//! it does — [`risk_of`] is the seam between the store's rows and `fathom_analyze::risk`.
//!
//! # `fathom episodes` — the same rows, for a script
//!
//! `fathom show` takes one revspec and answers about one episode; nothing printed the *list*. So the
//! reviewer's main list — the rail, the first thing the product shows — was a fact that only existed on a
//! screen, which is what ADR-006 §6.7 forbids and what `reco-11b` filed. The verb here is the read above
//! asked from a shell that takes no terminal: the same [`EpisodeQuery`] the shell filters with, the same
//! store rows, the same review states. `/` and `fathom episodes` cannot disagree about how many episodes
//! there are because there is exactly one episode read in this binary, and it is in this file.
//!
//! Three rules hold it together:
//!
//! * **The page is the façade's.** ADR-002 §5.3 gives the memory budget to `insight`, so the size of a
//!   first frame is a [`PageWindow`], clamped there. A rail that holds part of the index says
//!   `index holds N` in its strip rather than letting the reviewer conclude the repository was quiet.
//! * **A first frame is four queries.** One episode read, one review-state read, one path join and one
//!   per-file rollup — not one per row (`Store::episode_paths` and `Store::episode_files` are written
//!   for that reason, and say so).
//! * **An empty result is not this module's story.** No store, a store the caller could not open
//!   either, or an index with nothing live in it all return `None`, which is the signal `reco-1nu` built
//!   the empty states around: the shell prints what the caller knows about the index instead. A store the
//!   caller *did* read, and this read then could not open or could not query, returns an empty batch
//!   carrying the reason, because that reason is more specific than anything the shell can guess.

// The store reads serve both surfaces, so they are gated with the backend and not with the shell
// (`reco-11b`): a `minimal` binary compiles this module down to the one refusal it can print.
#[cfg(feature = "sqlite-bundled")]
use fathom_store::episode::{EpisodeQuery, EpisodeRow, ReviewState as StoredState};
#[cfg(feature = "sqlite-bundled")]
use fathom_store::error::Error as StoreError;
#[cfg(feature = "sqlite-bundled")]
use fathom_store::spine::Store;
#[cfg(feature = "sqlite-bundled")]
use std::collections::BTreeMap;
#[cfg(feature = "sqlite-bundled")]
use std::path::Path;

// The shell's shapes, gated with the shell: the frame types only exist in a build that can draw a frame.
// `PAGE_LIMIT_MAX` is shared — the shell clamps a frame to it and `fathom episodes` says out loud when a
// list is longer than a frame could hold.
#[cfg(any(feature = "tui", feature = "sqlite-bundled"))]
use fathom_analyze::insight::PAGE_LIMIT_MAX;
#[cfg(feature = "tui")]
use fathom_analyze::insight::PageWindow;
use fathom_core::exit::ExitCode;
#[cfg(feature = "tui")]
use fathom_core::time::Timestamp;
#[cfg(feature = "tui")]
use fathom_tui::index::Facts;
#[cfg(feature = "tui")]
use fathom_tui::timeline::{AuthorKind, Batch, Episode, ReviewState};

/// The page a first frame asks for: the largest one the façade will ever answer with, so the shell
/// opens with as much of the index as any surface is allowed to hold and the strip names the rest.
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
const FRAME_PAGE: u32 = PAGE_LIMIT_MAX;

/// The episodes the shell should open with, or `None` when the index has none to give.
///
/// `facts` is the same read `reco-1nu` added for the empty states, passed in rather than re-derived so
/// the strip's origin, the status row's epoch and the rail's rows cannot disagree about which epoch
/// they came from.
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
#[must_use]
pub fn load(root: &Path, facts: &Facts) -> Option<Batch> {
    // Absent, unreadable, or built without a backend: the sentence the shell prints for those is written
    // in `fathom_tui::index`, and it is better than anything this module could invent.
    if !facts.is_present() {
        return None;
    }
    let store = match Store::open(&crate::session::store_path(root)) {
        Ok(store) => store,
        // The caller opened this store moments ago to read `facts`, so a failure here is a fact about
        // the store, not about this build: say it instead of letting the rail print a general empty state
        // over a store that was there seconds earlier.
        Err(why) => {
            return Some(Batch::empty(
                facts.origin(),
                format!("the store would not open for the episode read: {why}"),
            ));
        }
    };
    let page = PageWindow::new(None, Some(FRAME_PAGE));
    let raw = match read(&store, &page) {
        Err(why) => {
            // Stated, not swallowed: an empty rail that says *why* is a fact the reviewer can act on,
            // and an empty rail that says "the timeline lists them" would now be a lie (ADR-001 §7.2).
            return Some(Batch::empty(
                facts.origin(),
                format!("the episode read failed: {why}"),
            ));
        }
        Ok(raw) if raw.rows.is_empty() => return None,
        Ok(raw) => raw,
    };
    let Raw {
        rows,
        states,
        paths,
        files,
    } = raw;
    let held = u64::try_from(rows.len()).unwrap_or(u64::MAX);
    let mut episodes: Vec<Episode> = rows
        .into_iter()
        .map(|row| {
            let state = states.get(&row.id).copied().unwrap_or_default();
            let touched = paths.get(&row.id).cloned().unwrap_or_default();
            let stats = files.by_episode.get(&row.id).map(Vec::as_slice);
            into_episode(row, state, touched, stats, files.truncated)
        })
        .collect();
    rank_page(&mut episodes);
    Some(Batch {
        origin: facts.origin(),
        episodes,
        // What the index holds, not what this page holds: the strip prints the difference, and the
        // count came from the same store in the same breath (`ADR-001 §7.2`).
        total: Some(facts.episodes.unwrap_or(held).max(held)),
        epoch: facts.epoch,
        why_empty: None,
    })
}

/// This binary cannot read a store, so the shell's empty state says so (`fathom --version`).
#[cfg(all(feature = "tui", not(feature = "sqlite-bundled")))]
#[must_use]
pub fn load(_root: &Path, _facts: &Facts) -> Option<Batch> {
    None
}

/// The three reads one page of the rail needs, kept together so "the read failed" is one branch.
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
struct Raw {
    /// The live episodes the page selects, newest first.
    rows: Vec<EpisodeRow>,
    /// Every review marker, keyed by episode id.
    states: BTreeMap<String, StoredState>,
    /// The paths each selected episode touched.
    paths: BTreeMap<String, Vec<String>>,
    /// What happened to each path (`reco-c05`): the per-file adds and deletes the ranking reads, and
    /// whether the read had to stop early.
    files: fathom_store::episode::EpisodeFiles,
}

/// Run them. The order is deliberate: the path join is keyed by the ids the episode read returned.
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
fn read(store: &Store, page: &PageWindow) -> Result<Raw, StoreError> {
    let rows = store.episodes(&EpisodeQuery {
        offset: i64::from(page.offset()),
        limit: i64::from(page.limit()),
        ..EpisodeQuery::default()
    })?;
    let ids: Vec<String> = rows.iter().map(|row| row.id.clone()).collect();
    Ok(Raw {
        states: store.review_states()?,
        paths: store.episode_paths(&ids)?,
        files: store.episode_files(&ids)?,
        rows,
    })
}

/// One store row, in the shape the rail draws.
///
/// The two unit conversions worth naming: the store keeps millis and the rail takes [`Timestamp`]
/// microseconds (the same conversion `e1.11` uses for session stamps), and the review-state enum is
/// spelled identically in both crates on purpose, so the conversion is the shared spelling rather than a
/// match that can drift.
///
/// The `delta` field is the one that is not a copy of a column: it is [`fathom_analyze::insight`]'s
/// versioned model, composed from the row and the per-file rows this page already read — no extra query,
/// which is the only reason a rail is allowed to carry a model at all. The card renders that document and
/// nothing else, the risk column reads the ranking inside it, and `fathom analyze <episode>` prints the
/// same structure for the same episode. The composition root no longer formats a single analysis figure
/// of its own; `e2.10`'s card and `reco-c05`'s reasons moved into the façade with `e2.11`, and that is the
/// whole content of this function's diff (ADR-001 §7.10, ADR-002 §5.3).
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
fn into_episode(
    row: EpisodeRow,
    state: StoredState,
    paths: Vec<String>,
    stats: Option<&[fathom_store::episode::EpisodeFile]>,
    partial: bool,
) -> Episode {
    let scale = |value: i64| u64::try_from(value).unwrap_or(u64::MAX);
    let micros = |millis: i64| Timestamp::from_unix_micros(millis.saturating_mul(1_000));
    let delta = fathom_analyze::insight::for_rows(
        &row,
        stats.unwrap_or_default(),
        match state {
            StoredState::Unread => fathom_store::episode::ReviewState::Unread,
            StoredState::Read => fathom_store::episode::ReviewState::Read,
            StoredState::Accepted => fathom_store::episode::ReviewState::Accepted,
            StoredState::Flagged => fathom_store::episode::ReviewState::Flagged,
            StoredState::Dismissed => fathom_store::episode::ReviewState::Dismissed,
            StoredState::Snoozed => fathom_store::episode::ReviewState::Snoozed,
        },
        partial,
    );
    Episode {
        id: row.id,
        fingerprint: row.fingerprint,
        fallback_fingerprint: row.fallback_fingerprint,
        shape_fingerprint: row.shape_fingerprint,
        members: row.members,
        created: micros(row.created_utc),
        last: micros(row.last_utc),
        author: AuthorKind::parse(&delta.episode.author_kind),
        author_name: delta.episode.author_name.clone(),
        author_confidence: delta.episode.author_confidence,
        summary: row.summary,
        paths,
        n_files: scale(row.n_files),
        n_add: scale(row.n_add),
        n_del: scale(row.n_del),
        n_binary: scale(row.n_binary),
        // The queue's third factor, and the honest answer this build has: nothing has measured what
        // depends on this change. `e3.1`/`e3.2` own the module graph; until it exists the composition root
        // says so rather than guessing from path prefixes, and the queue's strip prints the gap
        // (`e4.5`, `reco-wsj`).
        blast: fathom_analyze::insight::BlastInput::Unmeasured,
        state: ReviewState::parse(state.as_str()),
        session: row.session_id,
        turn: row.turn_id,
        boundary_rule: (!delta.episode.boundary_rule.is_empty())
            .then(|| delta.episode.boundary_rule.clone()),
        boundary_confidence: delta.episode.boundary_confidence,
        // The ranking lives inside the delta. Ranks 1..=N are filled by [`rank_page`] once every row of
        // the page exists; a score is meaningless among its peers until there are peers to compare it with.
        delta: Some(delta),
        live: row.live,
    }
}

/// Give every row of the page its place in the page: `risk.rank` / `risk.ranked`.
///
/// The order is the queue's order and is stated here rather than trusted from the shell because the
/// number printed in a row has to agree with the position of that row in the list, and the two are
/// computed in different crates. A row that is unranked (the file read never reached it) is not
/// counted, so "rank 3 of 212" never has holes in it.
#[cfg(all(feature = "tui", feature = "sqlite-bundled"))]
fn rank_page(episodes: &mut [Episode]) {
    let mut places: Vec<usize> = episodes
        .iter()
        .enumerate()
        .filter(|(_, episode)| episode.risk().is_some())
        .map(|(index, _)| index)
        .collect();
    let score = |episode: &Episode| episode.risk().map_or(0, |risk| risk.score);
    places.sort_by(|a, b| {
        let (left, right) = (&episodes[*a], &episodes[*b]);
        score(right)
            .cmp(&score(left))
            .then_with(|| right.size().cmp(&left.size()))
            .then_with(|| right.created.cmp(&left.created))
            .then_with(|| left.id.cmp(&right.id))
    });
    let ranked = places.len();
    for (place, index) in places.into_iter().enumerate() {
        if let Some(risk) = episodes[index]
            .delta
            .as_mut()
            .and_then(|delta| delta.risk.as_mut())
        {
            risk.rank = i64::try_from(place + 1).ok();
            risk.ranked = Some(i64::try_from(ranked).unwrap_or(i64::MAX));
        }
    }
}

// ---------------------------------------------------------------------------
// `fathom episodes` — the rail's rows, printed for a script, a CI step or a bug report (`reco-11b`)
// ---------------------------------------------------------------------------

use crate::verbs::EpisodesArgs;

/// The contract id the `--json` payload carries, mirrored by `schemas/episodes.schema.json` and pinned by
/// the round-trip test in `crates/fathom-cli/tests/schemas.rs`.
pub const CONTRACT: &str = "fathom.cli.episodes/1";

/// The author kinds `--kind` accepts, in the store's spelling.
#[cfg(feature = "sqlite-bundled")]
const KINDS: [&str; 3] = ["agent", "human", "unknown"];

/// Every review state, in the store's spelling and in the order both reports print them.
#[cfg(feature = "sqlite-bundled")]
const STATES: [&str; 6] = [
    "unread",
    "read",
    "accepted",
    "flagged",
    "dismissed",
    "snoozed",
];

/// How wide the summary column may get, in characters. A longer subject is cut with an ellipsis; the whole
/// subject is in `--json`.
#[cfg(feature = "sqlite-bundled")]
const SUMMARY_WIDTH: usize = 72;

/// `fathom episodes`.
///
/// # Errors
///
/// Exit `2` for a flag *value* that names nothing (`--kind sideways`, `--state never`, `--since someday`),
/// exit `3` when `--root` is not a repository, exit `12` when the repository has no index to list
/// (ADR-002 §7.3 — the code every store-backed verb uses for "run `fathom index`", with the same remedy).
///
/// An index with no episodes in it is not an error: it is the empty state, printed as a zero with the filter
/// that produced it, and exit 0. A verb that exits non-zero because a repository is quiet would make every
/// script that calls it special-case the quiet repository.
#[cfg(feature = "sqlite-bundled")]
pub fn run(args: &EpisodesArgs) -> eyre::Result<ExitCode> {
    use crate::output;
    use crate::verbs::{Pending, failed};
    use fathom_store::episode::EpisodeQuery;
    use fathom_store::spine::Store;

    let root = Path::new(&args.root);
    let redactor = crate::sanitize::Redactor::for_root(root);

    // The command line is judged before the repository is opened: a `--state` that is not a state is the
    // reviewer's typo whether or not there is an index to read (`fathom search` keeps the same order of
    // advice for the same reason — "go and build an index so that we can then refuse your argument" is the
    // wrong sentence to print first).
    for kind in &args.kinds {
        if !KINDS.contains(&kind.as_str()) {
            return Err(Pending::usage(
                format!("`--kind {kind}` is not an author kind"),
                format!(
                    "the store knows {} — `fathom attribution list` says who this index thinks wrote what",
                    KINDS.join(", ")
                ),
            )
            .into());
        }
    }
    let state = match args.state.as_deref() {
        Some(text) => {
            let name = text.trim();
            if !STATES.contains(&name) {
                return Err(Pending::usage(
                    format!("`--state {text}` is not a review state"),
                    format!(
                        "the states are {} — `fathom review status` prints what the ledger holds",
                        STATES.join(", ")
                    ),
                )
                .into());
            }
            // `--state unread` means "nobody has decided", which is not the same question as "the row says
            // unread": this is the store's own filter, so the count matches the shell's `u`.
            Some(StoredState::parse(name))
        }
        None => None,
    };
    let now = fathom_core::time::Timestamp::from_unix_micros(
        crate::clock::now_millis().saturating_mul(1_000),
    );
    let when = |spec: &str, flag: &str| -> Result<Option<i64>, eyre::Report> {
        match fathom_analyze::search::query::parse_when(spec, now) {
            Ok(at) => Ok(at.map(|at| at.as_unix_micros() / 1_000)),
            Err(why) => Err(Pending::usage(format!("`{flag} {spec}` is not a date"), why).into()),
        }
    };
    let since_utc = args
        .since
        .as_deref()
        .map(|spec| when(spec, "--since"))
        .transpose()?
        .flatten();
    let until_utc = args
        .until
        .as_deref()
        .map(|spec| when(spec, "--until"))
        .transpose()?
        .flatten();

    // `--limit 0` is "every row the filters select", which is `EpisodeQuery`'s documented meaning of a
    // non-positive limit rather than SQLite's `LIMIT 0`.
    let query = EpisodeQuery {
        authors: args.author.clone(),
        kinds: args.kinds.clone(),
        state,
        path: args.path.clone().filter(|text| !text.is_empty()),
        since_utc,
        until_utc,
        offset: args.offset.max(0),
        limit: if args.limit <= 0 { -1 } else { args.limit },
        include_dead: args.include_dead,
    };

    // Then the repository (exit 3, the environment's problem), then the store (exit 12, the index's).
    fathom_ingest::repo::Repository::discover(root).map_err(failed)?;
    let store_path = fathom_analyze::index::default_store_path(root);
    if !store_path.is_file() {
        return Err(Pending::new(
            ExitCode::IndexDegraded,
            format!(
                "no index at {} yet, so there is no episode list to print",
                redactor.path(&store_path)
            ),
            Some(crate::state::REMEDY_INDEX.to_owned()),
        )
        .into());
    }
    let store = Store::open(&store_path).map_err(failed)?;
    let epoch = store.epoch().map_err(failed)?;
    let page = Page::read(&store, &query).map_err(|why| {
        Pending::new(
            ExitCode::StoreCorrupt,
            format!("reading the episode list: {why}"),
            Some(crate::state::REMEDY_REBUILD.to_owned()),
        )
    })?;
    let report = Report::new(page, args, &query, &redactor, root, &store_path, epoch);
    if args.json {
        output::line(format_args!("{}", payload(&report)));
    } else {
        for line in render(&report) {
            output::line(format_args!("{line}"));
        }
    }
    Ok(ExitCode::Ok)
}

/// This binary cannot read a store, so the verb names the feature it is missing (`fathom --version`).
#[cfg(not(feature = "sqlite-bundled"))]
pub fn run(args: &EpisodesArgs) -> eyre::Result<ExitCode> {
    let _ = args;
    Err(crate::verbs::feature_absent(
        "fathom episodes",
        "sqlite-bundled",
    ))
}

/// One printed row: the store's row, the reviewer's verdict, and the paths the episode touched.
///
/// One struct rather than three parallel collections, because the text render and the JSON render must not
/// be able to disagree about which fields a row has — only about how they are laid out.
#[cfg(feature = "sqlite-bundled")]
struct Row<'a> {
    episode: &'a EpisodeRow,
    state: StoredState,
    paths: &'a [String],
}

/// What one read pulled: a page of rows, the totals around them, and the two joins a row needs.
///
/// Everything in here is owned by the read, which is what lets the same page be rendered twice (a text run
/// and a JSON run of the same verb print from one store query, not two).
#[cfg(feature = "sqlite-bundled")]
struct Page {
    /// The episodes the query selected, in the store's order: newest `last_utc` first, then id.
    rows: Vec<EpisodeRow>,
    /// Every review marker in the store, keyed by episode id.
    states: BTreeMap<String, StoredState>,
    /// The distinct paths each selected episode touched.
    paths: BTreeMap<String, Vec<String>>,
    /// Episodes the filters select, before paging — what `--state unread` counted.
    matched: i64,
    /// Episodes the index holds, whatever the filters: the number the rail's strip prints.
    held: i64,
    /// Every state's share of the filtered selection, in [`STATES`] order. The `--state` filter itself is
    /// not applied to this breakdown, so `--kind agent --state unread` still prints the whole agent split.
    counts: Vec<(&'static str, i64)>,
}

#[cfg(feature = "sqlite-bundled")]
impl Page {
    /// The page, the two totals, the review states and the path join — four reads and six counts, none of
    /// them per row. The same set [`read`] performs for a frame, minus the per-file rollup the rail needs for
    /// its risk column and a list does not.
    fn read(store: &Store, query: &EpisodeQuery) -> Result<Page, StoreError> {
        let rows = store.episodes(query)?;
        let matched = store.count_episodes(query)?;
        let held = store.count_episodes(&EpisodeQuery {
            include_dead: query.include_dead,
            ..EpisodeQuery::default()
        })?;
        let states = store.review_states()?;
        let ids: Vec<String> = rows.iter().map(|row| row.id.clone()).collect();
        let paths = store.episode_paths(&ids)?;
        let mut counts = Vec::with_capacity(STATES.len());
        for name in STATES {
            // The count SQL ignores paging, so this is the whole selection's breakdown and not the page's.
            let counted = store.count_episodes(&EpisodeQuery {
                state: Some(StoredState::parse(name)),
                ..query.clone()
            })?;
            counts.push((name, counted));
        }
        Ok(Page {
            rows,
            states,
            paths,
            matched,
            held,
            counts,
        })
    }

    /// Zip the three reads back into rows. Built on demand, because the two renderers ask for it once each.
    fn rows(&self) -> Vec<Row<'_>> {
        self.rows
            .iter()
            .map(|episode| Row {
                episode,
                state: self.states.get(&episode.id).copied().unwrap_or_default(),
                paths: self.paths.get(&episode.id).map_or(&[][..], Vec::as_slice),
            })
            .collect()
    }

    /// How many of the selected rows belong to no live commit — which is what `--include-dead` lets in.
    fn dead(&self) -> usize {
        self.rows.iter().filter(|row| !row.live).count()
    }

    /// What the report has to say that is not a row: the truncation, the ghosts, the empty answer.
    ///
    /// Both surfaces print this one list, because "the page is shorter than the index" is a fact a script
    /// needs as much as a human does (ADR-001 §7.2, and the whole reason `reco-11b` exists).
    fn notes(&self, query: &EpisodeQuery) -> Vec<String> {
        let mut out = Vec::new();
        if self.rows.is_empty() {
            // Three different noes, and a script should not have to tell them apart by arithmetic: the index
            // is empty, the filters selected nothing, or the page starts past the end of the selection.
            out.push(if self.held == 0 {
                "nothing in the index yet — `fathom index` at this root wrote no episodes"
                    .to_owned()
            } else if self.matched > 0 && query.offset > 0 {
                let matched = self.matched;
                format!(
                    "--offset {} is past the end: the filters select {matched}",
                    query.offset
                )
            } else {
                format!(
                    "no episode matches the filters — {} of {} excluded",
                    self.held.saturating_sub(self.matched),
                    self.held
                )
            });
        } else if self.matched as usize > self.rows.len() {
            out.push(format!(
                "the page is what was printed: {} selected, {} shown — raise --limit or page with --offset",
                self.matched,
                self.rows.len()
            ));
        } else if self.held > self.matched {
            out.push(format!(
                "the filters excluded {} of the {} episodes the index holds",
                self.held - self.matched,
                self.held
            ));
        }
        if query.include_dead {
            // Stated both ways: a script asking for ghosts and getting none needs to know the index really
            // has none, as much as it needs to hear which rows are ghosts. The pruner is what makes the
            // second half common — it removes a dead episode nobody decided on (ADR-004 §8.2).
            out.push(if self.dead() == 0 {
                "no row belongs to no live commit: nothing was rewritten away, or the pruner removed what \
                 was".to_owned()
            } else {
                format!(
                    "{} of the {} rows printed belong to no live commit — a rebase, an amend or a \
                     force-push replaced them",
                    self.dead(),
                    self.rows.len()
                )
            });
        }
        if self.rows.len() > PAGE_LIMIT_MAX as usize {
            out.push(format!(
                "this list is longer than a frame: the shell holds at most {PAGE_LIMIT_MAX} rows at a \
                 time, so {} of these are on no screen",
                self.rows.len() - PAGE_LIMIT_MAX as usize
            ));
        }
        out
    }
}

/// A page plus everything a report needs to say where the rows come from.
#[cfg(feature = "sqlite-bundled")]
struct Report<'a> {
    page: Page,
    args: &'a EpisodesArgs,
    query: &'a EpisodeQuery,
    /// `--root`, redacted for printing (ADR-002 §7.5: a report is pasteable or it is useless).
    root: String,
    /// The store's path, repository-relative.
    store: String,
    epoch: i64,
    /// The flags that narrowed the list, in one string; empty means none.
    filters: String,
    /// What the report has to say that is not a row.
    notes: Vec<String>,
}

#[cfg(feature = "sqlite-bundled")]
impl<'a> Report<'a> {
    fn new(
        page: Page,
        args: &'a EpisodesArgs,
        query: &'a EpisodeQuery,
        redactor: &crate::sanitize::Redactor,
        root: &Path,
        store_path: &Path,
        epoch: i64,
    ) -> Report<'a> {
        let notes = page.notes(query);
        Report {
            filters: filter_text(args, query),
            root: redactor.path(root),
            store: redactor.path(store_path),
            notes,
            page,
            args,
            query,
            epoch,
        }
    }

    /// The `--limit` as the reviewer typed it, with `none` for "every row the filters select".
    fn limit(&self) -> String {
        if self.args.limit <= 0 {
            "none".to_owned()
        } else {
            self.args.limit.to_string()
        }
    }
}

/// The text report, in the order a reader uses it: the index, the page, the filters, the state breakdown,
/// then the rows in the rail's own order, then what the rows cannot say.
#[cfg(feature = "sqlite-bundled")]
fn render(report: &Report) -> Vec<String> {
    let mut out = vec![
        format!(
            "index      {} · epoch {} · {} episode(s)",
            report.store, report.epoch, report.page.held
        ),
        format!(
            "selected   {} of {} · offset {} · limit {}",
            report.page.rows.len(),
            report.page.matched,
            report.args.offset.max(0),
            report.limit()
        ),
    ];
    if !report.filters.is_empty() {
        out.push(format!("filters    {}", report.filters));
    }
    let counts: Vec<String> = report
        .page
        .counts
        .iter()
        .filter(|(_, count)| *count > 0)
        .map(|(name, count)| format!("{count} {name}"))
        .collect();
    out.push(format!(
        "states     {}",
        if counts.is_empty() {
            "nothing in the selection".to_owned()
        } else {
            counts.join(" · ")
        }
    ));
    out.push(String::new());
    out.push(format!(
        "{:<24}  {:<22}  {:<7}  {:>8} {:>7} {:>6}  {:<10}  {:<18}  {}",
        "ID", "WHEN (UTC)", "KIND", "+ADD", "-DEL", "FILES", "STATE", "SESSION/TURN", "SUMMARY"
    ));
    for row in report.page.rows() {
        out.push(format!(
            "{:<24}  {:<22}  {:<7}  {:>8} {:>7} {:>6}  {:<10}  {:<18}  {}",
            row.episode.id,
            stamp(row.episode.last_utc),
            row.episode.author_kind,
            format!("+{}", row.episode.n_add),
            format!("-{}", row.episode.n_del),
            row.episode.n_files,
            row.state.as_str(),
            session_turn(row.episode),
            printable(&row.episode.summary, SUMMARY_WIDTH)
        ));
    }
    if !report.notes.is_empty() {
        out.push(String::new());
        out.extend(report.notes.iter().map(|note| format!("! {note}")));
    }
    out
}

/// The machine report: one value, contract first, and no clock of its own, so two runs over the same index
/// are byte-identical and a CI step can diff them.
#[cfg(feature = "sqlite-bundled")]
fn payload(report: &Report) -> serde_json::Value {
    let episodes: Vec<serde_json::Value> = report.page.rows().iter().map(row_json).collect();
    let counts: serde_json::Map<String, serde_json::Value> = report
        .page
        .counts
        .iter()
        .map(|(name, count)| ((*name).to_owned(), serde_json::json!(count)))
        .collect();
    serde_json::json!({
        "contract": CONTRACT,
        "root": report.root,
        "index": {
            "present": true,
            "path": report.store,
            "epoch": report.epoch,
            "episodes": report.page.held,
        },
        "filter": {
            "text": report.filters,
            "authors": report.args.author,
            "kinds": report.args.kinds,
            "state": report.args.state,
            "path": report.args.path,
            "since": report.args.since,
            "until": report.args.until,
            "since_utc_ms": report.query.since_utc,
            "until_utc_ms": report.query.until_utc,
            "include_dead": report.args.include_dead,
        },
        "page": {
            "offset": report.args.offset.max(0),
            "limit": if report.args.limit <= 0 { None } else { Some(report.args.limit) },
            "returned": report.page.rows.len(),
            "matched": report.page.matched,
            "truncated": report.page.matched as usize > report.page.rows.len(),
            "order": "last_utc desc, id",
        },
        "review_counts": counts,
        "episodes": episodes,
        "notes": report.notes,
    })
}

/// One row, as JSON. The keys are the columns and nothing is invented to fill a gap: what the store does not
/// know is `null`, which is the whole reason there is a JSON form. The confidence figures are the store's own
/// numbers passed through, because `fathom analyze` on the same episode prints those same numbers and a list
/// that rescaled them would be a second reality (ADR-001 §7.10).
#[cfg(feature = "sqlite-bundled")]
fn row_json(row: &Row) -> serde_json::Value {
    let episode = row.episode;
    serde_json::json!({
        "id": episode.id,
        "kind": episode.kind,
        "live": episode.live,
        "summary": episode.summary,
        "created_utc_ms": episode.created_utc,
        "last_utc_ms": episode.last_utc,
        "created": stamp(episode.created_utc),
        "last": stamp(episode.last_utc),
        "author_kind": episode.author_kind,
        "author_name": episode.author_name,
        "author_email": episode.author_email,
        "author_confidence": episode.author_confidence,
        "attribution_source": (!episode.attribution_source.is_empty())
            .then(|| episode.attribution_source.clone()),
        "attribution_agent": (!episode.attribution_agent.is_empty())
            .then(|| episode.attribution_agent.clone()),
        "n_commits": episode.n_commits(),
        "n_files": episode.n_files,
        "n_add": episode.n_add,
        "n_del": episode.n_del,
        "n_binary": episode.n_binary,
        "paths": row.paths,
        "review_state": row.state.as_str(),
        "session": episode.session_id,
        "turn": episode.turn_id,
        "turn_id": turn_id(episode),
        "first_oid": episode.first_oid,
        "last_oid": episode.last_oid,
        "boundary_rule": (!episode.boundary_rule.is_empty())
            .then(|| episode.boundary_rule.clone()),
        "boundary_confidence": episode.boundary_confidence,
        "intent_state": (!episode.intent_state.is_empty())
            .then(|| episode.intent_state.clone()),
        "overridden": episode.overridden,
    })
}

/// The flags that narrowed the list, in one string, so a report says what it filtered without the reader
/// having to remember what they typed. Empty means "no filter", and prints no line.
#[cfg(feature = "sqlite-bundled")]
fn filter_text(args: &EpisodesArgs, query: &EpisodeQuery) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !args.author.is_empty() {
        parts.push(format!("author={}", args.author.join(",")));
    }
    if !args.kinds.is_empty() {
        parts.push(format!("kind={}", args.kinds.join(",")));
    }
    if let Some(state) = args.state.as_deref() {
        parts.push(format!("state={state}"));
    }
    if let Some(path) = &args.path {
        parts.push(format!("path={path}"));
    }
    if let Some(since) = &args.since {
        parts.push(format!("since={since}"));
    }
    if let Some(until) = &args.until {
        parts.push(format!("until={until}"));
    }
    if query.include_dead {
        parts.push("include_dead".to_owned());
    }
    parts.join(" ")
}

/// The session/turn column: the session-qualified turn id the store already holds (`sess-7:3`), and a
/// stated absence when there is not one (ADR-006 §10.3 — `no-session` is an answer where a blank is not).
#[cfg(feature = "sqlite-bundled")]
fn session_turn(episode: &EpisodeRow) -> String {
    match (&episode.session_id, &episode.turn_id) {
        (Some(_), Some(turn)) => turn.clone(),
        (Some(session), None) => format!("{session}:unknown"),
        (None, _) => "no-session".to_owned(),
    }
}

/// The session-qualified turn id, in the spelling `fathom hooks --trailer` prints and ADR-003 §18.2 **D5**
/// requires — which is what the store keeps, because `e6.1` qualifies the bare trailer counter at ingest
/// (ADR-005 §1: two sessions' turn 3 are `sess-7:3` and `ci-build:3`, never two things called `3`). `None`
/// when a trailer named no turn at all.
#[cfg(feature = "sqlite-bundled")]
fn turn_id(episode: &EpisodeRow) -> Option<String> {
    episode.turn_id.clone()
}

/// A store timestamp in the shape `fathom tui --show-session` already prints its stamps — `YYYY-MM-DD HH:MM
/// UTC`. The store keeps milliseconds and [`Timestamp`] is microseconds, which is the only arithmetic here.
#[cfg(feature = "sqlite-bundled")]
fn stamp(millis: i64) -> String {
    let stamp = fathom_core::time::Timestamp::from_unix_micros(millis.saturating_mul(1_000));
    let (year, month, day) = stamp.civil();
    let (hour, minute, _) = stamp.time_of_day();
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02} UTC")
}

/// One field on one line of a table: whitespace folded, control characters — including the escape byte a
/// hostile commit subject could hide in, which is the reason this is not a plain `to_owned` — replaced, and
/// the result cut at `width` characters with an ellipsis inside the width.
///
/// `--json` prints the raw subject; this is only about what a terminal is asked to render.
#[cfg(feature = "sqlite-bundled")]
fn printable(text: &str, width: usize) -> String {
    let mut folded = String::with_capacity(text.len().min(width.max(16)));
    let mut space = false;
    for ch in text.chars() {
        let next = if ch.is_whitespace() || ch.is_control() {
            ' '
        } else {
            ch
        };
        if next == ' ' && space {
            continue;
        }
        space = next == ' ';
        folded.push(next);
    }
    let trimmed = folded.trim();
    if trimmed.chars().count() <= width {
        return trimmed.to_owned();
    }
    let cut: String = trimmed.chars().take(width.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}
