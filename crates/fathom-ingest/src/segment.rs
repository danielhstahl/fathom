//! `segment.rs` — `e1.3`: turns a walk of commits into the episodes a human reviews.
//!
//! An episode is *one agent turn, seen as one reviewable unit of change* (ADR-001 §4.1). Most of the time
//! a turn is one commit; often it is a commit plus the fixups that followed it; sometimes it is one commit
//! of a longer sequence with the same author inside the same minute. The rules below are ordered by how
//! much they should be trusted, and the rule that won is recorded on the episode — because a wrong guess
//! that says which rule made it is debuggable, and a wrong guess that does not is a mystery the reviewer
//! resolves by reading the diff anyway.
//!
//! # The boundary rules, in priority order (ADR-003 §4.2)
//!
//! This table *is* [ADR-003 §4.2](../../../docs/adr/0003-episodes.md), and the order is the tie-breaker
//! (its TB2): first opinion wins, nothing is blended, one rule is recorded. `tools/check_docs.py` does not
//! read Rust, so `tests/test_episode_identity_contract.py` diffs this table against the ADR and against
//! the config vocabulary, and fails on drift.
//!
//! | # | Rule | Class | Fires when | Confidence |
//! |---|---|---|---|---|
//! | 1 | `override` | human | `fathom.toml` forces a split before, or a merge with, this commit | 1.0 |
//! | 2 | `hook` | evidence | The commit names a harness turn that differs from the previous commit's | 1.0 |
//! | 3 | `trailer` | evidence | An `Agent-Turn:` trailer (or the reserved `Fathom-Episode:`) names a different turn than the previous commit's | 0.9 |
//! | 4 | `branch` | graph | The commit is a merge (or the previous one was): a branch boundary is a reviewable unit | 0.8 |
//! | 5 | `author` | heuristic | The author identity changed | 0.75 |
//! | 6 | `subject-claim` | heuristic | Both neighbours lead with a ticket token: equal keeps, different splits, one-sided is no opinion | 0.7 |
//! | 7 | `time-gap` | heuristic | The gap since the previous commit exceeds [`Rules::gap_millis`] | 0.6 |
//! | 8 | `message-shape` | heuristic | The subject is a *continuation* (`wip`, `fixup!`, "address review") or a new conventional type | 0.4 |
//! | 9 | `root` | heuristic | Nothing above fired and there is no previous commit | 0.5 |
//!
//! Rules 1–3 are what someone *stated* (a config line, an event, a trailer); 4 is a fact about the graph;
//! 5–8 are guesses. Confidence is what that distinction prices: an episode whose boundary says
//! `message-shape 0.40` is one the reviewer may want to re-cut, and the card says so (ADR-003 §4.7).
//!
//! Nine rungs, eight stored rule names: rung 6 is read out of the subject line like rung 8 is, so it
//! *stores* `message-shape` and the two are told apart by the confidence beside them — 0.70 is a claim the
//! author made, 0.40 is Fathom guessing from a convention. Appendix B's vocabulary stayed closed on
//! purpose ([`Rule::SubjectClaim`], ADR-003 §18.2 **D1**).
//!
//! # Identity, and why a rebase does not orphan a review
//!
//! [`identity`] derives the episode id from the three facts a rewrite preserves — who authored it, what
//! its subject says, which paths it touches — so the id of an episode survives `commit --amend`,
//! `rebase -i` and a force-push (ADR-003 I1). The store side ([`fathom_store::episode`]) adds a fallback
//! digest and a relink pass for the rewrites that change the subject or move files between commits
//! (ADR-003 §6).
//!
//! # What is deliberately not here
//!
//! Harness *events* (the JSONL stream of ADR-005) are a segmentation input, not a git fact, so they
//! arrive as [`CommitFacts::turn_id`] filled by whoever read them; this module never opens a file.
//!
//! # The trailer keys, and who writes them
//!
//! [`TRAILER_TURN`] is the key rung 3 reads, and it is the key [`crate::hooks::trailer_block`] writes —
//! one constant on each side, not two copies of a string. ADR-003 §18.2 **D5** decided this pairing (the
//! rule's *value* names a class of evidence, so minting a ninth `boundary_rule` would have been the drift;
//! what moved was the key), and [`TRAILER_EPISODE`] is **reserved** for the day Fathom itself declares an
//! episode start: nothing writes it, and reading it costs nothing.
//!
//! The *value* a turn id carries is qualified by [`qualify_turn`] before any rule compares it, on both
//! transports. The writer side qualifies at the source (ADR-003 §18.2 **D4**), but a trailer written before
//! that writer existed — or by a harness that counts turns per session and stops at the session — carries
//! a bare index, and a bare index read as an id reads two sessions' turn 3 as one turn. Rung 2 and rung 3
//! both compare qualified ids, so the defect cannot come back through the transport that is quieter about it.
//!
//! Like the rest of this module, no clock is read here: gaps are differences between commit dates given
//! in (ADR-002 §4.4).

use std::collections::BTreeMap;
use std::fmt;

use sha2::{Digest as _, Sha256};

/// The namespace of the identity digest, so a rule change cannot collide with an old id.
pub const IDENTITY_SCHEMA: &str = "fathom-episode/1";

/// The trailer key that carries the harness turn, lower-cased the way [`crate::record::Trailers`] stores
/// keys. This is the key rung 3 reads *and* the key [`crate::hooks`] writes (ADR-003 §18.2 **D5**).
pub const TRAILER_TURN_KEY: &str = "agent-turn";

/// [`TRAILER_TURN_KEY`] as it is spelled in a commit message.
pub const TRAILER_TURN: &str = "Agent-Turn";

/// The trailer key that carries the harness session.
pub const TRAILER_SESSION_KEY: &str = "agent-session";

/// [`TRAILER_SESSION_KEY`] as it is spelled in a commit message.
pub const TRAILER_SESSION: &str = "Agent-Session";

/// The reserved trailer key naming an episode start Fathom itself declared. Nothing writes it today, and
/// an additional trailer spelling is an amendment to ADR-003 §4.2, not a new enum value.
pub const TRAILER_EPISODE_KEY: &str = "fathom-episode";

/// [`TRAILER_EPISODE_KEY`] as it is spelled in a commit message.
pub const TRAILER_EPISODE: &str = "Fathom-Episode";

/// How the author of a commit is classified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorKind {
    /// Written by a coding agent.
    Agent,
    /// Written by a person.
    Human,
    /// No usable identity at all — ADR-001 §7.7's `unknown`, printed as `?`, never guessed at.
    Unknown,
}

impl AuthorKind {
    /// The store's spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            AuthorKind::Agent => "agent",
            AuthorKind::Human => "human",
            AuthorKind::Unknown => "unknown",
        }
    }
}

