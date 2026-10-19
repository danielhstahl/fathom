//! What a reviewer *does* to an episode, the vocabulary a dismissal must choose from, and the record
//! each act leaves behind.
//!
//! Four acts, one keystroke each: **accept** (`x`), **flag** (`f`), **dismiss** (`-`), **snooze** (`z`).
//! The keystroke is the cheap part; what makes them a workflow instead of four editor commands is that
//! each one produces a *record* — who, when, what they claimed, and which episode identity they were
//! signing (`e4.6` keys a sign-off to that identity and its fingerprint) — and two of them refuse to
//! happen without a reason attached.
//!
//! # Dismissals take a reason from a closed list
//!
//! A free-text dismissal cannot be counted, and the thing this queue has to make visible is a *pattern*:
//! forty dismissals that all say "not my area" is a queue the reviewer has decided is not for them, and
//! a queue that hides that is worse than no queue ([ADR-001 §7.7] — hidden suppression is the failure this
//! product names). So [`REASONS`] is a closed vocabulary of codes, each with the claim it states, and a
//! dismissal without a code in it is refused rather than stored as an empty string.
//!
//! "Closed" and "learnable" are not opposites here. The shipped list is fixed so every repository's
//! numbers mean the same thing; a repository may *add* codes ([`ReasonVocabulary`], fed by
//! `[queue.reasons]` in `fathom.toml`, read by `fathom-cli` because configuration is the CLI's and not
//! the analysis layer's), and the picker is ordered by what this repository has actually used — the
//! vocabulary learns which reasons you reach for, and nothing about that ordering changes what a stored
//! code means. An unknown code that is already in the ledger is still listed (marked `no longer in the
//! vocabulary`), because the alternative is a ledger entry that cannot be read back.
//!
//! # Snoozing is a condition, not a delay
//!
//! [`SnoozeUntil`] is a closed set of conditions the tool can actually evaluate — a week has passed, the index
//! has moved, that session produced another episode — each stamped at snooze time with the fact it will
//! be compared against ([`SnoozeUntil::stamp`], [`SnoozeUntil::read`]), so a snooze says in the ledger *what it is
//! waiting for* and a later run can tell whether it is still waiting.

use std::cmp::Ordering as CmpOrdering;
use std::collections::BTreeMap;

use fathom_core::time::Timestamp;
use serde::Serialize;

/// What a reviewer has done with an episode.
///
/// The spellings are the store's (`review_state.state`) on purpose, in both directions: a word that means
/// one thing in the ledger and another in the shell is how a sign-off becomes a mystery after a reindex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum Disposition {
    /// Nobody has opened it.
    Unread,
    /// Opened, no verdict. Not a decision, and never counted as one.
    Read,
    /// Signed off (`x`).
    Accepted,
    /// Called out for someone else, with a note (`f`).
    Flagged,
    /// Refused, with a reason from [`REASONS`] (`-`).
    Dismissed,
    /// Deferred until a condition ([`SnoozeUntil`]) (`z`).
    Snoozed,
}

impl Disposition {
    /// The store's spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Disposition::Unread => "unread",
            Disposition::Read => "read",
            Disposition::Accepted => "accepted",
            Disposition::Flagged => "flagged",
            Disposition::Dismissed => "dismissed",
            Disposition::Snoozed => "snoozed",
        }
    }

    /// Parse the store's spelling. An unknown word is `Unread`, never an error: a row written by a newer
    /// fathom must not make this one refuse to open, and the rail already prints `?` for what it cannot
    /// name (`e1.11`'s forward-compatibility rule, applied to verdicts).
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value {
            "read" => Disposition::Read,
            "accepted" => Disposition::Accepted,
            "flagged" => Disposition::Flagged,
            "dismissed" => Disposition::Dismissed,
            "snoozed" => Disposition::Snoozed,
            _ => Disposition::Unread,
        }
    }

    /// The word the queue prints.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Disposition::Unread => "unread",
            Disposition::Read => "opened",
            Disposition::Accepted => "reviewed",
            Disposition::Flagged => "flagged",
            Disposition::Dismissed => "dismissed",
            Disposition::Snoozed => "snoozed",
        }
    }

    /// The one-character badge the worklist and the rail print beside a row.
    #[must_use]
    pub const fn glyph(self) -> &'static str {
        match self {
            Disposition::Unread => "⚑",
            Disposition::Read => "✓",
            Disposition::Accepted => "✔",
            Disposition::Flagged => "⚐",
            Disposition::Dismissed => "×",
            Disposition::Snoozed => "z",
        }
    }

    /// Has a human decided something? `Read` is not a decision, which is why "opened" and "reviewed" are
    /// different words in [`Disposition::label`].
    #[must_use]
    pub const fn is_verdict(self) -> bool {
        !matches!(self, Disposition::Unread | Disposition::Read)
    }

    /// Does this row still want reading?
    ///
    /// `released` is the caller's answer to "has this snooze's condition happened?" — `false` for anything
    /// that is not snoozed. A snooze whose condition has been met is an open row again: the whole point of
    /// a condition rather than a delay is that it comes back by itself.
    #[must_use]
    pub const fn open(self, released: bool) -> bool {
        match self {
            Disposition::Unread | Disposition::Read => true,
            // A flag keeps the episode in the queue (ADR-004 §5): `f` is a reviewer saying "this one,
            // and someone should look at it", not a reviewer finishing with it. It sorts to the front
            // rather than leaving, and it is what stops a queue calling itself caught up.
            Disposition::Flagged => true,
            Disposition::Snoozed => released,
            Disposition::Accepted | Disposition::Dismissed => false,
        }
    }
}

