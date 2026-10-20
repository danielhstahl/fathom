//! `fathom_tui::ambient` — the glance pane, for the terminal beside the agent (`reco-8bx`).
//!
//! ADR-001 §4.1 promises a pane that "fits 80×12, one keypress drills in", and ADR-006 §5.7 registers it
//! as a view. This module is the *content* of that pane: the four facts the pane is allowed to read
//! (`episode.identity`, `risk.score`, `review.state`, queue counts, the index epoch, the caught-up state —
//! [`crate::views`]'s `ambient` entry names them) and the seven lines that answer the one question the view
//! asks: *is there anything I need to look at right now?*
//!
//! # Why a module and not another view in the shell
//!
//! The shell draws ambient under its own chrome, inside its own loop, over the rows the shell already
//! holds. The thing this ticket is about is the **mode**: a process that lives in a tmux split all day,
//! costing no animation and asking for no keystrokes. That process needs the same lines drawn with no
//! chrome at all, into a pane that may be 40 columns wide, from a caller that reads a store the shell may
//! not open. So the lines live here, as a function of data, and both the shell and `fathom ambient` render
//! the same function rather than keeping two copies of the copy.
//!
//! # Three rules this module keeps
//!
//! * **Every fact is handed over, none is computed here.** No store, no clock, no ranking: [`Glance`]
//!   arrives already read and already ranked (ADR-002 §4.4, §5.2). What *is* decided here is the order the
//!   reasons to look rank in, because that is a fact about the pane, and the pane owns the copy.
//! * **Unknown prints as `unknown`.** `0 open` and `unknown open` are different sentences and only one of
//!   them is a reason to relax (docs/concepts/ambient-mode.md). A [`QueueState`] that was never counted
//!   says so rather than printing a zero.
//! * **Caught-up is a designed state, not an empty one.** It is dimmed, it names itself, and it says what
//!   there is nothing of. No animation, no colour strobing, no cursor: this pane sits next to text someone
//!   is reading ([`Face::Faint`] is the whole palette of a quiet pane).
//!
//! [`lines`] and [`text`] are pure functions of the same [`Glance`] at the same size, twice: the draw is a
//! function of the facts, which is what lets a caller redraw *only* when the rendered text changed — the
//! property `fathom ambient`'s second acceptance criterion is about.

use fathom_analyze::insight::{QueueCounts, RiskView, short_id};
use fathom_core::time::Timestamp;

use crate::index::Read;
use crate::style::{Face, StyledLine};

/// The size ADR-006 §5.7 says the pane is ideal at.
pub const IDEAL: (usize, usize) = (80, 12);
/// The floor ADR-001 §4.1 promises: a 40-column tmux split, three rows of content.
pub const FLOOR: (usize, usize) = (40, 3);
/// How wide the reason line's label column is, plus the gap after it.
///
/// Sized to the longest label the pane can print (`index behind`), because a label column that only fits
/// the labels written today is a column that silently changes shape the day a fifth label arrives.
pub const LABEL_WIDTH: usize = 12;

/// Every line the pane draws when nothing has been trimmed: three facts, a gap, the reason, a gap, the
/// keys. Anything above [`FLOOR`]'s rows is space the pane may use and does not need.
pub const LINES: usize = 7;

/// What the pane would have you do, ranked by how much it is a *now* problem.
///
/// The order is the argument, and it is stated once so no two surfaces rank it differently:
///
/// 1. the pane cannot be trusted (no index, an index that would not open) — everything else on screen is
///    provisional until that is fixed, and a stale ambient pane is worse than none;
/// 2. the index does not know `HEAD` — the newest change is not on screen yet, whatever the screen says;
/// 3. nothing is keeping the index current and the last run was a while ago;
/// 4. there is an open episode the ranking calls high-risk;
/// 5. there is open work nothing scored — a queue of `?`s is not a caught-up queue;
/// 6. there is open work that is merely waiting;
/// 7. nothing. [`Need::Quiet`] is the resting state, and it is the only one that dims the pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Need {
    /// No index, or one that would not open.
    IndexBroken,
    /// `HEAD` is not in the index: the pane cannot see the newest work.
    HeadNotIndexed,
    /// No live watcher, and nothing indexed the repository recently.
    NothingWatching,
    /// An open episode the ranking puts in the high band.
    HighRisk,
    /// Open episodes with no risk score at all.
    Unranked,
    /// Open episodes, none of them high.
    OpenQueue,
    /// Nothing is waiting.
    Quiet,
}

impl Need {
    /// The stable spelling, for `--json` and for the pane's own label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Need::IndexBroken => "index_broken",
            Need::HeadNotIndexed => "head_not_indexed",
            Need::NothingWatching => "nothing_watching",
            Need::HighRisk => "high_risk",
            Need::Unranked => "unranked",
            Need::OpenQueue => "open_queue",
            Need::Quiet => "quiet",
        }
    }

    /// The two words the pane prints in the reason column.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Need::IndexBroken => "no index",
            Need::HeadNotIndexed => "index behind",
            Need::NothingWatching => "index stale",
            Need::HighRisk => "look now",
            Need::Unranked => "unscored",
            Need::OpenQueue => "waiting",
            Need::Quiet => "caught up",
        }
    }

    /// The face the reason is painted with.
    ///
    /// Only the three "something is wrong with this pane" states and [`Need::HighRisk`] get ink at all,
    /// and none of them get animation: the pane lives beside text someone is reading, so the loudest thing
    /// it may do is pick a colour (ADR-006 §10.2 — and the label carries the same fact, so the ink is
    /// never the only channel).
    #[must_use]
    pub const fn face(self) -> Face {
        match self {
            Need::IndexBroken | Need::HeadNotIndexed => Face::Failure,
            Need::NothingWatching | Need::HighRisk => Face::Warning,
            Need::Unranked | Need::OpenQueue => Face::Text,
            Need::Quiet => Face::Faint,
        }
    }
}

