//! Guardrail 3 calibration — the false-positive harness `e4.3` and `m1` are gated on (`e4.3a`, `reco-mkl.1`).
//!
//! ADR-001 §5.4 publishes `G3 ≤ 0.30` at alpha, `≤ 0.15` at 1.0 and `miss_rate ≤ 0.20`, and `e4.3` and
//! `m1` both defer to "the threshold agreed in `e0.12`". This is the thing that measures them: three real
//! OSS repositories pinned by commit, dispositions labelled by a person, and every false positive
//! attributed to the detector that produced it.
//!
//! ```text
//! python3 tools/fetch_g3_corpus.py                        # clone the three pinned repositories
//! cargo run -p fathom-analyze --features sqlite-bundled --example g3_calibration -- \
//!     --report benchmarks/reports/e4-3a-g3-calibration.json
//! cargo run -p fathom-analyze --features sqlite-bundled --example g3_calibration -- --check
//! ```
//!
//! # The four walls
//!
//! 1. **Corpus, truth, thresholds, report** — the shape spikes `s1` and `s2` proved durable. Three real
//!    repositories each pinned to one commit and one window (`corpus/*/pin.json`), a hand-authored
//!    `truth.json` beside each pin, `thresholds.json` for the gates, and a committed report that `--check`
//!    re-measures and refuses to disagree with.
//! 2. **The harness cannot author its own truth.** Nothing here writes to `corpus/*/truth.json`, and the
//!    harness refuses to certify a number whose flagged episodes have no labels — the `s1`/`s2` rule that a
//!    harness measuring its own truth measures the harness. See `crates/fathom-analyze/calibration/g3/README.md`.
//! 3. **The numbers come from the shipped code, not a copy of it.** The score is
//!    [`fathom_analyze::risk::collect::vector`] plus [`fathom_analyze::risk::score::score`] under
//!    [`fathom_analyze::risk::weights::Weights::defaults`]; the findings are
//!    [`fathom_analyze::detectors::scan`] with every detector enabled; the label vocabulary is
//!    [`fathom_analyze::queue::disposition::REASONS`], the closed list `e4.6` owns and §5.4 names as the
//!    labelling mechanism. A τ in `thresholds.json` that disagrees with the shipped default is an error.
//! 4. **Per detector, always.** A guardrail that cannot name which detector cried wolf is a guardrail that
//!    gets relaxed wholesale, so `G3` is printed per detector as well as in aggregate, and `e0.12`'s rule —
//!    a detector that alone breaches the alpha gate ships disabled rather than averaged away — is scored
//!    against the catalogue's shipped defaults.
//!
//! # Determinism
//!
//! The clock is a field in each pin, not a read; the repositories are pinned by commit; the analysis is
//! byte-stable (ADR-001 §7.4). `--check` therefore compares the whole report byte for byte, and there is no
//! `generated_utc` field to make that impossible. The repository under measurement is not configured: its
//! `fathom.toml` is deliberately not read, because a gate whose number depends on somebody else's config is
//! not the same gate twice.

#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::unwrap_used,
    clippy::expect_used
)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use fathom_analyze::detectors;
use fathom_analyze::detectors::model::{Content, Corpus, Episode, FileChange};
use fathom_analyze::detectors::policy::Policy;
use fathom_analyze::index::{self, Scope, Worktree};
use fathom_analyze::queue::is_code;
use fathom_analyze::risk::score;
use fathom_analyze::risk::weights::Weights;
use fathom_analyze::risk::{collect as risk_collect, collect::Supply};
use fathom_ingest::record::{Limits, Unclassified};
use fathom_ingest::repo::Repository;
use fathom_ingest::segment::Rules;
use fathom_store::episode::{EpisodeQuery, EpisodeRow};
use fathom_store::spine::Store;
use serde::Deserialize;
use serde_json::{Value, json};

/// Where the corpus lives, relative to the repository root.
const CALIBRATION: &str = "crates/fathom-analyze/calibration/g3";
/// The default clone directory — under `target/` so a clone is never mistaken for a checked-in asset.
const DEFAULT_WORKDIR: &str = "target/g3-corpus";
/// The default report path.
const DEFAULT_REPORT: &str = "benchmarks/reports/e4-3a-g3-calibration.json";

/// ADR-001 §5.3's `N`: the first five later episodes on an episode's lineage.
const REWORK_N: usize = 5;
/// ADR-001 §5.3's maturity lag: an episode is mature at five successors or seven days.
const MATURE_DAYS: i64 = 7;
/// ADR-001 §5.3 R2's threshold: a successor that takes back a fifth of what an episode added.
const R2_SHARE: f64 = 0.20;
/// R1's content rung: a successor that takes back essentially everything an episode added.
///
/// §5.3 writes R1 as "a commit whose diff reverts a member commit". git spells that two ways: a
/// `Revert "…"` subject, or a diff that removes what the other added. Both are recognised here, and this
/// constant is the share the second kind has to reach. It is a named number rather than a literal in a
/// comparison because it decides whether a squashed fix-up counts as a revert, and a guardrail's constants
/// have to be arguable.
const R1_SHARE: f64 = 0.90;
/// The `e4.4` calibration cut: the top decile by score, which is the queue a reviewer reads first. §5.4's
/// τ is "per-repo tunable", so the decile cut is a second honest reading of the same flag set. The gates
/// are still scored at the published τ.
const DECILE: f64 = 0.10;
/// The smallest labelled denominator a ratio gate will call measurable.
///
/// `G3` over two labelled episodes is not a rate, it is an anecdote, and an anecdote that prints `pass` is
/// worse than a refusal because it stops people asking. A gate over a smaller denominator than this is
/// `not-measurable`, which fails the run unless `thresholds.json` said so out loud.
const MIN_DENOMINATOR: usize = 5;

/// The bar a *default flip* has to clear, which is higher than the bar a measurement has to clear.
///
/// Turning a detector on or off by default changes what every repository sees, so it gets a stricter
/// standard than printing a number: five labelled flags can tell you a rate, they cannot tell you to
/// change everybody's defaults. The number is a threshold, so it is named and stated rather than implied.
const DEFAULT_FLIP_MIN_LABELLED: usize = 8;

/// How many commits the harness walks per measured episode.
///
/// The window is counted in episodes and the walk is counted in commits, and an episode is one to five
/// commits. Four leaves room for a merge-heavy history without walking a repository's whole life: the
/// harness has to be cheap enough that CI actually runs it on every pull request.
const WALK_COMMITS_PER_EPISODE: usize = 4;

/// The three detectors §5.3 R3 names.
const R3_DETECTORS: [&str; 3] = ["rework_loop", "churn_loop", "test_weakening"];

/// The thresholds the report measures the flag arm at, highest first.
///
/// §5.4 publishes one τ and gates on it, which is fine until nothing in the corpus reaches it and the
/// gate is a rate over six episodes. This grid is how the document gets the other half of the answer —
/// *what population a given τ actually flags*, and what `G3` and the miss rate do there — so that
/// setting or confirming the shipped default is a reading rather than a preference. It is fixed, and it
/// is read by the harness from the same measurements as every other number in the report: a sweep the
/// harness could tune to make a document read better would not be evidence.
const TAU_SWEEP: [u64; 10] = [70, 65, 60, 55, 50, 45, 40, 35, 30, 25];

