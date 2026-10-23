//! One place that decides how a scorecard reads, so the shell, `fathom check` (**reco-ngk**), the exported
//! artefact (**e5.7**) and a dashboard all print the same sentence for the same number (`e3.9`,
//! `reco-al1`).
//!
//! [`Report`] is the whole answer to "how is the architecture doing, and what happened since I looked":
//! the head scorecard, the previous one, the trend across what the index holds, and the since-last-review
//! delta — as one value, assembled once, in the composition root. It is a value rather than four getter
//! calls for the same reason [`crate::cycles::report::Report`] is: a surface that recomputed a graph to
//! answer a second question would be a second analysis (lint `M03`), and a document that disagrees with
//! its own JSON is two bugs instead of one.
//!
//! # The exported document
//!
//! [`Report::to_json`] is `fathom-architecture/1`. Three rules hold it together:
//!
//! * **Every number the prose prints is in the JSON, with the same rounding.** A test asserts it.
//! * **An absence is a key with a reason**, never a missing key: `"api": {"measured": false, "unscored":
//!   "…"}` says what is missing and why, and `"anchor": {"kind": "window", "price": "…"}` says which rung
//!   the delta came from and what that rung costs.
//! * **Partial is a state, not a rounding.** `truncated` and `thinned` are exported, so a consumer can tell
//!   "the index holds 40 revisions" from "the trend was capped at 40".

use serde_json::json;

use super::policy::Policy;
use super::review::Delta;
use super::score::Scorecard;
use super::trend::Trend;
use super::{CONTRACT, SCHEMA_VERSION};

/// What the index held when this was assembled — the denominator of every "of N revisions" claim in the
/// document, and the part a cached artefact has to carry because a reader of a two-week-old file cannot
/// ask the index again.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Index {
    /// Graph revisions the store lists, whether or not any of them is scored.
    pub revisions: usize,
    /// How many of them have a scorecard row.
    pub scored: usize,
    /// How many were read for this report.
    pub read: usize,
    /// The newest revision the store knows, and the one scored here, which are the same thing unless the
    /// caller asked for a specific revision.
    pub newest: String,
    /// The revision this report is about.
    pub head: String,
    /// The head's episode, when the index could name one.
    pub episode: Option<String>,
    /// How many revisions have no scorecard row yet, which is the difference between "nothing changed"
    /// and "nobody looked".
    pub unscored: usize,
}

impl Index {
    /// The line a pane prints.
    #[must_use]
    pub fn sentence(&self) -> String {
        format!(
            "{} graph revision(s) · {} scored ({} read for this report) · head {}{}",
            self.revisions,
            self.scored,
            self.read,
            short(&self.head),
            if self.unscored == 0 {
                String::new()
            } else {
                format!(" · {} unscored", self.unscored)
            }
        )
    }
}

/// The whole architecture answer, in one value.
#[derive(Debug, Clone)]
pub struct Report {
    /// The head scorecard: the newest revision, scored.
    pub head: Scorecard,
    /// The previous scored revision, when there is one — the per-episode comparison.
    pub previous: Option<Scorecard>,
    /// The delta from the reviewer's last review, when an anchor could be found.
    pub since: Option<Delta>,
    /// The trend across what was read.
    pub trend: Trend,
    /// The knobs in force.
    pub policy: Policy,
    /// What the index held.
    pub index: Index,
    /// The companion the head's stored row carries: what "since your last review" meant on the day this
    /// revision was scored, as [`Delta::companion_json`] wrote it. Present when the row has one, which is a
    /// fact about the past that no live computation can recover — review state can be cleared, and an episode
    /// can be re-segmented out from under the anchor.
    pub since_recorded: Option<serde_json::Value>,
    /// Everything that limited this answer, gathered from every part: a chain that would not materialise,
    /// a dimension that said nothing, a row written by another model.
    pub notices: Vec<String>,
}

impl Report {
    /// The one-line headline a card, a CI log or a dashboard shows.
    #[must_use]
    pub fn headline(&self) -> String {
        match &self.since {
            Some(delta) => format!("{} · {}", self.head.headline(), delta.headline()),
            None => format!(
                "{} · window mean {}",
                self.head.headline(),
                self.trend.window_summary().mean_score
            ),
        }
    }

