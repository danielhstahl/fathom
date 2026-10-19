//! The retrospective: what got skipped, who skipped it, and whether it is a pattern.
//!
//! The queue's honest failure mode is not a slow reviewer; it is a reviewer who clears it by dismissing
//! and then believes the queue. `x` and `-` look identical from where the strip sits — both advance the
//! cursor — so the ticket's fourth criterion is a view: *"what you skipped and why"*, with the dismissed
//! categories rolled up so systematic dismissing is visible instead of invisible.
//!
//! # What it is allowed to say
//!
//! It counts acts, buckets them by the closed reason vocabulary ([`super::disposition::REASONS`]), says
//! how many people did it, when the last one was, and which was the riskiest thing in each bucket. It
//! says **pattern**, never **wrong**: `[not-my-area] 12 of 18 dismissals (67%) · both reviewers · last
//! 2h ago — that is a pattern, not a stack` is a sentence the reviewer can act on, and the most it is
//! permitted to assert is that a category is large enough to be worth noticing. Anything harsher — a
//! score for review quality, a refusal to let you dismiss — would be the tool grading a human's judgement,
//! which ADR-001 §7.9 puts the other way round: the numbers are here for the reviewer to check, not for
//! the tool to opine.
//!
//! # The threshold, and why it is two numbers
//!
//! A category is `systematic` at [`SYSTEMATIC_MIN_COUNT`] acts **and** [`SYSTEMATIC_MIN_SHARE_BP`] of the
//! window's dismissals. Share alone flags a repository with three dismissals; count alone flags a
//! repository with ten thousand acts where 6% is still the biggest bucket. Two conditions, both printed
//! beside the verdict, so a reviewer can disagree with the rule rather than merely be subject to it.

use std::collections::BTreeMap;

use fathom_core::time::Timestamp;
use serde::Serialize;

use super::disposition::{Disposition, ReasonVocabulary, TriageRecord};
use super::order::Severity;

/// One reason's bucket while the rollup is being accumulated.
#[derive(Debug, Default)]
struct Bucket {
    /// Acts in it.
    count: usize,
    /// Who used it.
    actors: std::collections::BTreeSet<String>,
    /// The most recent act.
    last: Option<Timestamp>,
    /// The highest-risk thing dismissed under it.
    riskiest: Option<SkippedEpisode>,
}

/// How many acts in one category make it worth calling a pattern, at minimum.
pub const SYSTEMATIC_MIN_COUNT: usize = 5;

/// The same rule as a share of the window's dismissals, in basis points (4 000 = 40%).
pub const SYSTEMATIC_MIN_SHARE_BP: i64 = 4_000;

/// How far back the retrospective looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum QueueWindow {
    /// Everything the ledger holds.
    All,
    /// The last N days.
    Days(u32),
}

impl QueueWindow {
    /// The words the heading prints.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            QueueWindow::All => "everything the ledger holds".to_owned(),
            QueueWindow::Days(days) => format!("the last {days} days"),
        }
    }

    /// Is this act inside the window?
    #[must_use]
    pub fn contains(self, utc: Timestamp, now: Timestamp) -> bool {
        match self {
            QueueWindow::All => true,
            QueueWindow::Days(days) => {
                now.as_unix_seconds() - utc.as_unix_seconds()
                    <= i64::from(days).saturating_mul(86_400)
            }
        }
    }
}

/// One dismiss category, rolled up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rollup {
    /// The code.
    pub code: String,
    /// What the code claims, printed instead of the code where there is room.
    pub claim: String,
    /// How many acts.
    pub count: usize,
    /// Its share of the window's dismissals, in basis points.
    pub share_bp: i64,
    /// How many distinct reviewers used it — one person's habit and a team's habit are different things.
    pub actors: usize,
    /// When the most recent one was.
    pub last: Timestamp,
    /// The highest-risk episode dismissed under it, and who dismissed it.
    pub riskiest: Option<SkippedEpisode>,
    /// Whether this bucket crosses both thresholds in [`SYSTEMATIC_MIN_COUNT`] and
    /// [`SYSTEMATIC_MIN_SHARE_BP`].
    pub systematic: bool,
}

impl Rollup {
    /// The share as printed: `67%`.
    #[must_use]
    pub fn share(&self) -> String {
        format!("{}%", self.share_bp / 100)
    }
}

