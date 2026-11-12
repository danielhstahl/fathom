//! `fathom_tui::timeline::card` — the details pane: what the selected episode is, and what the rail
//! does not know about it.
//!
//! The card belongs to `e2.10`, and the *delta* belongs to `e2.11`: structure, dependency, shape and
//! finding rows are theirs to render, and until they land this pane prints the fields the rail is
//! actually cited on (ADR-006 §5.2's `rail`/`card` `reads` columns) plus one block saying which
//! ticket owns the rest and what to run instead. A preview that invented a `structure` line from the
//! file count would be the exact lie ADR-001 §7.10 is written against.
//!
//! Width matters here: the card is the pane the layout sells first, and at its documented minimum
//! (26 columns) a long path list has to wrap rather than clip a fact away.
//!
//! The rows are spent in the order [`ADR-006 §5.1.1`](https://github.com/)
//! fixes — header, five bands, provenance — and that order is the whole of this module's layout
//! policy. See [`detailed`] for what each band prints and who renders its strings.

use fathom_analyze::insight::{self, BriefRow, EpisodeDelta, Figure, SectionId, brief};
use fathom_core::time::Timestamp;

use crate::style::{Face, StyledLine};

use super::days;
use super::episode::Episode;
use super::render::compact;

/// What the card pane is drawing, beyond the episode itself (`e9.3`).
///
/// Two things, because the card is a document now and one of its bands opens: how deep into the
/// document the reader has walked, and whether band 4 is showing its full rows. Both live on the
/// shell rather than on the pane — the scroll offset is `App::scroll_of` like every other secondary
/// pane's, and the expansion is an answer about one episode — and the pane is handed both so it can
/// draw the same card the keystrokes say it is drawing.
#[derive(Debug, Clone, Copy, Default)]
pub struct Window {
    /// The pane's scroll offset, in rows: the first row of the card to draw.
    pub scroll: usize,
    /// Band 4 open: every reason line, then the whole ceiling, in place (`w`, `risk.explain`).
    pub expanded: bool,
}

impl Window {
    /// The card as the shell draws it with no keystroke spent on it: top of the document, band 4 brief.
    pub const BRIEF: Self = Self {
        scroll: 0,
        expanded: false,
    };
}

/// The card for the selected episode, in full: every row the pane has been asked to lay out, with no
/// window cut into it.
///
/// `width` is the pane's; `rows` is the number the shell has for it, which the row budget
/// ([ADR-006 §5.1.1 rule 4](https://github.com/)) spends — it is what decides whether figures wrap or
/// each take exactly one row. It is *not* a cap on the result: the card is a document a reader moves
/// inside (`e9.3`), so the rows past the pane's depth are returned rather than dropped, and
/// [`lines`] cuts the window and counts what is below it.
///
/// [`lines`]: crate::timeline::card::lines
#[must_use]
pub fn document(
    episode: Option<&Episode>,
    now: Timestamp,
    origin: &str,
    width: usize,
    rows: usize,
    window: Window,
) -> Vec<StyledLine> {
    match episode {
        None => stated(width),
        Some(episode) => detailed(episode, now, origin, width, rows, window),
    }
}

/// The card for the selected episode, cut to the pane: exactly `rows` lines, padded, starting at
/// `window.scroll`.
///
/// The overflow note is what the `Body` pane's `trim_rows` rule requires and what `e9.2`'s budget
/// promised: the last thing on screen counts the rows below it and names the motion that walks to
/// them. Before `e9.3` it could only name `<C-w>o`, because the pane owned no scroll offset and a
/// scroll a pane cannot show is a key that lies; now it names `j`, and the rows are really there.
#[must_use]
pub fn lines(
    episode: Option<&Episode>,
    now: Timestamp,
    origin: &str,
    width: usize,
    rows: usize,
    window: Window,
) -> Vec<StyledLine> {
    let card = document(episode, now, origin, width, rows, window);
    let from = window.scroll.min(card.len());
    let mut body: Vec<StyledLine> = card[from..].iter().take(rows).cloned().collect();
    // The note costs one of the pane's rows, so it replaces the last visible row rather than pushing
    // one off the bottom of the frame.
    if from + rows < card.len() && rows > 0 {
        let hidden = card.len() - from - rows + 1;
        body.truncate(rows.saturating_sub(1));
        // The note is itself one of the pane's rows, and at the ladder floor the card is twelve columns
        // wide: a sentence about what fell off the bottom, clipped to `… 21 more r…`, is the silent cut
        // wearing the honesty badge. So it has a short form, and both of its facts survive either way —
        // how many, and the key that walks them (`reco-5js3.3`).
        let many = super::rail::plural(hidden, "row");
        let long = format!("… {hidden} more {many} — `j` walks them, `G` walks to the end");
        let note = if crate::text::width(&long) <= width {
            long
        } else {
            let short = format!("… {hidden} more {many} · j · G");
            if crate::text::width(&short) <= width {
                short
            } else {
                format!("… +{hidden} j·G")
            }
        };
        body.push(StyledLine::with_face(note, Face::Faint));
    }
    while body.len() < rows {
        body.push(StyledLine::plain(""));
    }
    body.truncate(rows);
    body.into_iter().map(|line| line.padded(width)).collect()
}

/// The "nothing is selected" card: a reason, and the key that changes it.
///
/// Its third line used to name the tickets that owned the card's delta rows — `e2.10` and `e2.11`, both
/// of which have since shipped, which made the line a false statement about this build. An empty state may
/// name an owner only when the owner is still missing; what the reviewer needs instead is the guarantee
/// that made the pane worth filling (`e5.9`, ADR-006 §0.1).
fn stated(_width: usize) -> Vec<StyledLine> {
    vec![
        StyledLine::with_face("no episode selected", Face::Emphasis),
        StyledLine::with_face(
            "`j`/`k` walk the rail, `<Enter>` opens the card here",
            Face::Muted,
        ),
        StyledLine::with_face(
            "no row under the cursor means no figures, not none available",
            Face::Muted,
        ),
    ]
}

