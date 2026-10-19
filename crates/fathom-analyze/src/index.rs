//! `index.rs` — the incremental index (ADR-004 §6).
//!
//! The rule this file exists to enforce is §2's rule 3: a derived row is a pure function of a stable input
//! identity, and *"'invalidate X' is what happens automatically when the inputs that produce X change"*. So
//! there is no invalidation code in here. There is a planner that decides which inputs are worth asking
//! about, a loop that asks the cache for each one **by key**, and a loop that computes the ones that missed.
//! Editing one file changes one blob, which changes one `path_pair` digest, which misses while every other
//! key hits — not because anything noticed the edit, but because those keys were always different. That is
//! the whole mechanism, and the absence of an invalidation matrix is the point: a matrix is a thing someone
//! forgets a row of.
//!
//! Three passes, in this order:
//!
//! 1. **the spine** ([`fathom_ingest::ingest`]): commit and file rows. Its cache *is* the spine — "does
//!    `git_commit` already have this oid" is the lookup for `ingest.normalize` — and §4.5 says explicitly
//!    not to store a second copy of a result in `cache` when it has a dedicated table. Its counters
//!    therefore come from [`record_spine`], which is what lets `--stats` print a truthful row for it.
//! 2. **the analyses**: `diff.per_file` at `path_pair` scope and `structure.delta` at `tree` scope, one
//!    `cache` row each, read and written through [`Store::resolve`] and [`Store::put`].
//! 3. **the worktree** (§6.4): uncommitted paths, analysed exactly like committed ones and keyed the same
//!    way, so a staged analysis *is* the committed analysis once the commit lands (§6.4 R6) — which is the
//!    difference between "speculative work is cheap" and "speculative work is a second index".
//!
//! Crash recovery is not here either, on purpose: §9.2's `running → crashed` pass lives with the store
//! ([`crate::sink::StoreSink`]'s `begin`), and every unit below is idempotent, so a `kill -9` at any point
//! leaves a store the next run *finishes* rather than one it has to explain.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use fathom_ingest::diff::{self, Measurement};
use fathom_ingest::graph::WalkOptions;
use fathom_ingest::ingest::{self, IngestOptions, RunKind, RunReport};
use fathom_ingest::oid::Oid;
use fathom_ingest::record::{Hunk, Taxonomy, sha256_hex};
use fathom_ingest::repo::Repository;
use fathom_ingest::state::{Change as WorkChange, ScanOptions, Side, StateNotice, WorktreeKind};
use fathom_store::cache::{self, Config, Reason, Versions};
use fathom_store::spine::Store;
use serde::{Deserialize, Serialize};

use crate::cancel::Cancel;
use crate::error::Error;
use crate::sink::StoreSink;

/// The analyzers this driver runs, in the order `--stats` prints them.
///
/// A list rather than a discovery, because "what this run did" is a fact about the run and
/// [`cache::ANALYZERS`] is a fact about the build. `findings.detect` and `deps.resolve` are in the manifest
/// and not here — they arrive with `e1.6` and `e2.2` — and a report that inferred its rows from the manifest
/// would print a 0%-hit analyzer for work that never ran, which is the fastest way to make a warm cache look
/// broken.
pub const ANALYZED: &[&str] = &["ingest.normalize", "diff.per_file", "structure.delta"];

/// One worktree path the pass would not name an answer for, and the reason it declined (§7.6: a stated
/// absence, in the report rather than in a log).
type SkippedPath = (String, String);

/// Which commits' analyses to look up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    /// Only commits the spine does not already have — §6.1's R1, the default, and the reason a warm
    /// re-index costs what the change costs rather than what the repository costs.
    #[default]
    NewCommits,
    /// Every reachable commit: `--refresh`. The lookups all hit, and the counters are the whole point. This
    /// is the pass that *measures* §6's invalidation matrix — after a version bump or a config change it
    /// reports exactly the rows that moved, and nothing else.
    Everything,
}

/// What to do with uncommitted work (§6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Worktree {
    /// Analyse the staged and dirty paths as provisional records.
    #[default]
    Analyse,
    /// Skip them: what `--committed-only` means, and what a budget run may be told to do.
    Ignore,
}

/// Everything about *how* to index that is neither the repository nor the store.
#[derive(Debug, Clone)]
pub struct Options {
    /// The spine's options: which refs to walk, the size caps, the merge policy, the batching, and the
    /// new-commit budget that makes §7.6's stated partial reachable.
    pub ingest: IngestOptions,
    /// The `fathom.toml` table, as plain values; §5.5 digests only the keys each analyzer declared.
    pub config: Config,
    /// Analyzer version overrides — the knob that makes "what if `taxonomy.path` were version 3" a query
    /// rather than an edit to the source (§5.1).
    pub versions: Versions,
    /// Which commits to look up.
    pub scope: Scope,
    /// Recompute even when the lookup hits.
    ///
    /// What `fathom index --refresh` means. The lookups still happen and still count, so the report says "1
    /// looked up, 1 hit, 1 computed" — which is the honest description of a refresh, and the record of
    /// having *measured* §6's invalidation matrix rather than trusting it.
    pub refresh: bool,
    /// Whether to analyse the worktree.
    pub worktree: Worktree,
    /// How carefully to read the worktree.
    pub scan: ScanOptions,
    /// Stop after this many analyses *computed*. Lookups are unlimited and cheap; a budget that stops a run
    /// early is a stated partial, not a failure (§7.6).
    pub unit_budget: Option<usize>,
    /// Stop when this is triggered (§7.6). Checked between units rather than inside one: a unit is bounded
    /// by the size cap, which is what makes "instant" defensible instead of rhetorical.
    pub cancel: Option<Cancel>,
    /// The instant stamped into every row and counter. Required, because a crate below `fathom-cli` does
    /// not read the clock (ADR-002 §4.4); [`run`] refuses a `None` rather than quietly becoming a test that
    /// changes when the date does.
    pub now: Option<i64>,
    /// The episode boundary rules (`e1.3`): the gap, the agent markers and the forced splits/merges.
    ///
    /// These arrive as values because ADR-002 §4.5 forbids a crate below `fathom-cli` from reading a
    /// config file; the CLI turns `fathom.toml`'s `[[episodes.force-split]]` tables into this struct.
    pub segment: fathom_ingest::segment::Rules,
    /// Whether the run rebuilds the episode set (§4.3 of ADR-003's plan). On by default: every verb that
    /// reads episodes wants them present, and the step is idempotent.
    pub episodes: bool,
    /// Stop and park forever after this many analyses have been *computed*, without closing the run.
    ///
    /// A test seam, and only a test seam: it is how `tests/incremental_index.rs` gets a real `SIGKILL` into
    /// the middle of durable work at an exact point, instead of racing a timer against a loop whose speed
    /// depends on the machine. `None` in every real invocation.
    pub park_after_units: Option<usize>,
    /// Where to report progress while the run goes (`e5.9`).
    ///
    /// `None` is the default and is every headless caller: a tick is a callback into whoever is watching,
    /// and a library has nobody. `fathom index` installs one so a first index says what it is doing and
    /// how long it thinks that will take; `--json` installs nothing, because that stream belongs to the
    /// run report alone (ADR-002 §7.5).
    pub progress: Option<ProgressSink>,
}