/// One skipped episode, with the reason it was skipped for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkippedEpisode {
    /// The episode.
    pub episode: String,
    /// What was decided about it.
    pub disposition: Disposition,
    /// The dismiss code, if any.
    pub reason: String,
    /// The note, if any.
    pub note: String,
    /// Who did it.
    pub actor: String,
    /// When.
    pub utc: Timestamp,
    /// The risk at the time, and what it implies.
    pub risk: Option<u8>,
    /// See [`SkippedEpisode::risk`].
    pub severity: Option<Severity>,
}

/// How much of the index the reviewer has actually decided on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Coverage {
    /// Episodes with a verdict.
    pub decided: usize,
    /// Episodes in the index.
    pub total: usize,
    /// The share, in basis points.
    pub share_bp: i64,
}

impl Coverage {
    /// The line, printed even when the number is uncomfortable — which is when it matters.
    #[must_use]
    pub fn line(&self) -> String {
        if self.total == 0 {
            return "coverage: nothing indexed, so nothing reviewed".to_owned();
        }
        format!(
            "coverage: {} of {} episodes decided · {}%",
            self.decided,
            self.total,
            self.share_bp / 100
        )
    }
}

/// The retrospective, in the shape both the pane and the CLI print.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QueueRetrospective {
    /// The window it was built over.
    pub window: QueueWindow,
    /// Acts in the window, all dispositions.
    pub acts: usize,
    /// Acts in the window by disposition, in [`Disposition`]'s order, zeroes dropped.
    pub by_disposition: Vec<(Disposition, usize)>,
    /// Dismissals by category, largest first.
    pub dismissals: Vec<Rollup>,
    /// The dismissed-with-a-reason episodes, highest risk first — the list a reviewer most needs to see
    /// and least wants to.
    pub skipped: Vec<SkippedEpisode>,
    /// How much of the index has a verdict.
    pub coverage: Coverage,
    /// What the rollup wants said out loud, in order of how much it wants saying.
    pub notices: Vec<String>,
    /// The clock this was built against, kept so "last 2h ago" can be printed without any part of the
    /// rendering going and reading a clock of its own.
    pub now: Timestamp,
}