// ---------------------------------------------------------------------------------------------
// The row budget (ADR-006 §5.1.1 rule 4)
// ---------------------------------------------------------------------------------------------

/// The card's label column, in cells, when no label on the card needs more.
const LABEL_CELLS: usize = 9;

/// The fewest cells a value gets whatever the label costs. A value with no room wraps; it is never
/// squeezed out of existence.
const VALUE_FLOOR: usize = 8;

/// The fewest cells the head of a one-row value keeps before its tail starts eating it.
const HEAD_FLOOR: usize = 6;

/// The label the complexity figure is drawn under, and the name of the figure it is drawn from
/// (`e8.5`, `reco-l9bz.5` puts the figure in the document; the row below draws it).
///
/// The word is ADR-007's — §10.4 binds `e8.6` to one word for the rail column, the card row and the
/// diff-tree sort, and it is `complexity` — and the *slot* is this band's: complexity is a figure of the
/// `what` section (ADR-007 §5.3), so its row belongs beside the churn it qualifies, after the noise row
/// and before `where`. Reserving it here, in code and in [the card's own
/// document](../../../../docs/concepts/summary-card.md), is what keeps `e8.6` and `e9.2` from each
/// inventing a home for the same row and one of them losing. The figure is in the document now; where the
/// read made no roll-up there is nothing to draw and nothing is invented in its place: an absent figure is
/// an absent row, never a `0`.
const COMPLEXITY_ROW: &str = "complexity";
const COMPLEXITY_FIGURE: &str = "complexity";

/// One card row, before it is laid out.
enum Row {
    /// A line of its own: the identity, the facts row, a warning that says the card cannot be trusted.
    Line(StyledLine),
    /// One item of a list the row above it announced: `− text`, wrapped under its own bullet.
    ///
    /// Used by the ranking band's blind spots, which are seven separate figures with no label of
    /// their own — a label per row would read as seven figures of one kind the card invented, and the
    /// list is the model's, counted by the `blind` row that sits beside it.
    Item { text: String, face: Face },
    /// A line of the model's prose, placed under the row it belongs to and indented to the value column,
    /// so a sentence that happens to open with a word like `read` never reads as a label of its own
    /// (`reco-5js3.3`, ADR-006 §5.1.1 rule 2).
    Note { text: String, face: Face },
    /// `label  value`, wrapped under the label in **both** passes: the figure's sentence is the answer,
    /// and a cut-off half of it is not an answer. The ranking vector lives here (`reco-5js3.3`).
    Wrapped {
        label: String,
        value: String,
        face: Face,
    },
    /// `label  value`, wrapped under the label when it will not fit.
    Field {
        label: String,
        value: String,
        face: Face,
    },
    /// `label  head · tail` on **exactly one** row, for a value whose tail is the evidence: the tail
    /// survives and the head is cut with a visible `…` to pay for it.
    OneLine {
        label: String,
        head: String,
        tail: Vec<String>,
        face: Face,
    },
}

/// The card for one episode: the header, the five bands, then the provenance the bands did not need.
///
/// The order is the row budget, and it is the whole of the pane's layout policy:
///
/// 1. **the header** — the identity, then the facts row with the review state on it, then one row
///    for every warning that says the row cannot be trusted at all (ADR-003 §6.3's `gone —`, a failed
///    `integrity` self-check);
/// 2. **the code delta** (`what`): the churn, the noise/reformat fold on its own row, and the complexity
///    row [`COMPLEXITY_ROW`] — a slot reserved here before the figure existed, now drawn from the figure
///    `e8.5` (`reco-l9bz.5`) writes;
/// 3. **where it landed** (`where`);
/// 4. **what it touches and could break** — `dependencies`, `surface`, `tests`, `blast`, in the
///    document's order;
/// 5. **why it ranks here** — the vector with its place, the surfaces it moved, the counted ceiling;
/// 6. **one line of intent**, in its three states;
/// 7. **the provenance** — `members`, `boundary`, `session`, then the stamp naming the contract and the
///    index these rows came from.
///
/// Bands come before provenance because a row about the change is what the card is for: a pane that spent
/// its eighteen rows on `intent`, `boundary` and `members` and then told the reviewer to run `fathom analyze`
/// for the code delta had the order backwards (**reco-5js3.2**). Warnings are never demoted; provenance
/// always is.
fn detailed(
    episode: &Episode,
    now: Timestamp,
    origin: &str,
    width: usize,
    rows: usize,
    window: Window,
) -> Vec<StyledLine> {
    let sections: Option<Vec<BriefRow>> = episode.delta.as_ref().map(brief);
    let mut card: Vec<Row> = Vec::new();
    push_header(&mut card, episode, now);
    push_code_delta(&mut card, episode, sections.as_deref());
    push_landing(&mut card, episode, sections.as_deref());
    push_reach(&mut card, episode, sections.as_deref());
    push_ranking(&mut card, episode, sections.as_deref(), window.expanded);
    push_intent(&mut card, episode);
    push_provenance(&mut card, episode, origin);
    layout(card, width, rows)
}

/// Lay the rows out: one label column for the whole card, values wrapped under it — until the card does
/// not fit, and then one row per figure.
///
/// The second pass is the row budget doing its job — `budget` is the rows the frame handed the pane
/// (`App::pane_body_rows`, which is `frame_plan`'s own count: eighteen of them at 80×24), never a number
/// this pane invents. A figure that wraps over four rows is a figure that took three rows from another
/// band, and the bands are the questions: a pane that spent its rows on one long directory rollup had
/// printed one question four times and dropped four others. So when the
/// card will not fit, every figure gets exactly one row, cut with a visible `…`, and the counted remainder
/// the document appended (`· +3 more`) is the part that is kept — the row says how much it did not show.
/// `… N more rows` still counts what fell off the end.
fn layout(rows: Vec<Row>, width: usize, budget: usize) -> Vec<StyledLine> {
    let field = LABEL_CELLS;
    // The overflow note is one of the rows the shell gave the pane, so the card has one fewer than
    // `budget` as soon as it needs one.
    let wrapped = render(&rows, width, field, false);
    if budget == 0 || wrapped.len() <= budget.saturating_sub(1) {
        return wrapped;
    }
    render(&rows, width, field, true)
}

