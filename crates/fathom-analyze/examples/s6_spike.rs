//! Spike `s6` (`reco-f5a`) — what do a false relink and a missed relink cost, on annotated ground truth?
//!
//! **This is a spike, not a library.** ADR-003 §13.1 declined fuzzy re-linking "pending the false-relink
//! and missed-relink rates against annotated ground truth", spike `s2` measured *boundaries* instead, and
//! `docs/concepts/episode-segmentation.md` §8 recorded that the relink half was still owed. This is that
//! half. The corpus is `crates/fathom-fixtures/corpus/relinks/` — hand-authored rewrites over `s2`'s
//! annotated turn corpus, each stating where a decision belongs after the rewrite — and the written answer
//! is `docs/concepts/episode-relink.md`.
//!
//! ```text
//! cargo run -q -p fathom-analyze --features sqlite-bundled --example s6_spike -- \
//!     --report benchmarks/reports/s6-relink-accuracy.json
//! cargo run -q -p fathom-analyze --features sqlite-bundled --example s6_spike -- --check
//! ```
//!
//! The shipped relink pass is not reimplemented here and hoped to match: the ladder called `shipped` is a
//! copy of [`fathom_store::episode`]'s relink pass, and the harness runs **both** over every corpus event —
//! the copy, and `Store::write_episodes`' real pass on the same episodes and review rows — and fails unless
//! they agree link for link. A spike whose idea of the shipped tiers drifted from the tiers would measure
//! the spike.
//!
//! Two axes are scored and never pooled together, because they answer different questions:
//!
//! * **ladder** — which tiers are in play: each shipped tier alone, the shipped three together, and shipped
//!   plus a candidate fuzzy rung at a grid of thresholds and disciplines;
//! * **board** — how much of the corpus a reviewer has actually dispositioned, which is what RL0 (never
//!   relink into an episode that already has a decision) multiplies everything by.

#![allow(clippy::print_stdout, clippy::print_stderr, clippy::too_many_arguments)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::float_cmp
)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::needless_pass_by_value
)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use fathom_ingest::segment::{self, CommitFacts};
use serde::Deserialize;
use serde_json::{Value, json};

// ---------------------------------------------------------------------------------------------
// the turn corpus: the histories every rewrite is performed on
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RawHistory {
    history: String,
    provenance: String,
    lineage: Vec<RawCommit>,
    truth: RawTruth,
}

#[derive(Debug, Deserialize)]
struct RawCommit {
    oid: String,
    author_name: String,
    #[serde(default)]
    author_email: String,
    author_millis: i64,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawTruth {
    turns: Vec<RawTurn>,
}

#[derive(Debug, Deserialize)]
struct RawTurn {
    id: String,
    commits: Vec<String>,
}

/// One history, resolved: commits in lineage order, and the annotated turns over them.
struct History {
    id: String,
    provenance: String,
    commits: Vec<Commit>,
    turns: Vec<Turn>,
}

#[derive(Debug, Clone)]
struct Commit {
    oid: String,
    name: String,
    email: String,
    subject: String,
    paths: Vec<String>,
    millis: i64,
}

impl Commit {
    /// ADR-003 §2.6's author identity, from the same function the shipped digest uses.
    fn author_identity(&self) -> String {
        CommitFacts {
            author_name: self.name.clone(),
            author_email: self.email.clone(),
            ..CommitFacts::default()
        }
        .author_identity()
    }

    /// The normalised subject as a bag of tokens: what a subject-similarity rung would compare.
    fn tokens(&self) -> BTreeSet<String> {
        segment::normalize_subject(&self.subject)
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| t.len() > 1)
            .map(str::to_owned)
            .collect()
    }
}

struct Turn {
    id: String,
    /// Indices into [`History::commits`], in lineage order.
    commits: Vec<usize>,
}

fn load_history(path: &Path) -> History {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{}: the turn corpus is unreadable: {e}", path.display()));
    let raw: RawHistory =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let commits: Vec<Commit> = raw
        .lineage
        .iter()
        .map(|c| Commit {
            oid: c.oid.clone(),
            name: c.author_name.clone(),
            email: c.author_email.clone(),
            subject: c.subject.clone(),
            paths: c.paths.clone(),
            millis: c.author_millis,
        })
        .collect();
    let mut turns = Vec::new();
    for t in raw.truth.turns {
        let mut members = Vec::new();
        for prefix in &t.commits {
            let found: Vec<usize> = commits
                .iter()
                .enumerate()
                .filter(|(_, c)| c.oid.starts_with(prefix.as_str()))
                .map(|(i, _)| i)
                .collect();
            let [one] = found.as_slice() else {
                panic!(
                    "{}: turn {} names commit {prefix:?}, which resolves to {} commits",
                    raw.history,
                    t.id,
                    found.len()
                );
            };
            members.push(*one);
        }
        turns.push(Turn {
            id: t.id,
            commits: members,
        });
    }
    History {
        id: raw.history,
        provenance: raw.provenance,
        commits,
        turns,
    }
}

fn load_histories(dir: &Path) -> BTreeMap<String, History> {
    let mut out = BTreeMap::new();
    for entry in files_in(dir) {
        let history = load_history(&entry);
        out.insert(history.id.clone(), history);
    }
    assert!(
        !out.is_empty(),
        "no turn histories under {}; the spike cannot measure an empty corpus",
        dir.display()
    );
    out
}

fn files_in(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: cannot read the corpus directory: {e}", dir.display()))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    files
}

// ---------------------------------------------------------------------------------------------
// the relink corpus: hand-authored rewrites and hand-authored truth
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Manifest {
    file: String,
    events: Vec<EventSpec>,
}

#[derive(Debug, Deserialize)]
struct EventSpec {
    id: String,
    family: String,
    #[serde(default)]
    relation: String,
    history: String,
    #[serde(default)]
    expand: Option<String>,
    op: Op,
    #[serde(default)]
    decisions: Decisions,
    #[serde(default)]
    expect: Vec<Expect>,
    #[serde(default)]
    arrive_unread: Vec<String>,
    /// The rule form of `expect`, for rewrites that are uniform over a history (§4 R8): the human states the
    /// claim once and the enumeration is mechanical. Which turns the rewrite reworded and which it dropped is
    /// what the rewrite itself reports, never what the tiers would have matched.
    #[serde(default)]
    expect_rule: Option<ExpectRule>,
}

#[derive(Debug, Deserialize)]
struct ExpectRule {
    #[serde(default)]
    reworded: Option<RuleArm>,
    #[serde(default)]
    dropped: Option<RuleArm>,
}

#[derive(Debug, Deserialize)]
struct RuleArm {
    /// `Some("self")` is a carry owed to the after-image episode of the same turn; `None` is silence.
    #[serde(default)]
    to: Option<String>,
    #[serde(default)]
    catch: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Decisions {
    // The string is read for its shape, not its content: `"all"` is the only spelling the corpus uses, and
    // the deserialiser's job is to tell it apart from the `{only: […]}` form.
    #[allow(dead_code)]
    All(String),
    Only {
        only: Vec<String>,
        #[serde(default)]
        state: Option<String>,
    },
}

impl Default for Decisions {
    fn default() -> Self {
        Self::All("all".to_owned())
    }
}

#[derive(Debug, Clone, Deserialize)]
struct Expect {
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    to: Option<String>,
    #[serde(default)]
    catch: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Targets {
    /// As above: `"all"` is distinguished from a list, and its content is the same for every history.
    #[allow(dead_code)]
    All(String),
    List(Vec<String>),
}

#[derive(Debug, Deserialize)]
struct Op {
    kind: String,
    #[serde(default)]
    targets: Option<Targets>,
    #[serde(default)]
    targets_nth: Vec<usize>,
    #[serde(default)]
    drop: Option<Targets>,
    #[serde(default)]
    suffix: Option<String>,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    prefix: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    at: Option<usize>,
    #[serde(default)]
    reorder: Option<String>,
    #[serde(default)]
    reword_nth: Vec<usize>,
    #[serde(default)]
    drop_nth: Vec<usize>,
    /// Reword every `n`th turn of the history, counting from 1. A rule the corpus states once instead of
    /// thirty-two times, for the events whose whole point is that *many* episodes die at once.
    #[serde(default)]
    reword_every: Option<usize>,
}

impl Op {
    fn target_list(&self, history: &History, expanded: Option<&str>) -> Vec<String> {
        if let Some(turn) = expanded {
            return vec![turn.to_owned()];
        }
        let mut out = Vec::new();
        match &self.targets {
            Some(Targets::All(_)) => out.extend(history.turns.iter().map(|t| t.id.clone())),
            Some(Targets::List(list)) => out.extend(list.iter().cloned()),
            None => {}
        }
        for nth in &self.targets_nth {
            if let Some(turn) = history.turns.get(nth.saturating_sub(1)) {
                out.push(turn.id.clone());
            }
        }
        out
    }

    fn nth(&self, history: &History, nth: usize) -> Option<String> {
        history
            .turns
            .get(nth.saturating_sub(1))
            .map(|t| t.id.clone())
    }