impl fmt::Display for AuthorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which rule opened an episode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// A harness turn event named a new turn.
    Hook,
    /// An explicit trailer named a new episode or turn.
    Trailer,
    /// A `fathom.toml` override moved the boundary.
    Override,
    /// A merge: the branch boundary is the unit.
    Branch,
    /// The author changed.
    Author,
    /// Both neighbours' subjects claim a ticket: the same one they keep, different ones split.
    ///
    /// Stored as `message-shape`, not as a ninth `boundary_rule` value: this is a fact read out of the
    /// subject line, which is the class of evidence rung 8 already names, and ADR-003 Appendix B is closed
    /// (§18.2 **D1**). The confidence is what distinguishes them — 0.7 for a claim, 0.4 for a guess.
    SubjectClaim,
    /// The gap since the previous commit exceeded the threshold.
    TimeGap,
    /// The subject line's shape: a continuation, or a new conventional type.
    MessageShape,
    /// The first commit in the walk.
    Root,
}

impl Rule {
    /// The store's spelling, and the string the episode card prints.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Rule::Hook => "hook",
            Rule::Trailer => "trailer",
            Rule::Override => "override",
            Rule::Branch => "branch",
            Rule::Author => "author",
            Rule::SubjectClaim => "message-shape",
            Rule::TimeGap => "time-gap",
            Rule::MessageShape => "message-shape",
            Rule::Root => "root",
        }
    }
}

/// The commit facts a boundary decision uses.
///
/// Deliberately small and owned by the caller: the store can produce these from `git_commit` rows, and a
/// test can produce them from a literal, which is what makes the boundary corpus (`e1.3`'s acceptance
/// criterion) a table rather than a git fixture.
#[derive(Debug, Clone, Default)]
pub struct CommitFacts {
    /// The commit id.
    pub oid: String,
    /// Parent ids, first parent first.
    pub parents: Vec<String>,
    /// The author name as committed.
    pub author_name: String,
    /// The author address as committed.
    pub author_email: String,
    /// The author date, unix millis.
    pub author_utc: i64,
    /// The first line of the message.
    pub subject: String,
    /// Trailers, lower-cased key to last value.
    pub trailers: BTreeMap<String, String>,
    /// The paths the commit touched; the order here does not matter, [`identity`] sorts them.
    pub paths: Vec<String>,
    /// Files touched.
    pub n_files: i64,
    /// Lines added.
    pub n_add: i64,
    /// Lines removed.
    pub n_del: i64,
    /// Binary files touched.
    pub n_binary: i64,
    /// The harness session, when a trailer or an event named one.
    pub session_id: Option<String>,
    /// The harness turn, when a trailer or an event named one.
    pub turn_id: Option<String>,
}

impl CommitFacts {
    /// Whether this is a merge commit.
    #[must_use]
    pub fn is_merge(&self) -> bool {
        self.parents.len() > 1
    }

    /// The identity the author rules compare: the address when there is one, else the name.
    #[must_use]
    pub fn author_identity(&self) -> String {
        let email = self.author_email.trim().to_lowercase();
        if email.is_empty() {
            self.author_name.trim().to_lowercase()
        } else {
            email
        }
    }
}

/// A forced boundary from `fathom.toml` (`[[episodes.force-split]]`, `[[episodes.force-merge]]`).
#[derive(Debug, Clone, Default)]
pub struct Override {
    /// Match a commit by oid prefix.
    pub oid: Option<String>,
    /// Match a commit by a substring of its subject.
    pub subject: Option<String>,
}

impl Override {
    /// Does this override name `commit`?
    #[must_use]
    pub fn matches(&self, commit: &CommitFacts) -> bool {
        let by_oid = self
            .oid
            .as_deref()
            .is_some_and(|prefix| commit.oid.starts_with(prefix));
        let by_subject = self
            .subject
            .as_deref()
            .is_some_and(|needle| commit.subject.contains(needle));
        by_oid || by_subject
    }
}

/// The tunables, all of them overridable from `fathom.toml` (ADR-002 §4.5: they arrive as values, this
/// crate never reads a config file).
#[derive(Debug, Clone)]
pub struct Rules {
    /// The gap that starts a new episode, in milliseconds.
    pub gap_millis: i64,
    /// Substrings that mark an author as an agent.
    pub agent_markers: Vec<String>,
    /// Substrings in a subject that mean "still the same turn".
    pub continuation_markers: Vec<String>,
    /// Conventional-commit types: a change of type between neighbours is a new unit.
    pub conventional_types: Vec<String>,
    /// Forced splits, checked before the heuristics.
    pub force_split: Vec<Override>,
    /// Forced merges.
    pub force_merge: Vec<Override>,
}

impl Default for Rules {
    fn default() -> Self {
        Rules {
            // 30 minutes: the published default — `schemas/fathom.schema.json`,
            // `docs/concepts/config.md` and the repository's own `fathom.toml` all say 30, and ADR-003
            // §4.4 refuses to let a fourth number exist. Long enough that one turn's commits stay
            // together, short enough that a lunch break is not one episode. `s2` (reco-q6r) is the ticket
            // that gets to move it, with measurements (ADR-003 §13.7).
            gap_millis: 30 * 60 * 1000,
            agent_markers: [
                "bot",
                "agent",
                "codex",
                "copilot",
                "claude",
                "llm",
                "assistant",
                "cursor",
                "devin",
                "aider",
                "openhands",
                "swe-agent",
                "autofix",
            ]
            .iter()
            .map(|s| (*s).to_owned())
            .collect(),
            continuation_markers: [
                "wip",
                "fixup!",
                "squash!",
                "amend",
                "address review",
                "review feedback",
                "typo",
                "formatting",
                "fmt",
                "lint",
                "follow-up",
                "followup",
                "missed",
                "oops",
            ]
            .iter()
            .map(|s| (*s).to_owned())
            .collect(),
            conventional_types: [
                "feat", "fix", "docs", "style", "refactor", "perf", "test", "build", "ci", "chore",
                "revert",
            ]
            .iter()
            .map(|s| (*s).to_owned())
            .collect(),
            force_split: Vec::new(),
            force_merge: Vec::new(),
        }
    }
}

/// One episode: a run of commits one rule said belong together.
#[derive(Debug, Clone)]
pub struct Episode {
    /// The derived identity.
    pub id: String,
    /// The strong digest.
    pub fingerprint: String,
    /// The reword-tolerant digest.
    pub fallback_fingerprint: String,
    /// The files-and-author digest: the relink key when the subject changed.
    pub shape_fingerprint: String,
    /// Which rule opened it, and how far to trust it.
    pub rule: Rule,
    /// See [`Episode::rule`].
    pub confidence: f64,
    /// Whether a forced boundary moved this episode's start.
    pub overridden: bool,
    /// The author classification and its confidence.
    pub author_kind: AuthorKind,
    /// See [`Episode::author_kind`].
    pub author_confidence: f64,
    /// The author as committed on the first member.
    pub author_name: String,
    /// See [`Episode::author_name`].
    pub author_email: String,
    /// The harness session/turn, when either was named.
    pub session_id: Option<String>,
    /// See [`Episode::session_id`]. Session-qualified (`<session_id>:<turn_index>`) whenever a session is
    /// known, because a bare index is not an id (ADR-003 §9.1, §18.2 **D4**).
    pub turn_id: Option<String>,
    /// The members, oldest first.
    pub members: Vec<String>,
    /// The first and last member's oid.
    pub first_oid: String,
    /// See [`Episode::first_oid`].
    pub last_oid: String,
    /// The first and last member's author date.
    pub first_utc: i64,
    /// See [`Episode::first_utc`].
    pub last_utc: i64,
    /// The sums over the members.
    pub n_files: i64,
    /// See [`Episode::n_files`].
    pub n_add: i64,
    /// See [`Episode::n_files`].
    pub n_del: i64,
    /// See [`Episode::n_files`].
    pub n_binary: i64,
    /// The one-line summary: the first member's subject, or the merge's.
    pub summary: String,
    /// The ordinal on the lineage, assigned by [`segment`] in walk order.
    pub lineage_seq: i64,
}