impl core::fmt::Display for Disposition {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One dismiss reason: a code, the claim it states, and what it obliges the reviewer to add.
///
/// The `claim` is the load-bearing field. The retrospective prints claims, not codes, so a dismissal
/// reads back as a sentence someone is accountable for ("this belongs to someone else") rather than as a
/// token — which is the difference between an audit trail and a checkbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DismissReason {
    /// The stored code: `not-my-area`.
    pub code: String,
    /// The picker's words: `not my area`.
    pub label: String,
    /// What the reviewer is asserting, printed in the retrospective.
    pub claim: String,
    /// Whether this code obliges a note. `other` is the only one, and the obligation is what stops it
    /// becoming the drawer everything gets swept into.
    pub needs_note: bool,
    /// Whether this build shipped the code, or the repository added it.
    pub shipped: bool,
}

/// The shipped vocabulary: closed, ordered as the picker shows it, and short enough to learn.
///
/// Eight codes is a deliberate size. Fewer and `other` becomes the answer to everything; more and the
/// picker is a menu nobody reads, which is a free-text field with extra steps. Each one is a *claim about
/// the reviewer's own intent*, not a classification of the code — the queue is a model of attention, so
/// the reasons are about attention.
pub const REASONS: &[(&str, &str, &str, bool)] = &[
    (
        "not-my-area",
        "not my area",
        "this belongs to someone else, and I am not the reader for it",
        false,
    ),
    (
        "already-known",
        "I already know this",
        "I have seen this change and I do not need to read it again",
        false,
    ),
    (
        "intentional",
        "deliberate",
        "what looks risky here is what I wanted",
        false,
    ),
    (
        "false-positive",
        "the tool is wrong",
        "the finding that raised this does not describe my change — and that is a bug worth filing",
        false,
    ),
    (
        "low-stakes",
        "not worth the read",
        "reading this costs more than it could pay, and I accept that",
        false,
    ),
    (
        "superseded",
        "a later episode did it",
        "this has been overtaken, and the queue is listing history",
        false,
    ),
    (
        "will-revert",
        "this is coming out",
        "the change is going away before it merges, so reading it now is wasted",
        false,
    ),
    (
        "other",
        "something else (say what)",
        "none of the shipped reasons fit — and the note is where the reason actually lives",
        true,
    ),
];

/// The dismiss vocabulary in force for one repository: the shipped list, plus what that repository added,
/// ordered by what it has actually used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasonVocabulary {
    /// Every code available, shipped first.
    pub reasons: Vec<DismissReason>,
}

impl Default for ReasonVocabulary {
    /// The shipped list, never an empty one.
    ///
    /// A derived `Default` would be an empty vocabulary, and an empty vocabulary refuses every dismissal —
    /// so a caller that forgot to install one would ship a queue that cannot dismiss anything, quietly. The
    /// shipped list is the answer a forgotten field should produce.
    fn default() -> Self {
        ReasonVocabulary::shipped()
    }
}

impl ReasonVocabulary {
    /// The shipped vocabulary on its own — what a repository with no `[queue.reasons]` gets.
    #[must_use]
    pub fn shipped() -> Self {
        ReasonVocabulary {
            reasons: REASONS
                .iter()
                .map(|(code, label, claim, needs_note)| DismissReason {
                    code: (*code).to_owned(),
                    label: (*label).to_owned(),
                    claim: (*claim).to_owned(),
                    needs_note: *needs_note,
                    shipped: true,
                })
                .collect(),
        }
    }

