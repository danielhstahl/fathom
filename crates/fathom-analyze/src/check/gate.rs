//! The gate: what ran, what it objected to, and the one sentence that says why the run is red
//! (`e6.6`, `reco-ngk`).
//!
//! [`Gate`] is the whole answer to one `fathom check`, assembled from the [`CheckRun`]s the composition
//! root gathered. It is a value, not a process: it reads no clock, no store and no filesystem, so the
//! exit code, the JSON document, the SARIF log and the sentence a human reads are all projections of one
//! structure computed once — which is the only way five outputs can be relied on to agree.
//!
//! # The three states a check has, and why a fourth would be a lie
//!
//! ```text
//!   ran       — judged N things, objected to M of them
//!   off       — the policy switched it off, and says so in every output
//!   blocked   — could not judge, with the reason and the remedy
//! ```
//!
//! The distinction that matters is `ran{examined: 0}` versus `blocked`: an empty list of objections is a
//! clean bill of health *only* if something was examined. [`crate::layering`] learned the same lesson
//! ("0 dependency row(s) checked" reads as a pass and is not one) and the gate generalises it. A
//! repository whose index holds no module graph does not get a green *layering* row; it gets
//! `blocked` with *"no index at … yet"* and, if `[check] require` names the check, a failing run.
//!
//! # The threshold, and what "fails only on new violations" costs
//!
//! `fail_on` is a single ordering test on [`Severity`] — that is the whole mechanism. What makes it
//! *new violations only* is upstream of it: every check here is fed a delta (the layering report's
//! episode delta, the cycle ledger's `introduced` flag, the API diff, the dependency delta between the
//! two revisions), and the ratchet ledger has already removed what the repository agreed to forgive.
//! The gate holds no baseline of its own, because a second baseline is a second opinion about what is
//! forgiven and the two would diverge inside a release.
//!
//! # The one sentence
//!
//! [`Gate::sentence`] is the acceptance criterion: one sentence, and it points at the evidence. It is
//! generated from the same counts the JSON carries, so the log and the machine-readable document cannot
//! disagree, and it is a *method returning a String* rather than a printer: the CI log, the SARIF
//! `automationDetails` description and the markdown summary all quote it.

use serde::Serialize;

use super::finding::{Check, Finding, Severity};
use super::policy::Policy;

/// The contract id this document's shape is `schemas/check.schema.json` for.
pub const CONTRACT: &str = "fathom-check/1";

/// The range that was checked, and how the endpoints were chosen.
///
/// The *how* is not decoration: half of the questions about a CI gate are "what did it actually compare",
/// and a run that defaulted its base to `HEAD~1` because nothing said otherwise has to be able to say so
/// after the fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Range {
    /// The base endpoint, as resolved (a revision or the name it was resolved from).
    pub base: String,
    /// The head endpoint.
    pub head: String,
    /// Where the base came from: `--base`, `[check] base`, or the built-in default.
    pub base_from: String,
    /// Where the head came from, same spellings.
    pub head_from: String,
    /// Episodes the range holds, as the index counts them.
    pub episodes: usize,
    /// Whether the episode-shaped checks looked at fewer episodes than the range holds, and why.
    pub episodes_partial: Option<String>,
}

impl Range {
    /// The `base..head` spelling every output prints.
    #[must_use]
    pub fn text(&self) -> String {
        format!("{}..{}", self.base, self.head)
    }
}

/// Whether one check judged anything, and how loudly it is allowed to speak.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// It judged `examined` things. Zero examined is stated as zero, never folded into `blocked`.
    Ran {
        /// What was looked at — edges, packages, dependency rows, episodes. The denominator every count
        /// in the row is a count out of.
        examined: usize,
    },
    /// `[check.severity] <check> = off`, or the gate's own `mode = off`.
    Off,
    /// It could not judge, with the reason and — where there is one — the remedy.
    Blocked {
        /// The one-clause reason, in the engine's own words where one has them.
        reason: String,
        /// What to type, when there is something to type.
        remedy: Option<String>,
    },
}

