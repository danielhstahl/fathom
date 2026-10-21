//! `entry.rs` — one reviewer's decision, and the four digests that make it findable afterwards
//! (`e4.6`, `reco-rlf`).
//!
//! A sign-off is a claim about a *person* ("Dana accepted this") attached to a claim about *content*
//! ("…this change"). Everything hard about the ledger comes from keeping those two claims separate, and the
//! four digests on [`SignOff`] are the machinery for doing it:
//!
//! | Field | Digest of | Answers | Moves when |
//! |---|---|---|---|
//! | `fingerprint` | author, subject, paths (ADR-003 §2.2) | *is this the same episode?* | a reword, a squash that moved files, `--reset-author` |
//! | `fallback` | author, subject | *same episode, files moved?* | a reword |
//! | `shape` | author, paths | *same episode, subject changed?* | a file appearing or vanishing |
//! | `content` | the post-change blob of every path the episode touches | *is this the content they signed?* | **only when the content changes** |
//!
//! The fourth row is the one this ticket adds and the one ADR-003 cannot supply: its digests are *identity*
//! digests, built from the facts a rewrite preserves, and they exclude hunk text on purpose (§2.1) — so an
//! `--amend` that fixes a typo in a comment moves none of the first three and would sail through a ledger
//! that had only them. A sign-off that cannot tell "the history moved" from "the code changed" is the
//! failure the ticket names: *a stale sign-off is a bug, not a detail*.
//!
//! # The content digest, and what it deliberately ignores
//!
//! `content` is a digest over, for every path the episode touches, the *final* content of that path at the
//! end of the episode — the git blob oid the indexer recorded, or a token when the blob was not recorded.
//! Per path, last member wins, so a turn that edited one file four times is four facts folded into one.
//!
//! That choice buys the two properties a staleness digest has to have and no existing digest has:
//!
//! * **squash-robust**: three commits that churned a file back and forth and one commit that wrote the same
//!   final text have the same digest. A reviewer who signed the three-commit turn has not had the *content*
//!   changed under them when `rebase -i` flattens it, and saying otherwise would train reviewers to ignore
//!   the staleness flag — which is how a flag dies.
//! * **edit-sensitive**: one changed byte in one file is a different blob, so a content amend after
//!   sign-off is caught at the first index run afterwards, not at the audit.
//!
//! And it excludes, on purpose: line counts (a squash re-sums them), commit oids (a rewrite exists to
//! change them), author, subject and message (identity digests already claim those, and the reason there
//! are four digests is that each answers exactly one question), and the *order* of paths (a rebase reorders
//! nothing that a reviewer looked at).
//!
//! Where a blob oid was never recorded — the indexer's payload cap, or a row written by a build that stored
//! none — the digest is not computed and `content` is the word `unmeasured` rather than a value that looks
//! like a digest. `fathom review status` then reports the entry as stale with the reason "content was never
//! measured", because an unknown printed as a match is ADR-001 §7.6's failure with the label "fresh".
//!
//! # `digest`, the tamper field, and what it is not
//!
//! Every entry ends with `digest: sha256:…`, computed over the entry's own field lines in the order they
//! appear, excluding the digest line itself. Editing any value, adding a line, deleting one, or reordering
//! two of them changes the hash, so `fathom review verify` can say *this entry's text changed after it was
//! written* — the tripwire the ticket's fourth criterion asks for.
//!
//! It is a tripwire and not a signature: there is no secret in the hash, so it detects a silent edit and
//! does not defeat a determined forger who re-hashes. The signature is git — the entry is a committed line
//! in a file with a blame record — and the digest's whole job is to make the two ways a ledger actually gets
//! damaged (a rebase script that rewrites the file, a hand that "just fixes" one field) visible without
//! anyone reading every line. An honest ledger says which of the two it trusts; so does
//! [the format](crate::ledger::format).

use std::collections::BTreeSet;

use fathom_core::time::Timestamp;
use serde::Serialize;
use sha2::{Digest as _, Sha256};

/// The lowercase hex of a finished digest.
fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The framing prefix of every ledger digest. Moving it re-addresses every sign-off, so it moves only when
/// a digest's *meaning* changes (the same rule ADR-003 §2.4 sets for `IDENTITY_SCHEMA`).
pub const REVIEW_SCHEMA: &str = "fathom-review/1";

/// The framing prefix of the content digest.
pub const CONTENT_SCHEMA: &str = "fathom-review-content/1";

/// What a reviewer decided, in the ledger's own vocabulary.
///
/// The four are `ledger_entry.decision`'s CHECK list, verbatim, and they are exactly the four dispositions
/// of the attention queue that *settle* a row: `unread` and `read` are states of a queue, not acts, and a
/// ledger row that said `read` would be a record of nobody having decided anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Decision {
    /// "I accept this." The sign-off the north-star metric clocks.
    Accepted,
    /// "Look at this, someone else." Carried so the next reader sees who was asked.
    Flagged,
    /// "Not a problem, for a listed reason."
    Dismissed,
    /// "Not now, and here is what I am waiting for."
    Snoozed,
}