    /// Add a repository's own codes, ignoring ones already shipped and ones that are not shaped like a
    /// code (`[a-z][a-z0-9-]*`). A config that adds `not-my-area` again changes nothing, which is the
    /// right answer to a typo and the wrong answer to a redefinition — so a redefinition is reported by
    /// `fathom-cli`, which reads the file and so can say which line did it.
    #[must_use]
    pub fn with_added(mut self, extra: &[&str]) -> Self {
        for code in extra {
            let code = code.trim();
            if code.is_empty()
                || !is_code(code)
                || self.reasons.iter().any(|reason| reason.code == code)
            {
                continue;
            }
            self.reasons.push(DismissReason {
                code: code.to_owned(),
                label: code.replace('-', " "),
                claim: format!("this repository's own reason: {code}"),
                // A repository that added a code knows what it means by it; making it say so every time
                // is how the code stays meaningful six weeks later.
                needs_note: true,
                shipped: false,
            });
        }
        self
    }

    /// The codes in a use-'em-or-lose-'em order: most-used first, then the unused shipped ones, then
    /// anything the ledger holds that the vocabulary no longer has.
    ///
    /// This is the "learnable" half. Nothing is dropped: a code that was removed from the config is still
    /// listed (with its count) when it is in the ledger, because a dismissal already recorded has to remain
    /// readable. The picker is `j`/`k`/`<Enter>` (ADR-006 §7.3 `pick`), so re-ordering it cannot move a
    /// keystroke out from under anyone.
    #[must_use]
    pub fn ordered(&self, usage: &BTreeMap<String, i64>) -> Vec<DismissReason> {
        let mut reasons: Vec<DismissReason> = self.reasons.clone();
        for code in usage.keys() {
            if !reasons.iter().any(|reason| &reason.code == code) {
                reasons.push(DismissReason {
                    code: code.clone(),
                    label: code.replace('-', " "),
                    claim: "in the ledger, but not in this build's vocabulary".to_owned(),
                    needs_note: false,
                    shipped: false,
                });
            }
        }
        reasons.sort_by(|left, right| {
            let count = |reason: &DismissReason| *usage.get(&reason.code).unwrap_or(&0);
            count(right)
                .cmp(&count(left))
                .then_with(|| right.shipped.cmp(&left.shipped))
                .then_with(|| left.code.cmp(&right.code))
        });
        reasons
    }

    /// Look a code up.
    #[must_use]
    pub fn find(&self, code: &str) -> Option<&DismissReason> {
        self.reasons.iter().find(|reason| reason.code == code)
    }

    /// The claim a stored code carries, for the retrospective.
    #[must_use]
    pub fn claim(&self, code: &str) -> String {
        self.find(code)
            .map(|reason| reason.claim.clone())
            // A code the ledger holds and this build does not know: an older build's vocabulary, or a
            // repository whose config has since dropped it. The act stays readable and says which of the
            // two it is not, rather than being relabelled as no reason at all.
            .unwrap_or_else(|| format!("a reason this build does not ship (`{code}`)"))
    }
}

/// Is a string shaped like a reason code?
#[must_use]
pub fn is_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 32
        && code.chars().enumerate().all(|(index, ch)| match ch {
            'a'..='z' => true,
            '-' => index > 0,
            '0'..='9' => index > 0,
            _ => false,
        })
}

/// What a snooze is waiting for.
///
/// Every condition is something a later run can evaluate from rows it can read, which is the only kind of
/// condition worth storing: `z` must not be a promise the tool cannot keep. The stamp ([`SnoozeUntil::stamp`])
/// records the fact it will be compared against, so the ledger states the condition *and* its baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SnoozeUntil {
    /// Reopen after N days have passed since the snooze.
    AfterDays(u32),
    /// Reopen when the index publishes an epoch past the one it was on — "wait until there is new work".
    NextIndex,
    /// Reopen when the session that produced this episode produces another one — "wait until that agent
    /// turn is finished".
    NewInSession,
}

/// The conditions the picker offers, in the order it offers them.
pub const CONDITIONS: &[(&str, &str, &str)] = &[
    ("7d", "in a week", "reopen after 7 days"),
    ("1d", "tomorrow", "reopen after 1 day"),
    ("30d", "in a month", "reopen after 30 days"),
    (
        "next-index",
        "when new work lands",
        "reopen when the index has moved past this episode's epoch",
    ),
    (
        "new-in-session",
        "when that session continues",
        "reopen when the same harness session produces another episode",
    ),
];

