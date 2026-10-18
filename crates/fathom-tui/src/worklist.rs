//! `fathom_tui::worklist` — the Queue view's ranked list (`reco-c05`; `e4.5` owns what happens next).
//!
//! ADR-006 §5.1 names the queue *"the home view"* and draws it with `risk 82` in every row and a strip
//! that says `order: risk × staleness × blast radius (w)`. Until this module the pane drew an
//! apology: the shell had no ranked rows to put in it, so epic 1's acceptance question — *find and read
//! the three riskiest episodes* — had no screen to be answered on, and the ticket that recorded the
//! failure (`reco-c05`) listed the empty queue as one of the four reasons.
//!
//! What is here is the **list**, and only the list:
//!
//! * [`ranked`] puts the episodes the shell was given into the order the ranking says, with the
//!   deterministic tie-breaks `e4.5` will need anyway — score, then churn, then recency, then id, so
//!   two runs over the same index show the same screen (ADR-001 §7.4);
//! * [`lines`] draws two rows per episode the way §5.1's mock does: the identity, the scale, the
//!   number and the surface badge, then the one reason that put it there, citing a path;
//! * [`strip`] names the ranking function in the pane's own footer, because §5.1's note is explicit
//!   that a strip which does not say *how* it ordered is a strip that asks for trust.
//!
//! What is deliberately **not** here is the queue's workflow: `x`/`f`/`-`/`z` dispositioning into the
//! review ledger (`e4.6`), staleness and blast radius in the ordering (`e4.5`, which needs `e4.4`'s
//! features for it), and queue counts by severity. Rows are ranked, not triaged, and the strip says so.
//!
//! Nothing in this module computes a number. The order is a sort of values the caller handed over
//! ([`crate::timeline::Risk`], which `fathom-cli` fills from `fathom_analyze::risk`), which is what
//! keeps "the TUI contains no analysis logic" true while the panes are still the thing that answers.

use std::cmp::Ordering;

use fathom_core::time::Timestamp;

use crate::layout::display_width;
use crate::style::{Face, StyledLine};
use crate::timeline::{Episode, compact, relative};

/// The queue's total order, as one comparator, so the order the rail caches and the order [`ranked`]
/// builds cannot drift apart.
///
/// Ranked rows first, highest score first; unranked rows after every ranked one, in the rail's own
/// order. "Nothing ranked this" is a statement about the tool, and must not be smuggled into the
/// ranking as a low score.
#[must_use]
pub fn precede(left: &Episode, right: &Episode) -> Ordering {
    let score = |episode: &Episode| episode.risk().as_ref().map_or(-1_i64, |risk| risk.score);
    score(right)
        .cmp(&score(left))
        .then_with(|| right.size().cmp(&left.size()))
        .then_with(|| right.created.cmp(&left.created))
        .then_with(|| left.id.cmp(&right.id))
}

/// The queue's order: the given rows, riskiest first.
///
/// The tie-breaks are part of the contract, not a detail — `e4.5`'s acceptance criterion says queue
/// ordering is reproducible and that ties break deterministically, and a rank that moved between two
/// runs of the same index would take its printed reasons with it.
///
/// Unranked episodes go last, in the rail's own order (newest first, then id): "nothing ranked this"
/// is a statement about the tool, and must not be smuggled into the ranking as a low score.
#[must_use]
pub fn ranked<'a, I>(rows: I) -> Vec<&'a Episode>
where
    I: IntoIterator<Item = &'a Episode>,
{
    let mut order: Vec<&'a Episode> = rows.into_iter().collect();
    order.sort_by(|left, right| precede(left, right));
    order
}

/// Draw the worklist: two rows per episode, exactly `rows` lines, padded to `width`.
///
/// Row one is §5.1's row with the sparkline traded for the surface badge — the column that answers
/// "why is a three-line change up here". Row two is the single biggest reason, with the path that
/// carries it. `cursor` indexes `episodes`, not lines.
#[must_use]
pub fn lines(episodes: &[&Episode], cursor: usize, rows: usize, width: usize) -> Vec<StyledLine> {
    let mut body: Vec<StyledLine> = Vec::new();
    for (row, episode) in episodes.iter().enumerate() {
        head(&mut body, episode, row == cursor, width);
        reason(&mut body, episode, width);
    }
    if body.is_empty() {
        body.push(StyledLine::with_face(
            "nothing to rank — the shell was handed no episodes",
            Face::Muted,
        ));
    }
    if body.len() > rows && rows > 1 {
        let hidden = body.len() - rows + 1;
        body.truncate(rows.saturating_sub(1));
        body.push(StyledLine::with_face(
            format!("·· {hidden} more rows — `j` walks the list, `<C-w>o` fills the frame"),
            Face::Faint,
        ));
    }
    while body.len() < rows {
        body.push(StyledLine::plain(""));
    }
    body.truncate(rows);
    body.into_iter().map(|line| line.padded(width)).collect()
}