impl Decision {
    /// The ledger's spelling — and the store's, because there is one vocabulary (ADR-003 App. B).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Decision::Accepted => "accepted",
            Decision::Flagged => "flagged",
            Decision::Dismissed => "dismissed",
            Decision::Snoozed => "snoozed",
        }
    }

    /// Parse the ledger's spelling. `None` for anything else, including the two queue states that are not
    /// decisions: a ledger file must not quietly turn `read` into a verdict.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "accepted" => Some(Decision::Accepted),
            "flagged" => Some(Decision::Flagged),
            "dismissed" => Some(Decision::Dismissed),
            "snoozed" => Some(Decision::Snoozed),
            _ => None,
        }
    }

    /// Is this the act that says "reviewed"? Only `accepted` is: a flag asks someone else, a dismissal
    /// refuses, a snooze defers, and none of them may be counted as a review that happened.
    #[must_use]
    pub const fn is_sign_off(self) -> bool {
        matches!(self, Decision::Accepted)
    }
}

/// Who signed, as the ledger records it.
///
/// `kind` is `human` or `agent` — `ledger_entry.actor_kind`'s CHECK list. A sign-off written by a harness on
/// someone's behalf is a real fact worth recording and is *not* the same fact as a person accepting a
/// change, so the distinction is a field rather than a guess made later from the shape of the name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    /// `name <address>`, or a bare name, or the literal `unknown`. Never an invented identity
    /// (ADR-001 §7.7): a ledger whose "who" column is a guess is worse than one that admits it does not know.
    pub display: String,
    /// `human` or `agent`.
    pub kind: ActorKind,
}

/// Whether a human or an agent performed the act.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorKind {
    /// A person, at a keyboard, accountable for the verdict.
    Human,
    /// A harness or a bot recording the act on someone's behalf.
    Agent,
}

impl ActorKind {
    /// The ledger's spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ActorKind::Human => "human",
            ActorKind::Agent => "agent",
        }
    }

    /// Parse the ledger's spelling. Anything unrecognised is `Agent`, never `Human`: the claim that a person
    /// signed is the claim that needs evidence, and a typo in the field must not manufacture one.
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value {
            "human" => ActorKind::Human,
            _ => ActorKind::Agent,
        }
    }

    /// The kind of an actor string that was never labelled: a queue act's `actor`, which `fathom-cli` stamps
    /// from git config or from the reviewer's own configured identity.
    ///
    /// The default is `Human` for one reason — the only way a name gets into that field is a person typing it
    /// or a repository configuring it, so it carries the human's own identity unless it says otherwise. The
    /// spellings that do say otherwise are the ones a harness writes: a `(agent)` suffix or a `-bot` name. An
    /// agent may record a verdict; it may not be the accountable human (`e4.6`).
    #[must_use]
    pub fn of_actor_string(actor: &str) -> Self {
        if actor.contains("(agent)") || actor.ends_with("-bot") {
            ActorKind::Agent
        } else {
            ActorKind::Human
        }
    }
}

/// One final content fact: one path, as it stood at the end of the episode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileContent {
    /// The post-change path.
    pub path: String,
    /// What the path's content *was*, as far as the index knows.
    pub state: FileState,
}

/// How well the index knows what a path's content was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileState {
    /// The git blob oid of the final content. Blob oids survive every rewrite that preserves content, which
    /// is the entire reason the content digest is built from them.
    Blob(String),
    /// The path does not exist at the end of the episode — it was deleted, or added and then deleted. A
    /// deletion is content information, not an absence of it.
    Deleted,
    /// The indexer recorded no blob for this path (its payload cap). The digest is not computed, and the
    /// entry says so rather than hashing a placeholder that looks like a fact.
    Unmeasured,
}

/// The content an entry was signed against, and the digest of it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContentProfile {
    /// The digest, or empty when `measured` is false.
    pub digest: String,
    /// The per-path facts, sorted by path.
    pub files: Vec<FileContent>,
    /// Whether every path had a content fact. `false` means `digest` is empty and staleness cannot be
    /// decided — which is a reported answer, not a silent one.
    pub measured: bool,
    /// The lines the index counted across the episode, for the scale line a status prints. Not part of the
    /// digest: a squash re-sums them without changing a byte of content.
    pub n_add: i64,
    /// See [`ContentProfile::n_add`].
    pub n_del: i64,
}

/// One `(member, path)` touch, as narrow as the ledger needs it.
///
/// A named struct rather than the store's six-field tuple, because the digest rules depend on which of those
/// fields are content facts and which are bookkeeping — and a tuple cannot say so.
#[derive(Debug, Clone, Copy)]
pub struct DiffTouch<'a> {
    /// The member's ordinal within its episode: `0` is the oldest.
    pub ordinal: i64,
    /// The post-change path.
    pub path: &'a str,
    /// `add`, `modify`, `delete`, `rename` — the store's `file_change.change_kind`.
    pub change_kind: &'a str,
    /// Lines this touch added. Bookkeeping only, never in a digest.
    pub add: i64,
    /// Lines this touch deleted. Same.
    pub del: i64,
    /// The blob oid of the final content, when the indexer recorded one (ADR-004 §6.3).
    pub blob: Option<&'a str>,
}

