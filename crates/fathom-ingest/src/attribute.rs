//! `attribute.rs` — who wrote this commit: `agent`, `human` or `unknown`, with the evidence that says so
//! (`e4.1`, `reco-w7q`).
//!
//! ADR-003 §8 **H5** splits this seam on purpose: ADR-003 owns the *use of author identity in
//! segmentation* (the `author` boundary rung, which compares a raw identity and is untouched by anything
//! here), and this module owns attribution proper — the marker table, the registry of known agents, the
//! evidence list and the corrections. Nothing here changes [`crate::segment::CommitFacts::author_identity`],
//! so no episode id moves (§2.4's namespace stays put).
//!
//! # The ladder (ADR-003 §8 H4, extended)
//!
//! Each rung below is one class of evidence, one confidence, and one thing it can assert. The first rung
//! with an opinion decides the kind, exactly like the boundary cascade: nothing is blended, one verdict is
//! returned, and every rung that fired is kept in [`Attribution::evidence`] so the card can show its work.
//! "A guess that says which rule made it is debuggable" is the same argument ADR-003 §4 makes about
//! boundaries, applied to authorship.
//!
//! | # | Source | Class | Fires when | Asserts | Confidence |
//! |---|---|---|---|---|---|
//! | 1 | `correction` | human | a stored correction names one of this commit's identities | as corrected | 1.00 |
//! | 2 | `harness` | evidence | an ADR-005 turn or session names this commit | agent | 0.95 |
//! | 3 | `trailer` | evidence | a claim trailer (`Co-authored-by`, `Generated-by`, `Generated-with`, `Assisted-by`, `Agent`, …) names an agent, a bot, or a registry entry | agent | 0.95 |
//! | 4 | `declared` | evidence | any identity is in `attribution.agents` — the registry, wherever the identity appears | agent | 0.95 |
//! | 5 | `author` | heuristic | the author string is agent-shaped: a marker in the name or address-local, or git's own `[bot]` convention | agent | 0.85 |
//! | 6 | `committer` | heuristic | the *committer* is agent-shaped while nothing above fired — a bot applied a human-authored change | agent | 0.70 |
//! | 7 | `message` | heuristic | the subject states it: `🤖 Generated with …`, `(aider)`, "via Copilot" | agent | 0.65 |
//! | 8 | `identity` | heuristic | there is a real identity and no agent evidence at all | human | 0.50 |
//! | — | `absent` | — | no author and no committer identity anywhere | unknown | 0.00 |
//!
//! Rows 2–4 are things someone *stated*; 5–7 are Fathom reading shapes; 8 is the absence of evidence
//! named honestly. 0.95 for trailer evidence, 0.85 for an agent-sounding author string, 0.50 for "a real
//! identity with no agent evidence" and 0.00 for "no identity at all" are ADR-003 §8 **H4**'s published
//! prices and are not re-guessed here; rungs 1, 6 and 7 are the ones `e4.1` adds, and they are placed
//! where the same argument puts their neighbours — a stated thing above a shaped thing, a shaped identity
//! above a shaped message.
//!
//! # `unknown` is a verdict, not a shrug
//!
//! The ticket's rule is *never guess*, so these four cases return [`AuthorKind::Unknown`] with a
//! [`Attribution::reason`] that names the competing evidence rather than a bucket nobody chose:
//!
//! | Reason | When |
//! |---|---|
//! | no identity | neither an author nor a committer string exists to attribute |
//! | corrections disagree | two stored corrections match this commit and say different things |
//! | marker in a domain | the only agent evidence is a marker that matched inside an address's *domain* (`jane@assistant.fr` is a person at a hosting name, not an assistant) |
//! | below the bar | the strongest evidence is weaker than `attribution.ambiguous_below`, which a repository can raise if it would rather see `?` than a weak claim |
//!
//! Two *agents* both claiming a commit is deliberately not one of these: the kind ("an agent did this") is
//! not in doubt, only which one, so the kind stands and [`Attribution::agent`] is left unset with a reason
//! saying which two claimed it. Not guessing about one question does not license guessing about the next.
//!
//! # What is deliberately not claimed
//!
//! * **Per-file co-authorship.** ADR-003 §8 **H5** names it as this ticket's, and it is not here: a file's
//!   author is not a fact git records, and inferring it from hunks would be a guess wearing a fact's
//!   clothes. What is here instead is the honest version — when an episode's members disagree, the reason
//!   says `2 of 3 commits carry agent evidence` and nothing is averaged away.
//! * **Prose footers.** A harness that writes "Generated with …" as free text rather than as a trailer is
//!   invisible here, because the store keeps the message *head* and its digest, not its body. That is a
//!   stated limit with a remedy, not a silent gap: ADR-005's turn evidence and a trailer are the two
//!   declared channels, and rung 2 and rung 3 read both.
//! * **Alias resolution** (one human, three addresses) is *not* done: it changes episode identity, so
//!   ADR-003 §2.6 requires an `IDENTITY_SCHEMA` bump, and that is a separate decision with a separate
//!   cost. Corrections are the mechanism a user has today: they move the label, never the id.
//! * **The clock.** Like every other analyzer, this module reads no clock and opens no file: the registry,
//!   the marker table and the corrections arrive in [`Policy`] as values (`ADR-002 §4.5`).