/// One pass of the layout: wrapped, or one row per figure.
fn render(rows: &[Row], width: usize, field: usize, compact: bool) -> Vec<StyledLine> {
    let mut out: Vec<StyledLine> = Vec::new();
    for row in rows {
        match row {
            Row::Line(line) => out.push(line.clone()),
            Row::Note { text, face } => {
                // Indented to the value column and wrapped, in both passes: the note is the model's
                // sentence about the row above it, and shrinking it to a fragment would be the pane
                // re-wording the model.
                push_note(&mut out, width, text, *face);
            }
            Row::Item { text, face } => {
                // A list item keeps its words whatever pass the card is laid out in. The compact pass
                // exists so a *glance* figure takes one row instead of four; the expanded ranking band
                // is the one place the card is not glancing, and a blind spot clipped to `− whether a
                // migrat…` is the honest half of the ranking dropped by arithmetic (`e9.3`).
                push_item(&mut out, width, text, *face);
            }
            Row::Wrapped { label, value, face } => {
                push_field(&mut out, width, field, label, value, *face);
            }
            Row::Field { label, value, face } => {
                if compact {
                    let (head, remainder) = split_remainder(value);
                    push_one_line(&mut out, width, field, label, head, &remainder, *face);
                } else {
                    push_field(&mut out, width, field, label, value, *face);
                }
            }
            Row::OneLine {
                label,
                head,
                tail,
                face,
            } => push_one_line(&mut out, width, field, label, head, tail, *face),
        }
    }
    out
}

/// Split a value's counted remainder (`… · +3 more`) from its head.
///
/// The remainder is what the document appended, not what this pane appended, so cutting on it reads the
/// model's own marker rather than inventing one: the head keeps its own words, the tail keeps the count.
fn split_remainder(value: &str) -> (&str, Vec<String>) {
    if let Some((head, tail)) = value.rsplit_once(" · +")
        && tail.ends_with("more")
    {
        return (head, vec![format!("+{tail}")]);
    }
    (value, Vec::new())
}

/// The header: what this episode is, how big it is, when it happened, and every warning that outranks
/// the analysis.
fn push_header(rows: &mut Vec<Row>, episode: &Episode, now: Timestamp) {
    rows.push(Row::Line(StyledLine::with_face(
        format!("{} — {}", episode.short_id(), episode.summary),
        Face::Emphasis,
    )));
    // The review state moved onto the facts row (**reco-5js3.2**): it is one more fact about the row the
    // reviewer is looking at, and the row it used to own also carried `UTC`, which is not a fact about
    // anything. The order is by what a 35-column pane cannot afford to lose — how many commits, how
    // long ago, which review state (the glyph stays because `NO_COLOR` is a real terminal, `e5.6`).
    //
    // The attribution used to close this row as `agent 0.94`, and `e10.4` struck it (`reco-ydek.4`,
    // ADR-006 §5.1.2 §6): the rung's verdict is not printed at a human, with its confidence beside it
    // or without one — a reader handed the conclusion cannot weigh it. What did not change is the
    // datum: `author_kind`, its confidence and its evidence are still written, still correctable, and
    // still shipped as an `AttributionRef` in `fathom analyze --json` for a consumer that can use a
    // confidence.
    rows.push(Row::Line(StyledLine::with_face(
        format!(
            "{} commit{} · {} · {} {}",
            thousands(u64::try_from(episode.n_commits()).unwrap_or(u64::MAX)),
            if episode.n_commits() == 1 { "" } else { "s" },
            days::relative_ago(now, episode.created),
            episode.state.glyph(),
            episode.state
        ),
        Face::Muted,
    )));
    if !episode.live {
        rows.push(Row::Line(StyledLine::with_face(
            "gone — no current commit is in this episode any more (ADR-003 §6.3)",
            Face::Warning,
        )));
    }
    // A document that failed its own self-check is not a card: say so above the rows, where it is read
    // before any of them, rather than in the footer nobody scrolls to (**reco-5js3.2**).
    if let Some(delta) = episode.delta.as_ref()
        && !delta.integrity.verified
    {
        rows.push(Row::Line(StyledLine::with_face(
            "self-check FAILED — the rows below came from a document that did not verify itself",
            Face::Warning,
        )));
    }
}

/// The spine's own size line, from the columns the rail is cited on.
fn spine_size(episode: &Episode) -> String {
    format!(
        "+{} −{} · {} file{}{}",
        compact(episode.n_add),
        compact(episode.n_del),
        thousands(episode.n_files),
        if episode.n_files == 1 { "" } else { "s" },
        if episode.n_binary == 0 {
            String::new()
        } else {
            format!(" · {} binary", thousands(episode.n_binary))
        }
    )
}

/// Does this document actually describe a change? A document built from an identity and a score — what
/// `EpisodeDelta::sparse` is for — carries zeroes for totals, and a zero the read never measured is the
/// one figure the card may not print (ADR-001 §7.7).
fn measured(delta: Option<&EpisodeDelta>) -> bool {
    delta.is_some_and(|delta| {
        !delta.files.is_empty() || delta.totals.n_files > 0 || delta.totals.churn != 0
    })
}