impl Status {
    /// The JSON spelling.
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Ran { .. } => "ran",
            Status::Off => "off",
            Status::Blocked { .. } => "blocked",
        }
    }

    /// Whether this is the blocking state.
    #[must_use]
    pub fn is_blocked(&self) -> bool {
        matches!(self, Status::Blocked { .. })
    }
    /// The reason a blocked check gives, `None` for the other two states.
    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        match self {
            Status::Blocked { reason, .. } => Some(reason),
            _ => None,
        }
    }

    /// The remedy a blocked check offers, if it has one.
    #[must_use]
    pub fn remedy(&self) -> Option<&str> {
        match self {
            Status::Blocked { remedy, .. } => remedy.as_deref(),
            _ => None,
        }
    }
}

/// One check's row: its volume, its state, and what it objected to.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckRun {
    /// Which family this is.
    pub check: Check,
    /// The volume the policy gave it — `None` when it is off.
    pub volume: Option<Severity>,
    /// Whether it ran.
    pub status: Status,
    /// What it objected to, worst first. Truncated to `[check] max_findings`; the total is
    /// [`CheckRun::total`].
    pub findings: Vec<Finding>,
    /// How many findings there were before the cap.
    pub total: usize,
    /// Caveats from the engine: a partial read, a bound that bit, an ignored path count.
    pub notes: Vec<String>,
}

impl CheckRun {
    /// A check that judged things.
    #[must_use]
    pub fn ran(
        check: Check,
        volume: Option<Severity>,
        examined: usize,
        mut findings: Vec<Finding>,
    ) -> Self {
        findings.sort_by(compare);
        let total = findings.len();
        Self {
            check,
            volume,
            status: Status::Ran { examined },
            findings,
            total,
            notes: Vec::new(),
        }
    }

    /// A check the policy switched off.
    #[must_use]
    pub fn off(check: Check) -> Self {
        Self {
            check,
            volume: None,
            status: Status::Off,
            findings: Vec::new(),
            total: 0,
            notes: Vec::new(),
        }
    }

    /// A check that could not judge.
    #[must_use]
    pub fn blocked(check: Check, volume: Option<Severity>, reason: impl Into<String>) -> Self {
        Self {
            check,
            volume,
            status: Status::Blocked {
                reason: reason.into(),
                remedy: None,
            },
            findings: Vec::new(),
            total: 0,
            notes: Vec::new(),
        }
    }

    /// Attach the remedy to a blocked run (builder style; a no-op on the other states).
    #[must_use]
    pub fn with_remedy(mut self, remedy: impl Into<String>) -> Self {
        if let Status::Blocked { remedy: slot, .. } = &mut self.status {
            *slot = Some(remedy.into());
        }
        self
    }

    /// Attach engine caveats.
    #[must_use]
    pub fn with_notes(mut self, notes: Vec<String>) -> Self {
        self.notes = notes;
        self
    }

    /// Whether this row had any news at all.
    #[must_use]
    pub fn quiet(&self) -> bool {
        self.findings.is_empty()
    }
}

/// Findings silenced by `[check] ignore`, kept so a green run can say what it did not show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Silenced {
    /// The `check/rule` key.
    pub key: String,
    /// The `ignore` pattern that matched it.
    pub pattern: String,
    /// How many findings it swallowed.
    pub count: usize,
    /// The first sentence it swallowed, so the row is checkable.
    pub example: String,
}

/// The run's answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Gate {
    /// The policy in force, as read. Printed by every format, because a verdict without its threshold is
    /// a mood.
    pub policy: Policy,
    /// What was compared.
    pub range: Range,
    /// One row per check, in [`Check::ALL`] order.
    pub runs: Vec<CheckRun>,
    /// Findings the policy silenced, folded by key.
    pub silenced: Vec<Silenced>,
    /// Gate-level caveats: policy notices, resolution notices, anything that is not one check's.
    pub notices: Vec<String>,
    /// What built this answer (`fathom <version>`), set by the composition root.
    pub producer: String,
}