/// Which part of the run a [`Tick`] is reporting on (`e5.9`).
///
/// Four, because a run has four things to do and a reviewer waiting on one of them wants to know which
/// one is taking the time. One percentage across all four would be worse than useless: the walk and the
/// analyses cost nothing alike, so `60 %` would mean two different waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Walking commits into the spine.
    Walking,
    /// Deriving the analyses for the commits that need them.
    Analysing,
    /// Reading the uncommitted worktree (§6.4).
    Worktree,
    /// Re-deriving the episode set (ADR-003 §4.3).
    Episodes,
}

impl Phase {
    /// The word the progress row prints for this phase.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            // The walk is the phase ADR-006 §10.3's row is written about — `indexing … commits` — and the
            // derivation pass gets its own word, because a run that announces `indexing 100 % · done`
            // and then spends four more seconds on the worktree and the episodes has lied twice.
            Phase::Walking => "indexing",
            Phase::Analysing => "analysing",
            Phase::Worktree => "reading the worktree",
            Phase::Episodes => "deriving episodes",
        }
    }

    /// What one unit of this phase is called, so the row says `4 102 / 6 552 commits` and not `units`.
    #[must_use]
    pub const fn unit(self) -> &'static str {
        match self {
            Phase::Walking | Phase::Analysing => "commit",
            Phase::Worktree => "path",
            Phase::Episodes => "episode",
        }
    }
}

/// One progress tick, handed to [`Options::progress`].
///
/// `done` counts the current phase's units and `of` is `None` for as long as the total is genuinely
/// unknown — the spine walk does not know how many commits a repository has until it has walked it, and
/// printing `0 %` for "not counted yet" is the lie ADR-006 §0.1 refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tick {
    /// Which pass is working.
    pub phase: Phase,
    /// Units of it finished.
    pub done: u64,
    /// Units of it there are, once known.
    pub of: Option<u64>,
}

/// Where a run reports its progress.
///
/// A boxed closure rather than a trait, because there is one thing to report and a trait would be a
/// second vocabulary for it. `Send + Sync` because the caller that wants progress (`fathom index`) runs
/// the index on a worker thread while the main thread prints. The manual [`Debug`] is what lets
/// [`Options`] keep deriving its own: the useful thing a debug line can say about a function is that one
/// is installed.
#[derive(Clone)]
pub struct ProgressSink(Arc<dyn Fn(&Tick) + Send + Sync + 'static>);

impl ProgressSink {
    /// A sink that calls `f` for every tick.
    #[must_use]
    pub fn new(f: impl Fn(&Tick) + Send + Sync + 'static) -> Self {
        ProgressSink(Arc::new(f))
    }

    /// Report one tick.
    pub fn report(&self, tick: &Tick) {
        (self.0)(tick);
    }
}

impl std::fmt::Debug for ProgressSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProgressSink(<installed>)")
    }
}

impl Default for Options {
    fn default() -> Self {
        Options {
            ingest: IngestOptions::default(),
            config: Config::empty(),
            versions: crate::build::manifest_versions(),
            scope: Scope::default(),
            refresh: false,
            worktree: Worktree::default(),
            scan: ScanOptions::default(),
            unit_budget: None,
            cancel: None,
            now: None,
            segment: fathom_ingest::segment::Rules::default(),
            episodes: true,
            park_after_units: None,
            progress: None,
        }
    }
}

impl Options {
    /// Options that walk `HEAD` and change nothing else.
    #[must_use]
    pub fn head() -> Self {
        Options {
            ingest: IngestOptions {
                walk: WalkOptions::head(),
                ..IngestOptions::default()
            },
            ..Options::default()
        }
    }
}

/// What one run did — the thing `fathom index --stats` prints.
#[derive(Debug, Clone)]
pub struct Report {
    /// Cold or warm, as the spine decided.
    pub kind: RunKind,
    /// Commits the walk emitted.
    pub commits_walked: usize,
    /// Commits the spine did not have.
    pub commits_new: usize,
    /// Commits whose analyses were looked up this run.
    pub commits_considered: usize,
    /// The spine's own numbers, kept whole: rows written, per-commit failures, notices, truncation.
    pub spine: RunReport,
    /// What the episode step did (`e1.3`); all zeroes when `Options::episodes` is off.
    pub episodes: crate::episodes::Report,
    /// Per-analyzer counters for *this run*, keyed by analyzer id.
    pub analyzers: BTreeMap<String, AnalyzerStats>,
    /// The provisional records (§6.4).
    pub worktree: Vec<ProvisionalRecord>,
    /// Worktree paths that could not be analysed, with the reason — a stated absence (§7.6).
    pub worktree_skipped: Vec<SkippedPath>,
    /// The run stopped before it was done.
    pub interrupted: bool,
    /// Which of the two stopped it, for the exit code and the line of output.
    pub reason: Option<Interrupted>,
    /// Commits whose analysis failed, with the reason. A degraded run, not a failed one (§7.6).
    pub failures: Vec<String>,
    /// Wall time.
    pub elapsed: Duration,
}