    /// The prose, one element per line: headline, the since-review block, the scorecard, the window and
    /// the trend, then the knobs, then what could not be done.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        let mut out = vec![format!(
            "architecture-1 · {} · {}",
            short(&self.index.head),
            self.index.episode.as_deref().unwrap_or("no episode named")
        )];
        out.push(self.headline());
        out.push(String::new());

        if let Some(delta) = &self.since {
            out.push("since last review".to_owned());
            out.extend(indent(delta.render()));
            out.push(String::new());
        } else if let Some(recorded) = &self.since_recorded {
            // Nothing to compare *now*, and a record of a comparison that once existed. Printed rather than
            // kept in the JSON, because "since when?" is the question a reader of a stale review state has.
            out.push(format!(
                "recorded with this revision: {} scored revision(s) since the last review ({}{})",
                recorded["revisions"].as_i64().unwrap_or(0),
                recorded["anchor_kind"].as_str().unwrap_or("none"),
                recorded["state"]
                    .as_str()
                    .map_or(String::new(), |state| format!(", {state}")),
            ));
            out.push(String::new());
        }
        if let (Some(head), Some(previous)) = (self.trend.head.clone(), self.trend.previous.clone())
        {
            out.push(format!(
                "this revision vs the last scored one: {} → {} ({}{})",
                previous.score,
                head.score,
                if i64::from(head.score) > i64::from(previous.score) {
                    "+"
                } else {
                    ""
                },
                i64::from(head.score) - i64::from(previous.score)
            ));
        }

        out.push("scorecard".to_owned());
        out.extend(indent(
            self.head
                .sentence()
                .lines()
                .map(str::to_owned)
                .collect::<Vec<_>>(),
        ));
        out.push(String::new());

        out.push("trend".to_owned());
        out.extend(indent(self.trend.render()));
        out.push(String::new());

        out.push("knobs".to_owned());
        out.extend(indent(self.policy.lines()));

        let notices: Vec<String> = self.complaints();
        if !notices.is_empty() {
            out.push(String::new());
            out.push("what this could not do".to_owned());
            for notice in notices {
                out.push(format!("  notice: {notice}"));
            }
        }
        out
    }

    /// Every caveat, in the order a reader should see them: the head's own, then the trend's, then the
    /// ones the caller supplied.
    #[must_use]
    pub fn complaints(&self) -> Vec<String> {
        let mut out = self.head.notices.clone();
        if let Some(delta) = &self.since {
            out.extend(delta.notices.iter().cloned());
        }
        if self.trend.rows_foreign > 0 {
            out.push(format!(
                "{} scorecard row(s) were written by another model and are not in the trend",
                self.trend.rows_foreign
            ));
        }
        if self.trend.rows_unreadable > 0 {
            out.push(format!(
                "{} scorecard row(s) would not parse and are not in the trend",
                self.trend.rows_unreadable
            ));
        }
        if self.index.unscored > 0 {
            out.push(format!(
                "{} of {} graph revision(s) have no scorecard row, so the trend covers {} of them",
                self.index.unscored, self.index.revisions, self.trend.rows_read
            ));
        }
        out.extend(self.policy.notices.iter().cloned());
        out.extend(self.notices.iter().cloned());
        let mut seen: Vec<String> = Vec::new();
        out.retain(|line| {
            if seen.contains(line) {
                return false;
            }
            seen.push(line.clone());
            true
        });
        out
    }

    /// `fathom-architecture/1`: the document `fathom check`, the exported artefact and a dashboard read.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        json!({
            "contract": CONTRACT,
            "schema_version": SCHEMA_VERSION,
            "headline": self.headline(),
            "index": {
                "revisions": self.index.revisions,
                "scored": self.index.scored,
                "read": self.index.read,
                "unscored": self.index.unscored,
                "newest": self.index.newest,
                "head": self.index.head,
                "episode": self.index.episode,
            },
            "scorecard": self.head.to_json(),
            "previous": self.previous.as_ref().map(Scorecard::to_json),
            "since_last_review": self.since.as_ref().map(Delta::to_json),
            "since_recorded": self.since_recorded.clone(),
            "trend": self.trend.to_json(),
            "policy": self.policy.to_json(),
            "notices": self.complaints(),
        })
    }
}

/// Indent a block by two columns, which is how every multi-line section in this product is nested.
fn indent(lines: Vec<String>) -> Vec<String> {
    lines.into_iter().map(|line| format!("  {line}")).collect()
}

/// A revision shortened the way every other pane shortens one.
fn short(revision: &str) -> String {
    if revision.chars().count() <= 12 {
        return revision.to_owned();
    }
    revision.chars().take(7).collect()
}