use std::collections::BTreeSet;

use crate::segment::CommitFacts;

use serde::{Deserialize, Serialize};

/// The confidence a stored user correction carries: a human said who wrote it.
pub const CONFIDENCE_CORRECTION: f64 = 1.0;
/// ADR-003 §8 H4: trailer evidence that names an agent.
pub const CONFIDENCE_TRAILER: f64 = 0.95;
/// ADR-003 §8 H4: an agent-sounding author string.
pub const CONFIDENCE_AUTHOR: f64 = 0.85;
/// A bot applied a change an agent-shaped author did not write.
pub const CONFIDENCE_COMMITTER: f64 = 0.70;
/// The subject line states it (`🤖 Generated with …`).
pub const CONFIDENCE_MESSAGE: f64 = 0.65;
/// ADR-003 §8 H4: a real identity with no agent evidence — "probably human, do not bet on it".
pub const CONFIDENCE_IDENTITY: f64 = 0.50;
/// ADR-003 §8 H4: no identity at all.
pub const CONFIDENCE_ABSENT: f64 = 0.0;

/// The marker table ADR-003 §4.6 publishes as a `Rules` value: substrings that make a name or address
/// smell like an agent. Configurable through `attribution.markers`, which *extends* this list rather than
/// replacing it, so a repository naming a new harness does not lose the thirteen built-ins.
pub const DEFAULT_MARKERS: [&str; 13] = [
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
];

/// The registry of known agents (`attribution.agents.<id>`).
///
/// A registry entry is *declared* evidence: the user named this identity, so a hit is priced with the
/// trailer rung (0.95) wherever it is found — author, committer, or the value of a claim trailer. The
/// patterns are lower-cased substrings of a name or an address, so one entry can cover the several shapes
/// one product signs with (`claude` the name, `noreply@anthropic.com` the address).
pub const DEFAULT_AGENTS: [(&str, &[&str]); 13] = [
    ("claude", &["claude", "noreply@anthropic.com"]),
    ("copilot", &["copilot", "copilot-swe-agent"]),
    ("codex", &["codex", "openai-codex"]),
    ("cursor", &["cursor", "cursoragent"]),
    ("devin", &["devin"]),
    ("aider", &["aider"]),
    ("openhands", &["openhands", "all-hands"]),
    ("swe-agent", &["swe-agent", "swe agent"]),
    ("gemini", &["gemini"]),
    ("dependabot", &["dependabot"]),
    ("renovate", &["renovate"]),
    ("github-actions", &["github-actions"]),
    ("autofix", &["autofix"]),
];

/// The registry entries that name a **bumping service** rather than an authoring agent.
///
/// The distinction is not decorative and it is not a vibe: Dependabot, Renovate and the `github-actions`
/// bot exist to move a version pin on a schedule, and the whole of the diff they write *is* that pin.
/// ADR-001 §7.3 prices an unreviewed program's work highly because the reviewer has to read logic nobody
/// reasoned about; a resolver's bump has no logic in it, and charging it the CI or migration claim —
/// "an operator must act on it" — is the single largest reason a bot's routine led the review queue in
/// the Guardrail 3 corpus (`reco-6co`). `fathom-analyze`'s risk collector reads this list to cap that
/// claim at the dependency surface; a declared
/// authoring agent (`claude`, `copilot`, `codex`, …) is untouched by it, because that is the work the
/// thesis is about.
///
/// Names are the same strings as [`DEFAULT_AGENTS`] entries, matched the same way: a lower-cased
/// substring of the registry id the attribution wrote into `episode.attribution_agent`.
pub const RESOLVER_AGENTS: [&str; 3] = ["dependabot", "renovate", "github-actions"];

/// Is this attributed agent one of the bumping services in [`RESOLVER_AGENTS`]?
#[must_use]
pub fn is_resolver_agent(agent: &str) -> bool {
    let agent = agent.trim().to_ascii_lowercase();
    !agent.is_empty()
        && RESOLVER_AGENTS
            .iter()
            .any(|resolver| agent.contains(resolver))
}

/// The trailer keys whose *values* are read for an agent name (rung 3).
///
/// The key alone is never evidence: `Signed-off-by:` is the DCO line a thousand humans sign, and
/// `Co-authored-by:` is the line a human pairs with another human. What prices the rung is the value
/// naming an agent, a bot, or a registry entry — which is also why `Signed-off-by:
/// github-actions[bot] <…>` counts and `Signed-off-by: Taylor <taylor@example.com>` does not.
pub const CLAIM_TRAILERS: [&str; 12] = [
    "co-authored-by",
    "assisted-by",
    "generated-by",
    "generated-with",
    "generated-with-by",
    "agent",
    "agent-via",
    "signed-off-by",
    "reviewed-by",
    "tested-by",
    "reported-by",
    "helped-by",
];

