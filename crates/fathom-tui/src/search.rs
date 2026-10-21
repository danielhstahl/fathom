//! `fathom_tui::search` — the `/` box: what it scores against, how its rows read, and where `<Enter>`
//! goes. **This is ADR-006 §5.8's overlay in Rust**, owned by `reco-mkc` (`e5.2`).
//!
//! The scoring is not here. [`fathom_analyze::search`] owns the corpus, the scorer and the filters, and this
//! module only asks it a question and prints the answer — which is the whole reason the box can be honest
//! about *why* a row matched: the explanation and the ordering come from the same pass over the same fields,
//! so the preview pane cannot tell a story the ranking did not act on.
//!
//! Three rules:
//!
//! * **The box never reads.** The index is handed in ([`crate::frame::Options::search`]); a shell that
//!   opened a store to answer a keystroke would be a shell that broke ADR-002 §5.2 for the most
//!   latency-sensitive interaction in the application.
//! * **Every row says where `<Enter>` goes, and `<Enter>` goes there.** A hit whose kind has no view in this
//!   build says so in the preview pane instead of jumping somewhere nearby and hoping nobody notices.
//! * **An empty query is a question, not a blank.** It answers with the filter grammar and the last few
//!   searches, because a reviewer staring at an empty box is a reviewer about to open a second terminal.

use fathom_analyze::search::query::Query;
use fathom_analyze::search::{Hit, Index, Kind, Report};
use fathom_core::time::Timestamp;

/// How many hits the box asks for. ADR-004's budget is written against "top 20"; the box asks for a screen
/// and a half of them so a list that scrolls does not run out before the reviewer stops typing.
pub const RESULT_LIMIT: usize = 40;

/// What `<Enter>` on a hit does, decided by the hit's kind and not by a guess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    /// The timeline, with this episode selected — the deep link the reviewer means nine times out of ten.
    Timeline {
        /// The episode to select.
        episode: String,
        /// The hit's own label, for the toast that names what jumped.
        label: String,
    },
    /// The structure view, which is where a path lives.
    Structure {
        /// The path the hit named.
        path: String,
    },
    /// The dependency view.
    Dependencies {
        /// The package name the hit named.
        name: String,
    },
    /// Nothing to jump to: the box says why instead of moving the reviewer somewhere almost right.
    Say {
        /// The sentence.
        reason: String,
    },
}

impl Link {
    /// Where this hit goes, in the view that owns it.
    ///
    /// A finding is not a view in this build — findings are drawn *on* the episode that raised them — so a
    /// finding with an episode jumps to that episode and names the finding, and one without an episode
    /// says so. A symbol is the same story with `e3.1`: the scorer can rank a symbol row the moment a
    /// corpus contains one, and until the extractor exists the box does not pretend to have one.
    #[must_use]
    pub fn target(hit: &Hit) -> Link {
        match hit.kind {
            Kind::Episode => Link::Timeline {
                episode: hit.id.clone(),
                label: hit.label.clone(),
            },
            Kind::Path => Link::Structure {
                path: hit.label.clone(),
            },
            Kind::Dependency => Link::Dependencies {
                name: hit.label.clone(),
            },
            Kind::Finding => match &hit.episode {
                Some(episode) => Link::Timeline {
                    episode: episode.clone(),
                    label: format!("{} ({})", hit.label, hit.id),
                },
                None => Link::Say {
                    reason: format!(
                        "`{}` is a finding, and findings are drawn on the episode that raised them — \
                         `fathom why {}` has the evidence",
                        hit.label, hit.id
                    ),
                },
            },
            Kind::Symbol => Link::Say {
                reason: format!(
                    "`{}` is a symbol, and nothing has indexed symbols since `e3.1` — the corpus the \
                     box was handed has none",
                    hit.label
                ),
            },
        }
    }
}

/// The rows the two panes show for a query that was typed.
///
/// The result rows lead with a two-character kind, because the ADR's own mock does, and the kind is the
/// only thing in a fuzzy list that tells you whether you are about to open an episode or a lockfile. The
/// preview answers *why this matched* per token, which is the difference between a fuzzy list and a fuzzy
/// hunch.
///
/// `window` is `(first hit, how many to print)`. The box scores [`RESULT_LIMIT`] hits and prints the ones
/// that fit under the cursor, which is what makes the list scrollable and what the list motions in
/// [`crate::app::App::dispatch`] move — a result list the reviewer can page through and a result list whose
/// cursor walks off the bottom of the pane are not the same product (`e5.4`'s motion parity).
#[must_use]
pub fn rows(
    index: &Index,
    query: &Query,
    limit: usize,
    window: (usize, usize),
) -> (Vec<String>, Vec<String>, Report) {
    let report = index.search(query, limit);
    if report.hits.is_empty() {
        return (empty_results(&report), why_lines(&report, None), report);
    }
    let mut results = vec![format!(
        "{} of {} in a corpus of {} · scored in {}µs",
        report.hits.len(),
        report.candidates,
        report.documents,
        report.micros
    )];
    let mut group = None;
    for hit in report.hits.iter().skip(window.0).take(window.1.max(1)) {
        if group != Some(hit.kind) {
            group = Some(hit.kind);
            let count = report
                .groups
                .iter()
                .find(|(kind, _)| *kind == hit.kind)
                .map_or(0, |(_, count)| *count);
            results.push(format!("{} ({count})", hit.kind.as_str()));
        }
        results.push(row(hit));
    }
    (results, why_lines(&report, report.hits.first()), report)
}