/// One reason to look, as the caller states it.
#[derive(Debug, Clone)]
pub struct Reason {
    /// Which kind of problem this is, which is what ranks it.
    pub kind: Need,
    /// The sentence: a checkable claim, not an adjective.
    pub text: String,
}

impl Reason {
    /// A reason.
    #[must_use]
    pub fn new(kind: Need, text: impl Into<String>) -> Self {
        Reason {
            kind,
            text: text.into(),
        }
    }
}

/// The newest episode, in the four columns the pane prints.
#[derive(Debug, Clone, Default)]
pub struct Latest {
    /// The episode id, in full; [`short_id`] decides how much of it is printed.
    pub id: String,
    /// The one-line summary the rail prints.
    pub summary: String,
    /// `agent`, `human`, `unknown` — the text channel, never an icon alone.
    pub author: String,
    /// When the episode began; `None` is "the row did not say", which prints `unknown`.
    pub created: Option<Timestamp>,
    /// `unread`, `read`, `flagged`, … — `review.state`, the field that says whether you have seen it.
    pub state: String,
    /// A provisional episode (uncommitted work). Printed distinctly and never counted as committed
    /// (docs/concepts/episode.md#provisional-episodes).
    pub provisional: bool,
    /// The risk score and its model, when anything scored it.
    pub risk: Option<RiskView>,
}

/// The queue's numbers, as counted by whoever read the store.
#[derive(Debug, Clone, Default)]
pub struct QueueState {
    /// Was the queue counted at all? `false` prints `unknown`, which is the whole point of the field.
    pub known: bool,
    /// The counts, when they were counted.
    pub counts: Option<QueueCounts>,
    /// How many open episodes have been sitting unread for more than a day — the number that turns "waiting"
    /// into "overdue". `None` when the caller did not look.
    pub over_day: Option<usize>,
}

/// What the index said about itself.
#[derive(Debug, Clone, Default)]
pub struct IndexState {
    /// Whether a store could be read at all — the five-way answer `fathom-tui`'s empty states use.
    pub read: Read,
    /// The published epoch.
    pub epoch: Option<u64>,
    /// `ready`, `building`, or nothing when the store never said.
    pub status: Option<String>,
    /// Live episodes.
    pub episodes: Option<u64>,
    /// The last index run's finish, unix millis. This is the "0:03 ago" the pane prints.
    pub last_utc: Option<i64>,
    /// Does the index know `HEAD`? `None` is "not asked", which prints as `unknown`.
    pub head_indexed: Option<bool>,
}

/// Who is keeping the index current, as far as a heartbeat can say.
#[derive(Debug, Clone, Default)]
pub struct WatchState {
    /// This pane is the party keeping it current (`fathom ambient`'s own indexing).
    pub here: bool,
    /// A watcher's heartbeat was found.
    pub present: bool,
    /// And its process is still there.
    pub alive: bool,
    /// Its own words, verbatim: `idle · quiet 4 s · polling`.
    pub summary: String,
    /// The heartbeat's age, for "the watcher stopped an hour ago".
    pub age: Option<std::time::Duration>,
}

/// Everything the glance pane draws, already read.
///
/// The caller (`fathom-cli`) owns the clock, the store and the ranking; the pane owns the copy and the
/// order. A [`Glance`] that is equal to the previous one renders equal bytes, which is what a caller
/// compares to decide whether the screen has to change at all.
#[derive(Debug, Clone)]
pub struct Glance {
    /// The instant the pane is drawn for, from the caller's clock (ADR-002 §4.4).
    pub now: Timestamp,
    /// The repository, as displayed (redacted by the caller — a pane must not leak a home path).
    pub root: String,
    /// `HEAD`'s branch, when there is one.
    pub branch: Option<String>,
    /// The newest episode.
    pub latest: Option<Latest>,
    /// The queue's numbers.
    pub queue: QueueState,
    /// The index's numbers.
    pub index: IndexState,
    /// Who is watching.
    pub watch: WatchState,
    /// Every reason the caller could find, in any order — [`most_important`] picks.
    pub reasons: Vec<Reason>,
    /// Anything the caller had to say that is not a reason to look: a build with no watcher feature, a
    /// queue that could not be counted, an episode the store would not describe.
    pub notes: Vec<String>,
    /// The episode `<Enter>` opens, when there is one: the thing to look at, which is the top of the queue
    /// and not necessarily the newest row. `None` and the pane says so rather than opening a surprise.
    pub drill: Option<String>,
}

impl Default for Glance {
    /// A pane that has been handed nothing: every fact is "not read", which renders as the empty pane's
    /// copy rather than as zeroes (`docs/concepts/ambient-mode.md`: unknown prints as `unknown`).
    fn default() -> Self {
        Glance {
            now: Timestamp::UNIX_EPOCH,
            root: String::new(),
            branch: None,
            latest: None,
            queue: QueueState::default(),
            index: IndexState::default(),
            watch: WatchState::default(),
            reasons: Vec::new(),
            notes: Vec::new(),
            drill: None,
        }
    }
}