// ---------------------------------------------------------------------------------------------
// the corpus: pins (mechanical), truth (human), thresholds (the document, as numbers)
// ---------------------------------------------------------------------------------------------

/// One repository: where it came from, which commit it is pinned to, and how much of it is measured.
#[derive(Debug, Clone, Deserialize)]
struct Pin {
    /// The directory name, which is also the name in the report.
    repository: String,
    /// The clone URL — read by `tools/fetch_g3_corpus.py`, never by this harness.
    url: String,
    /// The commit the window ends at, full oid.
    head: String,
    /// How many episodes (newest first) the window takes.
    window: usize,
    /// The instant the corpus was authored, ISO-8601 Z. This is the harness's clock: maturity is measured
    /// against it, so the report is the same bytes the day it is re-run as the day it was blessed.
    pinned_now: String,
    /// Why this repository is in the corpus, in one sentence a reader can disagree with.
    why: String,
    /// Who pinned it, when, and with what command.
    pinned_by: String,
}

/// One hand-authored label: what a person decided about one episode, and on what evidence.
#[derive(Debug, Clone, Deserialize)]
struct Label {
    /// The commit the label is keyed to — a member of the episode it is about, which is what makes it
    /// survive a re-index and die loudly on a rebase instead of silently pointing somewhere else.
    commit: String,
    /// `accepted` / `flagged` / `dismissed` / `snoozed`, in `e4.6`'s spelling.
    decision: String,
    /// The dismiss-vocabulary code, required for a dismissal.
    #[serde(default)]
    reason: String,
    /// The reviewer's own words.
    note: String,
    /// Who decided.
    actor: String,
    /// What was looked at to decide — a path, a line, a finding message.
    evidence: String,
    /// Which part of the flag was the false alarm, when the reviewer can name it: `score`, or a detector
    /// that spoke on the episode. Empty means "every finding that spoke", which is §5.4's own attribution
    /// — a flag the reviewer did not want charges whatever raised it. `score` charges no detector: the
    /// ranking raised the flag and no finding was wrong.
    #[serde(default)]
    blame: Vec<String>,
}

/// The truth file beside a pin.
#[derive(Debug, Deserialize)]
struct Truth {
    schema: String,
    authoring: Authoring,
    labels: Vec<Label>,
}

/// Who wrote the truth, and by what protocol. Enforced in [`load_repo`]: an unattributable truth file is
/// a fixture, not a measurement.
#[derive(Debug, Deserialize)]
struct Authoring {
    annotated_by: String,
    protocol: String,
    annotated_on: String,
    /// What a label means, in the reviewer's own words.
    meaning: String,
}

/// One gate, in spike `s2`'s shape: a claim, what it is expected to do, and the measurement rule.
#[derive(Debug, Deserialize)]
struct Gate {
    id: String,
    claim: String,
    /// `pass`, `fail` or `not-measurable`. A gate that surprises us in either direction fails the run.
    expected: String,
    /// Dotted path to a number in the aggregate object.
    metric: String,
    /// Dotted path to that number's denominator, if it has one.
    #[serde(default)]
    denominator: Option<String>,
    #[serde(default)]
    min: Option<f64>,
    #[serde(default)]
    max: Option<f64>,
    /// The claim's own floor on the denominator, above [`MIN_DENOMINATOR`] when higher.
    #[serde(default)]
    min_denominator: Option<usize>,
    /// `count` marks an integer fact with no denominator; anything else is a ratio.
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    why: String,
}

#[derive(Debug, Deserialize)]
struct Thresholds {
    what: String,
    how_gates_are_set: String,
    /// §5.4's alpha threshold, checked against the shipped default τ.
    tau_alpha: u8,
    /// §5.4's 1.0 threshold.
    tau_1_0: u8,
    /// Why the two gates share one threshold. Printed, because it is the field a reader expects to be a
    /// second threshold and is not.
    tau_note: String,
    g3_alpha: f64,
    g3_1_0: f64,
    miss_rate_max: f64,
    /// §5.4's FP set, spelled in the shipped vocabulary. See `labelling_note`.
    fp_reasons: Vec<String>,
    labelling_note: String,
    gates: Vec<Gate>,
}

// ---------------------------------------------------------------------------------------------
// one measured episode
// ---------------------------------------------------------------------------------------------

/// One finding, reduced to what the metrics and R3 need: who raised it, how loudly, and what it cited.
#[derive(Debug)]
struct FindingRef {
    detector: String,
    severity: &'static str,
    /// The claim, as the detector wrote it. It is what a reviewer reads before deciding whether the
    /// detector was right, so it belongs in the report's receipts.
    message: String,
    /// Every evidence locator and detail, concatenated — the text R3 checks for an episode identity.
    cites: String,
}

/// §5.3's three rungs, computed for one episode.
#[derive(Debug, Default)]
struct Rework {
    r1: bool,
    r2: bool,
    r3: bool,
    /// How many successors were available. An episode with no successors has not "not been reworked"; it
    /// is not yet knowable, which is what `mature` is for.
    checked: usize,
    /// The share of what this episode added that its successors took back.
    taken_back: f64,
    /// Which rungs fired, for the report.
    via: Vec<&'static str>,
}

impl Rework {
    fn any(&self) -> bool {
        self.r1 || self.r2 || self.r3
    }
}

/// One episode, as measured: what the shipped code said about it, and what the person said about that.
struct Measured {
    id: String,
    members: Vec<String>,
    summary: String,
    lineage_ref: String,
    lineage_seq: i64,
    created_utc: i64,
    /// `agent` / `human` / `unknown`, as `e4.1` attributed it. Printed per repository because the
    /// population a guardrail is measured on has to be the population it will be applied to, and if it
    /// is not, the number says so out loud.
    author_kind: String,
    score: u8,
    scored: bool,
    /// The three features that moved the score most, as `points — reason`. The receipts print them so a
    /// label can be argued against the same sentence `fathom why` prints.
    top_reasons: Vec<String>,
    findings: Vec<FindingRef>,
    /// The file rows, hunks and all: R1 and R2 read the lines, so they live here rather than in a
    /// second structure that can disagree with the episode it describes.
    files: Vec<FileChange>,
    rework: Rework,
    mature: bool,
    label: Option<Label>,
}

impl Measured {
    /// §5.4's `F(τ,W)`: at or over the threshold, or carrying a high-severity finding.
    fn flagged(&self, tau: u64) -> bool {
        (self.scored && u64::from(self.score) >= tau)
            || self.findings.iter().any(|f| f.severity == "high")
    }
    /// The detectors that spoke on this episode, one entry each.
    fn detectors(&self) -> Vec<String> {
        let mut seen: Vec<String> = Vec::new();
        for f in &self.findings {
            if !seen.contains(&f.detector) {
                seen.push(f.detector.clone());
            }
        }
        seen
    }
}

/// One flagged episode, as the report's receipt: the number, the sentences behind it, and the verdict.
fn receipt(e: &Measured) -> Value {
    json!({
        "episode": e.id,
        "commit": e.members.first().cloned().unwrap_or_default(),
        "members": e.members.len(),
        "summary": e.summary,
        "score": e.score,
        "scored": e.scored,
        "why": e.top_reasons,
        "findings": e
            .findings
            .iter()
            .map(|f| json!({"detector": f.detector, "severity": f.severity, "message": f.message}))
            .collect::<Vec<_>>(),
        "mature": e.mature,
        "rework": e.rework.via,
        "taken_back": (e.rework.taken_back * 1_000.0).round() / 1_000.0,
        "label": e.label.as_ref().map(|label| json!({
            "decision": label.decision,
            "reason": label.reason,
            "blame": label.blame,
            "actor": label.actor,
            "note": label.note,
            "evidence": label.evidence,
        })),
    })
}