impl SnoozeUntil {
    /// The stored code.
    #[must_use]
    pub fn code(self) -> String {
        match self {
            SnoozeUntil::AfterDays(days) => format!("{days}d"),
            SnoozeUntil::NextIndex => "next-index".to_owned(),
            SnoozeUntil::NewInSession => "new-in-session".to_owned(),
        }
    }

    /// The words the picker and the ledger print.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            SnoozeUntil::AfterDays(days) => {
                format!("in {days} day{}", if days == 1 { "" } else { "s" })
            }
            SnoozeUntil::NextIndex => "when new work lands".to_owned(),
            SnoozeUntil::NewInSession => "when that session continues".to_owned(),
        }
    }

    /// Read a code back. Anything unrecognised is `None`, and the caller says which word it did not know
    /// rather than defaulting to a week.
    #[must_use]
    pub fn parse(code: &str) -> Option<Self> {
        match code {
            "next-index" => Some(SnoozeUntil::NextIndex),
            "new-in-session" => Some(SnoozeUntil::NewInSession),
            other => match other.strip_suffix('d') {
                Some(rest) => rest
                    .parse::<u32>()
                    .ok()
                    .filter(|days| (1..=365).contains(days))
                    .map(SnoozeUntil::AfterDays),
                None => None,
            },
        }
    }

    /// The condition with its baseline baked in: `7d`, `next-index:41`, `new-in-session:3`.
    ///
    /// Storing the baseline is what makes a snooze checkable years later: "reopen when the index moved"
    /// is meaningless once the index has moved forty times, and `next-index:41` is not.
    #[must_use]
    pub fn stamp(self, baseline: Option<u64>) -> String {
        match (self, baseline) {
            (SnoozeUntil::AfterDays(_), _) => self.code(),
            (_, Some(value)) => format!("{}:{value}", self.code()),
            (_, None) => self.code(),
        }
    }

    /// The condition and the baseline it was stamped with.
    #[must_use]
    pub fn read(stored: &str) -> Option<(Self, Option<u64>)> {
        let (code, baseline) = match stored.split_once(':') {
            Some((code, value)) => (code, value.parse::<u64>().ok()),
            None => (stored, None),
        };
        SnoozeUntil::parse(code).map(|until| (until, baseline))
    }

    /// Has this condition happened?
    ///
    /// `situation` is what the run can see now: the epoch, how many episodes the episode's session holds,
    /// and the clock. A snooze with no readable stamp counts as *not* released — a snooze that cannot be
    /// evaluated stays out of the queue and stays visible in the retrospective, which is honest, rather
    /// than falling back into the queue every run, which is noise.
    #[must_use]
    pub fn released(
        self,
        stamped: Option<u64>,
        act_utc: Timestamp,
        situation: &SnoozeSituation,
    ) -> bool {
        match self {
            SnoozeUntil::AfterDays(days) => {
                situation.now.as_unix_micros()
                    >= act_utc
                        .as_unix_micros()
                        .saturating_add(i64::from(days).saturating_mul(86_400 * 1_000_000))
            }
            SnoozeUntil::NextIndex => match stamped {
                Some(epoch) => situation.epoch > epoch,
                None => false,
            },
            SnoozeUntil::NewInSession => match stamped {
                Some(count) => situation.session_episodes > count,
                None => false,
            },
        }
    }
}

/// What a run knows about the world, for the purpose of deciding whether a snooze is finished.
#[derive(Debug, Clone, Copy)]
pub struct SnoozeSituation {
    /// The clock, passed in — nothing in this crate reads one (ADR-004 §5.4).
    pub now: Timestamp,
    /// The index epoch now.
    pub epoch: u64,
    /// How many episodes the snoozed episode's session now holds; `0` when it has no session.
    pub session_episodes: u64,
}

/// One triage act as the reviewer asked for it, before the ledger's identity fields are added.
///
/// The validation is the ticket's contract, not a form check: a dismissal with no code is a shrug, and a
/// flag with no note is a notification nobody can act on. Each refusal names the fix in the words the
/// status line has room for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriageAct<'a> {
    /// The episode being dispositioned.
    pub episode: &'a str,
    /// What the reviewer decided.
    pub disposition: Disposition,
    /// The dismiss code, when the disposition is `Dismissed`.
    pub reason: Option<&'a str>,
    /// Free text: required with `Flagged` and with `other`, optional elsewhere.
    pub note: Option<&'a str>,
    /// The snooze condition, when the disposition is `Snoozed`.
    pub until: Option<&'a str>,
}