impl Glance {
    /// The most important reason on screen, or `None` when the caller gave the pane nothing — which is
    /// read as caught up, and is the state the pane is designed to rest in.
    #[must_use]
    pub fn reason(&self) -> Option<&Reason> {
        most_important(self)
    }

    /// Is the reviewer caught up? `true` only when nothing is waiting *and* nothing about the pane itself
    /// needs fixing — a stale index over an empty queue is a broken pane, not a restful one.
    #[must_use]
    pub fn caught_up(&self) -> bool {
        !self.reasons.iter().any(|reason| reason.kind < Need::Quiet)
    }
}

/// The reason a reviewer most needs, which is the smallest [`Need`] the caller found.
#[must_use]
pub fn most_important(glance: &Glance) -> Option<&Reason> {
    glance
        .reasons
        .iter()
        .filter(|reason| reason.kind < Need::Quiet)
        .min_by(|left, right| {
            left.kind
                .cmp(&right.kind)
                .then_with(|| left.text.cmp(&right.text))
        })
}

/// What the *loop* knows about the reviewer's session, as opposed to what the repository knows about
/// itself. Two flags, both of them about the pane rather than the facts, which is why they are not
/// [`Glance`] fields.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mode {
    /// The reviewer pressed `p`: the facts on screen stay, and stop being refreshed.
    pub paused: bool,
    /// The reviewer pressed `?`: the key line becomes the whole legend.
    pub help: bool,
}

/// The pane, as styled lines, fitted to `cols` and truncated to `rows`.
///
/// The ladder is one step long, exactly as ADR-006 §6.5 describes the view's: at [`FLOOR`] or bigger every
/// line is drawn; below it the pane drops the reason line, then the index line, keeping the newest episode
/// and the queue count to the last. At fewer than [`FLOOR`] rows or columns there is nothing honest left,
/// and [`refusal`] is what a caller prints instead of drawing a lie.
#[must_use]
pub fn lines(glance: &Glance, mode: Mode, cols: usize, rows: usize) -> Vec<StyledLine> {
    if let Some(why) = refusal(cols, rows) {
        // The three refusal lines, padded, and the rest of the box empty: a refusal is a complete frame, and
        // it is the only frame this size gets. The loop's fingerprint sees it too, which is why a shrunken
        // pane redraws once with the refusal instead of freezing on the last thing that fitted.
        return finish(refusal_lines(why, cols), cols, rows.max(1));
    }
    let all = pane(glance, mode, cols);
    if rows >= all.len() {
        return finish(all, cols, rows);
    }
    // The ladder, longest to shortest. ADR-006 §6.5 gives this view exactly one step — content trim — and
    // the order it is sold in is the order here: the notes, then the keys, then the gap, then the index line.
    // What survives to the last three rows is the three ADR-001 §4.1 promises: what is newest, what is
    // waiting, and whether to look now.
    let kept = match rows {
        0..=3 => vec![all[0].clone(), all[1].clone(), all[4].clone()],
        4 => vec![
            all[0].clone(),
            all[1].clone(),
            all[2].clone(),
            all[4].clone(),
        ],
        5 => all[..5].to_vec(),
        _ => all[..rows.saturating_sub(1)].to_vec(),
    };
    finish(kept, cols, rows)
}

/// The refusal as lines: the first one loud, the remedies quiet.
fn refusal_lines(why: Vec<String>, cols: usize) -> Vec<StyledLine> {
    why.into_iter()
        .enumerate()
        .map(|(index, line)| {
            StyledLine::with_face(
                line,
                if index == 0 {
                    Face::Failure
                } else {
                    Face::Muted
                },
            )
            .fit(cols)
        })
        .collect()
}

/// The pane's lines, fitted to the width but not padded or cut to a height — the shape a pipe wants.
///
/// A terminal needs a box of exactly `rows`; a log, a tmux status line and `fathom ambient --once` need the
/// pane's own lines and nothing else. One function makes the content, [`finish`] turns it into a box, and
/// the two callers cannot disagree about the copy because there is no second copy of it.
#[must_use]
pub fn pane(glance: &Glance, mode: Mode, cols: usize) -> Vec<StyledLine> {
    if let Some(why) = refusal(cols, FLOOR.1) {
        return refusal_lines(why, cols);
    }
    let dim = glance.caught_up();
    let reason = most_important(glance);
    let need = reason.map_or(Need::Quiet, |reason| reason.kind);
    let mut out: Vec<StyledLine> = vec![
        latest_line(glance, dim),
        queue_line(glance, dim),
        index_line(glance, dim),
        StyledLine::plain(""),
        reason_line(glance, reason),
    ];
    for note in &glance.notes {
        out.push(StyledLine::with_face(note, Face::Muted));
    }
    out.push(StyledLine::plain(""));
    out.push(keys_line(mode, need, dim));
    out.into_iter().map(|line| line.fit(cols)).collect()
}

/// The same lines as text — `--plain`, a pipe, a screen reader, the golden file (ADR-006 §6.7).
#[must_use]
pub fn text(glance: &Glance, mode: Mode, cols: usize, rows: usize) -> Vec<String> {
    lines(glance, mode, cols, rows)
        .into_iter()
        .map(|line| line.text().to_owned())
        .collect()
}

/// [`text`] with no height limit: the pane's own lines, trimmed, for a caller with no rows to fill.
#[must_use]
pub fn text_lines(glance: &Glance, mode: Mode, cols: usize) -> Vec<String> {
    pane(glance, mode, cols)
        .into_iter()
        .map(|line| line.text().trim_end().to_owned())
        .collect()
}