    fn template(&self, value: &str, turn: &str) -> String {
        value.replace("<turn>", turn)
    }
}

/// One episode in a rewrite's before or after image, with everything a tier looks at.
#[derive(Debug, Clone)]
struct Ep {
    /// The annotation's key for it: a turn id, or `a+b`, or `t#1`, or `t~backport`.
    key: String,
    ordinal: usize,
    members: Vec<String>,
    fingerprint: String,
    fallback: String,
    shape: String,
    author: String,
    subject_tokens: BTreeSet<String>,
    path_set: BTreeSet<String>,
    first_utc: i64,
    last_utc: i64,
    id: String,
}

impl Ep {
    fn jaccard(&self, other: &Ep, metric: Metric) -> f64 {
        let (a, b) = match metric {
            Metric::Subject => (&self.subject_tokens, &other.subject_tokens),
            Metric::Paths => (&self.path_set, &other.path_set),
            Metric::Combined => {
                return 0.5 * self.jaccard(other, Metric::Subject)
                    + 0.5 * self.jaccard(other, Metric::Paths);
            }
        };
        let both = a.intersection(b).count();
        let either = a.union(b).count();
        if either == 0 {
            0.0
        } else {
            both as f64 / either as f64
        }
    }
}

/// One fully-resolved rewrite: which episodes died, which are live, and where the human says each verdict
/// belongs.
struct Instance {
    id: String,
    family: String,
    /// ADR-003 §6.4's `relation`, as the corpus declared it — printed by `--explain`, because a rewrite
    /// whose relation is not the one its family claims is a corpus bug hiding in a measurement.
    relation: String,
    /// The turn history this rewrite was laid on. Also `--explain`-only, and kept for the same reason.
    history: String,
    provenance: String,
    before: Vec<Ep>,
    after: Vec<Ep>,
    /// The commits of each image, for the store pin, which has to write commit rows before episode rows.
    before_commits: Vec<Commit>,
    after_commits: Vec<Commit>,
    /// `(dead key, landing key or None)` — the hand-authored truth.
    truth: Vec<(String, Option<String>)>,
    /// The annotation's claim about which tier can reach a carry, keyed by `(from, to)`.
    catch: BTreeMap<(String, Option<String>), String>,
    arrive_unread: Vec<String>,
    /// Which before episodes carry a verdict, as the corpus declares it.
    decided: BTreeSet<String>,
    /// The verdict each dispositioned episode carries, for the cost columns.
    states: BTreeMap<String, String>,
    /// Did the rewrite rename a twin's id without moving its digest (§6.2's blind spot, not a relink)?
    twin_id_churn: bool,
    /// How many annotation lines turned out to owe nothing because the digest survived untouched —
    /// the rewrite moved history without moving identity, which is the outcome §2.1 was built for.
    identity_survived: usize,
}

/// The unit a rewrite works on: one episode of the after image, before digests are taken.
struct Unit {
    key: String,
    commits: Vec<Commit>,
}

/// What a rewrite touched, as the rewrite itself reports it. The corpus's rule-form annotations (§4 R8)
/// are expanded against these two sets, which is why the op has to say what it did rather than leaving the
/// harness to infer it from the after image.
#[derive(Default)]
struct Rewritten {
    /// Turns whose *message* the rewrite rewrote (and so whose strong and fallback digests moved).
    reworded: BTreeSet<String>,
    /// Turns whose commits are gone from the lineage.
    dropped: BTreeSet<String>,
}

/// Apply one declared rewrite to one history: the after image's episodes, in walk order.
fn apply_op(history: &History, op: &Op, targets: &[String]) -> (Vec<Unit>, Rewritten) {
    let target_set: BTreeSet<String> = targets.iter().cloned().collect();
    let mut report = Rewritten::default();
    let mut dropped: BTreeSet<String> = match &op.drop {
        Some(Targets::All(_)) => history.turns.iter().map(|t| t.id.clone()).collect(),
        Some(Targets::List(list)) => list.iter().cloned().collect(),
        None => BTreeSet::new(),
    };
    match op.kind.as_str() {
        "drop" => dropped.extend(targets.iter().cloned()),
        "rebase+reword+drop" => {
            dropped.extend(op.drop_nth.iter().filter_map(|n| op.nth(history, *n)))
        }
        _ => {}
    }
    // Turns whose commits get a fresh oid (and so, possibly, a fresh subject, path set or author).
    let mut rewritten: BTreeSet<String> = BTreeSet::new();
    match op.kind.as_str() {
        "rewrite" | "reword" | "amend-add-path" | "amend-move-paths" | "reword-and-move"
        | "reset-author" | "reword+drop" => rewritten.extend(targets.iter().cloned()),
        "rebase+reword+drop" => {
            rewritten.extend(history.turns.iter().map(|t| t.id.clone()));
        }
        _ => {}
    }
    // What the rewrite reports having done, for the corpus's rule-form annotations (§4 R8).
    report.dropped = dropped.clone();
    if op.kind == "drop" {
        report.dropped.extend(targets.iter().cloned());
    }
    if let Some(every) = op.reword_every {
        for (i, turn) in history.turns.iter().enumerate() {
            if (i + 1) % every == 1 {
                report.reworded.insert(turn.id.clone());
            }
        }
    }
    match op.kind.as_str() {
        "reword" | "reword+drop" | "reword-and-move" => {
            report.reworded.extend(target_set.iter().cloned());
        }
        "rebase+reword+drop" => {
            report
                .reworded
                .extend(op.reword_nth.iter().filter_map(|n| op.nth(history, *n)));
        }
        _ => {}
    }
    let reworded: BTreeMap<String, String> = match op.kind.as_str() {
        "reword" | "reword+drop" => target_set
            .iter()
            .map(|t| {
                (
                    t.clone(),
                    match &op.subject {
                        Some(subject) => subject.clone(),
                        None => format!(
                            "{}{}",
                            subject_of(history, t.as_str()),
                            op.suffix.clone().unwrap_or_default()
                        ),
                    },
                )
            })
            .collect(),
        "reword-and-move" => target_set
            .iter()
            .map(|t| {
                (
                    t.clone(),
                    format!(
                        "{}{}",
                        subject_of(history, t.as_str()),
                        op.suffix.clone().unwrap_or_default()
                    ),
                )
            })
            .collect(),
        "rebase+reword+drop" => op
            .reword_nth
            .iter()
            .filter_map(|n| op.nth(history, *n))
            .chain(report.reworded.iter().cloned())
            .map(|t| {
                (
                    t.clone(),
                    format!(
                        "{}{}",
                        subject_of(history, t.as_str()),
                        op.suffix.clone().unwrap_or_default()
                    ),
                )
            })
            .collect(),
        _ => BTreeMap::new(),
    };

    let order: Vec<usize> = if op.reorder.as_deref() == Some("swap-adjacent") {
        let mut o = Vec::new();
        let mut i = 0;
        while i + 1 < history.turns.len() {
            o.push(i + 1);
            o.push(i);
            i += 2;
        }
        o.extend(i..history.turns.len());
        o
    } else {
        (0..history.turns.len()).collect()
    };

    let mut units: Vec<Unit> = Vec::new();
    let mut shift = 0_i64;
    for &ti in &order {
        let turn = &history.turns[ti];
        if dropped.contains(&turn.id) {
            continue;
        }
        let fresh = rewritten.contains(&turn.id);
        let mut commits = Vec::new();
        for (n, &ci) in turn.commits.iter().enumerate() {
            let base = &history.commits[ci];
            shift += 60_000;
            let mut commit = base.clone();
            if fresh {
                commit.oid = format!("{}-rew{n}", base.oid);
                commit.millis = base.millis + 86_400_000 + shift;
                if let Some(subject) = reworded.get(&turn.id).cloned() {
                    commit.subject = subject;
                }
                match op.kind.as_str() {
                    "amend-add-path" => {
                        let path = op.template(&op.path.clone().unwrap_or_default(), &turn.id);
                        if !commit.paths.contains(&path) {
                            commit.paths.push(path);
                        }
                    }
                    "amend-move-paths" | "reword-and-move" => {
                        commit.paths =
                            vec![op.template(&op.path.clone().unwrap_or_default(), &turn.id)];
                    }
                    "reset-author" => {
                        commit.name = op.name.clone().unwrap_or_default();
                        commit.email = op.email.clone().unwrap_or_default();
                    }
                    _ => {}
                }
            }
            commits.push(commit);
        }
        units.push(Unit {
            key: turn.id.clone(),
            commits,
        });
    }

    if op.kind == "squash" && targets.len() == 2 {
        units = squash(units, history, &targets[0], &targets[1], op);
    }
    if op.kind == "regroup" && targets.len() == 2 {
        units = regroup(units, &targets[0], &targets[1]);
    }
    if op.kind == "split" {
        let at = op.at.unwrap_or(1);
        let mut out = Vec::new();
        for unit in units {
            let n = unit.commits.len();
            if target_set.contains(&unit.key) && n > 1 {
                let at = at.clamp(1, n - 1);
                out.push(Unit {
                    key: format!("{}#1", unit.key),
                    commits: unit.commits[..at].to_vec(),
                });
                out.push(Unit {
                    key: format!("{}#2", unit.key),
                    commits: unit.commits[at..].to_vec(),
                });
            } else {
                out.push(unit);
            }
        }
        units = out;
    }
    if op.kind == "split-commit" && target_set.len() == 1 {
        let at = op.at.unwrap_or(1);
        let mut out = Vec::new();
        for unit in units {
            if unit.key != targets[0] || unit.commits.is_empty() {
                out.push(unit);
                continue;
            }
            let head = unit.commits[0].clone();
            let rest = unit.commits[1..].to_vec();
            let at = at.min(head.paths.len().saturating_sub(1));
            let mut first = head.clone();
            first.oid = format!("{}-half-a", head.oid);
            first.paths = head.paths[..at].to_vec();
            let mut second = head.clone();
            second.oid = format!("{}-half-b", head.oid);
            second.subject = format!("{} (second half)", head.subject);
            second.paths = head.paths[at..].to_vec();
            second.millis += 1;
            let mut tail = vec![second];
            tail.extend(rest);
            out.push(Unit {
                key: format!("{}#1", unit.key),
                commits: vec![first],
            });
            out.push(Unit {
                key: format!("{}#2", unit.key),
                commits: tail,
            });
        }
        units = out;
    }
    for (kind, suffix) in [("twin", "~twin"), ("backport", "~backport")] {
        if op.kind != kind {
            continue;
        }
        for t in targets {
            let Some(unit) = units.iter().find(|u| u.key == *t) else {
                continue;
            };
            let Some(head) = unit.commits.first() else {
                continue;
            };
            let mut added = head.clone();
            added.oid = format!("{}{kind}", head.oid);
            added.millis += 3_600_000;
            if kind == "backport" {
                added.subject =
                    format!("{}{}", op.prefix.clone().unwrap_or_default(), head.subject);
            }
            units.push(Unit {
                key: format!("{t}{suffix}"),
                commits: vec![added],
            });
        }
    }
    units.retain(|u| !u.commits.is_empty());
    report.reworded.retain(|t| {
        units.iter().any(|u| {
            u.key == *t
                && u.commits
                    .first()
                    .is_some_and(|c| c.subject != subject_of(history, t))
        })
    });
    (units, report)
}

fn subject_of(history: &History, turn: &str) -> String {
    history
        .turns
        .iter()
        .find(|t| t.id == turn)
        .and_then(|t| t.commits.first())
        .map(|c| history.commits[*c].subject.clone())
        .unwrap_or_default()
}

/// Two units collapse into one new commit whose path set is the union of the two originals' first members.
fn squash(units: Vec<Unit>, history: &History, a: &str, b: &str, op: &Op) -> Vec<Unit> {
    let Some(ai) = units.iter().position(|u| u.key == a) else {
        return units;
    };
    let Some(bi) = units.iter().position(|u| u.key == b) else {
        return units;
    };
    let first_a = units[ai].commits[0].clone();
    let first_b = units[bi].commits[0].clone();
    let keep_subject = match op.subject.as_deref() {
        None | Some("keep-first") => first_a.subject.clone(),
        Some(subject) => subject.to_owned(),
    };
    let mut paths: BTreeSet<String> = BTreeSet::new();
    paths.extend(
        history.commits[history
            .turns
            .iter()
            .position(|t| t.id == a)
            .map(|i| history.turns[i].commits[0])
            .unwrap_or(0)]
        .paths
        .iter()
        .cloned(),
    );
    paths.extend(first_b.paths.iter().cloned());
    let merged = Commit {
        oid: format!("{}-squashed", first_a.oid),
        name: first_a.name.clone(),
        email: first_a.email.clone(),
        subject: keep_subject,
        paths: paths.into_iter().collect(),
        millis: first_a.millis + 1,
    };
    let mut out = Vec::new();
    for (i, unit) in units.into_iter().enumerate() {
        if i == ai {
            out.push(Unit {
                key: format!("{a}+{b}"),
                commits: vec![merged.clone()],
            });
        } else if i != bi {
            out.push(unit);
        }
    }
    out
}

/// Two units become one episode over the *same* commits: a `force_merge`, or a rule change that merges.
fn regroup(units: Vec<Unit>, a: &str, b: &str) -> Vec<Unit> {
    let Some(ai) = units.iter().position(|u| u.key == a) else {
        return units;
    };
    let Some(bi) = units.iter().position(|u| u.key == b) else {
        return units;
    };
    let mut merged = units[ai].commits.clone();
    merged.extend(units[bi].commits.clone());
    let mut out = Vec::new();
    for (i, unit) in units.into_iter().enumerate() {
        if i == ai {
            out.push(Unit {
                key: format!("{a}+{b}"),
                commits: merged.clone(),
            });
        } else if i != bi {
            out.push(unit);
        }
    }
    out
}

/// Digest an episode's first member and give every episode in walk order its id, applying TB10.
fn build_eps(units: &[Unit]) -> Vec<Ep> {
    let mut eps: Vec<Ep> = Vec::new();
    for (ordinal, unit) in units.iter().enumerate() {
        let Some(first) = unit.commits.first() else {
            continue;
        };
        let digest = segment::identity(&first.author_identity(), &first.subject, &first.paths);
        let mut path_set = BTreeSet::new();
        path_set.extend(first.paths.iter().cloned());
        eps.push(Ep {
            key: unit.key.clone(),
            ordinal,
            members: unit.commits.iter().map(|c| c.oid.clone()).collect(),
            fingerprint: digest.fingerprint,
            fallback: digest.fallback_fingerprint,
            shape: digest.shape_fingerprint,
            author: first.author_identity(),
            subject_tokens: first.tokens(),
            path_set,
            first_utc: first.millis,
            last_utc: unit.commits.iter().map(|c| c.millis).max().unwrap_or(0),
            id: String::new(),
        });
    }
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for ep in &mut eps {
        let n = seen.entry(ep.fingerprint.clone()).or_insert(0);
        ep.id = if *n == 0 {
            segment::episode_id(&ep.fingerprint)
        } else {
            format!("{}-{}", segment::episode_id(&ep.fingerprint), *n + 1)
        };
        *n += 1;
    }
    eps
}

// ---------------------------------------------------------------------------------------------
// building instances: expanding the corpus, resolving the truth
// ---------------------------------------------------------------------------------------------

/// How much of the corpus a reviewer has dispositioned. Both boards are scored, always, because RL0 means
/// the answer to "how many false relinks?" is a function of this and not of the tiers.
#[derive(Debug, Clone, Copy)]
struct Board {
    name: &'static str,
    /// What it means for an episode to carry a verdict on this board.
    thin: bool,
    note: &'static str,
}

const BOARDS: [Board; 2] = [
    Board {
        name: "signed-off",
        thin: false,
        note: "every episode the corpus dispositioned carries a verdict: the worst case for what a wrong link can touch",
    },
    Board {
        name: "sparse",
        thin: true,
        note: "every third episode carries a verdict, which is closer to a real queue and is what RL0 multiplies everything by",
    },
];

fn verdict_carries(key: &str, decided: &BTreeSet<String>, ordinal: usize, board: Board) -> bool {
    decided.contains(key) && (!board.thin || ordinal.is_multiple_of(3))
}

/// The state a dispositioned episode carries. A quarter of the verdicts are refusals on purpose: a false
/// relink that moves a *refusal* is the one that silences a finding, and a corpus where every verdict is
/// `accepted` cannot measure that cost.
fn verdict_state(declared: Option<&str>, ordinal: usize) -> String {
    if let Some(state) = declared {
        return state.to_owned();
    }
    if ordinal.is_multiple_of(4) {
        "dismissed".to_owned()
    } else {
        "accepted".to_owned()
    }
}

/// Resolve an annotation key, which may name the event's target turn or an ordinal into the history's turn
/// list. `None` is the honest answer for an ordinal past the end of a short history: the annotation says
/// nothing about a turn that does not exist, and the event simply contributes nothing for it.
fn resolve_key(token: &str, history: &History, targets: &[String]) -> Option<String> {
    if token == "<target>" {
        return targets.first().cloned();
    }
    if let Some(n) = token.strip_prefix("nth:") {
        return n
            .parse::<usize>()
            .ok()
            .and_then(|n| history.turns.get(n.saturating_sub(1)))
            .map(|t| t.id.clone());
    }
    Some(token.to_owned())
}

fn build_instances(
    manifests: &[Manifest],
    histories: &BTreeMap<String, History>,
) -> (Vec<Instance>, Vec<String>) {
    let mut out = Vec::new();
    let mut problems = Vec::new();
    for manifest in manifests {
        for spec in &manifest.events {
            let history_ids: Vec<String> = if spec.history == "*" {
                histories.keys().cloned().collect()
            } else {
                if !histories.contains_key(&spec.history) {
                    problems.push(format!(
                        "{}: history {:?} is not in the turn corpus",
                        spec.id, spec.history
                    ));
                    continue;
                }
                vec![spec.history.clone()]
            };
            for hid in history_ids {
                let history = &histories[&hid];
                let expansions: Vec<Option<String>> = match spec.expand.as_deref() {
                    Some("each-turn") => history.turns.iter().map(|t| Some(t.id.clone())).collect(),
                    None => vec![None],
                    Some(other) => {
                        problems.push(format!("{}: unknown expand {other:?}", spec.id));
                        vec![None]
                    }
                };
                for expansion in expansions {
                    let targets = spec.op.target_list(history, expansion.as_deref());
                    let before = build_eps(
                        &history
                            .turns
                            .iter()
                            .map(|t| Unit {
                                key: t.id.clone(),
                                commits: t
                                    .commits
                                    .iter()
                                    .map(|i| history.commits[*i].clone())
                                    .collect(),
                            })
                            .collect::<Vec<Unit>>(),
                    );
                    let (after_units, did) = apply_op(history, &spec.op, &targets);
                    let after = build_eps(&after_units);
                    let before_commits: Vec<Commit> = history
                        .turns
                        .iter()
                        .flat_map(|t| t.commits.iter().map(|i| history.commits[*i].clone()))
                        .collect();
                    let after_commits: Vec<Commit> =
                        after_units.iter().flat_map(|u| u.commits.clone()).collect();
                    let declared_state = match &spec.decisions {
                        Decisions::Only { state, .. } => state.as_deref(),
                        Decisions::All(_) => None,
                    };
                    let decided: BTreeSet<String> = match &spec.decisions {
                        Decisions::All(_) => before.iter().map(|e| e.key.clone()).collect(),
                        Decisions::Only { only, .. } => only.iter().cloned().collect(),
                    };
                    let states: BTreeMap<String, String> = before
                        .iter()
                        .filter(|e| decided.contains(&e.key))
                        .map(|e| (e.key.clone(), verdict_state(declared_state, e.ordinal + 1)))
                        .collect();
                    let instance_id = match &expansion {
                        Some(turn) => format!("{}/{}#{turn}", spec.id, spec.history),
                        None => format!("{}/{}", spec.id, spec.history),
                    };
                    let after_ids: BTreeSet<&str> = after.iter().map(|e| e.id.as_str()).collect();
                    let after_fingerprints: BTreeSet<&str> =
                        after.iter().map(|e| e.fingerprint.as_str()).collect();
                    // An episode's content is gone when no after-image episode carries its digest. That is
                    // the test, not "its id is gone": TB10 allocates twin suffixes in walk order, so
                    // rewriting one of a twin pair renames the *other* one (`ep-x-2` becomes `ep-x`)
                    // without anything about its content moving. That rename is real and it moves a review
                    // row between twins with no relink involved at all; it is counted in `twin_id_churn`
                    // and annotated explicitly in the corpus, and it is not a relink event.
                    let dead: BTreeSet<String> = before
                        .iter()
                        .filter(|e| !after_fingerprints.contains(e.fingerprint.as_str()))
                        .map(|e| e.key.clone())
                        .collect();
                    let twin_id_churn = before.iter().any(|e| {
                        !after_ids.contains(e.id.as_str())
                            && after_fingerprints.contains(&e.fingerprint.as_str())
                    });
                    let after_keys: BTreeSet<&str> = after.iter().map(|e| e.key.as_str()).collect();
                    let mut truth = Vec::new();
                    let mut catch = BTreeMap::new();
                    let mut identity_survived = 0_usize;
                    for expect in &spec.expect {
                        let Some(from) = expect.from.as_deref() else {
                            continue;
                        };
                        let Some(from) = resolve_key(from, history, &targets) else {
                            continue;
                        };
                        let to = match expect.to.as_deref() {
                            Some(token) => match resolve_key(token, history, &targets) {
                                Some(landing) => Some(landing),
                                None => {
                                    problems.push(format!(
                                        "{instance_id}: expect names a landing that does not exist: {token}"
                                    ));
                                    continue;
                                }
                            },
                            None => None,
                        };
                        // The one legal way an expected donor is not dead: the rewrite hit one half of a
                        // twin pair, the digest survived on the other half, and so no relink is owed — the
                        // verdict moved (or did not) with the id, which is TB10's doing and §6.2's blind
                        // spot. Scored as a miss of its own kind, and labelled so it cannot hide in either
                        // of the other two miss categories.
                        let twin_id_move =
                            to.as_ref().is_some_and(|to| *to == from) && twin_id_churn;
                        let digest_survived = before
                            .iter()
                            .find(|e| e.key == from)
                            .is_some_and(|e| after.iter().any(|a| a.fingerprint == e.fingerprint));
                        if !dead.contains(&from) {
                            if !digest_survived {
                                problems.push(format!(
                                    "{instance_id}: expect names {from}, which the rewrite does not kill (before: {}, after: {})",
                                    before.iter().map(|e| e.key.as_str()).collect::<Vec<&str>>().join(","),
                                    after.iter().map(|e| e.key.as_str()).collect::<Vec<&str>>().join(",")
                                ));
                                continue;
                            }
                            // The episode did not die: some other after-image episode carries its digest —
                            // a twin kept it, or the rewrite happened to preserve all three facts it reads.
                            // Where the annotation asked for a carry onto itself that is a scored miss of the
                            // TB10 kind; anywhere else there is nothing owed and nothing to write.
                            if !twin_id_move {
                                identity_survived += 1;
                                continue;
                            }
                        }
                        if let Some(to) = to.as_ref().filter(|t| !after_keys.contains(t.as_str())) {
                            problems.push(format!(
                                "{}/{}: expect sends {from} to {to}, which the after image does not contain",
                                spec.id, spec.history
                            ));
                        }
                        let class = if twin_id_move {
                            "twin-id-churn".to_owned()
                        } else {
                            expect.catch.clone().unwrap_or_else(|| "none".to_owned())
                        };
                        catch.insert((from.clone(), to.clone()), class);
                        truth.push((from, to));
                    }
                    if let Some(rule) = &spec.expect_rule {
                        for key in &dead {
                            if truth.iter().any(|(from, _)| from == key) {
                                continue;
                            }
                            let reworded = did.reworded.contains(key) && !did.dropped.contains(key);
                            let arm = if reworded {
                                rule.reworded.as_ref()
                            } else if did.dropped.contains(key) {
                                rule.dropped.as_ref()
                            } else {
                                None
                            };
                            let Some(arm) = arm else {
                                problems.push(format!(
                                    "{instance_id}: the rewrite killed {key} and `expect_rule` has no arm for it"
                                ));
                                continue;
                            };
                            let to = arm.to.as_ref().map(|t| {
                                if t == "self" {
                                    key.clone()
                                } else {
                                    resolve_key(t, history, &targets).unwrap_or_default()
                                }
                            });
                            if let Some(to) =
                                to.as_ref().filter(|t| !after_keys.contains(t.as_str()))
                            {
                                problems.push(format!(
                                    "{instance_id}: expect_rule sends {key} to {to}, absent from the after image"
                                ));
                            }
                            catch.insert(
                                (key.clone(), to.clone()),
                                arm.catch.clone().unwrap_or_else(|| "none".to_owned()),
                            );
                            truth.push((key.clone(), to));
                        }
                    }
                    for key in &dead {
                        if !truth.iter().any(|(from, _)| from == key) {
                            problems.push(format!(
                                "{instance_id}: the rewrite kills {key} and the truth says nothing about it",
                            ));
                        }
                    }
                    for key in &spec.arrive_unread {
                        if !after_keys.contains(key.as_str()) {
                            problems.push(format!(
                                "{}/{}: arrive_unread names {key}, absent from the after image",
                                spec.id, spec.history
                            ));
                        }
                    }
                    out.push(Instance {
                        id: instance_id,
                        family: spec.family.clone(),
                        relation: spec.relation.clone(),
                        history: hid.clone(),
                        provenance: history.provenance.clone(),
                        before,
                        after,
                        before_commits,
                        after_commits,
                        truth,
                        catch,
                        arrive_unread: spec.arrive_unread.clone(),
                        decided,
                        states,
                        twin_id_churn,
                        identity_survived,
                    });
                }
            }
        }
    }
    (out, problems)
}

impl Instance {
    /// Did this episode carry a verdict on this board?
    fn carries(&self, ep: &Ep, board: Board) -> bool {
        verdict_carries(&ep.key, &self.decided, ep.ordinal + 1, board)
    }
}

// ---------------------------------------------------------------------------------------------
// the tiers, and the ladders that order them
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum Metric {
    Subject,
    Paths,
    Combined,
}

#[derive(Debug, Clone, Copy)]
enum Tier {
    /// §6.2 tier 1: shared `fallback_fingerprint`.
    Fingerprint,
    /// §6.2 tier 2: shared `shape_fingerprint`.
    Shape,
    /// §6.2 tier 3: a member commit in common.
    MemberSet,
    /// A candidate similarity rung: how similar, above what, author required, and whether a tie disqualifies.
    Fuzzy {
        metric: Metric,
        theta: f64,
        author: bool,
        unique: bool,
    },
    /// Recency alone, which is RL3 promoted from a tie-break to a rule.
    Recency { window_min: Option<i64> },
}

impl Tier {
    fn method(&self) -> String {
        match self {
            Self::Fingerprint => "fingerprint".to_owned(),
            Self::Shape => "shape".to_owned(),
            Self::MemberSet => "member-set".to_owned(),
            Self::Recency { .. } => "recency".to_owned(),
            Self::Fuzzy { metric, .. } => match metric {
                Metric::Subject => "fuzzy-subject".to_owned(),
                Metric::Paths => "fuzzy-paths".to_owned(),
                Metric::Combined => "fuzzy-combined".to_owned(),
            },
        }
    }
}

struct Ladder {
    id: &'static str,
    title: &'static str,
    rationale: &'static str,
    tiers: Vec<Tier>,
}

fn shipped_tiers() -> Vec<Tier> {
    vec![Tier::Fingerprint, Tier::Shape, Tier::MemberSet]
}

/// The shipped pass, then each of its tiers alone, then shipped plus a candidate rung at a grid of
/// thresholds and disciplines. Every candidate goes *after* the shipped three: RL1 says a stronger tier's
/// answer is never revised by a weaker one, so a fuzzy rung that fired first would not be measuring the
/// proposal §13.1 declined, it would be measuring a different and worse one.
fn ladders() -> Vec<Ladder> {
    let fuzzy = |id: &'static str,
                 title: &'static str,
                 metric: Metric,
                 theta: f64,
                 author: bool,
                 unique: bool| {
        Ladder {
            id,
            title,
            rationale: "shipped §6.2 ladder with the candidate rung last (RL1)",
            tiers: vec![
                Tier::Fingerprint,
                Tier::Shape,
                Tier::MemberSet,
                Tier::Fuzzy {
                    metric,
                    theta,
                    author,
                    unique,
                },
            ],
        }
    };
    vec![
        Ladder {
            id: "t1-only",
            title: "tier 1 alone (author + normalised subject)",
            rationale: "the census column: what the strongest digest could ever have known on its own",
            tiers: vec![Tier::Fingerprint],
        },
        Ladder {
            id: "t2-only",
            title: "tier 2 alone (author + path set)",
            rationale: "the census column",
            tiers: vec![Tier::Shape],
        },
        Ladder {
            id: "t3-only",
            title: "tier 3 alone (a member commit in common)",
            rationale: "the census column",
            tiers: vec![Tier::MemberSet],
        },
        Ladder {
            id: "shipped",
            title: "§6.2 as built: fingerprint › shape › member-set",
            rationale: "the incumbent, and the ladder pinned against `fathom-store`",
            tiers: shipped_tiers(),
        },
        Ladder {
            id: "shipped-no-member-set",
            title: "fingerprint › shape, with tier 3 removed",
            rationale: "what the member-set rung is worth, since its precondition is a surviving commit",
            tiers: vec![Tier::Fingerprint, Tier::Shape],
        },
        fuzzy(
            "fz-subject@0.30",
            "shipped + subject Jaccard ≥ 0.30",
            Metric::Subject,
            0.30,
            true,
            false,
        ),
        fuzzy(
            "fz-subject-unique@0.45",
            "shipped + subject Jaccard ≥ 0.45, only when unambiguous both ways",
            Metric::Subject,
            0.45,
            true,
            true,
        ),
        fuzzy(
            "fz-subject-anon@0.45",
            "shipped + subject Jaccard ≥ 0.45, author not required",
            Metric::Subject,
            0.45,
            false,
            false,
        ),
        fuzzy(
            "fz-subject-anon-unique@0.45",
            "shipped + subject Jaccard ≥ 0.45, author not required, only when unambiguous",
            Metric::Subject,
            0.45,
            false,
            true,
        ),
        fuzzy(
            "fz-combined-anon-unique@0.50",
            "shipped + half subject, half paths, author not required, only when unambiguous",
            Metric::Combined,
            0.50,
            false,
            true,
        ),
        fuzzy(
            "fz-subject@0.45",
            "shipped + subject Jaccard ≥ 0.45",
            Metric::Subject,
            0.45,
            true,
            false,
        ),
        fuzzy(
            "fz-subject@0.60",
            "shipped + subject Jaccard ≥ 0.60",
            Metric::Subject,
            0.60,
            true,
            false,
        ),
        fuzzy(
            "fz-subject@0.75",
            "shipped + subject Jaccard ≥ 0.75",
            Metric::Subject,
            0.75,
            true,
            false,
        ),
        fuzzy(
            "fz-paths@0.50",
            "shipped + path-set Jaccard ≥ 0.50",
            Metric::Paths,
            0.50,
            true,
            false,
        ),
        fuzzy(
            "fz-paths@0.70",
            "shipped + path-set Jaccard ≥ 0.70",
            Metric::Paths,
            0.70,
            true,
            false,
        ),
        fuzzy(
            "fz-paths@0.90",
            "shipped + path-set Jaccard ≥ 0.90",
            Metric::Paths,
            0.90,
            true,
            false,
        ),
        fuzzy(
            "fz-paths-anon@0.70",
            "shipped + path-set Jaccard ≥ 0.70, author NOT required",
            Metric::Paths,
            0.70,
            false,
            false,
        ),
        fuzzy(
            "fz-paths-unique@0.70",
            "shipped + path Jaccard ≥ 0.70 and only one candidate above it",
            Metric::Paths,
            0.70,
            true,
            true,
        ),
        fuzzy(
            "fz-combined@0.35",
            "shipped + ½·subject + ½·paths ≥ 0.35",
            Metric::Combined,
            0.35,
            true,
            false,
        ),
        fuzzy(
            "fz-combined@0.50",
            "shipped + ½·subject + ½·paths ≥ 0.50",
            Metric::Combined,
            0.50,
            true,
            false,
        ),
        fuzzy(
            "fz-combined@0.65",
            "shipped + ½·subject + ½·paths ≥ 0.65",
            Metric::Combined,
            0.65,
            true,
            false,
        ),
        fuzzy(
            "fz-combined-unique@0.50",
            "shipped + combined ≥ 0.50 with a unique best candidate",
            Metric::Combined,
            0.50,
            true,
            true,
        ),
        fuzzy(
            "fz-combined-anon@0.50",
            "shipped + combined ≥ 0.50, author NOT required",
            Metric::Combined,
            0.50,
            false,
            false,
        ),
        Ladder {
            id: "recency-any",
            title: "shipped + nearest dead episode, no window",
            rationale: "RL3's ordering promoted to a rule: the guess a relink pass makes if it trusts recency alone",
            tiers: vec![
                Tier::Fingerprint,
                Tier::Shape,
                Tier::MemberSet,
                Tier::Recency { window_min: None },
            ],
        },
        Ladder {
            id: "recency-30min",
            title: "shipped + nearest dead episode within 30 minutes",
            rationale: "the same guess, bounded by the published `episode.time_gap_minutes`",
            tiers: vec![
                Tier::Fingerprint,
                Tier::Shape,
                Tier::MemberSet,
                Tier::Recency {
                    window_min: Some(30),
                },
            ],
        },
    ]
}

#[derive(Debug, Clone)]
struct Link {
    from: String,
    to: String,
    method: String,
}

/// One pass of one ladder over one event: RL0's eligibility, RL1's order, RL2's one donor per landing,
/// RL3's closest-in-time tie-break, RL4's many landings per donor.
fn run_ladder(inst: &Instance, ladder: &Ladder, board: Board) -> Vec<Link> {
    let after_ids: BTreeSet<&str> = inst.after.iter().map(|e| e.id.as_str()).collect();
    let before_with_row: BTreeMap<&str, bool> = inst
        .before
        .iter()
        .map(|e| (e.id.as_str(), inst.carries(e, board)))
        .collect();
    let donors: Vec<&Ep> = inst
        .before
        .iter()
        .filter(|e| !after_ids.contains(e.id.as_str()) && inst.carries(e, board))
        .collect();
    let targets: Vec<&Ep> = {
        let mut t: Vec<&Ep> = inst
            .after
            .iter()
            .filter(|e| !before_with_row.get(e.id.as_str()).copied().unwrap_or(false))
            .collect();
        t.sort_by(|a, b| a.id.cmp(&b.id));
        t
    };
    let mut claimed: BTreeSet<String> = BTreeSet::new();
    let mut links = Vec::new();
    for tier in &ladder.tiers {
        let mut per_target: Vec<(&Ep, Vec<(&Ep, f64)>)> = Vec::new();
        for target in &targets {
            if claimed.contains(&target.id) {
                continue;
            }
            let mut scored: Vec<(&Ep, f64)> = donors
                .iter()
                .copied()
                .filter(|d| d.id != target.id)
                .filter_map(|d| match *tier {
                    Tier::Fingerprint => {
                        if !d.fallback.is_empty() && d.fallback == target.fallback {
                            Some((d, 0.9))
                        } else {
                            None
                        }
                    }
                    Tier::Shape => {
                        if !d.shape.is_empty() && d.shape == target.shape {
                            Some((d, 0.7))
                        } else {
                            None
                        }
                    }
                    Tier::MemberSet => {
                        let shared = d.members.iter().any(|m| target.members.contains(m));
                        if shared { Some((d, 0.6)) } else { None }
                    }
                    Tier::Fuzzy {
                        metric,
                        theta,
                        author,
                        unique: _,
                    } => {
                        if author && d.author != target.author {
                            return None;
                        }
                        let sim = target.jaccard(d, metric);
                        if sim >= theta { Some((d, sim)) } else { None }
                    }
                    Tier::Recency { window_min } => {
                        let gap = target.first_utc - d.last_utc;
                        if gap < 0 {
                            return None;
                        }
                        if window_min.is_some_and(|w| gap > w * 60_000) {
                            return None;
                        }
                        Some((d, 0.4))
                    }
                })
                .collect();
            // RL3: closest in time, then oldest-created, then id for a deterministic tie the store's SQL
            // would leave to the query planner. A spike that could not reproduce a store tie is a spike
            // with a nondeterministic report, so the tie is broken explicitly and the pin says so.
            scored.sort_by(|a, b| {
                b.0.last_utc
                    .cmp(&a.0.last_utc)
                    .then(b.0.first_utc.cmp(&a.0.first_utc))
                    .then(a.0.key.cmp(&b.0.key))
            });
            per_target.push((target, scored));
        }
        // The uniqueness discipline, both ways: a guess is allowed only where one dead episode is the only
        // candidate for one live episode *and* that live episode is the only place its verdict could go.
        // One-sided uniqueness — the target has one candidate — is not the discipline it appears to be: on a
        // mostly-undecided board every undecided episode has the same single candidate, and the rung stamps
        // one verdict onto all of them, which RL4 then permits.
        let donor_uses: BTreeMap<&str, usize> =
            per_target
                .iter()
                .fold(BTreeMap::new(), |mut acc, (_, scored)| {
                    for (donor, _) in scored {
                        *acc.entry(donor.key.as_str()).or_insert(0) += 1;
                    }
                    acc
                });
        let unique = matches!(tier, Tier::Fuzzy { unique: true, .. });
        for (target, scored) in per_target {
            let Some((donor, _)) = scored.first() else {
                continue;
            };
            if unique && (scored.len() != 1 || donor_uses.get(donor.key.as_str()) != Some(&1)) {
                continue;
            }
            if !claimed.insert(target.id.clone()) {
                continue;
            }
            links.push(Link {
                from: donor.key.clone(),
                to: target.key.clone(),
                method: tier.method(),
            });
        }
    }
    links
}

// ---------------------------------------------------------------------------------------------
// scoring
// ---------------------------------------------------------------------------------------------

/// One cell of the report. Every count is an integer; the rates are derived at print time so a reader can
/// re-derive them from the counts.
#[derive(Default, Clone, Debug)]
struct Tally {
    /// Carries the annotation says are owed.
    owed: usize,
    /// Owed carries the ladder wrote.
    caught: usize,
    /// Owed carries the ladder did not write.
    missed: usize,
    /// …of which the annotation itself says no shipped tier could have reached them (§6.3's floor).
    floor: usize,
    /// …of which there was no verdict to carry on this board: the annotation owes a carry, but nobody had
    /// dispositioned the episode, so there was no review row for any tier to move. Not a tier failure, and
    /// not a free pass either: it is the share of the corpus the relink pass never gets to act on.
    not_carriable: usize,
    /// …of which the blocker was TB10: the digest survived on a twin, so the id moved without a donor row
    /// to carry and §6.2 has nothing to relink from.
    twin_missed: usize,
    /// …of which a shipped tier had the evidence and missed it anyway.
    defect: usize,
    /// Annotations that forbid a dead episode's verdict from going anywhere, and were violated anyway.
    silent: usize,
    /// Relink rows the ladder wrote.
    written: usize,
    /// Rows written that the annotation does not authorise.
    wrong: usize,
    /// Owed carries crowded out by a different donor taking the same landing (RL2).
    crowded: usize,
    /// False relinks that were a *second* landing of the same donor's verdict: one dead episode's sign-off
    /// copied onto more than one live episode, which RL4 allows and a similarity rung causes in bulk.
    spray: usize,
    /// False relinks by the verdict they moved.
    wrong_accepted: usize,
    wrong_dismissed: usize,
    wrong_other: usize,
    /// False relinks that landed on an episode the annotation says must come back unread.
    unread_violations: usize,
    /// Events where TB10 renamed a twin's id while its digest survived: a verdict can move between twins
    /// with no relink row anywhere, which is why this is counted and not scored.
    twin_id_churn: usize,
    /// Events where the rewrite preserved every fact identity reads, so nothing detached and nothing was owed.
    identity_survived: usize,
    /// Ladder rows per method, for the census.
    by_method: BTreeMap<String, usize>,
}

impl Tally {
    fn absorb(&mut self, other: &Tally) {
        self.owed += other.owed;
        self.caught += other.caught;
        self.missed += other.missed;
        self.floor += other.floor;
        self.twin_missed += other.twin_missed;
        self.not_carriable += other.not_carriable;
        self.defect += other.defect;
        self.silent += other.silent;
        self.written += other.written;
        self.wrong += other.wrong;
        self.crowded += other.crowded;
        self.wrong_accepted += other.wrong_accepted;
        self.wrong_dismissed += other.wrong_dismissed;
        self.wrong_other += other.wrong_other;
        self.unread_violations += other.unread_violations;
        self.spray += other.spray;
        self.twin_id_churn += other.twin_id_churn;
        self.identity_survived += other.identity_survived;
        for (k, v) in &other.by_method {
            *self.by_method.entry(k.clone()).or_insert(0) += v;
        }
    }