impl ContentProfile {
    /// Fold the touches into one profile, last member winning per path.
    ///
    /// `rows` must be in `(ordinal, path)` order, which is what the store's read guarantees. `complete` is
    /// whether the read reached the end of its own cap: a truncated read cannot support a digest, so the
    /// profile comes back `measured = false` and the ledger prints why.
    #[must_use]
    pub fn from_diff_rows(rows: &[DiffTouch<'_>], complete: bool) -> Self {
        let mut last: std::collections::BTreeMap<&str, DiffTouch<'_>> =
            std::collections::BTreeMap::new();
        let mut n_add = 0_i64;
        let mut n_del = 0_i64;
        for row in rows {
            n_add += row.add;
            n_del += row.del;
            last.insert(row.path, *row);
        }
        let mut files = Vec::with_capacity(last.len());
        let mut measured = complete && !last.is_empty();
        for (path, touch) in last {
            let state = if touch.change_kind == "delete" {
                FileState::Deleted
            } else {
                match touch.blob {
                    Some(hex) => FileState::Blob(hex.to_owned()),
                    None => {
                        measured = false;
                        FileState::Unmeasured
                    }
                }
            };
            files.push(FileContent {
                path: path.to_owned(),
                state,
            });
        }
        let digest = if measured {
            content_digest(&files)
        } else {
            String::new()
        };
        ContentProfile {
            digest,
            files,
            measured,
            n_add,
            n_del,
        }
    }

    /// The paths, sorted — the `paths:` line and the shape the reviewer actually saw.
    #[must_use]
    pub fn paths(&self) -> Vec<PathFact> {
        self.files
            .iter()
            .map(|file| PathFact {
                path: file.path.clone(),
                content: match &file.state {
                    FileState::Blob(hex) => {
                        PathContent::Blob(hex.chars().take(BLOB_PREFIX).collect())
                    }
                    FileState::Deleted => PathContent::Absent,
                    FileState::Unmeasured => PathContent::Unknown,
                },
            })
            .collect()
    }

    /// Compare what was signed (`self`) with what the episode is now (`now`).
    #[must_use]
    pub fn compare(&self, now: &ContentProfile) -> ContentCompare {
        compare_signed(&self.digest, &self.paths(), now)
    }
}

/// The outcome of comparing signed content with current content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentCompare {
    /// Byte-for-byte the content that was signed.
    Same,
    /// One side was never measured, so nothing can be claimed either way.
    Unmeasured(String),
    /// The content moved, with the three ways it can move separated because the reviewer's next question
    /// after "is this stale?" is always "stale how?".
    Changed {
        /// Paths that appeared after the sign-off.
        added: Vec<String>,
        /// Paths that are gone.
        removed: Vec<String>,
        /// Paths that are still there with different content.
        modified: Vec<String>,
    },
}

impl ContentCompare {
    /// The sentence for the ledger line. Written as a method so the status printer, the `--json` payload and
    /// the test all quote the same words rather than each inventing a paraphrase.
    #[must_use]
    pub fn reason(&self) -> String {
        match self {
            ContentCompare::Same => String::new(),
            ContentCompare::Unmeasured(why) => why.clone(),
            ContentCompare::Changed {
                added,
                removed,
                modified,
            } => {
                let mut parts = Vec::new();
                if !modified.is_empty() {
                    parts.push(format!(
                        "{} file(s) changed after sign-off ({})",
                        modified.len(),
                        list(&modified[..modified.len().min(3)])
                    ));
                }
                if !added.is_empty() {
                    parts.push(format!(
                        "{} file(s) appeared after sign-off ({})",
                        added.len(),
                        list(&added[..added.len().min(3)])
                    ));
                }
                if !removed.is_empty() {
                    parts.push(format!(
                        "{} file(s) are gone that the sign-off covered ({})",
                        removed.len(),
                        list(&removed[..removed.len().min(3)])
                    ));
                }
                if parts.is_empty() {
                    // The digests disagreed and no per-path fact did. It cannot happen with digests built
                    // out of exactly these facts, and "it cannot happen" is not a reason to print nothing.
                    "the content digest changed after sign-off".to_owned()
                } else {
                    parts.join("; ")
                }
            }
        }
    }
}

/// `a, b and c`, for a reason line that has to fit on a terminal.
fn list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [first, second] => format!("{first} and {second}"),
        rest => format!("{}, {} and {}", rest[0], rest[1], rest[2..].join(", ")),
    }
}

/// The content digest over sorted per-path final states.
#[must_use]
pub fn content_digest(files: &[FileContent]) -> String {
    let mut hasher = sha2::Sha256::new();
    hasher.update(CONTENT_SCHEMA.as_bytes());
    hasher.update([0]);
    for file in files {
        hasher.update(file.path.as_bytes());
        hasher.update([0]);
        let token = match &file.state {
            FileState::Blob(hex) => format!("blob:{hex}"),
            FileState::Deleted => "deleted".to_owned(),
            FileState::Unmeasured => "unmeasured".to_owned(),
        };
        hasher.update(token.as_bytes());
        hasher.update([0]);
    }
    hex_digest(&hasher.finalize())
}