/// The pane's fingerprint: the text it would draw, joined.
///
/// A caller that polls does not need to know what changed, only whether anything did — and the honest
/// definition of "something changed" for a display is "the screen would say something different". Ages are
/// bucketed (see [`age`]) so a quiet repository stops redrawing once a minute at worst, which is the
/// difference between a pane and a spinner.
#[must_use]
pub fn fingerprint(glance: &Glance, mode: Mode, cols: usize, rows: usize) -> String {
    text(glance, mode, cols, rows).join("\n")
}

/// What to print when the pane will not fit, and the remedy. `None` means it fits.
#[must_use]
pub fn refusal(cols: usize, rows: usize) -> Option<Vec<String>> {
    if cols >= FLOOR.0 && rows >= FLOOR.1 {
        return None;
    }
    // Three short lines rather than one sentence. A pane this small is the one place a long message would be
    // clipped to nonsense, and `AMBIENT REFUSED: this pane needs at le…` refuses and informs in the same
    // breath while informing is the half the reviewer needs. Each line is under the 40-column floor.
    Some(vec![
        "AMBIENT REFUSED — TOO SMALL".to_owned(),
        format!("needs {}×3 · this is {cols}×{rows}", FLOOR.0),
        "or: fathom ambient --once".to_owned(),
    ])
}

/// Is this size below the pane's floor? Kept beside the refusal so a caller asks the same question the
/// renderer answers.
#[must_use]
pub fn fits(cols: usize, rows: usize) -> bool {
    refusal(cols, rows).is_none()
}

/// The unix seconds a [`Timestamp`] carries, with the saturation stated rather than assumed away.
#[must_use]
fn now_seconds(stamp: Timestamp) -> i64 {
    stamp.as_unix_micros() / 1_000_000
}

/// An age in the pane's own buckets: `now`, `<15s`, `30s`, `7m`, `3h`, `2d`.
///
/// Bucketed on purpose, and the buckets are deliberately coarse below a minute. A glance pane sits beside a
/// terminal someone is reading, and every age that crosses a bucket is a frame that has to be redrawn; the
/// acceptance criterion this implements is "no redraws without events", and an age that moved every second
/// would be a redraw with no event in it. `<15s` is the honest shape of "somewhere below the bucket" — the
/// alternative, printing `now` for fourteen seconds, is the lie this crate refuses everywhere else.
#[must_use]
pub fn age(seconds: i64) -> String {
    let seconds = seconds.max(0);
    if seconds < 5 {
        return "now".to_owned();
    }
    if seconds < 15 {
        return "<15s".to_owned();
    }
    if seconds < 60 {
        return format!("{}s", seconds / 15 * 15);
    }
    if seconds < 3_600 {
        return format!("{}m", seconds / 60);
    }
    if seconds < 86_400 {
        return format!("{}h", seconds / 3_600);
    }
    format!("{}d", seconds / 86_400)
}

/// The same age, in the sentence form: `now`, `48s ago`, `7m ago`.
///
/// Split from [`age`] because `now ago` is not English, and a pane that prints seven facts a second gets
/// read by a person.
#[must_use]
pub fn ago(seconds: i64) -> String {
    let words = age(seconds);
    if words == "now" {
        words
    } else {
        format!("{words} ago")
    }
}

/// The same age from two [`Timestamp`]s, with the one honest failure spelled out.
#[must_use]
pub fn age_since(now: Timestamp, then: Timestamp) -> String {
    let secs = (now.as_unix_micros() - then.as_unix_micros()) / 1_000_000;
    if secs < 0 {
        return "unknown".to_owned();
    }
    age(secs)
}

/// The first line: `latest e8f1a2 · agent · 12m · risk 82 ✱ · unread`.
fn latest_line(glance: &Glance, dim: bool) -> StyledLine {
    let mut line = StyledLine::new();
    let Some(latest) = &glance.latest else {
        line.push(ink(dim, Face::Meta), "latest ");
        line.push(
            Face::Muted,
            match glance.index.read {
                Read::Present => "no episodes in the index",
                Read::Absent => "nothing indexed here yet — `fathom index`",
                Read::NoBackend => "this binary has no index backend (`fathom --version`)",
                Read::Unreadable => "the index would not open — `fathom doctor`",
                Read::Unknown => "no episode was read",
            },
        );
        return line;
    };
    line.push(ink(dim, Face::Meta), "latest ");
    line.push(ink(dim, Face::Emphasis), &short_id(&latest.id));
    if latest.provisional {
        // Marked distinctly and never folded into a committed count (docs/concepts/episode.md).
        line.push(ink(dim, Face::Marker), " · provisional");
    }
    line.push(Face::Muted, " · ");
    line.push(ink(dim, Face::Text), &latest.author);
    line.push(Face::Muted, " · ");
    line.push(
        ink(dim, Face::Text),
        &latest.created.map_or_else(
            || "unknown".to_owned(),
            |created| age_since(glance.now, created),
        ),
    );
    push_risk(&mut line, latest.risk.as_ref(), dim);
    line.push(Face::Muted, " · ");
    line.push(ink(dim, Face::Text), &latest.state);
    line
}

/// The ink for one fact, given whether the pane is resting.
///
/// The caught-up state has to be legible to somebody who is not reading the colour, so it is dim *all over*
/// — a faint "caught up" next to a bright episode id is a pane arguing with itself.
#[must_use]
fn ink(dim: bool, face: Face) -> Face {
    if dim { Face::Faint } else { face }
}