/// The subject-line phrases that state authorship when they name an agent (`🤖` alone states it too).
pub const MESSAGE_PHRASES: [&str; 4] = ["generated with", "generated by", "co-authored", "via "];

/// How an author is classified.
///
/// The vocabulary is ADR-003 Appendix B's (`episode.author_kind`) and is closed: `unknown` is a terminal
/// value a reviewer acts on, not a gap in the data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthorKind {
    /// Written by a coding agent (or by a machine that is not a person at all — see the note in the
    /// module doc: `fathom-fixtures`' `Kind::Bot` is an `agent` label here, because the review question
    /// "did a machine write this" is the one this column answers).
    Agent,
    /// Written by a person.
    Human,
    /// Ambiguous, or nothing to go on — printed as `?`, never silently bucketed.
    Unknown,
}

impl AuthorKind {
    /// The store's and the wire's spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            AuthorKind::Agent => "agent",
            AuthorKind::Human => "human",
            AuthorKind::Unknown => "unknown",
        }
    }

    /// The card's spelling: `?` for `unknown`, because a weak claim and no claim must not look alike.
    #[must_use]
    pub const fn glyph(self) -> &'static str {
        match self {
            AuthorKind::Agent => "agent",
            AuthorKind::Human => "human",
            AuthorKind::Unknown => "?",
        }
    }

    /// Read the spelling back. `None` for anything else, so a caller decides what an unknown word means
    /// rather than this function guessing.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "agent" => Some(AuthorKind::Agent),
            "human" => Some(AuthorKind::Human),
            "unknown" => Some(AuthorKind::Unknown),
            _ => None,
        }
    }
}

impl std::fmt::Display for AuthorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which class of evidence an [`Evidence`] item is. The strings are the ones the store's JSON and the
/// delta document print, so they are a vocabulary, not a debug label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    /// A correction a user stored (`fathom attribution correct`).
    Correction,
    /// An ADR-005 harness turn or session named this commit.
    Harness,
    /// A claim trailer named an agent.
    Trailer,
    /// `attribution.agents` named this identity, wherever it was found.
    Declared,
    /// The author string is agent-shaped.
    Author,
    /// The committer string is agent-shaped.
    Committer,
    /// The subject line says so.
    Message,
    /// There is an identity and no agent evidence — the absence, priced.
    Identity,
    /// There is no identity at all.
    Absent,
}

impl Source {
    /// The wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Source::Correction => "correction",
            Source::Harness => "harness",
            Source::Trailer => "trailer",
            Source::Declared => "declared",
            Source::Author => "author",
            Source::Committer => "committer",
            Source::Message => "message",
            Source::Identity => "identity",
            Source::Absent => "absent",
        }
    }
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One thing the classifier saw.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    /// See [`Source`].
    pub source: Source,
    /// The literal, in the commit's own spelling: `co-authored-by: Claude <noreply@anthropic.com>`.
    pub detail: String,
    /// What this item alone would have priced the verdict at.
    pub confidence: f64,
}

impl Evidence {
    fn new(source: Source, detail: impl Into<String>, confidence: f64) -> Self {
        Evidence {
            source,
            detail: detail.into(),
            confidence,
        }
    }
}

/// The verdict for one episode: kind, agent, confidence, evidence, and a reason when the answer is
/// `unknown` or when something was *not* guessed that a lazier classifier would have guessed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attribution {
    /// See [`AuthorKind`].
    pub kind: AuthorKind,
    /// Which agent, when the evidence names one and names only one.
    pub agent: Option<String>,
    /// How far to trust [`Attribution::kind`]. `0.0` means "nothing was known", not "nothing matters".
    pub confidence: f64,
    /// Everything that fired, strongest first. Capped by the writer, not by this function.
    pub evidence: Vec<Evidence>,
    /// Why this is `unknown`, or why an obvious-looking label was *not* given. `None` when the answer
    /// was simply the answer.
    pub reason: Option<String>,
}

impl Attribution {
    /// The one-line form the card and `fathom attribution` print: `agent · claude · 0.95`.
    #[must_use]
    pub fn summary(&self) -> String {
        let agent = self.agent.as_deref().unwrap_or("—");
        format!("{} · {agent} · {:.2}", self.kind.glyph(), self.confidence)
    }

    /// The evidence as the single line the card prints under the summary.
    #[must_use]
    pub fn evidence_line(&self) -> String {
        if self.evidence.is_empty() {
            return "(no evidence)".to_owned();
        }
        self.evidence
            .iter()
            .map(|item| format!("{} {:.2} — {}", item.source, item.confidence, item.detail))
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// The evidence as the store's JSON, in the order given.
    ///
    /// `#[serde]` over the struct gives the same bytes for the same verdict, which is what lets the store
    /// write it and a test diff it.
    #[must_use]
    pub fn evidence_json(&self) -> String {
        serde_json::to_string(&self.evidence).unwrap_or_else(|_| "[]".to_owned())
    }

    /// Parse back what [`Attribution::evidence_json`] wrote. Bad JSON is no evidence, not a panic: a
    /// store row written by a newer build must still render.
    #[must_use]
    pub fn evidence_from_json(text: &str) -> Vec<Evidence> {
        serde_json::from_str(text).unwrap_or_default()
    }
}

/// One registry entry: an id a human chose, and the identity patterns that mean it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    /// The id printed as [`Attribution::agent`].
    pub id: String,
    /// Lower-cased substrings of a name or an address.
    pub patterns: Vec<String>,
}