/// One decision, as the ledger keeps it.
///
/// The typed fields are what the code reasons over; [`SignOff::body`] is what the *file* says, kept
/// separately on purpose: tamper detection has to hash the bytes a human committed, not the values this
/// build managed to parse out of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignOff {
    /// The entry id (`sg-…`), stable inside the ledger.
    pub id: String,
    /// The file the entry was read from or is destined for, relative to the repository root.
    pub file: String,
    /// The episode identity it was signed against.
    pub episode: String,
    /// ADR-003's strong digest at sign-off.
    pub fingerprint: String,
    /// ADR-003's author+subject digest at sign-off.
    pub fallback: String,
    /// ADR-003's author+paths digest at sign-off.
    pub shape: String,
    /// The content digest at sign-off; empty when it could not be measured.
    pub content: String,
    /// What the reviewer decided.
    pub decision: Decision,
    /// The dismiss vocabulary's code, when the decision used one.
    pub reason: String,
    /// The reviewer's own words.
    pub note: String,
    /// Who signed.
    pub actor: Actor,
    /// When, unix millis.
    pub signed_utc: i64,
    /// The member commits the sign-off covered — the *extent*, and what the last relink tier matches on.
    pub members: Vec<String>,
    /// The paths the sign-off covered, sorted, each with the content it had.
    pub paths: Vec<PathFact>,
    /// Commits, files, and the churn rollups, in that order: the `scale:` line.
    pub scale: Scale,
    /// The risk score at sign-off, if one had been computed, and the model that produced it.
    pub risk: Option<i64>,
    /// See [`SignOff::risk`].
    pub risk_model: String,
    /// The field lines exactly as the file holds them, in file order, without the digest line.
    pub body: Vec<(String, String)>,
    /// The tamper digest, as written.
    pub digest: String,
}

/// The scale rollups a sign-off records: what the reviewer was looking at.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Scale {
    /// Member commits covered.
    pub commits: i64,
    /// Files covered.
    pub files: i64,
    /// Lines added.
    pub add: i64,
    /// Lines removed.
    pub del: i64,
}

/// What a sign-off needs that is not already in the entry: the episode's own facts.
///
/// Plain data, built by [`crate::ledger::collect`] from the store so that signing, re-linking and reporting
/// are all pure functions a test can call with a literal.
#[derive(Debug, Clone, Default)]
pub struct EpisodeFacts {
    /// The id as the index has it now.
    pub id: String,
    /// `committed` or `provisional` — and a provisional episode may not be signed off at all
    /// (ADR-003 §7 P4).
    pub kind: String,
    /// Whether any current commit belongs to it.
    pub live: bool,
    /// The strong digest.
    pub fingerprint: String,
    /// The author+subject digest.
    pub fallback_fingerprint: String,
    /// The author+paths digest.
    pub shape_fingerprint: String,
    /// The first line of the first member, for the line a status prints.
    pub summary: String,
    /// The members, in order.
    pub members: Vec<String>,
    /// The lineage ordinal, and the two clock fields the relink tie-break reads.
    pub lineage_seq: i64,
    /// See [`EpisodeFacts::lineage_seq`].
    pub last_utc: i64,
    /// See [`EpisodeFacts::lineage_seq`].
    pub created_utc: i64,
    /// The content profile, as the index can currently build it.
    pub content: ContentProfile,
    /// The risk the queue would print, and its model. `None` when nothing scored it.
    pub risk: Option<i64>,
    /// See [`EpisodeFacts::risk`].
    pub risk_model: String,
}

impl EpisodeFacts {
    /// The path set the episode covers, sorted, each with its final content.
    #[must_use]
    pub fn paths(&self) -> Vec<PathFact> {
        self.content.paths()
    }
}

/// How many hex of a blob oid the ledger prints: enough to name a file's content unambiguously in a
/// repository (48 bits) and short enough to read.
const BLOB_PREFIX: usize = 12;

/// One path the sign-off covered, and what its content was.
///
/// The blob prefix is what lets a staleness message name the file that changed rather than shrugging that
/// "a digest differs". The *digest* remains the authority on whether the content moved; these facts name
/// where, and two 12-hex prefixes agreeing is a claim a reader can check with `git rev-parse`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathFact {
    /// The path as the index recorded it (the post-change path).
    pub path: String,
    /// Its content at the end of the episode, as far as anyone now knows.
    pub content: PathContent,
}

/// The content a recorded path had.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathContent {
    /// The first 12 hex of its git blob oid.
    Blob(String),
    /// The path did not exist at the end of the episode — it was deleted by it.
    Absent,
    /// No blob was recorded, so its content is not known.
    Unknown,
}

impl PathFact {
    /// The ledger's spelling: `src/a.rs@1a2b3c4d5e6f`, `src/gone.rs@-` for a deletion, bare `src/a.rs`
    /// when the content was never measured.
    #[must_use]
    pub fn display(&self) -> String {
        match &self.content {
            PathContent::Blob(prefix) => format!("{}@{prefix}", self.path),
            PathContent::Absent => format!("{}@-", self.path),
            PathContent::Unknown => self.path.clone(),
        }
    }

    /// The reverse of [`PathFact::display`]. A path that happens to contain `@` survives the round trip: the
    /// suffix only counts as a content marker when it is `-` or hex of the right length.
    #[must_use]
    pub fn parse(text: &str) -> PathFact {
        if let Some((path, suffix)) = text.rsplit_once('@') {
            let looks_like_marker = suffix == "-"
                || (suffix.len() == BLOB_PREFIX && suffix.chars().all(|c| c.is_ascii_hexdigit()));
            if looks_like_marker && !path.is_empty() {
                return PathFact {
                    path: path.to_owned(),
                    content: if suffix == "-" {
                        PathContent::Absent
                    } else {
                        PathContent::Blob(suffix.to_owned())
                    },
                };
            }
        }
        PathFact {
            path: text.to_owned(),
            content: PathContent::Unknown,
        }
    }