impl Episode {
    /// How many commits the episode spans.
    #[must_use]
    pub fn n_commits(&self) -> usize {
        self.members.len()
    }

    /// The turn number inside [`Episode::session_id`], when the id carries one: `Some(3)` for `s1:3`.
    ///
    /// Derived, never stored: a second copy of the number is a second thing to disagree with the id.
    /// `fathom show --json` prints it beside `turn_id` (ADR-003 §9.1).
    #[must_use]
    pub fn turn_index(&self) -> Option<u64> {
        turn_index(self.turn_id.as_deref())
    }
}

/// Compute the identity digests of a change.
///
/// The strong digest is what makes `episode_id` stable across a rewrite; the two weaker ones are what the
/// store's relink pass matches on when a rewrite breaks the strong one. All three are over *facts a
/// rewrite preserves*, and nothing else — a digest that included a tree id or a commit date would be a
/// digest that changes when you rebase, which is the whole bug this exists to kill. See [`Identity`] for
/// what each digest is over and what each therefore survives.
#[must_use]
pub fn identity(author_identity: &str, subject: &str, paths: &[impl AsRef<str>]) -> Identity {
    let subject = normalize_subject(subject);
    let mut sorted: Vec<&str> = paths.iter().map(AsRef::as_ref).collect();
    sorted.sort_unstable();
    sorted.dedup();
    let strong = {
        let mut digest = Sha256::new();
        digest.update(IDENTITY_SCHEMA.as_bytes());
        digest.update([0]);
        digest.update(author_identity.as_bytes());
        digest.update([0]);
        digest.update(subject.as_bytes());
        digest.update([0]);
        for path in &sorted {
            digest.update(path.as_bytes());
            digest.update([0]);
        }
        digest.finalize()
    };
    let fallback = {
        let mut digest = Sha256::new();
        digest.update(IDENTITY_SCHEMA.as_bytes());
        digest.update(b"fallback");
        digest.update([0]);
        digest.update(author_identity.as_bytes());
        digest.update([0]);
        digest.update(subject.as_bytes());
        // Every field is NUL-terminated, this one included. Omitting the separator here made the Rust
        // copy hash `author\0subject` where the reference implementation (`src/recommender/segment.py`)
        // and ADR-003 Appendix A hash `author\0subject\0`, so relink tier 1's key disagreed with the
        // published bytes while the strong digest — whose last field *is* terminated — agreed. A drift
        // that only shows up in the weaker digests is the kind that survives review, which is why the
        // published vector, not the two sources agreeing with each other, is the contract.
        digest.update([0]);
        digest.finalize()
    };
    let shape = {
        let mut digest = Sha256::new();
        digest.update(IDENTITY_SCHEMA.as_bytes());
        digest.update(b"shape");
        digest.update([0]);
        digest.update(author_identity.as_bytes());
        digest.update([0]);
        for path in &sorted {
            digest.update(path.as_bytes());
            digest.update([0]);
        }
        digest.finalize()
    };
    Identity {
        fingerprint: hex(&strong),
        fallback_fingerprint: hex(&fallback),
        shape_fingerprint: hex(&shape),
    }
}

/// The three digests [`identity`] derives for one change, in decreasing order of trust.
///
/// Kept next to [`identity`] rather than in the store because the framing *is* the identity contract:
/// ADR-003 Appendix A publishes digests for three fact-sets, and both this function and
/// `src/recommender/segment.py` must reproduce them byte for byte.
///
/// No single digest survives the three rewrites a reviewer actually performs, which is why there are three
/// and why the store's relink pass tries them in this order:
///
/// | digest | over | survives |
/// |---|---|---|
/// | [`Identity::fingerprint`] | author, subject, path set | `--amend --no-edit`, a plain rebase, a force-push |
/// | [`Identity::fallback_fingerprint`] | author, subject | an amend that adds files, a squash that keeps the subject |
/// | [`Identity::shape_fingerprint`] | author, path set | a reword, including one in the middle of a rebase |
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// Author, subject and path set: the id itself.
    pub fingerprint: String,
    /// Author and subject: the relink key when files moved.
    pub fallback_fingerprint: String,
    /// Author and path set: the relink key when the subject changed.
    pub shape_fingerprint: String,
}

/// The episode id: a short prefix of the strong digest, with a suffix only when two live episodes collide.
#[must_use]
pub fn episode_id(fingerprint: &str) -> String {
    format!("ep-{}", &fingerprint[..20.min(fingerprint.len())])
}

/// Normalise a subject line for identity and for the continuation heuristic.
///
/// Lower-cased, whitespace-collapsed, with `fixup!`/`squash!`/`amend!` and one leading conventional type
/// removed — so `fixup! feat(store): make epochs safe` and `feat(store): make epochs safe` normalise to
/// the same thing, which is both why a fixup groups with its target and why squashing it does not change
/// the episode's identity.
#[must_use]
pub fn normalize_subject(subject: &str) -> String {
    let mut out = subject.trim().to_lowercase();
    loop {
        let before = out.clone();
        for prefix in ["fixup! ", "squash! ", "amend! "] {
            if let Some(rest) = out.strip_prefix(prefix) {
                out = rest.trim_start().to_owned();
            }
        }
        if let Some((head, rest)) = out.split_once(':') {
            let head = head.trim().trim_end_matches('!');
            let base = head.split('(').next().unwrap_or(head).trim_end_matches(')');
            if is_conventional(base) {
                out = rest.trim_start().to_owned();
            }
        }
        if out == before {
            break;
        }
    }
    let mut collapsed = String::with_capacity(out.len());
    let mut pending_space = false;
    for ch in out.chars() {
        if ch.is_whitespace() {
            pending_space = true;
            continue;
        }
        if pending_space && !collapsed.is_empty() {
            collapsed.push(' ');
        }
        pending_space = false;
        collapsed.push(ch);
    }
    collapsed
}

