//! The answer a surface reads: what the rules found, what this episode did, in one object.
//!
//! The ticket's word is DELTA, and it is the reason this file exists rather than a bare
//! [`crate::layering::engine::Assessment`]: a reviewer with 34 pre-existing violations and one new one
//! needs the one, and needs to know the other 34 are not news. So [`Report`] carries three different
//! answers and names each:
//!
//! | field | question it answers |
//! |---|---|
//! | [`Report::violations`] | what does this revision do, right now, listed |
//! | [`Report::delta`] | what did *this episode* introduce and resolve |
//! | [`Report::verdict`] | which of those may alarm, given the committed baseline |
//!
//! # The cost, stated
//!
//! An episode costs two linear passes over a revision's dependency rows plus whatever the cycle rules
//! asked for, and **no second materialisation**: `crate::layering::episode` reads the stored
//! `graph_delta` and applies it to the graph it already had, which is the same seam
//! `crate::cycles::episode` uses. [`Counters`] is the payload that claim is checked against.
//!
//! # The exported numbers
//!
//! [`Report::to_json`] puts four numbers at the top level — `violations`, `new_violations`,
//! `baselined_violations`, `resolved_violations` — and two denominators, `edges_checked` and
//! `cross_layer_edges`. That is the contract the scorecard (**reco-al1**) trends and the CI gate
//! (**reco-ngk**) exits on; everything else is below them, and each of them is a number a reader can
//! re-derive from the lists beside it. The second denominator is the one that says whether the first
//! one meant anything: a rule set with no cross-layer row to judge has found nothing in a sense of
//! "nothing" that a reviewer cannot afford to confuse with the other one (**reco-842**).

use serde_json::{Value, json};

use fathom_core::time::Timestamp;

use crate::cycles::debt::{Mode, Ratchet};
use crate::layering::debt::Verdict;
use crate::layering::engine::{Assessment, Counters};
use crate::layering::finding::Violation;
use crate::layering::locate::Locate;
use crate::layering::rules::Rules;

/// What one episode did to the violation set.
///
/// Keyed by [`Violation::key`], because a violation's identity is the rule and the pair (or the member
/// set), and a pair that moved is a different fact. A violation that was there, was rewritten by a
/// rename, and is there again under a new key reads as one resolved and one introduced — which is
/// correct: the file that owed the debt is a different file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Delta {
    /// Keys that appear in this revision and not the last.
    pub introduced: Vec<String>,
    /// Keys that were there and are gone.
    pub resolved: Vec<String>,
    /// Keys that are in both: still there, so not news, and not forgotten either.
    pub persisting: usize,
    /// How many violations were on each side, for the ratio a card prints.
    pub before: usize,
    /// As above, for the revision the episode ended at.
    pub after: usize,
    /// Whether there was an episode at all. `false` is the cold-index case, and it must never be printed
    /// as "nothing changed": a repository nobody compared to has no introduced and no resolved, which is
    /// the absence of a question rather than an answer to one.
    pub episode: bool,
}

impl Delta {
    /// Diff two assessments' violation sets.
    #[must_use]
    pub fn of(before: &[Violation], after: &[Violation]) -> Self {
        let before_keys: Vec<String> = before.iter().map(Violation::key).collect();
        let after_keys: Vec<String> = after.iter().map(Violation::key).collect();
        let mut delta = Self {
            before: before.len(),
            after: after.len(),
            episode: true,
            ..Self::default()
        };
        for key in &after_keys {
            if before_keys.contains(key) {
                delta.persisting += 1;
            } else {
                delta.introduced.push(key.clone());
            }
        }
        for key in &before_keys {
            if !after_keys.contains(key) {
                delta.resolved.push(key.clone());
            }
        }
        delta.introduced.sort();
        delta.introduced.dedup();
        delta.resolved.sort();
        delta.resolved.dedup();
        delta
    }

