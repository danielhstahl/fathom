//! The index's graph pass (`e3.11`, `reco-dlp`): writing a revision's module graph from the repository.
//!
//! Every architecture surface in Fathom reads the module graph — `fathom architecture`, `fathom ratchet`,
//! the `layering` and `cycles` halves of `fathom check`, the `g` pane, the hotspot and API reports. Every
//! one of them was, until here, reading a store nothing in the shipped binary wrote: `Lane::record` existed,
//! was tested from `crates/fathom-analyze/tests/graph_lane.rs`, and had no producer. A repository whose
//! index cannot answer "did this turn make the architecture worse?" is the difference between Epic 3 being
//! a plan and Epic 3 being a product, so the producer lives here and `fathom index` calls it.
//!
//! # The four rules this pass exists to obey
//!
//! * **A revision's graph is a function of the whole corpus, so it is computed that way.** The extractor
//!   resolves imports *within a corpus* (`engine::extract_all`: a bounded fixpoint over every file's
//!   drafts), which means one file's structural record is not a function of its own bytes — move a module
//!   and every importer's resolved edges move with it, without a byte changing in any of them. So the pass
//!   reads the live corpus and calls [`crate::structure::extract`] once per revision, and does not pretend
//!   there is a per-blob cache underneath. There isn't, yet: see the ticket split out from this one.
//! * **The clock stays outside.** `Lane::record` is handed an instant and does not read one (ADR-002 §4.4).
//!   This pass stamps each revision with **its own commit's date**, not with the instant the run started:
//!   `graph_revisions()` is ordered `built_utc DESC`, so a pass that stamped `now` would make every
//!   revision of a cold backfill equally new and turn "the newest revision the graph holds" into a coin
//!   toss. Stamping the commit date also makes a rebuild byte-identical to the run it replaces, which is
//!   what `e3.2`'s reproducibility criterion then asserts.
//! * **A chain is only as good as its base.** `record` computes a delta against the cursor the lane is
//!   *holding*, so a base may only be claimed when [`Lane::cursor_tree`] agrees. Where it does not — the
//!   first revision of a pass, a gap in the history, a merge whose first parent is elsewhere — the revision
//!   is recorded as an anchor and says so, rather than writing a delta that lies about its parent.
//! * **The cost is bounded and named.** `graph.window` is the lane's *coverage* window: one pass writes the
//!   owed revisions among the newest `window` revisions and refuses everything older, rather than queueing
//!   it for a later run (`reco-r3x`). `graph.max_corpus_mib` caps what one revision's structural pass may
//!   read. A pass that refuses says which knob refused it, because a slow surprise is how the next person
//!   turns `graph.enabled` off without ever getting an answer — and because a quiet run that quietly spent
//!   a second on history is the same lie told on the cheap path.
//!
//! # What one revision costs
//!
//! One tree enumeration, the blobs of its live source files, and one corpus extraction. The enumeration is
//! why the pass walks a tree per revision rather than folding each commit's changed paths into the last
//! revision's live list: the fold is cheaper and is what a `structure.delta` link does, but a corpus
//! extraction dominates it by two orders of magnitude, and being wrong about one renamed file in a chain
//! of thirty is worse than a tree walk.

use std::collections::BTreeMap;
use std::time::Duration;

use crate::error::Error;
use crate::graph::{GraphPolicy, Lane, ModuleInput, builder_from_config_text};
use crate::index::Options;
use crate::structure::{ExtractPolicy, FileStructure, Language, SourceFile};
use fathom_ingest::oid::Oid;
use fathom_ingest::repo::{BlobRead, Repository};
use fathom_store::graph::PendingRevision;
use fathom_store::spine::Store;

