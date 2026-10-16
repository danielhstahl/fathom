//! `fathom_tui::timeline::render` — how a rail row is built, and what it gives up when the pane is
//! narrow.
//!
//! ADR-006 §5.2 fixes the drop order and this file implements exactly it: **sparkline first, then
//! relative time to date-only, then author, then the risk number — and the state glyph survives
//! last**, because the marker is the one column that also carries *state* with colour switched off
//! (§10.2). Deciding that in one function means the ladder is testable at every width instead of
//! being a rumour in a comment, and `tests/timeline_behaviour.rs` walks it.
//!
//! Nothing here touches the rail's own state: the row is a pure function of one episode, the frame's
//! instant and the loaded corpus's scale, which is what lets a golden file pin a row without a whole
//! application in the fixture.

use fathom_core::time::Timestamp;
use unicode_width::UnicodeWidthStr;

use crate::style::{Face, StyledLine};

use super::days;
use super::episode::{AuthorKind, Episode};

/// How many cells the change-size sparkline draws.
pub const SPARK_CELLS: usize = 6;

/// The faces a rail row uses, in one table so `e5.6`'s theme has one thing to map. Not colours —
/// [`Face::style`] is the only place a colour is named; this is the *choice* of face per column, so
/// "the id is metadata and the marker is a state" is one editable statement.
mod face {
    use super::Face;
    pub(super) const ID: Face = Face::Muted;
    pub(super) const TIME: Face = Face::Text;
    pub(super) const AUTHOR: Face = Face::Text;
    pub(super) const SIZE: Face = Face::Text;
    pub(super) const SPARK: Face = Face::Faint;
    pub(super) const RISK: Face = Face::Number;
    pub(super) const STATE: Face = Face::Marker;
    pub(super) const MISSING: Face = Face::Warning;
    pub(super) const GROUP: Face = Face::Dir;
    pub(super) const CURSOR: Face = Face::Emphasis;
}

/// Which columns a rail row has room for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Columns {
    /// Everything: `▸ e8f1a2 0:12 agent  +412 −90 ●●●●○○ 82 ⚑` (ADR-006 §5.2's own row).
    Full,
    /// No sparkline — the first thing the ADR sells.
    NoSparkline,
    /// Relative time collapses to a date: `14 Oct` instead of `0:12`.
    DateOnly,
    /// No author badge.
    NoAuthor,
    /// No risk *number*; the state glyph stays, because it is also the state.
    NoRisk,
    /// Identity and state only — the 18-column strip the ladder sells the rail down to.
    Identity,
}

impl Columns {
    /// The width ladder, in cells of rail content.
    ///
    /// The thresholds are the pane's own documented numbers from the view registry (`ideal_cols` 42,
    /// `min_cols` 32, `strip_width` 18, `hard_min` 28) with the columns' own widths added up, so the
    /// ladder and the layout come from the same arithmetic rather than two guesses.
    #[must_use]
    pub const fn for_width(width: usize) -> Columns {
        if width >= 40 {
            Columns::Full
        } else if width >= 34 {
            Columns::NoSparkline
        } else if width >= 32 {
            Columns::DateOnly
        } else if width >= 27 {
            Columns::NoAuthor
        } else if width >= 22 {
            Columns::NoRisk
        } else {
            Columns::Identity
        }
    }

    /// The one-line name a test failure and a toast both print.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Columns::Full => "all six columns",
            Columns::NoSparkline => "no sparkline",
            Columns::DateOnly => "date instead of age",
            Columns::NoAuthor => "no author",
            Columns::NoRisk => "no risk number",
            Columns::Identity => "id and state only",
        }
    }
}

/// The corpus's scale for the sparkline: the biggest change among the loaded rows.
///
/// The sparkline is *relative to this rail*, which is the honest reading: `●●●●○○` means "large
/// compared with what else is here today", which is what a reviewer uses it for, and it does not
/// pretend to a units-free absolute that no two repositories would share.
#[derive(Debug, Clone, Copy, Default)]
pub struct Scale {
    /// The largest [`Episode::size`] among the loaded rows, at least 1 so the log has something to
    /// divide by.
    pub max: u64,
}

