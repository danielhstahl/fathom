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
    /// The harness event stream, already read, when the repository has one (`e4.2`, `e6.1`).
    ///
    /// `None` is the common case — most repositories have no adapter installed — and it is a state, not a
    /// degraded mode: the trailer fallback still runs. The CLI owns the reading (ADR-002 §4.5: this crate
    /// does not open files it was not handed) and hands the ledger over as data, exactly as it hands over
    /// the config.
    pub hooks: Option<fathom_ingest::hooks::Ledger>,
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
            hooks: None,
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
    /// The run found nothing to do and skipped the walk, the analyses and the episode rebuild instead of
    /// repeating them (`reco-7tc`).
    ///
    /// What it means for the other fields: `commits_walked` is `0` because nothing was walked, not because
    /// the repository has no history, and [`Report::episodes`] is the store's own tally rather than a step
    /// this run performed. Nothing was written, so nothing is counted.
    pub quiet: bool,
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
    /// Every commit this run could not finish, rendered as `<commit>: <reason>` — the commits the object
    /// database would not serve *and* the commits whose analysis failed. A degraded run, not a failed one
    /// (§7.6), and the line `fathom index` prints when a repository is damaged.
    pub failures: Vec<String>,
    /// Wall time.
    pub elapsed: Duration,
    /// Where that wall time went, in order, as `(phase, milliseconds)`.
    ///
    /// `e7.2`'s real-repository pass (`reco-66q`) found a cold index 25x over its budget with no way to
    /// say which half was late: the per-analyzer counters add up to a fraction of the run, and the rest
    /// was invisible. A run that cannot name its phases cannot be made faster, only guessed at.
    pub phases: Vec<(String, u128)>,
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

/// The phase names, in run order, as the report prints them.
pub const PHASE_SPINE: &str = "spine";
/// The ref snapshot (§4.1) — the store write that says what the refs said.
pub const PHASE_REFS: &str = "refs";
/// The per-commit analysis loop: everything between the spine and the worktree.
pub const PHASE_ANALYSES: &str = "analyses";
/// The uncommitted pass (§6.4).
pub const PHASE_WORKTREE: &str = "worktree";
/// The episode rebuild (§4.3), which is a query over the commit rows and not a second walk.
pub const PHASE_EPISODES: &str = "episodes";
/// Closing the `index_run` row and flushing the counters (§9.2).
pub const PHASE_CLOSE: &str = "close";