impl Gate {
    /// Assemble the answer from the runs: silence what `ignore` silences, sort, and be done.
    #[must_use]
    pub fn build(policy: Policy, range: Range, mut runs: Vec<CheckRun>) -> Self {
        let mut silenced: Vec<Silenced> = Vec::new();
        for run in &mut runs {
            let (kept, swallowed) =
                partition_ignored(&policy, run.check, std::mem::take(&mut run.findings));
            run.findings = kept;
            run.total = run.findings.len() + swallowed.iter().map(|item| item.count).sum::<usize>();
            for item in swallowed {
                if let Some(existing) = silenced
                    .iter_mut()
                    .find(|slot| slot.key == item.key && slot.pattern == item.pattern)
                {
                    existing.count += item.count;
                } else {
                    silenced.push(item);
                }
            }
            run.findings.sort_by(compare);
            // The per-check list bound. Applied here rather than in a printer so the five formats cannot
            // disagree about how many findings they are showing: `total` keeps the true count, and
            // `truncated` says the list is a prefix.
            if run.findings.len() > policy.max_findings {
                run.findings.truncate(policy.max_findings);
            }
        }
        runs.sort_by_key(|run| run.check);
        silenced.sort_by(|a, b| a.key.cmp(&b.key).then_with(|| a.pattern.cmp(&b.pattern)));
        Self {
            policy,
            range,
            runs,
            silenced,
            notices: Vec::new(),
            producer: String::new(),
        }
    }

    /// The findings at or above `fail_on` — the ones that make the run red.
    ///
    /// `mode = off` answers empty whatever the numbers are: the mode is the operator's stated choice to
    /// not have the gate decide, and the run says so out loud rather than being argued with.
    #[must_use]
    pub fn failing(&self) -> Vec<&Finding> {
        if !self.policy.decides() {
            return Vec::new();
        }
        let Some(threshold) = self.policy.fail_on else {
            return Vec::new();
        };
        let mut out: Vec<&Finding> = self
            .runs
            .iter()
            .flat_map(|run| run.findings.iter())
            .filter(|finding| finding.severity >= threshold)
            .collect();
        out.sort_by(|a, b| compare(a, b));
        out
    }

    /// The required checks that did not run. A required check is one where "I could not look" is a
    /// failure, because a gate that shrugs is how a pipeline goes green on a broken day.
    #[must_use]
    pub fn blocked_required(&self) -> Vec<&CheckRun> {
        self.runs
            .iter()
            .filter(|run| self.policy.requires(run.check) && run.status.is_blocked())
            .collect()
    }

    /// The counts by severity over everything that ran and was not silenced, in [`Severity::ALL`] order.
    ///
    /// An array of pairs rather than a map: a JSON object whose keys are the three severities is a shape a
    /// schema has to enumerate twice, and a consumer that iterates wants the order the report prints.
    #[must_use]
    pub fn counts(&self) -> [(Severity, usize); 3] {
        Severity::ALL.map(|level| {
            let count = self
                .runs
                .iter()
                .flat_map(|run| run.findings.iter())
                .filter(|finding| finding.severity == level)
                .map(|finding| finding.count)
                .sum::<usize>();
            (level, count)
        })
    }

    /// Total findings listed (after the cap), across every check.
    #[must_use]
    pub fn listed(&self) -> usize {
        self.runs
            .iter()
            .map(|run| run.findings.len())
            .sum::<usize>()
    }

    /// Total findings found before the cap.
    #[must_use]
    pub fn found(&self) -> usize {
        self.runs.iter().map(|run| run.total).sum::<usize>()
    }

    /// Whether nothing at all was examined — the state that must never read as a pass.
    #[must_use]
    pub fn nothing_examined(&self) -> bool {
        !self
            .runs
            .iter()
            .any(|run| matches!(run.status, Status::Ran { examined } if examined > 0))
    }

    /// The exit code: `7` when the run fails **and the mode enforces**, `0` otherwise.
    ///
    /// Everything else (no repository, no index, a bad policy) is decided before this is built — those
    /// are the composition root's typed failures, and conflating "the gate objected" with "the gate broke"
    /// is exactly what ADR-002 §7.3 exists to prevent.
    ///
    /// `mode = warn` objects without blocking — the same three-way switch `[ratchet] mode` is, and for the
    /// same reason: a repository adopts the gate by listening first, and opts into the blocked merge with
    /// `mode = "deny"` in the file the review sees. A default that blocked on day one would be unwound by
    /// the second week, which is the failure mode the whole ratchet design exists to avoid.
    #[must_use]
    pub fn exit_code(&self) -> fathom_core::exit::ExitCode {
        if self.outcome() == Outcome::Fail && self.policy.enforces() {
            fathom_core::exit::ExitCode::CheckViolations
        } else {
            fathom_core::exit::ExitCode::Ok
        }
    }