/// What one graph pass did, in the units a report line prints.
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// Revisions written by this pass.
    pub revisions: usize,
    /// How many of them are anchors — `Lane::cursor_tree` disagreed about their base, or the chain ran its
    /// length.
    pub anchors: usize,
    /// Modules in the newest revision this pass wrote, ignoring the ones `graph.ignored_dirs` dropped.
    pub modules: usize,
    /// Nodes in the newest revision.
    pub nodes: usize,
    /// Edges in the newest revision.
    ///
    /// Edges as the graph stores them, which is what a dependency claim is made of: module-to-module,
    /// import and wildcard and package-granular kinds together.
    pub edges: usize,
    /// Modules whose structural record could not be read, with the reason counted rather than guessed at:
    /// over `extract.max_file_bytes`, not UTF-8, a blob git will not serve.
    pub unavailable: usize,
    /// Files the tree held that are not modules: not one of the six languages, or under
    /// `graph.ignored_dirs`.
    pub skipped: usize,
    /// Rows deleted because `--refresh` asked for the lane to be re-derived (ADR-004 §9.5: derived data is
    /// recomputed, not migrated), as `(table, rows)`.
    pub reset: Vec<(String, i64)>,
    /// Wall time, stamped by the caller that ran the pass (`index::run`) rather than by this module: the
    /// analyzer reads no clock of its own (ADR-002 §4.4), and one clock in the index driver is what makes
    /// this number comparable with the other phase numbers.
    pub elapsed: Duration,
    /// Did this pass run at all? `false` means `graph.enabled`/`--no-graph` turned it off, which is a
    /// different statement from "ran and owed nothing", and the JSON says which (`e3.11`).
    pub ran: bool,
    /// Why the pass stopped short of graphing everything, when it did: a cancellation, a corpus over
    /// `graph.max_corpus_mib`, or revisions older than `graph.window`. Stated, because a graph that stops
    /// halfway must not read as a graph that is finished (§7.6).
    pub partial: Option<String>,
    /// How many owed revisions the window refused — what the lane owes that no pass will ever write at this
    /// configuration (`graph.window`; the store's own number is `PendingWork::owed`).
    ///
    /// Separate from [`Report::partial`] because it is not progress paused: it is what the configuration
    /// chose. A run that wrote nothing *and* refused nothing is the quiet case, and it prints nothing.
    pub outside_window: usize,
    /// The notices from `[graph]`, `[layers]` and `[extract]`, plus at most a handful of the refusals: a
    /// typo in a config file is a notice and a default, never a lost architecture view.
    pub notices: Vec<String>,
}

/// How many refusal notices one pass prints. The count is in [`Report::unavailable`]; the notices are
/// examples, and a repository with four thousand unreadable files should not print four thousand lines.
pub const NOTICE_LIMIT: usize = 6;

impl Report {
    /// The report as data: `fathom index --json`'s `graph` section (`e3.11`).
    ///
    /// `None` when the pass never ran, which serialises as `null` — the same convention `prune` uses, so a
    /// consumer can tell "not asked" from "asked, nothing to do" without reading a zero as a verdict.
    #[must_use]
    pub fn json(&self) -> Option<serde_json::Value> {
        if !self.ran {
            return None;
        }
        Some(serde_json::json!({
            "revisions": self.revisions,
            "anchors": self.anchors,
            "modules": self.modules,
            "nodes": self.nodes,
            "edges": self.edges,
            "unavailable": self.unavailable,
            "skipped": self.skipped,
            "reset_rows": self.reset.iter().map(|(_, rows)| rows).sum::<i64>(),
            "elapsed_ms": self.elapsed.as_millis(),
            "partial": self.partial,
            "outside_window": self.outside_window,
            "notices": self.notices,
        }))
    }

    /// Did this pass write anything at all?
    #[must_use]
    pub fn wrote_anything(&self) -> bool {
        self.revisions > 0 || !self.reset.is_empty()
    }