/// One stored user correction: "this identity is a human / an agent".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Correction {
    /// The identity key (see [`identity_key`]) the user corrected.
    pub identity: String,
    /// The kind to give it.
    pub kind: AuthorKind,
    /// The agent to name, when `kind` is [`AuthorKind::Agent`].
    pub agent: Option<String>,
}

/// The attribution rule values: everything [`attribute`] reads that a repository can disagree with.
///
/// Like [`crate::segment::Rules`], this arrives as a value — `fathom-cli` reads `fathom.toml` and the
/// store reads its corrections table, and nothing below the CLI opens either (ADR-002 §4.5).
#[derive(Debug, Clone)]
pub struct Policy {
    /// The marker table: [`DEFAULT_MARKERS`] plus `attribution.markers`.
    pub markers: Vec<String>,
    /// The registry: [`DEFAULT_AGENTS`] plus `attribution.agents.*`.
    pub agents: Vec<Agent>,
    /// Whether the committer counts as evidence (rung 6). On by default; a repository whose CI commits
    /// everything can turn it off rather than watch every episode read `agent`.
    pub trust_committer: bool,
    /// Below this confidence the verdict is `unknown` with the reason printed. The built-in 0.50 prices
    /// rung 8 (`identity`) exactly at the bar, so nothing is demoted until a repository asks for it.
    pub ambiguous_below: f64,
    /// The corrections the store held for this run.
    pub corrections: Vec<Correction>,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            markers: DEFAULT_MARKERS.iter().map(|s| (*s).to_owned()).collect(),
            agents: DEFAULT_AGENTS
                .iter()
                .map(|(id, patterns)| Agent {
                    id: (*id).to_owned(),
                    patterns: patterns.iter().map(|p| (*p).to_owned()).collect(),
                })
                .collect(),
            trust_committer: true,
            ambiguous_below: CONFIDENCE_IDENTITY,
            corrections: Vec::new(),
        }
    }
}

impl Policy {
    /// Which registry agent, if any, an identity key or free-text value belongs to.
    ///
    /// Exact key matches are tried before substrings so an entry written as a whole address cannot be
    /// shadowed by a shorter substring from another entry.
    #[must_use]
    pub fn registry_match(&self, text: &str) -> Option<&str> {
        let text = text.trim().to_ascii_lowercase();
        if text.is_empty() {
            return None;
        }
        self.agents
            .iter()
            .find(|agent| agent.patterns.iter().any(|p| p == &text))
            .or_else(|| {
                self.agents
                    .iter()
                    .find(|agent| agent.patterns.iter().any(|p| text.contains(p.as_str())))
            })
            .map(|agent| agent.id.as_str())
    }

    /// Is this marker table empty of a word? Used by the CLI's notice path, which says out loud when a
    /// configured marker cannot ever match.
    #[must_use]
    pub fn has_marker(&self, marker: &str) -> bool {
        let needle = marker.trim().to_ascii_lowercase();
        !needle.is_empty() && self.markers.iter().any(|m| m == &needle)
    }
}

/// The stable key for "whoever this identity is".
///
/// The address when there is one — lower-cased and trimmed, the same normalisation ADR-003 §2.4 uses for
/// identity — else the name. Prefixed with `identity:` so a key printed by `fathom attribution correct`
/// is recognisable when someone pastes it into `fathom.toml`'s own lists later, and so a name that
/// happens to read `agent` cannot be confused with the vocabulary word.
#[must_use]
pub fn identity_key(name: &str, email: &str) -> Option<String> {
    let email = email.trim().to_ascii_lowercase();
    if !email.is_empty() {
        return Some(format!("identity:{email}"));
    }
    let name = name.trim().to_ascii_lowercase();
    if name.is_empty() {
        None
    } else {
        Some(format!("identity:{name}"))
    }
}

/// Split a trailer value like `Claude <noreply@anthropic.com>` into the two halves a signature is made
/// of. A value with no angle brackets is all name — which is how `Assisted-by: claude-code` arrives.
#[must_use]
pub fn split_identity(value: &str) -> (String, String) {
    let value = value.trim();
    if let Some((name, rest)) = value.split_once('<')
        && let Some((email, _)) = rest.split_once('>')
    {
        return (name.trim().to_owned(), email.trim().to_owned());
    }
    (value.to_owned(), String::new())
}

/// One identity the classifier looked at, with where it was found.
#[derive(Debug, Clone)]
struct Seen {
    key: String,
    name: String,
    email: String,
    /// The card's spelling of *this* identity: `Claude <noreply@anthropic.com>`.
    display: String,
}

impl Seen {
    fn new(name: &str, email: &str) -> Option<Self> {
        let key = identity_key(name, email)?;
        Some(Seen {
            key,
            name: name.trim().to_owned(),
            email: email.trim().to_owned(),
            display: if email.trim().is_empty() {
                name.trim().to_owned()
            } else {
                format!("{} <{}>", name.trim(), email.trim())
            },
        })
    }