    /// A delta with nothing to compare against: the snapshot case, where nothing can be called news.
    #[must_use]
    pub fn alone(after: &[Violation]) -> Self {
        Self {
            after: after.len(),
            episode: false,
            ..Self::default()
        }
    }

    /// Whether the episode changed nothing architectural. The fast path a reviewer pays for most turns.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.introduced.is_empty() && self.resolved.is_empty()
    }

    /// How much moved.
    #[must_use]
    pub fn len(&self) -> usize {
        self.introduced.len() + self.resolved.len()
    }

    /// The one line a pane prints.
    #[must_use]
    pub fn sentence(&self) -> String {
        if !self.episode {
            return format!(
                "no earlier revision to compare to: {} violation(s) in this revision, none of them \
                 known to be new",
                self.after
            );
        }
        if self.before == 0 && self.after == 0 {
            return "no layering violation in either revision".to_string();
        }
        format!(
            "{} violation(s) introduced, {} resolved, {} unchanged ({} → {} in total)",
            self.introduced.len(),
            self.resolved.len(),
            self.persisting,
            self.before,
            self.after
        )
    }

    /// The machine-readable form.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "episode": self.episode,
            "introduced": self.introduced,
            "resolved": self.resolved,
            "persisting": self.persisting,
            "before": self.before,
            "after": self.after,
        })
    }
}

/// One layering answer, in one object: the state, the episode, the verdict, and what it cost to get.
#[derive(Debug, Clone)]
pub struct Report {
    /// The rules that produced it, kept so a report can be read without the config in hand.
    rules: Rules,
    /// The current revision's violations, in key order, after any bound.
    pub violations: Vec<Violation>,
    /// What this episode did.
    pub delta: Delta,
    /// Baseline versus this revision: debt, news, back-due, stale.
    pub verdict: Verdict,
    /// What was checked and what could not be.
    pub counters: Counters,
    /// How much of the file:line evidence arrived with the records.
    pub located: Locate,
    /// A bound stopped a list, or a rule was refused: a stated partial, never a shorter answer.
    pub truncated: bool,
    /// Every caveat, in the order it was found: the load's, the pass's and the ledger's.
    pub notices: Vec<String>,
    /// How many violations were found before any bound.
    pub found: usize,
}

impl Report {
    /// Build the report from one or two assessments.
    ///
    /// `before` is the assessment of the revision the episode started at; `None` is the snapshot case,
    /// where nothing can honestly be called introduced.
    #[must_use]
    pub fn build(
        rules: &Rules,
        after: Assessment,
        before: Option<&Assessment>,
        locate: &Locate,
        ratchet: &Ratchet,
        now: Timestamp,
    ) -> Self {
        let delta = match before {
            Some(before) => Delta::of(&before.violations, &after.violations),
            None => Delta::alone(&after.violations),
        };
        let verdict = Verdict::assess(&after.violations, &delta, ratchet, now, rules.mode);
        let mut notices: Vec<String> = Vec::new();
        for notice in rules
            .notices
            .iter()
            .chain(after.notices.iter())
            .chain(before.into_iter().flat_map(|before| before.notices.iter()))
        {
            if !notices.contains(notice) {
                notices.push(notice.clone());
            }
        }
        Self {
            rules: rules.clone(),
            violations: after.violations.clone(),
            delta,
            verdict,
            counters: after.counters,
            located: locate.clone(),
            truncated: after.truncated,
            notices,
            found: after.found,
        }
    }

    /// The rules that produced this report.
    #[must_use]
    pub fn rules(&self) -> &Rules {
        &self.rules
    }

    /// How many violations may alarm.
    #[must_use]
    pub fn violations_count(&self) -> usize {
        self.verdict.violations.len()
    }

    /// How many of those this episode caused — the number the reviewer's attention is actually on.
    #[must_use]
    pub fn new_violations(&self) -> usize {
        self.verdict.introduced_count()
    }

    /// How many are baselined debt and therefore never news.
    #[must_use]
    pub fn baselined_violations(&self) -> usize {
        self.verdict.baselined_count()
    }