    /// The line `fathom index` prints. Empty when the pass had nothing to say, so a caller can print the
    /// report only when there is a report to print. A pass that wrote nothing *because it refused* still has
    /// a line: the refusal is the news, and a run that declined in silence is the failure this ticket exists
    /// to prevent.
    #[must_use]
    pub fn sentence(&self) -> String {
        if !self.wrote_anything() {
            return match &self.partial {
                Some(why) => format!("module graph: nothing written — {why}"),
                None => String::new(),
            };
        }
        let mut out = format!(
            "module graph: {revision} revision{revision_plural} ({anchor} anchor{anchor_plural}) \
             · {modules} modules, {edges} edges in the newest · {ms} ms",
            revision = self.revisions,
            revision_plural = if self.revisions == 1 { "" } else { "s" },
            anchor = self.anchors,
            anchor_plural = if self.anchors == 1 { "" } else { "s" },
            modules = self.modules,
            edges = self.edges,
            ms = self.elapsed.as_millis(),
        );
        if self.unavailable > 0 {
            out.push_str(&format!(
                " · {n} module(s) with no structural record (`extract.max_file_bytes`, or a file git \
                 will not serve)",
                n = self.unavailable
            ));
        }
        if let Some(why) = &self.partial {
            out.push_str(&format!(" · stopped early: {why}"));
        }
        if !self.reset.is_empty() {
            out.push_str(" · re-derived from nothing (--refresh)");
        }
        out
    }
}

/// One revision's live modules: `(path, blob)` pairs sorted by path, and the bytes the pass read for the
/// ones it could read.
#[derive(Default)]
struct Corpus {
    /// Every module the tree holds, sorted by path — `Lane::record`'s whole live list.
    live: Vec<(String, String)>,
    /// The structural record per blob. A blob read once, extracted once: the same blob at two paths is one
    /// parse, which is also the only reuse available to a pass that must resolve a whole corpus.
    records: BTreeMap<String, FileStructure>,
    /// Modules the corpus extraction refused, with the reason.
    unavailable: usize,
    /// Files that are not modules at all.
    skipped: usize,
    /// Bytes handed to the extractor, which is what `graph.max_corpus_mib` is measured against.
    bytes: usize,
    /// At most [`NOTICE_LIMIT`] of the refusals, named. [`Corpus::unavailable`] is the number a report
    /// prints; these are the examples a reader can act on.
    notes: Vec<String>,
}

impl Corpus {
    /// Record a refusal, up to the limit. A repository with four thousand unreadable files prints six
    /// examples and the total, not four thousand lines.
    fn note(&mut self, note: &str) {
        if self.notes.len() < NOTICE_LIMIT {
            self.notes.push(note.to_owned());
        }
    }
}