    /// The part before `@`, lower-cased: `copilot-swe-agent[bot]` for a noreply address.
    fn local(&self) -> &str {
        self.email.split('@').next().unwrap_or_default()
    }

    /// The part after `@`, lower-cased. Empty when there is no address at all.
    fn domain(&self) -> &str {
        self.email.split_once('@').map_or("", |(_, domain)| domain)
    }
}

/// What one identity smells like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// A registry entry named it: declared, not inferred.
    Registered,
    /// A marker appeared in the name or the address-local part.
    MarkerInName,
    /// git's own bot convention: `[bot]` on the name or the address.
    BotSuffix,
    /// A marker appeared *only* in the address's domain, which is a hosting name and not a claim about
    /// who writes the code.
    MarkerInDomain,
    /// Nothing.
    Plain,
}

impl Shape {
    /// Whether this shape is agent evidence at all, and how much it is worth where it was found.
    ///
    /// `MarkerInDomain` is the one that is *not* evidence: it becomes the ambiguity that rung 8 must not
    /// paper over.
    fn confidence(self, source: Source) -> Option<f64> {
        match self {
            Shape::Registered => Some(CONFIDENCE_TRAILER),
            Shape::BotSuffix | Shape::MarkerInName => match source {
                Source::Trailer | Source::Harness => Some(CONFIDENCE_TRAILER),
                Source::Committer => Some(CONFIDENCE_COMMITTER),
                _ => Some(CONFIDENCE_AUTHOR),
            },
            Shape::MarkerInDomain | Shape::Plain => None,
        }
    }
}

/// How an identity reads against the registry and the marker table.
#[must_use]
fn shape_of(seen: &Seen, policy: &Policy) -> Shape {
    if policy.registry_match(&seen.key).is_some()
        || policy.registry_match(&seen.name).is_some()
        || (!seen.email.is_empty() && policy.registry_match(&seen.email).is_some())
    {
        return Shape::Registered;
    }
    let name = seen.name.to_ascii_lowercase();
    let local = seen.local().to_ascii_lowercase();
    let domain = seen.domain().to_ascii_lowercase();
    if name.contains("[bot]") || local.contains("[bot]") || name.ends_with(" bot") {
        return Shape::BotSuffix;
    }
    let mut domain_only = false;
    for marker in &policy.markers {
        let needle = marker.to_ascii_lowercase();
        if needle.is_empty() {
            continue;
        }
        if name.contains(&needle) || local.contains(&needle) {
            return Shape::MarkerInName;
        }
        if domain.contains(&needle) {
            domain_only = true;
        }
    }
    if domain_only {
        Shape::MarkerInDomain
    } else {
        Shape::Plain
    }
}

/// One claim: "this evidence says the kind, at this confidence, and names this agent if it names one".
#[derive(Debug, Clone)]
struct Claim {
    confidence: f64,
    kind: AuthorKind,
    agent: Option<String>,
    evidence: Evidence,
}

/// Attribute one commit.
#[must_use]
pub fn attribute_one(commit: &CommitFacts, policy: &Policy) -> Attribution {
    decide(std::slice::from_ref(commit), policy, false)
}

/// Attribute an episode: the union of its members' evidence, decided by the strongest rung that fired.
///
/// Why the episode and not just its first commit: a turn whose third commit carries
/// `Co-authored-by: Claude` is an agent turn even though the first two said nothing, and the reviewer
/// who is told `human 0.50` because the classifier only looked at the head of the run is being lied to.
/// Why the *strongest* and not a vote: ADR-003's whole argument about boundaries is that stated evidence
/// outranks inference, and three quiet commits do not outvote one statement.
#[must_use]
pub fn attribute(commits: &[CommitFacts], policy: &Policy) -> Attribution {
    decide(commits, policy, true)
}

