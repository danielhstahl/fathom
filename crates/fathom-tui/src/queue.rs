//! `fathom_tui::queue` — the shell's half of the attention queue (`e4.5`, `reco-wsj`).
//!
//! The ordering, the four acts, the dismiss vocabulary and the retrospective all live in
//! `fathom_analyze::queue` and reach here through [`fathom_analyze::insight`], which is the only door this
//! crate may open (ADR-002 §5.3, lint `M02`). What is in *this* module is the part a shell has to own:
//! what the reviewer has done since the frame opened, which chooser is on screen, and the trait object
//! that writes an act to disk.
//!
//! # The one rule that keeps it honest
//!
//! **An act is not taken until it is written.** [`Ledger::record`] is called first and the shell's own
//! state changes only if it returns `Ok`; a refusal is a toast that says what failed and why. A queue that
//! moved the row and *then* discovered the ledger was closed would be showing a sign-off that does not
//! exist — the exact class of lie ADR-001 §7.7 names, and the reason the order of those two statements is
//! a contract rather than a style.
//!
//! # When there is no ledger
//!
//! `None` is a real configuration: a `--plain` frame over an unindexed directory, a test, an embedder with
//! no store. Then acts stay in memory for the length of the run, the retrospective says so out loud, and
//! the strip carries `no ledger — nothing written this run`. `fathom-tui` does not pretend a write
//! happened because a keystroke did.

use std::collections::BTreeMap;
use std::sync::Arc;

use fathom_analyze::insight::{
    DismissReason, Disposition, QueuePolicy, QueueRanking, QueueRetrospective, QueueRow,
    QueueWindow, ReasonVocabulary, SnoozeSituation, SnoozeUntil, TriageRecord, rank_queue,
    retrospect,
};
use fathom_core::time::Timestamp;

use crate::style::{Face, StyledLine};

use crate::timeline::{Episode, ReviewState};

/// Something that can put an act in the ledger, and read the ledger back.
///
/// A trait object for the same reason [`crate::diff::PatchProvider`] is one: the shell opens nothing
/// (ADR-002 §3), so the composition root installs the party that can. `fathom-cli`'s implementation holds
/// the store path and opens per call, which is what lets a long-running frame hold no write lock.
pub trait Ledger: Send + Sync {
    /// Write one act, durably.
    ///
    /// The single reason this returns `Result`: the caller may not mark an episode reviewed until the
    /// record exists.
    ///
    /// # Errors
    ///
    /// A one-line reason the act could not be written, phrased as what the reviewer should do next.
    fn record(&self, record: &TriageRecord) -> Result<(), String>;

    /// The acts already in the ledger, newest first, capped at `limit`.
    ///
    /// # Errors
    ///
    /// A one-line reason the ledger could not be read. The retrospective prints it rather than falling
    /// back to "nothing has been dismissed", which is the difference between an unknown and a clean sheet.
    fn history(&self, limit: i64) -> Result<Vec<TriageRecord>, String>;
}

/// A ledger in the shape [`crate::frame::Options`] can hold.
#[derive(Clone)]
pub struct LedgerHandle(pub Arc<dyn Ledger>);

impl core::fmt::Debug for LedgerHandle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("LedgerHandle").field(&"<ledger>").finish()
    }
}

/// What the composition root hands the shell about triage (`e4.5`).
///
/// One struct rather than five `Options` fields because they are one decision — *who is reviewing, in
/// which repository, with which vocabulary, writing where* — and a shell that was handed four of the five
/// would render a queue whose acts were unsigned, unwritten or judged by the wrong vocabulary. Every field
/// has an honest empty: no ledger, `unknown` actor, shipped vocabulary, empty history.
#[derive(Debug, Clone, Default)]
pub struct Setup {
    /// The dismiss vocabulary in force: shipped codes plus `queue.dismiss_reasons` from the config.
    pub vocabulary: ReasonVocabulary,
    /// The ordering's knobs, as the config set them.
    pub policy: QueuePolicy,
    /// `name <address>`, or `unknown` — never a guess (ADR-001 §7.7).
    pub actor: String,
    /// Where acts go. `None` is `--plain` over an unindexed directory, and is printed as such.
    pub ledger: Option<LedgerHandle>,
    /// The ledger's recent acts, newest first, already read so the frame's first paint is complete.
    pub history: Vec<TriageRecord>,
    /// Why the history is missing or short, if it is (`no store at …`), printed not swallowed.
    pub notices: Vec<String>,
    /// How many live episodes the index holds: the retrospective's coverage denominator.
    pub total: usize,
}