impl Report {
    /// Every lookup made this run across the analyzers that ran.
    #[must_use]
    pub fn lookups(&self) -> i64 {
        self.analyzers.values().map(|stats| stats.lookups).sum()
    }

    /// Every answer found this run, stored failures included.
    #[must_use]
    pub fn hits(&self) -> i64 {
        self.analyzers
            .values()
            .map(|stats| stats.hits + stats.negative_hits)
            .sum()
    }

    /// Analyses computed rather than found, which is what a budget counts and what a slow warm run would show.
    #[must_use]
    pub fn computed(&self) -> i64 {
        self.analyzers.values().map(|stats| stats.puts).sum()
    }
}

/// Why a run stopped early. Both cases are stated partials (§7.6), not failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interrupted {
    /// Something pulled the cancel handle.
    Cancelled,
    /// [`Options::unit_budget`] ran out.
    Budget,
}

/// One analyzer's counters for one run.
///
/// Every field is a row of the store's `counter` table, so `fathom index --stats`, `fathom stats` and a
/// dashboard read one set of numbers instead of three that drift.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AnalyzerStats {
    /// Lookups made.
    pub lookups: i64,
    /// Answers found.
    pub hits: i64,
    /// Stored failures found, which are answers (§5.6).
    pub negative_hits: i64,
    /// No row for this input at all: the cache is cold, which is not a defect.
    pub cold: i64,
    /// A row for this input exists under a *different* key — something was bumped (§5.6).
    pub bumped: i64,
    /// A row exists but was built in an older epoch (§6.3).
    pub epoch_stale: i64,
    /// Rows written.
    pub puts: i64,
    /// Payload bytes served from cache.
    pub bytes: i64,
    /// Milliseconds spent computing the ones that missed.
    pub cost_ms: i64,
}

impl AnalyzerStats {
    /// Hit rate as a percentage.
    ///
    /// `None` when nothing was looked up: "there was nothing to ask" is not 0%, and printing 0% for an
    /// analyzer that never ran is how a perfectly warm cache gets debugged as a broken one.
    #[must_use]
    pub fn hit_rate(&self) -> Option<f64> {
        if self.lookups == 0 {
            return None;
        }
        // `f64::from(u32)` rather than `as f64`: no truncation to reason about, and a lookup count that
        // does not fit in a `u32` is clamped, which is a percentage nobody will act on either way.
        let hits = f64::from(u32::try_from(self.hits + self.negative_hits).unwrap_or(u32::MAX));
        let lookups = f64::from(u32::try_from(self.lookups).unwrap_or(u32::MAX));
        Some(hits * 100.0 / lookups)
    }

    /// The one-line form `fathom index --stats` prints.
    #[must_use]
    pub fn summary(&self) -> String {
        let mut out = format!(
            "{}/{} hit, {} cold, {} bumped",
            self.hits + self.negative_hits,
            self.lookups,
            self.cold,
            self.bumped
        );
        if self.epoch_stale > 0 {
            out.push_str(&format!(", {} epoch", self.epoch_stale));
        }
        if self.negative_hits > 0 {
            out.push_str(&format!(", {} stored failure", self.negative_hits));
        }
        out
    }
}

/// One provisional record (§6.4): the analysis of an uncommitted path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisionalRecord {
    /// The path, quoted the way the store quotes it.
    pub path: String,
    /// Staged (`git diff --cached`) or merely dirty (`git diff`).
    pub staged: bool,
    /// What the lookup decided. A hit here means the answer was already cached under the same content
    /// identity — which, for a path that has just been committed, is §6.4 R6's "reused, if it happens to be
    /// right" happening by itself.
    pub reason: Reason,
}

