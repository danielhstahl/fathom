//! The plan: what the file would look like after this run, and the diff that says who did it and why.
//!
//! [`Plan::build`] is the whole policy of `e3.10` in one pure function. It is deliberately not a set of
//! methods on [`Ratchet`] — the ledger type is a *read* of a file, and a read cannot also be the act of
//! changing it, or the two would disagree about which rows the repository has agreed to.

use fathom_core::exit::ExitCode;
use fathom_core::time::{MICROS_PER_DAY, Timestamp};
use serde_json::{Value, json};

use crate::cycles::Cycle;
use crate::cycles::debt::{Allowance, EXPIRY_WARNING_DAYS, Mode, Ratchet, pattern_for};
use crate::layering::finding::Violation;

/// The contract id this plan's JSON payloads carry, so a CI script can tell a ratchet document from any
/// other document the same run prints.
pub const CONTRACT: &str = "fathom-ratchet/1";

/// How far ahead a granted allowance reaches when the author did not name a date: one quarter. An
/// allowance with no end is the thing this file exists to refuse, and a default that is a quarter long turns
/// up in a review before anybody has forgotten why it exists.
pub const DEFAULT_GRACE_DAYS: i64 = 90;

/// The kinds this verb can judge, and therefore the only kinds it may tighten.
///
/// A row of any other kind (`api`, `dep`, `hotspot` — ADR-004's `ratchet_entry` list) is left exactly where
/// it is: this run has no evidence about it, and a writer that deletes what it cannot see is worse than one
/// that keeps a row nobody has claimed yet.
pub const JUDGED_KINDS: [&str; 2] = ["cycle", "layer"];

/// One violation, in the two words the ledger needs to match it and the one line a reviewer reads.
///
/// The two engines that find violations disagree about everything else — a cycle is a member set, a
/// layering violation is a rule and an edge — and the ratchet does not care: [`Offence::pattern`] asks the
/// engine that owns the kind what this offence's identity is, which is the same question the two `debt`
/// modules already answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offence {
    /// `cycle` or `layer` ([`JUDGED_KINDS`]).
    pub kind: String,
    /// The key as its engine spells it: a cycle's display path, a layering violation's `rule::src->dst`.
    pub key: String,
    /// The line a reviewer reads, carried from the finding itself rather than paraphrased here.
    pub body: String,
}

impl Offence {
    /// A cycle, in the ledger's shape.
    #[must_use]
    pub fn cycle(cycle: &Cycle) -> Self {
        Offence {
            kind: "cycle".to_string(),
            key: cycle.display(),
            body: cycle.sentence(),
        }
    }

    /// A layering violation, in the ledger's shape.
    #[must_use]
    pub fn layer(violation: &Violation) -> Self {
        Offence {
            kind: crate::layering::debt::KIND.to_string(),
            key: violation.key(),
            body: violation.sentence(),
        }
    }

    /// The matching form, asked of the engine that owns this kind.
    #[must_use]
    pub fn pattern(&self) -> String {
        pattern_for(&self.kind, &self.key)
    }

    /// The machine-readable form.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({ "kind": self.kind, "key": self.key, "detail": self.body })
    }
}

/// The deliberate act: a permission being granted, with the four things that make it reviewable.
///
/// The constructor refuses an empty reason on purpose. `--update-baseline` with `--reason ""` is the exact
/// shape of a loosening nobody will be able to justify in six months, and a type that cannot be built without
/// one is the cheapest place to say so — the flag parser is the second line of defence, not the first.
#[derive(Debug, Clone)]
pub struct Grant {
    /// Who is on the hook for paying the debt off.
    pub owner: String,
    /// Why the allowance exists. Never empty ([`Grant::new`]).
    pub reason: String,
    /// The last day it is live, as the first instant after it — or `None` for a row written to never expire,
    /// which is allowed and said out loud, because refusing it would only teach authors to write a date a
    /// year further out.
    pub expires: Option<Timestamp>,
    /// The day the act happened, as the first instant after it, like every other date in the file.
    pub granted: Timestamp,
    /// Who asked for it.
    pub granted_by: String,
    /// The keys to grant, empty meaning "everything currently uncovered". Non-empty is `--key`, and a key
    /// that matches nothing is [`Plan::refused`] rather than a silent no-op.
    pub keys: Vec<String>,
}

