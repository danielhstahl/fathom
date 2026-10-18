//! Spike `s2` (`reco-q6r`) — does the boundary cascade find the boundaries a human would draw, on
//! histories an agent produced?
//!
//! **This is a spike, not a library.** ADR-003 §12 lists "the boundary *quality* of the rule set —
//! precision/recall against 200 annotated real turns" as the question this ADR deliberately does not
//! decide, and §13.7 defers every constant in §4.4 to it. This is the measurement: a hand-annotated turn
//! corpus (`crates/fathom-fixtures/corpus/turns/`), a strategy interpreter that runs subsets and
//! reorderings of the cascade, and a report. The written answer is
//! `docs/concepts/episode-segmentation.md`, and the decision lands in ADR-003.
//!
//! ```text
//! cargo run -p fathom-analyze --example s2_spike -- \
//!     --report benchmarks/reports/s2-segmentation-accuracy.json
//! cargo run -p fathom-analyze --example s2_spike -- --check
//! ```
//!
//! The shipped cascade is not reimplemented here: the `shipped` strategy is built out of
//! [`fathom_ingest::segment`]'s own predicates, and the harness asserts on every history that its
//! partitions equal what [`segment::segment`] produces. A spike that quietly re-derived the rules would
//! measure its own copy of the design instead of the design.
//!
//! Scoring is on **boundary positions** — the set of commits that open an episode — because that is what
//! the ticket asks for and what a reviewer sees. The pair co-membership F1 `e1.3` gates on is printed
//! beside it, since the two disagree in an informative direction: on a corpus where 76 % of turns are one
//! commit long, the trivial "every commit is its own episode" partition scores high on both.

#![allow(clippy::print_stdout, clippy::print_stderr, clippy::too_many_arguments)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use fathom_ingest::segment::{self, CommitFacts, Rules};
use serde::Deserialize;
use serde_json::{Value, json};

// ---------------------------------------------------------------------------------------------
// the corpus
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Raw {
    history: String,
    provenance: String,
    #[serde(default)]
    clock: String,
    lineage: Vec<RawCommit>,
    truth: RawTruth,
}

#[derive(Debug, Deserialize, Clone)]
struct RawCommit {
    oid: String,
    #[serde(default)]
    parents: Vec<String>,
    author_name: String,
    #[serde(default)]
    author_email: String,
    author_millis: i64,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    trailers: BTreeMap<String, String>,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    n_files: i64,
    #[serde(default)]
    n_add: i64,
    #[serde(default)]
    n_del: i64,
    #[serde(default)]
    n_binary: i64,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    turn_id: Option<String>,
    #[serde(default)]
    merge: bool,
    #[serde(default)]
    harness: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct RawTruth {
    turns: Vec<RawTurn>,
    #[serde(default)]
    pathologies: Vec<RawPathology>,
}

#[derive(Debug, Deserialize)]
struct RawTurn {
    id: String,
    commits: Vec<String>,
    #[serde(default)]
    kind: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawPathology {
    kind: String,
    at: String,
    episode: String,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    requires: Vec<String>,
    #[serde(default)]
    why: String,
}

/// One history, resolved: commit facts, the truth turn of each commit, and the pathologies with their
/// commit references turned into positions.
struct History {
    id: String,
    provenance: String,
    clock: String,
    facts: Vec<CommitFacts>,
    /// The turn the *harness* names for each commit, when it names one: the beads claim window on the real
    /// stratum, the scenario's own session/turn on the fixture stratum. Evidence the commit does not carry,
    /// which is exactly why it is kept out of `CommitFacts`.
    harness_turn: Vec<Option<String>>,
    /// Truth turn index per commit position.
    turn_of: Vec<usize>,
    turns: Vec<Turn>,
    pathologies: Vec<Pathology>,
}

struct Turn {
    id: String,
    kind: String,
    commits: Vec<usize>,
}

struct Pathology {
    kind: String,
    at: usize,
    episode: usize,
    paths: BTreeSet<String>,
    requires: Vec<usize>,
    why: String,
}

fn resolve(prefix: &str, oids: &[String]) -> Option<usize> {
    let mut found = oids.iter().enumerate().filter_map(|(i, oid)| {
        if oid.starts_with(prefix) || oid == prefix {
            Some(i)
        } else {
            None
        }
    });
    let first = found.next()?;
    if found.next().is_some() {
        return None;
    }
    Some(first)
}

fn load(path: &Path) -> History {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{}: the turn corpus is unreadable: {e}", path.display()));
    let raw: Raw =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let oids: Vec<String> = raw.lineage.iter().map(|c| c.oid.clone()).collect();
    let facts: Vec<CommitFacts> = raw
        .lineage
        .iter()
        .map(|c| CommitFacts {
            oid: c.oid.clone(),
            // A merge is a commit with two parents; the authored stratum says which ones are merges
            // explicitly because its second parent is a stand-in rather than a real branch head.
            parents: if c.merge && c.parents.len() == 1 {
                vec![
                    c.parents[0].clone(),
                    format!("synthetic-second-parent:{}", c.oid),
                ]
            } else {
                c.parents.clone()
            },
            author_name: c.author_name.clone(),
            author_email: c.author_email.clone(),
            author_utc: c.author_millis,
            subject: c.subject.clone(),
            trailers: c.trailers.clone(),
            paths: c.paths.clone(),
            n_files: c.n_files,
            n_add: c.n_add,
            n_del: c.n_del,
            n_binary: c.n_binary,
            session_id: c.session_id.clone(),
            turn_id: c.turn_id.clone(),
        })
        .collect();
    let harness_turn: Vec<Option<String>> = raw
        .lineage
        .iter()
        .map(|c| {
            c.harness
                .as_ref()
                .and_then(|h| h.get("turn_id"))
                .and_then(|v| v.as_str())
                .map(str::to_owned)
        })
        .collect();
    let mut turns = Vec::new();
    let mut turn_of = vec![usize::MAX; raw.lineage.len()];
    for (index, turn) in raw.truth.turns.iter().enumerate() {
        let mut commits = Vec::new();
        for prefix in &turn.commits {
            let at = resolve(prefix, &oids).unwrap_or_else(|| {
                panic!(
                    "{}: turn {} names {prefix:?}, which is not one commit",
                    path.display(),
                    turn.id
                )
            });
            turn_of[at] = index;
            commits.push(at);
        }
        commits.sort();
        turns.push(Turn {
            id: turn.id.clone(),
            kind: turn.kind.clone().unwrap_or_else(|| "unknown".to_owned()),
            commits,
        });
    }
    let mut pathologies = Vec::new();
    for raw_pathology in &raw.truth.pathologies {
        let at = resolve(&raw_pathology.at, &oids)
            .unwrap_or_else(|| panic!("{}: pathology at {:?}", path.display(), raw_pathology.at));
        let episode = raw
            .truth
            .turns
            .iter()
            .position(|t| t.id == raw_pathology.episode)
            .unwrap_or_else(|| {
                panic!(
                    "{}: pathology names turn {:?}",
                    path.display(),
                    raw_pathology.episode
                )
            });
        let mut paths: BTreeSet<String> = raw_pathology.paths.iter().cloned().collect();
        if paths.is_empty() {
            paths.extend(raw.lineage[at].paths.iter().cloned());
        }
        let requires: Vec<usize> = if raw_pathology.requires.is_empty() {
            // The default co-residence set: everything in the truth turn that touches the pathology's
            // paths. Stated once, here, so the number in the report means one thing everywhere.
            let at_paths: BTreeSet<String> = raw.lineage[at].paths.iter().cloned().collect();
            let union: BTreeSet<String> = paths.union(&at_paths).cloned().collect();
            turns[episode]
                .commits
                .iter()
                .filter_map(|c| {
                    let touches =
                        raw.lineage
                            .get(*c)
                            .map(|commit| {
                                commit.paths.iter().any(|p| {
                                    union.contains(p) || union.contains(&normalize_path(p))
                                })
                            })
                            .unwrap_or(false);
                    if touches || *c == at { Some(*c) } else { None }
                })
                .collect()
        } else {
            raw_pathology
                .requires
                .iter()
                .map(|prefix| {
                    resolve(prefix, &oids).unwrap_or_else(|| {
                        panic!("{}: pathology requires {prefix:?}", path.display())
                    })
                })
                .collect()
        };
        pathologies.push(Pathology {
            kind: raw_pathology.kind.clone(),
            at,
            episode,
            paths,
            requires,
            why: raw_pathology.why.clone(),
        });
    }
    History {
        id: raw.history,
        provenance: raw.provenance,
        clock: raw.clock,
        facts,
        harness_turn,
        turn_of,
        turns,
        pathologies,
    }
}

fn normalize_path(path: &str) -> String {
    path.trim_start_matches("./").to_owned()
}

fn load_all(dir: &Path) -> Vec<History> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    assert!(
        !paths.is_empty(),
        "no turn-corpus files in {}; pass --corpus",
        dir.display()
    );
    paths.iter().map(|path| load(path.as_path())).collect()
}

// ---------------------------------------------------------------------------------------------
// strategies: subsets and reorderings of ADR-003 §4.2's cascade
// ---------------------------------------------------------------------------------------------

/// One rung of the cascade. The spike's `shipped` strategy is the ADR's order; every other strategy is a
/// subset or a reordering of the same rungs, so "what if we only had X" is a question about the rules the
/// product has, not about a model invented for the measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rung {
    /// A rung that always splits: the `every commit is its own episode` baseline.
    Always,
    /// turn evidence the commit itself carries (`Agent-Turn:` trailer or an ingested event).
    HookCommit,
    /// the same, compared session-qualified — what ADR-003 §9.1 says a `turn_id` already is.
    HookCommitQ,
    /// turn evidence the harness holds but does not write on the commit (what `e6.1` would deliver).
    HookHarness,
    /// ADR-003 rung 3 as written: a `Fathom-Episode:` trailer.
    TrailerFathom,
    /// the trailer an agent actually writes, treated as the rung-3 evidence.
    TrailerAgent,
    Branch,
    Author,
    TimeGap,
    /// §4.2 rung 7 verbatim: continuation keeps, TB7's guarded type change splits.
    MessageShape,
    /// rung 7 with TB7's split direction removed: continuation markers still keep, a changed
    /// conventional type never splits. The variant that answers "is TB7 paying for itself".
    MessageShapeKeep,
    /// the ticket-token test placed at rung 7's position (inside message-shape).
    TokenInShape,
    /// the same test promoted above `time-gap`, which is where the measurement says it belongs.
    TokenAboveGap,
}