    fn json(&self) -> Value {
        let rate = |a: usize, b: usize| -> Value {
            if b == 0 {
                Value::Null
            } else {
                json!((a as f64 / b as f64 * 1000.0).round() / 1000.0)
            }
        };
        json!({
            "owed": self.owed,
            "caught": self.caught,
            "owed_with_a_verdict_behind_it": self.owed.saturating_sub(self.not_carriable),
            "owed_but_nobody_had_decided": self.not_carriable,
            "missed": self.missed,
            "missed_where_no_tier_could": self.floor,
            "missed_where_a_twin_kept_the_digest": self.twin_missed,
            "missed_where_a_tier_had_the_evidence": self.defect,
            "must_stay_silent": self.silent,
            "written": self.written,
            "false_relinks": self.wrong,
            "crowded_out": self.crowded,
            "false_carrying_accepted": self.wrong_accepted,
            "false_carrying_dismissed": self.wrong_dismissed,
            "false_carrying_other": self.wrong_other,
            "false_on_must_arrive_unread": self.unread_violations,
            "false_from_a_donor_that_landed_more_than_once": self.spray,
            "twin_id_churn_events": self.twin_id_churn,
            "identity_survived_annotations": self.identity_survived,
            "missed_rate": rate(self.missed, self.owed.saturating_sub(self.not_carriable)),
            "false_rate_of_written": rate(self.wrong, self.written),
            "catch_rate": rate(self.caught, self.owed),
            "by_method": self.by_method,
        })
    }