/// A repository after measurement, before scoring.
struct Repo {
    pin: Pin,
    episodes: Vec<Measured>,
    /// The reason codes the shipped vocabulary offers.
    vocabulary: BTreeSet<String>,
    /// Labels that did not resolve to an episode in the window.
    orphaned: Vec<Label>,
    /// Dismissals whose code is not in the shipped vocabulary.
    refused: Vec<String>,
    /// Every score the repository produced, for the decile cut.
    scores: Vec<u8>,
}

/// One cut's numbers.
#[derive(Debug, Default)]
struct Cut {
    tau: u64,
    flagged: usize,
    labelled: usize,
    fp: usize,
    tp: usize,
    unlabelled: Vec<String>,
    /// Flags dismissed as false alarms that the reviewer blamed on the ranking rather than on a finding.
    blamed_score: Vec<String>,
    g3: Option<f64>,
}

/// One detector's own numbers at one cut.
#[derive(Debug, Default)]
struct DetectorScore {
    flagged: usize,
    labelled: usize,
    offenders: Vec<String>,
    g3: Option<f64>,
}

// ---------------------------------------------------------------------------------------------
// loading
// ---------------------------------------------------------------------------------------------

fn read<T: serde::de::DeserializeOwned>(path: &Path, what: &str) -> T {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{} ({}) cannot be read: {e}", what, path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| {
        panic!(
            "{} ({}) is not what this harness expects: {e}",
            what,
            path.display()
        )
    })
}

/// Parse the pins' ISO-8601 instant. The pins carry a fixed clock so the harness never reads a clock;
/// this is the civil-days algorithm, not a date library for one field.
fn iso_to_millis(text: &str) -> i64 {
    let (date, time) = text
        .split_once('T')
        .unwrap_or_else(|| panic!("pin timestamp {text:?} is not `YYYY-MM-DDTHH:MM:SSZ`"));
    let nums: Vec<i64> = date
        .split('-')
        .chain(time.trim_end_matches('Z').split(':'))
        .map(|part| {
            part.parse::<i64>()
                .unwrap_or_else(|_| panic!("pin timestamp {text:?} has a non-numeric field"))
        })
        .collect();
    let [y, m, d, h, mi, s] = nums.as_slice() else {
        panic!("pin timestamp {text:?} is not `YYYY-MM-DDTHH:MM:SSZ`");
    };
    let a = (14 - m) / 12;
    let yy = y + 4800 - a;
    let mm = m + (12 * a) - 3;
    let days = d + (153 * mm + 2) / 5 + 365 * yy + yy / 4 - yy / 100 + yy / 400 - 32_045;
    ((days - 24_405_876) * 86_400 + h * 3_600 + mi * 60 + s) * 1_000
}

/// Load one repository's pin and truth, enforcing the rules that make the labels trustworthy.
fn load_repo(dir: &Path) -> (Pin, Truth) {
    let pin: Pin = read(&dir.join("pin.json"), "a corpus pin");
    let truth: Truth = read(&dir.join("truth.json"), "a corpus truth file");
    assert_eq!(
        truth.schema,
        "fathom.g3-labels/1",
        "{}: schema {} is not this harness's schema",
        dir.display(),
        truth.schema
    );
    // The `s2` rule, enforced rather than trusted: a truth file that does not name a person, a protocol
    // and what a label means is a file nobody can audit.
    for (field, value) in [
        ("annotated_by", &truth.authoring.annotated_by),
        ("protocol", &truth.authoring.protocol),
        ("annotated_on", &truth.authoring.annotated_on),
        ("meaning", &truth.authoring.meaning),
    ] {
        assert!(
            !value.trim().is_empty(),
            "{}: `authoring.{field}` is empty. Hand-authored truth says who wrote it, how, when and \
             what each row means, or it is a fixture.",
            dir.display()
        );
    }
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for label in &truth.labels {
        assert!(
            seen.insert(label.commit.as_str()),
            "{}: commit {} is labelled twice; two labels on one episode are two opinions in disguise",
            dir.display(),
            label.commit
        );
        assert!(
            !label.note.trim().is_empty() && !label.evidence.trim().is_empty(),
            "{}: the label on {} has no note or no evidence. A verdict that does not say what was read \
             cannot measure whether the flag was worth raising.",
            dir.display(),
            label.commit
        );
        assert!(
            matches!(
                label.decision.as_str(),
                "accepted" | "flagged" | "dismissed" | "snoozed"
            ),
            "{}: label on {} decides {:?}, which is not one of the four settling acts `e4.6` recognises",
            dir.display(),
            label.commit,
            label.decision
        );
        for token in &label.blame {
            assert!(
                token == "score" || detectors::by_id(token).is_some(),
                "{}: the label on {} blames {token:?}, which is neither `score` nor a detector in the \
                 catalogue. Say who cried wolf by their catalogue id, or say `score`.",
                dir.display(),
                label.commit
            );
        }
        if label.decision == "dismissed" {
            assert!(
                !label.reason.trim().is_empty(),
                "{}: the dismissal on {} carries no reason code, and a dismissal without a code cannot \
                 be counted — which is the whole point of the closed vocabulary.",
                dir.display(),
                label.commit
            );
        }
    }
    (pin, truth)
}

// ---------------------------------------------------------------------------------------------
// measurement
// ---------------------------------------------------------------------------------------------