/// Index `repo` into `store`.
///
/// # Errors
///
/// [`Error::Ingest`] when the repository cannot be walked or a batch cannot be written, and [`Error::Store`]
/// when the cache cannot be read or written. One analysis failing is not an error: it is a row's `error`
/// column or an entry in [`Report::spine`]'s failures, and the run continues (§7.6).
pub fn run(
    repo: &Repository,
    store: &mut Store,
    taxonomy: &dyn Taxonomy,
    options: &Options,
) -> Result<Report, Error> {
    let started = Instant::now();
    // ADR-002 §4.4: a crate below `fathom-cli` does not read the clock. The instant is an argument, and a
    // run that was not given one refuses rather than quietly becoming a golden test that changes daily.
    let now = options.now.ok_or(Error::InvalidRequest {
        why: "an index run must be given the instant to stamp its rows with; the clock belongs to the CLI",
    })?;
    store.flush_counters(now)?;
    let before = store.counters("cache.")?;

    // 1. The spine, first and through `ingest::run`, which owns the walk, the batching, the resume query and
    //    the `index_run` row — so this run is durable bookkeeping from here on (§9.2). It walks the history
    //    once, and nothing below walks it again: the analyses ask the store which commits this run wrote.
    let mut sink = StoreSink::deferring(store, now).with_progress(options.progress.clone());
    let spine = ingest::run(repo, taxonomy, &options.ingest, &mut sink)?;
    // The walk's own closing tick: it is the first time the run knows how many commits the repository
    // had, and a phase that ends without ever reaching its total reads like a run that stopped early.
    if let Some(progress) = &options.progress {
        let seen = u64::try_from(spine.commits_seen).unwrap_or(u64::MAX);
        progress.report(&Tick {
            phase: Phase::Walking,
            done: seen,
            of: Some(seen),
        });
    }
    // Which run the spine opened, and therefore which one this function closes at the end. `None` means the
    // walk opened no run at all — no refs, nothing to walk — in which case there is no row to close.
    let spine_run = sink.run_id;
    record_spine(store, &spine);

    // 1b. The ref snapshot, which is what the episode step reads to tell "committed and reachable" from
    // "committed and rewritten away". The spine is append-only, so without this the commits a rebase or a
    // force-push deleted stay in the store forever and the rail shows them beside their replacements —
    // ADR-004 Appendix A §4.1's "what the refs said at this instant", which until `e1.3` needed it, nothing
    // had ever said.
    snapshot_refs(repo, store, now)?;

    // 2. What the analyses owe work on, which is a query and not a walk (§6.1 R1): the commits written by
    //    this run, plus any commit whose run never closed cleanly. The second half is what makes an
    //    interrupted index *finish* rather than become silently incomplete forever — a killed run's commits
    //    are stamped `crashed` on the next open ([`crate::sink::StoreSink`]'s `begin`), and their derived
    //    rows are exactly the ones nobody will ask about again unless somebody asks.
    let targets: Vec<String> = match options.scope {
        Scope::NewCommits => store.oids_pending_derivation(spine_run.unwrap_or(0))?,
        Scope::Everything => store.all_oids()?,
    };

    // 3. The analyses.
    let mut computed = 0_usize;
    let mut interrupted = None;
    let mut failures: Vec<String> = Vec::new();
    // The one denominator this run can state up front: the commits that owe derivation. Every tick of the
    // loop below is measured against it, which is what makes the CLI's `indexing 62 % · ~14 s left` a
    // measured figure rather than a guess (`e5.9`).
    let analyses = u64::try_from(targets.len()).unwrap_or(u64::MAX);
    for (index, oid) in targets.iter().enumerate() {
        if let Some(sink) = &options.progress {
            sink.report(&Tick {
                phase: Phase::Analysing,
                done: u64::try_from(index).unwrap_or(u64::MAX),
                of: Some(analyses),
            });
        }
        if triggered(options) {
            interrupted = Some(Interrupted::Cancelled);
            break;
        }
        if matches!(options.unit_budget, Some(budget) if computed >= budget) {
            interrupted = Some(Interrupted::Budget);
            break;
        }
        let commit = match oid.parse::<Oid>() {
            Ok(commit) => commit,
            // A commit id in the store that will not parse is a broken row, not a broken analysis; there is
            // no key to hang it on, so the report is the only place left to say it.
            Err(err) => {
                failures.push(format!("{oid}: stored commit id does not parse ({err})"));
                continue;
            }
        };
        match analyse_commit(repo, store, options, &commit, now) {
            Ok(made) => {
                computed += made;
                if matches!(options.park_after_units, Some(stop) if computed >= stop) {
                    park_forever();
                }
            }
            // One commit's analysis failing must not end the run. Where there is a key to hang the failure
            // on, it went into that row's `error` column on the way out, so the next lookup for this input
            // gets the same answer cheaply instead of reading the same broken object again (§5.6). Where
            // there is no key, this list is the only place left to say it — and it is in the report, not
            // only in a log.
            Err(err) => failures.push(format!("{oid}: {err}")),
        }
    }

    // The analyses' closing tick, and only for a run that was not stopped: a cancelled run that printed
    // `100 %` would be the false report ADR-004 §9.2 exists to prevent (§7.6's stated partial says how
    // far it got instead).
    if let Some(progress) = &options.progress
        && interrupted.is_none()
    {
        progress.report(&Tick {
            phase: Phase::Analysing,
            done: analyses,
            of: Some(analyses),
        });
    }

    // 4. The worktree (§6.4), unless asked not to look at it or asked to stop.
    let (worktree, worktree_skipped) = if options.worktree == Worktree::Ignore || triggered(options)
    {
        (Vec::new(), Vec::new())
    } else {
        if let Some(sink) = &options.progress {
            sink.report(&Tick {
                phase: Phase::Worktree,
                done: 0,
                of: None,
            });
        }
        let (rows, skipped) = analyse_worktree(repo, store, options, now)?;
        // The phase's closing tick, with what it actually read. Without it the row a terminal keeps
        // reprinting is the opening one — `reading the worktree unknown % · 0 paths` — and a reviewer
        // watching that row on a dirty tree has no way to tell "started" from "finished with nothing".
        if let Some(progress) = &options.progress {
            let n = u64::try_from(rows.len()).unwrap_or(u64::MAX);
            progress.report(&Tick {
                phase: Phase::Worktree,
                done: n,
                of: Some(n),
            });
        }
        (rows, skipped)
    };

    if triggered(options) && interrupted.is_none() {
        interrupted = Some(Interrupted::Cancelled);
    }

    // 4b. The episodes. A query over the commit rows the spine already wrote, not a second git walk: the
    //     boundaries are a property of the neighbourhood around each commit, so the whole lineage is
    //     re-derived, and the identity rule means the reruns produce the same ids (§4.3 of the plan).
    let episodes = if options.episodes && interrupted.is_none() {
        if let Some(sink) = &options.progress {
            sink.report(&Tick {
                phase: Phase::Episodes,
                done: 0,
                of: None,
            });
        }
        let rebuilt = crate::episodes::rebuild(store, &options.segment, now)?;
        // Same argument as the worktree's closing tick: the opening row says `0 episodes`, and the only
        // thing that can correct it is a tick that says how many there are now.
        if let Some(progress) = &options.progress {
            let n = u64::try_from(rebuilt.episodes).unwrap_or(u64::MAX);
            progress.report(&Tick {
                phase: Phase::Episodes,
                done: n,
                of: Some(n),
            });
        }
        rebuilt
    } else {
        crate::episodes::Report::default()
    };

    // 4c. Close the `index_run` row, and only now. Closing it when the commit rows landed would stamp `ok`
    //     on a run whose analyses were then interrupted, and §9.2's whole recovery argument is that this
    //     stamp is the truth: a run that lies `ok` is a run whose unfinished work nobody asks about again,
    //     which is how a killed index would become permanently, silently incomplete.
    if let Some(run) = spine_run {
        let mut stats = crate::sink::run_stats(&spine);
        stats.interrupted |= interrupted.is_some();
        if stats.error.is_none() {
            stats.error = failures.first().cloned();
        }
        store.finish_run(run, now, &stats)?;
    }

    // 5. Counters, flushed and read back, so what `--stats` prints is what is in the database rather than a
    //    parallel set collected for the console. The delta, because a run reports what *it* did.
    store.flush_counters(now)?;
    let after = store.counters("cache.")?;
    let delta = delta(&before, &after);
    let analyzers = ANALYZED
        .iter()
        .map(|id| ((*id).to_owned(), stats_of(&delta, id)))
        .collect();

    Ok(Report {
        kind: spine.kind,
        commits_walked: spine.commits_seen,
        commits_new: spine.commits_new,
        commits_considered: targets.len(),
        spine,
        episodes,
        analyzers,
        worktree,
        worktree_skipped,
        interrupted: interrupted.is_some(),
        reason: interrupted,
        failures,
        elapsed: started.elapsed(),
    })
}