    /// Does this fact agree with the current content of the same path? A shortcut over
    /// [`PathContent::agrees_with`].
    #[must_use]
    pub fn agrees_with(&self, now: &PathContent) -> Option<bool> {
        self.content.agrees_with(now)
    }
}

impl PathContent {
    /// Does a recorded content fact agree with the current one? `None` means "cannot tell", which is an
    /// answer and not a shrug — it is what keeps an unmeasured entry from reading as a match.
    ///
    /// Blob comparison is a prefix test in both directions, because the ledger prints 12 hex and the index
    /// holds the whole oid.
    #[must_use]
    pub fn agrees_with(&self, now: &PathContent) -> Option<bool> {
        match (self, now) {
            (PathContent::Unknown, _) | (_, PathContent::Unknown) => None,
            (PathContent::Absent, PathContent::Absent) => Some(true),
            (PathContent::Absent, _) | (_, PathContent::Absent) => Some(false),
            (PathContent::Blob(before), PathContent::Blob(after)) => Some(
                before == after || before.starts_with(after.as_str()) || after.starts_with(before),
            ),
        }
    }
}

/// The `paths:` field: a comma-separated list of [`PathFact`] spellings.
#[must_use]
pub fn render_paths(paths: &[PathFact]) -> String {
    paths
        .iter()
        .map(PathFact::display)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Compare what a sign-off named (`signed`, with its `digest`) against the content profile the index can
/// build now. The message is the point: "stale" is the verdict, "these three files changed" is the advice.
#[must_use]
pub fn compare_signed(
    signed_digest: &str,
    signed: &[PathFact],
    now: &ContentProfile,
) -> ContentCompare {
    if signed_digest.is_empty()
        || signed
            .iter()
            .any(|fact| fact.content == PathContent::Unknown)
    {
        return ContentCompare::Unmeasured(
            "the content was not fully measured at sign-off, so freshness cannot be checked"
                .to_owned(),
        );
    }
    if !now.measured {
        return ContentCompare::Unmeasured(
            "the index holds no readable blob for the current content, so freshness cannot be checked"
                .to_owned(),
        );
    }
    let current = now.paths();
    if signed_digest == now.digest {
        return ContentCompare::Same;
    }
    let before: std::collections::BTreeMap<&str, &PathContent> = signed
        .iter()
        .map(|fact| (fact.path.as_str(), &fact.content))
        .collect();
    let after: std::collections::BTreeMap<&str, &PathContent> = current
        .iter()
        .map(|fact| (fact.path.as_str(), &fact.content))
        .collect();
    let added: Vec<String> = after
        .keys()
        .filter(|path| !before.contains_key(*path))
        .map(|path| (*path).to_owned())
        .collect();
    let removed: Vec<String> = before
        .keys()
        .filter(|path| !after.contains_key(*path))
        .map(|path| (*path).to_owned())
        .collect();
    let modified: Vec<String> = before
        .keys()
        .filter(|path| {
            after.get(*path).is_some_and(|state| {
                before
                    .get(*path)
                    .is_some_and(|mine| mine.agrees_with(state) != Some(true))
            })
        })
        .map(|path| (*path).to_owned())
        .collect();
    if modified.is_empty() && added.is_empty() && removed.is_empty() {
        // The digests disagree and every named path agrees. The named facts are prefixes, so a collision is
        // possible in principle; the digest is the authority, and this is what an honest "something moved,
        // here is how far the evidence goes" reads like.
        return ContentCompare::Changed {
            added,
            removed,
            modified: current.iter().map(|fact| fact.path.clone()).collect(),
        };
    }
    ContentCompare::Changed {
        added,
        removed,
        modified,
    }
}

/// Why a sign-off was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignError {
    /// The identity is missing or is the literal `unknown`. "A sign-off without an identity is not a
    /// sign-off" is the glossary's sentence, and refusing is the only honest response to it.
    NoIdentity,
    /// A provisional episode: uncommitted work, which cannot be reproduced, so there is nothing a later
    /// reader could check the sign-off against (ADR-003 §7 P4).
    Provisional,
    /// The episode has no content profile at all — no member rows in the index — so the entry could never
    /// be checked for staleness and would masquerade as fresh forever.
    NoContent,
}

impl SignError {
    /// The refusal, phrased as the advice it is.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            SignError::NoIdentity => {
                "a sign-off needs an identity: set `git config user.name`/`user.email`, or \
                 `$FATHOM_REVIEWER`, or `[review] identity = \"Name <address>\"` in fathom.toml, or pass \
                 `--identity`"
                    .to_owned()
            }
            SignError::Provisional => {
                "a provisional episode is uncommitted work: sign it off after the \
                                       commit lands, so there is content a later reader can check"
                    .to_owned()
            }
            SignError::NoContent => {
                "the index holds no diff for this episode, so a sign-off against it \
                                     could never be checked for staleness; run `fathom index` first"
                    .to_owned()
            }
        }
    }
}