impl Grant {
    /// A grant with a reason, an owner and an expiry, or the reason it was refused.
    ///
    /// # Errors
    ///
    /// A message naming what is missing when `owner` or `reason` is blank.
    pub fn new(
        owner: &str,
        reason: &str,
        expires: Option<Timestamp>,
        granted: Timestamp,
        granted_by: &str,
    ) -> Result<Self, String> {
        if reason.trim().is_empty() {
            return Err(
                "raising the baseline needs a reason — the sentence a reviewer reads instead of guessing. \
                 Pass it with `--reason \"...\"`."
                    .to_string(),
            );
        }
        if owner.trim().is_empty() {
            return Err(
                "raising the baseline needs an owner — an allowance nobody owns is an allowance nobody \
                 removes. Pass it with `--owner ...`."
                    .to_string(),
            );
        }
        Ok(Grant {
            owner: owner.trim().to_string(),
            reason: reason.trim().to_string(),
            expires,
            granted,
            granted_by: granted_by.trim().to_string(),
            keys: Vec::new(),
        })
    }

    /// Restrict the grant to these keys — `--key`, repeatable.
    #[must_use]
    pub fn with_keys(mut self, keys: &[String]) -> Self {
        self.keys = keys.iter().map(|key| key.trim().to_string()).collect();
        self
    }

    /// Whether this grant names keys at all: "adopt what is there" versus "grant these, and tell me if one
    /// of them is a typo".
    #[must_use]
    pub fn selective(&self) -> bool {
        !self.keys.is_empty()
    }

    /// Does one `--key` entry cover this offence?
    ///
    /// A bare key is tried as the offence's own grammar, because a reviewer pasting a key out of a report
    /// does not retype the kind; `kind=pattern` is the spelling that says which kind was meant, and is the
    /// only one that may refuse to match. Matching runs through the owning engine's grammar, so a pasted key
    /// normalises exactly as the row it will become.
    #[must_use]
    pub fn covers_key(offence: &Offence, want: &str) -> bool {
        let (kind, rest) = match want.split_once('=') {
            Some((kind, rest)) if !kind.trim().is_empty() => (Some(kind.trim()), rest.trim()),
            _ => (None, want.trim()),
        };
        if let Some(kind) = kind
            && kind != offence.kind
        {
            return false;
        }
        pattern_for(kind.unwrap_or(&offence.kind), rest) == offence.pattern()
    }

    /// Does the caller's `--key` list cover this offence at all?
    #[must_use]
    pub fn covers(&self, offence: &Offence) -> bool {
        self.keys.is_empty()
            || self
                .keys
                .iter()
                .any(|want| Grant::covers_key(offence, want))
    }

    /// The row this grant would write for one offence.
    #[must_use]
    pub fn allowance_for(&self, offence: &Offence) -> Allowance {
        Allowance {
            kind: offence.kind.clone(),
            key: offence.key.clone(),
            pattern: offence.pattern(),
            owner: self.owner.clone(),
            reason: self.reason.clone(),
            expires: self.expires,
            granted: Some(self.granted),
            granted_by: self.granted_by.clone(),
            line: 0,
        }
    }
}

/// One row leaving the file, with the sentence that says why the ratchet turned.
#[derive(Debug, Clone)]
pub struct Removal {
    /// The row, as it was written.
    pub allowance: Allowance,
    /// Why it went: the violation it forgave is gone.
    pub why: String,
}

impl Removal {
    /// The machine-readable form.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "kind": self.allowance.kind,
            "key": self.allowance.key,
            "owner": self.allowance.owner,
            "expires": self.allowance.expiry_day(),
            "reason": self.allowance.reason,
            "why": self.why,
        })
    }
}

/// Which kinds this run could actually judge.
///
/// A kind that was not evaluated is never tightened: switch the `[layering]` table off for an afternoon and
/// the layer rows in the ledger must still be there when it comes back, because what changed was the
/// question, not the code.
#[derive(Debug, Clone, Copy, Default)]
pub struct Evaluated {
    /// The cycle set was read ([`crate::cycles`]).
    pub cycles: bool,
    /// Layering rules were in force ([`crate::layering::rules::Rules::is_inert`] was false).
    pub layer: bool,
}