/// Never return, leaving the run open and the store's WAL un-checkpointed, so that whatever signal the
/// harness sends is the one being tested.
///
/// This is the body of a process the test is about to `kill -9`; there is no cleanup to do here and there
/// must not be any — a `Drop`, a flush or a graceful exit would be the thing the test is pretending did not
/// happen.
fn park_forever() -> ! {
    loop {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

fn triggered(options: &Options) -> bool {
    options
        .cancel
        .as_ref()
        .is_some_and(|cancel| cancel.is_triggered())
}

/// The per-name increase between two counter snapshots.
fn delta(before: &BTreeMap<String, i64>, after: &BTreeMap<String, i64>) -> BTreeMap<String, i64> {
    after
        .iter()
        .map(|(name, value)| {
            (
                name.clone(),
                *value - before.get(name).copied().unwrap_or(0),
            )
        })
        .collect()
}

/// Write the ref snapshot `ref_tip` exists for (ADR-004 Appendix A §4.1: what the refs said at this
/// instant, not a history).
///
/// Names that do not name a commit — notes, the stash, hand-made namespaces — are skipped rather than
/// stored, because `ref_tip` is read to answer "is this commit still reachable from a name a human would
/// cite?" and a notes ref is not one. `HEAD` gets its own row, detached or not, because a detached `HEAD`
/// is exactly the state an episode lineage has to survive.
fn snapshot_refs(repo: &Repository, store: &mut Store, now: i64) -> Result<(), Error> {
    use fathom_ingest::repo::{RefFilter, RefKind, RefTarget};
    let mut tips: Vec<(String, String, &str)> = Vec::new();
    for reference in repo.refs(RefFilter::All)? {
        let kind = match reference.kind {
            RefKind::Branch => "branch",
            RefKind::Tag => "tag",
            RefKind::RemoteBranch => "remote",
            _ => continue,
        };
        let target = match (&reference.target, &reference.peeled) {
            (RefTarget::Direct { oid }, _) => oid.hex(),
            (RefTarget::Symbolic { .. }, Some(peeled)) => peeled.hex(),
            _ => continue,
        };
        tips.push((reference.name, target, kind));
    }
    if let Some(head) = repo.head_oid()? {
        tips.push(("HEAD".to_owned(), head.hex(), "head"));
    }
    store.replace_ref_tips(&tips, now)?;
    Ok(())
}

/// The spine's counters, from its [`RunReport`].
///
/// `ingest.normalize` is the one analyzer whose cache is not the `cache` table, because its result already
/// has a dedicated one: §4.5's `git_commit` *is* the materialised record, and storing it twice would give
/// two copies to disagree about. Its counters come from the run's own lookups — every commit the walk
/// emitted was a lookup, every commit the store already had was a hit — which is what lets `--stats` print a
/// truthful row for it instead of a blank, and what makes a warm run able to say "501 looked up, 500 hit".
pub fn record_spine(store: &mut Store, report: &RunReport) {
    store.bump_counter("cache.lookup.ingest.normalize", fit(report.commits_seen));
    store.bump_counter("cache.hit.ingest.normalize", fit(report.commits_existing));
    store.bump_counter("cache.cold.ingest.normalize", fit(report.commits_new));
    store.bump_counter(
        "cache.cost_ms.ingest.normalize",
        i64::try_from(report.elapsed.as_millis()).unwrap_or(i64::MAX),
    );
}

fn fit(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// One analyzer's counters out of a counter table.
#[must_use]
pub fn stats_of(counters: &BTreeMap<String, i64>, analyzer_id: &str) -> AnalyzerStats {
    let named = |thing: &str| format!("cache.{thing}.{analyzer_id}");
    let get = |name: &str| counters.get(name).copied().unwrap_or(0);
    AnalyzerStats {
        lookups: get(&named("lookup")),
        hits: get(&named("hit")),
        negative_hits: get(&named("negative_hit")),
        cold: get(&named("cold")),
        bumped: get(&named("bump")),
        epoch_stale: get(&named("epoch")),
        puts: get(&named("put")),
        bytes: get(&named("bytes")),
        cost_ms: get(&named("cost_ms")),
    }
}

/// One path one commit touched, in the shape the two analyses consume.
///
/// Built from the spine's `git_commit_file` rows rather than from a fresh two-tree walk, for two reasons
/// that are not convenience. The first is cost: [`fathom_ingest::ingest`] has already walked both trees,
/// resolved the renames, decided the binary question and recorded both blob ids, so a warm run that walked
/// the trees again would pay twice for facts the store already holds — and §6.1 R1's "a warm run costs what
/// the change costs" is a sentence about not paying twice. The second is correctness:
/// [`Repository::changed_paths`] is a raw tree comparison and rename detection lives in normalize, so a
/// tree-walk-driven warm path would report every `git mv` as a delete plus an unrelated add and score a
/// moved file as maximum churn — a wrong answer, not merely a slow one.
///
/// The tree comparison itself is not dead: it is the primitive `fathom-ingest` publishes for callers that
/// have no spine to read from, and `tests/changed_paths.rs` uses it as the independent check that these
/// rows say what git says.
struct Unit {
    /// The path after the change, in `git_commit_file.new_path`'s quoted form.
    path: String,
    /// The path before, on a rename or copy — the name the parent's inventory has to lose.
    old_path: Option<String>,
    /// sha256 of the raw path bytes (§4.1), which is the key half and is never re-derived from `path`.
    path_hash: String,
    /// `add`, `modify`, `delete`, `rename` or `copy`.
    kind: String,
    /// The blob before, `None` on an add.
    before: Option<Oid>,
    /// The blob after, `None` on a delete.
    after: Option<Oid>,
    /// The mode after.
    mode: Option<u32>,
    /// Measured binary as the spine measured it: such a path has no line analysis to make (§7.1), and the
    /// answer is already in a column.
    binary: bool,
}

/// The analyses one commit is worth. Returns how many were *computed*, which is what a budget counts.
fn analyse_commit(
    repo: &Repository,
    store: &mut Store,
    options: &Options,
    commit: &Oid,
    now: i64,
) -> Result<usize, Error> {
    let row = store.read_commit(&commit.hex())?;
    let tree = hex_oid(&row.tree_oid, "tree")?;
    // The first parent's *tree*, because that is the side normalize diffed a merge against — asking the
    // second parent would compute a delta this store never claimed to record. It has to be the tree and not
    // the commit: `structure.delta` is keyed on tree identity (§5.3) so that two commits reaching the same
    // tree analyse it once, and keying it on a commit id would make every lookup a miss while looking
    // exactly as warm as if it had worked.
    let parent_tree = match row.parent_oids.first() {
        Some(hex) => Some(hex_oid(&store.read_commit(hex)?.tree_oid, "parent tree")?),
        None => None,
    };
    let mut units = Vec::with_capacity(row.files.len());
    for file in &row.files {
        units.push(Unit {
            path: file.new_path.clone(),
            old_path: file.old_path.clone(),
            path_hash: file.path_hash.clone(),
            kind: file.change_kind.clone(),
            before: file
                .blob_old
                .as_deref()
                .map(|hex| hex_oid(hex, "old blob"))
                .transpose()?,
            after: file
                .blob_new
                .as_deref()
                .map(|hex| hex_oid(hex, "new blob"))
                .transpose()?,
            mode: file.mode_new.map(|mode| u32::try_from(mode).unwrap_or(0)),
            binary: file.is_binary,
        });
    }

    let mut computed = 0_usize;
    for unit in &units {
        if triggered(options) {
            break;
        }
        if matches!(options.unit_budget, Some(budget) if computed >= budget) {
            break;
        }
        computed += analyse_path_pair(repo, store, options, unit, now)?;
    }
    computed += analyse_structure(store, repo, options, tree, parent_tree, &units, now)?;
    Ok(computed)
}

/// Parse a stored hex id, blaming the column rather than the caller when it is not one.
fn hex_oid(hex: &str, what: &str) -> Result<Oid, Error> {
    hex.parse::<Oid>().map_err(|err| Error::SpineRowUnreadable {
        what: format!("the {what} column is not an oid ({hex}): {err}"),
    })
}

/// One `path_pair` unit: what happened to one path in one commit (§6's row two).
fn analyse_path_pair(
    repo: &Repository,
    store: &mut Store,
    options: &Options,
    unit: &Unit,
    now: i64,
) -> Result<usize, Error> {
    if unit.binary {
        // No line analysis of a binary file, and the reason is in the row rather than in a re-read of the
        // object: the spine measured it once. There is no cache row either, because there is no answer to
        // cache — a `binary: true` payload would be a second place to say the same thing.
        return Ok(0);
    }
    let hex = |oid: Option<Oid>| oid.map_or_else(String::new, |oid| oid.hex());
    let outcome = store.resolve(
        "diff.per_file",
        &cache::path_pair_digest(&unit.path_hash, &hex(unit.before), &hex(unit.after)),
        Some("path_pair"),
        &options.versions,
        &options.config,
        now,
    )?;
    if outcome.hit() && !options.refresh {
        return Ok(0);
    }
    let started = Instant::now();
    let measured = match diff::measure_blobs(
        repo,
        unit.before,
        unit.after,
        &options.ingest.normalize.limits,
    ) {
        Ok(measured) => measured,
        Err(err) => {
            // A stored failure is an answer (§5.6): the next lookup for this input is a negative hit
            // instead of another read of the same broken object, and `--stats` counts it as one.
            store.put(&outcome.key, &[], Some(&err.to_string()), 0, now)?;
            return Ok(1);
        }
    };
    let cost_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    store.put(
        &outcome.key,
        per_file_payload(unit, &measured).as_bytes(),
        None,
        cost_ms,
        now,
    )?;
    Ok(1)
}

/// The `structure.delta` unit: the file inventory one tree introduces (§6's row one).
fn analyse_structure(
    store: &mut Store,
    repo: &Repository,
    options: &Options,
    tree: Oid,
    parent_tree: Option<Oid>,
    units: &[Unit],
    now: i64,
) -> Result<usize, Error> {
    // The `tree` scope's input hash is the tree OID itself (§5.3), so two commits that reach the same tree
    // analyse it once, and a revert that lands back on an old tree is a hit instead of a re-run.
    let outcome = store.resolve(
        "structure.delta",
        &tree.hex(),
        Some("tree"),
        &options.versions,
        &options.config,
        now,
    )?;
    if outcome.hit() && !options.refresh {
        return Ok(0);
    }
    let started = Instant::now();
    // The parent's inventory is the whole trick. When it is in the cache, this inventory is that one plus
    // this commit's changed paths and no tree object outside the changed spine is read at all — which is
    // §6.1 R2 and §6.6 R2: a warm run's cost is the change, never the repository. When it is not (a root
    // commit, a shallow boundary, the first run after a bump) the tree is enumerated once, and the payload
    // says which of the two it is, because "cold" and "re-derived" are different diagnoses (§5.6).
    let parent_inventory = match parent_tree {
        None => None,
        Some(parent) => {
            store
                .resolve(
                    "structure.delta",
                    &parent.hex(),
                    Some("tree"),
                    &options.versions,
                    &options.config,
                    now,
                )?
                .payload
        }
    };
    let inventory = match parent_inventory.as_deref().and_then(Inventory::from_json) {
        Some(inventory) => inventory.merged(repo, units)?,
        None => Inventory::enumerate(repo, tree)?,
    };
    let cost_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    store.put(
        &outcome.key,
        inventory.to_json().as_bytes(),
        None,
        cost_ms,
        now,
    )?;
    Ok(1)
}
/// One file in an inventory.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct File {
    /// The path, quoted the way `git_commit_file.new_path` quotes it — a byte-injective spelling, which is
    /// what lets two inventories be merged without a lossy comparison.
    path: String,
    /// The raw mode, `0o100644`-style.
    mode: u32,
    /// The blob's size, taken from its object header.
    size: u64,
}

/// How an inventory was made, recorded in the payload: `full` cost the tree, `merged` cost the change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Derived {
    /// Derived from the parent tree's inventory plus this commit's changed paths.
    Merged,
    /// Enumerated from the tree itself.
    Full,
}

/// A tree's file inventory — `structure.delta`'s result, and the reason §6.1 R2's "the cost does not depend
/// on the repository's size" is worth writing down instead of hoping someone notices.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inventory {
    files: Vec<File>,
    directories: usize,
    truncated: bool,
    derived: Derived,
}