/// Index one pinned repository and measure every episode in its window with the shipped code.
fn measure_repo(clone: &Path, pin: &Pin, truth: &Truth, work: &Path, policy: &Policy) -> Repo {
    std::fs::create_dir_all(work).expect("the harness's store directory can be created");
    let store_path = work.join(format!("{}.sqlite3", pin.repository));
    if store_path.exists() {
        std::fs::remove_file(&store_path).expect("the harness's own store can be replaced");
    }
    let repository = Repository::discover(clone)
        .unwrap_or_else(|e| panic!("{}: the pinned clone cannot be opened: {e}", pin.repository));
    let mut store = Store::open(&store_path).expect("a store can be opened for the corpus");
    store.advance_epoch().expect("a fresh epoch can be started");
    let options = index::Options {
        ingest: fathom_ingest::ingest::IngestOptions {
            walk: fathom_ingest::graph::WalkOptions {
                // Newest-first, and only as many as the window needs. The default order follows the
                // first-parent chain to the root, so a bare `limit` would hand the harness the
                // repository's *oldest* commits — a real trap, and the reason the order is named here
                // rather than left at its default.
                order: fathom_ingest::graph::WalkOrder::DateNewestFirst,
                limit: Some(pin.window * WALK_COMMITS_PER_EPISODE),
                ..fathom_ingest::graph::WalkOptions::head()
            },
            ..fathom_ingest::ingest::IngestOptions::default()
        },
        // The clock is the pin's, never this process's — `Options::now` is an argument for exactly this
        // reason — and the repository's own `fathom.toml` is not read: see the module docs.
        now: Some(iso_to_millis(&pin.pinned_now)),
        scope: Scope::Everything,
        worktree: Worktree::Ignore,
        segment: Rules::default(),
        hooks: None,
        refresh: true,
        ..index::Options::head()
    };
    index::run(&repository, &mut store, &Unclassified, &options)
        .unwrap_or_else(|e| panic!("{}: indexing the pinned window failed: {e}", pin.repository));
    store.settle_epoch().expect("the epoch can be settled");

    let query = EpisodeQuery {
        limit: i64::try_from(pin.window).expect("a window fits in an i64"),
        ..EpisodeQuery::default()
    };
    let rows: Vec<EpisodeRow> = store
        .episodes(&query)
        .expect("the window's episodes can be read");
    let content = detectors::collect::Reader::new(&store, &repository, Limits::default())
        .collect(&query)
        .expect("the window's content can be read");
    if !content.notices.is_empty() {
        // A stated partial, not a silent one (ADR-001 §7.7): the content detectors that could not read a
        // path say `not examined`, and the number is printed as the smaller claim it then is.
        eprintln!(
            "{}: {} episode(s) had content this harness could not fully read; the content detectors \
             report `not examined` there rather than clean. First: {}",
            pin.repository,
            content.notices.len(),
            content.notices[0]
        );
    }

    // The corpus the history-shaped detectors are allowed to see, built the way `fathom why` builds it:
    // rows only, oldest first, no content. Anything richer here would measure a corpus the shipped verb
    // never hands the detectors.
    let bare: Vec<Episode> = content
        .episodes
        .iter()
        .rev()
        .map(|episode| {
            let mut episode = episode.clone();
            for file in &mut episode.files {
                file.hunks.clear();
                file.note.clear();
            }
            episode.content = Content::Absent;
            episode.content_note.clear();
            episode
        })
        .collect();
    let corpus = Corpus::of(&bare);
    let weights = Weights::defaults();

    let mut episodes: Vec<Measured> = Vec::new();
    for (row, episode) in rows.iter().zip(content.episodes.iter()) {
        assert_eq!(
            row.id, episode.id,
            "the row read and the content read disagree about which episode is which"
        );
        let scan = detectors::scan(episode, &corpus, policy);
        let vector = risk_collect::vector(row, episode, &scan, &Supply::nothing());
        let scored = score::score(&vector, &weights);
        episodes.push(Measured {
            id: row.id.clone(),
            members: row.members.clone(),
            summary: row.summary.clone(),
            lineage_ref: row.lineage_ref.clone(),
            lineage_seq: row.lineage_seq,
            created_utc: row.created_utc,
            author_kind: row.author_kind.clone(),
            score: scored.score,
            scored: scored.scored,
            top_reasons: scored
                .terms
                .iter()
                .take(3)
                .map(|term| format!("{} — {}", term.points.0, term.reason))
                .collect(),
            findings: scan
                .findings
                .iter()
                .map(|f| FindingRef {
                    detector: f.detector_id.clone(),
                    severity: f.severity.as_str(),
                    message: f.message.clone(),
                    cites: f
                        .evidence
                        .iter()
                        .map(|e| format!("{} {}", e.locator, e.detail))
                        .collect::<Vec<_>>()
                        .join(" "),
                })
                .collect(),
            files: episode.files.clone(),
            rework: Rework::default(),
            mature: false,
            label: None,
        });
    }

    let mut repo = Repo {
        pin: pin.clone(),
        vocabulary: fathom_analyze::queue::REASONS
            .iter()
            .map(|(code, ..)| (*code).to_owned())
            .collect(),
        episodes,
        orphaned: Vec::new(),
        refused: Vec::new(),
        scores: Vec::new(),
    };
    rework_pass(&mut repo);
    let now = iso_to_millis(&pin.pinned_now);
    for e in &mut repo.episodes {
        e.mature = e.rework.checked >= REWORK_N || now - e.created_utc >= MATURE_DAYS * 86_400_000;
    }
    attach_labels(&mut repo, truth);
    repo.scores = repo
        .episodes
        .iter()
        .filter(|e| e.scored)
        .map(|e| e.score)
        .collect();
    repo
}

/// The successors of one episode: the next [`REWORK_N`] later episodes on its lineage, oldest first.
fn successors(repo: &Repo, index: usize) -> Vec<usize> {
    let lineage = repo.episodes[index].lineage_ref.as_str();
    let seq = repo.episodes[index].lineage_seq;
    if lineage.is_empty() {
        return Vec::new();
    }
    let mut later: Vec<(i64, usize)> = repo
        .episodes
        .iter()
        .enumerate()
        .filter(|(_, e)| e.lineage_ref == lineage && e.lineage_seq > seq)
        .map(|(j, e)| (e.lineage_seq, j))
        .collect();
    later.sort();
    later.into_iter().take(REWORK_N).map(|(_, j)| j).collect()
}

/// The lines an episode added, per path, as multisets: a line added twice has to be taken back twice.
fn added_lines(files: &[FileChange]) -> BTreeMap<&str, BTreeMap<&str, usize>> {
    let mut out: BTreeMap<&str, BTreeMap<&str, usize>> = BTreeMap::new();
    for file in files {
        for hunk in &file.hunks {
            for line in &hunk.added {
                *out.entry(file.path.as_str())
                    .or_default()
                    .entry(line.as_str())
                    .or_insert(0) += 1;
            }
        }
    }
    out
}

/// §5.3's R1 and R2, measured as the share of what an episode added that its successors took back.
fn taken_back(episodes: &[Measured], index: usize, successors: &[usize]) -> (bool, f64) {
    let added = added_lines(&episodes[index].files);
    let total: usize = added.values().map(|m| m.values().sum::<usize>()).sum();
    if total == 0 {
        // No content, no claim: an episode whose lines were never read has not "not been reverted".
        return (false, 0.0);
    }
    let mut removed: BTreeMap<&str, BTreeMap<&str, usize>> = BTreeMap::new();
    for &j in successors {
        for file in &episodes[j].files {
            for hunk in &file.hunks {
                for line in &hunk.removed {
                    *removed
                        .entry(file.path.as_str())
                        .or_default()
                        .entry(line.as_str())
                        .or_insert(0) += 1;
                }
            }
        }
    }
    let mut taken = 0usize;
    for (path, lines) in &added {
        let Some(gone) = removed.get(path) else {
            continue;
        };
        for (line, count) in lines {
            taken += (*count).min(gone.get(line).copied().unwrap_or(0));
        }
    }
    let share = f64::from(taken as u32) / f64::from(total as u32);
    (share >= R2_SHARE, share)
}