/// The ledger's clock: the whole second, and nothing finer.
///
/// The file is the truth (`ADR-004 §9.1`), and the file renders a stamp as `YYYY-MM-DDTHH:MM:SSZ` —
/// [`iso_utc`] drops everything after the second and [`parse_iso_utc`] can only read back what it wrote.
/// So an entry whose in-memory stamp has a sub-second part is an entry that changes when it is re-read:
/// its `signed:` line, and therefore its derived id, come back different from what was written. That is
/// not cosmetic. `collect::unsettled` asks "have I already rendered this act?" by rebuilding the act's
/// entry identity and looking for it among the parsed entries, and an act logged by the TUI carries the
/// millisecond its keystroke landed in — so the answer was always *no*: `fathom review sync` re-rendered
/// every act on every run, appended a second copy of a sign-off that had not changed, and then the mirror
/// refused the duplicate id (`reco-2pw`).
///
/// Truncating here, in the one place that turns an instant into a ledger stamp, is what makes the
/// round trip through the file an identity: sign, write, re-read, and the entry is the same entry.
#[must_use]
pub const fn ledger_clock(millis: i64) -> i64 {
    millis - millis.rem_euclid(1_000)
}

/// Build a sign-off for `episode`.
///
/// Refuses rather than writes a weak entry: no identity, a provisional episode, or an episode whose content
/// the index cannot describe. Each refusal is a [`SignError`] with the remedy in its own sentence.
pub fn sign(
    episode: &EpisodeFacts,
    actor: &Actor,
    decision: Decision,
    note: &str,
    signed_utc: i64,
) -> Result<SignOff, SignError> {
    if actor.display.trim().is_empty() || actor.display == "unknown" {
        return Err(SignError::NoIdentity);
    }
    if episode.kind == "provisional" {
        return Err(SignError::Provisional);
    }
    if episode.content.files.is_empty() {
        return Err(SignError::NoContent);
    }
    // The file cannot hold a finer stamp than this, so nothing else may claim to (`ledger_clock`).
    let signed_utc = ledger_clock(signed_utc);
    let scale = Scale {
        commits: episode.members.len().try_into().unwrap_or(i64::MAX),
        files: episode.content.files.len().try_into().unwrap_or(i64::MAX),
        add: episode.content.n_add,
        del: episode.content.n_del,
    };
    let mut entry = SignOff {
        id: entry_id(
            &episode.id,
            &episode.content.digest,
            actor,
            decision,
            signed_utc,
        ),
        file: String::new(),
        episode: episode.id.clone(),
        fingerprint: episode.fingerprint.clone(),
        fallback: episode.fallback_fingerprint.clone(),
        shape: episode.shape_fingerprint.clone(),
        content: episode.content.digest.clone(),
        decision,
        reason: String::new(),
        note: note.to_owned(),
        actor: actor.clone(),
        signed_utc,
        members: episode.members.clone(),
        paths: episode.paths(),
        scale,
        risk: episode.risk,
        risk_model: episode.risk_model.clone(),
        body: Vec::new(),
        digest: String::new(),
    };
    entry.body = entry.canonical_body();
    entry.digest = entry_digest(&entry.id, &entry.body);
    Ok(entry)
}

/// The field order a new entry writes, which is also the order its digest is computed over — so the order is
/// a contract, and changing it changes every hash.
///
/// The reasoning behind it is "what a reader needs first": the episode, the decision, the who and the when;
/// then the four digests, in the order their questions get asked (identity, then the two weaker identity
/// digests, then content); then the scale; then the reviewer's words last, because they are the only prose
/// and the only part worth reading twice.
impl SignOff {
    /// The field lines a *newly built* entry writes, in canonical order.
    fn canonical_body(&self) -> Vec<(String, String)> {
        let mut body = vec![
            ("episode".to_owned(), self.episode.clone()),
            ("decision".to_owned(), self.decision.as_str().to_owned()),
            ("actor".to_owned(), self.actor.display.clone()),
            ("actor-kind".to_owned(), self.actor.kind.as_str().to_owned()),
            ("signed".to_owned(), iso_utc(self.signed_utc)),
            ("fingerprint".to_owned(), self.fingerprint.clone()),
            (
                "fallback".to_owned(),
                or_unmeasured(&self.fallback, "fallback"),
            ),
            ("shape".to_owned(), or_unmeasured(&self.shape, "shape")),
            (
                "content".to_owned(),
                or_unmeasured(&self.content, "content"),
            ),
            ("members".to_owned(), join8(&self.members)),
            ("paths".to_owned(), render_paths(&self.paths)),
            (
                "scale".to_owned(),
                format!(
                    "{} commit(s), {} file(s), +{} -{}",
                    self.scale.commits, self.scale.files, self.scale.add, self.scale.del
                ),
            ),
        ];
        if self.risk.is_some() {
            body.push((
                "risk".to_owned(),
                format!(
                    "{} ({})",
                    self.risk.unwrap_or_default(),
                    if self.risk_model.is_empty() {
                        "unknown model"
                    } else {
                        &self.risk_model
                    }
                ),
            ));
        }
        if !self.reason.is_empty() {
            body.push(("reason".to_owned(), self.reason.clone()));
        }
        if !self.note.is_empty() {
            body.push(("note".to_owned(), self.note.clone()));
        }
        // Every value is folded before it is stored, because the digest is a hash of the *file's* lines and
        // the file cannot hold a newline inside a `- key: value` line. Folding at render time instead would
        // mean the digest and the bytes on disk disagreed — which is indistinguishable from tampering, and
        // would report every honest entry as edited. `one_line` is idempotent, so `render_entry` folding
        // again costs nothing.
        body.into_iter()
            .map(|(key, value)| (key, super::format::one_line(&value)))
            .collect()
    }