fn is_conventional(head: &str) -> bool {
    [
        "feat", "fix", "docs", "style", "refactor", "perf", "test", "build", "ci", "chore",
        "revert",
    ]
    .contains(&head)
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

/// TB7's clock guard: a changed conventional type splits only when the clock moved by more than
/// `gap_millis / TB7_CLOCK_GUARD_DIVISOR`.
///
/// §4.4 published `gap ÷ 9` — 3 minutes 20 seconds at the published gap — as a guess, and ADR-003 §18.2
/// **D6** found it was a defect: 3.33 min is a *shorter* interval than the "the review came back" pattern
/// TB7 exists to tolerate, so §5.2's own worked example (`feat:` … `fix: review feedback` three minutes
/// later … `test:` four minutes after that) was being cut into three episodes by the rule meant to keep it
/// in one. The divisor is now derived rather than guessed. The turn corpus has five within-turn adjacencies
/// where the conventional type changed; four of them are 3 or 4 minutes apart and are annotated as one
/// turn, and the guard has to clear them, so the guard must exceed 4 minutes. The corpus's pooled-F1
/// argmax is a 1-minute guard (+0.010 over this one) and it is declined on purpose: a 1-minute guard cuts
/// all five of those adjacencies, which is the exact failure the guard is for, and the whole curve from 4
/// to 30 minutes is flat within 0.005. Five buys margin over the measured 4 and sits on that flat part.
/// Measured by `tb7_guard_sweep` in spike `s2`'s harness, asserted against this number by
/// `test_the_tb7_guard_is_the_value_the_corpus_derived`, and argued in ADR-003 §4.3.
pub const TB7_CLOCK_GUARD_DIVISOR: i64 = 5;

/// Which conventional type a subject declares, if any.
///
/// Public because the boundary rule that uses it (ADR-003 §4.2 rung 7, TB7) has to be reusable by the
/// measurement that scores it: spike `s2` (`reco-q6r`) evaluates alternative rule subsets, and a spike that
/// re-implemented type parsing would be scoring its own copy rather than the shipped rule.
pub fn conventional_type(subject: &str) -> Option<&'static str> {
    let lower = subject.trim().to_lowercase();
    let head = lower.split_once(':').map(|(head, _)| head.trim())?;
    let head = head.split('(').next().unwrap_or(head).trim_end_matches('!');
    [
        "feat", "fix", "docs", "style", "refactor", "perf", "test", "build", "ci", "chore",
        "revert",
    ]
    .iter()
    .copied()
    .find(|ty| *ty == head)
}

/// Is the subject a continuation of the previous change?
///
/// A single-word marker (`wip`, `fixup!`, `lint`) is a *prefix* claim — "lint" in the middle of a subject
/// about a feature is not a continuation. A phrase (`address review`) may appear anywhere, because the
/// phrase is specific enough to mean the same thing wherever it sits.
#[must_use]
pub fn is_continuation(subject: &str, rules: &Rules) -> bool {
    let normalized = normalize_subject(subject);
    rules.continuation_markers.iter().any(|marker| {
        if marker.contains(' ') {
            normalized.contains(marker.as_str())
        } else {
            normalized.starts_with(marker.as_str())
        }
    })
}

/// The ticket a subject claims, if it claims one (ADR-003 §4.2 rung 6, §18.2 **D1**).
///
/// This is the one signal the corpus forced out of the data rather than one the ADR thought of first: a
/// loop-driven history writes the ticket it is working on at the front of every commit of the turn and
/// changes it between turns, which makes a leading tracker token a *claim the author made about this
/// commit* (ADR-001 §4.3 counts a commit message as a claim about work) rather than a pattern Fathom
/// guessed. That is what licenses pricing it at 0.7 — above every clock, below every declared rung.
///
/// Deliberately narrow: a *leading* token, not a substring anywhere, because a token found in prose about a
/// different ticket is not a claim about this commit. `reco-q6r: …`, `[JIRA-123] …`, `#412 …` qualify;
/// `chore`, `contracts.yml` and a subject that merely contains a hyphen do not.
#[must_use]
pub fn subject_claim(subject: &str) -> Option<String> {
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
    if looks_like_tracker_id(&candidate) {
        Some(candidate.to_lowercase())
    } else {
        None
    }
}

/// The shape test [`subject_claim`] applies: a short prefix with a separator, which is what makes a false
/// positive rare enough to trust the token as evidence at all.
#[must_use]
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

/// The subject-claim rung's opinion about the boundary before `commit`: `None` is "no opinion", which is
/// TB6 read honestly — a token on one side only is a claim about that side, not a disagreement between
/// the two, and the corpus's 60 within-turn adjacencies are what that restraint buys.
#[must_use]
pub fn claim_opinion(previous: &str, commit: &str) -> Option<bool> {
    match (subject_claim(commit), subject_claim(previous)) {
        (Some(mine), Some(theirs)) => Some(mine != theirs),
        _ => None,
    }
}

/// Do two commits touch at least one path in common?
#[must_use]
pub fn shares_path(previous: &CommitFacts, commit: &CommitFacts) -> bool {
    !previous.paths.is_empty()
        && previous
            .paths
            .iter()
            .any(|path| commit.paths.iter().any(|other| other == path))
}

/// Classify an author as agent or human, with the confidence the evidence supports.
#[must_use]
pub fn author_kind(facts: &CommitFacts, rules: &Rules) -> (AuthorKind, f64) {
    // A trailer is a statement, not a guess: `Co-authored-by: Claude <noreply@anthropic.com>` and
    // `Assisted-by: claude-code` are the two spellings that exist in the wild today.
    let trailer_text: String = facts
        .trailers
        .iter()
        .filter(|(key, _)| {
            key.contains("co-authored-by")
                || key.contains("assisted-by")
                || key.contains("generated-with")
                || key.contains("agent")
                || key.contains("session")
        })
        .map(|(_, value)| format!(" {value}"))
        .collect();
    for marker in &rules.agent_markers {
        let needle = marker.to_lowercase();
        if trailer_text.to_lowercase().contains(&needle) {
            return (AuthorKind::Agent, 0.95);
        }
        let identity = format!("{} {}", facts.author_name, facts.author_email).to_lowercase();
        if identity.contains(&needle) {
            return (AuthorKind::Agent, 0.85);
        }
    }
    if facts.author_identity().is_empty() {
        return (AuthorKind::Unknown, 0.0);
    }
    // No agent evidence and a real identity: "human" is the honest default, but the claim is weak, and
    // the card prints the weakness.
    (AuthorKind::Human, 0.5)
}

/// Segment a walk of commits into episodes.
///
/// `commits` must be in review order — oldest first along the lineage the caller walked (the store hands
/// this in topologically sorted).
#[must_use]
pub fn segment(commits: &[CommitFacts], rules: &Rules) -> Vec<Episode> {
    let mut episodes: Vec<Episode> = Vec::new();
    let mut used_ids: BTreeMap<String, usize> = BTreeMap::new();
    for (index, commit) in commits.iter().enumerate() {
        let (starts, rule, confidence) = if index == 0 {
            (true, Rule::Root, 0.5)
        } else {
            decide(&commits[index - 1], commit, rules)
        };
        let overridden = rules
            .force_split
            .iter()
            .chain(rules.force_merge.iter())
            .any(|force| force.matches(commit));
        if episodes.is_empty() || (starts && index > 0) {
            let mut episode = new_episode(commit, rule, confidence, overridden, rules);
            let base = episode_id(&episode.fingerprint);
            let count = used_ids.entry(base.clone()).or_insert(0);
            *count += 1;
            episode.id = if *count == 1 {
                base
            } else {
                format!("{base}-{count}")
            };
            episodes.push(episode);
        } else {
            if let Some(tail) = episodes.last_mut() {
                extend(tail, commit);
                tail.overridden |= overridden;
            }
        }
    }
    for (ordinal, episode) in episodes.iter_mut().enumerate() {
        episode.lineage_seq = i64::try_from(ordinal).unwrap_or(i64::MAX);
    }
    episodes
}