fn decide(commits: &[CommitFacts], policy: &Policy, per_commit: bool) -> Attribution {
    let mut claims: Vec<Claim> = Vec::new();
    let mut weak: Vec<String> = Vec::new();
    let mut any_identity = false;
    let multi = commits.len() > 1;

    for commit in commits {
        let stamp = if per_commit && multi {
            let short: String = commit.oid.chars().take(7).collect();
            format!("{short}: ")
        } else {
            String::new()
        };
        let mut seen: Vec<Seen> = Vec::new();
        let push_seen = |seen: &mut Vec<Seen>, candidate: Option<Seen>| {
            if let Some(candidate) = candidate
                && !seen.iter().any(|existing| existing.key == candidate.key)
            {
                seen.push(candidate);
            }
        };
        push_seen(
            &mut seen,
            Seen::new(&commit.author_name, &commit.author_email),
        );
        push_seen(
            &mut seen,
            Seen::new(&commit.committer_name, &commit.committer_email),
        );
        if commit.trailers.contains_key("co-authored-by") {
            // `git interpret-trailers` keeps the *last* value of a repeated key, which is what
            // `Trailers` stores; a message with two `Co-authored-by:` lines therefore contributes one
            // of them. That is a stated limit of the store's column, not of this classifier, and the
            // single value it does keep is the one a `--signoff` rewrite leaves behind.
            push_seen(
                &mut seen,
                trailer_identity(&commit.trailers, "co-authored-by"),
            );
        }
        any_identity |= !seen.is_empty();

        // Rung 1 — a user's own word, which outranks every heuristic here for the same reason
        // `override` outranks the boundary cascade: it is not a guess about the commit, it is a
        // statement about the identity.
        for seen in &seen {
            if let Some(correction) = policy
                .corrections
                .iter()
                .find(|correction| correction.identity == seen.key)
            {
                claims.push(Claim {
                    confidence: CONFIDENCE_CORRECTION,
                    kind: correction.kind,
                    agent: correction.agent.clone(),
                    evidence: Evidence::new(
                        Source::Correction,
                        format!(
                            "{stamp}a stored correction says {} is {}",
                            seen.display,
                            correction.kind.as_str()
                        ),
                        CONFIDENCE_CORRECTION,
                    ),
                });
            }
        }

        // Rung 2 — the harness said so. ADR-005's turn/session evidence is a statement by the thing that
        // ran the turn, which is why it is priced with the trailers and not below them.
        if let Some(session) = commit
            .session_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            let agent = policy.registry_match(session).map(str::to_owned);
            claims.push(Claim {
                confidence: CONFIDENCE_TRAILER,
                kind: AuthorKind::Agent,
                agent,
                evidence: Evidence::new(
                    Source::Harness,
                    format!("{stamp}a harness session names this commit: {session}"),
                    CONFIDENCE_TRAILER,
                ),
            });
        } else if commit
            .turn_id
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        {
            claims.push(Claim {
                confidence: CONFIDENCE_TRAILER,
                kind: AuthorKind::Agent,
                agent: None,
                evidence: Evidence::new(
                    Source::Harness,
                    format!("{stamp}a harness turn names this commit"),
                    CONFIDENCE_TRAILER,
                ),
            });
        }

        // Rung 3 — a claim trailer whose *value* names an agent, a bot, or a registry entry.
        for key in CLAIM_TRAILERS {
            let Some(value) = commit.trailers.get(key) else {
                continue;
            };
            if value.trim().is_empty() {
                continue;
            }
            let (name, email) = split_identity(value);
            let seen = Seen::new(&name, &email);
            let (shape, probe) = match &seen {
                Some(seen) => (shape_of(seen, policy), seen.display.clone()),
                None => (Shape::Plain, value.trim().to_owned()),
            };
            let registered = policy
                .registry_match(&probe)
                .or_else(|| policy.registry_match(&name))
                .map(str::to_owned);
            let confidence = registered
                .as_ref()
                .map(|_| CONFIDENCE_TRAILER)
                .or_else(|| shape.confidence(Source::Trailer));
            if let Some(confidence) = confidence {
                claims.push(Claim {
                    confidence,
                    kind: AuthorKind::Agent,
                    agent: registered.clone().or_else(|| {
                        seen.as_ref()
                            .and_then(|s| policy.registry_match(&s.name).map(str::to_owned))
                    }),
                    evidence: Evidence::new(
                        if registered.is_some() {
                            Source::Declared
                        } else {
                            Source::Trailer
                        },
                        format!("{stamp}{key}: {value}"),
                        confidence,
                    ),
                });
            }
        }

        // Rungs 4–6 — the identities themselves: author, then committer.
        let author = Seen::new(&commit.author_name, &commit.author_email);
        if let Some(seen) = &author {
            match shape_of(seen, policy) {
                Shape::Registered => claims.push(Claim {
                    confidence: CONFIDENCE_TRAILER,
                    kind: AuthorKind::Agent,
                    agent: policy.registry_match(&seen.key).map(str::to_owned),
                    evidence: Evidence::new(
                        Source::Declared,
                        format!("{stamp}the author {} is a configured agent", seen.display),
                        CONFIDENCE_TRAILER,
                    ),
                }),
                Shape::BotSuffix | Shape::MarkerInName => claims.push(Claim {
                    confidence: CONFIDENCE_AUTHOR,
                    kind: AuthorKind::Agent,
                    agent: policy.registry_match(&seen.name).map(str::to_owned),
                    evidence: Evidence::new(
                        Source::Author,
                        format!("{stamp}the author {} reads as an agent", seen.display),
                        CONFIDENCE_AUTHOR,
                    ),
                }),
                Shape::MarkerInDomain => weak.push(format!(
                    "{stamp}the marker matched the domain in {}",
                    seen.display
                )),
                Shape::Plain => {}
            }
        }
        if policy.trust_committer
            && let Some(seen) = Seen::new(&commit.committer_name, &commit.committer_email)
        {
            {
                let shape = shape_of(&seen, policy);
                let is_author = author.as_ref().is_some_and(|a| a.key == seen.key);
                if !is_author {
                    match shape {
                        Shape::Registered => claims.push(Claim {
                            confidence: CONFIDENCE_TRAILER,
                            kind: AuthorKind::Agent,
                            agent: policy.registry_match(&seen.key).map(str::to_owned),
                            evidence: Evidence::new(
                                Source::Declared,
                                format!(
                                    "{stamp}the committer {} is a configured agent",
                                    seen.display
                                ),
                                CONFIDENCE_TRAILER,
                            ),
                        }),
                        Shape::BotSuffix | Shape::MarkerInName => claims.push(Claim {
                            confidence: CONFIDENCE_COMMITTER,
                            kind: AuthorKind::Agent,
                            agent: policy.registry_match(&seen.name).map(str::to_owned),
                            evidence: Evidence::new(
                                Source::Committer,
                                format!("{stamp}the committer {} reads as an agent", seen.display),
                                CONFIDENCE_COMMITTER,
                            ),
                        }),
                        Shape::MarkerInDomain => weak.push(format!(
                            "{stamp}the marker matched the domain in the committer {}",
                            seen.display
                        )),
                        Shape::Plain => {}
                    }
                }
            }
        }

        // Rung 7 — the subject line says it, in its own words or in the agent emoji.
        if let Some(agent) = message_claim(&commit.subject, policy) {
            let (confidence, source) = match &agent {
                Some(_) => (CONFIDENCE_TRAILER, Source::Declared),
                None => (CONFIDENCE_MESSAGE, Source::Message),
            };
            claims.push(Claim {
                confidence,
                kind: AuthorKind::Agent,
                agent,
                evidence: Evidence::new(
                    source,
                    format!("{stamp}the subject states it: {}", commit.subject),
                    confidence,
                ),
            });
        }
    }

    let mut evidence: Vec<Evidence> = claims.iter().map(|claim| claim.evidence.clone()).collect();
    evidence.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.detail.cmp(&b.detail))
    });

    // Nothing claimed anything: either there was no identity at all, or there was a person-shaped one and
    // no agent evidence, which ADR-003 §8 H4 prices at 0.50 and calls human.
    let Some(best) = claims.iter().map(|claim| claim.confidence).reduce(f64::max) else {
        if !any_identity {
            return Attribution {
                kind: AuthorKind::Unknown,
                agent: None,
                confidence: CONFIDENCE_ABSENT,
                evidence: vec![Evidence::new(
                    Source::Absent,
                    "neither an author nor a committer identity is recorded",
                    CONFIDENCE_ABSENT,
                )],
                reason: Some("no identity to attribute: nothing here says who wrote it".to_owned()),
            };
        }
        if let Some(weak) = first_weak(&weak) {
            return Attribution {
                kind: AuthorKind::Unknown,
                agent: None,
                confidence: CONFIDENCE_ABSENT,
                evidence: weak_evidence(&weak),
                reason: Some(format!(
                    "the only thing that looked like an agent was {weak}, which is not evidence of one"
                )),
            };
        }
        let identity = Attribution {
            kind: AuthorKind::Human,
            agent: None,
            confidence: CONFIDENCE_IDENTITY,
            evidence: vec![Evidence::new(
                Source::Identity,
                "an ordinary identity with no agent evidence anywhere on the episode",
                CONFIDENCE_IDENTITY,
            )],
            reason: None,
        };
        // The absence-of-evidence rung is the one a cautious repository most wants to refuse, so it
        // respects the bar too: raising `attribution.ambiguous_below` above 0.50 turns "probably human"
        // into `?` plus the reason, which is the whole point of the dial.
        if CONFIDENCE_IDENTITY < policy.ambiguous_below {
            return below_the_bar(CONFIDENCE_IDENTITY, policy, identity.evidence);
        }
        return identity;
    };

    let at_best: Vec<&Claim> = claims
        .iter()
        .filter(|claim| claim.confidence == best)
        .collect();
    let kinds: BTreeSet<&'static str> = at_best
        .iter()
        .map(|claim| claim.kind.as_str())
        .collect::<BTreeSet<_>>();

    // Two stored corrections, two answers, same rung: the user is the top of the ladder, and when the top
    // contradicts itself the only honest answer is that this is unknown until someone decides.
    if kinds.len() > 1 {
        let named = at_best
            .iter()
            .map(|claim| format!("{} ({:.2})", claim.evidence.detail, claim.confidence))
            .collect::<Vec<_>>()
            .join("; ");
        return Attribution {
            kind: AuthorKind::Unknown,
            agent: None,
            confidence: best,
            evidence,
            reason: Some(format!(
                "the evidence at the top rung disagrees — {named} — so nothing is guessed"
            )),
        };
    }

    let kind = at_best
        .first()
        .map(|claim| claim.kind)
        .unwrap_or(AuthorKind::Unknown);
    let agents: Vec<String> = {
        let mut found: Vec<String> = Vec::new();
        for claim in &claims {
            if let Some(agent) = &claim.agent
                && !found.contains(agent)
            {
                found.push(agent.clone());
            }
        }
        found
    };

    if kind == AuthorKind::Human && best < policy.ambiguous_below {
        return below_the_bar(best, policy, evidence);
    }
    if kind == AuthorKind::Unknown && best < policy.ambiguous_below {
        return below_the_bar(best, policy, evidence);
    }

    // The kind is settled; is *which agent* settled? Two registry agents both claiming the episode is an
    // agent answer with an agent-identity question mark, and the two are different columns on purpose.
    let agent = match kind {
        AuthorKind::Agent if agents.len() == 1 => Some(agents[0].clone()),
        AuthorKind::Agent if agents.len() > 1 => {
            return Attribution {
                kind,
                agent: None,
                confidence: best,
                evidence,
                reason: Some(format!(
                    "two agents claim this — {} — so neither is picked",
                    agents.join(", ")
                )),
            };
        }
        _ => None,
    };

    Attribution {
        kind,
        agent,
        confidence: best,
        evidence,
        reason: if kind == AuthorKind::Unknown {
            Some("the classifier found evidence it could not price".to_owned())
        } else {
            None
        },
    }
}