    /// The `digest:` value over `body`, exposed for `verify`, which re-hashes what the file says.
    #[must_use]
    pub fn digest_for(id: &str, body: &[(String, String)]) -> String {
        entry_digest(id, body)
    }

    /// The keys this build knows, for the parse report: a key that is *not* in here is a typo, and a typo in
    /// a fingerprint field must not read as "no fingerprint".
    #[must_use]
    pub const fn known_keys() -> [&'static str; 15] {
        [
            "episode",
            "decision",
            "actor",
            "actor-kind",
            "signed",
            "fingerprint",
            "fallback",
            "shape",
            "content",
            "members",
            "paths",
            "scale",
            "risk",
            "reason",
            "note",
        ]
    }

    /// The keys every entry must carry, which is `known_keys` without the optional ones.
    ///
    /// `risk`, `reason` and `note` are a reviewer's prose and a risk model's opinion: an entry with no note is
    /// a complete entry, and a `verify` that said otherwise would make every honest ledger look broken.
    pub const fn required_keys() -> [&'static str; 12] {
        [
            "episode",
            "decision",
            "actor",
            "actor-kind",
            "signed",
            "fingerprint",
            "fallback",
            "shape",
            "content",
            "members",
            "paths",
            "scale",
        ]
    }

    /// Which required keys are missing from this entry — the completeness half of `verify`.
    #[must_use]
    pub fn missing_keys(&self) -> Vec<&'static str> {
        let have: BTreeSet<&str> = self.body.iter().map(|(key, _)| key.as_str()).collect();
        SignOff::required_keys()
            .iter()
            .filter(|key| !have.contains(**key))
            .copied()
            .collect()
    }
}

/// The tamper digest: a hash of the entry's own lines, framed by the format and the id so an id swap cannot
/// reuse a digest.
#[must_use]
pub fn entry_digest(id: &str, body: &[(String, String)]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(REVIEW_SCHEMA.as_bytes());
    hasher.update([0]);
    hasher.update(id.as_bytes());
    hasher.update([0]);
    for (key, value) in body {
        hasher.update(key.as_bytes());
        hasher.update(b": ");
        hasher.update(value.as_bytes());
        hasher.update(b"\n");
    }
    format!("sha256:{}", hex_digest(&hasher.finalize()))
}

/// The entry id: `sg-` plus a digest of the act, so the same act rendered twice is one entry.
///
/// Derived, never random: a random id would make `fathom review sync` append a second copy of an act it had
/// already rendered every time it ran after a `git clean`, and the ledger's deduplication is by id.
#[must_use]
pub fn entry_id(
    episode: &str,
    content: &str,
    actor: &Actor,
    decision: Decision,
    signed_utc: i64,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(REVIEW_SCHEMA.as_bytes());
    hasher.update([0]);
    hasher.update(episode.as_bytes());
    hasher.update([0]);
    hasher.update(content.as_bytes());
    hasher.update([0]);
    hasher.update(actor.display.as_bytes());
    hasher.update([0]);
    hasher.update(decision.as_str().as_bytes());
    hasher.update([0]);
    hasher.update(signed_utc.to_string().as_bytes());
    format!("sg-{}", &hex_digest(&hasher.finalize())[..16])
}

/// `…` for a digest a writer could not compute, with the field name in the note so the file reads as
/// sentences rather than as blanks.
fn or_unmeasured(value: &str, field: &str) -> String {
    if value.is_empty() {
        format!("unmeasured ({field} not recorded at sign-off)")
    } else {
        value.to_owned()
    }
}