    fn get(&self, metric: &str) -> Option<f64> {
        let rate = |a: usize, b: usize| -> Option<f64> {
            if b == 0 {
                None
            } else {
                Some(a as f64 / b as f64)
            }
        };
        match metric {
            "owed" => Some(self.owed as f64),
            "owed_decided" => Some((self.owed.saturating_sub(self.not_carriable)) as f64),
            "not_carriable" => Some(self.not_carriable as f64),
            "caught" => Some(self.caught as f64),
            "missed" => Some(self.missed as f64),
            "floor" => Some(self.floor as f64),
            "twin_missed" => Some(self.twin_missed as f64),
            "defect" => Some(self.defect as f64),
            "silent" => Some(self.silent as f64),
            "written" => Some(self.written as f64),
            "false" => Some(self.wrong as f64),
            "crowded" => Some(self.crowded as f64),
            "false_accepted" => Some(self.wrong_accepted as f64),
            "unread_violations" => Some(self.unread_violations as f64),
            "spray" => Some(self.spray as f64),
            "twin_id_churn" => Some(self.twin_id_churn as f64),
            "identity_survived" => Some(self.identity_survived as f64),
            "missed_rate" => rate(self.missed, self.owed.saturating_sub(self.not_carriable)),
            "missed_rate_of_all_owed" => rate(self.missed, self.owed),
            "false_rate" => rate(self.wrong, self.written),
            "spray_rate" => rate(self.spray, self.wrong),
            "catch_rate" => rate(self.caught, self.owed),
            _ => None,
        }
    }
}

fn score(inst: &Instance, links: &[Link], board: Board) -> Tally {
    let expected: BTreeSet<(String, String)> = inst
        .truth
        .iter()
        .filter_map(|(from, to)| to.as_ref().map(|to| (from.clone(), to.clone())))
        .collect();
    let emitted: BTreeSet<(String, String)> = links
        .iter()
        .map(|l| (l.from.clone(), l.to.clone()))
        .collect();
    let mut tally = Tally {
        owed: expected.len(),
        ..Tally::default()
    };
    // A `silent` annotation is a dead episode whose verdict must not end up anywhere. It is violated the
    // moment the ladder moves it, which is the same event as a false relink but named separately because
    // the annotation, not a judgement call, is what forbids it.
    tally.silent = inst
        .truth
        .iter()
        .filter(|(from, to)| to.is_none() && emitted.iter().any(|(other, _)| other == from))
        .count();
    tally.written = links.len();
    tally.caught = expected.intersection(&emitted).count();
    let row_exists = |from: &str| {
        inst.before
            .iter()
            .any(|e| e.key == from && inst.carries(e, board))
    };
    for (from, to) in expected.difference(&emitted) {
        if !row_exists(from) {
            tally.not_carriable += 1;
            continue;
        }
        tally.missed += 1;
        match inst
            .catch
            .get(&(from.clone(), Some(to.clone())))
            .map(String::as_str)
        {
            Some("twin-id-churn") => tally.twin_missed += 1,
            Some("none") | None => tally.floor += 1,
            _ => tally.defect += 1,
        }
        if emitted
            .iter()
            .any(|(other_from, other_to)| other_to == to && other_from != from)
        {
            tally.crowded += 1;
        }
    }
    let donor_landings: BTreeMap<&str, usize> = links.iter().fold(BTreeMap::new(), |mut acc, l| {
        *acc.entry(l.from.as_str()).or_insert(0) += 1;
        acc
    });
    for (from, to) in emitted.difference(&expected) {
        tally.wrong += 1;
        if donor_landings.get(from.as_str()).copied().unwrap_or(0) > 1
            && !inst
                .before
                .iter()
                .any(|e| e.key == *from && inst.after.iter().any(|a| a.key == *to && a.id == e.id))
        {
            tally.spray += 1;
        }
        match inst.states.get(from).map(String::as_str) {
            Some("accepted") => tally.wrong_accepted += 1,
            Some("dismissed") => tally.wrong_dismissed += 1,
            _ => tally.wrong_other += 1,
        }
        if inst.arrive_unread.iter().any(|k| k == to) {
            tally.unread_violations += 1;
        }
    }
    for link in links {
        *tally.by_method.entry(link.method.clone()).or_insert(0) += 1;
    }
    tally.twin_id_churn = usize::from(inst.twin_id_churn);
    tally.identity_survived = inst.identity_survived;
    let _ = board;
    tally
}

// ---------------------------------------------------------------------------------------------
// the pin: the spike's copy of the shipped pass against `fathom-store`'s real one
// ---------------------------------------------------------------------------------------------

fn pin_against_store(inst: &Instance, board: Board) -> Vec<String> {
    use fathom_store::episode::{EpisodeRow, ReviewState};
    use fathom_store::spine::{CommitRow, FileRow, Store, Trigger};

    const T0: i64 = 2_000_000_000_000;
    let mut store = match Store::open_in_memory() {
        Ok(store) => store,
        Err(e) => return vec![format!("cannot open an in-memory store: {e}")],
    };
    // `episode_member` has a real foreign key on `git_commit`, so the pin seeds the commits the episodes
    // are made of before it can pretend an index pass found the episodes.
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let commits: Vec<&Commit> = inst
        .before_commits
        .iter()
        .chain(inst.after_commits.iter())
        .filter(|c| seen.insert(c.oid.as_str()))
        .collect();
    let rows: Vec<CommitRow> = commits
        .iter()
        .enumerate()
        .map(|(i, c)| CommitRow {
            oid: c.oid.clone(),
            tree_oid: format!("{:064x}", i + 1),
            parent_oids: Vec::new(),
            author_name: c.name.clone(),
            author_email: c.email.clone(),
            author_utc: c.millis,
            committer_utc: c.millis,
            committer_name: c.name.clone(),
            committer_email: c.email.clone(),
            message_head: c.subject.clone(),
            message_hash: format!("{:064x}", i + 1),
            trailers: BTreeMap::new(),
            n_files: i64::try_from(c.paths.len()).unwrap_or(1),
            n_add: 10,
            n_del: 1,
            n_binary: 0,
            oversized: false,
            files: c
                .paths
                .iter()
                .enumerate()
                .map(|(j, path)| FileRow {
                    path_hash: format!("{:064x}", i * 100 + j + 1),
                    new_path: path.clone(),
                    old_path: None,
                    change_kind: "modify".to_owned(),
                    mode_old: Some(0o100_644),
                    mode_new: Some(0o100_644),
                    is_binary: false,
                    n_add: 10,
                    n_del: 1,
                    hunk_headers: Vec::new(),
                    blob_old: None,
                    blob_new: Some("e".repeat(40)),
                    path_class: "source".to_owned(),
                    taxonomy_version: 1,
                    noise_class: "informative".to_owned(),
                })
                .collect(),
        })
        .collect();
    let now = T0;
    let run = match store.begin_run("cold", Trigger::Manual, now) {
        Ok(run) => run,
        Err(e) => return vec![format!("beginning the run: {e}")],
    };
    if let Err(e) = store.write_commits(run, now, &rows) {
        return vec![format!("writing the commits: {e}")];
    }
    let tip = rows
        .last()
        .map(|r| r.oid.clone())
        .unwrap_or_else(|| "0".repeat(40));
    if let Err(e) = store.replace_ref_tips(
        &[
            ("HEAD".to_owned(), tip.clone(), "head"),
            ("refs/heads/main".to_owned(), tip, "branch"),
        ],
        now,
    ) {
        return vec![format!("recording the ref tips: {e}")];
    }
    let row = |ep: &Ep| EpisodeRow {
        id: ep.id.clone(),
        fingerprint: ep.fingerprint.clone(),
        fallback_fingerprint: ep.fallback.clone(),
        shape_fingerprint: ep.shape.clone(),
        kind: "committed".to_owned(),
        lineage_ref: "refs/heads/main".to_owned(),
        lineage_seq: i64::try_from(ep.ordinal).unwrap_or(0),
        created_utc: ep.first_utc,
        created_source: "index".to_owned(),
        created_confidence: 0.3,
        boundary_rule: "author".to_owned(),
        boundary_confidence: 0.75,
        author_kind: "agent".to_owned(),
        author_confidence: 0.85,
        author_name: ep.author.clone(),
        author_email: String::new(),
        first_oid: ep.members.first().cloned().unwrap_or_default(),
        last_oid: ep.members.last().cloned().unwrap_or_default(),
        members: ep.members.clone(),
        n_files: ep.path_set.len().max(1) as i64,
        n_add: 10,
        n_del: 1,
        n_binary: 0,
        first_utc: ep.first_utc,
        last_utc: ep.last_utc,
        summary: ep.key.clone(),
        live: true,
        ..EpisodeRow::default()
    };
    let before: Vec<EpisodeRow> = inst.before.iter().map(row).collect();
    if let Err(e) = store.write_episodes(now, &before) {
        return vec![format!("writing the before image: {e}")];
    }
    for ep in &inst.before {
        if inst.carries(ep, board) {
            let state = inst
                .states
                .get(&ep.key)
                .map(String::as_str)
                .unwrap_or("accepted");
            if let Err(e) =
                store.set_review(&ep.id, ReviewState::parse(state), "s6-spike", None, now)
            {
                return vec![format!("setting a review row: {e}")];
            }
        }
    }
    let after: Vec<EpisodeRow> = inst.after.iter().map(row).collect();
    if let Err(e) = store.write_episodes(now + 1000, &after) {
        return vec![format!("writing the after image: {e}")];
    }
    let theirs: BTreeSet<(String, String, String)> = match store.episode_rewrites() {
        Ok(rows) => rows
            .into_iter()
            .map(|(old, new, _relation, method)| (old, new, method))
            .collect(),
        Err(e) => return vec![format!("reading the rewrite rows: {e}")],
    };
    // The store keys rows by episode id; the spike scores by annotation key. Translate through the images.
    let before_key: BTreeMap<&str, &str> = inst
        .before
        .iter()
        .map(|e| (e.id.as_str(), e.key.as_str()))
        .collect();
    let after_key: BTreeMap<&str, &str> = inst
        .after
        .iter()
        .map(|e| (e.id.as_str(), e.key.as_str()))
        .collect();
    let theirs: BTreeSet<(String, String, String)> = theirs
        .into_iter()
        .filter_map(|(old, new, method)| {
            let from = before_key.get(old.as_str()).map(|k| (*k).to_owned())?;
            let to = after_key.get(new.as_str()).map(|k| (*k).to_owned())?;
            Some((from, to, method))
        })
        .collect();
    let shipped = ladders().into_iter().find(|l| l.id == "shipped").unwrap();
    let mine: BTreeSet<(String, String, String)> = run_ladder(inst, &shipped, board)
        .into_iter()
        .map(|l| (l.from, l.to, l.method))
        .collect();
    let mut problems = Vec::new();
    for link in theirs.difference(&mine) {
        problems.push(format!(
            "{}: the store wrote {}→{} ({}) and the spike did not",
            inst.id, link.0, link.1, link.2
        ));
    }
    for link in mine.difference(&theirs) {
        problems.push(format!(
            "{}: the spike wrote {}→{} ({}) and the store did not",
            inst.id, link.0, link.1, link.2
        ));
    }
    problems
}

// ---------------------------------------------------------------------------------------------
// the gates
// ---------------------------------------------------------------------------------------------

/// One claim from `crates/fathom-analyze/spike/s6/thresholds.json`, evaluated against the measured table.
///
/// A gate names a ladder, a board, a scope, optionally a family, and a metric, and says what it expects.
/// A gate that comes out the other way from its claim fails the run, whether it was expected to pass or to
/// fail: the declared failures are the spike's findings, and a finding that quietly became true is as much
/// a change in the world as one that became false.
fn evaluate_gates(
    path: &Path,
    index: &BTreeMap<(String, String, String, String), Tally>,
) -> Vec<Value> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| {
        panic!(
            "no thresholds file at {}; the spike's gates must be committed: {e}",
            path.display()
        )
    });
    let thresholds: Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let gates = thresholds["gates"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: no `gates` array", path.display()));
    let lookup =
        |ladder: &str, board: &str, scope: &str, family: &str, metric: &str| -> Option<f64> {
            index
                .get(&(
                    ladder.to_owned(),
                    board.to_owned(),
                    scope.to_owned(),
                    family.to_owned(),
                ))
                .and_then(|tally| tally.get(metric))
        };
    let mut rows = Vec::new();
    for gate in gates {
        let id = gate["id"].as_str().unwrap_or("?").to_owned();
        let ladder = gate["ladder"].as_str().unwrap_or("shipped");
        let board = gate["board"].as_str().unwrap_or("signed-off");
        let scope = gate["scope"].as_str().unwrap_or("pooled");
        let family = gate["family"].as_str().unwrap_or("*");
        let metric = gate["metric"].as_str().unwrap_or("caught");
        let measured = if gate["kind"].as_str() == Some("delta") {
            let other = gate["ladder_b"].as_str().unwrap_or("shipped");
            let a = lookup(ladder, board, scope, family, metric);
            let b = lookup(other, board, scope, family, metric);
            match (a, b) {
                (Some(a), Some(b)) => Some(a - b),
                _ => None,
            }
        } else {
            lookup(ladder, board, scope, family, metric)
        };
        let expected_pass = gate["expected"].as_str().unwrap_or("pass") != "fail";
        let (pass, detail) = match measured {
            None => (
                false,
                "no measurement for this cell — the denominator is empty".to_owned(),
            ),
            Some(value) => {
                let rounded = (value * 1000.0).round() / 1000.0;
                let mut ok = true;
                if let Some(min) = gate["min"].as_f64() {
                    ok &= rounded >= min;
                }
                if let Some(max) = gate["max"].as_f64() {
                    ok &= rounded <= max;
                }
                (ok, format!("{metric} = {rounded}"))
            }
        };
        rows.push(json!({
            "id": id,
            "claim": gate["claim"],
            "ladder": ladder,
            "board": board,
            "scope": if family == "*" { scope.to_owned() } else { format!("{scope} · {family}") },
            "metric": metric,
            "expected": if expected_pass { "pass" } else { "fail" },
            "measured": measured.map(|v| (v * 1000.0).round() / 1000.0),
            "detail": detail,
            "pass": pass,
            "as_expected": pass == expected_pass,
        }));
    }
    rows
}

// ---------------------------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------------------------

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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let check = args.iter().any(|a| a == "--check");
    let root = PathBuf::from(arg("--root").as_deref().unwrap_or("."));
    let turns_dir = PathBuf::from(
        arg("--corpus")
            .as_deref()
            .unwrap_or("crates/fathom-fixtures/corpus/turns"),
    )
    .let_relative(&root);
    let relink_dir = PathBuf::from(
        arg("--relink-corpus")
            .as_deref()
            .unwrap_or("crates/fathom-fixtures/corpus/relinks"),
    )
    .let_relative(&root);
    let report_path = PathBuf::from(
        arg("--report")
            .as_deref()
            .unwrap_or("benchmarks/reports/s6-relink-accuracy.json"),
    )
    .let_relative(&root);
    let thresholds_path = PathBuf::from(
        arg("--thresholds")
            .as_deref()
            .unwrap_or("crates/fathom-analyze/spike/s6/thresholds.json"),
    )
    .let_relative(&root);