#[derive(Debug, Clone)]
struct Strategy {
    id: &'static str,
    title: &'static str,
    rationale: &'static str,
    rungs: &'static [Rung],
    gap_minutes: i64,
}

impl Strategy {
    fn gap_millis(&self) -> i64 {
        self.gap_minutes * 60 * 1000
    }
    fn render(&self) -> String {
        self.rungs
            .iter()
            .map(|rung| match rung {
                Rung::Always => "always-split",
                Rung::HookCommit => "hook(commit)",
                Rung::HookCommitQ => "hook(commit,session-qualified)",
                Rung::HookHarness => "hook(harness)",
                Rung::TrailerFathom => "trailer(Fathom-Episode)",
                Rung::TrailerAgent => "trailer(Agent-Turn)",
                Rung::Branch => "branch",
                Rung::Author => "author",
                Rung::TimeGap => "time-gap",
                Rung::MessageShape => "message-shape",
                Rung::MessageShapeKeep => "message-shape(keep-only)",
                Rung::TokenInShape => "ticket-token",
                Rung::TokenAboveGap => "ticket-token",
            })
            .collect::<Vec<_>>()
            .join(" › ")
    }
}

/// The ticket a subject claims, if it claims one: the leading tracker token (`reco-q6r:`, `[JIRA-123]`,
/// `#412 `) that a loop-driven history puts on every commit of a turn and changes between turns.
///
/// Deliberately narrow — a leading token, not a substring anywhere — because the value of this signal is
/// that it is a *claim the author made about this commit*, and a token found in prose about another ticket
/// is not that.
fn ticket_token(subject: &str) -> Option<String> {
    let text = subject.trim();
    let cleaned = text
        .trim_start_matches("fixup! ")
        .trim_start_matches("squash! ")
        .trim_start_matches("amend! ");
    let candidate = if let Some(rest) = cleaned.strip_prefix('[') {
        rest.split(']').next().unwrap_or_default().to_owned()
    } else if cleaned.starts_with('#') {
        cleaned
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '#')
            .collect::<String>()
    } else {
        cleaned
            .split(':')
            .next()
            .unwrap_or_default()
            .trim()
            .to_owned()
    };
    let looks_like_tracker_id = looks_like_tracker_id(&candidate);
    if looks_like_tracker_id {
        Some(candidate.to_lowercase())
    } else {
        None
    }
}