impl Evaluated {
    /// Whether one kind was judged. An unrecognised kind is treated as not judged by the layer engine,
    /// which is the conservative answer: nothing is tightened on a kind nobody read.
    #[must_use]
    pub fn holds(&self, kind: &str) -> bool {
        match kind {
            "cycle" => self.cycles,
            "layer" => self.layer,
            _ => false,
        }
    }

    /// Both engines ran, which is what [`crate::ratchet::inventory`] reports for a repository with rules.
    #[must_use]
    pub fn both() -> Self {
        Evaluated {
            cycles: true,
            layer: true,
        }
    }
}

/// What one run of the ratchet does, and would have done.
#[derive(Debug, Clone)]
pub struct Plan {
    /// Where the ledger lives, as the config named it.
    pub source: String,
    /// Whether a baseline file was read at all. `false` is stated, never treated as an empty ledger — and
    /// it is why a bare run never *creates* the file: creating it is a widening act.
    pub baselined: bool,
    /// The mode that decides how loudly an uncovered violation speaks.
    pub mode: Mode,
    /// The instant expiries were judged against.
    pub now: Timestamp,
    /// The rows as they were read.
    pub before: Vec<Allowance>,
    /// The rows as they should be written, in the file's own order.
    pub after: Vec<Allowance>,
    /// New rows: the widening this run was asked to do.
    pub added: Vec<Allowance>,
    /// Rows rewritten in place by the grant — an expired allowance re-granted is a fresh loosening and is
    /// printed as one, not hidden inside "unchanged".
    pub renewed: Vec<Allowance>,
    /// Rows leaving the file: the ratchet turning.
    pub removed: Vec<Removal>,
    /// Live rows whose day is inside the warning window — the ones that must be seen before they lapse.
    pub expiring: Vec<Allowance>,
    /// Rows whose day has passed and whose violation stayed: debt back due, and a re-grant is a new act.
    pub lapsed: Vec<Allowance>,
    /// Violations no live row covers. These are the ones that may fail CI.
    pub uncovered: Vec<Offence>,
    /// Rows this run left alone because it cannot judge their kind.
    pub untouched: Vec<Allowance>,
    /// Everything the run noticed, in the order it noticed it.
    pub notices: Vec<String>,
    /// Everything the run refused, each with what to type instead.
    pub refused: Vec<String>,
}