    let histories = load_histories(&turns_dir);
    let mut manifests = Vec::new();
    for path in files_in(&relink_dir) {
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{}: unreadable relink corpus: {e}", path.display()));
        manifests.push(
            serde_json::from_str::<Manifest>(&text)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display())),
        );
    }
    let (instances, problems) = build_instances(&manifests, &histories);
    if !problems.is_empty() {
        eprintln!(
            "the relink corpus disagrees with itself in {} place(s); the spike refuses to score it",
            problems.len()
        );
        for problem in problems.iter().take(40) {
            eprintln!("  {problem}");
        }
        std::process::exit(1);
    }
    assert!(
        !instances.is_empty(),
        "the relink corpus produced no events: nothing to measure"
    );

    if let Some(needle) = arg("--explain") {
        let mut shown = 0;
        for inst in instances.iter().filter(|i| i.id.contains(&needle)).take(6) {
            println!(
                "── {} (relation {}, history {})",
                inst.id, inst.relation, inst.history
            );
            println!("   truth: {:?}", inst.truth);
            println!("   catch: {:?}", inst.catch);
            for board in BOARDS {
                let after_ids: BTreeSet<&str> = inst.after.iter().map(|e| e.id.as_str()).collect();
                println!(
                    "   board {}: donors={:?} targets={:?}",
                    board.name,
                    inst.before
                        .iter()
                        .filter(|e| !after_ids.contains(e.id.as_str()) && inst.carries(e, board))
                        .map(|e| e.key.clone())
                        .collect::<Vec<String>>(),
                    inst.after
                        .iter()
                        .filter(|e| !inst
                            .before
                            .iter()
                            .any(|b| b.id == e.id && inst.carries(b, board)))
                        .map(|e| e.key.clone())
                        .collect::<Vec<String>>()
                );
                for lid in ["shipped", "fz-subject@0.45", "recency-any"] {
                    let ladder = ladders().into_iter().find(|l| l.id == lid).unwrap();
                    let links = run_ladder(inst, &ladder, board);
                    let t = score(inst, &links, board);
                    println!(
                        "      {lid:16} links={:?} missed={} false={}",
                        links
                            .iter()
                            .map(|l| format!("{}→{}:{}", l.from, l.to, l.method))
                            .collect::<Vec<String>>(),
                        t.missed,
                        t.wrong
                    );
                }
            }
            shown += 1;
        }
        println!("({shown} instances explained)");
        return;
    }

    // The pin, on every instance and every board.
    let mut pin_problems: Vec<String> = Vec::new();
    // One board is enough to pin a pass whose only input on this axis is which review rows exist; the
    // boards differ by a subset relation, and the pin is already ~3k in-memory store transactions.
    for inst in &instances {
        pin_problems.extend(pin_against_store(inst, BOARDS[0]));
    }

    let scopes = ["pooled", "real", "fixture", "authored"];
    let ladder_list = ladders();
    let mut index: BTreeMap<(String, String, String, String), Tally> = BTreeMap::new();
    let mut per_ladder: Vec<Value> = Vec::new();
    for ladder in &ladder_list {
        let mut boards = serde_json::Map::new();
        for board in BOARDS {
            let mut scopes_map = serde_json::Map::new();
            let mut family_map: BTreeMap<String, Tally> = BTreeMap::new();
            let mut scope_totals: BTreeMap<String, Tally> = scopes
                .iter()
                .map(|s| ((*s).to_owned(), Tally::default()))
                .collect();
            for inst in &instances {
                let links = run_ladder(inst, ladder, board);
                let tally = score(inst, &links, board);
                for scope in ["pooled", inst.provenance.as_str()] {
                    if let Some(total) = scope_totals.get_mut(scope) {
                        total.absorb(&tally);
                    }
                }
                family_map
                    .entry(inst.family.clone())
                    .or_default()
                    .absorb(&tally);
                index.insert(
                    (
                        ladder.id.to_owned(),
                        board.name.to_owned(),
                        inst.provenance.clone(),
                        inst.family.clone(),
                    ),
                    tally.clone(),
                );
            }
            for (family, tally) in &family_map {
                index.insert(
                    (
                        ladder.id.to_owned(),
                        board.name.to_owned(),
                        "pooled".to_owned(),
                        family.clone(),
                    ),
                    tally.clone(),
                );
                index.insert(
                    (
                        ladder.id.to_owned(),
                        board.name.to_owned(),
                        "all-families".to_owned(),
                        family.clone(),
                    ),
                    tally.clone(),
                );
            }
            for (scope, tally) in &scope_totals {
                index.insert(
                    (
                        ladder.id.to_owned(),
                        board.name.to_owned(),
                        scope.clone(),
                        "*".to_owned(),
                    ),
                    tally.clone(),
                );
                scopes_map.insert(scope.clone(), tally.json());
            }
            boards.insert(
                board.name.to_owned(),
                json!({
                    "note": board.note,
                    "scopes": scopes_map,
                    "families": family_map
                        .iter()
                        .map(|(f, t)| (f.clone(), t.json()))
                        .collect::<serde_json::Map<String, Value>>(),
                }),
            );
        }
        per_ladder.push(json!({
            "id": ladder.id,
            "title": ladder.title,
            "rationale": ladder.rationale,
            "tiers": ladder.tiers.iter().map(|t| t.method()).collect::<Vec<String>>(),
            "boards": boards,
        }));
    }

    let shipped = "shipped";
    let marginal = |id: &str, board: &str| -> Value {
        let a = index.get(&(
            id.to_owned(),
            board.to_owned(),
            "pooled".to_owned(),
            "*".to_owned(),
        ));
        let b = index.get(&(
            shipped.to_owned(),
            board.to_owned(),
            "pooled".to_owned(),
            "*".to_owned(),
        ));
        match (a, b) {
            (Some(a), Some(b)) => {
                let caught = a.caught as i64 - i64::try_from(b.caught).unwrap_or(i64::MAX);
                let wrong = a.wrong as i64 - i64::try_from(b.wrong).unwrap_or(i64::MAX);
                json!({
                    "ladder": id,
                    "board": board,
                    "bought_carries": caught,
                    "bought_false_relinks": wrong,
                    "net_at_3_to_1": caught - wrong * 3,
                })
            }
            _ => Value::Null,
        }
    };
    let fuzzy_sweep: Vec<Value> = ladder_list
        .iter()
        .filter(|l| l.id != shipped && l.tiers.len() > shipped_tiers().len())
        .flat_map(|l| {
            BOARDS
                .iter()
                .map(|b| marginal(l.id, b.name))
                .collect::<Vec<Value>>()
        })
        .collect();

    let mut gates = evaluate_gates(&thresholds_path, &index);
    // The pin is a gate, not a footnote: a relink measurement made with a copy of the pass that is not the
    // shipped pass is a rumour, and the only way to keep the copy honest is to fail a build over it.
    gates.push(json!({
        "id": "store-pin-agrees",
        "claim": "the spike's copy of §6.2's ladder writes, link for link, what `fathom_store`'s own relink \
pass writes for the same episodes and the same review rows, on every event of the corpus",
        "expected": "pass",
        "ladder": "shipped",
        "board": "signed-off",
        "scope": "pooled",
        "metric": "pin_mismatches",
        "measured": pin_problems.len(),
        "pass": pin_problems.is_empty(),
        "as_expected": pin_problems.is_empty(),
        "detail": if pin_problems.is_empty() {
            format!("{} events scored, link for link", instances.len())
        } else {
            format!(
                "{} mismatching events; first: {}",
                pin_problems.len(),
                pin_problems[0]
            )
        },
    }));
    let gates_as_expected = gates
        .iter()
        .all(|row| row["as_expected"].as_bool().unwrap_or(false));

    let corpus = json!({
        "turn_histories": histories.len(),
        "annotated_turns": histories.values().map(|h| h.turns.len()).sum::<usize>(),
        "rewrite_events": instances.len(),
        "families": instances.iter().map(|i| i.family.clone()).collect::<BTreeSet<String>>(),
        "corpus_files": manifests.iter().map(|m| m.file.clone()).collect::<Vec<String>>(),
    });

    let report = json!({
        "what": "Spike s6 (`reco-f5a`): the false-relink and missed-relink rates ADR-003 §13.1 named as the \
            condition for revisiting fuzzy re-linking, measured against the hand-authored rewrite corpus in \
            crates/fathom-fixtures/corpus/relinks/. Every number is deterministic: no clock, no wall order, \
            no randomness — `--check` compares the committed report byte for byte.",
        "how_to_rerun": "cargo run -q -p fathom-analyze --features sqlite-bundled --example s6_spike -- \
            --report benchmarks/reports/s6-relink-accuracy.json",
        "corpus": corpus,
        "boards": BOARDS.iter().map(|b| json!({"name": b.name, "note": b.note})).collect::<Vec<Value>>(),
        "ladders": ladder_list.iter().map(|l| json!({
            "id": l.id, "title": l.title, "rationale": l.rationale,
            "tiers": l.tiers.iter().map(|t| t.method()).collect::<Vec<String>>(),
        })).collect::<Vec<Value>>(),
        "store_pin": {
            "what": "The spike's copy of §6.2's pass is checked against `fathom_store::episode`'s real \
                relink pass, run on the same episodes and review rows, for every event on every board.",
            "events_checked": instances.len(),
            "mismatches": pin_problems.len(),
            "agrees": pin_problems.is_empty(),
            "examples": pin_problems.iter().take(20).map(|p| json!(p)).collect::<Vec<Value>>(),
        },
        "results": per_ladder,
        "fuzzy_marginal_price": fuzzy_sweep,
        "gates": gates,
        "gates_all_as_expected": gates_as_expected,
    });

    let rendered = serde_json::to_string_pretty(&report)
        .unwrap_or_else(|e| panic!("the report the spike built cannot be serialized: {e}"));

    if check {
        let committed = std::fs::read_to_string(&report_path).unwrap_or_else(|_| {
            panic!(
                "no report at {}; run without --check first",
                report_path.display()
            )
        });
        let stale = committed != rendered;
        let gate_fail = report
            .get("gates")
            .and_then(|g| g.as_array())
            .map(|rows| {
                rows.iter()
                    .filter(|r| !r["as_expected"].as_bool().unwrap_or(false))
                    .count()
            })
            .unwrap_or(0);
        if stale {
            eprintln!(
                "s6 spike: the committed report is not what the corpus and the rules produce now."
            );
        }
        if gate_fail > 0 {
            eprintln!(
                "s6 spike: {} gate(s) came out other than claimed — see the report",
                gate_fail
            );
        }
        if !pin_problems.is_empty() {
            eprintln!(
                "s6 spike: the spike's copy of the shipped pass disagrees with fathom-store on {} event(s)",
                pin_problems.len()
            );
        }
        if stale || gate_fail > 0 || !pin_problems.is_empty() {
            std::process::exit(1);
        }
        println!("s6 spike: report matches the corpus exactly");
        println!(
            "  {} rewrite events, {} ladders, {} gates, store pin {} for {} events",
            instances.len(),
            ladder_list.len(),
            gates.len(),
            if pin_problems.is_empty() {
                "agrees"
            } else {
                "DISAGREES"
            },
            instances.len()
        );
        return;
    }

    if let Some(parent) = report_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&report_path, &rendered)
        .unwrap_or_else(|e| panic!("{}: cannot write the report: {e}", report_path.display()));
    println!(
        "s6 spike: wrote {} ({:.1} KB)",
        report_path.display(),
        rendered.len() as f64 / 1024.0
    );
    println!(
        "  {} rewrite events over {} histories, {} ladders, {} boards",
        instances.len(),
        histories.len(),
        ladder_list.len(),
        BOARDS.len()
    );
    println!(
        "  store pin: {} over {} events",
        if pin_problems.is_empty() {
            "agrees"
        } else {
            "DISAGREES"
        },
        instances.len()
    );
    for problem in pin_problems.iter().take(10) {
        println!("    {problem}");
    }
    let shown = [
        "shipped",
        "fz-combined@0.50",
        "fz-paths@0.70",
        "recency-any",
    ];
    for row in &per_ladder {
        let id = row["id"].as_str().unwrap_or("?");
        if !shown.contains(&id) {
            continue;
        }
        let tally = &row["boards"]["signed-off"]["scopes"]["pooled"];
        println!(
            "  {id:22} owed {:5} caught {:5} missed {:5} (rate {}) written {:5} false {:5} (rate {})",
            tally["owed"].as_u64().unwrap_or(0),
            tally["caught"].as_u64().unwrap_or(0),
            tally["missed"].as_u64().unwrap_or(0),
            tally["missed_rate"],
            tally["written"].as_u64().unwrap_or(0),
            tally["false_relinks"].as_u64().unwrap_or(0),
            tally["false_rate_of_written"],
        );
    }
    let mut failed = 0;
    for row in &gates {
        if row["as_expected"].as_bool().unwrap_or(false) {
            continue;
        }
        failed += 1;
        println!(
            "  gate {}: expected {}, measured {} ({})",
            row["id"].as_str().unwrap_or("?"),
            row["expected"].as_str().unwrap_or("?"),
            row["measured"],
            row["detail"].as_str().unwrap_or("")
        );
    }
    if failed > 0 {
        std::process::exit(1);
    }
}