    /// The verdict, one of three, computed by the same test [`Gate::exit_code`] uses.
    #[must_use]
    pub fn outcome(&self) -> Outcome {
        if !self.policy.decides() {
            return Outcome::Off;
        }
        if !self.failing().is_empty() || !self.blocked_required().is_empty() {
            return Outcome::Fail;
        }
        // A gate that examined nothing has not passed anything. `off` is a stated decision — the operator
        // said which checks they do not want — while `blocked` is the absence of a decision, and the two
        // must not share an exit code. This is the rule that refuses the green run on a broken day.
        if self.nothing_examined() && self.runs.iter().any(|run| run.status.is_blocked()) {
            return Outcome::Fail;
        }
        Outcome::Pass
    }

    /// The one sentence, and the only thing a reader has to read.
    ///
    /// One sentence, with the numbers, the range and the worst finding's own pointer in it — the
    /// acceptance criterion. Everything longer is a different output format's job.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self.outcome() {
            Outcome::Off => format!(
                "fathom check decided nothing: `check.mode = off`, so {} finding(s) in {} are reported \
                 and no threshold was applied",
                self.found(),
                self.range.text()
            ),
            Outcome::Fail => {
                let mut out = if let Some(run) = self.blocked_required().first() {
                    format!(
                        "fathom check failed: `{}` is required and could not run — {}",
                        run.check.as_str(),
                        run.status.reason().unwrap_or("no reason given")
                    )
                } else if self.failing().is_empty() && self.nothing_examined() {
                    format!(
                        "fathom check failed: nothing was examined in {} — {}",
                        self.range.text(),
                        self.first_block_reason()
                            .unwrap_or_else(|| "every check was blocked".to_owned())
                    )
                } else {
                    let failing = self.failing();
                    let worst = failing.first();
                    let mut out = format!(
                        "fathom check failed: {} finding(s) at or above `{}` in {}",
                        failing.iter().map(|f| f.count).sum::<usize>(),
                        self.policy.fail_on.map_or("never", Severity::as_str),
                        self.range.text()
                    );
                    if let Some(worst) = worst {
                        let where_ = worst
                            .primary()
                            .map(|item| format!(" at {}", item.text()))
                            .unwrap_or_default();
                        let _ = std::fmt::Write::write_fmt(
                            &mut out,
                            format_args!(
                                " — worst is {} [{}]{}: {}",
                                worst.severity.as_str(),
                                worst.key(),
                                where_,
                                worst.message
                            ),
                        );
                    }
                    if self.listed() < self.found() {
                        let _ = std::fmt::Write::write_fmt(
                            &mut out,
                            format_args!(" ({} of {} listed)", self.listed(), self.found()),
                        );
                    }
                    out
                };
                // The one sentence has to explain the exit code too: a reader who sees a red verdict on a
                // run that exited 0 needs to know which of the two the mode decided.
                if !self.policy.enforces() {
                    out.push_str(" — `check.mode = warn`, so this is reported and not enforced");
                }
                out
            }
            Outcome::Pass if self.nothing_examined() => format!(
                "fathom check passed {} with nothing examined: every check was off or blocked, which is \
                 not the same as clean{}",
                self.range.text(),
                self.first_block_reason()
                    .map(|reason| format!(" — {reason}"))
                    .unwrap_or_default()
            ),
            Outcome::Pass => {
                let silenced = self.silenced.iter().map(|row| row.count).sum::<usize>();
                let mut out = format!(
                    "fathom check passed {}: {} finding(s) below the `{}` threshold, {} check(s) ran",
                    self.range.text(),
                    self.found(),
                    self.policy.fail_on.map_or("never", Severity::as_str),
                    self.runs
                        .iter()
                        .filter(|run| matches!(run.status, Status::Ran { .. }))
                        .count()
                );
                if silenced > 0 {
                    let _ = std::fmt::Write::write_fmt(
                        &mut out,
                        format_args!(", {silenced} silenced by `check.ignore`"),
                    );
                }
                out
            }
        }
    }

    /// The first blocked reason, for the pass-with-nothing-examined sentence.
    fn first_block_reason(&self) -> Option<String> {
        self.runs
            .iter()
            .find_map(|run| run.status.reason())
            .map(str::to_owned)
    }

    /// The whole answer as data: `fathom check --json` and nothing else prints this exact document.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::json;
        json!({
            "contract": CONTRACT,
            "producer": self.producer,
            "outcome": self.outcome().as_str(),
            "exit": self.exit_code().as_i32(),
            "exit_name": self.exit_code().name(),
            "why": self.sentence(),
            "range": {
                "base": self.range.base,
                "head": self.range.head,
                "base_from": self.range.base_from,
                "head_from": self.range.head_from,
                "text": self.range.text(),
                "episodes": self.range.episodes,
                "episodes_partial": self.range.episodes_partial,
            },
            "policy": self.policy,
            "summary": {
                "found": self.found(),
                "listed": self.listed(),
                "failing": self.failing().iter().map(|f| f.count).sum::<usize>(),
                "silenced": self.silenced.iter().map(|row| row.count).sum::<usize>(),
                // The number to read on a run that *passed*. `blocked_required` says a required check was
                // blocked, which the exit code already reflects; this says how much of the gate did not look,
                // which is the difference between "clean" and "clean as far as two checks could tell".
                "blocked": self.runs.iter().filter(|run| run.status.is_blocked()).count(),
                "by_severity": self.counts().iter().map(|(level, count)| json!({
                    "severity": level.as_str(),
                    "count": count,
                })).collect::<Vec<_>>(),
                "blocked_required": self.blocked_required().iter().map(|run| run.check.as_str()).collect::<Vec<_>>(),
            },
            "checks": self.runs.iter().map(|run| json!({
                "check": run.check.as_str(),
                "title": run.check.title(),
                "volume": run.volume.map(Severity::as_str),
                "status": run.status.as_str(),
                "examined": match run.status { Status::Ran { examined } => Some(examined), _ => None },
                "reason": run.status.reason(),
                "remedy": run.status.remedy(),
                "notes": run.notes,
                "findings_total": run.total,
                "truncated": run.findings.len() < run.total,
                "findings": run.findings,
            })).collect::<Vec<_>>(),
            "silenced": self.silenced,
            "notices": self.notices,
        })
    }
}