/// Band 1 — what changed, and how much of it is real.
///
/// One row from the document (its `what` row: churn first, then the counted remainder), the noise fold on
/// a row of its own, and the reserved complexity row. With no document in hand the pane prints the spine
/// columns it is cited on, which is the only number this pane formats itself.
fn push_code_delta(rows: &mut Vec<Row>, episode: &Episode, sections: Option<&[BriefRow]>) {
    match brief_row(sections, SectionId::What) {
        Some(row) if measured(episode.delta.as_ref()) => rows.push(Row::Field {
            label: row.label(),
            value: row.value.clone(),
            face: Face::Text,
        }),
        // No document, or a document with no change in it: the spine's columns, which is what the rail is
        // cited on and what the worklist row already prints — the same bytes from the same columns.
        _ => rows.push(Row::Field {
            label: "size".to_owned(),
            value: spine_size(episode),
            face: Face::Text,
        }),
    }
    push_noise(rows, episode.delta.as_ref());
    push_complexity(rows, episode.delta.as_ref());
}

/// The noise/reformat fold, on a row of its own.
///
/// It used to be one figure inside the `what` row's `+N more`, which is another way of not printing it:
/// "5 of 9 files were discounted" is the difference between a 412-line change and a 412-line change that
/// is mostly a lockfile, and a reviewer cannot weigh a figure that is folded into a counter.
fn push_noise(rows: &mut Vec<Row>, delta: Option<&EpisodeDelta>) {
    let Some(delta) = delta.filter(|delta| measured(Some(delta))) else {
        return;
    };
    if let Some(figure) = figure(delta, SectionId::What, "noise folded") {
        rows.push(Row::Field {
            label: "noise".to_owned(),
            value: figure_value(figure),
            face: Face::Text,
        });
    } else if !delta.files.is_empty() {
        // Zero is a measurement: the fold ran over these files and discounted none of them.
        rows.push(Row::Field {
            label: "noise".to_owned(),
            value: format!(
                "none of {} file(s) discounted — nothing matched the noise rule ({})",
                thousands(u64::try_from(delta.totals.files_read.max(0)).unwrap_or(u64::MAX)),
                fathom_analyze::insight::Totals::DISCOUNT_RULE
            ),
            face: Face::Text,
        });
    }
}

/// The complexity row: drawn from the document's `what`-section figure — the one `e8.5`
/// (`reco-l9bz.5`) writes, from `fathom-store`'s roll-up — and from nothing else.
///
/// ADR-007 §5.3 names the three things this row carries and the order it carries them in: **delta**
/// first, **absolute at head** second, **coverage** third, and the unknowns with their reason codes.
/// The figure's `value` is the delta (and, for a floor, the floor's own sentence); the other two come
/// out of the same figure's `raw` members, which `EpisodeDelta::verify()` already proves agree with
/// the block beside them — so the row is one figure read three ways, not three sources that can drift
/// (ADR-001 §7.10).
///
/// Coverage is on the card *always*, including at 100 %: §5.4's rule is that a reader has to be able to
/// see the reliability of a number where the number is explained, and `5 of 5 files measured` is a
/// different claim from a silent one — it is the difference between "everything was counted" and "we
/// did not say". A floor states its own coverage inside the figure's value, which is why the clause
/// below is only added when there was nothing to warn about.
fn push_complexity(rows: &mut Vec<Row>, delta: Option<&EpisodeDelta>) {
    let Some(figure) = delta.and_then(|delta| figure(delta, SectionId::What, COMPLEXITY_FIGURE))
    else {
        return;
    };
    let (value, face) = if figure.known {
        // The card's own thousands grouping, on the store's i64s: a negative figure is not a thing this
        // column can hold (ADR-007 §5.1 sums `V(G)`s, each at least 1), so the clamp is a width guard
        // and not a claim about sign.
        let count = |value: i64| thousands(u64::try_from(value).unwrap_or(u64::MAX));
        let mut value = figure.value.clone();
        if let Some(absolute) = figure.raw.get("absolute") {
            value.push_str(&format!(" · {} at head", count(*absolute)));
        }
        let measured = figure.raw.get("coverage_measured").copied().unwrap_or(0);
        let touched = figure.raw.get("coverage_total").copied().unwrap_or(0);
        let unmeasured = figure.raw.get("unmeasurable").copied().unwrap_or(0);
        // A floor already carries its own denominator: `complexity_headline` is the one string every
        // surface prints for a measured figure, and §5.3 makes it spell "2 of 5 measured · 3 unmeasured:
        // …" beside the tilde. Adding a second denominator would be the same claim twice, in two widths,
        // from one figure (ADR-001 §7.10). So what is added here is the coverage of a count that needed
        // no warning — and even that is worth the cells.
        if touched > 0 && unmeasured == 0 {
            value.push_str(&format!(
                " · {} of {} file{} measured",
                count(measured),
                count(touched),
                if touched == 1 { "" } else { "s" }
            ));
        }
        (value, Face::Text)
    } else {
        // `?` and its reason arrive together, always. ADR-006 §10.3: a shrug is not a remedy, and a
        // blank is not an unknown — the reason sentence names the codes, the dominant one in words, and
        // the command that would fill the column.
        (
            format!(
                "? — {}",
                figure.reason.as_deref().unwrap_or("nothing stated why")
            ),
            Face::Warning,
        )
    };
    rows.push(Row::Field {
        label: COMPLEXITY_ROW.to_owned(),
        value,
        face,
    });
}