impl<'a> TriageAct<'a> {
    /// The accept act, which needs nothing: signing off is the whole act.
    #[must_use]
    pub fn accept(episode: &'a str) -> Self {
        TriageAct {
            episode,
            disposition: Disposition::Accepted,
            reason: None,
            note: None,
            until: None,
        }
    }

    /// The flag act, which requires a note.
    #[must_use]
    pub fn flag(episode: &'a str, note: &'a str) -> Self {
        TriageAct {
            episode,
            disposition: Disposition::Flagged,
            reason: None,
            note: Some(note),
            until: None,
        }
    }

    /// The dismiss act, which requires a code.
    #[must_use]
    pub fn dismiss(episode: &'a str, reason: &'a str) -> Self {
        TriageAct {
            episode,
            disposition: Disposition::Dismissed,
            reason: Some(reason),
            note: None,
            until: None,
        }
    }

    /// The snooze act, which requires a condition.
    #[must_use]
    pub fn snooze(episode: &'a str, until: &'a str) -> Self {
        TriageAct {
            episode,
            disposition: Disposition::Snoozed,
            reason: None,
            note: None,
            until: Some(until),
        }
    }

    /// Check the act against the vocabulary in force.
    ///
    /// # Errors
    ///
    /// A one-line reason the act cannot be recorded, phrased as what to press next.
    pub fn validate(&self, vocabulary: &ReasonVocabulary) -> Result<(), String> {
        match self.disposition {
            Disposition::Accepted | Disposition::Unread | Disposition::Read => Ok(()),
            Disposition::Flagged => match self.note.map(str::trim) {
                None | Some("") => Err(
                    "a flag needs a note — `f` then type what you want someone to look at \
                     (a flag with no note is a notification, not a review)"
                        .to_owned(),
                ),
                Some(_) => Ok(()),
            },
            Disposition::Dismissed => {
                let Some(code) = self.reason.map(str::trim).filter(|code| !code.is_empty()) else {
                    return Err(
                        "a dismissal needs a reason — `-` then pick one (the queue counts them, \
                         and a reasonless dismissal cannot be counted)"
                            .to_owned(),
                    );
                };
                let Some(reason) = vocabulary.find(code) else {
                    return Err(format!(
                        "`{code}` is not a dismiss reason — `-` lists what this repository accepts \
                         ({} of them)",
                        vocabulary.reasons.len()
                    ));
                };
                if reason.needs_note && self.note.map(str::trim).unwrap_or("").is_empty() {
                    return Err(format!(
                        "`{code}` needs a note — it is the reason that says \"none of the others \
                         fit\", so the note is the only part with content in it"
                    ));
                }
                Ok(())
            }
            Disposition::Snoozed => {
                let Some(code) = self.until.map(str::trim) else {
                    return Err(
                        "a snooze needs a condition — `z` then pick one (a snooze with no condition \
                         is an episode you never look at again)"
                            .to_owned(),
                    );
                };
                match SnoozeUntil::parse(code) {
                    None => Err(format!(
                        "`{code}` is not a snooze condition — `z` offers {}",
                        CONDITIONS
                            .iter()
                            .map(|(code, _, _)| format!("`{code}`"))
                            .collect::<Vec<_>>()
                            .join(" ")
                    )),
                    Some(_) => Ok(()),
                }
            }
        }
    }
}

/// One recorded act: what `fathom-cli` writes when a keystroke dispositioned an episode, and what the
/// retrospective reads back.
///
/// Six facts, and each one is there because a question gets asked of it later. **Who** — `actor`, so a
/// pattern can be a person's pattern and not a fog. **When** — `utc`, so a window can be drawn. **Why** —
/// `reason` and `note`, the claim and its detail. **What** — `episode` *and* `fingerprint`, because an
/// episode id is derived and `e4.6` is the ticket that re-links a sign-off across a rewrite; an act
/// recorded without the fingerprint it was made against cannot be re-linked honestly. **Risk** — the score
/// at the time of the act, because "dismissed three high-risk episodes" cannot be recovered after the
/// weights move.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TriageRecord {
    /// The episode the act was taken on.
    pub episode: String,
    /// The digest it was taken against (`e4.6` re-links sign-offs with this).
    pub fingerprint: String,
    /// What was decided.
    pub disposition: Disposition,
    /// The dismiss code, empty for the other acts.
    pub reason: String,
    /// The note, empty when none was given.
    pub note: String,
    /// The snooze condition, stamped with its baseline. Empty for the other acts.
    pub until: String,
    /// Who did it: `name <address>`, or `unknown` when nothing could say (never invented).
    pub actor: String,
    /// When.
    pub utc: Timestamp,
    /// The risk score at the time, and the model that scored it. Both optional because a row can be
    /// dispositioned before anything has scored it.
    pub risk: Option<u8>,
    /// See [`TriageRecord::risk`].
    pub risk_model: String,
}