impl Setup {
    /// Fold the setup into a fresh shell state.
    #[must_use]
    pub fn state(&self) -> State {
        let mut state = State {
            vocabulary: self.vocabulary.clone(),
            policy: self.policy,
            actor: self.actor.clone(),
            ledger: self.ledger.clone(),
            history: Vec::new(),
            decided: BTreeMap::new(),
            until: BTreeMap::new(),
            sessions: BTreeMap::new(),
            retrospective: false,
            pick: None,
            pick_reason: None,
            notices: self.notices.clone(),
            total: self.total,
        };
        state.load_history(&self.history);
        state
    }
}

/// What the shell knows about triage, and what has happened since the frame opened.
#[derive(Debug, Clone)]
pub struct State {
    /// The dismiss vocabulary in force — shipped codes plus whatever this repository added.
    pub vocabulary: ReasonVocabulary,
    /// The ordering's knobs, as the repository's config set them.
    pub policy: QueuePolicy,
    /// Who signs an act here: `name <address>`, or `unknown`. Never invented (ADR-001 §7.7).
    pub actor: String,
    /// Where acts go, when there is somewhere.
    pub ledger: Option<LedgerHandle>,
    /// Every act the shell knows about: the ledger's history at load, then this session's on top, so the
    /// newest act is at the front.
    pub history: Vec<TriageRecord>,
    /// Each episode's current disposition, as the last act on it left it.
    pub decided: BTreeMap<String, Disposition>,
    /// The snooze stamp per episode (`7d`, `next-index:41`), read back out of the history.
    pub until: BTreeMap<String, String>,
    /// Which harness session holds which episodes, so a `new-in-session` snooze can be evaluated.
    pub sessions: BTreeMap<String, Vec<String>>,
    /// Whether the worklist pane is showing the retrospective instead of the worklist (`r`).
    pub retrospective: bool,
    /// The episode the open chooser is about, fixed when the chooser opens (`e4.5`).
    ///
    /// Captured at open rather than read at choose: the reviewer may move the cursor inside the picker,
    /// and an act that followed the *list's* cursor as well as the picker's would apply a reason to
    /// whichever row happened to be under a different cursor.
    pub pick: Option<String>,
    /// The dismiss code held over while the reviewer types the note that `other` demands.
    pub pick_reason: Option<String>,
    /// What went wrong reading the ledger, if anything — printed, not swallowed.
    pub notices: Vec<String>,
    /// How many live episodes the index holds: the retrospective's denominator.
    pub total: usize,
}

impl Default for State {
    fn default() -> Self {
        State {
            vocabulary: ReasonVocabulary::shipped(),
            policy: QueuePolicy::default(),
            actor: "unknown".to_owned(),
            ledger: None,
            history: Vec::new(),
            decided: BTreeMap::new(),
            until: BTreeMap::new(),
            sessions: BTreeMap::new(),
            retrospective: false,
            pick: None,
            pick_reason: None,
            notices: Vec::new(),
            total: 0,
        }
    }
}

impl State {
    /// The state of a shell that was handed nothing: shipped vocabulary, `unknown` actor, no ledger.
    #[must_use]
    pub fn absent() -> Self {
        State::default()
    }

    /// Fold the ledger's history into the shell's view of which episodes are already decided.
    ///
    /// Newest-first, so the *last* act on an episode is the one that wins and so `r` can show the whole
    /// ledger a reviewer has built across sessions rather than only this afternoon's.
    pub fn load_history(&mut self, records: &[TriageRecord]) {
        for record in records {
            self.apply(record.clone());
        }
    }

    /// Record an act in the shell's own state.
    ///
    /// Callers go through [`State::commit`], which writes first; this is the half that is pure enough to
    /// test without a store.
    pub fn apply(&mut self, record: TriageRecord) {
        self.decided
            .insert(record.episode.clone(), record.disposition);
        if record.disposition == Disposition::Snoozed {
            self.until
                .insert(record.episode.clone(), record.until.clone());
        } else {
            self.until.remove(&record.episode);
        }
        self.history.insert(0, record);
    }

    /// Take an act: write it, and change nothing unless the write landed.
    ///
    /// # Errors
    ///
    /// The ledger's own reason, verbatim, when it refused. With no ledger attached the act is kept in
    /// memory and the caller is told so in the returned `Err` — which is not a failure and not a success
    /// either, and the toast line has to carry the difference.
    /// Take an act: write it, and change the shell's own state only if the write landed.
    ///
    /// Returns the line to show and whether it was actually written. The line is
    /// [`TriageRecord::line`] — the same sentence `fathom queue --retrospect` prints — so what the reviewer
    /// is told at the keystroke and what the ledger later says about the act are one string produced in
    /// one place, not two wordings that can drift.
    #[must_use]
    pub fn commit(&mut self, record: TriageRecord) -> (String, bool) {
        let written = self.ledger.is_some();
        // The write, before anything at all changes. A refusal returns here: the row keeps its state, the
        // history keeps nothing, and the reviewer hears the reason — a queue that moved on anyway would be
        // advertising a decision that was never recorded (ADR-001 §7.7).
        if let Some(ledger) = &self.ledger
            && let Err(why) = ledger.0.record(&record)
        {
            return (
                format!(
                    "{} — NOT written: {why}",
                    record.line(record.utc, &self.vocabulary)
                ),
                false,
            );
        }
        let line = record.line(record.utc, &self.vocabulary);
        self.apply(record);
        if written {
            (line, true)
        } else {
            (
                format!("{line} — in memory only: no ledger attached to this run"),
                false,
            )
        }
    }