impl Plan {
    /// Decide what the file should become.
    ///
    /// `offences` is what the two engines found in the revision being looked at — the whole revision, not
    /// this episode's delta, because a baseline is a statement about the repository and not about the turn.
    /// `grant` is the deliberate act, if there is one; without it nothing is ever added.
    #[must_use]
    #[allow(clippy::too_many_lines)] // a row's four fates and the one act of widening, in one pass
    pub fn build(
        baseline: &Ratchet,
        offences: &[Offence],
        grant: Option<&Grant>,
        evaluated: Evaluated,
        now: Timestamp,
    ) -> Self {
        let mut plan = Plan {
            source: baseline
                .source
                .clone()
                .unwrap_or_else(|| crate::cycles::debt::DEFAULT_BASELINE.to_string()),
            baselined: baseline.loaded,
            mode: baseline.mode,
            now,
            before: baseline.allowances().to_vec(),
            after: Vec::new(),
            added: Vec::new(),
            renewed: Vec::new(),
            removed: Vec::new(),
            expiring: Vec::new(),
            lapsed: Vec::new(),
            uncovered: Vec::new(),
            untouched: Vec::new(),
            notices: baseline.notices.clone(),
            refused: Vec::new(),
        };
        let covered_by_live = |offence: &Offence| {
            baseline.allowances().iter().any(|row| {
                row.kind == offence.kind && row.pattern == offence.pattern() && row.is_live(now)
            })
        };
        let matched = |row: &Allowance| {
            offences
                .iter()
                .any(|offence| offence.kind == row.kind && offence.pattern() == row.pattern)
        };

        // Every row keeps one of four fates: it goes, it stays as live debt, it stays as debt that is back
        // due, or it is left alone because this run cannot see its kind.
        let mut kept: Vec<Allowance> = Vec::new();
        for row in baseline.allowances() {
            if !JUDGED_KINDS.contains(&row.kind.as_str()) || !evaluated.holds(&row.kind) {
                plan.untouched.push(row.clone());
                kept.push(row.clone());
                continue;
            }
            if !matched(row) {
                plan.removed.push(Removal {
                    allowance: row.clone(),
                    why: format!(
                        "no {} violation in this revision matches it any more — the debt was paid, so the \
                         permission goes with it",
                        row.kind
                    ),
                });
                continue;
            }
            if !row.is_live(now) {
                plan.lapsed.push(row.clone());
            }
            kept.push(row.clone());
        }
        if !plan.untouched.is_empty() {
            let mut kinds: Vec<&str> = plan.untouched.iter().map(|row| row.kind.as_str()).collect();
            kinds.sort_unstable();
            kinds.dedup();
            plan.notices.push(format!(
                "{} row(s) of kind {} were left alone: this verb judges {} and nothing else",
                plan.untouched.len(),
                kinds.join(", "),
                JUDGED_KINDS.join(", ")
            ));
        }

        // What is news against the ledger as it stands — before any widening, so the act is measured against
        // what the repository had agreed to rather than against what it is about to agree to.
        let mut news: Vec<Offence> = offences
            .iter()
            .filter(|offence| !covered_by_live(offence))
            .cloned()
            .collect();
        news.sort_by(|a, b| (&a.kind, a.pattern()).cmp(&(&b.kind, b.pattern())));
        // The widening act, if it was asked for.
        if let Some(grant) = grant {
            let mut fresh: Vec<Allowance> = Vec::new();
            let mut replaced: Vec<Allowance> = Vec::new();
            for offence in news.iter().filter(|offence| grant.covers(offence)) {
                let row = grant.allowance_for(offence);
                // Re-granting an expired row replaces it: two rows for one fact would make the next run's
                // verdict depend on which one the reader found first.
                if let Some(at) = kept
                    .iter()
                    .position(|row| row.kind == offence.kind && row.pattern == offence.pattern())
                {
                    let old = kept.remove(at);
                    plan.lapsed.retain(|lapsed| {
                        !(lapsed.kind == old.kind && lapsed.pattern == old.pattern)
                    });
                    replaced.push(row.clone());
                } else {
                    fresh.push(row.clone());
                }
            }
            for want in &grant.keys {
                if offences
                    .iter()
                    .any(|offence| Grant::covers_key(offence, want))
                {
                    continue;
                }
                plan.refused.push(format!(
                    "`{want}` matches no violation in this revision, so nothing was written for it: a row \
                     that matches nothing is deleted the next time the ratchet tightens, so writing one \
                     would be a permission nobody can see being used."
                ));
            }
            plan.added = fresh;
            plan.renewed = replaced;
            for row in plan.added.iter().chain(plan.renewed.iter()) {
                kept.push(row.clone());
            }
            plan.notices.push(format!(
                "{} allowance(s) granted by {} until {}, for: {}",
                plan.added.len() + plan.renewed.len(),
                if grant.granted_by.is_empty() {
                    "nobody in particular"
                } else {
                    &grant.granted_by
                },
                match grant.expires {
                    Some(expires) => format!("the end of {}", day_before(expires)),
                    None => "ever, which is a decision worth re-reading".to_string(),
                },
                grant.reason
            ));
        } else if !news.is_empty() {
            plan.notices.push(format!(
                "{} violation(s) have no live allowance. The ratchet does not widen itself: `fathom \
                 ratchet --update-baseline --reason \"...\"` writes them, and the diff is the review.",
                news.len()
            ));
        }

        plan.after = kept;
        plan.after.sort_by(|a, b| {
            (&a.kind, a.pattern.clone(), a.key.clone()).cmp(&(
                &b.kind,
                b.pattern.clone(),
                b.key.clone(),
            ))
        });
        // The verdict a surface reads is about the file *after* this run: an offence the act just granted is
        // debt now, and reporting it as news in the same breath as writing its row is how a gate starts
        // disagreeing with the file it is gating on.
        plan.uncovered = offences
            .iter()
            .filter(|offence| {
                !plan.after.iter().any(|row| {
                    row.kind == offence.kind && row.pattern == offence.pattern() && row.is_live(now)
                })
            })
            .cloned()
            .collect();
        if grant.is_some() && !plan.uncovered.is_empty() {
            plan.notices.push(format!(
                "{} violation(s) are still uncovered after the act — `--key` named only some of them.",
                plan.uncovered.len()
            ));
        }
        plan.expiring = plan
            .after
            .iter()
            .filter(|row| row.is_live(now) && row.expires_soon(now))
            .cloned()
            .collect();
        plan.lapsed
            .sort_by(|a, b| (&a.kind, a.pattern.clone()).cmp(&(&b.kind, b.pattern.clone())));
        plan
    }