impl TriageRecord {
    /// The one-line form: what `fathom queue --retrospect` and the ledger mirror print, and the shape a
    /// reviewer can grep for.
    #[must_use]
    pub fn line(&self, now: Timestamp, vocabulary: &ReasonVocabulary) -> String {
        let why = match self.disposition {
            Disposition::Dismissed => {
                let mut why = if self.reason.is_empty() {
                    "no reason recorded".to_owned()
                } else {
                    self.reason.clone()
                };
                if !self.note.is_empty() {
                    why.push_str(" — ");
                    why.push_str(&self.note);
                }
                if !self.reason.is_empty() && vocabulary.find(&self.reason).is_none() {
                    why.push_str(" (not in this build's vocabulary)");
                }
                why
            }
            Disposition::Flagged => {
                if self.note.is_empty() {
                    "no note".to_owned()
                } else {
                    self.note.clone()
                }
            }
            Disposition::Snoozed => format!("until {}", self.until),
            Disposition::Accepted => "signed off".to_owned(),
            Disposition::Read => "opened".to_owned(),
            Disposition::Unread => "nothing".to_owned(),
        };
        format!(
            "{} {} {} · {} · risk {} · {}",
            self.episode.chars().take(6).collect::<String>(),
            self.disposition.glyph(),
            self.disposition.label(),
            why,
            self.risk
                .map_or_else(|| "?".to_owned(), |score| score.to_string()),
            who_and_when(&self.actor, self.utc, now),
        )
    }
}