impl Scale {
    /// One pass over the rows.
    #[must_use]
    pub fn of(episodes: &[Episode]) -> Scale {
        Scale {
            max: episodes.iter().map(Episode::size).max().unwrap_or(1).max(1),
        }
    }

    /// How many of [`SPARK_CELLS`] cells this episode fills: a log scale, so a 4 000-line commit does
    /// not flatten every 40-line one to nothing, and a zero-size episode still lights one cell rather
    /// than looking absent.
    #[must_use]
    pub fn level(&self, episode: &Episode) -> usize {
        let size = episode.size();
        if size == 0 {
            return 1;
        }
        // `ceil(6 · ln(size+1) / ln(max+1))`, both sides logged: a linear scale would put every
        // ordinary commit at one cell beside an agent's 4 000-line turn, and the column would stop
        // being a comparison. `+1` because `ln(1) = 0` would put the smallest change at zero cells,
        // and an empty cell run reads as "no change" rather than "small".
        let cell = f64::from(u32::try_from(SPARK_CELLS).unwrap_or(6));
        let numerator = f64::from(size.min(u64::from(u32::MAX)) as f32 + 1.0).ln();
        let denominator = f64::from(self.max.min(u64::from(u32::MAX)) as f32 + 1.0)
            .ln()
            .max(1.0);
        ((numerator / denominator * cell).ceil() as usize).clamp(1, SPARK_CELLS)
    }

    /// The sparkline itself, or `bin` for a change that is all binary — a row of dots about a binary
    /// file would be a chart of nothing.
    #[must_use]
    pub fn spark(&self, episode: &Episode) -> String {
        if episode.n_binary > 0 && episode.n_add + episode.n_del == 0 {
            return "bin".to_owned();
        }
        let filled = self.level(episode);
        let mut out = String::with_capacity(SPARK_CELLS * 3);
        for index in 0..SPARK_CELLS {
            out.push(if index < filled { '●' } else { '○' });
        }
        out
    }
}

/// One episode row, fitted to `width`.
///
/// The columns are the ADR-006 §5.2 set in the ADR's order, and `columns` picks how far down the
/// ladder the width has pushed them. The extra rule here is the one the ladder exists to make true:
/// when the row still does not fit the pane — a wide `+1 204 −904`, a six-digit risk, a terminal
/// narrower than any documented minimum — the cells that go are the risk and then the change size,
/// never the state glyph, because the glyph is the only cell that also encodes *state* under
/// `NO_COLOR`. A rail that clipped the glyph away would be dropping the one column the ladder says
/// it does not drop, and the truncation `…` would not make that honest.
#[must_use]
pub fn episode_line(
    episode: &Episode,
    now: Timestamp,
    scale: Scale,
    columns: Columns,
    cursor: bool,
    width: usize,
) -> StyledLine {
    // Every cell carries the priority it surrenders at on overflow: the bigger the number, the sooner
    // it goes. `0` is the identity, `1` the state — the two cells that are never surrendered.
    let mut cells: Vec<(Face, String, u8)> = vec![(
        face::CURSOR,
        if cursor {
            "▸".to_owned()
        } else {
            " ".to_owned()
        },
        0,
    )];
    cells.push((face::ID, format!(" {}", episode.short_id()), 0));
    let time = match columns {
        Columns::Full | Columns::NoSparkline => days::relative(now, episode.created),
        _ => days::date_only(episode.created),
    };
    cells.push((face::TIME, format!(" {time}"), 2));
    if columns < Columns::NoAuthor {
        cells.push((
            if episode.author == AuthorKind::Unknown {
                face::MISSING
            } else {
                face::AUTHOR
            },
            format!(" {:<5}", episode.author.badge()),
            3,
        ));
    }
    cells.push((
        face::SIZE,
        format!(" +{} −{}", compact(episode.n_add), compact(episode.n_del)),
        5,
    ));
    if columns < Columns::NoSparkline {
        cells.push((face::SPARK, format!(" {}", scale.spark(episode)), 4));
    }
    match episode.risk {
        Some(risk) if columns < Columns::NoRisk => {
            cells.push((face::RISK, format!(" {risk}"), 6));
        }
        Some(_) => {}
        // `unknown` is a value, and it prints: an empty risk column reads as "this episode is
        // unranked", which is a different claim from "nothing has ranked it yet" (ADR-001 §7.7).
        None => cells.push((face::MISSING, " ?".to_owned(), 6)),
    }
    // The state marker: unread, opened, accepted, flagged, refused, snoozed — or `?` for a row whose
    // state the index did not say. Dead episodes carry `gone` as well, because a sign-off on history
    // that is no longer here must not be quiet about it (ADR-003 I2).
    let marker = if episode.live {
        episode.state.glyph().to_owned()
    } else {
        "gone".to_owned()
    };
    cells.push((face::STATE, format!(" {marker}"), 1));

    let mut used: usize = cells.iter().map(|(_, text, _)| text.width()).sum();
    // Surrender the most expendable cell until the row fits, then stop: below the identity-plus-state
    // floor there is nothing left to drop, and `fit` will clip with `…` and say so.
    while used > width {
        let Some(worst) = cells
            .iter()
            .map(|(_, _, priority)| *priority)
            .max()
            .filter(|priority| *priority > 1)
        else {
            break;
        };
        let index = cells
            .iter()
            .rposition(|(_, _, priority)| *priority == worst)
            .unwrap_or(0);
        used -= cells[index].1.width();
        cells.remove(index);
    }

    let mut line = StyledLine::new();
    for (face, text, _) in cells {
        line.push(face, &text);
    }
    line
}