/// The risk phrase, with the band in the text channel and not only in the ink.
fn push_risk(line: &mut StyledLine, risk: Option<&RiskView>, dim: bool) {
    let Some(risk) = risk else {
        line.push(Face::Muted, " · risk ");
        line.push(ink(dim, Face::Warning), "unscored");
        return;
    };
    let score = i32::try_from(risk.score.max(0)).unwrap_or(i32::MAX);
    let (band, face) = if score >= 70 {
        ("✱ high", Face::Failure)
    } else if score >= 40 {
        ("• medium", Face::Warning)
    } else {
        ("· low", Face::Muted)
    };
    line.push(Face::Muted, " · risk ");
    line.push(ink(dim, face), &format!("{score} {band}"));
}

/// The second line: `queue  7 open · 2 snoozed · 1 over 24 h`.
fn queue_line(glance: &Glance, dim: bool) -> StyledLine {
    let mut line = StyledLine::new();
    let face = if dim { Face::Faint } else { Face::Muted };
    line.push(face, "queue  ");
    let state = &glance.queue;
    if !state.known {
        line.push(
            if dim { Face::Faint } else { Face::Warning },
            "unknown — the queue could not be read",
        );
        return line;
    }
    let counts = state.counts.unwrap_or_default();
    if state.known && counts.open == 0 {
        // Written here rather than borrowed from the full view's counts line, which says "caught up" for the
        // same state: one claim made in two places is two chances to make it wrongly, and an empty queue can
        // sit over an index that died an hour ago. This line states the count; the attention line is the only
        // place in the pane that says "caught up".
        line.push(
            if dim { Face::Faint } else { Face::Text },
            &format!(
                "0 open · {} dealt with",
                counts.total.saturating_sub(counts.open)
            ),
        );
    } else {
        line.push(if dim { Face::Faint } else { Face::Text }, &counts.line());
    }
    // A zero is left out rather than printed: the counts line drops its empty bands for the same reason,
    // and `0 over 24 h` reads as a fact about nothing.
    if let Some(over) = state.over_day.filter(|over| *over > 0) {
        line.push(Face::Muted, &format!(" · {over} over 24 h"));
    }
    line
}

/// The third line: `index  epoch 41 · ready · 3 412 eps · 3s ago · watching here`.
///
/// This is the line that makes the rest of the pane believeable, so it carries the epoch, the status, the
/// size and the age in one breath — the claim docs/concepts/ambient-mode.md calls "a claim you can
/// disbelieve at a glance". A missing number prints `?`, never a blank.
fn index_line(glance: &Glance, dim: bool) -> StyledLine {
    let mut line = StyledLine::new();
    let index = &glance.index;
    let value = if dim { Face::Faint } else { Face::Text };
    line.push(if dim { Face::Faint } else { Face::Meta }, "index  ");
    match index.read {
        Read::Absent => {
            line.push(Face::Warning, "no index — `fathom index` builds one");
            return line;
        }
        Read::Unreadable => {
            line.push(
                Face::Failure,
                "the index would not open — `fathom doctor` says why",
            );
            return line;
        }
        Read::NoBackend => {
            line.push(
                Face::Failure,
                "this binary was built without an index backend (`fathom --version`)",
            );
            return line;
        }
        Read::Unknown => {
            line.push(Face::Muted, "index state was not read");
            return line;
        }
        Read::Present => {}
    }
    line.push(
        value,
        &format!(
            "epoch {} · {} · {} eps · {}",
            index
                .epoch
                .map_or("?".to_owned(), |epoch| epoch.to_string()),
            index
                .status
                .clone()
                .unwrap_or_else(|| "status unknown".to_owned()),
            index
                .episodes
                .map_or("?".to_owned(), crate::app::format_thousands),
            index.last_utc.map_or_else(
                || "age unknown".to_owned(),
                |last| { ago(now_seconds(glance.now).saturating_sub(last).max(0)) }
            ),
        ),
    );
    // `HEAD not indexed` is deliberately *not* here: it is the pane's headline when it is true, and a
    // duplicate on this line would be the clause that got clipped at 80 columns and the clause that
    // pushed the watcher off the end at 40. The line keeps the four facts the reason line cannot carry.
    line.push(Face::Muted, " · ");
    line.push(watch_face(&glance.watch), &watch_words(&glance.watch));
    line
}

/// Who is keeping the index current, in the pane's own words.
fn watch_words(watch: &WatchState) -> String {
    if watch.here {
        return "watching here".to_owned();
    }
    if !watch.present {
        return "no watcher — `fathom watch`".to_owned();
    }
    if !watch.alive {
        return "watcher stopped".to_owned();
    }
    if watch.summary.is_empty() {
        "watching".to_owned()
    } else {
        watch.summary.clone()
    }
}

/// The ink for that sentence: a watcher that is not there is a warning, a dead one is a failure, and a
/// live one is just a fact.
fn watch_face(watch: &WatchState) -> Face {
    if watch.here {
        return Face::Text;
    }
    if !watch.present {
        return Face::Warning;
    }
    if watch.alive {
        Face::Muted
    } else {
        Face::Failure
    }
}

/// The line the pane exists for: the one reason, or the caught-up statement.
fn reason_line(glance: &Glance, reason: Option<&Reason>) -> StyledLine {
    let mut line = StyledLine::new();
    let Some(reason) = reason else {
        line.push(Face::Faint, "caught up  ");
        line.push(Face::Faint, &caught_up_words(glance));
        return line;
    };
    line.push(
        reason.kind.face(),
        &format!("{:<LABEL_WIDTH$}  ", reason.kind.label()),
    );
    line.push(
        if reason.kind == Need::Quiet {
            Face::Faint
        } else {
            Face::Text
        },
        &reason.text,
    );
    line
}