/// `daniel · 2h ago`, the way every ledger line ends.
#[must_use]
pub fn who_and_when(actor: &str, utc: Timestamp, now: Timestamp) -> String {
    let seconds = (now.as_unix_seconds() - utc.as_unix_seconds()).max(0);
    let ago = if seconds < 60 {
        "just now".to_owned()
    } else if seconds < 3_600 {
        format!("{}m ago", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h ago", seconds / 3_600)
    } else {
        format!("{}d ago", seconds / 86_400)
    };
    let who = if actor.trim().is_empty() {
        "unknown"
    } else {
        actor.trim()
    };
    format!("{who} · {ago}")
}

/// The order acts are listed in when two happen at the same instant.
///
/// Newest first, then by episode id so a tie does not depend on which row the store returned first.
#[must_use]
pub fn newest_first(left: &TriageRecord, right: &TriageRecord) -> CmpOrdering {
    right
        .utc
        .cmp(&left.utc)
        .then_with(|| left.episode.cmp(&right.episode))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::order::DAY_SECONDS;

    const NOW: i64 = 1_700_000_000;

    fn at(seconds: i64) -> Timestamp {
        Timestamp::from_unix_micros(seconds * 1_000_000)
    }

    fn record(id: &str, disposition: Disposition, reason: &str) -> TriageRecord {
        TriageRecord {
            episode: id.to_owned(),
            fingerprint: "sha256:fixture".to_owned(),
            disposition,
            reason: reason.to_owned(),
            note: String::new(),
            until: String::new(),
            actor: "ada <ada@example.com>".to_owned(),
            utc: at(NOW - 3_600),
            risk: Some(77),
            risk_model: "spine-1".to_owned(),
        }
    }

    #[test]
    fn the_four_acts_are_exactly_the_words_the_store_already_stores() {
        // ADR-004 §5 shipped `review_state` with these values in migration 3; the queue may not invent a
        // second vocabulary for the same fact, so this test is the seam between the two.
        for (word, disposition) in [
            ("unread", Disposition::Unread),
            ("read", Disposition::Read),
            ("accepted", Disposition::Accepted),
            ("flagged", Disposition::Flagged),
            ("dismissed", Disposition::Dismissed),
            ("snoozed", Disposition::Snoozed),
        ] {
            assert_eq!(Disposition::parse(word), disposition, "{word}");
            assert_eq!(disposition.as_str(), word);
        }
        assert_eq!(
            Disposition::parse("something a future build invented"),
            Disposition::Unread,
            "an unrecognised marker is 'not read yet', never 'decided'"
        );
    }

    #[test]
    fn a_dismissal_without_a_code_and_a_flag_without_a_note_are_refused_in_words() {
        let vocabulary = ReasonVocabulary::shipped();
        assert_eq!(TriageAct::accept("e").validate(&vocabulary), Ok(()));

        let bare = TriageAct {
            episode: "e",
            disposition: Disposition::Dismissed,
            reason: None,
            note: None,
            until: None,
        };
        let why = match bare.validate(&vocabulary) {
            Ok(()) => panic!("a dismissal with no code is not an act"),
            Err(why) => why,
        };
        assert!(why.contains("reason"), "{why}");

        let unknown = TriageAct::dismiss("e", "out-of-hours").validate(&vocabulary);
        assert!(
            unknown.is_err(),
            "the vocabulary is closed: a code this build never heard of is not a reason"
        );

        assert!(TriageAct::flag("e", "").validate(&vocabulary).is_err());
        assert!(
            TriageAct::flag("e", "the pool is being drained")
                .validate(&vocabulary)
                .is_ok()
        );
        assert!(TriageAct::snooze("e", "").validate(&vocabulary).is_err());
        assert!(
            TriageAct::snooze("e", "nonsense")
                .validate(&vocabulary)
                .is_err()
        );
        assert!(TriageAct::snooze("e", "7d").validate(&vocabulary).is_ok());
    }

    #[test]
    fn other_needs_a_note_because_other_without_a_note_is_a_shrug() {
        let vocabulary = ReasonVocabulary::shipped();
        assert!(
            TriageAct::dismiss("e", "other")
                .validate(&vocabulary)
                .is_err()
        );
        assert!(
            TriageAct::dismiss("e", "intentional")
                .validate(&vocabulary)
                .is_ok()
        );
        assert!(
            vocabulary
                .find("other")
                .is_some_and(|reason| reason.needs_note),
            "`other` is shipped, and shipped with the note requirement"
        );
    }

    #[test]
    fn a_repository_can_add_a_code_but_not_erase_the_ledger_by_doing_it() {
        let vocabulary = ReasonVocabulary::shipped().with_added(&["out-of-hours", "not-my-area"]);
        assert!(vocabulary.find("out-of-hours").is_some());
        assert_eq!(
            vocabulary
                .reasons
                .iter()
                .filter(|r| r.code == "not-my-area")
                .count(),
            1,
            "adding a code that is already shipped must not double it in the picker"
        );
        // An act written under a code the repository has since dropped still has to be readable, and a
        // list assembled from usage has to cope with codes it never shipped.
        let usage: BTreeMap<String, i64> =
            [("out-of-hours".to_owned(), 4), ("ghost-code".to_owned(), 2)]
                .into_iter()
                .collect();
        let ordered = vocabulary.ordered(&usage);
        assert!(ordered.iter().any(|reason| reason.code == "ghost-code"));
        assert_eq!(ordered[0].code, "out-of-hours", "what you use goes first");
        assert_eq!(
            vocabulary.claim("ghost-code"),
            "a reason this build does not ship (`ghost-code`)",
            "an unknown code is printed as itself, not silently dropped"
        );
    }

    #[test]
    fn usage_ordering_puts_the_frequently_used_first_and_keeps_the_rest_in_order() {
        let vocabulary = ReasonVocabulary::shipped();
        let usage: BTreeMap<String, i64> =
            [("low-stakes".to_owned(), 2), ("not-my-area".to_owned(), 9)]
                .into_iter()
                .collect();
        let ordered = vocabulary.ordered(&usage);
        let codes: Vec<&str> = ordered.iter().map(|reason| reason.code.as_str()).collect();
        assert_eq!(&codes[..2], &["not-my-area", "low-stakes"][..]);
        assert_eq!(codes.len(), REASONS.len(), "nothing is dropped by ordering");
        let mut unused: Vec<&str> = codes[2..].to_vec();
        unused.sort_unstable();
        assert_eq!(
            unused,
            codes[2..],
            "the codes nobody has used keep a stable order rather than a hash's: {codes:?}"
        );
    }

    #[test]
    fn each_snooze_condition_is_checked_against_what_the_run_can_see() {
        let situation = SnoozeSituation {
            now: at(NOW),
            epoch: 42,
            session_episodes: 5,
        };
        let week = SnoozeUntil::AfterDays(7);
        assert!(
            !week.released(None, at(NOW - 3 * DAY_SECONDS), &situation),
            "three days into a week the reviewer is still away"
        );
        assert!(week.released(None, at(NOW - 8 * DAY_SECONDS), &situation));

        // The baseline is stamped at the act, which is what makes the condition checkable later: "when the
        // index moves" is meaningless after it has moved forty times.
        let stamp = SnoozeUntil::NextIndex.stamp(Some(41));
        assert_eq!(stamp, "next-index:41");
        let (until, baseline) = SnoozeUntil::read(&stamp).unwrap_or_else(|| panic!("round trip"));
        assert_eq!(until, SnoozeUntil::NextIndex);
        assert_eq!(baseline, Some(41));
        assert!(
            until.released(baseline, at(NOW - 10), &situation),
            "42 > 41"
        );
        let (same, same_baseline) =
            SnoozeUntil::read("next-index:42").unwrap_or_else(|| panic!("round trip"));
        assert!(
            !same.released(same_baseline, at(NOW - 10), &situation),
            "an epoch that has not moved is not a release"
        );

        let session = SnoozeUntil::NewInSession.stamp(Some(4));
        let (session_until, session_baseline) =
            SnoozeUntil::read(&session).unwrap_or_else(|| panic!("round trip"));
        assert!(
            session_until.released(session_baseline, at(NOW), &situation),
            "5 > 4"
        );
        assert!(
            !SnoozeUntil::NewInSession.released(Some(5), at(NOW), &situation),
            "the same count is not 'new episodes arrived'"
        );
        assert!(
            !SnoozeUntil::NextIndex.released(None, at(NOW), &situation),
            "a snooze nobody can evaluate stays out of the queue rather than back in it every run"
        );
    }

    #[test]
    fn a_record_line_carries_who_when_why_and_the_risk_at_the_time() {
        let vocabulary = ReasonVocabulary::shipped();
        let line = record("ep-abcdef1234", Disposition::Dismissed, "not-my-area")
            .line(at(NOW), &vocabulary);
        assert!(line.contains("ep-abc"), "{line}");
        assert!(line.contains("dismissed"), "{line}");
        assert!(line.contains("not-my-area"), "{line}");
        assert!(line.contains("ada <ada@example.com>"), "{line}");
        assert!(line.contains("1h ago"), "{line}");
        assert!(
            line.contains("risk 77"),
            "the score at the time is the point: {line}"
        );

        let flag = TriageRecord {
            note: "the pool is being drained".to_owned(),
            ..record("ep-abcdef1234", Disposition::Flagged, "")
        };
        assert!(
            flag.line(at(NOW), &vocabulary).contains("pool"),
            "{}",
            flag.line(at(NOW), &vocabulary)
        );
        let no_note = TriageRecord {
            note: String::new(),
            ..record("ep-abcdef1234", Disposition::Flagged, "")
        };
        assert!(no_note.line(at(NOW), &vocabulary).contains("no note"));
    }

    #[test]
    fn acts_are_listed_newest_first_and_a_tie_falls_to_the_episode_id() {
        let old = record("a", Disposition::Accepted, "");
        let new = TriageRecord {
            utc: at(NOW),
            ..record("b", Disposition::Accepted, "")
        };
        let mut records = [old.clone(), new.clone()];
        records.sort_by(newest_first);
        assert_eq!(
            records
                .iter()
                .map(|r| r.episode.as_str())
                .collect::<Vec<_>>(),
            vec!["b", "a"]
        );
        let same_instant = vec![
            record("zz", Disposition::Accepted, ""),
            record("aa", Disposition::Accepted, ""),
        ];
        let mut forwards = same_instant.clone();
        let mut backwards = same_instant;
        backwards.reverse();
        forwards.sort_by(newest_first);
        backwards.sort_by(newest_first);
        assert_eq!(forwards, backwards, "a tie must not depend on row order");
    }

    #[test]
    fn which_dispositions_still_want_reading_is_one_table_not_four() {
        assert!(Disposition::Unread.open(false));
        assert!(Disposition::Read.open(false));
        assert!(
            Disposition::Flagged.open(false),
            "a flag keeps its episode in the queue"
        );
        assert!(!Disposition::Snoozed.open(false));
        assert!(Disposition::Snoozed.open(true));
        assert!(!Disposition::Accepted.open(false));
        assert!(!Disposition::Dismissed.open(false));
        assert!(Disposition::Accepted.is_verdict());
        assert!(
            !Disposition::Read.is_verdict(),
            "opening a thing is not deciding it"
        );
    }
}