/// The verdict, as one of three named states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Something at or above the threshold, or a required check that could not run.
    Fail,
    /// Nothing above the threshold, and everything required ran.
    Pass,
    /// `check.mode = off`: reported, not decided.
    Off,
}

impl Outcome {
    /// The JSON spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Fail => "fail",
            Outcome::Pass => "pass",
            Outcome::Off => "off",
        }
    }
}

/// The order every list uses: worst first, then family, then rule, then fingerprint, so two runs that
/// found the same things print them in the same order.
fn compare(a: &Finding, b: &Finding) -> std::cmp::Ordering {
    b.severity
        .cmp(&a.severity)
        .then_with(|| a.check.cmp(&b.check))
        .then_with(|| a.rule.cmp(&b.rule))
        .then_with(|| a.message.cmp(&b.message))
}

/// Split findings into kept and silenced, folding the silenced ones by (`key`, `pattern`).
fn partition_ignored(
    policy: &Policy,
    check: Check,
    findings: Vec<Finding>,
) -> (Vec<Finding>, Vec<Silenced>) {
    let mut kept = Vec::with_capacity(findings.len());
    let mut silenced: Vec<Silenced> = Vec::new();
    for finding in findings {
        let key = finding.key();
        match policy.silenced_by(&key) {
            Some(pattern) => {
                let pattern = pattern.to_owned();
                if let Some(existing) = silenced
                    .iter_mut()
                    .find(|slot| slot.key == key && slot.pattern == pattern)
                {
                    existing.count += finding.count;
                } else {
                    silenced.push(Silenced {
                        key,
                        pattern,
                        count: finding.count,
                        example: finding.message.clone(),
                    });
                }
            }
            None => kept.push(finding),
        }
    }
    let _ = check;
    (kept, silenced)
}