/// The shape test `ticket_token` applies: `reco-nts`, `ADR-005`, `JIRA-1234`, `#412`, and not `chore`,
/// `contracts.yml` or a subject that merely happens to contain a hyphen. A tracker id is a *short prefix
/// with a separator*, which is what makes a false positive rare enough to trust as evidence.
fn looks_like_tracker_id(candidate: &str) -> bool {
    let text = candidate.trim();
    if text.is_empty() || text.len() > 24 || text.contains(' ') {
        return false;
    }
    if let Some(rest) = text.strip_prefix('#') {
        return !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit());
    }
    let mut parts = text.splitn(2, '-');
    let head = parts.next().unwrap_or_default();
    let tail = parts.next().unwrap_or_default();
    if tail.is_empty() {
        return false;
    }
    head.len() >= 2
        && head.len() <= 10
        && head.chars().all(|c| c.is_ascii_alphabetic())
        && tail.len() <= 12
        && tail.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Does this subject carry the shape of a ticket token at all (used to say "no evidence" honestly: a token
/// that is absent is not a token that disagrees — ADR-003 TB6).
fn token_opinion(previous: &str, commit: &str) -> Option<bool> {
    let (mine, theirs) = (ticket_token(commit), ticket_token(previous));
    match (mine, theirs) {
        (Some(mine), Some(theirs)) => Some(mine != theirs),
        _ => None,
    }
}

/// The turn id two commits name, session-qualified when a session is known (ADR-003 §9.1's
/// `<session_id>:<turn_index>`), which is what `Rung::HookCommitQ` compares.
fn qualified(commit: &CommitFacts, qualified: bool) -> Option<String> {
    commit.turn_id.as_ref().map(|turn| {
        if qualified {
            match &commit.session_id {
                Some(session) => format!("{session}:{turn}"),
                None => turn.clone(),
            }
        } else {
            turn.clone()
        }
    })
}

/// One rung's opinion about the boundary before `commit`. `None` is "no opinion", which is what makes the
/// cascade a cascade rather than a vote.
fn rung_opinion(
    rung: Rung,
    strategy: &Strategy,
    rules: &Rules,
    previous: &CommitFacts,
    commit: &CommitFacts,
    previous_harness: Option<&str>,
    harness: Option<&str>,
) -> Option<bool> {
    match rung {
        Rung::Always => Some(true),
        Rung::HookCommit => match (qualified(previous, false), qualified(commit, false)) {
            (Some(mine), Some(theirs)) => Some(mine != theirs),
            _ => None,
        },
        Rung::HookCommitQ => match (qualified(previous, true), qualified(commit, true)) {
            (Some(mine), Some(theirs)) => Some(mine != theirs),
            _ => None,
        },
        Rung::HookHarness => match (previous_harness, harness) {
            (Some(mine), Some(theirs)) => Some(mine != theirs),
            _ => None,
        },
        Rung::TrailerFathom => {
            let mine = commit.trailers.get("fathom-episode")?;
            let same = previous
                .trailers
                .get("fathom-episode")
                .is_some_and(|other| other == mine);
            Some(!same)
        }
        Rung::TrailerAgent => {
            let mine = agent_turn(commit)?;
            let same = agent_turn(previous).is_some_and(|other| other == mine);
            Some(!same)
        }
        Rung::Branch => {
            if commit.is_merge() || previous.is_merge() {
                Some(true)
            } else {
                None
            }
        }
        Rung::Author => {
            if commit.author_identity() != previous.author_identity() {
                Some(true)
            } else {
                None
            }
        }
        Rung::TimeGap => {
            if commit.author_utc - previous.author_utc > strategy.gap_millis() {
                Some(true)
            } else {
                None
            }
        }
        Rung::MessageShape => {
            if segment::is_continuation(&commit.subject, rules) {
                return Some(false);
            }
            let mine = segment::conventional_type(&commit.subject);
            let theirs = segment::conventional_type(&previous.subject);
            if mine.is_some()
                && mine != theirs
                && !segment::shares_path(previous, commit)
                && commit.author_utc - previous.author_utc > strategy.gap_millis() / 9
            {
                return Some(true);
            }
            None
        }
        Rung::MessageShapeKeep => {
            if segment::is_continuation(&commit.subject, rules) {
                return Some(false);
            }
            None
        }
        Rung::TokenInShape | Rung::TokenAboveGap => {
            token_opinion(&previous.subject, &commit.subject)
        }
    }
}

fn agent_turn(commit: &CommitFacts) -> Option<String> {
    let raw = commit
        .trailers
        .get("agent-turn")
        .or_else(|| commit.trailers.get("turn"))
        .cloned()?;
    match commit
        .trailers
        .get("agent-session")
        .or_else(|| commit.trailers.get("session"))
    {
        Some(session) => Some(format!("{session}:{raw}")),
        None => Some(raw),
    }
}

/// Episode index per commit position, for one strategy. Index 0 always opens an episode (ADR-003's `root`).
fn partition(
    history: &History,
    strategy: &Strategy,
    rules: &Rules,
) -> (Vec<usize>, Vec<Option<bool>>) {
    let mut labels = vec![0usize; history.facts.len()];
    let mut fired: Vec<Option<bool>> = vec![None; history.facts.len()];
    let mut current = 0usize;
    for index in 1..history.facts.len() {
        let previous = &history.facts[index - 1];
        let commit = &history.facts[index];
        let mut verdict: Option<bool> = None;
        for rung in strategy.rungs {
            if let Some(opinion) = rung_opinion(
                *rung,
                strategy,
                rules,
                previous,
                commit,
                history.harness_turn[index - 1].as_deref(),
                history.harness_turn[index].as_deref(),
            ) {
                verdict = Some(opinion);
                break;
            }
        }
        if verdict.unwrap_or(false) {
            current += 1;
        }
        labels[index] = current;
        fired[index] = verdict;
    }
    (labels, fired)
}

// ---------------------------------------------------------------------------------------------
// measurement
// ---------------------------------------------------------------------------------------------

/// How a pathology's kind is scored. Stated once so the number in the report is one thing:
///
/// * `split`  — the problem is reviewable only if `requires` sit on one card; a cut inside that set hides it.
/// * `merge`  — the problem is a *turn*; a strategy that swallows the turn into a neighbour hides it,
///   because review state is held per episode (ADR-003 §6), so the neighbour's disposition covers it.
/// * `beyond` — the problem spans two turns by nature (a fix three turns late). No boundary can protect it;
///   the remedy is the lineage window (ADR-001 §5). Counted, never scored against a strategy.
/// * `inherent` — the corpus annotates a real act as two turns because ADR-003 E1 forces contiguity
///   (§8 H2's interleave). The cost is the split, which every correct strategy performs; counted as price.
fn hides_via(kind: &str) -> &'static str {
    match kind {
        "same_turn_rework" | "silent_revert" | "churn_across_boundary" => "split",
        "absorbed_turn" => "merge",
        "cross_turn_correction" => "beyond",
        "turn_cut_by_interleave" => "inherent",
        _ => "split",
    }
}

#[derive(Debug, Default, Clone)]
struct Measure {
    boundaries_true: usize,
    boundaries_pred: usize,
    tp: usize,
    pairs_true: usize,
    pairs_pred: usize,
    pairs_tp: usize,
    turns: usize,
    turns_split: usize,
    turns_absorbed: usize,
    pathologies: usize,
    hidden_split: usize,
    hidden_merge: usize,
    beyond: usize,
    inherent: usize,
    commits: usize,
    histories: usize,
}

impl Measure {
    fn absorb(&mut self, other: &Measure) {
        let a: &mut Measure = self;
        let b = other;
        a.boundaries_true += b.boundaries_true;
        a.boundaries_pred += b.boundaries_pred;
        a.tp += b.tp;
        a.pairs_true += b.pairs_true;
        a.pairs_pred += b.pairs_pred;
        a.pairs_tp += b.pairs_tp;
        a.turns += b.turns;
        a.turns_split += b.turns_split;
        a.turns_absorbed += b.turns_absorbed;
        a.pathologies += b.pathologies;
        a.hidden_split += b.hidden_split;
        a.hidden_merge += b.hidden_merge;
        a.beyond += b.beyond;
        a.inherent += b.inherent;
        a.commits += b.commits;
        a.histories += b.histories;
    }