impl Inventory {
    fn enumerate(repo: &Repository, tree: Oid) -> Result<Self, Error> {
        let listing = repo.tree_files(tree, None)?;
        Ok(Inventory {
            files: listing
                .files
                .iter()
                .map(|file| File {
                    path: file.path.quoted(),
                    mode: file.mode,
                    size: file.size,
                })
                .collect(),
            directories: listing.directories,
            truncated: listing.truncated,
            derived: Derived::Full,
        })
    }

    /// Apply one commit's changed paths to its parent's inventory.
    ///
    /// The only git work this does is reading the object *headers* of the paths that changed, which inflate
    /// nothing (`Repository::object_header`'s whole reason for existing).
    fn merged(self, repo: &Repository, units: &[Unit]) -> Result<Self, Error> {
        let mut files: BTreeMap<String, File> = self
            .files
            .into_iter()
            .map(|file| (file.path.clone(), file))
            .collect();
        let directories = self.directories;
        for unit in units {
            // A rename is one unit that loses one name and gains another, which is why `old_path` is a field
            // of the unit rather than a second unit: the alternative is charging a moved file its full size
            // as churn, which is the mistake §6's invalidation matrix exists to prevent.
            if let Some(old) = &unit.old_path {
                files.remove(old);
            }
            let Some(after) = unit.after else {
                // Gone. A directory disappearing is the sum of its files disappearing, and each of those is
                // its own unit, so the count needs no separate handling either.
                files.remove(&unit.path);
                continue;
            };
            let size = repo.object_header(after)?.map_or(0, |header| header.size);
            let _ = files.insert(
                unit.path.clone(),
                File {
                    path: unit.path.clone(),
                    mode: unit.mode.unwrap_or(0o100644),
                    size,
                },
            );
        }
        Ok(Inventory {
            files: files.into_values().collect(),
            directories,
            truncated: false,
            derived: Derived::Merged,
        })
    }