/// Member oids, truncated to eight hex each for the printed line; the full oids are in `scope` JSON.
fn join8(members: &[String]) -> String {
    members
        .iter()
        .map(|oid| oid.chars().take(8).collect::<String>())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Unix millis to `2026-10-19T14:22:03Z`, the only timestamp format in the file.
///
/// Not the store's integer: the ledger is read by people and diffed in a PR, and a nanosecond count is
/// unreadable in a diff. `fathom_core::time` does the civil-date split; the millisecond remainder is
/// dropped because a sign-off's claim is "at this second", and a parse of this string back to millis is
/// exact to the second, which is all either direction needs.
#[must_use]
pub fn iso_utc(millis: i64) -> String {
    Timestamp::from_unix_micros(millis * 1_000).to_string()
}

/// The reverse of [`iso_utc`]. `None` for anything that is not that shape, so a hand-edited timestamp is a
/// parse problem rather than a silent epoch.
#[must_use]
pub fn parse_iso_utc(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    if bytes.len() != 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
    {
        return None;
    }
    let number = |range: std::ops::Range<usize>| text[range].parse::<i64>().ok();
    let (Some(year), Some(month), Some(day)) = (number(0..4), number(5..7), number(8..10)) else {
        return None;
    };
    let (Some(hour), Some(minute), Some(second)) = (number(11..13), number(14..16), number(17..19))
    else {
        return None;
    };
    let days = days_from_civil(year, month, day);
    Some(((days * 86_400) + (hour * 3_600) + (minute * 60) + second) * 1_000)
}

/// Howard Hinnant's `days_from_civil`: days since 1970-01-01, proleptic Gregorian, no lookup tables.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn actor() -> Actor {
        Actor {
            display: "Dana Ruiz <dana@example.com>".to_owned(),
            kind: ActorKind::Human,
        }
    }

    fn episode(digest: &str) -> EpisodeFacts {
        EpisodeFacts {
            id: "ep-abc".to_owned(),
            kind: "committed".to_owned(),
            live: true,
            fingerprint: "f".repeat(64),
            fallback_fingerprint: "b".repeat(64),
            shape_fingerprint: "s".repeat(64),
            summary: "feat: thing".to_owned(),
            members: vec!["0123456789abcdef".to_owned()],
            content: ContentProfile {
                digest: digest.to_owned(),
                files: vec![FileContent {
                    path: "src/a.rs".to_owned(),
                    state: FileState::Blob("aa".repeat(20)),
                }],
                measured: true,
                n_add: 10,
                n_del: 2,
            },
            ..EpisodeFacts::default()
        }
    }

    /// One touch, in the shape `from_diff_rows` takes.
    fn touch<'a>(
        ordinal: i64,
        path: &'a str,
        kind: &'a str,
        blob: Option<&'a str>,
    ) -> DiffTouch<'a> {
        DiffTouch {
            ordinal,
            path,
            change_kind: kind,
            add: 1,
            del: 1,
            blob,
        }
    }

    #[test]
    fn a_sign_off_without_an_identity_is_refused_not_written() {
        let ghost = Actor {
            display: "unknown".to_owned(),
            kind: ActorKind::Human,
        };
        let err = sign(
            &episode("d".repeat(64).as_str()),
            &ghost,
            Decision::Accepted,
            "",
            1,
        )
        .unwrap_err();
        assert!(err.message().contains("identity"), "{err:?}");
    }

    #[test]
    fn a_provisional_episode_cannot_be_signed_off() {
        let mut ep = episode("d".repeat(64).as_str());
        ep.kind = "provisional".to_owned();
        let err = sign(&ep, &actor(), Decision::Accepted, "", 1).unwrap_err();
        assert!(err.message().contains("uncommitted"), "{err:?}");
    }

    #[test]
    fn the_same_act_renders_the_same_entry() {
        let ep = episode("d".repeat(64).as_str());
        let one = sign(&ep, &actor(), Decision::Accepted, "ok", 1_700_000_000_000)
            .expect("the fixture episode signs");
        let two = sign(&ep, &actor(), Decision::Accepted, "ok", 1_700_000_000_000)
            .expect("the fixture episode signs");
        assert_eq!(
            one, two,
            "sync's deduplication is by id, so the id must be derived"
        );
        assert!(one.id.starts_with("sg-"));
        assert_eq!(one.digest, entry_digest(&one.id, &one.body));
    }

    #[test]
    fn editing_any_field_breaks_the_digest() {
        let ep = episode("d".repeat(64).as_str());
        let entry = sign(&ep, &actor(), Decision::Accepted, "checked", 1)
            .expect("the fixture episode signs");
        for index in 0..entry.body.len() {
            let mut tampered = entry.clone();
            tampered.body[index].1 = format!("{}x", tampered.body[index].1);
            assert_ne!(
                entry_digest(&tampered.id, &tampered.body),
                entry.digest,
                "flipping `{}` must not leave the digest alone",
                entry.body[index].0
            );
        }
    }

    #[test]
    fn the_content_digest_survives_a_squash_and_refuses_an_edit() {
        // Three members that churn one file to the same final text as one member that wrote it once.
        let rows = [
            touch(0, "src/a.rs", "add", Some("bbb")),
            touch(1, "src/a.rs", "modify", Some("ccc")),
            touch(2, "src/a.rs", "modify", Some("ddd")),
        ];
        let squashed = ContentProfile::from_diff_rows(&rows, true);
        let single =
            ContentProfile::from_diff_rows(&[touch(2, "src/a.rs", "add", Some("ddd"))], true);
        assert_eq!(
            squashed.digest, single.digest,
            "the content a reviewer signed did not change when the history was flattened"
        );
        let edited =
            ContentProfile::from_diff_rows(&[touch(0, "src/a.rs", "modify", Some("eee"))], true);
        assert!(!squashed.compare(&edited).eq(&ContentCompare::Same));
        assert!(
            squashed
                .compare(&edited)
                .reason()
                .contains("changed after sign-off")
        );
    }

    #[test]
    fn an_unmeasured_content_is_never_reported_as_a_match() {
        let unmeasured =
            ContentProfile::from_diff_rows(&[touch(0, "src/a.rs", "modify", None)], true);
        assert!(!unmeasured.measured);
        let reason = unmeasured.compare(&ContentProfile::default()).reason();
        assert!(reason.contains("cannot be checked"), "{reason}");
    }

    #[test]
    fn a_timestamp_survives_the_round_trip() {
        let text = iso_utc(1_760_000_000_000);
        assert_eq!(text.matches('T').count(), 1, "{text}");
        assert_eq!(parse_iso_utc(&text), Some(1_760_000_000_000));
        assert_eq!(parse_iso_utc("yesterday"), None);
    }
}