/// §5.3's three rungs, for every episode in the window.
fn rework_pass(repo: &mut Repo) {
    for i in 0..repo.episodes.len() {
        let succ = successors(repo, i);
        let summary = repo.episodes[i].summary.to_lowercase();
        let members: Vec<String> = repo.episodes[i]
            .members
            .iter()
            .map(|m| m[..12.min(m.len())].to_lowercase())
            .collect();
        // R1 by message. git writes `Revert "<subject>"` and names the oid in the body; the index keeps
        // the first line in `summary`, so the message rung matches the subject and the content rung below
        // is what catches a revert that only named its oid in the body.
        let declared = succ.iter().any(|&j| {
            let subject = repo.episodes[j].summary.to_lowercase();
            subject.starts_with("revert")
                && (members.iter().any(|m| subject.contains(m.as_str()))
                    || subject.contains(&summary))
        });
        let (r2, share) = taken_back(&repo.episodes, i, &succ);
        let episode_id = repo.episodes[i].id.clone();
        let cited: Vec<&'static str> = succ
            .iter()
            .flat_map(|&j| repo.episodes[j].findings.iter())
            .filter(|f| {
                R3_DETECTORS.contains(&f.detector.as_str())
                    && (f.cites.contains(&episode_id)
                        || members.iter().any(|m| f.cites.contains(m.as_str())))
            })
            .map(|f| {
                R3_DETECTORS
                    .iter()
                    .find(|d| **d == f.detector.as_str())
                    .copied()
                    .unwrap_or("rework_loop")
            })
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let e = &mut repo.episodes[i];
        e.rework.checked = succ.len();
        e.rework.taken_back = share;
        e.rework.r1 = declared || share >= R1_SHARE;
        e.rework.r2 = r2;
        e.rework.r3 = !cited.is_empty();
        e.rework.via = ["R1", "R2", "R3"]
            .into_iter()
            .zip([e.rework.r1, e.rework.r2, e.rework.r3])
            .filter(|(_, fired)| *fired)
            .map(|(rung, _)| rung)
            .collect();
    }
}

/// Join the hand-authored labels to the measured episodes, loudly on both failures.
fn attach_labels(repo: &mut Repo, truth: &Truth) {
    for label in &truth.labels {
        if label.decision == "dismissed" && !repo.vocabulary.contains(&label.reason) {
            repo.refused.push(format!(
                "{}: {} is not in the shipped dismiss vocabulary",
                label.commit, label.reason
            ));
            continue;
        }
        let hit = repo
            .episodes
            .iter()
            .position(|e| e.members.iter().any(|m| m.starts_with(&label.commit)));
        match hit {
            Some(i) => repo.episodes[i].label = Some(label.clone()),
            None => repo.orphaned.push(label.clone()),
        }
    }
}

/// Whether a label is one of §5.4's false positives.
fn is_fp(label: &Label, fp_reasons: &[String]) -> bool {
    label.decision == "dismissed" && fp_reasons.iter().any(|r| r == &label.reason)
}

/// Whether a label is one of §5.4's true positives. `Thresholds::labelling_note` explains why `accepted`
/// stands in for §5.4's `accepted-with-required-change`.
fn is_tp(label: &Label, reworked: bool) -> bool {
    label.decision == "accepted" || reworked
}

fn rate(numerator: usize, denominator: usize) -> Option<f64> {
    if denominator == 0 {
        return None;
    }
    let ratio = f64::from(numerator as u32) / f64::from(denominator as u32);
    Some((ratio * 1_000.0).round() / 1_000.0)
}

/// The numbers for one flag set, in aggregate and per detector.
///
/// `tau` is only a label here: the flag set itself is the predicate, because the corpus is scored at three
/// thresholds — §5.4's published default, each repository's top-decile cut, and the union of the two,
/// which is the population the per-detector attribution is actually about.
fn score_cut(
    repo: &Repo,
    tau: u64,
    in_cut: impl Fn(&Measured) -> bool,
    fp_reasons: &[String],
) -> (Cut, BTreeMap<String, DetectorScore>) {
    let mut cut = Cut {
        tau,
        ..Default::default()
    };
    let mut detectors: BTreeMap<String, DetectorScore> = BTreeMap::new();
    for e in &repo.episodes {
        if !in_cut(e) {
            continue;
        }
        cut.flagged += 1;
        let speakers = e.detectors();
        for id in &speakers {
            detectors.entry(id.clone()).or_default().flagged += 1;
        }
        let Some(label) = &e.label else {
            cut.unlabelled.push(e.id.clone());
            continue;
        };
        let fp = is_fp(label, fp_reasons);
        cut.labelled += 1;
        cut.fp += usize::from(fp);
        cut.tp += usize::from(is_tp(label, e.rework.any()));
        if fp && label.blame.iter().any(|token| token == "score") {
            cut.blamed_score.push(e.id.clone());
        }
        // A detector is in the denominator when the reviewer's objection was about a *finding* — either
        // unnamed (§5.4's own rule: whatever raised an unwanted flag shares the charge) or named and one
        // of its own. `score` alone means the ranking raised a flag no finding justified, and no detector
        // is answerable for it.
        let named: Vec<&String> = label.blame.iter().filter(|t| *t != "score").collect();
        let in_denominator = |id: &str| {
            if !speakers.iter().any(|s| s == id) {
                false
            } else if named.is_empty() {
                true
            } else {
                named.iter().any(|t| t.as_str() == id)
            }
        };
        for id in &speakers {
            if !in_denominator(id) {
                continue;
            }
            let d = detectors.entry(id.clone()).or_default();
            d.labelled += 1;
            if fp {
                d.offenders.push(e.id.clone());
            }
        }
    }
    for d in detectors.values_mut() {
        d.g3 = rate(d.offenders.len(), d.labelled);
    }
    cut.g3 = rate(cut.fp, cut.labelled);
    (cut, detectors)
}

/// The decile cut for one repository: the score at the top decile boundary of what it scored.
fn decile_tau(scores: &[u8]) -> u64 {
    if scores.is_empty() {
        return 101;
    }
    let mut sorted = scores.to_vec();
    sorted.sort_unstable();
    let idx = ((sorted.len() as f64) * (1.0 - DECILE)) as usize;
    u64::from(sorted[idx.min(sorted.len() - 1)])
}

/// The cut as JSON.
fn cut_json(cut: &Cut) -> Value {
    json!({
        "tau": cut.tau,
        "flagged": cut.flagged,
        "labelled": cut.labelled,
        "false_positives": cut.fp,
        "true_positives": cut.tp,
        "g3": cut.g3,
        "unlabelled_flagged": cut.unlabelled,
        "blamed_on_score": cut.blamed_score,
    })
}