impl QueueRetrospective {
    /// The pane's lines: the heading, the categories, the skipped list, the coverage line.
    ///
    /// Plain strings, because the shell owns colour (ADR-006 §6.4) and `--plain` owns the pipe. The
    /// indentation is the structure: one line per category, one line per skipped episode, and the
    /// notices last so the sentence that argues comes after the numbers it argues about.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        let mut out = vec![
            format!("what you skipped, and why — {}", self.window.label()),
            format!(
                "{} acts: {}",
                self.acts,
                self.by_disposition
                    .iter()
                    .map(|(disposition, count)| format!("{count} {}", disposition.label()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ];
        if self.dismissals.is_empty() {
            out.push(String::new());
            out.push("nothing dismissed — every episode here was read or signed off".to_owned());
        } else {
            out.push(String::new());
            out.push("dismissed, by reason:".to_owned());
            for rollup in &self.dismissals {
                out.push(format!(
                    "  {:<16} {:>3} {:>4}  {} reviewer{} · riskiest {} · last {}",
                    rollup.code,
                    rollup.count,
                    rollup.share(),
                    rollup.actors,
                    if rollup.actors == 1 { "" } else { "s" },
                    rollup
                        .riskiest
                        .as_ref()
                        .map(|skip| format!("{}", skip.risk.unwrap_or(0)))
                        .unwrap_or_else(|| "?".to_owned()),
                    epoch_phrase(rollup.last, self.now),
                ));
                out.push(format!("  {:<16}{}", "", rollup.claim));
                if rollup.systematic {
                    out.push(format!(
                        "  {:<16}{} ≥ {} acts and ≥ {}% of dismissals — a pattern, not a stack",
                        "",
                        "→",
                        SYSTEMATIC_MIN_COUNT,
                        SYSTEMATIC_MIN_SHARE_BP / 100
                    ));
                }
            }
        }
        if !self.skipped.is_empty() {
            out.push(String::new());
            out.push("ranked highest and dismissed anyway:".to_owned());
            for skip in self.skipped.iter().take(5) {
                out.push(format!(
                    "  {} risk {:>3} {} · {} · {}",
                    skip.episode.chars().take(6).collect::<String>(),
                    skip.risk.unwrap_or(0),
                    skip.disposition.label(),
                    if skip.reason.is_empty() {
                        skip.note.clone()
                    } else {
                        skip.reason.clone()
                    },
                    skip.actor,
                ));
            }
        }
        out.push(String::new());
        out.push(self.coverage.line());
        out.extend(self.notices.iter().map(|notice| format!("! {notice}")));
        out
    }
}

/// A timestamp as `today` / `yesterday` / `3d ago`, short enough for a rolled-up row. `now` is passed
/// in, as everywhere in this crate: a rendering that reads the clock is a rendering that cannot be
/// golden-tested.
#[must_use]
fn epoch_phrase(utc: Timestamp, now: Timestamp) -> String {
    let days = now.as_unix_seconds() / 86_400 - utc.as_unix_seconds() / 86_400;
    match days {
        ..=0 => "today".to_owned(),
        1 => "yesterday".to_owned(),
        days => format!("{days}d ago"),
    }
}

/// Build the retrospective.
///
/// `records` is the ledger, newest first or not — this sorts. `total` is how many live episodes the index
/// holds, which is what makes the coverage line a denominator rather than a count of what happened to be
/// in the ledger. `now` is the clock, passed in (ADR-004 §5.4).
#[must_use]
pub fn retrospect(
    records: &[TriageRecord],
    vocabulary: &ReasonVocabulary,
    total: usize,
    window: QueueWindow,
    now: Timestamp,
) -> QueueRetrospective {
    let in_window: Vec<&TriageRecord> = records
        .iter()
        .filter(|record| window.contains(record.utc, now))
        .collect();
    let mut by_disposition: BTreeMap<Disposition, usize> = BTreeMap::new();
    for record in &in_window {
        *by_disposition.entry(record.disposition).or_default() += 1;
    }
    let dismissals_total = by_disposition
        .get(&Disposition::Dismissed)
        .copied()
        .unwrap_or(0);

    let mut buckets: BTreeMap<String, Bucket> = BTreeMap::new();
    let mut skipped: Vec<SkippedEpisode> = Vec::new();
    for record in &in_window {
        if record.disposition != Disposition::Dismissed {
            continue;
        }
        let code = if record.reason.is_empty() {
            "unstated".to_owned()
        } else {
            record.reason.clone()
        };
        let bucket = buckets.entry(code).or_default();
        bucket.count += 1;
        bucket.actors.insert(record.actor.clone());
        if bucket.last.is_none_or(|last| record.utc > last) {
            bucket.last = Some(record.utc);
        }
        let skip = SkippedEpisode {
            episode: record.episode.clone(),
            disposition: record.disposition,
            reason: record.reason.clone(),
            note: record.note.clone(),
            actor: record.actor.clone(),
            utc: record.utc,
            risk: record.risk,
            severity: record.risk.map(Severity::of),
        };
        if bucket
            .riskiest
            .as_ref()
            .is_none_or(|best| skip.risk.unwrap_or(0) > best.risk.unwrap_or(0))
        {
            bucket.riskiest = Some(skip.clone());
        }
        skipped.push(skip);
    }

    let mut dismissals: Vec<Rollup> = buckets
        .into_iter()
        .map(|(code, bucket)| {
            let share_bp = if dismissals_total == 0 {
                0
            } else {
                i64::try_from(bucket.count).unwrap_or(i64::MAX) * 10_000
                    / i64::try_from(dismissals_total).unwrap_or(i64::MAX)
            };
            Rollup {
                claim: vocabulary.claim(&code),
                code,
                count: bucket.count,
                share_bp,
                actors: bucket.actors.len(),
                // A bucket only exists because an act landed in it, so the timestamp is always there;
                // `UNIX_EPOCH` prints the same "a long time ago" as any other fallback would.
                last: bucket.last.unwrap_or(Timestamp::UNIX_EPOCH),
                riskiest: bucket.riskiest,
                systematic: bucket.count >= SYSTEMATIC_MIN_COUNT
                    && share_bp >= SYSTEMATIC_MIN_SHARE_BP,
            }
        })
        .collect();
    dismissals.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.code.cmp(&right.code))
    });
    skipped.sort_by(|left, right| {
        right
            .risk
            .unwrap_or(0)
            .cmp(&left.risk.unwrap_or(0))
            .then_with(|| right.utc.cmp(&left.utc))
            .then_with(|| left.episode.cmp(&right.episode))
    });
    let decided = by_disposition
        .iter()
        .filter(|(disposition, _)| disposition.is_verdict())
        .map(|(_, count)| *count)
        .sum();
    let mut notices: Vec<String> = Vec::new();
    for rollup in dismissals.iter().filter(|rollup| rollup.systematic) {
        notices.push(format!(
            "`{}` is {} of {} dismissals in {} — the queue is being emptied, not read",
            rollup.code,
            rollup.count,
            dismissals_total,
            window.label()
        ));
    }
    let unstated = skipped.iter().filter(|skip| skip.reason.is_empty()).count();
    if unstated > 0 {
        notices.push(format!(
            "{unstated} dismissals carry no reason at all — they were written before the reason \
             was mandatory, or by a writer that did not enforce it"
        ));
    }
    let unranked = skipped.iter().filter(|skip| skip.risk.is_none()).count();
    if unranked > 0 {
        notices.push(format!(
            "{unranked} dismissed episodes had no risk score when they were dismissed, so their \
             absence from the ranking is not evidence that they were safe"
        ));
    }
    QueueRetrospective {
        window,
        acts: in_window.len(),
        by_disposition: by_disposition
            .into_iter()
            .filter(|(_, count)| *count > 0)
            .collect(),
        dismissals,
        skipped,
        coverage: Coverage {
            decided,
            total,
            share_bp: if total == 0 {
                0
            } else {
                i64::try_from(decided).unwrap_or(i64::MAX) * 10_000
                    / i64::try_from(total).unwrap_or(i64::MAX)
            },
        },
        notices,
        now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::disposition::REASONS;

    const NOW: i64 = 1_700_000_000;

    fn at(seconds: i64) -> Timestamp {
        Timestamp::from_unix_micros(seconds * 1_000_000)
    }

    /// A dismiss act, the shape the ledger hands back.
    fn dismissed(
        id: &str,
        reason: &str,
        risk: Option<u8>,
        actor: &str,
        age_hours: i64,
    ) -> TriageRecord {
        TriageRecord {
            episode: id.to_owned(),
            fingerprint: format!("sha256:{id}"),
            disposition: Disposition::Dismissed,
            reason: reason.to_owned(),
            note: String::new(),
            until: String::new(),
            actor: actor.to_owned(),
            utc: at(NOW - age_hours * 3_600),
            risk,
            risk_model: "spine-1".to_owned(),
        }
    }

    fn accept(id: &str, actor: &str) -> TriageRecord {
        TriageRecord {
            disposition: Disposition::Accepted,
            ..dismissed(id, "", Some(10), actor, 1)
        }
    }

    #[test]
    fn an_empty_ledger_says_nothing_was_skipped_rather_than_nothing_was_found() {
        let view = retrospect(
            &[],
            &ReasonVocabulary::shipped(),
            40,
            QueueWindow::All,
            at(NOW),
        );
        assert_eq!(view.acts, 0);
        assert!(view.dismissals.is_empty());
        let lines = view.lines();
        assert!(
            lines.iter().any(|line| line.contains("nothing dismissed")),
            "{lines:?}"
        );
        assert!(
            view.coverage.line().contains("0 of 40"),
            "coverage is a denominator over the index, not over the ledger: {}",
            view.coverage.line()
        );
    }

    #[test]
    fn a_rollup_carries_count_share_people_and_the_riskiest_thing_in_the_bucket() {
        let records = vec![
            dismissed("e-one", "not-my-area", Some(81), "ada", 3),
            dismissed("e-two", "not-my-area", Some(44), "bo", 2),
            dismissed("e-three", "intentional", Some(60), "ada", 1),
            accept("e-four", "ada"),
        ];
        let view = retrospect(
            &records,
            &ReasonVocabulary::shipped(),
            10,
            QueueWindow::Days(7),
            at(NOW),
        );
        assert_eq!(view.acts, 4);
        assert_eq!(view.dismissals.len(), 2);
        let top = &view.dismissals[0];
        assert_eq!(top.code, "not-my-area");
        assert_eq!(top.count, 2);
        assert_eq!(top.share_bp, 6_666, "2 of 3 dismissals, in basis points");
        assert_eq!(top.share(), "66%");
        assert_eq!(
            top.actors, 2,
            "two reviewers is a different sentence from one"
        );
        assert_eq!(
            top.riskiest.as_ref().and_then(|skip| skip.risk),
            Some(81),
            "the riskiest thing in the bucket is the number that makes a category worth reading"
        );
        assert!(
            top.claim.contains("someone else"),
            "the bucket prints its claim, not only its code: {}",
            top.claim
        );
        assert!(!top.systematic, "two acts is not a pattern at any share");
    }

    #[test]
    fn a_pattern_needs_both_a_count_and_a_share_and_says_which_rule_it_met() {
        let few_but_concentrated: Vec<TriageRecord> = (0..4)
            .map(|n| dismissed(&format!("e{n}"), "low-stakes", Some(20), "ada", 1))
            .collect();
        let view = retrospect(
            &few_but_concentrated,
            &ReasonVocabulary::shipped(),
            50,
            QueueWindow::All,
            at(NOW),
        );
        assert_eq!(view.dismissals[0].share_bp, 10_000);
        assert!(
            !view.dismissals[0].systematic,
            "100% of four dismissals is still four dismissals"
        );

        let many: Vec<TriageRecord> = (0..5)
            .map(|n| dismissed(&format!("e{n}"), "already-known", Some(75), "ada", 1))
            .collect();
        let view = retrospect(
            &many,
            &ReasonVocabulary::shipped(),
            50,
            QueueWindow::All,
            at(NOW),
        );
        let top = &view.dismissals[0];
        assert!(
            top.systematic,
            "five acts, all of the dismissals: a pattern"
        );
        assert!(!view.notices.is_empty(), "a pattern is printed, not hidden");
        assert!(
            view.notices[0].contains("emptied, not read"),
            "{:?}",
            view.notices
        );
        let lines = view.lines();
        assert!(
            lines
                .iter()
                .any(|line| line.contains("a pattern, not a stack")),
            "{lines:?}"
        );
    }

    #[test]
    fn a_reason_that_is_not_in_the_vocabulary_still_gets_its_own_bucket() {
        // A ledger written by an older build, or by a repository whose config has since dropped the
        // code. Dropping the acts would be the tool losing the reviewer's history on the floor.
        let records = vec![dismissed("e-one", "out-of-hours", Some(30), "ada", 1)];
        let view = retrospect(
            &records,
            &ReasonVocabulary::shipped(),
            5,
            QueueWindow::All,
            at(NOW),
        );
        assert_eq!(view.dismissals[0].code, "out-of-hours");
        assert!(
            !view.dismissals[0].claim.is_empty(),
            "an unknown code still gets a sentence"
        );
    }

    #[test]
    fn a_window_excludes_the_old_acts_and_an_unindexed_index_says_so() {
        let records = vec![
            dismissed("e-new", "intentional", Some(30), "ada", 2),
            dismissed("e-old", "intentional", Some(90), "ada", 24 * 30),
        ];
        let week = retrospect(
            &records,
            &ReasonVocabulary::shipped(),
            5,
            QueueWindow::Days(7),
            at(NOW),
        );
        assert_eq!(week.acts, 1);
        let all = retrospect(
            &records,
            &ReasonVocabulary::shipped(),
            5,
            QueueWindow::All,
            at(NOW),
        );
        assert_eq!(all.acts, 2);
        assert_eq!(all.dismissals[0].count, 2);

        let nothing = retrospect(
            &[],
            &ReasonVocabulary::shipped(),
            0,
            QueueWindow::All,
            at(NOW),
        );
        assert!(
            nothing.coverage.line().contains("nothing indexed"),
            "an empty index is not a clean sheet: {}",
            nothing.coverage.line()
        );
    }

    #[test]
    fn a_dismissal_with_no_risk_and_no_reason_is_called_out_separately() {
        let mut bare = dismissed("e-bare", "", None, "ada", 1);
        bare.reason.clear();
        let records = vec![bare];
        let view = retrospect(
            &records,
            &ReasonVocabulary::shipped(),
            3,
            QueueWindow::All,
            at(NOW),
        );
        assert_eq!(view.dismissals[0].code, "unstated");
        assert_eq!(view.notices.len(), 2, "{:?}", view.notices);
        assert!(
            view.notices
                .iter()
                .any(|notice| notice.contains("no reason at all")),
            "{:?}",
            view.notices
        );
        assert!(
            view.notices
                .iter()
                .any(|notice| notice.contains("no risk score")),
            "a dismissed episode nothing scored is not evidence of safety: {:?}",
            view.notices
        );
    }

    #[test]
    fn every_line_of_the_retrospective_fits_a_pane_and_the_shipped_reasons_are_the_buckets() {
        let records: Vec<TriageRecord> = REASONS
            .iter()
            .enumerate()
            .map(|(n, (code, _, _, _))| dismissed(&format!("e{n}"), code, Some(50), "ada", 1))
            .collect();
        let view = retrospect(
            &records,
            &ReasonVocabulary::shipped(),
            100,
            QueueWindow::All,
            at(NOW),
        );
        assert_eq!(view.dismissals.len(), REASONS.len());
        for line in view.lines() {
            assert!(!line.contains('\n'), "a line is a line: {line:?}");
        }
        assert!(
            view.lines()
                .iter()
                .any(|line| line.contains("what you skipped, and why")),
            "the pane has to say what it is showing"
        );
    }
}