/// Band 2 — where it landed.
fn push_landing(rows: &mut Vec<Row>, episode: &Episode, sections: Option<&[BriefRow]>) {
    if let Some(row) =
        brief_row(sections, SectionId::Where).filter(|_| measured(episode.delta.as_ref()))
    {
        rows.push(Row::Field {
            label: row.label(),
            value: row.value.clone(),
            face: Face::Text,
        });
        return;
    }
    if episode.paths.is_empty() {
        rows.push(Row::Field {
            label: "where".to_owned(),
            value: "paths unknown — the index recorded none for this episode".to_owned(),
            face: Face::Warning,
        });
    } else {
        // No document, and this row has no spine column to fall back on the way band 1 has: counting the
        // index's path list into two-level prefixes was `card::top_paths`, a second answer to the
        // document's question, computed in the pane and free to disagree with it (`e9.1`'s worst of four).
        // It goes, and the gap it leaves is drawn as a gap: the count the index really holds, and the verb
        // that writes the row this one reads.
        rows.push(Row::Field {
            label: "where".to_owned(),
            value: format!(
                "no rollup for the {} path{} on this row — {} writes it",
                thousands(u64::try_from(episode.paths.len()).unwrap_or(u64::MAX)),
                if episode.paths.len() == 1 { "" } else { "s" },
                insight::UNDELIVERED_VERB
            ),
            face: Face::Warning,
        });
    }
}

/// Band 3 — what it touches and could break: the four rows, in the document's order, and never a
/// re-wording of any of them.
fn push_reach(rows: &mut Vec<Row>, episode: &Episode, sections: Option<&[BriefRow]>) {
    let Some(sections) = sections else {
        // No document answers any of the four, and saying so once, with the verb that fills them, is the
        // honest row. Four rows of "not supplied" would be four rows that say one thing.
        rows.push(Row::Field {
            label: "delta".to_owned(),
            value: format!(
                "no document was handed to this card — {} is the one source",
                insight::UNDELIVERED_VERB
            ),
            face: Face::Warning,
        });
        return;
    };
    for id in [
        SectionId::Dependencies,
        SectionId::Surface,
        SectionId::Tests,
        SectionId::Blast,
    ] {
        if let Some(row) = brief_row(Some(sections), id) {
            rows.push(Row::Field {
                label: row.label(),
                value: row.value.clone(),
                face: Face::Text,
            });
        }
    }
    let _ = episode;
}

/// Band 4 — why this one is in front of the reviewer: the vector, what it is made of, and the ceiling.
///
/// Two depths, one order, and one spelling each. Brief, the band is three rows: the document's own
/// `attention` row (which *is* `RiskView::headline()` with the place appended, so the arithmetic is not
/// printed a second time as a `risk`/`vector` pair), the surfaces badge, and the counted ceiling.
/// Expanded — what `w` does now that the pane it used to swap for is gone (ADR-006 §5.1.1) — the band
/// prints the model's whole argument in place, with every other band still on the screen: every line of
/// [`RiskView::reasons()`], then every line of `BLIND_SPOTS`, then the counted ceiling, then the model's
/// own line about what a ceiling is for. No second narrative: every row's bytes come out of a `RiskView`
/// accessor, and the pane only places them and wraps them.
fn push_ranking(
    rows: &mut Vec<Row>,
    episode: &Episode,
    sections: Option<&[BriefRow]>,
    expanded: bool,
) {
    match brief_row(sections, SectionId::Attention) {
        Some(row) => {
            let value = row.value.trim();
            // A document can carry a ranking with no arithmetic in it — the bare `RiskView` a fixture or an
            // older index hands over — and then its own first figure is the empty string, which would
            // print as a row beginning with a separator. Ask the model for its headline instead; it is the
            // same accessor, not a second opinion.
            let value = if value.is_empty() || value.starts_with('·') {
                headline(episode.risk())
            } else {
                value.to_owned()
            };
            rows.push(Row::Wrapped {
                label: row.label(),
                value,
                face: Face::Text,
            });
        }
        // No document: the row says what is missing and who owns it, which is the card's own gap line.
        None => rows.push(Row::Field {
            label: "risk".to_owned(),
            value: match episode.risk() {
                Some(risk) => headline(Some(risk)),
                None => UNRANKED.to_owned(),
            },
            face: if episode.risk().is_some() {
                Face::Text
            } else {
                Face::Warning
            },
        }),
    }
    let Some(risk) = episode.risk() else {
        // An unranked episode asked "why is this one here" of, and the answer is a fact about the build,
        // not an empty section: say it as one, and say it whether or not `w` is holding the band open.
        if expanded {
            rows.push(Row::Line(StyledLine::with_face(
                UNRANKED_EXPANDED,
                Face::Warning,
            )));
        }
        return;
    };
    let badge = risk.badge();
    if !badge.is_empty() {
        rows.push(Row::Field {
            label: "surfaces".to_owned(),
            value: badge,
            face: Face::Muted,
        });
    }
    if expanded {
        // Every reason the model gave, in its own words, largest component first.
        for reason in risk.reasons() {
            rows.push(Row::Field {
                label: "reason".to_owned(),
                value: reason,
                face: Face::Text,
            });
        }
        // Then the whole ceiling: `blind_spot_note()` counts it for the brief band, and this is the
        // list it counts — which is the promise the model ships in that string ("`w` lists them all").
        for spot in &risk.blind_spots {
            rows.push(Row::Item {
                text: spot.clone(),
                face: Face::Warning,
            });
        }
    }
    // The ceiling on its own row, in the model's words (`RiskView::blind_spot_note`), and counted: one
    // ceiling row per card, brief or expanded — two rows both labelled `blind` is the defect `e9.1` named.
    if let Some(note) = risk.blind_spot_note() {
        rows.push(Row::Field {
            label: "blind".to_owned(),
            value: note,
            face: Face::Warning,
        });
    }
    if expanded {
        // What to do with a ceiling, in the voice of the thing that has it (moved into `fathom-analyze`
        // with this row, so no pane gets to word it).
        if let Some(line) = risk.ceiling_note() {
            rows.push(Row::Note {
                text: line,
                face: Face::Faint,
            });
        }
    }
}

/// What an unranked episode's ranking band says, and what it says once `w` asks it for the argument.
const UNRANKED: &str = "unknown — nothing has ranked this episode (`reco-c05` ranks the spine; e4.4 \
    ranks findings)";
const UNRANKED_EXPANDED: &str = "nothing to expand — no ranking, so no reasons and no ceiling to list \
    (`reco-c05` ranks the spine; e4.4 ranks findings)";

