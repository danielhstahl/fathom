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

/// The card for the selected episode.
///
/// `width` is the pane's, `rows` the number the shell has for it; the result is exactly `rows` lines,
/// padded, so a short card never leaves the pane's border ragged.
#[must_use]
pub fn lines(
    episode: Option<&Episode>,
    now: Timestamp,
    origin: &str,
    width: usize,
    rows: usize,
) -> Vec<StyledLine> {
    let mut body: Vec<StyledLine> = match episode {
        None => stated(width),
        Some(episode) => detailed(episode, now, origin, width, rows),
    };
    // The overflow note is what the `Body` pane's `trim_rows` rule requires: content goes from the
    // end, and the last thing on screen says that happened rather than letting a line vanish.
    if body.len() > rows && rows > 0 {
        let hidden = body.len() - rows + 1;
        body.truncate(rows.saturating_sub(1));
        body.push(StyledLine::with_face(
            format!(
                "… {hidden} more {} — `<C-w>o` gives this pane the frame",
                super::rail::plural(hidden, "row"),
            ),
            Face::Faint,
        ));
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

/// The label `e8.6`'s complexity figure is drawn under, and the name of the figure it is drawn from
/// (`e8.5` puts the figure in the document; `e8.6` draws the row).
///
/// The word is ADR-007's — §10.4 binds `e8.6` to one word for the rail column, the card row and the
/// diff-tree sort, and it is `complexity` — and the *slot* is this band's: complexity is a figure of the
/// `what` section (ADR-007 §5.3), so its row belongs beside the churn it qualifies, after the noise row
/// and before `where`. Reserving it here, in code and in [the card's own
/// document](../../../../docs/concepts/summary-card.md), is what keeps `e8.6` and `e9.2` from each
/// inventing a home for the same row and one of them losing. Until the figure is written there is
/// nothing to draw and nothing is invented in its place: an absent figure is an absent row, never a `0`.
const COMPLEXITY_ROW: &str = "complexity";
const COMPLEXITY_FIGURE: &str = "complexity";

/// One card row, before it is laid out.
enum Row {
    /// A line of its own: the identity, the attribution, a warning that says the card cannot be trusted.
    Line(StyledLine),
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
/// 1. **the header** — the identity, then the attribution line with the review state on it, then one row
///    for every warning that says the row cannot be trusted at all (ADR-003 §6.3's `gone —`, a failed
///    `integrity` self-check);
/// 2. **the code delta** (`what`): the churn, the noise/reformat fold on its own row, and the reserved
///    complexity row [`COMPLEXITY_ROW`];
/// 3. **where it landed** (`where`);
/// 4. **what it touches and could break** — `dependencies`, `surface`, `tests`, `blast`, in the
///    document's order;
/// 5. **why it ranks here** — the vector with its place, the surfaces it moved, the counted ceiling;
/// 6. **one line of intent**, in its three states;
/// 7. **the provenance** — `members`, `boundary`, `session`, then the stamp naming the contract and the
///    index these rows came from.
///
/// Bands come before provenance because a row about the change is what the card is for: a pane that spent
/// nineteen rows on `intent`, `boundary` and `members` and then told the reviewer to run `fathom analyze`
/// for the code delta had the order backwards (**reco-5js3.2**). Warnings are never demoted; provenance
/// always is.
fn detailed(
    episode: &Episode,
    now: Timestamp,
    origin: &str,
    width: usize,
    rows: usize,
) -> Vec<StyledLine> {
    let sections: Option<Vec<BriefRow>> = episode.delta.as_ref().map(brief);
    let mut card: Vec<Row> = Vec::new();
    push_header(&mut card, episode, now);
    push_code_delta(&mut card, episode, sections.as_deref());
    push_landing(&mut card, episode, sections.as_deref());
    push_reach(&mut card, episode, sections.as_deref());
    push_ranking(&mut card, episode, sections.as_deref());
    push_intent(&mut card, episode);
    push_provenance(&mut card, episode, origin);
    layout(card, width, rows)
}

/// Lay the rows out: one label column for the whole card, values wrapped under it — until the card does
/// not fit, and then one row per figure.
///
/// The second pass is the row budget doing its job. A figure that wraps over four rows is a figure that
/// took three rows from another band, and the bands are the questions: a pane that spent nineteen rows on
/// one long directory rollup had printed one question four times and dropped four others. So when the
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

/// The header: who this episode is, who did it and when, and every warning that outranks the analysis.
fn push_header(rows: &mut Vec<Row>, episode: &Episode, now: Timestamp) {
    rows.push(Row::Line(StyledLine::with_face(
        format!("{} — {}", episode.short_id(), episode.summary),
        Face::Emphasis,
    )));
    let confidence = episode
        .author_confidence
        .map_or_else(|| "?".to_owned(), |value| format!("{value:.2}"));
    // The review state moved onto the attribution line (**reco-5js3.2**): it is one more fact about the
    // row the reviewer is looking at, and the row it used to own also carried `UTC`, which is not a fact
    // about anything. The order is by what a 35-column pane cannot afford to lose — how many commits, how
    // long ago, which review state (the glyph stays because `NO_COLOR` is a real terminal, `e5.6`) — and
    // the attribution confidence, which is a fact about the *record*, goes last.
    rows.push(Row::Line(StyledLine::with_face(
        format!(
            "{} commit{} · {} · {} {} · {} {}",
            thousands(u64::try_from(episode.n_commits()).unwrap_or(u64::MAX)),
            if episode.n_commits() == 1 { "" } else { "s" },
            days::relative_ago(now, episode.created),
            episode.state.glyph(),
            episode.state,
            episode.author.badge(),
            confidence
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

/// The reserved complexity row: drawn from `e8.6`'s figure — the one `e8.5` puts in the document — and
/// from nothing else.
fn push_complexity(rows: &mut Vec<Row>, delta: Option<&EpisodeDelta>) {
    let Some(figure) = delta.and_then(|delta| figure(delta, SectionId::What, COMPLEXITY_FIGURE))
    else {
        return;
    };
    let (value, face) = if figure.known {
        (figure_value(figure), Face::Text)
    } else {
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
        // The reclaimed rows land here: the list is what answers "where", so it is allowed as many
        // prefixes as the card has room for, and it wraps under its own label rather than being cut to
        // four. `e9.3` retires this rollup in favour of the document's own, which is ranked by churn.
        let list: Vec<String> = top_paths(&episode.paths)
            .iter()
            .map(|(path, count)| format!("{path} ({count})"))
            .collect();
        rows.push(Row::Field {
            label: "where".to_owned(),
            value: list.join(" · "),
            face: Face::Text,
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
/// Three rows, and one spelling each. The vector row is the document's own `attention` row, which is
/// `RiskView::headline()` with the place in the ranking appended — so it is *not* printed again as a
/// separate `risk` and `vector` pair, which is how one frame came to carry the same string twice.
fn push_ranking(rows: &mut Vec<Row>, episode: &Episode, sections: Option<&[BriefRow]>) {
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
            rows.push(Row::Field {
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
                None => {
                    "unknown — nothing has ranked this episode (`reco-c05` ranks the spine; e4.4 \
                        ranks findings)"
                        .to_owned()
                }
            },
            face: if episode.risk().is_some() {
                Face::Text
            } else {
                Face::Warning
            },
        }),
    }
    let Some(risk) = episode.risk() else {
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
    // The ceiling on its own row, in the model's words (`RiskView::blind_spot_note`), and counted: the
    // whole list is `w`'s, and a card that printed it would be a card that had run out of room for the
    // change. One ceiling row per card — two rows both labelled `blind` is the defect `e9.1` named.
    if let Some(note) = risk.blind_spot_note() {
        rows.push(Row::Field {
            label: "blind".to_owned(),
            value: note,
            face: Face::Warning,
        });
    }
}

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

/// The `i` pane: one episode's intent, in full (`e4.2`, `reco-c54`).
///
/// The lines come from [`insight::intent_block`] — the function `fathom show` and the delta document
/// render through — because the card, the CLI and an exported document must not be three places that spell
/// an intent differently (ADR-001 §7.10). What this pane adds over the card's one-line `intent` field is
/// the *plan* and the *declared scope*: the two things a reviewer checks a change against, which do not fit
/// in a field and are useless one at a time.
#[must_use]
pub fn intent_lines(episode: Option<&Episode>, width: usize, rows: usize) -> Vec<StyledLine> {
    let mut body: Vec<StyledLine> = match episode {
        None => vec![StyledLine::with_face(
            "no episode selected — `j`/`k` choose one, then `i`",
            Face::Muted,
        )],
        Some(episode) => {
            let mut out: Vec<StyledLine> = Vec::new();
            out.push(StyledLine::with_face(
                format!("{} — {}", episode.short_id(), episode.summary),
                Face::Emphasis,
            ));
            match episode
                .delta
                .as_ref()
                .and_then(|delta| delta.episode.intent.as_ref())
            {
                None => {
                    out.push(StyledLine::with_face(
                        "this row carries no intent document — it was written before intent capture, \
                         and `fathom index --rebuild` reads the trailers and the stream again",
                        Face::Warning,
                    ));
                    out.push(StyledLine::with_face(
                        "`none recorded` and `never looked` are different facts; this is the second one",
                        Face::Faint,
                    ));
                }
                Some(intent) => {
                    out.push(StyledLine::plain(""));
                    for line in insight::intent_block(intent) {
                        let face = if line.starts_with("intent    no ") {
                            // Both absences — "no recorded intent" and "no text captured" — are warnings:
                            // each is the card saying it looked and came back empty.
                            Face::Warning
                        } else if line.starts_with("reason") || line.starts_with("scope") {
                            Face::Muted
                        } else {
                            Face::Text
                        };
                        // The label is kept, with a hanging indent on the wrap. Stripping it would save six
                        // columns and lose the one thing that tells `scope` from `reason` from `plan` once a
                        // long value has wrapped past the first row.
                        let (label, value) = match line.split_once("  ") {
                            Some((label, value)) => (label.trim_end(), value.trim()),
                            None => ("", line.trim()),
                        };
                        let indent = " ".repeat(label.len() + 1);
                        let body = width.saturating_sub(label.len() + 1).max(8);
                        for (index, chunk) in wrap(value, body).into_iter().enumerate() {
                            let mut row = StyledLine::plain("");
                            row.push(
                                Face::Muted,
                                if index == 0 || label.is_empty() {
                                    label
                                } else {
                                    indent.as_str()
                                },
                            );
                            if !label.is_empty() {
                                row.push(face, &format!(" {chunk}"));
                            } else {
                                row.push(face, &chunk);
                            }
                            out.push(row);
                        }
                    }
                    out.push(StyledLine::with_face(
                        "the scope is what was *declared*; the scope-creep finding over it is `e4.3`'s",
                        Face::Faint,
                    ));
                }
            }
            out
        }
    };
    if body.len() > rows && rows > 0 {
        let hidden = body.len() - rows + 1;
        body.truncate(rows.saturating_sub(1));
        body.push(StyledLine::with_face(
            format!("… {hidden} more rows — `i` again, or `<Esc>`, to go back"),
            Face::Faint,
        ));
    }
    while body.len() < rows {
        body.push(StyledLine::plain(""));
    }
    body.truncate(rows);
    body.into_iter().map(|line| line.padded(width)).collect()
}

/// The `w` pane: one episode's ranking, in full, with the ceiling it comes under.
///
/// This is the answer to "why that one", which ADR-006 §5.1 insists is a sentence and not a
/// recommendation ("rank 4 of 7 because risk 82 = 0.4·size + 0.3·test + 0.3·blast" is checkable;
/// "recommended for you" is not).
#[must_use]
pub fn risk_lines(episode: Option<&Episode>, width: usize, rows: usize) -> Vec<StyledLine> {
    let mut body: Vec<StyledLine> = match episode {
        None => vec![StyledLine::with_face(
            "no episode selected — `j`/`k` choose one, then `w`",
            Face::Muted,
        )],
        Some(episode) => {
            let mut out: Vec<StyledLine> = Vec::new();
            out.push(StyledLine::with_face(
                format!("{} — {}", episode.short_id(), episode.summary),
                Face::Emphasis,
            ));
            match episode.risk() {
                None => out.push(StyledLine::with_face(
                    "nothing has ranked this episode, so there is nothing to explain (reco-c05 \
                     ranks the spine; e4.4 ranks findings)",
                    Face::Warning,
                )),
                Some(risk) => {
                    out.push(StyledLine::plain(format!(
                        "{} · {}",
                        if risk.vector.is_empty() {
                            format!("risk {}", risk.score)
                        } else {
                            risk.vector.clone()
                        },
                        risk.place()
                    )));
                    let badge = risk.badge();
                    if !badge.is_empty() {
                        out.push(StyledLine::with_face(
                            format!("surfaces {badge}"),
                            Face::Muted,
                        ));
                    }
                    out.push(StyledLine::plain(""));
                    for reason in risk.reasons() {
                        for chunk in wrap(&reason, width.saturating_sub(2).max(8)) {
                            out.push(StyledLine::with_face(format!("· {chunk}"), Face::Text));
                        }
                    }
                    if !risk.blind_spots.is_empty() {
                        out.push(StyledLine::plain(""));
                        out.push(StyledLine::with_face(
                            format!(
                                "a spine cannot see {} thing{}:",
                                risk.blind_spots.len(),
                                if risk.blind_spots.len() == 1 { "" } else { "s" }
                            ),
                            Face::Warning,
                        ));
                        for spot in &risk.blind_spots {
                            for chunk in wrap(spot, width.saturating_sub(4).max(8)) {
                                out.push(StyledLine::with_face(
                                    format!("  − {chunk}"),
                                    Face::Warning,
                                ));
                            }
                        }
                        out.push(StyledLine::with_face(
                            "read those by hand: the ranking is a reading order, not an all-clear",
                            Face::Faint,
                        ));
                    }
                }
            }
            out
        }
    };
    if body.len() > rows && rows > 0 {
        let hidden = body.len() - rows + 1;
        body.truncate(rows.saturating_sub(1));
        body.push(StyledLine::with_face(
            format!("… {hidden} more rows — `<C-w>o` gives this pane the frame"),
            Face::Faint,
        ));
    }
    while body.len() < rows {
        body.push(StyledLine::plain(""));
    }
    body.truncate(rows);
    body.into_iter().map(|line| line.padded(width)).collect()
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
    // The head is guaranteed half the row — or its own length, when it is shorter than that — and the tail
    // takes whatever the head did not need. A one-word sentence therefore carries all of its provenance,
    // and a long paragraph is cut before its citation is.
    let head_floor = HEAD_FLOOR.min(room);
    let head_cost = crate::text::width(head).clamp(head_floor, (room / 2).max(head_floor));
    let mut kept: Vec<&str> = Vec::new();
    for part in tail {
        let candidate: Vec<&str> = kept
            .iter()
            .copied()
            .chain(std::iter::once(part.as_str()))
            .collect();
        if tail_cells(&candidate) + TAIL_SEPARATOR.chars().count() + head_cost > room {
            break;
        }
        kept.push(part);
    }
    let head_room = room
        .saturating_sub(
            tail_cells(&kept)
                + if kept.is_empty() {
                    0
                } else {
                    TAIL_SEPARATOR.chars().count()
                },
        )
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
    if !kept.is_empty() {
        line.push(
            Face::Muted,
            &format!("{TAIL_SEPARATOR}{}", kept.join(TAIL_SEPARATOR)),
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
fn tail_cells(parts: &[&str]) -> usize {
    crate::text::width(&parts.join(TAIL_SEPARATOR))
}

/// The wrap lives with the rail's renderer now (`render::wrap`): the empty rail and the card
/// fold text the same way, and two copies of a word-wrap drift.
use super::render::wrap;

/// The path prefixes that account for the most of the episode's files — `where`, the way the card
/// prints it when no delta document was handed over (`src/store (7) · tests (2)`).
///
/// Six, not four: `e9.2` moved the intent row from three rows to one and the reclaimed space goes to the
/// code delta, of which this row is a part. The row wraps under its own label, so the cap is what the
/// reviewer can *read* at the documented width rather than what fits on one line.
#[must_use]
pub fn top_paths(paths: &[String]) -> Vec<(String, usize)> {
    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for path in paths {
        // Two levels: `src/store/epoch.rs` counts as `src/store`, a root file counts as itself.
        let key = match path.split_once('/') {
            Some((head, rest)) => match rest.split_once('/') {
                Some((second, _)) => format!("{head}/{second}"),
                None => head.to_owned(),
            },
            None => path.clone(),
        };
        *counts.entry(key).or_insert(0) += 1;
    }
    let mut rows: Vec<(String, usize)> = counts.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    rows.truncate(6);
    rows
}

/// A number with its thousands separators, for counts the card states in words.
fn thousands(value: u64) -> String {
    crate::app::format_thousands(value)
}