    fn json(&self) -> Value {
        let rate = |num: usize, den: usize| {
            if den == 0 {
                1.0
            } else {
                (num as f64 / den as f64 * 10_000.0).round() / 10_000.0
            }
        };
        let precision = rate(self.tp, self.boundaries_pred);
        let recall = rate(self.tp, self.boundaries_true);
        let f1 = if precision + recall == 0.0 {
            0.0
        } else {
            (2.0 * precision * recall / (precision + recall) * 10_000.0).round() / 10_000.0
        };
        let pair_precision = rate(self.pairs_tp, self.pairs_pred);
        let pair_recall = rate(self.pairs_tp, self.pairs_true);
        let pair_f1 = if pair_precision + pair_recall == 0.0 {
            0.0
        } else {
            (2.0 * pair_precision * pair_recall / (pair_precision + pair_recall) * 10_000.0).round()
                / 10_000.0
        };
        json!({
            "commits": self.commits,
            "histories": self.histories,
            "boundaries_true": self.boundaries_true,
            "boundaries_predicted": self.boundaries_pred,
            "boundaries_correct": self.tp,
            "boundary_precision": precision,
            "boundary_recall": recall,
            "boundary_f1": f1,
            "pair_f1": pair_f1,
            "pair_precision": pair_precision,
            "pair_recall": pair_recall,
            "turns": self.turns,
            "turns_split": self.turns_split,
            "turns_absorbed": self.turns_absorbed,
            "pathologies_annotated": self.pathologies,
            "pathologies_hidden_by_split": self.hidden_split,
            "pathologies_hidden_by_merge": self.hidden_merge,
            "pathologies_hidden_total": self.hidden_split + self.hidden_merge,
            "pathologies_beyond_segmentation": self.beyond,
            "pathologies_inherent_to_definition": self.inherent,
        })
    }
}

/// Score one strategy on one history: the partitions, and what the partitions do to the annotated
/// pathologies.
fn measure(history: &History, labels: &[usize]) -> Measure {
    let mut m = Measure {
        commits: history.facts.len(),
        histories: 1,
        ..Default::default()
    };
    let turns = &history.turn_of;
    for index in 1..history.facts.len() {
        let truth = turns[index] != turns[index - 1];
        let mine = labels[index] != labels[index - 1];
        if truth {
            m.boundaries_true += 1;
        }
        if mine {
            m.boundaries_pred += 1;
        }
        if truth && mine {
            m.tp += 1;
        }
    }
    for a in 0..history.facts.len() {
        for b in 0..history.facts.len() {
            if a >= b {
                continue;
            }
            let truth = turns[a] == turns[b];
            let mine = labels[a] == labels[b];
            if truth {
                m.pairs_true += 1;
            }
            if mine {
                m.pairs_pred += 1;
            }
            if truth && mine {
                m.pairs_tp += 1;
            }
        }
    }
    m.turns = history.turns.len();
    for (index, turn) in history.turns.iter().enumerate() {
        let first = turn.commits[0];
        if turn.commits.iter().any(|c| labels[*c] != labels[first]) {
            m.turns_split += 1;
        }
        let own: BTreeSet<usize> = turn.commits.iter().map(|c| labels[*c]).collect();
        if history
            .facts
            .iter()
            .enumerate()
            .any(|(c, _)| turns[c] != index && own.contains(&labels[c]))
        {
            m.turns_absorbed += 1;
        }
    }
    for pathology in &history.pathologies {
        m.pathologies += 1;
        match hides_via(&pathology.kind) {
            "split" => {
                let here = labels[pathology.at];
                if pathology
                    .requires
                    .iter()
                    .any(|other| labels[*other] != here)
                {
                    m.hidden_split += 1;
                }
            }
            "merge" => {
                if history.facts.iter().enumerate().any(|(c, _)| {
                    turns[c] != pathology.episode && labels[c] == labels[pathology.at]
                }) {
                    m.hidden_merge += 1;
                }
            }
            "beyond" => m.beyond += 1,
            _ => m.inherent += 1,
        }
    }
    m
}

/// For every boundary position, which single signal would have fired there. This is the table that says
/// what a strategy could ever have known, independent of what the others outrank it.
fn census(
    history: &History,
    rules: &Rules,
    gap_minutes: i64,
) -> BTreeMap<String, BTreeMap<String, usize>> {
    // per signal: [true-boundaries-detected, true-boundaries-missed, false-boundaries-fired,
    //              false-boundaries-withheld]
    let signals = [
        "hook(commit)",
        "hook(commit,session-qualified)",
        "hook(harness)",
        "trailer(Fathom-Episode)",
        "trailer(Agent-Turn)",
        "branch",
        "author",
        "time-gap",
        "message-shape",
        "ticket-token",
    ];
    let mut out: BTreeMap<String, BTreeMap<String, usize>> = signals
        .iter()
        .map(|name| {
            (
                (*name).to_owned(),
                [
                    "true_boundaries_found",
                    "true_boundaries_missed",
                    "false_boundaries_fired",
                    "false_boundaries_withheld",
                ]
                .iter()
                .map(|key| ((*key).to_owned(), 0usize))
                .collect(),
            )
        })
        .collect();
    let probe = Strategy {
        id: "probe",
        title: "probe",
        rationale: "",
        rungs: &[],
        gap_minutes,
    };
    for index in 1..history.facts.len() {
        let truth = history.turn_of[index] != history.turn_of[index - 1];
        let pairs = [
            ("hook(commit)", Rung::HookCommit),
            ("hook(commit,session-qualified)", Rung::HookCommitQ),
            ("hook(harness)", Rung::HookHarness),
            ("trailer(Fathom-Episode)", Rung::TrailerFathom),
            ("trailer(Agent-Turn)", Rung::TrailerAgent),
            ("branch", Rung::Branch),
            ("author", Rung::Author),
            ("time-gap", Rung::TimeGap),
            ("message-shape", Rung::MessageShape),
            ("ticket-token", Rung::TokenAboveGap),
        ];
        for (name, rung) in pairs {
            let opinion = rung_opinion(
                rung,
                &probe,
                rules,
                &history.facts[index - 1],
                &history.facts[index],
                history.harness_turn[index - 1].as_deref(),
                history.harness_turn[index].as_deref(),
            );
            let slot = out
                .get_mut(name)
                .unwrap_or_else(|| panic!("census signal {name} vanished from its own table"));
            let key = match (truth, opinion) {
                (true, Some(true)) => "true_boundaries_found",
                (true, _) => "true_boundaries_missed",
                (false, Some(true)) => "false_boundaries_fired",
                (false, _) => "false_boundaries_withheld",
            };
            *slot.entry(key.to_owned()).or_insert(0) += 1;
        }
    }
    out
}