/// What "caught up" is a claim about: nothing waiting, and how long the index has been saying so.
///
/// The second half is not decoration. "Caught up" over an index that stopped an hour ago is a different
/// claim from "caught up" over one that finished three seconds ago, and the pane has the number, so it
/// prints it instead of letting the reviewer relax for the wrong reason (ADR-001 §7.2).
fn caught_up_words(glance: &Glance) -> String {
    let waiting = glance.queue.counts.as_ref().map_or_else(
        || "nothing is waiting".to_owned(),
        |counts| {
            format!(
                "{} of {} episodes dealt with",
                counts.total.saturating_sub(counts.open),
                counts.total
            )
        },
    );
    match glance.index.last_utc {
        Some(last) => {
            let age = now_seconds(glance.now).saturating_sub(last).max(0);
            format!(
                "{waiting} · index current {}",
                if age < 5 {
                    "now".to_owned()
                } else {
                    format!("{age} ago")
                }
            )
        }
        None => format!("{waiting} · no index run was recorded"),
    }
}

/// The last line: the keys, and nothing else.
fn keys_line(mode: Mode, need: Need, dim: bool) -> StyledLine {
    let mut line = StyledLine::new();
    if mode.help {
        line.push(
            Face::Muted,
            "<Enter> open · r re-read · p pause · q quit · ? this line",
        );
        return line;
    }
    if mode.paused {
        line.push(ink(dim, Face::Warning), "paused");
        line.push(
            Face::Faint,
            " — `p` resumes; the facts above are as of the pause",
        );
        return line;
    }
    line.push(Face::Faint, "<Enter> ");
    line.push(
        Face::Faint,
        match need {
            Need::Quiet => "the full view",
            _ => "open it",
        },
    );
    line.push(Face::Faint, " · r re-read · p pause · q quit · ? keys");
    line
}

/// Fit, pad and cut to the box, so every line the pane returns is exactly `cols` wide and there are
/// exactly `rows` of them — the contract every other pane in this crate keeps with its renderer.
fn finish(lines: Vec<StyledLine>, cols: usize, rows: usize) -> Vec<StyledLine> {
    let mut out: Vec<StyledLine> = lines
        .into_iter()
        .map(|line| line.fit(cols).padded(cols))
        .collect();
    while out.len() < rows {
        out.push(StyledLine::plain("").padded(cols));
    }
    out.truncate(rows);
    out
}

// ---------------------------------------------------------------------------------------------
// the mode: one loop, no chrome, no ticks
// ---------------------------------------------------------------------------------------------

use std::io::Write as _;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::{Terminal, TerminalOptions, Viewport};

use crate::error::Error;
use crate::event::{Bus, Message, Received, spawn_input_reader};
use crate::terminal::{Guard, Signals, TerminalRestore};

/// The default poll. Chosen by the acceptance criteria, not by taste: a new high-risk episode has to be on
/// screen inside a second, and the poll itself is a `stat` and a ref read, so a cheaper poll buys nothing
/// and a slower one misses the criterion. Slower is available (`fathom ambient --interval-ms`) for a
/// repository where even the cheap probe is not free.
pub const DEFAULT_POLL: Duration = Duration::from_millis(400);

/// The facts, on demand.
///
/// One method, because the pane needs one thing: *tell me what is true now*. The caller decides what
/// "now" cost — a `stat` when nothing moved, a store read when something did, an index run when `HEAD`
/// moved — and the loop never has to know. It is the same inversion [`crate::frame::FrameSink`] is for
/// the shell: the loop owns the screen and the clock, the caller owns the data (ADR-002 §4.3, §5.2).
pub trait Feed {
    /// The current facts. `force` asks for a re-read even if nothing appears to have moved, which is what
    /// `r` means; a caller that caches should still not cache for longer than it can defend.
    fn glance(&mut self, force: bool) -> Glance;

    /// Is the run over for a reason the loop cannot see — a wall-clock deadline, a stop file, a signal the
    /// caller owns? Checked once per pass.
    fn expired(&self) -> bool {
        false
    }
}

/// Where a glance run put the reviewer when it finished.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum End {
    /// `q`, `<C-c>`, or a signal: the reviewer left the pane.
    #[default]
    Quit,
    /// `<Enter>`: the reviewer asked for the full view of that episode.
    Drill {
        /// The episode to open, from [`Glance::drill`]. `None` means the pane had nothing to point at.
        episode: Option<String>,
    },
    /// The caller's own stop condition said so (a deadline, `--until`).
    Expired,
    /// Every sender is gone, which in practice means the caller's loop stopped.
    Disconnected,
}

impl End {
    /// The one-line summary a caller prints on the way out.
    #[must_use]
    pub fn summary(&self) -> String {
        match self {
            End::Quit => "left the glance pane".to_owned(),
            End::Drill { episode: Some(id) } => format!("opening {id} in full"),
            End::Drill { episode: None } => {
                "asked for the full view with nothing selected".to_owned()
            }
            End::Expired => "the run reached its deadline".to_owned(),
            End::Disconnected => "the run stopped".to_owned(),
        }
    }
}