/// The ranking line, from [`RiskView`]'s own accessors: the arithmetic when there is any, the score when
/// there is not, and the place in the ranked set beside it.
fn headline(risk: Option<&fathom_analyze::insight::RiskView>) -> String {
    let Some(risk) = risk else {
        return "unknown — nothing has ranked this episode".to_owned();
    };
    let mut value = risk.headline();
    let place = risk.place();
    if !place.is_empty() {
        value.push_str(&format!(" · {place}"));
    }
    value
}

/// Band 5 — what the agent said it was doing, on one row.
///
/// Four states, four different openings, and one row each: *recorded* (the text, with the source and the
/// confidence that made it worth quoting), *digest-only* (the harness took a digest of the prompt and not
/// its text, which is what this project's own streams carry), *none* (every source was read and said
/// nothing, with the reason), and *not on this row* (this build wrote no intent for the episode, or handed
/// the card no document at all). Every one of them is drawn, because a row that prints nothing when
/// nobody recorded anything is indistinguishable from a card that never looked — and those two are the
/// difference between an agent that worked unbriefed and a build that predates capture (`e4.2`, `e9.1`).
///
/// The state always leads, and only the recorded state has a tail. A quoted sentence is long enough that
/// it cannot keep its words and its source in one row, so the evidence — who said it, the confidence, the
/// verb that prints the plan and scope — is ordered ahead of the cut and the head loses its last words
/// instead. An absence is a short phrase, so an absence is never what gets cut.
fn push_intent(rows: &mut Vec<Row>, episode: &Episode) {
    let Some(delta) = episode.delta.as_ref() else {
        rows.push(Row::OneLine {
            label: "intent".to_owned(),
            head: "not handed to this card — no delta document".to_owned(),
            // Not "re-index": this card was handed a row and no document at all, and re-indexing would
            // not change that. `delta` says the same thing one row above.
            tail: Vec::new(),
            face: Face::Warning,
        });
        return;
    };
    let Some(intent) = delta.episode.intent.as_ref() else {
        rows.push(Row::OneLine {
            label: "intent".to_owned(),
            head: "not on this row — re-index to capture it (`fathom index`)".to_owned(),
            tail: Vec::new(),
            face: Face::Warning,
        });
        return;
    };
    if intent.state == insight::INTENT_RECORDED {
        let first = intent
            .text
            .as_deref()
            .unwrap_or_default()
            .lines()
            .next()
            .unwrap_or_default()
            .trim();
        let head = if first.is_empty() {
            "recorded with no text".to_owned()
        } else {
            first.to_owned()
        };
        let mut tail = vec![
            format!("via {}", intent.source),
            format!("{:.2}", intent.confidence),
        ];
        // The plan and the declared scope are evidence, and evidence lives in the delta document: name
        // the verb that prints it whole rather than growing the row to hold it (ADR-006 §5.1.1).
        if !intent.plan.is_empty() || !intent.scope.is_empty() {
            tail.push(format!("`fathom show {}`", episode.short_id()));
        }
        rows.push(Row::OneLine {
            label: "intent".to_owned(),
            head,
            tail,
            face: Face::Text,
        });
        return;
    }
    if intent.state == insight::INTENT_DIGEST_ONLY {
        // One phrase, not a head and a tail: "no text captured" is the fact, and the digest beside it is
        // the detail — if the row can only carry one of them it is the fact, so the cut falls at the end.
        rows.push(Row::OneLine {
            label: "intent".to_owned(),
            head: format!(
                "no text captured · prompt {} · via {}",
                intent
                    .prompt
                    .as_ref()
                    .and_then(|prompt| prompt.sha256.as_deref())
                    .and_then(|sha| sha.get(..12))
                    .unwrap_or("—"),
                intent.source
            ),
            tail: Vec::new(),
            face: Face::Warning,
        });
        return;
    }
    rows.push(Row::OneLine {
        label: "intent".to_owned(),
        head: format!(
            "no recorded intent — {}",
            intent
                .reason
                .clone()
                .unwrap_or_else(|| "nothing stated it".to_owned())
        ),
        tail: Vec::new(),
        face: Face::Warning,
    });
}