/// Which slice of the hits a pane of `visible` rows shows, given the cursor.
///
/// The cursor is kept inside the window rather than the window kept at a round offset, because a cursor that
/// walks off the bottom is indistinguishable from a list that ran out — and the whole point of the row's
/// `>` marker is that it is on screen.
#[must_use]
pub fn window(cursor: usize, visible: usize) -> (usize, usize) {
    let visible = visible.max(1);
    if cursor < visible {
        return (0, visible);
    }
    (cursor - visible + 1, visible)
}

/// The rows for an empty box: what a query is made of, and the last few the reviewer typed.
#[must_use]
pub fn blank(recent: &[String], index: Option<&Index>) -> (Vec<String>, Vec<String>) {
    let mut results = Vec::new();
    match index {
        None => results.push(
            "nothing to search yet — this repository has no index for `/` to read (`fathom index`)"
                .to_owned(),
        ),
        Some(index) => {
            results.push(format!(
                "a corpus of {} rows · {} episodes indexed",
                index.len(),
                index.documents()
            ));
            if !recent.is_empty() {
                results.push("last searches".to_owned());
                for query in recent.iter().take(5) {
                    results.push(format!("  {query}"));
                }
            }
        }
    }
    let mut why = vec!["what a query is made of".to_owned()];
    why.extend(
        FILTERS
            .iter()
            .map(|(token, meaning)| format!("{token} {meaning}")),
    );
    if index.is_some() {
        why.push(String::new());
        why.push("↑ walks the last searches · Enter opens the top row".to_owned());
    }
    (results, why)
}

/// One result row: kind, identity, label, and whatever the corpus knows about it.
fn row(hit: &Hit) -> String {
    let mut line = format!("{} {}", short_kind(hit.kind), clip(&hit.label, 34));
    if let Some(risk) = hit.risk {
        line.push_str(&format!(" · risk {risk}"));
    }
    if !hit.detail.is_empty() {
        line.push_str(&format!(" · {}", clip(&hit.detail, 28)));
    }
    line
}

/// The preview pane: the per-token explanation, then the excerpt, then anything the report had to say.
fn why_lines(report: &Report, hit: Option<&Hit>) -> Vec<String> {
    let mut lines = Vec::new();
    match hit {
        None => {
            lines.push("nothing matched this query".to_owned());
            // The gaps belong here as much as in the list: "no symbol matches" is a different answer from
            // "nothing indexed symbols", and only one of them is the reviewer's fault.
            for gap in &report.gaps {
                lines.extend(
                    wrap(&format!("{}: {}", gap.kind.as_str(), gap.because), 22)
                        .take(3)
                        .map(|line| format!("! {line}")),
                );
            }
        }
        Some(hit) => {
            lines.push("why this matched".to_owned());
            for reason in &hit.why {
                lines.push(format!("  {reason}"));
            }
            if !hit.snippet.is_empty() {
                lines.push(String::new());
                for line in wrap(&hit.snippet, 22).take(3) {
                    lines.push(format!("  {line}"));
                }
            }
        }
    }
    for note in &report.notes {
        lines.push(String::new());
        lines.extend(wrap(note, 22).take(4).map(|line| format!("! {line}")));
    }
    lines
}

/// What an empty result set says. An empty list is an answer, and the answer is the report's own notes and
/// gaps — "no row matches" and "nothing was scored" are different sentences.
fn empty_results(report: &Report) -> Vec<String> {
    let mut lines = vec![format!(
        "nothing in a corpus of {} matches `{}`",
        report.documents, report.query
    )];
    for note in &report.notes {
        lines.extend(wrap(note, 44).take(4).map(|line| format!("! {line}")));
    }
    for gap in &report.gaps {
        lines.extend(
            wrap(&gap.because, 44)
                .take(4)
                .map(|line| format!("! {line}")),
        );
    }
    if lines.len() == 1 {
        lines.push("try a shorter word, or drop a filter".to_owned());
    }
    lines
}

/// The filter grammar, in the words the box prints. ADR-006 §5.8 lists the tokens; this is the same list
/// where a reviewer will actually read it, which is inside the box with an empty query.
pub const FILTERS: &[(&str, &str)] = &[
    ("author:", "who wrote it"),
    ("since:", "on or after a day (7d, 2026-03-01, yesterday)"),
    ("until:", "on or before a day"),
    ("risk>", "at least this risky"),
    ("risk<", "at most this risky"),
    ("kind:", "episode, path, dependency, finding"),
    ("path:", "touches this path"),
    ("dep:", "depends on this package"),
    ("finding:", "raised this finding kind"),
];

/// The identity a row prints: long enough to tell two episodes apart, short enough for a toast.
#[must_use]
pub fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

/// The two-character kind the result rows lead with.
const fn short_kind(kind: Kind) -> &'static str {
    match kind {
        Kind::Episode => "ep",
        Kind::Symbol => "sy",
        Kind::Finding => "fd",
        Kind::Dependency => "dp",
        Kind::Path => "pa",
    }
}

/// The query the box is asking, with the shell's clock — the shell does not read the clock for itself
/// (ADR-002 §4.4), so `since:` and `until:` are resolved against the instant the frame is drawn for.
#[must_use]
pub fn query(text: &str, now: Timestamp) -> Query {
    Query::parse(text, now)
}

/// Clip to a display width with an ellipsis, in characters — a label clipped mid-grapheme is worse than a
/// long one, and every pane in this application clips the same way (`reco-2a3`).
#[must_use]
pub fn clip(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count <= width {
        return text.to_owned();
    }
    let kept: String = text.chars().take(width.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// Wrap at a width without breaking a word, because the preview pane is 23 columns wide and a note that
/// runs off its edge is a note nobody reads.
fn wrap(text: &str, width: usize) -> impl Iterator<Item = String> + '_ {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.chars().count() + word.chars().count() + 1 > width && !current.is_empty() {
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
    lines.into_iter()
}