/// The phase marks as `(name, whole milliseconds)`, each phase measured from the previous mark.
fn phase_pairs(marks: Vec<(&'static str, Duration)>) -> Vec<(String, u128)> {
    let mut out = Vec::with_capacity(marks.len());
    let mut previous = Duration::ZERO;
    for (name, at) in marks {
        let _ = out.push((name.to_owned(), at.saturating_sub(previous).as_millis()));
        previous = at;
    }
    out
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

    // 0. The no-op check, before anything is read or written (`reco-7tc`). See [`nothing_new`] for why the
    //    refs are allowed to answer "is this fresh?" on their own, and for the four things that refuse.
    if nothing_new(repo, store, options)? {
        return quiet_report(repo, store, options, now, started);
    }

    store.flush_counters(now)?;
    let before = store.counters("cache.")?;

    // 1. The spine, first and through `ingest::run`, which owns the walk, the batching, the resume query and
    //    the `index_run` row — so this run is durable bookkeeping from here on (§9.2). It walks the history
    //    once, and nothing below walks it again: the analyses ask the store which commits this run wrote.
    let mut phases: Vec<(&'static str, Duration)> = Vec::with_capacity(6);
    let mut sink = StoreSink::deferring(store, now).with_progress(options.progress.clone());
    let spine = ingest::run(repo, taxonomy, &options.ingest, &mut sink)?;
    phases.push((PHASE_SPINE, started.elapsed()));
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
    phases.push((PHASE_REFS, started.elapsed()));

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
    let mut failures: Vec<String> = spine.failures.iter().map(ToString::to_string).collect();
    // A commit the object database would not serve is recorded against its own id, where every later read
    // can find it: `fathom doctor`, `fathom stats` and `fathom show` all answer "which commit, and why" in
    // the store's own words instead of re-walking the history to guess (ADR-001 §7.6). A run that later
    // *does* read the commit withdraws the record in the same transaction that writes the row, so a
    // repository the user repaired with `git fsck` stops being reported as damaged.
    for failure in &spine.failures {
        store.record_unreadable_commit(
            &failure.oid.hex(),
            &failure.reason,
            spine_run.unwrap_or(0),
            now,
        )?;
    }
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

    phases.push((PHASE_ANALYSES, started.elapsed()));

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

    phases.push((PHASE_WORKTREE, started.elapsed()));
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
        let rebuilt =
            crate::episodes::rebuild(store, &options.segment, options.hooks.as_ref(), now)?;
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

    phases.push((PHASE_EPISODES, started.elapsed()));

    // 4c. Close the `index_run` row, and only now. Closing it when the commit rows landed would stamp `ok`
    //     on a run whose analyses were then interrupted, and §9.2's whole recovery argument is that this
    //     stamp is the truth: a run that lies `ok` is a run whose unfinished work nobody asks about again,
    //     which is how a killed index would become permanently, silently incomplete.
    if let Some(run) = spine_run {
        let mut stats = crate::sink::run_stats(&spine);
        stats.interrupted |= interrupted.is_some();
        store.finish_run(run, now, &stats)?;
    }

    // The witness the next no-op check reads (`reco-7tc`). A run that stopped early is the interesting one:
    // it may have stopped between the walk and the rebuild, and a store whose refs say "nothing has moved"
    // while its episodes say "an analysis is missing" is a store that would skip finishing its own work.
    // Such a run unsnapshots the refs, which is the one sentence the no-op check cannot answer from.
    match interrupted {
        Some(_) => store.forget_ref_snapshot()?,
        None if options.episodes => {
            store.set_meta(REBUILD_WITNESS_KEY, &rebuild_witness(options))?;
        }
        None => {}
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

    phases.push((PHASE_CLOSE, started.elapsed()));
    Ok(Report {
        kind: spine.kind,
        phases: phase_pairs(phases),
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
        quiet: false,
    })
}

/// Whether this run has nothing to do — and the `HEAD` that says so (`reco-7tc`).
///
/// ADR-004 §11 calls the no-op re-index "the resume path's floor", and every path whose common case *is* the
/// no-op — the watcher's debounce-and-index loop, a CI preflight asking "is this fresh?", a script that runs
/// `fathom index` before reading the store — was paying a whole history walk and a whole episode rebuild to
/// discover that nothing had moved. The refs answer that question without the walk: when every ref says what
/// the store's last snapshot said, and `HEAD` has a commit row of its own, and no commit is owed a
/// derivation, then the walk would emit commits the spine already has, the analysis pass would look up
/// nothing, and the episode rebuild would write back the rows it had just read. So all three are skipped.
///
/// Five things refuse, each for its own reason:
///
/// * a run asked to look at everything (`--refresh`, `--rebuild`): that pass exists to re-measure the cache,
///   and skipping the walk would make it silently mean something else;
/// * a `HEAD` with no commit row of its own — including an unborn branch, where there is no state to be quiet
///   about and the ordinary path is the one that says so;
/// * refs that moved, moved away, or were never snapshotted — or were deliberately unsnapshotted: a
///   correction to an attribution (`reco-w7q`) changes what the store should say about commits it has already
///   indexed without moving any ref, so the write that stores one drops the snapshot the refs would have
///   answered from. Either way there is no snapshot to answer from, which is the right answer rather than a
///   bug to work around;
/// * an input the rebuild reads that no ref describes: a harness stream that is not the one the last rebuild
///   read (`e4.2`), or a `[episode]`/`[intent]` block that was edited (`reco-w7q`'s corrections are the third,
///   and they are handled by dropping the snapshot above). [`rebuild_witness`] spells all three out, because
///   the honest statement of the quiet path is not "the refs did not move" but "nothing the rebuild would
///   have read has moved", and a stream that appears, disappears or grows is exactly such a change. A store
///   that cannot say *which* stream and *which* rules the intents were computed from has to rebuild, which is
///   the same reason it has to walk when a ref moved;
/// * commits the store still owes an analysis for (§9.2) — the residue of a run that was killed or stopped out
///   of its unit budget, and exactly the work the next run exists to finish.
///
/// A dirty worktree is **not** a reason to refuse: a working file is a fact about the working tree, the refs
/// say nothing about it, and §6.4's provisional records are still produced on the quiet path.
fn nothing_new(repo: &Repository, store: &Store, options: &Options) -> Result<bool, Error> {
    if options.refresh || options.scope != Scope::NewCommits {
        return Ok(false);
    }
    let Some(head) = repo.head_oid()? else {
        return Ok(false);
    };
    let hex = head.hex();
    // "The refs match the snapshot" is only the same statement as "nothing new" if the snapshot describes a
    // repository this store actually holds rows for.
    if store.indexed_oids(&[hex])?.is_empty() {
        return Ok(false);
    }
    let stored: BTreeSet<(String, String, String)> = store
        .ref_tips()?
        .into_iter()
        .map(|tip| (tip.name, tip.oid, tip.kind))
        .collect();
    let current: BTreeSet<(String, String, String)> = ref_tip_rows(repo)?
        .into_iter()
        .map(|(name, oid, kind)| (name, oid, kind.to_owned()))
        .collect();
    if stored != current {
        return Ok(false);
    }
    // The changes no ref reports, in one comparison: the stream and the configuration the last rebuild read,
    // against the ones this run was handed. Same witness means the intent the episodes carry was computed
    // from exactly these inputs; no witness, or a different one, means the rebuild has to run.
    if store.meta(REBUILD_WITNESS_KEY)?.as_deref() != Some(rebuild_witness(options).as_str()) {
        return Ok(false);
    }
    // The other half of "nothing": a run that was killed or stopped out of its unit budget left commits owed
    // an analysis, and finishing them is exactly what the next run is for (§9.2).
    Ok(store.pending_derivation()?.is_empty())
}

/// The `meta` key holding the [`rebuild_witness`] of the run that last rebuilt the episodes (`reco-7tc`).
///
/// It is written by the run that read the inputs rather than by whatever changes them, for the reason every
/// generation counter in this codebase is bumped at one place: a witness the reader could forget to record is
/// a no-op check that silently stops noticing the change.
const REBUILD_WITNESS_KEY: &str = "index.rebuild_witness";

/// What a run was handed that git cannot report, on one line (`reco-7tc`).
///
/// Two inputs, both outside the object database. The **harness stream** is counted, not hashed, and `"none"`
/// is a different answer from `"stream=0+0-0"` in the same way `fathom index --json` prints an events file
/// that does not exist: "no stream" and "a stream that said nothing" are different reasons for the same empty
/// intent tally. The **rules** — the `[episode]` and `[intent]` blocks, which the CLI hands over as values
/// (ADR-002 §4.5) — are rendered through `Debug` and hashed, because the witness only has to *change* when a
/// rule changes and a rendered form a refactor can survive silently is the worse failure. A `Rules` field that
/// is added without appearing in its own `Debug` output is invisible here, which is the trade this makes; the
/// three tests named in the CHANGELOG's entry for `reco-7tc` are what catch it.
fn rebuild_witness(options: &Options) -> String {
    let stream = options.hooks.as_ref().map_or_else(
        || "stream=none".to_string(),
        |ledger| {
            format!(
                "stream={}+{}-{}",
                ledger.turns.len(),
                ledger.opened,
                ledger.trimmed
            )
        },
    );
    format!(
        "{stream} rules={}",
        digest(&format!("{:?}", options.segment))
    )
}

/// Twelve hex characters of SipHash, enough to notice a change and short enough to live in `meta`.
///
/// `DefaultHasher`'s algorithm is a Rust-internal detail that changes across toolchains, which costs one
/// unnecessary rebuild the first time a store is opened by a new compiler — the loud direction to fail.
fn digest(text: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    format!("{:012x}", hasher.finish())
}

/// The report of a run that had nothing to do (`reco-7tc`), built without walking anything.
///
/// It opens no `index_run` row. That table is the history of the runs that walked — `fathom stats`' "last
/// run" line, `fathom doctor`'s crashed-run report, the resume query's idea of which run wrote which commit
/// — and a pass that walked nothing and wrote nothing has no numbers to add there. It does run the recovery
/// stamp, because that is the one piece of bookkeeping a no-op can legitimately do: a run left `running` by a
/// process that died is still a lie the first time anyone looks, and this is the first time anyone looks.
fn quiet_report(
    repo: &Repository,
    store: &mut Store,
    options: &Options,
    now: i64,
    started: Instant,
) -> Result<Report, Error> {
    store.recover_runs(now)?;
    store.flush_counters(now)?;
    let before = store.counters("cache.")?;

    // The worktree pass is the only reading a quiet run still owes: the provisional records of §6.4 describe
    // the working tree, which no ref describes, and they are cheap when the tree is clean.
    let (worktree, worktree_skipped) = if options.worktree == Worktree::Ignore || triggered(options)
    {
        (Vec::new(), Vec::new())
    } else {
        analyse_worktree(repo, store, options, now)?
    };

    store.flush_counters(now)?;
    let delta = delta(&before, &store.counters("cache.")?);
    let episodes = crate::episodes::tally(store)?;
    Ok(Report {
        kind: RunKind::Warm,
        commits_walked: 0,
        commits_new: 0,
        commits_considered: 0,
        spine: RunReport::default(),
        // The store's own tally, because a rebuild did not run to produce a fresh one. Printing zeroes here
        // would be printing a sentence about the repository that is not true of it.
        episodes,
        analyzers: ANALYZED
            .iter()
            .map(|id| ((*id).to_owned(), stats_of(&delta, id)))
            .collect(),
        worktree,
        worktree_skipped,
        interrupted: false,
        reason: None,
        failures: Vec::new(),
        elapsed: started.elapsed(),
        // The phases a quiet run has: the check itself, and the worktree pass it still does (§6.4).
        phases: phase_pairs(vec![(PHASE_WORKTREE, started.elapsed())]),
        quiet: true,
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
    store.replace_ref_tips(&ref_tip_rows(repo)?, now)?;
    Ok(())
}

/// Every ref the repository names right now, as the `(name, target oid, kind)` rows [`Store::replace_ref_tips`]
/// stores — extracted so that the no-op check compares exactly what the snapshot writes, rather than an
/// approximation of it that could drift into saying a ref moved when it did not (`reco-7tc`).
///
/// The three skipped `kind`s are the same three `snapshot_refs` has always skipped; see its comment for why a
/// notes ref is not an answer to "is this commit still reachable from a name a human would cite?".
fn ref_tip_rows(repo: &Repository) -> Result<Vec<(String, String, &'static str)>, Error> {
    use fathom_ingest::repo::{RefFilter, RefKind, RefTarget};
    let mut tips: Vec<(String, String, &'static str)> = Vec::new();
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
    Ok(tips)
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
    let row = store
        .read_commit(&commit.hex())
        .map_err(|_| Error::MissingCommitRow {
            what: format!("commit {} has no row in the store", &commit.hex()[..12]),
        })?;
    let tree = hex_oid(&row.tree_oid, "tree")?;
    // The first parent's *tree*, because that is the side normalize diffed a merge against — asking the
    // second parent would compute a delta this store never claimed to record. It has to be the tree and not
    // the commit: `structure.delta` is keyed on tree identity (§5.3) so that two commits reaching the same
    // tree analyse it once, and keying it on a commit id would make every lookup a miss while looking
    // exactly as warm as if it had worked.
    let parent_tree = match row.parent_oids.first() {
        Some(hex) => Some(hex_oid(
            &store
                .read_commit(hex)
                .map_err(|_| Error::MissingCommitRow {
                    what: format!(
                        "the first parent {} has no row in the store, which normally means its own \
                         objects could not be read; `fathom doctor` names the commits the object \
                         database is hiding",
                        &hex[..12]
                    ),
                })?
                .tree_oid,
            "parent tree",
        )?),
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

/// How many delta links a tree's inventory chain carries before it is anchored again.
///
/// The cadence is `s3`'s: ADR-004 §13.7 anchors a shared graph snapshot every 32 revisions for the same
/// reason — a chain that is never re-anchored is cheap to write and ruinous to read. Thirty-two small
/// links read back in well under a millisecond, and one tree walk per 32 commits is 3 % of the cost of
/// walking the tree on every commit, which is what this used to do.
const INVENTORY_ANCHOR_EVERY: u32 = 32;

/// The most a chain is walked backwards when one is read back. Past it the tree is enumerated instead,
/// which is always right (§7.6: a bad read degrades to re-derivation, never to a partial answer).
const INVENTORY_CHAIN_LIMIT: u32 = INVENTORY_ANCHOR_EVERY * 4;

/// Whether a link whose parent sits `dep` links below its anchor has to be an anchor itself.
///
/// Spelled out as a function because it is the whole anchor policy, and the policy is what a reader of
/// §6.1 R2 will want to check: at most [`INVENTORY_ANCHOR_EVERY`] delta links ever sit between two
/// anchors, in either order of derivation, on either side of a crash.
const fn needs_anchor(dep: u32) -> bool {
    dep >= INVENTORY_ANCHOR_EVERY
}

/// The `structure.delta` unit: the file inventory one tree introduces (§6's row one).
///
/// The payload is a **chain**, not a copy. A link is either an anchor — the tree enumerated, the only
/// thing in this function that costs the repository — or a delta: the paths this commit moved, and the
/// parent tree they apply to. Every link is O(change) to write except one in [`INVENTORY_ANCHOR_EVERY`],
/// which is O(tree), so §6.1 R2's "the cost is the change, never the repository" is a property of the
/// code and not a hope.
///
/// It was not, before `e7.2`. This function wrote the whole inventory into every row and read the parent's
/// whole inventory to build it, which is O(files) per commit in time and in bytes; measured on real
/// repositories (`benchmarks/reports/e7-2-real-repos.json`) that was 5.4–6.3 ms and 172 KB of payload per
/// commit, 76 % of `git/git`'s cold index and the largest single term in the store's analysis cache. And
/// the merge it was built around never fired: it asked for the *parent's* row while the derivation loop
/// walked commits in hash order, so the answer was missing every time and every commit enumerated.
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
    // The parent's link is the whole trick, and reading it is now cheap because a link is small. What the
    // link says is only "how long is the chain below me" — the delta itself never needs the parent's files,
    // because the commit's own changed paths are already in `units`.
    let parent = match parent_tree {
        None => None,
        Some(parent) => store
            .resolve(
                "structure.delta",
                &parent.hex(),
                Some("tree"),
                &options.versions,
                &options.config,
                now,
            )?
            .payload
            .as_deref()
            .and_then(Link::from_json)
            .map(|link| (parent, link)),
    };
    // No link to build on — a root commit, a shallow boundary, a parent this run has not reached yet, or a
    // payload in some older shape — or a chain that has run its length: spend a tree walk, once, so that
    // every reader after this one pays at most `INVENTORY_ANCHOR_EVERY` small links.
    let link = match &parent {
        None => Link::enumerate(repo, tree)?,
        Some((_, prev)) if needs_anchor(prev.dep) => Link::enumerate(repo, tree)?,
        Some((parent, prev)) => Link::delta(repo, *parent, prev.dep + 1, units)?,
    };
    let cost_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    store.put(&outcome.key, link.to_json().as_bytes(), None, cost_ms, now)?;
    Ok(1)
}

/// One file in an inventory: `[path, mode, size]`, positional.
///
/// Positional because this is written once per changed path and read back on every hit, and saying
/// `"path"`/`"mode"`/`"size"` three times per row costs more than the values do. The tuple order is part
/// of the payload's shape, which is what `structure.delta`'s analyzer version is for (§5.5).
type InventoryFile = (String, u32, u64);

/// One link of a tree's inventory chain — an anchor, or the change one commit made to its parent.
///
/// Both kinds are the same shape so that one reader handles both: apply `gone`, then `files`, walking
/// from the anchor down. `full` is the tree enumerated; `delta` is the parent's inventory plus this
/// commit's changed paths.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Link {
    /// `full` or `delta`, and recorded in the payload because "cold" and "re-derived" are different
    /// diagnoses (§5.6).
    kind: String,
    /// Links between this one and the anchor below it; `0` on an anchor.
    dep: u32,
    /// The tree this delta applies to — the first parent's tree. Absent on an anchor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    base: Option<String>,
    /// Files this link introduces, as `[path, mode, size]`: every file on an anchor, the added and
    /// modified ones on a delta.
    files: Vec<InventoryFile>,
    /// Paths this link removes. A rename is its source here and its destination in `files`, which is why
    /// `old_path` is a field of the unit rather than a second unit: the alternative is charging a moved
    /// file its full size as churn, the mistake §6's invalidation matrix exists to prevent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    gone: Vec<String>,
    /// Directories, counted where a tree was actually walked. A delta cannot say anything true about
    /// directories — whether a path's parent already exists is a fact about the tree it lands in — so a
    /// chain carries its anchor's count and does not invent a new one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dirs: Option<usize>,
    /// The enumeration hit its walk cap (§6's truncation rule), so the inventory is a stated partial.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    truncated: bool,
}

impl Link {
    /// Enumerate a tree: the one thing in this lane that costs the repository.
    fn enumerate(repo: &Repository, tree: Oid) -> Result<Self, Error> {
        let listing = repo.tree_files(tree, None)?;
        Ok(Link {
            kind: "full".to_owned(),
            dep: 0,
            base: None,
            files: listing
                .files
                .iter()
                .map(|file| (file.path.quoted(), file.mode, file.size))
                .collect(),
            gone: Vec::new(),
            dirs: Some(listing.directories),
            truncated: listing.truncated,
        })
    }

    /// The change one commit's paths make to its parent's inventory.
    ///
    /// The only git work this does is reading the object *headers* of the paths that changed, which
    /// inflate nothing (`Repository::object_header`'s whole reason for existing).
    fn delta(repo: &Repository, base: Oid, dep: u32, units: &[Unit]) -> Result<Self, Error> {
        let mut files: BTreeMap<String, (u32, u64)> = BTreeMap::new();
        let mut gone: BTreeSet<String> = BTreeSet::new();
        for unit in units {
            if let Some(old) = &unit.old_path {
                let _ = gone.insert(old.clone());
            }
            let Some(after) = unit.after else {
                // Gone. A directory disappearing is the sum of its files disappearing, and each of those
                // is its own unit, so nothing needs counting separately.
                let _ = files.remove(&unit.path);
                let _ = gone.insert(unit.path.clone());
                continue;
            };
            let _ = gone.remove(&unit.path);
            let size = repo.object_header(after)?.map_or(0, |header| header.size);
            let _ = files.insert(unit.path.clone(), (unit.mode.unwrap_or(0o100644), size));
        }
        Ok(Link {
            kind: "delta".to_owned(),
            dep,
            base: Some(base.hex()),
            files: files
                .into_iter()
                .map(|(path, (mode, size))| (path, mode, size))
                .collect(),
            gone: gone.into_iter().collect(),
            dirs: None,
            truncated: false,
        })
    }

    /// Apply this link to a file map, in the direction the chain is read: down from the anchor.
    fn apply(&self, files: &mut BTreeMap<String, (u32, u64)>) {
        for path in &self.gone {
            let _ = files.remove(path);
        }
        for (path, mode, size) in &self.files {
            let _ = files.insert(path.clone(), (*mode, *size));
        }
    }

    /// Compact JSON. Infallible by construction: every field is a string, an integer or a bool.
    fn to_json(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| String::from(r#"{"kind":"full","dep":0,"files":[]}"#))
    }

    /// `None` for anything that is not a payload this wrote — a truncated blob, an older shape, a corrupt
    /// byte. The caller then enumerates the tree, which is always right (§7.6).
    fn from_json(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

/// How a read-back inventory was made, so a reader can tell what it cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Derived {
    /// An anchor was reached, and the deltas above it were applied: the chain carried the answer.
    Chain,
    /// The chain was missing, unreadable or longer than [`INVENTORY_CHAIN_LIMIT`], so the tree was
    /// enumerated instead (§7.6's degrade-to-re-derivation).
    Enumerated,
}

/// A tree's file inventory, read back out of the chain `structure.delta` keeps.
#[derive(Debug, Clone)]
pub struct Inventory {
    /// Every file the tree holds, as `[path, mode, size]`, sorted by path. The path is quoted the way
    /// `git_commit_file.new_path` quotes it — a byte-injective spelling, which is what lets two links be
    /// merged without a lossy comparison.
    pub files: Vec<InventoryFile>,
    /// Directories, from the anchor the walk reached; `None` when the tree was enumerated by a path that
    /// did not count them.
    pub directories: Option<usize>,
    /// The enumeration that produced an anchor hit its walk cap.
    pub truncated: bool,
    /// How this answer was made.
    pub derived: Derived,
    /// Delta links walked to get here; `0` for an anchor or a fresh enumeration.
    pub links: u32,
}

/// Read a tree's inventory back: walk the chain of `structure.delta` links up to its anchor, then apply
/// the links forward.
///
/// This is the reader the lane's contract implies and nothing shipped yet uses: the delta is only worth
/// storing if the inventory it describes can still be had. It costs at most
/// [`INVENTORY_CHAIN_LIMIT`] small reads, and when it cannot get there — a pruned link, an old payload, a
/// chain from before this representation existed — it enumerates the tree, which is slower and exactly
/// correct.
///
/// # Errors
///
/// [`Error`] if the store or the repository cannot be read.
pub fn inventory(
    store: &mut Store,
    repo: &Repository,
    options: &Options,
    tree: Oid,
    now: i64,
) -> Result<Inventory, Error> {
    let mut links: Vec<Link> = Vec::new();
    let mut cursor = tree;
    let mut broken = false;
    for _ in 0..=INVENTORY_CHAIN_LIMIT {
        let outcome = store.resolve(
            "structure.delta",
            &cursor.hex(),
            Some("tree"),
            &options.versions,
            &options.config,
            now,
        )?;
        let Some(link) = outcome.payload.as_deref().and_then(Link::from_json) else {
            broken = true;
            break;
        };
        let anchor = link.kind == "full";
        let base = link.base.clone();
        links.push(link);
        if anchor {
            break;
        }
        let Some(hex) = base else {
            broken = true;
            break;
        };
        cursor = match hex.parse::<Oid>() {
            Ok(oid) => oid,
            Err(_) => {
                broken = true;
                break;
            }
        };
    }
    let walked = links.len();
    let anchor = match links.last() {
        Some(last) if last.kind == "full" && !broken => last.clone(),
        // Nothing to build from: a pruned link, a payload from before this shape, a chain longer than the
        // walk allows. Enumerate, and say in the answer that that is what happened.
        _ => {
            let enumerated = Link::enumerate(repo, tree)?;
            return Ok(Inventory {
                files: enumerated.files,
                directories: enumerated.dirs,
                truncated: enumerated.truncated,
                derived: Derived::Enumerated,
                links: 0,
            });
        }
    };
    let mut files: BTreeMap<String, (u32, u64)> = BTreeMap::new();
    for link in links.iter().rev() {
        link.apply(&mut files);
    }
    Ok(Inventory {
        files: files
            .into_iter()
            .map(|(path, (mode, size))| (path, mode, size))
            .collect(),
        directories: anchor.dirs,
        truncated: anchor.truncated,
        derived: Derived::Chain,
        links: u32::try_from(walked.saturating_sub(1)).unwrap_or(u32::MAX),
    })
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
    fn a_link_round_trips_through_its_own_payload() {
        let link = Link {
            kind: "delta".to_owned(),
            dep: 3,
            base: Some("a".repeat(40)),
            files: vec![("src/main.rs".to_owned(), 0o100_755, 4_096)],
            gone: vec![("README.md".to_owned())],
            dirs: None,
            truncated: false,
        };
        let read = Link::from_json(link.to_json().as_bytes()).expect("the payload is this shape");
        assert_eq!(read.kind, "delta");
        assert_eq!(read.dep, 3);
        assert_eq!(read.base.as_deref(), Some("a".repeat(40).as_str()));
        assert_eq!(
            read.files,
            vec![("src/main.rs".to_owned(), 0o100_755, 4_096)]
        );
        assert_eq!(read.gone, vec!["README.md".to_owned()]);
        // An anchor says nothing about a parent and nothing about directories, and does not pay for the
        // two `None`s and the empty list in every row of the bigger payload.
        let anchor = Link {
            kind: "full".to_owned(),
            dep: 0,
            base: None,
            files: vec![("README.md".to_owned(), 0o100_644, 12)],
            gone: Vec::new(),
            dirs: Some(2),
            truncated: false,
        };
        let json = anchor.to_json();
        assert!(!json.contains("base"), "an anchor names no parent: {json}");
        assert!(!json.contains("gone"), "and removes nothing: {json}");
        assert!(
            json.contains("\"dirs\":2"),
            "and counts what it counted: {json}"
        );
        // Anything that is not this shape is `None`, and the caller re-derives rather than trusting it.
        assert!(Link::from_json(b"not json").is_none());
    }

    #[test]
    fn a_chain_of_links_applies_in_the_direction_it_is_read() {
        // The reader walks up to the anchor and applies down from it, so the anchor's list is the base
        // and every delta after it is one more edit: add, modify, move, delete, in that order of applying.
        let anchor = Link {
            kind: "full".to_owned(),
            dep: 0,
            base: None,
            files: vec![
                ("README.md".to_owned(), 0o100_644, 12),
                ("src/main.rs".to_owned(), 0o100_644, 10),
            ],
            gone: Vec::new(),
            dirs: Some(1),
            truncated: false,
        };
        let moved = Link {
            kind: "delta".to_owned(),
            dep: 1,
            base: None,
            files: vec![("src/lib.rs".to_owned(), 0o100_644, 10)],
            gone: vec![("src/main.rs".to_owned())],
            dirs: None,
            truncated: false,
        };
        let last = Link {
            kind: "delta".to_owned(),
            dep: 2,
            base: None,
            files: vec![("docs/x.md".to_owned(), 0o100_644, 3)],
            gone: vec![("README.md".to_owned())],
            dirs: None,
            truncated: false,
        };
        // Collected the way the reader collects them — the tree asked about first, its parent next, the
        // anchor last — and applied in the opposite direction, which is what [`inventory`] does and what
        // this test exists to pin: apply oldest-first, so a later link's removal wins over an earlier
        // link's addition and the result is the newest tree rather than the anchor's.
        let links = [&last, &moved, &anchor];
        let mut files: BTreeMap<String, (u32, u64)> = BTreeMap::new();
        for link in links.iter().rev() {
            link.apply(&mut files);
        }
        assert_eq!(
            files
                .into_iter()
                .map(|(path, (mode, size))| (path, mode, size))
                .collect::<Vec<_>>(),
            vec![
                ("docs/x.md".to_owned(), 0o100_644, 3),
                ("src/lib.rs".to_owned(), 0o100_644, 10),
            ],
            "the tree the three commits arrive at, and nothing left over from the one they started at"
        );
    }

    #[test]
    fn a_chain_is_never_longer_than_the_anchor_interval() {
        // The read cost bound, as a fact about the policy rather than about one repository.
        assert!(!needs_anchor(0));
        assert!(!needs_anchor(INVENTORY_ANCHOR_EVERY - 1));
        assert!(needs_anchor(INVENTORY_ANCHOR_EVERY));
        assert!(needs_anchor(INVENTORY_ANCHOR_EVERY * 7));
        assert!(
            INVENTORY_CHAIN_LIMIT > INVENTORY_ANCHOR_EVERY,
            "a reader must be able to reach an anchor before it gives up and enumerates"
        );
    }
}
