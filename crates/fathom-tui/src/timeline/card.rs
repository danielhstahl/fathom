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

use fathom_analyze::insight::{self, brief};
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
        Some(episode) => detailed(episode, now, origin, width),
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
fn stated(_width: usize) -> Vec<StyledLine> {
    vec![
        StyledLine::with_face("no episode selected", Face::Emphasis),
        StyledLine::with_face(
            "`j`/`k` walk the rail, `<Enter>` opens the card here",
            Face::Muted,
        ),
        StyledLine::with_face(
            "the card's delta rows are owned by e2.10 and e2.11",
            Face::Muted,
        ),
    ]
}

/// The card for one episode.
fn detailed(episode: &Episode, now: Timestamp, origin: &str, width: usize) -> Vec<StyledLine> {
    let mut out: Vec<StyledLine> = Vec::new();
    let title = format!("{} — {}", episode.short_id(), episode.summary);
    out.push(StyledLine::with_face(title, Face::Emphasis));
    let confidence = episode
        .author_confidence
        .map_or_else(|| "?".to_owned(), |value| format!("{value:.2}"));
    out.push(StyledLine::plain(format!(
        "{} commit{} · {} {} · {}",
        thousands(u64::try_from(episode.n_commits()).unwrap_or(u64::MAX)),
        if episode.n_commits() == 1 { "" } else { "s" },
        episode.author.badge(),
        confidence,
        days::relative_ago(now, episode.created)
    )));
    out.push(StyledLine::with_face(
        format!(
            "{} {} · {} · {origin}",
            episode.state.glyph(),
            episode.state,
            "UTC"
        ),
        Face::Muted,
    ));
    if !episode.live {
        out.push(StyledLine::with_face(
            "gone — no current commit is in this episode any more (ADR-003 §6.3)",
            Face::Warning,
        ));
    }
    out.push(StyledLine::plain(""));
    push_field(
        &mut out,
        width,
        "size",
        &format!(
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
        ),
    );
    if episode.paths.is_empty() {
        push_field(
            &mut out,
            width,
            "where",
            "paths unknown — the index recorded none for this episode",
        );
    } else {
        let counts = top_paths(&episode.paths);
        let list: Vec<String> = counts
            .iter()
            .map(|(path, count)| format!("{path} ({count})"))
            .collect();
        push_field(&mut out, width, "where", &list.join(" · "));
    }
    if let Some(rule) = &episode.boundary_rule {
        let confidence = episode
            .boundary_confidence
            .map_or_else(|| "?".to_owned(), |value| format!("{value:.2}"));
        push_field(
            &mut out,
            width,
            "boundary",
            &format!("{rule} {confidence} — a rule, not a fact (ADR-003 §4.7)"),
        );
    } else {
        push_field(
            &mut out,
            width,
            "boundary",
            "unknown — no rule was recorded",
        );
    }
    let session = match (&episode.session, &episode.turn) {
        (Some(session), Some(turn)) => format!("session {session} · turn {turn}"),
        (Some(session), None) => format!("session {session} · turn unknown"),
        (None, _) => "no harness session on this episode".to_owned(),
    };
    push_field(&mut out, width, "session", &session);
    push_field(
        &mut out,
        width,
        "members",
        &if episode.members.is_empty() {
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
    );
    out.push(StyledLine::plain(""));
    push_delta(&mut out, width, episode);
    push_risk(&mut out, width, episode);
    out
}

/// The seven sections, as the delta model words them (`e2.11`, ADR-001 §7.9).
///
/// This is the whole of the card's analysis surface, and it is a *render*, not a computation: every
/// string on these rows came out of [`EpisodeDelta`] — a figure's value, a gap's short form, the owner
/// plan key that has to fix it. The card decides nothing about what an episode means: it has no
/// threshold, no classifier, no arithmetic on the counters above. That is what makes the same seven
/// sections appear in `fathom analyze --json`, in the export and in an agent's tool result without any
/// of them agreeing by convention.
///
/// With no delta the card says so and names the verb that produces one, in one row: a card that printed
/// nothing here would look like an episode with nothing to report, which is a different and much more
/// dangerous claim.
fn push_delta(out: &mut Vec<StyledLine>, width: usize, episode: &Episode) {
    let Some(delta) = episode.delta.as_ref() else {
        push_field(
            out,
            width,
            "delta",
            &format!(
                "no document was handed to this card — {} is the one source",
                insight::UNDELIVERED_VERB
            ),
        );
        return;
    };
    for row in brief(delta) {
        push_field(out, width, &row.label(), &row.value);
    }
    // The ranking's ceiling on its own row, so the pane's word-wrap cannot cut the phrase in half and
    // leave a bare number that reads as an all-clear — the failure ADR-001 §7.9 is written against. The
    // sentence is the model's (`RiskView::blind_spot_note`), so no surface words the ceiling differently.
    if let Some(risk) = delta.risk.as_ref()
        && let Some(note) = risk.blind_spot_note()
    {
        push_field(out, width, "blind", &note);
    }
    // The stamp the card is answered under, so a screenshot says which contract it was drawn from.
    push_field(
        out,
        width,
        "model",
        &format!(
            "{contract} v{version} · self-check {check}",
            contract = delta.contract,
            version = delta.schema_version,
            check = if delta.integrity.verified {
                "clean"
            } else {
                "FAILED — the rows above are not trustworthy"
            },
        ),
    );
}

/// The `risk` field, and the reasons behind it.
///
/// Three depths, because the pane is small and the question has three sizes. With no ranking the row
/// says so and names the ticket; with a bare score it prints the score; with a full [`RiskView`] it
/// prints the score, its place in the ranking, the arithmetic, and one line per component that
/// contributed — which is what `w` opens and what makes the number an argument rather than an assertion.
///
/// The number is read out of the delta document, which is the only place it lives: the card has no
/// ranking of its own to fall back on, and an episode whose delta carries no ranking is an episode that
/// has not been ranked.
fn push_risk(out: &mut Vec<StyledLine>, width: usize, episode: &Episode) {
    let Some(risk) = episode.risk() else {
        push_field(
            out,
            width,
            "risk",
            "unknown — nothing has ranked this episode (`reco-c05` ranks the spine; e4.4 ranks \
             findings)",
        );
        return;
    };
    let head = format!(
        "{model}{score}{place}",
        model = if risk.model.is_empty() {
            String::new()
        } else {
            format!("{} ", risk.model)
        },
        score = risk.score,
        place = if risk.place().is_empty() {
            String::new()
        } else {
            format!(" · {}", risk.place())
        }
    );
    push_field(out, width, "risk", &head);
    let badge = risk.badge();
    if !badge.is_empty() {
        push_field(out, width, "surfaces", &badge);
    }
    if !risk.vector.is_empty() {
        push_field(out, width, "vector", &risk.vector);
    }
    for reason in risk.reasons() {
        push_field(out, width, "why", &reason);
    }
    if let Some(first) = risk.blind_spots.first() {
        push_field(
            out,
            width,
            "blind",
            &format!(
                "a spine cannot see: {}{} — `w` lists all {}",
                first,
                if risk.blind_spots.len() > 1 {
                    format!(" ({} more)", risk.blind_spots.len() - 1)
                } else {
                    String::new()
                },
                risk.blind_spots.len()
            ),
        );
    }
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
fn push_field(out: &mut Vec<StyledLine>, width: usize, label: &str, value: &str) {
    let head = format!("{label:<9}");
    let indent = " ".repeat(head.chars().count().min(width));
    let room = width.saturating_sub(head.chars().count()).max(8);
    let mut first = true;
    for chunk in wrap(value, room) {
        if first {
            let mut line = StyledLine::new();
            line.push(Face::Muted, &head);
            line.push(Face::Text, &chunk);
            out.push(line);
            first = false;
        } else {
            out.push(StyledLine::plain(format!("{indent}{chunk}")));
        }
    }
}

/// The wrap lives with the rail's renderer now (`render::wrap`): the empty rail and the card
/// fold text the same way, and two copies of a word-wrap drift.
use super::render::wrap;

/// The four path prefixes that account for the most of the episode's files — `where`, the way the
/// card's mock prints it (`src/store (7) · tests (2)`).
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
    rows.truncate(4);
    rows
}

/// A number with its thousands separators, for counts the card states in words.
fn thousands(value: u64) -> String {
    crate::app::format_thousands(value)
}