    /// Compact JSON. Infallible by construction: every field is a string, an integer or a bool.
    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| String::from(r#"{"derived":"full"}"#))
    }

    /// `None` for anything that is not a payload this wrote — a truncated blob, an old shape, a corrupt
    /// byte. The caller then enumerates the tree, which is always right (§7.6: a bad read degrades to
    /// re-derivation, never to a wrong answer).
    fn from_json(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

/// A per-file result, as the analyzer's own payload.
///
/// Short keys, because this is written once per `(path, old blob, new blob)` and read back on every hit.
/// The field names are the analyzer's business and deliberately *not* the store's columns: §5's payloads
/// are blobs, not rows, and §9.5's "migrations never migrate derived data" depends on that distinction.
fn per_file_payload(unit: &Unit, measured: &Measurement) -> String {
    let hunks: Vec<String> = measured.hunks.iter().map(Hunk::header).collect();
    serde_json::json!({
        "path": unit.path,
        "old_path": unit.old_path,
        "kind": unit.kind,
        "old": unit.before.map(|oid| oid.hex()),
        "new": unit.after.map(|oid| oid.hex()),
        "mode_new": unit.mode,
        "content": format!("{:?}", measured.content),
        "added": measured.added,
        "removed": measured.removed,
        "hunks": hunks,
        "hunk_total": measured.hunk_total,
    })
    .to_string()
}

/// The provisional pass (§6.4).
///
/// The key for an uncommitted path *is* the key its commit will use: the path, the committed side's blob, and
/// the blob id the working content hashes to — which git computed for the status scan already, so nothing is
/// hashed twice. That is why no "provisional" flag is needed for correctness: when the commit lands, the
/// first pass looks up the same key and finds the row. Content that never gets committed is a row nothing
/// looks up again, and §8's retention is what eventually prunes it.
fn analyse_worktree(
    repo: &Repository,
    store: &mut Store,
    options: &Options,
    now: i64,
) -> Result<(Vec<ProvisionalRecord>, Vec<SkippedPath>), Error> {
    let state = repo.working_state(&options.scan)?;
    let mut skipped: Vec<SkippedPath> = state
        .notices
        .iter()
        .filter_map(|notice| match notice {
            StateNotice::UnreadablePath { path, reason } => {
                Some((path.quoted(), format!("unreadable: {reason}")))
            }
            StateNotice::Oversized { count } => Some((
                format!("{count} path(s)"),
                String::from("over the size cap, so not compared"),
            )),
            _ => None,
        })
        .collect();
    let mut records = Vec::new();
    let mut seen = BTreeSet::new();

    for change in state.staged.iter().chain(state.unstaged.iter()) {
        // A path both staged and dirty is analysed once, from the staged side: that is what `git commit`
        // would record right now, and the working side's row would be discarded by the next `git add`.
        if !seen.insert(change.path.as_bytes().to_vec()) {
            continue;
        }
        let Some(after) = change.after.map(|side| side.oid) else {
            // Deleted on the working side, or never hashed: an identity exists for a deletion (the before
            // side alone), but not for a file the scan refused to read, and guessing one is not on the table.
            if change.kind == fathom_ingest::state::ChangeKind::Deleted {
                match provisional_one(repo, store, options, change, None, now) {
                    Ok(record) => records.push(record),
                    Err(err) => skipped.push((change.path.quoted(), err.to_string())),
                }
            } else {
                skipped.push((
                    change.path.quoted(),
                    String::from(
                        "the working side was not hashed, so there is no identity to key on",
                    ),
                ));
            }
            continue;
        };
        if change
            .on_disk
            .as_ref()
            .is_some_and(|disk| disk.kind == WorktreeKind::Directory)
        {
            // A directory where a file was expected is a type change, and there is no file to analyse.
            continue;
        }
        match provisional_one(repo, store, options, change, Some(after), now) {
            Ok(record) => records.push(record),
            Err(err) => skipped.push((change.path.quoted(), err.to_string())),
        }
    }
    Ok((records, skipped))
}

/// One provisional analysis.
fn provisional_one(
    repo: &Repository,
    store: &mut Store,
    options: &Options,
    change: &WorkChange,
    working: Option<Oid>,
    now: i64,
) -> Result<ProvisionalRecord, Error> {
    let before = change.before.map(|side| side.oid);
    let outcome = store.resolve(
        "diff.per_file",
        &cache::path_pair_digest(
            &sha256_hex(change.path.as_bytes()),
            &before.map_or_else(String::new, |oid| oid.hex()),
            &working.map_or_else(String::new, |oid| oid.hex()),
        ),
        Some("path_pair"),
        &options.versions,
        &options.config,
        now,
    )?;
    if !outcome.hit() {
        let started = Instant::now();
        // The measurement has to be the one the committed pass will make, or the provisional number and the
        // committed number would tell different stories about the same bytes. For a staged path both sides
        // are already in the object database, so it is literally the same call; for a dirty one the new side
        // comes off disk under the same cap ([`diff::measure_working`]).
        let measured = match working {
            None => diff::measure_blobs(repo, before, None, &options.ingest.normalize.limits)?,
            Some(oid) if repo.has_object(oid) => {
                diff::measure_blobs(repo, before, Some(oid), &options.ingest.normalize.limits)?
            }
            Some(_) => {
                diff::measure_working(repo, &change.path, before, &options.ingest.normalize.limits)?
            }
        };
        let cost_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        store.put(
            &outcome.key,
            working_file_payload(change, &measured).as_bytes(),
            None,
            cost_ms,
            now,
        )?;
    }
    store.bump_counter(
        if outcome.hit() {
            "provisional.hit"
        } else {
            "provisional.computed"
        },
        1,
    );
    Ok(ProvisionalRecord {
        path: change.path.quoted(),
        staged: change.side == Side::Staged,
        reason: outcome.reason,
    })
}

/// A dirty file's result, in the same shape as a committed one (§6.4's `working` example).
fn working_file_payload(change: &WorkChange, measured: &Measurement) -> String {
    let hunks: Vec<String> = measured.hunks.iter().map(Hunk::header).collect();
    serde_json::json!({
        "path": change.path.quoted(),
        "kind": format!("{:?}", change.kind),
        "side": change.side.git_equivalent(),
        "old": change.before.map(|side| side.oid.hex()),
        "new": change.after.map(|side| side.oid.hex()),
        "content": format!("{:?}", measured.content),
        "added": measured.added,
        "removed": measured.removed,
        "hunks": hunks,
        "hunk_total": measured.hunk_total,
    })
    .to_string()
}

/// Where a run's rows live by default: `.fathom/index.sqlite3` under the repository root (§3).
#[must_use]
pub fn default_store_path(root: &std::path::Path) -> std::path::PathBuf {
    root.join(".fathom").join("index.sqlite3")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn a_run_with_nothing_to_ask_reports_no_hit_rate_rather_than_zero() {
        let stats = stats_of(&BTreeMap::new(), "diff.per_file");
        assert_eq!(stats.lookups, 0);
        assert!(
            stats.hit_rate().is_none(),
            "an analyzer that never ran is not 0% warm: {stats:?}"
        );
    }

    #[test]
    fn counters_are_read_from_the_store_names_not_from_a_parallel_set() {
        let mut counters = BTreeMap::new();
        for (name, value) in [
            ("cache.lookup.diff.per_file", 7_i64),
            ("cache.hit.diff.per_file", 5),
            ("cache.cold.diff.per_file", 1),
            ("cache.bump.diff.per_file", 1),
            ("cache.bytes.diff.per_file", 400),
        ] {
            let _ = counters.insert(name.to_owned(), value);
        }
        let stats = stats_of(&counters, "diff.per_file");
        assert_eq!(
            (
                stats.lookups,
                stats.hits,
                stats.cold,
                stats.bumped,
                stats.bytes
            ),
            (7, 5, 1, 1, 400)
        );
        assert!(stats.summary().starts_with("5/7 hit, 1 cold, 1 bumped"));
    }

    #[test]
    fn a_run_reports_what_it_did_rather_than_what_the_store_has_ever_done() {
        let mut before = BTreeMap::new();
        let mut after = BTreeMap::new();
        for (name, value) in [
            ("cache.hit.structure.delta", 40_i64),
            ("cache.lookup.structure.delta", 60),
        ] {
            let _ = before.insert(name.to_owned(), value - 10);
            let _ = after.insert(name.to_owned(), value);
        }
        let stats = stats_of(&delta(&before, &after), "structure.delta");
        assert_eq!(
            (stats.lookups, stats.hits),
            (10, 10),
            "the run's own numbers, not the store's lifetime totals"
        );
    }

    #[test]
    fn an_inventory_round_trips_through_its_own_payload() {
        let inventory = Inventory {
            files: vec![
                File {
                    path: "README.md".to_owned(),
                    mode: 0o100644,
                    size: 12,
                },
                File {
                    path: "src/main.rs".to_owned(),
                    mode: 0o100755,
                    size: 4_096,
                },
            ],
            directories: 2,
            truncated: false,
            derived: Derived::Merged,
        };
        let json = inventory.to_json();
        let read = Inventory::from_json(json.as_bytes()).unwrap();
        assert_eq!(read.files.len(), 2);
        assert_eq!(read.files[1].mode, 0o100755);
        assert_eq!(read.derived, Derived::Merged);
        // Anything that is not this shape is `None`, and the caller re-derives rather than trusting it.
        assert!(Inventory::from_json(b"not json").is_none());
    }
}
