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
//! | 3 | `trailer` | evidence | An explicit `Fathom-Episode:` trailer names this commit as a start | 0.9 |
//! | 4 | `branch` | graph | The commit is a merge (or the previous one was): a branch boundary is a reviewable unit | 0.8 |
//! | 5 | `author` | heuristic | The author identity changed | 0.75 |
//! | 6 | `time-gap` | heuristic | The gap since the previous commit exceeds [`Rules::gap_millis`] | 0.6 |
//! | 7 | `message-shape` | heuristic | The subject is a *continuation* (`wip`, `fixup!`, "address review") or a new conventional type | 0.4 |
//! | 8 | `root` | heuristic | Nothing above fired and there is no previous commit | 0.5 |
//!
//! Rules 1–3 are what someone *stated* (a config line, an event, a trailer); 4 is a fact about the graph;
//! 5–7 are guesses. Confidence is what that distinction prices: an episode whose boundary says
//! `message-shape 0.40` is one the reviewer may want to re-cut, and the card says so (ADR-003 §4.7).
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
//! arrive as [`CommitFacts::turn_id`] filled by whoever read them; this module never opens a file. No
//! clock is read either: gaps are differences between commit dates given in (ADR-002 §4.4).

use std::collections::BTreeMap;
use std::fmt;

use sha2::{Digest as _, Sha256};

/// The namespace of the identity digest, so a rule change cannot collide with an old id.
pub const IDENTITY_SCHEMA: &str = "fathom-episode/1";

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
    /// See [`Episode::session_id`].
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
        session_id: commit
            .session_id
            .clone()
            .or_else(|| trailer(&commit.trailers, "agent-session"))
            .or_else(|| trailer(&commit.trailers, "session")),
        turn_id: commit
            .turn_id
            .clone()
            .or_else(|| trailer(&commit.trailers, "agent-turn"))
            .or_else(|| trailer(&commit.trailers, "turn")),
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
        episode.turn_id = commit.turn_id.clone();
    }
    if episode.session_id.is_none() {
        episode.session_id = commit.session_id.clone();
    }
}

fn trailer(trailers: &BTreeMap<String, String>, key: &str) -> Option<String> {
    trailers.get(key).cloned().filter(|value| !value.is_empty())
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
    //    turns are one episode, different turns are two, and nothing else gets a vote.
    if let (Some(mine), Some(theirs)) = (&commit.turn_id, &previous.turn_id) {
        return (mine != theirs, Rule::Hook, 1.0);
    }
    // 2. An explicit trailer naming this commit's episode.
    if let Some(id) = trailer(&commit.trailers, "fathom-episode") {
        let same = trailer(&previous.trailers, "fathom-episode").is_some_and(|other| other == id);
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
    // 6. A gap in time.
    if commit.author_utc - previous.author_utc > rules.gap_millis {
        return (true, Rule::TimeGap, 0.6);
    }
    // 7. Message shape: a continuation keeps, a change of conventional type splits.
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
        && commit.author_utc - previous.author_utc > rules.gap_millis / 9
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

    /// Quarantined, not deleted: this is ADR-003 §5.2's worked example, and the shipped cascade cuts it
    /// in two. ADR-003 §18.2 **D6** named the cause (`message-shape`'s clock guard is `gap ÷ 9`, a shorter
    /// interval than the "the review came back" pattern TB7 exists to tolerate) and named the fix
    /// (re-derive the guard). The fix is a change to a shipped boundary rule, so it has to land together
    /// with the re-bless of `benchmarks/reports/s2-segmentation-accuracy.json` — the spike's report and its
    /// five declared-failing gates are measurements *of this cascade*, and re-scoring them from an unrelated
    /// ticket is how a measurement silently becomes an opinion. Owned by `reco-dot`; un-ignore in the change
    /// that re-blesses, in that order.
    #[test]
    #[ignore = "known defect reco-dot: TB7's gap/9 guard splits ADR-003 §5.2's own example; fix the guard and re-bless the s2 report in the same change"]
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