/// What a glance run counted, and how it ended.
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// How many times the facts were asked for.
    pub polls: u64,
    /// How many times the pane was actually drawn — the number the "no redraws without events" criterion
    /// is about, and the one a `--self-check` run prints.
    pub draws: u64,
    /// How many keys the reviewer pressed.
    pub keys: u64,
    /// How long the run was open.
    pub elapsed: Duration,
    /// How much of that was spent working rather than waiting, which is what a CPU budget is really about.
    pub busy: Duration,
    /// Why it stopped.
    pub end: End,
    /// The terminal refused mid-run, the reason. A pane that cannot repaint is a stated pane, not a blank
    /// one; the loop stops rather than spinning against a closed pipe.
    pub failure: Option<String>,
}

impl Report {
    /// The run's own numbers in one line, for stderr on the way out.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "polls {} · draws {} · keys {} · busy {:.1} ms of {} ms · {}",
            self.polls,
            self.draws,
            self.keys,
            f64::from(u32::try_from(self.busy.as_micros()).unwrap_or(u32::MAX)) / 1_000.0,
            self.elapsed.as_millis(),
            self.end.summary()
        )
    }

    /// Busy time as a share of wall time — the pane's own idle cost, measured rather than claimed.
    #[must_use]
    pub fn busy_share(&self) -> f64 {
        if self.elapsed.is_zero() {
            return 0.0;
        }
        f64::from(u32::try_from(self.busy.as_micros()).unwrap_or(u32::MAX))
            / f64::from(u32::try_from(self.elapsed.as_micros()).unwrap_or(u32::MAX))
    }
}

/// Somewhere to put a glance pane. The trait is one method because the pane is one draw.
pub trait GlanceSink {
    /// Draw the pane. The sink owns the size the terminal has.
    ///
    /// # Errors
    ///
    /// Why the screen refused, as [`Error::Terminal`].
    fn draw(&mut self, lines: &[StyledLine]) -> Result<(), Error>;

    /// The box to draw into.
    fn size(&self) -> (u16, u16);
}

/// A sink that keeps every frame as text: the headless path and every test of the loop.
#[derive(Debug, Default)]
pub struct HeadlessSink {
    /// The frames drawn, oldest first.
    pub frames: Vec<Vec<String>>,
    /// The size to claim, so a test decides the terminal rather than inheriting one.
    pub size: (u16, u16),
}

impl HeadlessSink {
    /// One recorded frame, as plain text.
    #[must_use]
    pub fn frame(&self, index: usize) -> Vec<String> {
        self.frames.get(index).cloned().unwrap_or_default()
    }
}

impl GlanceSink for HeadlessSink {
    fn draw(&mut self, lines: &[StyledLine]) -> Result<(), Error> {
        self.frames
            .push(lines.iter().map(|line| line.text().to_owned()).collect());
        Ok(())
    }

    fn size(&self) -> (u16, u16) {
        self.size
    }
}

/// A sink on a real terminal, folding the theme to the depth the terminal answered for (`e5.5`, `e5.6`).
pub struct TerminalSink<B: Backend> {
    /// The terminal, held for the length of the run.
    pub terminal: Terminal<B>,
    /// The palette, already resolved by the caller.
    pub theme: crate::theme::Theme,
}

impl<B: Backend> GlanceSink for TerminalSink<B> {
    fn draw(&mut self, lines: &[StyledLine]) -> Result<(), Error> {
        let theme = self.theme.clone();
        self.terminal
            .draw(|frame| crate::render::blit_themed(frame.buffer_mut(), lines, &theme))
            .map(|_| ())
            .map_err(|err| Error::Terminal {
                why: err.to_string(),
            })
    }

    fn size(&self) -> (u16, u16) {
        self.terminal
            .size()
            .map_or((80, 12), |size| (size.width, size.height))
    }
}

/// How to run the pane.
#[derive(Debug, Clone)]
pub struct RunOptions {
    /// How often to ask the feed. See [`DEFAULT_POLL`].
    pub poll: Duration,
    /// Take the alternate screen. `false` draws inline, which keeps the pane in scrollback.
    pub alt_screen: bool,
    /// Stop after this many draws — how a test, and `--once`, drive the same loop a person uses.
    pub max_draws: Option<u64>,
    /// The palette to paint with. The caller resolves it (ADR-002 §4.5); the pane has no opinion.
    pub theme: crate::theme::Theme,
    /// What the terminal said it is, for the same reason: colour depth is the caller's read, not the
    /// pane's (ADR-002 §3).
    pub term: crate::terminal::Probe,
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions {
            poll: DEFAULT_POLL,
            alt_screen: true,
            max_draws: None,
            theme: crate::theme::dark(),
            term: crate::terminal::Probe::unknown(),
        }
    }
}