    /// Does the file need rewriting?
    #[must_use]
    pub fn changed(&self) -> bool {
        !self.added.is_empty() || !self.renewed.is_empty() || !self.removed.is_empty()
    }

    /// The bytes to write, header included.
    #[must_use]
    pub fn render(&self) -> String {
        crate::cycles::debt::render_rows(&self.after)
    }

    /// How many live rows the file will hold: the number a pane calls "the debt".
    #[must_use]
    pub fn baselined_count(&self) -> usize {
        self.after
            .iter()
            .filter(|row| row.is_live(self.now))
            .count()
    }

    /// The `-`/`+` lines a reviewer reads: one per row, each naming its kind, its key, its owner, its date
    /// and its reason. This is what "the diff shows who and why" means in a terminal, where the pull
    /// request's own diff has not happened yet.
    #[must_use]
    pub fn diff(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for removal in &self.removed {
            out.push(format!(
                "- {} ({})",
                row_line(&removal.allowance),
                removal.why
            ));
        }
        for row in self.added.iter().chain(self.renewed.iter()) {
            out.push(format!("+ {}", row_line(row)));
        }
        out
    }

    /// The lines for rows that are about to stop being forgiven, with the days left spelled out.
    #[must_use]
    pub fn expiring_lines(&self) -> Vec<String> {
        self.expiring
            .iter()
            .map(|row| expiring_line(row, self.now))
            .collect()
    }

    /// The lines for rows whose day has passed and whose violation stayed.
    #[must_use]
    pub fn lapsed_lines(&self) -> Vec<String> {
        self.lapsed
            .iter()
            .map(|row| lapsed_line(row, self.now))
            .collect()
    }

    /// The paragraph a pane prints first: the ledger's state, then what this run did about it.
    #[must_use]
    pub fn summary(&self) -> String {
        let mut out = format!(
            "{} allowance(s) live in {} — {} expiring within {} day(s), {} back due, {} violation(s) \
             uncovered",
            self.baselined_count(),
            self.source,
            self.expiring.len(),
            EXPIRY_WARNING_DAYS,
            self.lapsed.len(),
            self.uncovered.len()
        );
        if !self.baselined {
            out.push_str(" (no baseline file was found, so nothing is forgiven)");
        }
        let mut acts: Vec<String> = Vec::new();
        if !self.added.is_empty() {
            acts.push(format!("{} granted", self.added.len()));
        }
        if !self.renewed.is_empty() {
            acts.push(format!("{} re-granted", self.renewed.len()));
        }
        if !self.removed.is_empty() {
            acts.push(format!(
                "{} removed as the violations went away",
                self.removed.len()
            ));
        }
        if acts.is_empty() {
            out.push_str("; the file is unchanged");
        } else {
            out.push_str(&format!("; the ratchet turned: {}", acts.join(", ")));
        }
        out
    }

    /// Whether anything here is news at the mode in force.
    #[must_use]
    pub fn alarms(&self) -> bool {
        self.mode != Mode::Off && !self.uncovered.is_empty()
    }

    /// The four things a CI step alerts on, as named booleans rather than one blanket flag (`e3.10`).
    ///
    /// A consumer that gets one `true` cannot tell "a cycle is uncovered" from "an allowance lapses on
    /// Thursday", and those are different pages to file. `off` means off: the mode is a decision to stop
    /// asking, so nothing alarms in it — a run that still failed in `off` would be a mode change smuggled in
    /// as a number.
    #[must_use]
    pub fn alarms_json(&self) -> Value {
        let asking = self.mode != Mode::Off;
        json!({
            "cycle": asking && self.uncovered.iter().any(|offence| offence.kind == "cycle"),
            "layer": asking && self.uncovered.iter().any(|offence| offence.kind == "layer"),
            "expiring": asking && !self.expiring.is_empty(),
            "back_due": asking && !self.lapsed.is_empty(),
        })
    }

    /// The exit code the mode asks for, or `None` when the mode does not decide exit codes.
    #[must_use]
    pub fn exit_code(&self) -> Option<ExitCode> {
        if self.mode == Mode::Deny && !self.uncovered.is_empty() {
            return Some(ExitCode::CheckViolations);
        }
        None
    }