/// Write a graph for every revision the store does not have one for, newest `graph.window` of them.
///
/// `options.graph` decides whether this does anything at all; [`crate::index::GraphPass::Off`] is the
/// cheap, honest no-op and returns a default report. The pass may stop early — at a cancellation, at the
/// corpus cap — and every early stop lands in [`Report::partial`], never hidden (§7.6).
///
/// The window is a cutoff and not a batch, which is the difference between a quiet run and a cheap one
/// (`reco-r3x`). Read as a batch size, `graph.window = 32` on a 500-commit history means the runs *after*
/// the cold one each settle 32 more of the revisions it left behind — ten runs at a second each, including
/// the ones that answer `nothing_new: true` — and the store ends up holding a graph row for every revision
/// anyway. Read as a cutoff, the same configuration means "the lane covers the newest 32 revisions": the
/// no-op writes nothing and costs milliseconds, and a repository that wants the longer trend raises the
/// knob and says so out loud. `Store::graph_pending_revisions` is where the store says which of the two it
/// is, and `graph.window = 0` stays "every owed revision, no cutoff".
///
/// # Errors
///
/// [`Error::Store`] when the store will not read or write, and [`Error::Ingest`] when a *tree* cannot be
/// walked. One unreadable blob is not an error: it is one module with no structural record.
pub fn run(
    repo: &Repository,
    store: &mut Store,
    options: &Options,
    now: i64,
) -> Result<Report, Error> {
    let crate::index::GraphPass::On { config_text } = &options.graph else {
        return Ok(Report::default());
    };
    let mut report = Report {
        ran: true,
        ..Default::default()
    };

    // `[layers]`, `[graph]` and `[extract]`, read from the text the CLI handed over: the crate that owns
    // the repository's config file is the CLI, and this is the crate that has to obey it (ADR-002 §4.5).
    let (builder, notices) = builder_from_config_text(config_text);
    report.notices = notices;
    let (extract, extract_notices) = ExtractPolicy::from_repo_config_text(config_text);
    report.notices.extend(extract_notices);
    let policy = builder.policy.clone();

    // The repository's own `graph.enabled = false` is obeyed here as well as by the CLI that calls this, so a
    // caller that forgot to ask cannot write a lane into a repository that refused one. Refusing means doing
    // nothing at all — including not honouring `--refresh`, which would otherwise delete the lane the pass
    // has just declined to rebuild.
    if let Some(why) = policy.disabled_reason() {
        report.ran = false;
        report.notices.push(format!("graph: off — {why}"));
        return Ok(report);
    }

    // `--refresh` is the documented way to ask "re-derive what the cache already answered", and the graph
    // lane is derived data: a `[layers]` change or an extractor bump means the stored chain was built
    // under rules that no longer hold, and ADR-004 §9.5's answer is to recompute rather than patch. There
    // is no per-tree version of this: a chain is intact from its anchor or it is not intact.
    if options.refresh {
        let removed = store.graph_reset()?;
        report.reset = removed.into_iter().collect();
    }

    // The work list comes from the store, not from the run: a repository indexed before the lane had a
    // writer gets backfilled by the next ordinary `fathom index`, and an interrupted pass is finished by
    // the next one rather than being lost with the run that stopped. What is *older* than the window is
    // not finished by the next one, and no longer pretends to be.
    let work = store.graph_pending_revisions(if policy.window > 0 {
        policy.window
    } else {
        usize::MAX
    })?;
    let behind = work.owed.saturating_sub(work.revisions.len());
    report.outside_window = behind;
    if work.revisions.is_empty() {
        return Ok(report);
    }
    if behind > 0 {
        report.partial = Some(format!(
            "{behind} owed revision(s) are older than `graph.window = {window}`, which is a coverage \
             window and not a queue; raise it, or set it to 0, to backfill them",
            window = policy.window
        ));
    }
    let work = &work.revisions;

    // One reborrow, held for the whole loop: the lane takes `&mut Store` and keeps it, so nothing after
    // this line may touch the store through the caller's reference — which is why the work list was read
    // above and every store write this pass makes goes through the lane.
    let store = &mut *store;
    let mut lane = Lane::new(store, builder)?;
    for target in work {
        if let Some(cancel) = &options.cancel
            && cancel.is_triggered()
        {
            report.partial = Some("the run was cancelled".to_owned());
            break;
        }
        if let Some(progress) = &options.progress {
            progress.report(&crate::index::Tick {
                phase: crate::index::Phase::Graph,
                done: u64::try_from(report.revisions).unwrap_or(u64::MAX),
                of: Some(u64::try_from(work.len()).unwrap_or(u64::MAX)),
            });
        }
        let corpus = corpus_of(repo, &policy, &extract, target)?;
        if corpus.bytes > policy.max_corpus_bytes {
            let note = format!(
                "its newest revision holds {:.1} MiB of source, over `graph.max_corpus_mib = {}`; \
                 raise the knob, narrow `graph.ignored_dirs`, or set `graph.enabled = false` to stop \
                 asking",
                corpus.bytes as f64 / fathom_core::memory::MIB as f64,
                policy.max_corpus_bytes / fathom_core::memory::MIB as usize
            );
            report.partial = Some(match report.partial.take() {
                Some(behind) => format!("{behind} And it stopped at the corpus cap: {note}"),
                None => note,
            });
            report.unavailable += corpus.unavailable;
            report.skipped += corpus.skipped;
            report.notices.extend(corpus.notes);
            break;
        }
        report.unavailable += corpus.unavailable;
        report.skipped += corpus.skipped;
        report.notices.extend(corpus.notes);

        let mut inputs: Vec<ModuleInput<'_>> = Vec::with_capacity(corpus.live.len());
        for (path, blob) in &corpus.live {
            inputs.push(ModuleInput {
                path,
                blob,
                file: corpus.records.get(blob),
            });
        }
        let base = lane
            .cursor_tree()
            .zip(target.first_parent_tree.as_deref())
            .filter(|(cursor, parent)| *cursor == *parent)
            .map(|(_, parent)| parent.to_owned());
        let stamp = if target.when > 0 { target.when } else { now };
        let summary = lane.record(&target.tree_oid, base.as_deref(), &inputs, stamp)?;
        report.revisions += 1;
        if summary.is_anchor {
            report.anchors += 1;
        }
        report.modules = summary.build.modules.saturating_sub(summary.build.ignored);
        report.nodes = summary.nodes;
        report.edges = summary.edges;
    }

    // The lane gives the store back before the counters are written: one writer of a store at a time, and
    // the borrow checker is the thing that says so here rather than a convention.
    drop(lane);
    store.bump_counter(
        "graph.revise.revisions",
        i64::try_from(report.revisions).unwrap_or(i64::MAX),
    );
    store.bump_counter(
        "graph.revise.unavailable",
        i64::try_from(report.unavailable).unwrap_or(i64::MAX),
    );
    Ok(report)
}