fn new_episode(
    commit: &CommitFacts,
    rule: Rule,
    confidence: f64,
    overridden: bool,
    rules: &Rules,
) -> Episode {
    let identity = identity(&commit.author_identity(), &commit.subject, &commit.paths);
    // ADR-003 §8 H4: the marker table is a rule value like every other tunable, so the attribution used
    // here is the caller's rather than a second compiled-in copy of it. Reading `Rules::default()` here
    // would hand a repository that extends the marker list one attribution for the boundary and another
    // for the card.
    let (kind, kind_confidence) = author_kind(commit, rules);
    Episode {
        id: String::new(),
        fingerprint: identity.fingerprint,
        fallback_fingerprint: identity.fallback_fingerprint,
        shape_fingerprint: identity.shape_fingerprint,
        rule,
        confidence,
        overridden,
        author_kind: kind,
        author_confidence: kind_confidence,
        author_name: commit.author_name.clone(),
        author_email: commit.author_email.clone(),
        session_id: named_session(commit),
        turn_id: named_turn(commit).or_else(|| named_trailer_turn(commit)),
        members: vec![commit.oid.clone()],
        first_oid: commit.oid.clone(),
        last_oid: commit.oid.clone(),
        first_utc: commit.author_utc,
        last_utc: commit.author_utc,
        n_files: commit.n_files,
        n_add: commit.n_add,
        n_del: commit.n_del,
        n_binary: commit.n_binary,
        summary: commit.subject.clone(),
        lineage_seq: 0,
    }
}

fn extend(episode: &mut Episode, commit: &CommitFacts) {
    episode.members.push(commit.oid.clone());
    episode.last_oid = commit.oid.clone();
    episode.last_utc = episode.last_utc.max(commit.author_utc);
    episode.n_files += commit.n_files;
    episode.n_add += commit.n_add;
    episode.n_del += commit.n_del;
    episode.n_binary += commit.n_binary;
    if episode.turn_id.is_none() {
        episode.turn_id = named_turn(commit).or_else(|| named_trailer_turn(commit));
    }
    if episode.session_id.is_none() {
        episode.session_id = named_session(commit);
    }
}