    /// The machine-readable form: the whole verdict, the file's after, and the refusals.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "contract": CONTRACT,
            "source": self.source,
            "baselined": self.baselined,
            "mode": self.mode.as_str(),
            "summary": self.summary(),
            "alarms": self.alarms_json(),
            "exit": self.exit_code().map(|code| code.as_i32()),
            "allowances": self.after.iter().map(|row| row_json(row, self.now)).collect::<Vec<Value>>(),
            "added": self.added.iter().map(|row| row_json(row, self.now)).collect::<Vec<Value>>(),
            "renewed": self.renewed.iter().map(|row| row_json(row, self.now)).collect::<Vec<Value>>(),
            "removed": self.removed.iter().map(Removal::to_json).collect::<Vec<Value>>(),
            "expiring": self.expiring.iter().map(|row| row_json(row, self.now)).collect::<Vec<Value>>(),
            "back_due": self.lapsed.iter().map(|row| row_json(row, self.now)).collect::<Vec<Value>>(),
            "uncovered": self.uncovered.iter().map(Offence::to_json).collect::<Vec<Value>>(),
            "untouched": self.untouched.iter().map(|row| row_json(row, self.now)).collect::<Vec<Value>>(),
            "notices": self.notices,
            "refused": self.refused,
            "changed": self.changed(),
            "days_before_warning": EXPIRY_WARNING_DAYS,
        })
    }
}

/// One row, as the file and every surface agree to print it: who owns it, until when, who granted it and
/// why — in that order, which is the order a reviewer asks the questions in.
fn row_line(row: &Allowance) -> String {
    let mut out = format!(
        "{} {} · owner {} · {}",
        row.kind,
        row.key,
        if row.owner.is_empty() {
            "nobody"
        } else {
            &row.owner
        },
        match row.expiry_day() {
            Some(day) => format!("expires {day}"),
            None => "no expiry".to_string(),
        }
    );
    if let Some(day) = row.granted_day() {
        out.push_str(&format!(
            " · granted {day} by {}",
            if row.granted_by.is_empty() {
                "unknown"
            } else {
                &row.granted_by
            }
        ));
    }
    if !row.reason.is_empty() {
        out.push_str(&format!(" · reason: {}", row.reason));
    }
    out
}

/// One row, machine-readable, with the two derived facts a consumer would otherwise recompute badly.
fn row_json(row: &Allowance, now: Timestamp) -> Value {
    json!({
        "kind": row.kind,
        "key": row.key,
        "owner": row.owner,
        "reason": row.reason,
        "expires": row.expiry_day(),
        // Spelled out on every row rather than only on the permanent ones: a consumer that has to guess what
        // an absent key means has a second contract nobody wrote down, and "this allowance never expires" is
        // exactly the fact a reviewer should never have to infer.
        "expires_never": row.expires.is_none(),
        "days_left": row.days_left(now),
        "expiring_soon": row.expires_soon(now),
        "granted": row.granted_day(),
        "granted_by": row.granted_by,
        "live": row.is_live(now),
    })
}

/// The day an instant falls at the end of, spelled as the file spelled it.
fn day_before(instant: Timestamp) -> String {
    let (year, month, day) =
        Timestamp::from_unix_micros(instant.as_unix_micros() - MICROS_PER_DAY).civil();
    format!("{year:04}-{month:02}-{day:02}")
}

/// One row that is about to lapse, in the one wording every surface that prints it uses.
#[must_use]
fn expiring_line(row: &Allowance, now: Timestamp) -> String {
    let days = row.days_left(now).map_or_else(
        || "no expiry".to_string(),
        |days| format!("{days} day(s) left"),
    );
    format!(
        "{}/{} — allowance expires {} ({}), owner {}, granted for: {}",
        row.kind,
        row.key,
        row.expiry_day().unwrap_or_else(|| "never".to_string()),
        days,
        if row.owner.is_empty() {
            "nobody"
        } else {
            &row.owner
        },
        row.reason
    )
}

/// One row whose day has gone by, in the same one wording.
#[must_use]
fn lapsed_line(row: &Allowance, _now: Timestamp) -> String {
    format!(
        "{}/{} — allowance expired {} and the violation may well still be there; owner {}, reason given: \
         {}",
        row.kind,
        row.key,
        row.expiry_day()
            .unwrap_or_else(|| "on its date".to_string()),
        if row.owner.is_empty() {
            "nobody"
        } else {
            &row.owner
        },
        row.reason
    )
}