/// The empty rail's own rows: the statement, wrapped to the pane, and nothing else.
///
/// Drawn as content rather than as a chrome line because the pane's body is where a reviewer looks,
/// and because a statement that only appears in the strip is a statement that gets truncated
/// (ADR-006 §6.4 makes the same argument about the status line).
#[must_use]
pub fn empty_lines(words: &str, width: usize, visible: usize) -> Vec<StyledLine> {
    let body = width.saturating_sub(2).max(8);
    let lines = wrap(words, body);
    let mut out: Vec<StyledLine> = Vec::with_capacity(visible);
    for (row, line) in lines.iter().take(visible).enumerate() {
        let mut styled = StyledLine::new();
        styled.push(Face::Warning, if row == 0 { "· " } else { "  " });
        styled.push(Face::Warning, line);
        out.push(styled.padded(width));
    }
    if lines.len() > visible && visible > 0 {
        let mut styled = StyledLine::new();
        styled.push(
            Face::Muted,
            &format!(
                "  … {} more rows of the statement",
                lines.len() - visible + 1,
            ),
        );
        out.pop();
        out.push(styled.padded(width));
    }
    while out.len() < visible {
        out.push(StyledLine::plain("").padded(width));
    }
    out
}

/// Greedy word wrap at `width` cells, shared by the rail's empty statement and the card's fields.
///
/// A word that fits moves to the next line whole; only a single word longer than the pane is broken,
/// because "…a boundaryru / le, not a fact" is what a naive character-wrap writes and it is unreadable
/// in a way that clipping plainly is not.
#[must_use]
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let mut word = word;
        // A word longer than the whole pane: fill the line, then continue with the remainder.
        while word.chars().count() > width {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            let head: String = word.chars().take(width).collect();
            word = &word[head.len()..];
            if word.is_empty() {
                current.push_str(&head);
                break;
            }
            lines.push(head);
        }
        if word.is_empty() {
            continue;
        }
        if !current.is_empty() && current.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// A group header: `Tue 14 Oct ── 6 episodes`, dimmed because it is structure, not content.
#[must_use]
pub fn group_line(label: &str, cursor: bool) -> StyledLine {
    let mut line = StyledLine::new();
    line.push(face::CURSOR, if cursor { "▸" } else { " " });
    line.push(face::GROUP, label);
    line
}

/// The counted stand-in for a collapsed group's rows — never a silent gap.
#[must_use]
pub fn hidden_line(count: usize, cursor: bool) -> StyledLine {
    let mut line = StyledLine::new();
    line.push(face::CURSOR, if cursor { "▸" } else { " " });
    line.push(Face::Faint, &format!("··· {count} more — <Space> opens"));
    line
}

/// `12 040`, the same thousands separator the rest of the frame uses, but only past four digits so
/// an ordinary `+412` stays the width it is in ADR-006 §5.2's mock.
#[must_use]
pub fn compact(value: u64) -> String {
    if value < 1000 {
        return value.to_string();
    }
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(ch);
    }
    out
}