fn below_the_bar(best: f64, policy: &Policy, evidence: Vec<Evidence>) -> Attribution {
    Attribution {
        kind: AuthorKind::Unknown,
        agent: None,
        confidence: best,
        evidence,
        reason: Some(format!(
            "the strongest evidence is {:.2}, below attribution.ambiguous_below = {:.2}",
            best, policy.ambiguous_below
        )),
    }
}

fn first_weak(weak: &[String]) -> Option<String> {
    weak.first().cloned()
}

fn weak_evidence(weak: &str) -> Vec<Evidence> {
    vec![Evidence::new(
        Source::Identity,
        weak.to_owned(),
        CONFIDENCE_ABSENT,
    )]
}

/// Does the subject line state that an agent wrote this?
///
/// `Some(None)` means "the subject says so, without naming who"; `Some(Some(id))` is a named agent and is
/// priced as a declaration, because naming your agent is a statement, not a shape.
fn message_claim(subject: &str, policy: &Policy) -> Option<Option<String>> {
    let text = subject.to_ascii_lowercase();
    if text.contains('\u{1f916}') {
        return Some(policy.registry_match(subject).map(str::to_owned));
    }
    let states = MESSAGE_PHRASES.iter().any(|p| text.contains(p));
    if !states {
        return None;
    }
    let named = policy
        .agents
        .iter()
        .find(|agent| {
            agent
                .patterns
                .iter()
                .any(|pattern| text.contains(pattern.as_str()))
        })
        .map(|agent| agent.id.clone());
    if named.is_some() {
        return Some(named);
    }
    if policy
        .markers
        .iter()
        .any(|marker| !marker.is_empty() && text.contains(marker.as_str()))
    {
        return Some(None);
    }
    None
}