/// Plain-text projection of [`lines`], for `--plain`, the screen reader and the golden files.
#[must_use]
pub fn text(episodes: &[&Episode], cursor: usize, rows: usize, width: usize) -> Vec<String> {
    lines(episodes, cursor, rows, width)
        .into_iter()
        .map(|line| line.text().to_owned())
        .collect()
}

/// Row one: `▸  1 e8f1a2 agent  +412 −90  risk 88 migration+ci`.
fn head(out: &mut Vec<StyledLine>, episode: &Episode, cursor: bool, width: usize) {
    let rank = episode
        .risk()
        .as_ref()
        .and_then(|risk| risk.rank)
        .map_or_else(|| "·".to_owned(), |rank| rank.to_string());
    let mut line = StyledLine::new();
    line.push(
        if cursor { Face::Emphasis } else { Face::Muted },
        &format!("{}{:>3} ", if cursor { "▸" } else { " " }, rank),
    );
    line.push(Face::Muted, &episode.short_id());
    line.push(Face::Text, &format!(" {}", episode.author.badge()));
    line.push(
        Face::Text,
        &format!(" +{} −{}", compact(episode.n_add), compact(episode.n_del)),
    );
    match episode.risk() {
        Some(risk) => {
            line.push(Face::Number, &format!(" risk {}", risk.score));
            let badge = risk.badge();
            if !badge.is_empty() {
                line.push(Face::Marker, &format!(" {badge}"));
            }
        }
        None => line.push(Face::Warning, " risk ?"),
    }
    out.push(fit(line, width));
}

/// Row two: the biggest contributing reason, indented under the row it explains.
fn reason(out: &mut Vec<StyledLine>, episode: &Episode, width: usize) {
    let Some(risk) = episode.risk() else {
        out.push(fit(
            StyledLine::with_face(
                "     unranked — reco-c05 ranks the spine, e4.4 ranks findings",
                Face::Muted,
            ),
            width,
        ));
        return;
    };
    let words = risk.reasons().first().cloned().unwrap_or_else(|| {
        if risk.vector.is_empty() {
            "no component contributed".to_owned()
        } else {
            risk.vector.clone()
        }
    });
    let mut line = StyledLine::new();
    line.push(Face::Muted, "     · ");
    line.push(Face::Text, &words);
    out.push(fit(line, width));
}

/// The strip the worklist pane carries: the ranking named, what it cannot see, and what it is not.
#[must_use]
pub fn strip(held: usize, ranked: usize, model: &str, blind_spots: usize) -> String {
    if ranked == 0 {
        return format!(
            "{held} episodes, none ranked — reco-c05 ranks the spine, e4.4 ranks findings"
        );
    }
    format!(
        "order: {model}, riskiest first, over {ranked} ranked of {held} held — it cannot see \
         {blind_spots} things (read those by hand) — triage is e4.5's"
    )
}

/// How many of the given rows carry a ranking.
#[must_use]
pub fn ranked_count(rows: &[&Episode]) -> usize {
    rows.iter()
        .filter(|episode| episode.risk().is_some())
        .count()
}

/// The model name to print, taken from the rows rather than from a constant the shell could be wrong about.
#[must_use]
pub fn model_name(rows: &[&Episode]) -> String {
    rows.iter()
        .filter_map(|episode| episode.risk())
        .find(|risk| !risk.model.is_empty())
        .map_or_else(|| "unranked".to_owned(), |risk| risk.model.clone())
}

/// The toast `w` prints: the score, the arithmetic and the rank, in one line.
#[must_use]
pub fn explain_words(episode: Option<&Episode>, now: Timestamp) -> String {
    let Some(episode) = episode else {
        return "no episode in scope — `j`/`k` choose one, then `w`".to_owned();
    };
    match episode.risk() {
        Some(risk) if !risk.vector.is_empty() => {
            format!(
                "{} — {} · {}",
                episode.short_id(),
                risk.vector,
                risk.place()
            )
        }
        Some(risk) => format!(
            "{} — risk {} · {}",
            episode.short_id(),
            risk.score,
            risk.place()
        ),
        None => format!(
            "{} ({}) is unranked — reco-c05 ranks the spine, e4.4 ranks findings",
            episode.short_id(),
            relative(now, episode.created)
        ),
    }
}

/// Clip a row to the pane rather than let it break the frame's border, and say so with `…`.
fn fit(line: StyledLine, width: usize) -> StyledLine {
    let text = line.text();
    if display_width(text) <= width {
        return line;
    }
    // Rebuild in the same faces is more work than the pane needs: the clipped row is the same
    // sentence, and the faces of what got clipped do not change what is left readable.
    let kept: String = text.chars().take(width.saturating_sub(1)).collect();
    StyledLine::with_face(format!("{}…", kept.trim_end()), Face::Text)
}