/// Read one revision's live corpus: enumerate the tree, read the source blobs, extract them together.
///
/// The whole tree, every revision, on purpose: a revision's structural records are a function of the corpus
/// they were resolved against, so a fold over the previous revision's live list could only be correct if
/// the fold also re-resolved every importer of anything that moved — which is the corpus pass again. See
/// the module docs and the follow-up ticket.
fn corpus_of(
    repo: &Repository,
    policy: &GraphPolicy,
    extract: &ExtractPolicy,
    target: &PendingRevision,
) -> Result<Corpus, Error> {
    let mut corpus = Corpus::default();
    let Ok(oid) = target.tree_oid.parse::<Oid>() else {
        // A tree id in the store that will not parse is a broken row, not a broken repository, and there is
        // no key to hang the finding on except the report.
        return Ok(corpus);
    };
    let listing = repo.tree_files(oid, None)?;
    let mut sources: Vec<(String, Vec<u8>)> = Vec::with_capacity(listing.files.len());
    let mut read: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for file in &listing.files {
        let path = file.path.quoted();
        if policy.ignores(&path) || !Language::detect(&path).is_known() {
            // Not a module: it still got its file-level analysis from `e1.4`, somewhere else.
            corpus.skipped += 1;
            continue;
        }
        let blob = file.oid.hex();
        corpus.live.push((path.clone(), blob.clone()));
        if file.size > u64::try_from(extract.max_file_bytes).unwrap_or(u64::MAX) {
            corpus.unavailable += 1;
            corpus.note(&format!(
                "{path}: {} bytes is over `extract.max_file_bytes = {}`, so it is a module with no structural record",
                file.size, extract.max_file_bytes
            ));
            continue;
        }
        if !read.insert(blob) {
            // The same content at a second path: one parse, two modules, and the lane sees both.
            continue;
        }
        match repo.blob(file.oid, extract.max_file_bytes) {
            Ok(BlobRead::Present { bytes, .. }) => {
                corpus.bytes += bytes.len();
                sources.push((file.path.quoted(), bytes));
            }
            // Over the cap after all, or not a blob, or not there. Either way the module stays in the
            // graph with no structural record, which is the state the lane already spells.
            Ok(_) | Err(_) => {
                corpus.unavailable += 1;
                corpus.note(&format!(
                    "{path}: git would not serve the blob, so it is a module with no structural record"
                ));
            }
        }
    }
    corpus.live.sort();

    // One corpus, in path order: resolution is a fixpoint over the corpus, and its answer must not depend
    // on the order the tree walk happened to hand the files over.
    let inputs: Vec<SourceFile<'_>> = sources
        .iter()
        .map(|(path, bytes)| SourceFile::new(path, bytes))
        .collect();
    let extracted = crate::structure::extract(&inputs, extract);
    let mut path_to_blob: BTreeMap<String, String> = BTreeMap::new();
    for (path, blob) in &corpus.live {
        let _ = path_to_blob.insert(path.clone(), blob.clone());
    }
    for record in extracted.files {
        if record.structure.is_none() {
            corpus.unavailable += 1;
            corpus.note(&format!(
                "{}: the structural extractor refused it, so it is a module with no record",
                record.path
            ));
        }
        // The record is keyed by the *blob*, because that is what the lane compares. Two paths, one blob:
        // one record, and the record's own `path` field is the first of them, which nothing downstream
        // reads — the lane keys a module by `ModuleInput::path`, not by the record's.
        if let Some(blob) = path_to_blob.get(&record.path) {
            let _ = corpus.records.insert(blob.clone(), record);
        }
    }
    Ok(corpus)
}