fn trailer_identity(
    trailers: &std::collections::BTreeMap<String, String>,
    key: &str,
) -> Option<Seen> {
    let value = trailers.get(key)?;
    let (name, email) = split_identity(value);
    Seen::new(&name, &email)
}

/// The identities a correction can key on for these commits, in stable order.
///
/// This is the ticket's *signature*: the set of identity keys a commit presents. `fathom attribution
/// correct` prints it, because a correction that keys on nothing is a correction that applies to nothing,
/// and a user who is shown the key can see the scope of what they just decided.
#[must_use]
pub fn signatures(commits: &[CommitFacts]) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    let mut push = |key: Option<String>| {
        if let Some(key) = key
            && !keys.contains(&key)
        {
            keys.push(key);
        }
    };
    for commit in commits {
        push(identity_key(&commit.author_name, &commit.author_email));
        push(identity_key(
            &commit.committer_name,
            &commit.committer_email,
        ));
        push(trailer_identity(&commit.trailers, "co-authored-by").map(|seen| seen.key));
        for key in ["assisted-by", "generated-by", "generated-with", "agent"] {
            if let Some(value) = commit.trailers.get(key) {
                let (name, email) = split_identity(value);
                push(identity_key(&name, &email));
            }
        }
    }
    keys.sort();
    keys
}

#[cfg(test)]
mod tests {
    use super::is_resolver_agent;

    /// The bots `reco-6co`'s surface cap is drawn around: a service that moves pins on a schedule, which
    /// is a different kind of change from a bot turn that wrote logic. Matched the way the registry's
    /// `match` strings are matched — case-insensitive containment over both identity fields — because the
    /// names differ per forge (`dependabot[bot]` and its noreply address are the same actor).
    #[test]
    fn pin_moving_bots_are_distinguished_from_writing_bots() {
        for who in [
            "dependabot[bot]",
            "49699333+dependabot[bot]@users.noreply.github.com",
            "Renovate Bot",
            "renovate@[1071683].bot.github.com",
            "github-actions[bot]",
        ] {
            assert!(is_resolver_agent(who), "{who} moves pins");
        }
        for who in [
            "copilot",
            "github-copilot[bot]",
            "claude",
            "github-aw[bot]",
            "bot@noreply",
            "paulus@palletsprojects.com",
            "",
        ] {
            assert!(
                !is_resolver_agent(who),
                "{who} is an author — ADR-001 §7.3 is about exactly this kind of change"
            );
        }
    }
}