/// The loop, with no opinion about where the pane goes or where the facts come from.
///
/// It is tickless in the sense ADR-006 §0.1 means: it blocks on the bus for one poll interval, drains
/// everything that arrived, asks the feed once, and draws **only if the text changed**. A quiet repository
/// therefore costs one `stat` and one ref read per interval and no bytes to the terminal at all, which is
/// the whole of the second acceptance criterion.
#[must_use]
pub fn run_loop(
    feed: &mut dyn Feed,
    bus: &Bus,
    sink: &mut dyn GlanceSink,
    signals: Option<&Signals>,
    options: &RunOptions,
) -> Report {
    let started = Instant::now();
    let mut report = Report::default();
    let mut mode = Mode::default();
    let mut drawn: Option<String> = None;
    let mut last: Option<Glance> = None;
    let mut force = true;
    let mut quit: Option<End> = None;
    while quit.is_none() {
        if let Some(signal) = signals.and_then(Signals::take) {
            quit = Some(match signal {
                crate::terminal::Signal::Interrupt
                | crate::terminal::Signal::Terminate
                | crate::terminal::Signal::Hangup => End::Quit,
            });
        }
        if quit.is_none() {
            let batch = match bus.recv_timeout(options.poll) {
                Received::Message(message) => {
                    let mut batch = vec![message];
                    batch.extend(bus.drain());
                    batch
                }
                Received::Disconnected => {
                    quit = Some(End::Disconnected);
                    Vec::new()
                }
                Received::Timeout => Vec::new(),
            };
            for message in batch {
                report.keys += 1;
                match message {
                    Message::Key(key) => {
                        if key.code == KeyCode::Esc
                            || key.code == KeyCode::Char('q')
                            || (key.modifiers.contains(KeyModifiers::CONTROL)
                                && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('d')))
                        {
                            quit = Some(End::Quit);
                        } else {
                            match key.code {
                                KeyCode::Enter => quit = Some(End::Drill { episode: None }),
                                KeyCode::Char('r') => force = true,
                                KeyCode::Char('p') | KeyCode::Char(' ') => {
                                    mode.paused = !mode.paused;
                                }
                                KeyCode::Char('?') => mode.help = !mode.help,
                                _ => {}
                            }
                        }
                    }
                    Message::Resize { .. } | Message::Repaint => force = true,
                    _ => {}
                }
            }
        }
        if quit.is_none() && feed.expired() {
            quit = Some(End::Expired);
        }
        if quit.is_some() {
            break;
        }
        let worked = Instant::now();
        if mode.paused {
            // Paused means paused: the facts on screen stop being refreshed, and nothing is asked for on
            // the reviewer's behalf until `p` says otherwise. `--once` never reaches this branch.
        } else {
            let glance = feed.glance(force);
            last = Some(glance.clone());
            force = false;
            report.polls += 1;
            let (cols, rows) = sink.size();
            let pane = lines(&glance, mode, usize::from(cols), usize::from(rows));
            // The redraw test is on the *text*: if the screen would say the same thing, nothing is
            // written. Ages are bucketed so this stays true of a quiet repository for minutes at a time.
            let printed = pane
                .iter()
                .map(|line| line.text().to_owned())
                .collect::<Vec<_>>()
                .join("\n");
            if drawn.as_deref() != Some(printed.as_str()) {
                if let Err(err) = sink.draw(&pane) {
                    report.failure = Some(err.to_string());
                    report.end = End::Quit;
                    break;
                }
                report.draws += 1;
                drawn = Some(printed);
                if let Some(limit) = options.max_draws
                    && report.draws >= limit
                {
                    quit = Some(End::Quit);
                }
            }
        }
        report.busy += worked.elapsed();
    }
    // `<Enter>` means "the episode this pane was talking about", so it is taken from the facts actually on
    // screen — not from a fresh read, which could have moved in the milliseconds since.
    if let Some(End::Drill { episode }) = &mut quit {
        *episode = last
            .as_ref()
            .and_then(|glance| glance.drill.clone())
            .or_else(|| feed.glance(false).drill.clone());
    }
    report.end = quit.unwrap_or(End::Quit);
    report.elapsed = started.elapsed();
    report
}

/// Open the screen, run [`run_loop`], and put the screen back before anything else happens.
///
/// Mirrors [`crate::frame::run`] exactly where mirroring matters — the restore guard exists before the
/// first draw and runs before the caller prints anything — and nowhere else, because the glance pane has
/// no panes, no keymap and no session to restore.
///
/// # Errors
///
/// [`Error::Terminal`] if the screen could not be taken or a frame could not be written.
pub fn run(feed: &mut dyn Feed, options: &RunOptions) -> Result<Report, Error> {
    let restore = TerminalRestore::stdout_sink();
    crate::terminal::install_panic_hook(&restore);
    let (signals, notices) = crate::terminal::install_signal_hooks();
    let guard = Guard::take(&restore, options.alt_screen).map_err(|why| Error::Terminal { why })?;
    let terminal = Terminal::with_options(
        CrosstermBackend::new(std::io::stdout()),
        TerminalOptions {
            viewport: Viewport::Fullscreen,
        },
    )
    .map_err(|err| Error::Terminal {
        why: err.to_string(),
    })?;
    let _ = crossterm::execute!(std::io::stdout(), crossterm::cursor::Hide);
    for notice in &notices {
        // A signal hook the OS refused is worth a line here and now, while there is a stderr to put it
        // on; once the screen is taken there is nowhere honest left to say it.
        let _ = writeln!(std::io::stderr(), "fathom: {notice}");
    }
    let mut sink = TerminalSink {
        terminal,
        theme: options.theme.resolve(options.term.color()),
    };
    let bus = Bus::new();
    let reader = spawn_input_reader(Arc::clone(&bus));
    let report = run_loop(feed, &bus, &mut sink, Some(&signals), options);
    drop(reader);
    if options.alt_screen {
        // Empty the buffer while it is still ours. The alternate screen survives being left, so a pane
        // that simply walked away leaves its last sentence behind for the next thing that takes the
        // screen — which, after `<Enter>`, is the full view, and a full view with a stale glance line
        // welded into row four is a bug report waiting to happen. Inline mode is deliberately skipped:
        // there the drawing is in the reviewer's own scrollback, and clearing it would be vandalism.
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
            crossterm::cursor::MoveTo(0, 0)
        );
    }
    drop(guard);
    if let Some(why) = &report.failure {
        return Err(Error::Terminal { why: why.clone() });
    }
    Ok(report)
}