/// Whether a path is a module the graph should hold: not ignored, and one of the six grammars.
///
/// Kept as a function rather than inlined into [`corpus_of`] because the tests below are the readable
/// statement of what `[graph] ignored_dirs` and `Language::Unknown` actually do.
#[cfg(test)]
fn is_module(policy: &GraphPolicy, path: &str) -> bool {
    !policy.ignores(path) && Language::detect(path).is_known()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_that_wrote_nothing_says_nothing() {
        assert!(!Report::default().wrote_anything());
        assert!(Report::default().sentence().is_empty());
    }

    #[test]
    fn the_sentence_says_what_it_wrote_and_only_what_it_wrote() {
        let report = Report {
            revisions: 3,
            anchors: 1,
            modules: 7,
            edges: 9,
            elapsed: Duration::from_millis(41),
            ..Report::default()
        };
        let line = report.sentence();
        assert!(line.contains("3 revisions"), "{line}");
        assert!(line.contains("(1 anchor)"), "{line}");
        assert!(line.contains("7 modules, 9 edges"), "{line}");
        assert!(line.contains("41 ms"), "{line}");
        assert!(!line.contains("no structural record"), "{line}");
        assert!(!line.contains("stopped early"), "{line}");
        assert!(!line.contains("--refresh"), "{line}");
    }

    #[test]
    fn a_refusal_has_a_line_and_an_ordinary_warm_run_does_not() {
        // The distinction the whole report exists to keep: "nothing was owed" prints nothing, and "we
        // declined" prints the reason, because a silent decline is indistinguishable from the first.
        assert!(Report::default().sentence().is_empty());
        let refused = Report {
            ran: true,
            partial: Some(
                "its newest revision holds 214.0 MiB of source, over `graph.max_corpus_mib = 64"
                    .to_owned(),
            ),
            ..Report::default()
        };
        let line = refused.sentence();
        assert!(line.contains("nothing written"), "{line}");
        assert!(line.contains("graph.max_corpus_mib"), "{line}");
        assert!(!line.contains("re-derived"), "{line}");
    }

    #[test]
    fn a_stated_partial_is_in_the_sentence() {
        let report = Report {
            revisions: 1,
            anchors: 1,
            partial: Some("the run was cancelled".to_owned()),
            ..Report::default()
        };
        assert!(
            report
                .sentence()
                .contains("stopped early: the run was cancelled")
        );
    }

    #[test]
    fn the_module_test_is_a_grammar_and_a_directory() {
        let (policy, _) =
            GraphPolicy::from_repo_config_text("[graph]\nignored_dirs = [\"vendor\"]\n");
        assert!(is_module(&policy, "src/lib.rs"));
        assert!(is_module(&policy, "lib/thing.py"));
        assert!(!is_module(&policy, "README.md"));
        assert!(!is_module(&policy, "vendor/dep.go"));
    }
}