    /// How often each dismiss code appears in what this shell knows, which is what orders the picker.
    #[must_use]
    pub fn usage(&self) -> BTreeMap<String, i64> {
        let mut usage: BTreeMap<String, i64> = BTreeMap::new();
        for record in &self.history {
            if record.disposition == Disposition::Dismissed && !record.reason.is_empty() {
                *usage.entry(record.reason.clone()).or_default() += 1;
            }
        }
        usage
    }

    /// The picker's rows, in the order the reviewer will want them.
    #[must_use]
    pub fn reasons(&self) -> Vec<DismissReason> {
        self.vocabulary.ordered(&self.usage())
    }

    /// The disposition a given episode carries, if any act has touched it.
    #[must_use]
    pub fn disposition_of(&self, id: &str) -> Option<Disposition> {
        self.decided.get(id).copied()
    }

    /// What a row's disposition is *now*: the store's marker on load, this shell's afterwards.
    #[must_use]
    pub fn disposition(&self, episode: &Episode) -> Disposition {
        self.disposition_of(&episode.id)
            .unwrap_or(match episode.state {
                ReviewState::Unread => Disposition::Unread,
                ReviewState::Read => Disposition::Read,
                ReviewState::Accepted => Disposition::Accepted,
                ReviewState::Flagged => Disposition::Flagged,
                ReviewState::Dismissed => Disposition::Dismissed,
                ReviewState::Snoozed => Disposition::Snoozed,
            })
    }

    /// Which sessions hold which episodes, in the shape [`State::released`] needs.
    #[must_use]
    pub fn sessions_of(rows: &[Episode]) -> BTreeMap<String, Vec<String>> {
        let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for row in rows {
            if let Some(session) = row.session.as_deref() {
                map.entry(session.to_owned())
                    .or_default()
                    .push(row.id.clone());
            }
        }
        map
    }

    /// Has a snooze's condition happened yet?
    ///
    /// The stamp is compared against what the frame can see now: the epoch it was given and how many
    /// episodes this episode's session holds. A stamp that cannot be parsed keeps the episode snoozed — a
    /// snooze nobody can evaluate stays out of the queue and stays in the retrospective, rather than
    /// falling back into the worklist every run and teaching the reviewer to ignore it.
    #[must_use]
    pub fn released(&self, episode: &Episode, now: Timestamp, epoch: u64) -> bool {
        let Some(stored) = self.until.get(&episode.id) else {
            return false;
        };
        let Some((until, baseline)) = SnoozeUntil::read(stored) else {
            return false;
        };
        let session_episodes = episode
            .session
            .as_deref()
            .and_then(|session| self.sessions.get(session))
            .map_or(1, |rows| {
                u64::try_from(rows.len()).unwrap_or(u64::MAX).max(1)
            });
        let act_utc = self
            .history
            .iter()
            .find(|record| {
                record.episode == episode.id && record.disposition == Disposition::Snoozed
            })
            .map_or(Timestamp::UNIX_EPOCH, |record| record.utc);
        until.released(
            baseline,
            act_utc,
            &SnoozeSituation {
                now,
                epoch,
                session_episodes,
            },
        )
    }
}

/// The rows the ordering needs, marshalled from the rail's rows.
///
/// Nothing is computed here beyond reading fields: risk comes from the delta the row already carries,
/// staleness is `created`, blast is whatever [`Episode::blast`] holds (nothing, until `e3.2` has a module
/// graph to put in it), and the disposition is [`State::disposition`].
#[must_use]
pub fn rows(episodes: &[&Episode], state: &State) -> Vec<QueueRow> {
    episodes
        .iter()
        .map(|episode| QueueRow {
            episode: episode.id.clone(),
            risk: episode
                .risk()
                .and_then(|risk| u8::try_from(risk.score.max(0)).ok()),
            risk_model: episode
                .risk()
                .map_or_else(String::new, |risk| risk.model.clone()),
            created: episode.created,
            size: episode.size(),
            blast: episode.blast.clone(),
            disposition: state.disposition(episode),
        })
        .collect()
}