fn trailer(trailers: &BTreeMap<String, String>, key: &str) -> Option<String> {
    trailers.get(key).cloned().filter(|value| !value.is_empty())
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// The session this commit's turn evidence belongs to (ADR-005 §3.3).
///
/// An ingested event's session outranks a trailer, which outranks the bare `Session:` spelling a
/// hand-written message still uses.
#[must_use]
pub fn named_session(commit: &CommitFacts) -> Option<String> {
    non_empty(commit.session_id.as_deref())
        .or_else(|| trailer(&commit.trailers, TRAILER_SESSION_KEY))
        .or_else(|| trailer(&commit.trailers, "session"))
}

/// Put a turn id into the shape ADR-003 §9.1 and ADR-005 §3.3 define: `<session_id>:<turn_index>`.
///
/// The qualification is not decoration and this function is the only place it happens. A bare `3` makes
/// two sessions' turn 3 one turn, the `hook` rung declines to cut between them, and two agent turns
/// become one episode — ADR-003 §18.2 **D4**, which is a defect with a number on it (+0.015 pooled
/// boundary F1, +0.031 on the fixture stratum) rather than a tidiness preference. A turn id that already
/// carries a session is passed through untouched (the trailer writer [`crate::hooks::trailer_block`
/// qualifies at the source], and so does an ingested event), and a turn with no session anywhere stays
/// bare, because inventing a session would be inventing evidence.
#[must_use]
pub fn qualify_turn(session_id: Option<&str>, turn_id: Option<&str>) -> Option<String> {
    let turn = non_empty(turn_id)?;
    if turn.contains(':') {
        return Some(turn);
    }
    match non_empty(session_id) {
        Some(session) => Some(format!("{session}:{turn}")),
        None => Some(turn),
    }
}

/// The [`Episode::turn_index`] that goes with a qualified id: `Some(3)` for `s1:3`, `None` for a bare
/// `3`, a non-numeric index, or no turn at all. `fathom show --json` prints this next to `turn_id` so a
/// consumer does not have to re-derive the split (ADR-003 §9.1).
#[must_use]
pub fn turn_index(turn_id: Option<&str>) -> Option<u64> {
    let (_, index) = turn_id?.rsplit_once(':')?;
    index.parse().ok()
}

/// The session this commit names, qualified: what rung 2 compares (ADR-003 §9.1, §18.2 **D4**).
#[must_use]
pub fn named_turn(commit: &CommitFacts) -> Option<String> {
    qualify_turn(named_session(commit).as_deref(), commit.turn_id.as_deref())
}

/// The turn or episode a *trailer* names for this commit, qualified the same way: the contract key first,
/// the reserved one second.
#[must_use]
fn named_trailer_turn(commit: &CommitFacts) -> Option<String> {
    let trailers = &commit.trailers;
    let bare =
        trailer(trailers, TRAILER_TURN_KEY).or_else(|| trailer(trailers, TRAILER_EPISODE_KEY));
    qualify_turn(named_session(commit).as_deref(), bare.as_deref())
}

/// Decide whether `commit` opens an episode after `previous`, and name the rule that decided.
///
/// Returns `(starts, rule, confidence)`. The rules are tried in the order the module doc lists; the first
/// one with an opinion wins, which is what makes the recorded rule the actual reason.
#[must_use]
pub fn decide(previous: &CommitFacts, commit: &CommitFacts, rules: &Rules) -> (bool, Rule, f64) {
    // 3. Forced boundaries are the user's own act, so they outrank even harness evidence.
    if rules.force_split.iter().any(|force| force.matches(commit)) {
        return (true, Rule::Override, 1.0);
    }
    if rules.force_merge.iter().any(|force| force.matches(commit)) {
        return (false, Rule::Override, 1.0);
    }
    // 1. Harness evidence outranks everything the git history has to say. Both commits name a turn: equal
    //    turns are one episode, different turns are two, and nothing else gets a vote. Compared
    //    session-qualified (ADR-003 §9.1, §18.2 D4): the trailer transport hands this rung a bare index,
    //    and `s1`'s turn 3 beside `s2`'s turn 3 is two turns, not one.
    if let (Some(mine), Some(theirs)) = (named_turn(commit), named_turn(previous)) {
        return (mine != theirs, Rule::Hook, 1.0);
    }
    // 3. A trailer naming this commit's turn. `Agent-Turn:` is the key the writer emits
    //    (`crate::hooks::trailer_block`, ADR-003 §18.2 D5); `Fathom-Episode:` is the reserved spelling for
    //    a start Fathom declares itself, read here so the day it is written nothing has to change. A
    //    trailer is a statement, so a *different* turn on the two sides is a boundary, and a turn named on
    //    one side alone is still a statement about that side (unlike one-sided *event* evidence, which
    //    TB6 refuses to read as a boundary).
    if let Some(id) = named_trailer_turn(commit) {
        let same = named_trailer_turn(previous).is_some_and(|other| other == id);
        return (!same, Rule::Trailer, 0.9);
    }
    // 4. A merge is its own unit: reviewing "the merge of branch X" beside the commits on branch X is two
    //    reviews of different things.
    if commit.is_merge() || previous.is_merge() {
        return (true, Rule::Branch, 0.8);
    }
    // 5. A different author is a different turn, always. This is also what keeps an interleaved human
    //    commit from being absorbed into the agent's episode around it.
    if commit.author_identity() != previous.author_identity() {
        return (true, Rule::Author, 0.75);
    }
    // 6. What the author of both commits *said* they were working on. Two loop-driven commits that each
    //    lead with a ticket token are one turn when they name the same ticket and two when they do not;
    //    a token on one side alone is no opinion at all (TB6). Above the clock on purpose — a claim the
    //    author made outranks a proxy for "a new decision", which is all the gap is (ADR-003 §4.3, §18.2
    //    **D1**). It stores `message-shape`: a subject-line claim is not a ninth vocabulary value.
    if let Some(splits) = claim_opinion(&previous.subject, &commit.subject) {
        return (splits, Rule::SubjectClaim, 0.7);
    }
    // 7. A gap in time.
    if commit.author_utc - previous.author_utc > rules.gap_millis {
        return (true, Rule::TimeGap, 0.6);
    }
    // 8. Message shape: a continuation keeps, a change of conventional type splits.
    if is_continuation(&commit.subject, rules) {
        return (false, Rule::MessageShape, 0.4);
    }
    // A change of conventional type is the weakest signal there is — an agent routinely does
    // `feat:` then `fix:` then `test:` inside one turn — so it only splits when the change is *also*
    // somewhere else (different files) and the clock has moved. Both guards matter: same files means the
    // same edit being finished, and a couple of minutes means the same decision being carried out.
    let mine = conventional_type(&commit.subject);
    let theirs = conventional_type(&previous.subject);
    if mine.is_some()
        && mine != theirs
        && !shares_path(previous, commit)
        && commit.author_utc - previous.author_utc > rules.gap_millis / TB7_CLOCK_GUARD_DIVISOR
    {
        return (true, Rule::MessageShape, 0.4);
    }
    // Nothing said "new", so this commit continues the episode; the rule named is the one that kept it.
    (false, Rule::MessageShape, 0.4)
}

/// Did the boundary rule say "start a new episode"?
impl Rule {
    /// Whether this boundary opens a new episode rather than continuing the current one.
    #[must_use]
    pub const fn starts(self) -> bool {
        !matches!(self, Rule::MessageShape)
    }
}

/// Score a segmentation against ground truth.
///
/// `truth` is one label per commit in `commits` order; two segmentations are compared as *partitions*, so
/// the labels themselves need not agree with anything — only which commits share an episode. Scoring
/// pairs rather than boundaries means a segmentation that gets the grouping right but splits the walk in a
/// different place is still wrong, which is the point: the grouping is the product.
#[must_use]
pub fn score(commits: &[CommitFacts], predicted: &[Episode], truth: &[String]) -> Score {
    assert_eq!(
        commits.len(),
        truth.len(),
        "ground truth is one label per commit"
    );
    let position: BTreeMap<&str, usize> = commits
        .iter()
        .enumerate()
        .map(|(index, commit)| (commit.oid.as_str(), index))
        .collect();
    let mut mine: std::collections::BTreeSet<(usize, usize)> = std::collections::BTreeSet::new();
    for episode in predicted {
        let indices: Vec<usize> = episode
            .members
            .iter()
            .filter_map(|oid| position.get(oid.as_str()).copied())
            .collect();
        for a in &indices {
            for b in &indices {
                if a != b {
                    mine.insert(((*a).min(*b), (*a).max(*b)));
                }
            }
        }
    }
    let mut theirs: std::collections::BTreeSet<(usize, usize)> = std::collections::BTreeSet::new();
    for a in 0..truth.len() {
        for b in 0..truth.len() {
            if a != b && truth[a] == truth[b] {
                theirs.insert((a.min(b), a.max(b)));
            }
        }
    }
    let tp = mine.intersection(&theirs).count();
    let precision = if mine.is_empty() {
        1.0
    } else {
        tp as f64 / mine.len() as f64
    };
    let recall = if theirs.is_empty() {
        1.0
    } else {
        tp as f64 / theirs.len() as f64
    };
    Score {
        precision,
        recall,
        f1: if precision + recall == 0.0 {
            0.0
        } else {
            2.0 * precision * recall / (precision + recall)
        },
    }
}

/// The pair of numbers a segmentation report leads with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Score {
    /// Of the groupings this segmentation drew, what share are right.
    pub precision: f64,
    /// Of the groupings the ground truth drew, what share were found.
    pub recall: f64,
    /// Their harmonic mean, which is the number the acceptance criterion is written against.
    pub f1: f64,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn commit(oid: &str, author: &str, minutes: i64, subject: &str, paths: &[&str]) -> CommitFacts {
        CommitFacts {
            oid: oid.to_owned(),
            author_name: author.to_owned(),
            author_email: format!("{author}@example.invalid"),
            author_utc: minutes * 60_000,
            subject: subject.to_owned(),
            paths: paths.iter().map(|p| (*p).to_owned()).collect(),
            n_files: paths.len() as i64,
            n_add: 10,
            ..CommitFacts::default()
        }
    }

    /// ADR-003 §5.2's worked example, and the reason TB7's clock guard is `gap ÷ 5`.
    ///
    /// This was `#[ignore]`d — quarantined rather than deleted, with the defect named — from the moment the
    /// failure was found (`reco-dot`) until the guard was re-derived from the turn corpus and spike `s2`'s
    /// report re-blessed **in the same change**. §18.2 **D6** ordered it in that sequence deliberately: a
    /// shipped boundary rule and the measurement of that rule move together, or the measurement quietly
    /// becomes an opinion. `gap ÷ 9` (3 min 20 s) was a shorter interval than the "the review came back"
    /// pattern TB7 exists to tolerate, so the rule meant to hold this run of commits in one episode was
    /// cutting it in two.
    #[test]
    fn a_run_of_agent_commits_inside_the_gap_is_one_episode() {
        let commits = vec![
            commit(
                "a1",
                "pi-agent",
                0,
                "feat: add epoch column",
                &["src/store.rs"],
            ),
            commit(
                "a2",
                "pi-agent",
                3,
                "fix: review feedback on the epoch column",
                &["src/store.rs"],
            ),
            commit(
                "a3",
                "pi-agent",
                7,
                "test: cover epoch migration",
                &["tests/epoch.rs"],
            ),
        ];
        let episodes = segment(&commits, &Rules::default());
        assert_eq!(episodes.len(), 1, "one turn is one episode: {episodes:?}");
        assert_eq!(episodes[0].n_commits(), 3);
        assert_eq!(episodes[0].author_kind, AuthorKind::Agent);
    }

    #[test]
    fn a_human_commit_between_agent_commits_is_its_own_episode() {
        let commits = vec![
            commit("a1", "pi-agent", 0, "feat: one", &["a"]),
            commit("h1", "daniel", 5, "fix: hand edit", &["b"]),
            commit("a2", "pi-agent", 6, "feat: two", &["c"]),
        ];
        let episodes = segment(&commits, &Rules::default());
        assert_eq!(episodes.len(), 3);
        assert_eq!(episodes[1].author_kind, AuthorKind::Human);
        assert_eq!(episodes[1].rule, Rule::Author);
    }

    #[test]
    fn a_long_gap_splits_even_the_same_author() {
        let commits = vec![
            commit("a1", "pi-agent", 0, "feat: one", &["a"]),
            commit("a2", "pi-agent", 120, "feat: two", &["a"]),
        ];
        let episodes = segment(&commits, &Rules::default());
        assert_eq!(episodes.len(), 2);
        assert_eq!(episodes[1].rule, Rule::TimeGap);
        assert!((episodes[1].confidence - 0.6).abs() < f64::EPSILON);
    }

    #[test]
    fn a_turn_id_is_qualified_before_any_rule_compares_it() {
        // ADR-003 §18.2 **D4**. Two transports hand rung 2 and rung 3 a bare index; both have to be
        // qualified in the same place, or the defect comes back through whichever one nobody fixed.
        assert_eq!(
            qualify_turn(Some("s1"), Some("3")),
            Some("s1:3".to_owned()),
            "the trailer transport's `Agent-Turn: 3` with `Agent-Session: s1`"
        );
        assert_eq!(
            qualify_turn(Some("s1"), Some("s2:3")),
            Some("s2:3".to_owned()),
            "an id that already carries a session is left alone — the writer qualifies at the source"
        );
        assert_eq!(
            qualify_turn(None, Some("3")),
            Some("3".to_owned()),
            "no session anywhere means no qualification: inventing one would be inventing evidence"
        );
        assert_eq!(qualify_turn(Some("s1"), None), None);
        assert_eq!(qualify_turn(Some("s1"), Some("  ")), None);

        // The event transport's allocator and this join must not grow two opinions of the same id.
        assert_eq!(
            qualify_turn(Some("s1"), Some("3")),
            Some(crate::hooks::qualify_turn_id("s1", 3))
        );

        // The trailer path: same author, same minute, both claiming turn 3 of a different session.
        let with = |session: &str, paths: &[&str]| CommitFacts {
            oid: format!("c-{session}"),
            author_name: "pi-agent".to_owned(),
            author_email: "pi-agent@example.invalid".to_owned(),
            author_utc: 0,
            subject: "feat: a turn".to_owned(),
            trailers: BTreeMap::from([
                (TRAILER_SESSION_KEY.to_owned(), session.to_owned()),
                (TRAILER_TURN_KEY.to_owned(), "3".to_owned()),
            ]),
            paths: paths.iter().map(|p| (*p).to_owned()).collect(),
            n_files: paths.len() as i64,
            ..CommitFacts::default()
        };
        let (starts, rule, confidence) = decide(
            &with("s1", &["src/a.rs"]),
            &with("s2", &["src/b.rs"]),
            &Rules::default(),
        );
        assert!(starts, "two sessions' turn 3 are two turns");
        assert_eq!(rule, Rule::Trailer);
        assert!((confidence - 0.9).abs() < f64::EPSILON);
        let (keeps, _, _) = decide(
            &with("s1", &["src/a.rs"]),
            &with("s1", &["src/b.rs"]),
            &Rules::default(),
        );
        assert!(
            !keeps,
            "the same session's turn 3 is the same turn whatever it touched"
        );
    }

    #[test]
    fn two_commits_claiming_one_ticket_are_one_episode() {
        // ADR-003 §4.2 rung 6, §18.2 **D1** (`reco-quk.2`). A loop's history: every commit leads with the
        // ticket it is working on. Two commits naming one ticket are one turn however few files they share;
        // the next ticket is the next episode, minutes later and without the clock ever being consulted.
        let claim = |oid: &str, minute: i64, subject: &str, paths: &[&str]| CommitFacts {
            oid: oid.to_owned(),
            author_name: "pi-agent".to_owned(),
            author_email: "pi-agent@example.invalid".to_owned(),
            author_utc: minute * 60_000,
            subject: subject.to_owned(),
            paths: paths.iter().map(|p| (*p).to_owned()).collect(),
            n_files: paths.len() as i64,
            ..CommitFacts::default()
        };
        let commits = vec![
            claim("c1", 0, "reco-e61: the reader", &["src/reader.rs"]),
            claim(
                "c2",
                4,
                "reco-e61: (part 2) the reader",
                &["src/reader_cache.rs"],
            ),
            claim("c3", 8, "reco-e62: the writer", &["src/writer.rs"]),
        ];
        let episodes = segment(&commits, &Rules::default());
        assert_eq!(
            episodes.len(),
            2,
            "one ticket, one episode; the next ticket, the next one: {episodes:?}"
        );
        assert_eq!(episodes[0].n_commits(), 2);
        let second = &episodes[1];
        assert_eq!(second.rule, Rule::SubjectClaim);
        assert!(
            (second.confidence - 0.7).abs() < f64::EPSILON,
            "a claim is priced 0.7 — above the clock, below every declared rung"
        );
        assert_eq!(
            second.rule.as_str(),
            "message-shape",
            "and it *stores* message-shape"
        );
    }

    #[test]
    fn a_token_on_one_side_only_is_no_opinion() {
        // TB6, and the reason the rung costs nothing on the corpus's 60 within-turn adjacencies: a claim
        // about one commit is not a disagreement between two.
        let plain = CommitFacts {
            oid: "c1".to_owned(),
            author_name: "pi-agent".to_owned(),
            author_email: "pi-agent@example.invalid".to_owned(),
            author_utc: 0,
            subject: "the reader".to_owned(),
            paths: vec!["src/reader.rs".to_owned()],
            ..CommitFacts::default()
        };
        let claimed = CommitFacts {
            subject: "reco-e61: the reader".to_owned(),
            ..plain.clone()
        };
        assert_eq!(claim_opinion(&plain.subject, &claimed.subject), None);
        assert_eq!(claim_opinion(&claimed.subject, &plain.subject), None);
        assert_eq!(claim_opinion("reco-a: x", "reco-b: y"), Some(true));
        assert_eq!(claim_opinion("reco-a: x", "reco-a: y"), Some(false));
    }

    #[test]
    fn subject_claim_reads_a_leading_token_and_nothing_else() {
        // The table ADR-003 §18.2 D1 measured, and the one `src/recommender/segment.py::subject_claim`
        // is run against by `test_the_subject_claim_rule_is_one_rule_in_two_languages`. The false-positive
        // half is the load-bearing half: a rung that reads `chore:` or `contracts.yml` as a ticket would
        // split one turn's commits, which is the direction that hides work rather than showing it.
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
            assert_eq!(
                subject_claim(subject).as_deref(),
                *expected,
                "subject_claim({subject:?})"
            );
        }
    }

    #[test]
    fn a_rebase_preserves_identity_and_the_weaker_digests_cover_what_it_cannot() {
        // Rebase: same author, same subject, same paths — the strong digest is unchanged, so the episode
        // id is unchanged and the review state is found again without a relink step existing.
        let original = identity(
            "pi-agent@example.invalid",
            "feat: make the store epoch safe",
            &["src/a.rs"],
        );
        let rebased = identity(
            "pi-agent@example.invalid",
            "feat: make the store epoch safe",
            &["src/a.rs"],
        );
        assert_eq!(rebased.fingerprint, original.fingerprint);
        // An amend that adds a file: the strong digest moves, and the subject-only digest is what survives
        // it — that is the fallback's whole reason for existing.
        let amended = identity(
            "pi-agent@example.invalid",
            "feat: make the store epoch safe",
            &["src/a.rs", "src/b.rs"],
        );
        assert_ne!(amended.fingerprint, original.fingerprint);
        assert_eq!(
            amended.fallback_fingerprint, original.fallback_fingerprint,
            "the fallback digest ignores paths, which is what survives the amend"
        );
        // A reword: the mirror image, and the case the subject-only digest cannot see at all.
        let reworded = identity(
            "pi-agent@example.invalid",
            "feat: make the store safe by epoch",
            &["src/a.rs"],
        );
        assert_ne!(reworded.fingerprint, original.fingerprint);
        assert_ne!(reworded.fallback_fingerprint, original.fallback_fingerprint);
        assert_eq!(
            reworded.shape_fingerprint, original.shape_fingerprint,
            "so the third digest is over author and paths, which is what a reword leaves alone"
        );
    }

    #[test]
    fn identity_matches_the_published_golden_vector() {
        // ADR-003 Appendix A. The framing (namespace, NUL separators, author-then-subject-then-sorted-
        // paths) is the identity contract, and two implementations — this one and
        // `src/recommender/segment.py` — must agree with the published bytes rather than with each
        // other's source. If this test fails, *nothing* here has been misread: it means an id already
        // issued by a user's store no longer resolves, which is the §2.4 namespace bump.
        let vector = identity(
            "dev@example.invalid",
            "add the epoch column",
            &["src/b.rs", "src/a.rs", "src/a.rs"],
        );
        assert_eq!(
            vector.fingerprint, "9461ceafd7f5d52996f6d47a058857ed874a520a56616fca89f416cc7694bda7",
            "strong digest: author + normalised subject + sorted path set"
        );
        assert_eq!(
            vector.fallback_fingerprint,
            "603dca60ea5b444ea8133b587c9804bc5d83d2dc6fd848173a90c1ddc36d6124",
            "relink tier 1's key: author + subject only"
        );
        assert_eq!(
            vector.shape_fingerprint,
            "c3115f2777576a3da22bc546ca75a58c9fd3679405ed02f6cd6d6b5990a54b02",
            "relink tier 2's key: author + path set only"
        );
        assert_eq!(episode_id(&vector.fingerprint), "ep-9461ceafd7f5d52996f6");
        // Vector 2: the normalisation is part of the vector, not an implementation detail — the fixup
        // prefix and the conventional type are stripped to a fixed point before hashing.
        assert_eq!(
            identity(
                "dev@example.invalid",
                "fixup! feat(store): Make Epochs  SAFE",
                &["src/store.rs"],
            )
            .fingerprint,
            "6a045309070f68ff6f8c5ae912da2ff12d2eef6ae99bbf841810653a0bbb3f69"
        );
    }

    #[test]
    fn a_cherry_pick_is_a_second_episode_that_shares_the_digest_not_the_id() {
        // ADR-003 §5.9. `git cherry-pick` preserves the author, the subject and the paths, so the pick
        // and its source hash identically — which is the one case where the digest is *too* equal, and
        // TB10 is the answer: walk order keeps the bare id, the later twin gets `-2`, and the two stay
        // separate episodes with separate review state, because a sign-off on `main` is not a sign-off on
        // the release branch.
        let commits = vec![
            commit("c1", "dev", 0, "fix: the crash", &["src/a.rs"]),
            commit("c2", "dev", 60, "fix: the crash", &["src/a.rs"]),
        ];
        let episodes = segment(&commits, &Rules::default());
        assert_eq!(episodes.len(), 2, "two lands, two reviews");
        assert_eq!(
            episodes[0].fingerprint, episodes[1].fingerprint,
            "the same change, so the same digest"
        );
        assert_eq!(episodes[0].id, "ep-82b50d5356e8f59a3473", "vector 3");
        assert_eq!(
            episodes[1].id,
            format!("{}-2", episodes[0].id),
            "the twin is disambiguated, not merged"
        );
        assert_eq!(episodes[1].rule, Rule::TimeGap);
    }

    #[test]
    fn a_fixup_subject_normalises_to_its_target() {
        assert_eq!(
            normalize_subject("fixup! feat(store): make epochs safe"),
            normalize_subject("feat(store): make epochs safe")
        );
    }

    #[test]
    fn a_forced_split_makes_a_boundary_and_says_so() {
        let commits = vec![
            commit("a1", "pi-agent", 0, "feat: one", &["a"]),
            commit("a2", "pi-agent", 1, "feat: two", &["b"]),
        ];
        let rules = Rules {
            force_split: vec![Override {
                oid: Some("a2".to_owned()),
                subject: None,
            }],
            ..Rules::default()
        };
        let episodes = segment(&commits, &rules);
        assert_eq!(episodes.len(), 2);
        assert_eq!(episodes[1].rule, Rule::Override);
        assert!(episodes[1].overridden);
    }

    #[test]
    fn a_merge_is_its_own_episode() {
        let mut merge = commit(
            "m1",
            "pi-agent",
            30,
            "Merge branch 'epoch' into main",
            &["a"],
        );
        merge.parents = vec!["a1".to_owned(), "b1".to_owned()];
        let commits = vec![commit("a1", "pi-agent", 0, "feat: one", &["a"]), merge];
        let episodes = segment(&commits, &Rules::default());
        assert_eq!(episodes.len(), 2);
        assert_eq!(episodes[1].rule, Rule::Branch);
    }

    #[test]
    fn scoring_a_perfect_segmentation_is_one() {
        let commits = vec![
            commit("a1", "pi-agent", 0, "feat: one", &["a"]),
            commit("a2", "pi-agent", 1, "fix: one follow-up", &["a"]),
            commit("b1", "daniel", 90, "docs: readme", &["README.md"]),
        ];
        let episodes = segment(&commits, &Rules::default());
        let score = score(
            &commits,
            &episodes,
            &["t1".to_owned(), "t1".to_owned(), "t2".to_owned()],
        );
        assert!((score.precision - 1.0).abs() < f64::EPSILON);
        assert!((score.recall - 1.0).abs() < f64::EPSILON);
    }
}