    /// How many violations this episode removed.
    #[must_use]
    pub fn resolved_violations(&self) -> usize {
        self.delta.resolved.len()
    }

    /// Whether anything here is news at the configured mode.
    #[must_use]
    pub fn alarms(&self) -> bool {
        self.verdict.alarms()
    }

    /// The exit code the mode asks for, if any.
    #[must_use]
    pub fn exit_code(&self) -> Option<fathom_core::exit::ExitCode> {
        self.verdict.exit_code()
    }

    /// The news, grouped by the rule that produced it — the roll-up a card prints when the list is long.
    #[must_use]
    pub fn by_rule(&self) -> Vec<(String, usize)> {
        let mut out: Vec<(String, usize)> = Vec::new();
        for entry in &self.verdict.violations {
            if let Some(row) = out
                .iter_mut()
                .find(|(name, _)| *name == entry.violation.rule)
            {
                row.1 += 1;
            } else {
                out.push((entry.violation.rule.clone(), 1));
            }
        }
        out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        out
    }

    /// The paragraph for a pane: what was checked, what the episode did, and what the verdict says.
    #[must_use]
    pub fn sentence(&self) -> String {
        // The empty case is worth naming rather than summarising: "0 dependency row(s) checked" reads
        // like a clean bill of health and is not one — it means nothing was looked at.
        if self.counters.checked == 0 && self.counters.rules > 0 && self.verdict.mode != Mode::Off {
            return "nothing was examined: this revision has no dependency rows to judge, which is not \
                    the same as a clean architecture"
                .to_string();
        }
        let mut out = self.counters.sentence();
        out.push('\n');
        out.push_str(&self.delta.sentence());
        out.push('\n');
        out.push_str(&self.verdict.sentence());
        if self.truncated {
            out.push('\n');
            out.push_str(&format!(
                "the report bound stopped the list: {} violation(s) were found, {} are listed",
                self.found,
                self.violations.len()
            ));
        }
        out
    }

    /// The lines for a card: a header, one per rule that has news, then the news itself up to `cap`.
    #[must_use]
    pub fn summary_lines(&self, cap: usize) -> Vec<String> {
        let mut out = vec![self.delta.sentence(), self.counters.sentence()];
        for (rule, count) in self.by_rule() {
            out.push(format!("  {count} under `{rule}`"));
        }
        for entry in self.verdict.violations.iter().take(cap) {
            out.push(format!("  {}", entry.sentence()));
        }
        let hidden = self.verdict.violations.len().saturating_sub(cap);
        if hidden > 0 {
            out.push(format!("  … and {hidden} more"));
        }
        for debt in self.verdict.debt.iter().take(2) {
            out.push(format!("  debt: {}", debt.sentence()));
        }
        out
    }

    /// The exported form.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "violations": self.violations_count(),
            "new_violations": self.new_violations(),
            "baselined_violations": self.baselined_violations(),
            "resolved_violations": self.resolved_violations(),
            "edges_checked": self.counters.checked,
            "cross_layer_edges": self.counters.cross_layer,
            "rules_active": self.counters.rules,
            "rules_refused": self.counters.refused,
            "alarms": self.alarms(),
            "exit": self.exit_code().map(|code| code.as_i32()),
            "truncated": self.truncated,
            "found": self.found,
            "inert": self.rules.is_inert(),
            "rules": self.rules.to_json(),
            "delta": self.delta.to_json(),
            "verdict": self.verdict.to_json(),
            "counters": self.counters.to_json(),
            "locate": self.located.to_json(),
            "by_rule": self.by_rule().iter().map(|(rule, count)| json!({
                "rule": rule, "violations": count,
            })).collect::<Vec<Value>>(),
            "violation_list": self
                .violations
                .iter()
                .map(Violation::to_json)
                .collect::<Vec<Value>>(),
            "notices": self.notices,
        })
    }
}