fn percentile(scores: &[u8], q: f64) -> u8 {
    if scores.is_empty() {
        return 0;
    }
    let mut sorted = scores.to_vec();
    sorted.sort_unstable();
    let idx = ((sorted.len() as f64) * q) as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// What one τ does to the whole corpus: the flag population it raises, the `G3` the labels can answer
/// for at it, the miss rate it produces, and how many flags nobody has labelled. See [`TAU_SWEEP`].
fn tau_sweep(repos: &[Repo], fp_reasons: &[String]) -> Vec<Value> {
    TAU_SWEEP
        .iter()
        .map(|&tau| {
            let mut flagged = 0_usize;
            let mut labelled = 0_usize;
            let mut fp = 0_usize;
            let mut tp = 0_usize;
            let mut unlabelled = 0_usize;
            let mut mature_rework = 0_usize;
            let mut missed = 0_usize;
            let mut episodes = 0_usize;
            for repo in repos {
                for e in &repo.episodes {
                    episodes += 1;
                    if e.flagged(tau) {
                        flagged += 1;
                        match &e.label {
                            None => unlabelled += 1,
                            Some(label) => {
                                labelled += 1;
                                fp += usize::from(is_fp(label, fp_reasons));
                                tp += usize::from(is_tp(label, e.rework.any()));
                            }
                        }
                    }
                    if e.mature && e.rework.any() {
                        mature_rework += 1;
                        if !e.flagged(tau) {
                            missed += 1;
                        }
                    }
                }
            }
            json!({
                "tau": tau,
                "flagged": flagged,
                "flag_share": rate(flagged, episodes),
                "labelled": labelled,
                "unlabelled": unlabelled,
                "false_positives": fp,
                "true_positives": tp,
                "g3": rate(fp, labelled),
                "mature_rework": mature_rework,
                "missed_rework": missed,
                "miss_rate": rate(missed, mature_rework),
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// the gates
// ---------------------------------------------------------------------------------------------

fn lookup(needle: &Value, path: &str) -> Option<f64> {
    let mut cur = needle;
    for part in path.split('.') {
        cur = cur.get(part)?;
    }
    cur.as_f64()
}

/// What a gate says about the measurement. Three states, because `pass` on an empty denominator is the
/// way a guardrail lies.
fn evaluate(gate: &Gate, aggregate: &Value) -> (String, String) {
    if gate.kind.as_deref() != Some("count") {
        let denominator = gate
            .denominator
            .as_deref()
            .and_then(|d| lookup(aggregate, d))
            .map(|d| d as usize);
        if denominator.is_none() {
            return (
                "not-measurable".to_owned(),
                format!(
                    "no denominator at {}",
                    gate.denominator.as_deref().unwrap_or("?")
                ),
            );
        }
        let denominator = denominator.unwrap_or(0);
        let floor = gate.min_denominator.unwrap_or(MIN_DENOMINATOR);
        if denominator < floor {
            return (
                "not-measurable".to_owned(),
                format!("denominator {denominator} is under the floor {floor}"),
            );
        }
    }
    let Some(measured) = lookup(aggregate, &gate.metric) else {
        return (
            "not-measurable".to_owned(),
            format!("no measurement at {}", gate.metric),
        );
    };
    let mut ok = true;
    if let Some(max) = gate.max {
        ok &= measured <= max;
    }
    if let Some(min) = gate.min {
        ok &= measured >= min;
    }
    (
        if ok { "pass" } else { "fail" }.to_owned(),
        format!("measured {measured}"),
    )
}

// ---------------------------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------------------------

fn arg(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let check = args.iter().any(|a| a == "--check");
    let workdir = PathBuf::from(
        arg(&args, "--workdir")
            .as_deref()
            .unwrap_or(DEFAULT_WORKDIR),
    );
    let report_path = PathBuf::from(arg(&args, "--report").as_deref().unwrap_or(DEFAULT_REPORT));
    let root = PathBuf::from(arg(&args, "--root").as_deref().unwrap_or("."));
    let calibration = root.join(CALIBRATION);
    let thresholds: Thresholds = read(&calibration.join("thresholds.json"), "the gate file");

    let shipped = Weights::defaults();
    assert_eq!(
        u64::from(thresholds.tau_alpha),
        u64::from(shipped.tau),
        "thresholds.json sets tau_alpha = {} but the shipped default is {}; §5.4's number and the \
         product's are one number, so change the document, not this file",
        thresholds.tau_alpha,
        shipped.tau
    );
    for code in &thresholds.fp_reasons {
        assert!(
            is_code(code),
            "thresholds.json lists {code} as a false-positive reason and the shipped dismiss vocabulary \
             has no such code — §5.4's labelling mechanism is `e4.6`'s closed list, so a code that left \
             the list has to be re-mapped in the document, not invented here"
        );
    }

    let policy = Policy::all_enabled();
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(calibration.join("corpus"))
        .expect("the corpus directory exists")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    assert_eq!(
        dirs.len(),
        3,
        "the corpus is three real repositories; found {}",
        dirs.len()
    );
    let mut repos: Vec<Repo> = Vec::new();
    for dir in &dirs {
        let (pin, truth) = load_repo(dir);
        let clone = workdir.join(&pin.repository);
        assert!(
            clone.exists(),
            "{}: the pinned clone is not at {}. Run `python3 tools/fetch_g3_corpus.py --dest {}` — this \
             harness measures real repositories and refuses to invent one.",
            pin.repository,
            clone.display(),
            workdir.display()
        );
        repos.push(measure_repo(&clone, &pin, &truth, &workdir, &policy));
    }

    // Per repository, at the published τ and at the decile cut; the aggregate sums the same rows.
    let mut repo_json: Vec<Value> = Vec::new();
    let aggregate_cuts: [&str; 3] = ["tau_alpha", "tau_decile", "corpus"];
    let mut pooled: BTreeMap<String, (Cut, BTreeMap<String, DetectorScore>)> = BTreeMap::new();
    for key in aggregate_cuts {
        pooled.insert(key.to_owned(), (Cut::default(), BTreeMap::new()));
    }
    let mut decile_taus: BTreeMap<String, u64> = BTreeMap::new();

    for repo in &repos {
        let alpha = u64::from(thresholds.tau_alpha);
        let decile = decile_tau(&repo.scores);
        let (cut_a, det_a) = score_cut(repo, alpha, |e| e.flagged(alpha), &thresholds.fp_reasons);
        let (cut_d, det_d) = score_cut(repo, decile, |e| e.flagged(decile), &thresholds.fp_reasons);
        let (cut_u, det_u) = score_cut(
            repo,
            0,
            |e| e.flagged(alpha) || e.flagged(decile),
            &thresholds.fp_reasons,
        );
        decile_taus.insert(repo.pin.repository.clone(), decile);
        for (key, (cut, det)) in [
            ("tau_alpha", (&cut_a, &det_a)),
            ("tau_decile", (&cut_d, &det_d)),
            ("corpus", (&cut_u, &det_u)),
        ] {
            let entry = pooled.get_mut(key).expect("the cut exists");
            entry.0.flagged += cut.flagged;
            entry.0.labelled += cut.labelled;
            entry.0.fp += cut.fp;
            entry.0.tp += cut.tp;
            entry.0.unlabelled.extend(cut.unlabelled.iter().cloned());
            entry
                .0
                .blamed_score
                .extend(cut.blamed_score.iter().cloned());
            for (id, d) in det {
                let p = entry.1.entry(id.clone()).or_default();
                p.flagged += d.flagged;
                p.labelled += d.labelled;
                p.offenders.extend(d.offenders.iter().cloned());
            }
        }

        let mature = repo.episodes.iter().filter(|e| e.mature).count();
        let mature_rework = repo
            .episodes
            .iter()
            .filter(|e| e.mature && e.rework.any())
            .count();
        let missed = repo
            .episodes
            .iter()
            .filter(|e| e.mature && e.rework.any() && !e.flagged(alpha))
            .count();
        // Per detector, at the corpus cut — the same population `aggregate.detectors_corpus` is computed
        // over, so a repository row and the aggregate cannot tell different stories.
        let per_detector: Vec<Value> = detectors::ids()
            .into_iter()
            .map(|id| {
                let empty = DetectorScore::default();
                let d = det_u.get(id).unwrap_or(&empty);
                json!({
                    "id": id,
                    "default_enabled": detectors::default_enabled().contains(id),
                    "spoke": d.flagged,
                    "answerable_for": d.labelled,
                    "false_positives": d.offenders.len(),
                    "g3": d.g3,
                })
            })
            .collect();
        repo_json.push(json!({
            "repository": repo.pin.repository,
            "url": repo.pin.url,
            "head": repo.pin.head,
            "window": repo.pin.window,
            "pinned_now": repo.pin.pinned_now,
            "pinned_by": repo.pin.pinned_by,
            "why": repo.pin.why,
            "episodes": repo.episodes.len(),
            "episodes_by_author_kind": {
                "agent": repo.episodes.iter().filter(|e| e.author_kind == "agent").count(),
                "human": repo.episodes.iter().filter(|e| e.author_kind == "human").count(),
                "unknown": repo
                    .episodes
                    .iter()
                    .filter(|e| !matches!(e.author_kind.as_str(), "agent" | "human"))
                    .count(),
            },
            "scored": repo.scores.len(),
            "score_median": percentile(&repo.scores, 0.5),
            "score_max": repo.scores.iter().copied().max().unwrap_or(0),
            "decile_tau": decile,
            "labelled_total": repo.episodes.iter().filter(|e| e.label.is_some()).count(),
            "labels": repo
                .episodes
                .iter()
                .filter(|e| e.label.is_some())
                .map(|e| {
                    let label = e.label.as_ref().expect("just filtered");
                    json!({
                        "commit": label.commit,
                        "episode": e.id,
                        "decision": label.decision,
                        "reason": label.reason,
                        "actor": label.actor,
                        "score": e.score,
                        "rework": e.rework.via,
                    })
                })
                .collect::<Vec<_>>(),
            "orphaned_labels": repo.orphaned.iter().map(|l| l.commit.clone()).collect::<Vec<_>>(),
            // Dispositions whose episode the cut stopped flagging. They are not a hole in the guardrail — a
            // hole is a flag nobody read — they are a reading that stands while the ranking moved under it,
            // which is what happens when a ticket changes what gets flagged. They are printed rather than
            // folded into `labelled`, because a rate may only be computed over what the cut flagged.
            "labels_unflagged": repo
                .episodes
                .iter()
                .filter(|e| e.label.is_some() && !e.flagged(alpha) && !e.flagged(decile))
                .map(|e| {
                    let label = e.label.as_ref().expect("just filtered");
                    json!({
                        "episode": e.id,
                        "commit": label.commit,
                        "decision": label.decision,
                        "reason": label.reason,
                        "score": e.score,
                    })
                })
                .collect::<Vec<_>>(),
            "refused_labels": repo.refused,
            "mature": mature,
            "mature_rework": mature_rework,
            "missed_rework": missed,
            "miss_rate": rate(missed, mature_rework),
            "rework_by_rung": {
                "R1": repo.episodes.iter().filter(|e| e.rework.r1).count(),
                "R2": repo.episodes.iter().filter(|e| e.rework.r2).count(),
                "R3": repo.episodes.iter().filter(|e| e.rework.r3).count(),
            },
            "at_tau_alpha": cut_json(&cut_a),
            "at_tau_decile": cut_json(&cut_d),
            "at_corpus_cut": cut_json(&cut_u),
            "missed_at_decile": repo
                .episodes
                .iter()
                .filter(|e| e.mature && e.rework.any() && !e.flagged(decile))
                .count(),
            "detectors": per_detector,
            "receipts": repo
                .episodes
                .iter()
                .filter(|e| e.flagged(alpha) || e.flagged(decile))
                .map(receipt)
                .collect::<Vec<_>>(),
        }));
    }

    // The aggregate the gates read.
    let mut aggregate = json!({});
    aggregate["tau_sweep"] = json!({
        "note": "§5.4's `F(tau,W)` read at every threshold on the grid, over every episode: the flag \
                 population the threshold raises, the `G3` the hand-authored labels can answer for at \
                 it, the miss rate it produces, and `unlabelled` — flags nobody has read, which is \
                 what refuses to become a rate. This is where a published default is chosen or \
                 confirmed from; `thresholds.json` states the bar and the gate, and nowhere else \
                 does τ move.",
        "grid": TAU_SWEEP,
        "rows": tau_sweep(&repos, &thresholds.fp_reasons),
    });
    aggregate["detectors_note"] = json!(
        "A detector's denominator (`answerable_for`) is the labelled flags it was answerable for: every \
         flag it spoke on with no `blame` (§5.4's rule that whatever raised an unwanted flag shares the \
         charge), plus the flags whose `blame` named it. A flag whose `blame` named other detectors is \
         not charged here, and one blamed on `score` is in no detector's denominator at all — so these \
         denominators do not sum to the corpus total, by design. `spoke` is the wider count (every flag \
         at this cut whose findings included this detector's) and is shown so the narrow one reads as a \
         share of it. A detector with `g3: null` raised nothing here, which is no evidence rather than \
         a clean bill of health (ADR-001 §7.7); `defaults.never_fired` names them."
    );
    for key in aggregate_cuts {
        let (cut, det) = &pooled[key];
        aggregate[format!("cut_{key}")] = json!({
            // The pooled `Cut` sums counts, not thresholds; the published τ is one number across the
            // whole corpus, so it is stated from the threshold rather than summed from the parts.
            "tau": if key == "tau_alpha" { json!(thresholds.tau_alpha) } else { Value::Null },
            "per_repository_tau": if key == "tau_decile" {
                json!(decile_taus)
            } else {
                Value::Null
            },
            "tau_note": match key {
                "tau_alpha" => "ADR-001 §5.4's published default, and the shipped `risk.tau`".to_owned(),
                "tau_decile" => "each repository's own top-decile boundary".to_owned(),
                _ => "the union of both cuts: every episode this corpus flagged".to_owned(),
            },
            "flagged": cut.flagged,
            "labelled": cut.labelled,
            "false_positives": cut.fp,
            "true_positives": cut.tp,
            "g3": rate(cut.fp, cut.labelled),
        });
        let mut map = json!({});
        // Every catalogue id appears, including the ones that never spoke: a table with only the
        // detectors that fired is a table that implies the rest were fine.
        for id in detectors::ids() {
            let empty = DetectorScore::default();
            let d = det.get(id).unwrap_or(&empty);
            map[id] = json!({
                "ships_enabled": detectors::default_enabled().contains(id),
                "spoke": d.flagged,
                "answerable_for": d.labelled,
                "false_positives": d.offenders.len(),
                "g3": rate(d.offenders.len(), d.labelled),
            });
        }
        aggregate[format!("detectors_{key}")] = map;
    }
    let mature: usize = repos
        .iter()
        .map(|r| r.episodes.iter().filter(|e| e.mature).count())
        .sum();
    let mature_rework: usize = repos
        .iter()
        .map(|r| {
            r.episodes
                .iter()
                .filter(|e| e.mature && e.rework.any())
                .count()
        })
        .sum();
    let alpha = u64::from(thresholds.tau_alpha);
    let missed: usize = repos
        .iter()
        .map(|r| {
            r.episodes
                .iter()
                .filter(|e| e.mature && e.rework.any() && !e.flagged(alpha))
                .count()
        })
        .sum();
    let missed_decile: usize = repos
        .iter()
        .zip(repos.iter().map(|r| decile_taus[&r.pin.repository]))
        .map(|(r, tau)| {
            r.episodes
                .iter()
                .filter(|e| e.mature && e.rework.any() && !e.flagged(tau))
                .count()
        })
        .sum();
    aggregate["miss"] = json!({
        "mature": mature,
        "mature_rework": mature_rework,
        "missed": missed,
        "miss_rate": rate(missed, mature_rework),
        "missed_at_decile": missed_decile,
        "miss_rate_at_decile": rate(missed_decile, mature_rework),
    });

    // `e0.12`'s rule as a count: a detector enabled by default whose own false positives push the alpha
    // gate past `g3_alpha` is a detector that must ship disabled. Every disagreement between this
    // measurement and the shipped catalogue is one row here, and the gate wants zero rows.
    let enabled = detectors::default_enabled();
    let mut breaches: Vec<String> = Vec::new();
    let mut eligible: Vec<String> = Vec::new();
    let mut measured: Vec<String> = Vec::new();
    let mut silent: Vec<String> = Vec::new();
    for id in detectors::ids() {
        let Some(d) = pooled["corpus"].1.get(id) else {
            silent.push(format!("{id} raised nothing in the corpus"));
            continue;
        };
        if d.labelled < DEFAULT_FLIP_MIN_LABELLED || d.g3.is_none() {
            measured.push(format!(
                "{id} measured on {} labelled flag(s), under the {DEFAULT_FLIP_MIN_LABELLED} a default flip needs",
                d.labelled
            ));
            continue;
        }
        let g3 = d.g3.unwrap_or(0.0);
        let breach = g3 > thresholds.g3_alpha;
        if breach && enabled.contains(id) {
            breaches.push(format!(
                "{id} ships enabled and measures g3 {g3:.3} over its own findings"
            ));
        }
        if !breach && !enabled.contains(id) {
            eligible.push(format!("{id} ships disabled and measures g3 {g3:.3}"));
        }
    }
    // A detector the corpus never saw is not a detector that passed. §5.4's numbers can only be
    // demanded of a detector that was measured, and saying which were not is the difference between a
    // gate and a shrug (ADR-001 §7.7).
    aggregate["defaults"] = json!({
        "breaches": breaches.len(),
        "breach_detail": breaches,
        "eligible_to_reenable": eligible.len(),
        "eligible_detail": eligible,
        "under_measured": measured,
        "never_fired": silent,
    });
    aggregate["coverage"] = json!({
        "unlabelled_flagged_alpha": pooled["tau_alpha"].0.unlabelled.len(),
        "unlabelled_flagged_decile": pooled["tau_decile"].0.unlabelled.len(),
        "unlabelled_total": pooled
            .values()
            .map(|(cut, _)| cut.unlabelled.len())
            .sum::<usize>(),
        "blamed_on_score": pooled["corpus"].0.blamed_score.len(),
        "labels_total": repos
            .iter()
            .map(|r| r.episodes.iter().filter(|e| e.label.is_some()).count())
            .sum::<usize>(),
        "labels_unflagged_total": repos
            .iter()
            .map(|r| {
                r.episodes
                    .iter()
                    .filter(|e| e.label.is_some() && !e.flagged(u64::from(thresholds.tau_alpha)) && !e.flagged(decile_taus[&r.pin.repository]))
                    .count()
            })
            .sum::<usize>(),
    });

    let gates: Vec<Value> = thresholds
        .gates
        .iter()
        .map(|gate| {
            let (status, detail) = evaluate(gate, &aggregate);
            json!({
                "id": gate.id,
                "claim": gate.claim,
                "expected": gate.expected,
                "status": status,
                "detail": detail,
                "why": gate.why,
            })
        })
        .collect();
    let surprises: Vec<&Value> = gates
        .iter()
        .filter(|gate| gate["status"] != gate["expected"])
        .collect();

    let report = json!({
        "schema": "fathom.g3-calibration/1",
        "what": thresholds.what,
        "how_gates_are_set": thresholds.how_gates_are_set,
        "formulas": {
            "G3": "|{e in F(tau,W): labelled(e) and FP(e)}| / |{e in F(tau,W): labelled(e)}| — ADR-001 §5.4",
            "F(tau,W)": "risk(e) >= tau, or any finding at severity high",
            "FP(e)": "a dismissal whose reason code is in `fp_reasons`",
            "TP(e)": "disposition `accepted`, or rework(e) — see labelling_note. TP is reported beside G3 and is not in its denominator: §5.4's denominator is every labelled flag, true positives and false ones together",
            "miss_rate": "|{e in Mature(W): rework(e) and risk(e) < tau}| / |{e in Mature(W): rework(e)}|",
            "rework": "ADR-001 §5.3 R1|R2|R3 with N = 5; R1's content rung fires at an undo share >= 0.90",
            "mature": ">= 5 successors on the lineage, or older than 7 days against the pin's clock",
            "fp_reasons": thresholds.fp_reasons,
            "labelling_note": thresholds.labelling_note,
            "tau_alpha": thresholds.tau_alpha,
            "tau_1_0": thresholds.tau_1_0,
            "tau_note": thresholds.tau_note,
            "g3_alpha": thresholds.g3_alpha,
            "g3_1_0": thresholds.g3_1_0,
            "miss_rate_max": thresholds.miss_rate_max,
            "min_denominator": MIN_DENOMINATOR,
            "decile": DECILE,
        },
        "policy": {
            "detectors_enabled": policy.enabled.iter().cloned().collect::<Vec<_>>(),
            "note": "every detector is enabled for the measurement. The four that ship disabled have to \
                     be measured too, or nothing can decide whether they deserve to ship enabled.",
            "weights": "features-1 as shipped, with the shipped default tau",
        },
        "repositories": repo_json,
        "aggregate": aggregate,
        "gates": gates,
    });

    let rendered = format!(
        "{}\n",
        serde_json::to_string_pretty(&report)
            .expect("the report this harness built can be serialised")
    );

    if check {
        let committed = std::fs::read_to_string(&report_path).unwrap_or_else(|_| {
            panic!(
                "no report at {}; run without --check first",
                report_path.display()
            )
        });
        if committed == rendered {
            println!("g3: the committed report reproduces from the pinned corpus exactly");
        } else {
            let diff = committed
                .lines()
                .zip(rendered.lines())
                .find(|(a, b)| a != b)
                .map_or_else(
                    || "one file is a prefix of the other".to_owned(),
                    |(a, b)| format!("committed:   {a}\nre-measured: {b}"),
                );
            panic!(
                "g3: the committed report is not what the pinned corpus and the shipped detectors and \
                 weights produce now.\n{diff}"
            );
        }
        if !surprises.is_empty() {
            for gate in &surprises {
                eprintln!(
                    "  {}: expected {}, got {} — {}",
                    gate["id"].as_str().unwrap_or("?"),
                    gate["expected"].as_str().unwrap_or("?"),
                    gate["status"].as_str().unwrap_or("?"),
                    gate["detail"].as_str().unwrap_or("")
                );
            }
            panic!(
                "g3: {} gate(s) came out other than claimed — a gate that quietly starts passing has to \
                 be re-read as well as one that fails",
                surprises.len()
            );
        }
        return;
    }

    if let Some(parent) = report_path.parent() {
        std::fs::create_dir_all(parent).expect("the report directory exists");
    }
    std::fs::write(&report_path, &rendered).expect("the report can be written");
    println!(
        "g3: wrote {} — {} repositories, {} gates, {} surprise(s)",
        report_path.display(),
        repos.len(),
        gates.len(),
        surprises.len()
    );
    for gate in &gates {
        println!(
            "  {:<40} {:<16} {}",
            gate["id"].as_str().unwrap_or("?"),
            gate["status"].as_str().unwrap_or("?"),
            gate["detail"].as_str().unwrap_or("")
        );
    }
}