/// Which episodes the queue holds, in queue order, with the counts that go with them.
///
/// One call, one pass: the ranking *and* the counts, so the strip's numbers and the list's order cannot
/// disagree about what is open. The `released` probe consults the shell's snooze stamps, which is the only
/// place that knows them.
#[must_use]
pub fn ranking(episodes: &[&Episode], state: &State, now: Timestamp, epoch: u64) -> QueueRanking {
    let inputs = rows(episodes, state);
    let open: Vec<&Episode> = episodes.to_vec();
    rank_queue(&inputs, now, &state.policy, |id: &str| {
        open.iter()
            .any(|episode| episode.id == id && state.released(episode, now, epoch))
    })
}

/// Stamp a snooze condition with the baseline it will be checked against.
///
/// "Reopen when the index moves" is meaningless once the index has moved forty times; `next-index:41` is
/// not. The shell is the only party that knows the epoch and what the session holds, so the shell stamps
/// and the analysis only ever evaluates what was stamped — which is the difference between a condition and
/// a date the reviewer hoped would work (`e4.5`).
#[must_use]
pub fn stamp_condition(
    episode: &Episode,
    until: &str,
    epoch: u64,
    session_episodes: u64,
) -> String {
    let Some((condition, _)) = SnoozeUntil::read(until) else {
        // Not a condition this build knows: keep the reviewer's words rather than dropping the act, and let
        // the retrospective report that the condition could not be evaluated.
        return until.to_owned();
    };
    let baseline = match condition {
        SnoozeUntil::AfterDays(_) => None,
        SnoozeUntil::NextIndex => Some(epoch),
        SnoozeUntil::NewInSession => Some(session_episodes.max(1)),
    };
    let _ = episode;
    condition.stamp(baseline)
}

impl State {
    /// Whether this run has any act of its own to talk about.
    #[must_use]
    pub fn has_acts(&self) -> bool {
        !self.history.is_empty()
    }
}

/// The count line, plus the one gap the analysis cannot see (`e4.5`).
///
/// The sentence is the analysis's — one source for "4 open, and the factor that had no number" — and this
/// wrapper marks only what the analysis has no way to know: a run with no ledger attached, where every act
/// so far is tonight's and dies with the process. That mark belongs to the shell, because the shell is the
/// thing that was or was not handed a ledger.
#[must_use]
pub fn counts_line(state: &State, ranking: &QueueRanking) -> String {
    let mut line = ranking.count_line();
    if state.ledger.is_none() && state.has_acts() {
        line.push_str(" · in memory only: no ledger attached to this run");
    }
    line
}

/// The retrospective over everything this shell knows: the ledger's history plus tonight's acts.
///
/// The whole ledger rather than a week by default, because the pattern a reviewer needs to see is the one
/// they have been building for months; narrowing it is a knob for whoever asks (`--window`).
#[must_use]
pub fn retrospective(state: &State, now: Timestamp) -> QueueRetrospective {
    retrospect(
        &state.history,
        &state.vocabulary,
        state.total,
        QueueWindow::All,
        now,
    )
}

/// Draw the retrospective in the worklist pane (`r`).
///
/// The lines come from `fathom_analyze::queue::retrospect` as plain text — the analysis owns what the
/// sentence says — and the faces are chosen here, because colour belongs to the shell (ADR-006 §6.4). The
/// ledger's own read errors are printed as `!` lines: a retrospective that could not read the ledger must
/// not be allowed to look like a clean sheet.
#[must_use]
pub fn retrospective_lines(
    state: &State,
    now: Timestamp,
    width: usize,
    rows: usize,
) -> Vec<StyledLine> {
    let view = retrospective(state, now);
    let mut out: Vec<StyledLine> = Vec::new();
    // The honesty rule, in the shell because the shell is what failed to read: an unreadable ledger gets
    // its complaint printed, and the "nothing was dismissed" line is withheld — silence about the ledger
    // plus a clean sheet is exactly the lie a reviewer would act on.
    let unreadable = !state.notices.is_empty();
    for line in view.lines() {
        if unreadable && line.contains("nothing dismissed") {
            continue;
        }
        let face = if line.starts_with('!') {
            Face::Warning
        } else if line.starts_with("what you skipped") {
            Face::Emphasis
        } else if line.starts_with("  ") {
            Face::Text
        } else {
            Face::Muted
        };
        out.push(StyledLine::with_face(line, face));
    }
    for notice in &state.notices {
        out.push(StyledLine::with_face(
            format!("! ledger: {notice}"),
            Face::Warning,
        ));
    }
    while out.len() < rows {
        out.push(StyledLine::plain(""));
    }
    out.truncate(rows.max(1));
    out.into_iter().map(|line| line.padded(width)).collect()
}