/// The provenance: what the index said about the record, as opposed to what the change did.
///
/// `members`, `boundary`, `session`, then the stamp, in that fixed order and last, because every one of
/// them is a fact about how the tool came to know this episode rather than about the episode.
fn push_provenance(rows: &mut Vec<Row>, episode: &Episode, origin: &str) {
    rows.push(Row::Field {
        label: "members".to_owned(),
        value: if episode.members.is_empty() {
            "unknown — the index named no member commits".to_owned()
        } else {
            let shown: Vec<&str> = episode.members.iter().take(3).map(String::as_str).collect();
            let rest = episode.members.len().saturating_sub(shown.len());
            format!(
                "{}{}",
                shown
                    .iter()
                    // A SHA is one token: cutting it at seven cells produced `e8f1a2-`, which looks
                    // like a typo rather than an id. Ellipsis on the end is the honest cut.
                    .map(|sha| {
                        let chars: Vec<char> = sha.chars().collect();
                        if chars.len() <= 12 {
                            (*sha).to_owned()
                        } else {
                            format!("{}…", chars[..11].iter().collect::<String>())
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
                if rest > 0 {
                    format!(" +{rest} more")
                } else {
                    String::new()
                }
            )
        },
        face: Face::Text,
    });
    rows.push(Row::Field {
        label: "boundary".to_owned(),
        value: match &episode.boundary_rule {
            Some(rule) => format!(
                "{rule} {} — a rule, not a fact (ADR-003 §4.7)",
                episode
                    .boundary_confidence
                    .map_or_else(|| "?".to_owned(), |value| format!("{value:.2}"))
            ),
            None => "unknown — no rule was recorded".to_owned(),
        },
        face: Face::Text,
    });
    rows.push(Row::Field {
        label: "session".to_owned(),
        value: match (&episode.session, &episode.turn) {
            (Some(session), Some(turn)) => format!("session {session} · turn {turn}"),
            (Some(session), None) => format!("session {session} · turn unknown"),
            (None, _) => "no harness session on this episode".to_owned(),
        },
        face: Face::Text,
    });
    if let Some(delta) = episode.delta.as_ref() {
        // The stamp the card is answered under, so a screenshot says which contract it was drawn from.
        rows.push(Row::Field {
            label: "model".to_owned(),
            value: format!(
                "{contract} v{version} · self-check {check}",
                contract = delta.contract,
                version = delta.schema_version,
                check = if delta.integrity.verified {
                    "clean"
                } else {
                    "FAILED — see the warning above the rows"
                },
            ),
            face: Face::Muted,
        });
    }
    rows.push(Row::Field {
        label: "read".to_owned(),
        value: format!("{origin} · times UTC"),
        face: Face::Faint,
    });
}

/// The document's brief row for one section.
fn brief_row(sections: Option<&[BriefRow]>, section: SectionId) -> Option<&BriefRow> {
    sections?.iter().find(|row| row.id == section.as_str())
}

/// One named figure of one section, straight out of the document.
///
/// The pane may place a figure, wrap it and clip it; it may not word one. Every string that reaches a row
/// through here is a string the analysis wrote, with the evidence pointer still attached to it
/// (ADR-006 §5.1.1's "one figure, one string, one surface").
fn figure<'a>(delta: &'a EpisodeDelta, section: SectionId, label: &str) -> Option<&'a Figure> {
    delta
        .sections
        .iter()
        .find(|row| row.id == section.as_str())?
        .figures
        .iter()
        .find(|figure| figure.label == label)
}

/// A figure's value, verbatim.
fn figure_value(figure: &Figure) -> String {
    figure.value.clone()
}

/// `label  value`, wrapped at the pane's width with the continuation lined up under the value.
///
/// The column is nine cells because that is what a 35-column pane can pay for and still leave the value
/// two thirds of the row; a label longer than the column (`x dependencies`) starts its own value one cell
/// further out rather than running into it, so the column stays a column for the rows that fit it.
fn push_field(
    out: &mut Vec<StyledLine>,
    width: usize,
    field: usize,
    label: &str,
    value: &str,
    face: Face,
) {
    let start = field.max(crate::text::width(label) + 1);
    let head = format!("{label:<start$}");
    let indent = " ".repeat(start.min(width));
    let room = width.saturating_sub(start).max(VALUE_FLOOR);
    for (index, chunk) in wrap(value, room).into_iter().enumerate() {
        if index == 0 {
            let mut line = StyledLine::new();
            line.push(Face::Muted, &head);
            line.push(face, &chunk);
            out.push(line);
        } else {
            out.push(StyledLine::plain(format!("{indent}{chunk}")));
        }
    }
}

/// One row, whatever the value costs: `label  head · tail`.
///
/// Used only by the intent band, where the row budget is one row and the tail is the evidence. The parts
/// of the tail are taken in order for as long as a usable head still fits beside them — "usable" is half
/// the row, because the head is the answer to the band's question and the tail is its citation — and the
/// head is the thing that gets cut, with `…`, so the cut is visible rather than a sentence that simply
/// stops.
fn push_one_line(
    out: &mut Vec<StyledLine>,
    width: usize,
    field: usize,
    label: &str,
    head: &str,
    tail: &[String],
    face: Face,
) {
    let start = field.max(crate::text::width(label) + 1);
    let room = width.saturating_sub(start).max(1);
    // The head is guaranteed half the row — or its own length, when it is shorter than that — and the
    // tail takes whatever the head did not need. A one-word sentence therefore carries all of its
    // provenance, and a long paragraph is cut before its citation is.
    let head_floor = HEAD_FLOOR.min(room);
    let head_cost = crate::text::width(head).clamp(head_floor, (room / 2).max(head_floor));
    let mut kept: Vec<String> = Vec::new();
    for part in tail {
        let candidate: Vec<String> = kept
            .iter()
            .cloned()
            .chain(std::iter::once(part.clone()))
            .collect();
        if tail_cells(&candidate) + TAIL_SEPARATOR.chars().count() + head_cost > room {
            break;
        }
        kept = candidate;
    }
    // A tail part that did not fit is counted, not silently dropped: `· +2 more` says the row has
    // provenance it could not show. When not even one part of the tail fits, the marker would eat exactly
    // the text it exists to report on, so the row's own `…` is left to carry the cut (`reco-5js3.3`).
    let mut dropped = tail.len() - kept.len();
    let mut marker = String::new();
    if dropped > 0 && !kept.is_empty() {
        loop {
            let candidate = format!("{TAIL_SEPARATOR}+{dropped} more");
            let cost = tail_cells(&kept) + crate::text::width(&candidate) + head_cost;
            if cost <= room {
                marker = candidate;
                break;
            }
            if kept.len() <= 1 {
                // Paying for the marker would push off the last part of the provenance, which is the one
                // thing the row may not cut. The row's own `…` carries the cut instead.
                break;
            }
            kept.pop();
            dropped = tail.len() - kept.len();
        }
    }
    let tail_cells_used = tail_cells(&kept)
        + if kept.is_empty() && marker.is_empty() {
            0
        } else {
            TAIL_SEPARATOR.chars().count()
        };
    let head_room = room
        .saturating_sub(tail_cells_used + crate::text::width(&marker))
        .max(1);
    let mut line = StyledLine::new();
    line.push(Face::Muted, &format!("{label:<start$}"));
    let clipped = clip_head(head, head_room);
    match clipped.strip_suffix('…') {
        Some(body) => {
            line.push(face, body);
            // The cut is chrome, not content: the same face the pane clips every overflowing row with.
            line.push(Face::Muted, "…");
        }
        None => line.push(face, &clipped),
    }
    if !kept.is_empty() || !marker.is_empty() {
        let shown: Vec<&str> = kept
            .iter()
            .map(String::as_str)
            .chain(std::iter::once(marker.as_str()))
            .filter(|part| !part.is_empty())
            .collect();
        line.push(
            Face::Muted,
            &format!("{TAIL_SEPARATOR}{}", shown.join(TAIL_SEPARATOR)),
        );
    }
    out.push(line);
}

/// Clip a value to `room` cells, preferring to drop whole figures rather than half of one.
///
/// These values are often lists of figures — `src/store 9f +338 −74 · tests 1f +37 −8` — and a number cut
/// in half reads as a different number: `9f +3…` is not a count anyone can check, and it is one character
/// away from a figure that was never in the document. So when the cut lands inside a figure, the row backs
/// off to the last whole figure instead — but only when that figure still fills half the row. Truncating
/// at a figure boundary is what ADR-006 §5.1.1 sanctions ("truncate it with a counted remainder");
/// wasting two thirds of the row to do it is just a shorter row, so prose values, which have no figures
/// to protect, keep their full width and the ordinary character cut.
fn clip_head(head: &str, room: usize) -> String {
    if crate::text::width(head) <= room {
        return head.to_owned();
    }
    let mut best: Option<&str> = None;
    for (index, _) in head.match_indices(TAIL_SEPARATOR) {
        let candidate = &head[..index];
        if crate::text::width(candidate) + 1 > room {
            break;
        }
        best = Some(candidate);
    }
    match best {
        Some(prefix) if crate::text::width(prefix) + 1 >= room / 2 => format!("{prefix}…"),
        _ => crate::text::clip(head, room),
    }
}

/// The join used inside a one-row value.
const TAIL_SEPARATOR: &str = " · ";

/// How many cells a joined tail costs.
fn tail_cells<T: AsRef<str>>(parts: &[T]) -> usize {
    crate::text::width(
        &parts
            .iter()
            .map(|part| part.as_ref())
            .collect::<Vec<_>>()
            .join(TAIL_SEPARATOR),
    )
}

/// One item of a list, wrapped under its own bullet.
///
/// The bullet is two cells and the continuation lines line up under the text, which is what makes a
/// seven-item list read as a list at 26 columns instead of as a paragraph that has lost its shape.
fn push_item(out: &mut Vec<StyledLine>, width: usize, text: &str, face: Face) {
    let room = width.saturating_sub(crate::text::width(ITEM_BULLET));
    if room < PROSE_ROOM_MIN {
        // Too narrow to wrap legibly: one row per blind spot, cut where it has to be cut, with the cut
        // on the screen. The list is still there and the `blind` row above still counts it.
        let mut line = StyledLine::new();
        line.push(Face::Muted, ITEM_BULLET);
        line.push(face, &crate::text::clip(text, room.max(VALUE_FLOOR)));
        out.push(line);
        return;
    }
    for (index, chunk) in wrap(text, room).into_iter().enumerate() {
        let mut line = StyledLine::new();
        if index == 0 {
            line.push(Face::Muted, ITEM_BULLET);
        } else {
            line.push(Face::Muted, &" ".repeat(crate::text::width(ITEM_BULLET)));
        }
        line.push(face, &chunk);
        out.push(line);
    }
}

/// The bullet a list item leads with.
const ITEM_BULLET: &str = "  − ";

/// One of the model's own sentences, placed under the row it belongs to. It starts at the value column
/// rather than the label column: a note that opens with a word like `read` would otherwise read as a
/// figure the reviewer has to parse (`reco-5js3.3`, ADR-006 §5.1.1 rule 2).
fn push_note(out: &mut Vec<StyledLine>, width: usize, text: &str, face: Face) {
    push_prose(out, width, NOTE_MARK, text, face);
}

/// The mark a note leads with: the row above it is what the sentence is about, and a note that opened
/// flush-left with a word like `read` would read as a figure of its own.
const NOTE_MARK: &str = "↳ ";

/// Fewest cells a sentence of the model's gets before the card stops wrapping it. Below this the wrap
/// turns prose into a narrow column of fragments — `− whether` / `a` / `migratio` / `n can be` — which
/// is neither the figure nor a visible cut, so the row is clipped with `…` instead, the same way every
/// other over-long value is (`e9.3`).
const PROSE_ROOM_MIN: usize = 20;

/// One of the model's sentences in the value column: wrapped while it is readable, cut with a visible
/// `…` when the pane is too narrow to wrap it legibly. Never dropped, and never a silent truncation.
fn push_prose(out: &mut Vec<StyledLine>, width: usize, mark: &str, text: &str, face: Face) {
    let indent = " ".repeat(LABEL_CELLS);
    let room = width.saturating_sub(LABEL_CELLS + crate::text::width(mark));
    if room < PROSE_ROOM_MIN {
        let mut line = StyledLine::new();
        line.push(Face::Muted, &indent);
        line.push(Face::Muted, mark);
        line.push(
            face,
            &crate::text::clip(
                text,
                width.saturating_sub(LABEL_CELLS + crate::text::width(mark)),
            ),
        );
        out.push(line);
        return;
    }
    for (index, chunk) in wrap(text, room.max(1)).into_iter().enumerate() {
        let mut line = StyledLine::new();
        line.push(Face::Muted, &indent);
        if index == 0 {
            line.push(Face::Muted, mark);
        } else {
            line.push(Face::Muted, &" ".repeat(crate::text::width(mark)));
        }
        line.push(face, &chunk);
        out.push(line);
    }
}

/// The wrap lives with the rail's renderer now (`render::wrap`): the empty rail and the card
/// fold text the same way, and two copies of a word-wrap drift.
use super::render::wrap;

/// A number with its thousands separators, for counts the card states in words.
fn thousands(value: u64) -> String {
    crate::app::format_thousands(value)
}