/// Which annotated pathologies a partition hides, with the reasoning the annotator wrote down. This is the
/// table the recommendation's "failure modes, enumerated" acceptance criterion is made of: not a count, the
/// named instances.
fn hidden_pathologies(history: &History, labels: &[usize]) -> Vec<Value> {
    let mut out = Vec::new();
    for pathology in &history.pathologies {
        let hidden = match hides_via(&pathology.kind) {
            "split" => {
                let here = labels[pathology.at];
                pathology
                    .requires
                    .iter()
                    .any(|other| labels[*other] != here)
            }
            "merge" => history.facts.iter().enumerate().any(|(c, _)| {
                history.turn_of[c] != pathology.episode && labels[c] == labels[pathology.at]
            }),
            _ => false,
        };
        if hidden {
            out.push(json!({
                "history": history.id,
                "turn": history.turns[pathology.episode].id,
                "kind": pathology.kind,
                "at": &history.facts[pathology.at].oid[..12],
                "at_subject": history.facts[pathology.at].subject,
                "paths": pathology.paths.iter().take(4).cloned().collect::<Vec<String>>(),
                "why": pathology.why,
            }));
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// the strategy table
// ---------------------------------------------------------------------------------------------

/// The cascade ADR-003 §4.2 ships, in its published order, minus `override` (no forced boundaries in the
/// corpus) and `root` (the absence of a boundary, handled by every strategy the same way).
///
/// `hook` is compared **session-qualified**: ADR-003 §9.1 always said a `turn_id` is
/// `<session_id>:<turn_index>`, §18.2 **D4** found the shipped transport carrying the bare index, and
/// `reco-quk.3` landed the qualification — so this list is production again, which is the only reason the
/// harness's production-equality check below passes.
const SHIPPED: &[Rung] = &[
    Rung::HookCommitQ,
    Rung::TrailerFathom,
    Rung::Branch,
    Rung::Author,
    Rung::TimeGap,
    Rung::MessageShape,
];

fn strategies() -> Vec<Strategy> {
    let out = vec![
        Strategy {
            id: "all-one",
            title: "never split: the whole history is one episode",
            rationale: "the floor. A strategy that cannot beat it is not detecting boundaries at all.",
            rungs: &[],
            gap_minutes: 30,
        },
        Strategy {
            id: "all-split",
            title: "every commit is its own episode",
            rationale: "the floor on the other side, and the one an agent-loop history makes unnervingly \
                        strong: most turns are one commit.",
            rungs: &[Rung::Always],
            gap_minutes: 30,
        },
        Strategy {
            id: "A-time-author",
            title: "time and author heuristics only",
            rationale: "the ticket's first strategy: what a reviewer gets from git alone, with no harness \
                        evidence of any kind.",
            rungs: &[Rung::Author, Rung::TimeGap],
            gap_minutes: 30,
        },
        Strategy {
            id: "B-hook-commit",
            title: "hook-event-driven boundaries, commit evidence only",
            rationale: "the ticket's second strategy as shipped: turn evidence that reaches `turn_id` today \
                        means a trailer, and a trailer means `e6.1` wired the harness.",
            rungs: &[Rung::HookCommit],
            gap_minutes: 30,
        },
        Strategy {
            id: "B-hook-harness",
            title: "hook-event-driven boundaries, harness evidence",
            rationale: "the same rule with the evidence the harness holds but does not write on the commit: \
                        the ceiling of the hook strategy once `e6.1` lands.",
            rungs: &[Rung::HookHarness],
            gap_minutes: 30,
        },
        Strategy {
            id: "B-hook-then-git",
            title: "hook first, git heuristics where there is no evidence",
            rationale: "the thing the ADR's §9.2 degradation table actually implies for a partially wired \
                        harness, and the honest answer to \"what would we ship if hooks arrived tomorrow\".",
            rungs: &[Rung::HookCommit, Rung::Branch, Rung::Author, Rung::TimeGap],
            gap_minutes: 30,
        },
        Strategy {
            id: "C-trailer-shape",
            title: "trailers plus message-shape signals",
            rationale: "the ticket's third strategy, spelled as ADR-003 writes it: rung 3 keys on a \
                        `Fathom-Episode:` trailer, which nothing in the corpus writes except the history \
                        authored to carry it.",
            rungs: &[Rung::TrailerFathom, Rung::MessageShape],
            gap_minutes: 30,
        },
        Strategy {
            id: "C-agent-trailer-shape",
            title: "the trailer agents actually write, plus message-shape",
            rationale: "the same strategy spelled as the world writes it: `Agent-Turn:`/`Agent-Session:`.",
            rungs: &[Rung::TrailerAgent, Rung::MessageShape],
            gap_minutes: 30,
        },
        Strategy {
            id: "D-shipped",
            title: "the cascade ADR-003 §4.2 ships, at its published constants",
            rationale: "the incumbent: hook(session-qualified) › trailer › branch › author › time-gap(30) › message-shape.",
            rungs: SHIPPED,
            gap_minutes: 30,
        },
        Strategy {
            id: "E-token-in-shape",
            title: "shipped, with the ticket-token test inside message-shape",
            rationale: "the minimal change: a token pair is one more message-shape signal, so it sits at \
                        rung 7 and loses to the clock whenever the clock has an opinion.",
            rungs: &[
                Rung::HookCommit,
                Rung::TrailerFathom,
                Rung::Branch,
                Rung::Author,
                Rung::TimeGap,
                Rung::TokenInShape,
                Rung::MessageShape,
            ],
            gap_minutes: 30,
        },
        Strategy {
            id: "E-token-above-gap",
            title: "shipped, with the ticket-token test above time-gap",
            rationale: "the same signal promoted: a claim the author made about the commit is evidence, not \
                        a guess about the clock, so it belongs above the clock.",
            rungs: &[
                Rung::HookCommit,
                Rung::TrailerFathom,
                Rung::Branch,
                Rung::Author,
                Rung::TokenAboveGap,
                Rung::TimeGap,
                Rung::MessageShape,
            ],
            gap_minutes: 30,
        },
        Strategy {
            id: "G-no-tb7",
            title: "the recommendation with TB7's split direction removed",
            rationale: "the measurement for keeping ADR-003's weakest rule: does TB7's split direction pay \n                        for the false cuts it makes?",
            rungs: &[
                Rung::HookCommit,
                Rung::TrailerFathom,
                Rung::Branch,
                Rung::Author,
                Rung::TokenAboveGap,
                Rung::TimeGap,
                Rung::MessageShapeKeep,
            ],
            gap_minutes: 30,
        },
        Strategy {
            id: "F-hook-qualified",
            title: "the promoted token, plus session-qualified turn ids",
            rationale: "E-token-above-gap with `turn_id` compared as §9.1 defines it, which is the fix for \
                        the collision the trailer transport loses.",
            rungs: &[
                Rung::HookCommitQ,
                Rung::TrailerFathom,
                Rung::Branch,
                Rung::Author,
                Rung::TokenAboveGap,
                Rung::TimeGap,
                Rung::MessageShape,
            ],
            gap_minutes: 30,
        },
    ];
    out
}

fn sweep_strategies() -> Vec<(&'static str, &'static [Rung])> {
    vec![
        ("A-time-author", &[Rung::Author, Rung::TimeGap]),
        ("D-shipped", SHIPPED),
        (
            "E-token-above-gap",
            &[
                Rung::HookCommit,
                Rung::TrailerFathom,
                Rung::Branch,
                Rung::Author,
                Rung::TokenAboveGap,
                Rung::TimeGap,
                Rung::MessageShape,
            ],
        ),
    ]
}

// ---------------------------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------------------------

/// Turn a `segment::segment` result into the same label shape `partition` produces, so the two can be
/// compared position by position.
fn production_labels(history: &History, rules: &Rules) -> Vec<usize> {
    let episodes = segment::segment(&history.facts, rules);
    let mut labels = vec![usize::MAX; history.facts.len()];
    let position: BTreeMap<&str, usize> = history
        .facts
        .iter()
        .enumerate()
        .map(|(index, fact)| (fact.oid.as_str(), index))
        .collect();
    for (ordinal, episode) in episodes.iter().enumerate() {
        for member in &episode.members {
            if let Some(index) = position.get(member.as_str()) {
                labels[*index] = ordinal;
            }
        }
    }
    labels
}

fn same_partition(a: &[usize], b: &[usize]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for index in 1..a.len() {
        let left = a[index] != a[index - 1];
        let right = b[index] != b[index - 1];
        if left != right {
            return false;
        }
    }
    true
}

/// The spike's own key signal, checked before it is used to score anything. A spike that silently
/// mis-parsed its own new signal would report a verdict about its parser.
fn self_check() {
    let cases: &[(&str, Option<&str>)] = &[
        (
            "reco-nts: a boundary a human draws is configuration",
            Some("reco-nts"),
        ),
        ("ADR-005: harness integration contract", Some("adr-005")),
        (
            "fixup! reco-a11: wip, the markdown writer",
            Some("reco-a11"),
        ),
        ("[JIRA-1234] the export verb", Some("jira-1234")),
        ("#412 add the thing", Some("#412")),
        ("chore: keep tmp.txt untracked", None),
        ("contracts.yml: grep the keyprobe's real machine keys", None),
        ("docs: ADR-001 product thesis (reco-982)", None),
        ("Round 1: settle the window and the spin", None),
        ("Add the scheduler and the queue", None),
        ("chore(deps): bump serde to 1.0.219", None),
        ("revert \"feat: the export footer\"", None),
    ];
    for (subject, expected) in cases {
        let found = ticket_token(subject);
        let ok = found.as_deref() == *expected;
        assert!(
            ok,
            "ticket_token({subject:?}) = {found:?}, expected {expected:?}"
        );
    }
}

fn metric(row: &Value, name: &str) -> Option<f64> {
    row.get(name).and_then(Value::as_f64)
}

fn main() {
    self_check();
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let check = args.iter().any(|a| a == "--check");
    let root = PathBuf::from(arg("--root").as_deref().unwrap_or("."));
    let corpus_dir = PathBuf::from(
        arg("--corpus")
            .as_deref()
            .unwrap_or("crates/fathom-fixtures/corpus/turns"),
    )
    .let_relative(&root);
    let report_path = PathBuf::from(
        arg("--report")
            .as_deref()
            .unwrap_or("benchmarks/reports/s2-segmentation-accuracy.json"),
    )
    .let_relative(&root);
    let thresholds_path = PathBuf::from(
        arg("--thresholds")
            .as_deref()
            .unwrap_or("crates/fathom-analyze/spike/s2/thresholds.json"),
    )
    .let_relative(&root);

    let histories = load_all(&corpus_dir);
    let rules = Rules::default();
    let published_gap_minutes = rules.gap_millis / 60_000;

    // The spike's own floor: its `shipped` strategy must be production, not the spike's idea of it.
    let shipped = strategies()
        .into_iter()
        .find(|s| s.id == "D-shipped")
        .unwrap_or_else(|| {
            panic!("the spike must carry the incumbent strategy it is scored against")
        });
    let mut equality: Vec<Value> = Vec::new();
    for history in &histories {
        let (mine, _) = partition(history, &shipped, &rules);
        let theirs = production_labels(history, &rules);
        if !same_partition(&mine, &theirs) {
            let mut detail = Vec::new();
            for index in 1..history.facts.len() {
                let a = mine[index] != mine[index - 1];
                let b = theirs[index] != theirs[index - 1];
                if a != b {
                    detail.push(json!({"commit": &history.facts[index].oid[0..12],
                                       "spike_splits": a, "production_splits": b}));
                }
            }
            equality.push(json!({"history": history.id, "differences": detail}));
        }
    }

    let scopes = ["pooled", "real", "fixture", "authored"];
    let mut per_strategy: Vec<Value> = Vec::new();
    for strategy in strategies() {
        let mut totals: BTreeMap<String, Measure> = scopes
            .iter()
            .map(|scope| ((*scope).to_owned(), Measure::default()))
            .collect();
        let mut hides: Vec<Value> = Vec::new();
        for history in &histories {
            let (labels, _) = partition(history, &strategy, &rules);
            let measured = measure(history, &labels);
            for row in hidden_pathologies(history, &labels) {
                hides.push(row);
            }
            for scope in ["pooled", history.provenance.as_str()] {
                totals
                    .get_mut(scope)
                    .unwrap_or_else(|| panic!("scope {scope:?} is built above and must exist"))
                    .absorb(&measured);
            }
            totals.insert(format!("history:{}", history.id), measured.clone());
        }
        per_strategy.push(json!({
            "id": strategy.id,
            "title": strategy.title,
            "rationale": strategy.rationale,
            "rungs": strategy.render(),
            "gap_minutes": strategy.gap_minutes,
            "hides": hides,
            "results": totals
                .iter()
                .map(|(scope, measure)| (scope.clone(), measure.json()))
                .collect::<serde_json::Map<String, Value>>(),
        }));
    }

    // The gap sweep: every value a reviewer might set `episode.time_gap_minutes` to, against the strategies
    // whose behaviour it changes.
    let mut gap_sweep: Vec<Value> = Vec::new();
    for (base_id, rungs) in sweep_strategies() {
        for gap in [1i64, 2, 5, 10, 15, 20, 30, 45, 60, 90, 120, 240] {
            let strategy = Strategy {
                id: base_id,
                title: "",
                rationale: "",
                rungs,
                gap_minutes: gap,
            };
            let mut totals: BTreeMap<String, Measure> = scopes
                .iter()
                .map(|scope| ((*scope).to_owned(), Measure::default()))
                .collect();
            for history in &histories {
                let (labels, _) = partition(history, &strategy, &rules);
                let measured = measure(history, &labels);
                totals
                    .get_mut("pooled")
                    .unwrap_or_else(|| panic!("scope `pooled` is built above and must exist"))
                    .absorb(&measured);
                let scope = history.provenance.as_str();
                totals
                    .get_mut(scope)
                    .unwrap_or_else(|| panic!("scope {scope:?} is built above and must exist"))
                    .absorb(&measured);
            }
            gap_sweep.push(json!({
                "strategy": base_id,
                "gap_minutes": gap,
                "published_default": gap == published_gap_minutes,
                "pooled": totals["pooled"].json(),
                "real": totals["real"].json(),
                "fixture": totals["fixture"].json(),
                "authored": totals["authored"].json(),
            }));
        }
    }

    // The signal census at the published gap.
    let mut census_totals: BTreeMap<String, BTreeMap<String, BTreeMap<String, usize>>> =
        BTreeMap::new();
    for scope in scopes {
        let mut totals: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
        for history in &histories {
            if scope != "pooled" && history.provenance != scope {
                continue;
            }
            for (signal, counts) in census(history, &rules, published_gap_minutes) {
                let slot = totals.entry(signal).or_default();
                for (key, value) in counts {
                    *slot.entry(key).or_insert(0) += value;
                }
            }
        }
        census_totals.insert(scope.to_owned(), totals);
    }

    // Corpus statistics: what the annotated turns actually look like, since that is what makes a
    // precision number mean anything.
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut turn_sizes: BTreeMap<String, usize> = BTreeMap::new();
    let mut turn_kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut corpus_histories: Vec<Value> = Vec::new();
    let mut totals_by: BTreeMap<String, [usize; 4]> = BTreeMap::new();
    for history in &histories {
        for turn in &history.turns {
            *turn_kinds.entry(turn.kind.clone()).or_insert(0) += 1;
            let bucket = match turn.commits.len() {
                1 => "1",
                2 => "2",
                3 => "3",
                _ => "4 or more",
            };
            *turn_sizes.entry(bucket.to_owned()).or_insert(0) += 1;
        }
        for pathology in &history.pathologies {
            *kinds.entry(pathology.kind.clone()).or_insert(0) += 1;
        }
        let row = totals_by
            .entry(history.provenance.clone())
            .or_insert([0; 4]);
        row[0] += history.facts.len();
        row[1] += history.turns.len();
        row[2] += history.pathologies.len();
        row[3] += history
            .facts
            .iter()
            .filter(|fact| fact.turn_id.is_some())
            .count();
        corpus_histories.push(json!({
            "history": history.id,
            "provenance": history.provenance,
            "commits": history.facts.len(),
            "turns": history.turns.len(),
            "pathologies": history.pathologies.len(),
            "clock": history.clock,
        }));
    }

    let gates = evaluate_gates(&thresholds_path, &per_strategy, &gap_sweep, &census_totals);
    let gates_as_expected = gates
        .iter()
        .all(|row| row["as_expected"].as_bool().unwrap_or(false));

    let report = json!({
        "what_these_numbers_are": "Boundary precision and recall of ADR-003 §4.2's cascade and six \
            alternative rule subsets, scored on the hand-annotated turn corpus in \
            crates/fathom-fixtures/corpus/turns (see its README for what each stratum can certify). A \
            boundary is a commit that opens an episode; precision is the share of drawn boundaries that are \
            right, recall the share of the annotated ones that were found. Pair F1 is the co-membership \
            measure `e1.3` gates on, printed beside it because on agent histories the two disagree. \
            Everything here is deterministic: no clock, no wall order, no randomness — `--check` compares \
            bytes.",
        "definitions": {
            "boundary": "a commit position where an episode starts; position 0 is never a boundary",
            "turns_split": "annotated turns the strategy cut into more than one episode",
            "turns_absorbed": "annotated turns sharing an episode with at least one other turn",
            "pathologies_hidden_by_split": "a split-hiding pathology whose `requires` set the strategy cut",
            "pathologies_hidden_by_merge": "a merge-hiding pathology whose turn the strategy absorbed",
            "pathologies_beyond_segmentation": "annotated problems that span two turns by nature: no \
                boundary can protect them and none should be bent to try",
            "pathologies_inherent_to_definition": "annotated real acts the corpus records as two turns \
                because ADR-003 E1 makes episodes contiguous (§8 H2)"
        },
        "corpus": {
            "histories": corpus_histories,
            "totals": {
                "histories": histories.len(),
                "commits": histories.iter().map(|h| h.facts.len()).sum::<usize>(),
                "turns": histories.iter().map(|h| h.turns.len()).sum::<usize>(),
                "pathologies": histories.iter().map(|h| h.pathologies.len()).sum::<usize>(),
                "by_provenance": totals_by
                    .into_iter()
                    .map(|(k, v)| {
                        (k, json!({"commits": v[0], "turns": v[1], "pathologies": v[2],
                                   "commits_with_turn_evidence": v[3]}))
                    })
                    .collect::<serde_json::Map<String, Value>>(),
                "turn_sizes": turn_sizes,
                "turn_kinds": turn_kinds,
            },
        },
        "pathology_kinds": kinds
            .into_iter()
            .map(|(kind, count)| {
                let via = hides_via(&kind).to_owned();
                (kind, json!({"count": count, "scored_via": via}))
            })
            .collect::<serde_json::Map<String, Value>>(),
        "production_equality": {
            "shipped_strategy_matches_segment": equality.is_empty(),
            "histories_compared": histories.len(),
            "differences": equality,
        },
        "signal_census": census_totals,
        "strategies": per_strategy,
        "gap_sweep": gap_sweep,
        "gates": gates,
        "gates_all_as_expected": gates_as_expected,
    });

    let rendered = serde_json::to_string_pretty(&report)
        .unwrap_or_else(|e| panic!("the report the spike built cannot be serialized: {e}"))
        + "\n";
    if check {
        let committed = std::fs::read_to_string(&report_path).unwrap_or_else(|_| {
            panic!(
                "no report at {}; run without --check first",
                report_path.display()
            )
        });
        if committed == rendered {
            println!("s2 spike: report matches the corpus exactly");
            return;
        }
        eprintln!(
            "s2 spike: the committed report is not what the corpus and the rules produce now."
        );
        let gate_fail = report
            .get("gates")
            .and_then(|g| g.as_array())
            .map(|rows| {
                rows.iter()
                    .filter(|r| !r["as_expected"].as_bool().unwrap_or(false))
                    .count()
            })
            .unwrap_or(0);
        eprintln!(
            "s2 spike: {} gate(s) came out other than claimed — see the report",
            gate_fail
        );
        std::process::exit(1);
    }
    if let Some(parent) = report_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(&report_path, &rendered)
        .unwrap_or_else(|e| panic!("{}: cannot write the report: {e}", report_path.display()));
    println!(
        "s2 spike: wrote {} ({} bytes)",
        report_path.display(),
        rendered.len()
    );
    let rows = |key: &str| -> Vec<Value> {
        report
            .get(key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_else(|| panic!("the report the spike just built has no `{key}` array"))
    };
    let number = |row: &Value, path: &str| -> f64 {
        row.pointer(path)
            .and_then(Value::as_f64)
            .unwrap_or_else(|| panic!("the report the spike just built has no numeric {path:?}"))
    };
    for row in rows("strategies") {
        println!(
            "  {:<24} pooled P {:.3} R {:.3} F1 {:.3} | real F1 {:.3} | hidden {}/{}",
            row["id"].as_str().unwrap_or("?"),
            number(&row, "/results/pooled/boundary_precision"),
            number(&row, "/results/pooled/boundary_recall"),
            number(&row, "/results/pooled/boundary_f1"),
            number(&row, "/results/real/boundary_f1"),
            number(&row, "/results/pooled/pathologies_hidden_total"),
            number(&row, "/results/pooled/pathologies_annotated"),
        );
    }
    for row in rows("gates") {
        let note = if row["as_expected"].as_bool().unwrap_or(false) {
            String::new()
        } else {
            format!(
                "  \u{2190} expected {}, which is the finding",
                row["expected"].as_str().unwrap_or("?")
            )
        };
        println!(
            "  gate {:<38} {:<5} measured {:?}{}",
            row["id"].as_str().unwrap_or("?"),
            if row["pass"].as_bool().unwrap_or(false) {
                "PASS"
            } else {
                "FAIL"
            },
            row["measured"].as_f64().unwrap_or(0.0),
            note,
        );
    }
    if !gates_as_expected {
        eprintln!(
            "s2 spike: a gate came out the other way from what thresholds.json claims; re-read it"
        );
        std::process::exit(1);
    }
}

trait LetRelative {
    fn let_relative(self, root: &Path) -> PathBuf;
}

impl LetRelative for PathBuf {
    fn let_relative(self, root: &Path) -> PathBuf {
        if self.is_absolute() {
            self
        } else {
            root.join(self)
        }
    }
}

/// Score the gates `crates/fathom-analyze/spike/s2/thresholds.json` declares. A gate is a claim about the
/// measurement that the document is not allowed to contradict, and it is written down as data rather than as
/// prose so that "did the spike actually clear what it says it cleared" is a lookup and not a reading
/// comprehension test.
///
/// Two kinds: `threshold` (a strategy/scope/metric must be within a bound) and `gap_argmax` (the gap that
/// maximises a metric must sit at or below a bound, which is the shape of "the published default is not
/// where the optimum is").
fn evaluate_gates(
    path: &Path,
    strategies: &[Value],
    gap_sweep: &[Value],
    census: &BTreeMap<String, BTreeMap<String, BTreeMap<String, usize>>>,
) -> Vec<Value> {
    let thresholds: Value = match std::fs::read_to_string(path) {
        Ok(text) => {
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        }
        Err(_) => panic!(
            "no thresholds file at {}; the spike's gates must be committed",
            path.display()
        ),
    };
    let gates = thresholds
        .get("gates")
        .and_then(|g| g.as_array())
        .unwrap_or_else(|| panic!("{}: no `gates` array", path.display()));
    let mut rows = Vec::new();
    for gate in gates {
        let id = gate["id"].as_str().unwrap_or("?").to_owned();
        let claim = gate["claim"].as_str().unwrap_or("").to_owned();
        let kind = gate["kind"].as_str().unwrap_or("threshold");
        let scope = gate["scope"].as_str().unwrap_or("pooled");
        let metric_name = gate["metric"].as_str().unwrap_or("boundary_f1");
        let (measured, pass, detail) = match kind {
            "census" => {
                let signal = gate["signal"].as_str().unwrap_or("");
                let value = census
                    .get(scope)
                    .and_then(|inner| inner.get(signal))
                    .and_then(|counts| {
                        counts
                            .get(metric_name)
                            .map(|count| f64::from(u32::try_from(*count).unwrap_or(u32::MAX)))
                    });
                match value {
                    Some(value) => {
                        let min = gate["min"].as_f64();
                        let max = gate["max"].as_f64();
                        let pass = min.is_none_or(|limit| value >= limit)
                            && max.is_none_or(|limit| value <= limit);
                        (value, pass, json!({"signal": signal, "scope": scope}))
                    }
                    None => (
                        0.0,
                        false,
                        json!({"error": "no census row for that signal and scope"}),
                    ),
                }
            }
            "delta" => {
                let a = strategies
                    .iter()
                    .find(|s| s["id"].as_str() == gate["strategy_a"].as_str());
                let b = strategies
                    .iter()
                    .find(|s| s["id"].as_str() == gate["strategy_b"].as_str());
                match (a, b) {
                    (Some(a), Some(b)) => {
                        let value = metric(&a["results"][scope], metric_name).unwrap_or(0.0)
                            - metric(&b["results"][scope], metric_name).unwrap_or(0.0);
                        let value = (value * 10_000.0).round() / 10_000.0;
                        let min = gate["min"].as_f64();
                        let max = gate["max"].as_f64();
                        let pass = min.is_none_or(|limit| value >= limit)
                            && max.is_none_or(|limit| value <= limit);
                        (
                            value,
                            pass,
                            json!({"a": gate["strategy_a"], "b": gate["strategy_b"], "scope": scope}),
                        )
                    }
                    _ => (
                        0.0,
                        false,
                        json!({"error": "a strategy named by this gate does not exist"}),
                    ),
                }
            }
            "gap_argmax" => {
                let wanted = gate["gap_sweep_of"].as_str().unwrap_or("");
                let rows: Vec<&Value> = gap_sweep
                    .iter()
                    .filter(|row| row["strategy"].as_str() == Some(wanted))
                    .collect();
                // Ties go to the smallest gap: a plateau's named optimum should be the cheapest value on
                // it, not the last one the sweep reached at the same height.
                let best = rows.iter().fold(None::<(i64, f64)>, |acc, row| {
                    let gap = row["gap_minutes"].as_i64().unwrap_or(0);
                    let value = metric(&row[scope], metric_name).unwrap_or(0.0);
                    match acc {
                        None => Some((gap, value)),
                        Some((best_gap, best_value)) => {
                            if value > best_value + 1e-12 || (value == best_value && gap < best_gap)
                            {
                                Some((gap, value))
                            } else {
                                acc
                            }
                        }
                    }
                });
                match best {
                    Some((gap, value)) => {
                        let max_gap = gate["max_gap_minutes"].as_i64();
                        let min_gap = gate["min_gap_minutes"].as_i64();
                        let pass = max_gap.is_none_or(|limit| gap <= limit)
                            && min_gap.is_none_or(|limit| gap >= limit);
                        (
                            value,
                            pass,
                            json!({"best_gap_minutes": gap, "at_value": value}),
                        )
                    }
                    None => (
                        0.0,
                        false,
                        json!({"error": "no sweep rows for that strategy"}),
                    ),
                }
            }
            _ => {
                let row = strategies
                    .iter()
                    .find(|s| s["id"].as_str() == gate["strategy"].as_str());
                match row {
                    Some(row) => {
                        let value = metric(&row["results"][scope], metric_name).unwrap_or(0.0);
                        let min = gate["min"].as_f64();
                        let max = gate["max"].as_f64();
                        let pass = min.is_none_or(|limit| value >= limit)
                            && max.is_none_or(|limit| value <= limit);
                        (value, pass, json!({"strategy": row["id"], "scope": scope}))
                    }
                    None => (0.0, false, json!({"error": "no strategy with that id"})),
                }
            }
        };
        let expected = gate["expected"].as_str().unwrap_or("pass");
        let expected_pass = expected != "fail";
        rows.push(json!({
            "id": id,
            "claim": claim,
            "kind": kind,
            "metric": metric_name,
            "expected": if expected_pass { "pass" } else { "fail" },
            "measured": measured,
            "as_expected": pass == expected_pass,
            "bound": {
                "min": gate.get("min").cloned().unwrap_or(Value::Null),
                "max": gate.get("max").cloned().unwrap_or(Value::Null),
                "max_gap_minutes": gate.get("max_gap_minutes").cloned().unwrap_or(Value::Null),
                "min_gap_minutes": gate.get("min_gap_minutes").cloned().unwrap_or(Value::Null),
            },
            "detail": detail,
            "pass": pass,
        }));
    }
    rows
}