/// The ledger read on its own, with no graph to say what is still owed (`e3.10`).
///
/// [`Plan`] needs a graph; the TUI's architecture map and the exported artefact do not have one and must
/// still answer the only question that matters before a date: *is anything about to lapse?* A surface that
/// had to shell out to `fathom ratchet` and parse its prose to find out would be a surface with a second,
/// worse ratchet. This reads the file and the clock and nothing else, and therefore never claims a violation
/// is gone or still there — only a plan, which has the graph, may say that.
#[derive(Debug, Clone)]
pub struct Ledger {
    /// Where it was read from, as the config named it.
    pub source: String,
    /// Whether a file was found. `false` is an answer, not an error.
    pub read: bool,
    /// How many rows are live at `now`.
    pub live: usize,
    /// How many rows the file holds at all.
    pub rows: usize,
    /// Live rows inside the warning window.
    pub expiring: Vec<Allowance>,
    /// Rows whose day has gone by.
    pub lapsed: Vec<Allowance>,
    /// The instant the reading is dated to.
    pub now: Timestamp,
    /// What the reader refused or found odd.
    pub notices: Vec<String>,
}

impl Default for Ledger {
    /// An empty, unread ledger at the epoch: the shape a surface prints when it has no file to read.
    fn default() -> Self {
        Self {
            source: String::new(),
            read: false,
            live: 0,
            rows: 0,
            expiring: Vec::new(),
            lapsed: Vec::new(),
            now: Timestamp::UNIX_EPOCH,
            notices: Vec::new(),
        }
    }
}

impl Ledger {
    /// Read a ledger someone has already parsed.
    #[must_use]
    pub fn of(ratchet: &Ratchet, now: Timestamp) -> Self {
        let rows = ratchet.allowances();
        Ledger {
            source: ratchet
                .source
                .clone()
                .unwrap_or_else(|| crate::cycles::debt::DEFAULT_BASELINE.to_string()),
            read: ratchet.loaded,
            live: rows.iter().filter(|row| row.is_live(now)).count(),
            rows: rows.len(),
            expiring: rows
                .iter()
                .filter(|row| row.is_live(now) && row.expires_soon(now))
                .cloned()
                .collect(),
            lapsed: rows
                .iter()
                .filter(|row| !row.is_live(now))
                .cloned()
                .collect(),
            now,
            notices: ratchet.notices.clone(),
        }
    }

    /// The lines for the rows that are about to lapse.
    #[must_use]
    pub fn expiring_lines(&self) -> Vec<String> {
        self.expiring
            .iter()
            .map(|row| expiring_line(row, self.now))
            .collect()
    }

    /// The lines for the rows whose day has gone by.
    #[must_use]
    pub fn lapsed_lines(&self) -> Vec<String> {
        self.lapsed
            .iter()
            .map(|row| lapsed_line(row, self.now))
            .collect()
    }

    /// Whether there is anything a surface should spend a row on. A missing file is an answer the verb gives
    /// in its own words; a read-only surface stays quiet about it, because every repository without a
    /// baseline would otherwise carry the same line forever.
    #[must_use]
    pub fn speaks(&self) -> bool {
        !self.expiring.is_empty() || !self.lapsed.is_empty()
    }

    /// The one-line state of the ledger, for a strip.
    #[must_use]
    pub fn sentence(&self) -> String {
        if !self.read {
            return format!("no ratchet baseline at {}", self.source);
        }
        let mut out = format!("debt {} allowance(s) live in {}", self.live, self.source);
        if !self.expiring.is_empty() {
            out.push_str(&format!(
                ", {} expiring within {} day(s)",
                self.expiring.len(),
                EXPIRY_WARNING_DAYS
            ));
        }
        if !self.lapsed.is_empty() {
            out.push_str(&format!(", {} lapsed", self.lapsed.len()));
        }
        out
    }

    /// The machine-readable form, the same shape `Plan` uses for its own rows.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "source": self.source,
            "read": self.read,
            "live": self.live,
            "rows": self.rows,
            "expiring": self.expiring.iter().map(|row| row_json(row, self.now)).collect::<Vec<Value>>(),
            "lapsed": self.lapsed.iter().map(|row| row_json(row, self.now)).collect::<Vec<Value>>(),
            "notices": self.notices,
        })
    }
}
