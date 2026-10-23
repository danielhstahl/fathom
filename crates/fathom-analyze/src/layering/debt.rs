//! Only *new* violations alarm: the same ledger, a second kind of row.
//!
//! [`crate::cycles::debt`] states the argument in full and it is not repeated here: a committed
//! baseline with an owner and an expiry on every row, class **M** because it is committed, and a verdict
//! that splits four ways — **debt**, **violations**, **back** (lapsed and still there) and **stale**
//! (gone, offered as a tightening, never applied). What this file adds is the layering half of the same
//! file, `kind = "layer"`:
//!
//! ```toml
//! [[allowance]]
//! kind = "layer"
//! key = "no_upward::crates/store/src/sink.rs->crates/tui/src/panel.rs"
//! owner = "team-storage"
//! expires = "2026-04-30"
//! reason = "the sink moves to l2 with the export rewrite"
//! ```
//!
//! # The key, which is the whole matching contract
//!
//! A layering key names **the rule and the pair**: `rule::src->dst` for an edge-shaped rule,
//! `rule::a|b|c` for a cycle-shaped one, the members sorted and deduplicated exactly as
//! [`crate::cycles::Cycle::key`] does so an author can paste either the witness path or the set. The
//! rule name is part of the identity on purpose: a baselined boundary violation does not forgive the
//! same edge's upward reference, because those are two different decisions and one of them may be the
//! one being fixed.
//!
//! [`normalize_key`] is what makes an authored key and a generated one meet: arrows in either spell
//! (`->`, `→`), the two boundary spells (`<-`, `←`), any amount of space, a member list in any order.
//! What it refuses to do is *guess a rule*: a key with no `::` matches nothing and says so, because a
//! key that matched every violation of a kind would be a baseline that quietly widened itself.
//!
//! # Baseline-relative, not episode-relative
//!
//! Like the cycle ledger: a violation is a violation when no live allowance covers it, whether it
//! appeared in this episode or in 2019. The episode's own delta travels beside it as
//! [`Entry::introduced`], which is the difference between *this turn did it* and *there are violations
//! here* — the first is reviewable and the second is a backlog.

use fathom_core::exit::ExitCode;
use fathom_core::time::Timestamp;

use crate::cycles::debt::{Allowance, Mode, Ratchet};
use crate::layering::finding::Violation;
use crate::layering::report::Delta;

/// The kind field a layering row carries in `.fathom/ratchet.toml`.
pub const KIND: &str = "layer";

/// The matching form of a layering key as an author wrote it.
///
/// Three things vary between the key a person typed and the key this module generates, and none of
/// them may decide whether a violation is forgiven: the arrow (`→`, `->`, `←`), the whitespace around
/// it, and — for a cycle rule — the order and rotation the members were written in, which is the same
/// promise `Debt` makes for the cycle ledger. A key with no `::` is returned trimmed and matches
/// nothing: a bare pair would not say which rule it forgives, and forgiving every rule that names an
/// edge because one of them was written down would be a baseline that widened itself.
#[must_use]
pub fn normalize_key(text: &str) -> String {
    let Some((rule, tail)) = text.split_once("::") else {
        return text.trim().to_string();
    };
    let rule = rule.trim();
    let tail = tail.trim();
    let is_set = tail.contains('|');
    let flat = tail
        .replace(['\u{2192}', '\u{2190}'], "|")
        .replace("->", "|")
        .replace("<-", "|");
    if flat.contains('|') {
        let parts: Vec<String> = flat
            .split('|')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(str::to_string)
            .collect();
        // Exactly two ends is a pair. A pair written the way the rule was written (`target <- the only
        // source`) describes the same edge as `source -> target`, and a violation's key always names
        // the edge in the direction it runs, so the arrow is normalised rather than preserved.
        if parts.len() == 2 && !is_set {
            if tail.contains('\u{2190}') || tail.contains("<-") {
                return format!("{rule}::{}->{}", parts[1], parts[0]);
            }
            return format!("{rule}::{}->{}", parts[0], parts[1]);
        }
        // Anything else names a cycle's member set.
        let mut members = parts;
        members.sort();
        members.dedup();
        return format!("{rule}::{}", members.join("|"));
    }
    format!("{rule}::{}", crate::cycles::debt::normalize_key(tail))
}

/// The generated side of the same contract: a violation's own key, already normal.
#[must_use]
pub fn normalize_violation(violation: &Violation) -> String {
    normalize_key(&violation.key())
}

/// One baselined violation, printed with the two things that make a baseline reviewable.
#[derive(Debug, Clone)]
pub struct Debt {
    /// The violation's key, so a debt and a violation of the same fact can be joined.
    pub key: String,
    /// The rule it forgives.
    pub rule: String,
    /// Who is on the hook.
    pub owner: String,
    /// Why it was forgiven.
    pub reason: String,
    /// The instant the allowance stops covering it.
    pub expires: Option<Timestamp>,
    /// Whole days left, `None` when lapsed or unbounded.
    pub days_left: Option<i64>,
    /// Whether it matched an allowance that has since lapsed — which moves the row from debt to back.
    pub lapsed: bool,
    /// The rule's own body, carried so the printed debt says what is forgiven and not merely where.
    pub body: String,
}

impl Debt {
    /// The last day the allowance is live, spelled as its author spelled it.
    #[must_use]
    pub fn expiry_day(&self) -> Option<String> {
        self.expires.map(|expires| {
            let (year, month, day) = Timestamp::from_unix_micros(
                expires.as_unix_micros() - fathom_core::time::MICROS_PER_DAY,
            )
            .civil();
            format!("{year:04}-{month:02}-{day:02}")
        })
    }

    /// The one line a pane prints.
    #[must_use]
    pub fn sentence(&self) -> String {
        let expiry = match self.expiry_day() {
            Some(day) if !self.lapsed => match self.days_left {
                Some(days) => format!("expires {day} ({} day(s) left)", days),
                None => format!("expires {day}"),
            },
            Some(day) => format!("expired {day}"),
            None => "no expiry".to_string(),
        };
        format!(
            "{} — owner {}, {}, {}",
            self.key,
            if self.owner.is_empty() {
                "nobody"
            } else {
                &self.owner
            },
            expiry,
            self.body
        )
    }

    /// The machine-readable form.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::json;
        json!({
            "key": self.key,
            "rule": self.rule,
            "owner": self.owner,
            "reason": self.reason,
            "expires": self.expiry_day(),
            "days_left": self.days_left,
            "lapsed": self.lapsed,
            "body": self.body,
        })
    }
}

/// One violation that is news, with the reason it is news.
#[derive(Debug, Clone)]
pub struct Entry {
    /// The violation, verbatim: rule, edge, `file:line`, why, fix.
    pub violation: Violation,
    /// Why this is news rather than debt — no allowance, or one that lapsed, with its date.
    pub why: String,
    /// Whether *this episode* introduced it, as opposed to it having always been unbudgeted.
    pub introduced: bool,
}

impl Entry {
    /// The one line a pane prints.
    #[must_use]
    pub fn sentence(&self) -> String {
        format!(
            "{}{} — {}",
            self.violation.sentence(),
            if self.introduced {
                ""
            } else {
                " (pre-existing)"
            },
            self.why
        )
    }

    /// The machine-readable form: the whole violation, plus the verdict's two fields.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let mut out = self.violation.to_json();
        if let Some(map) = out.as_object_mut() {
            map.insert("news_because".to_string(), serde_json::json!(self.why));
            map.insert("introduced".to_string(), serde_json::json!(self.introduced));
        }
        out
    }
}

/// The verdict a surface reads: what is forgiven, what is news, and what the file still claims.
#[derive(Debug, Clone, Default)]
pub struct Verdict {
    /// The mode that decided how loudly the news is said.
    pub mode: Mode,
    /// Whether a baseline file was read at all. `false` is stated, never treated as an empty ledger.
    pub baselined: bool,
    /// Baselined and live: never a violation.
    pub debt: Vec<Debt>,
    /// No live allowance.
    pub violations: Vec<Entry>,
    /// The allowance lapsed and the violation stayed.
    pub back: Vec<Debt>,
    /// The allowance outlived the violation: a tightening the file could adopt, never applied here.
    pub stale: Vec<Allowance>,
    /// Allowances expiring within the warning window, so a pane can ask before they lapse.
    pub expiring: Vec<Allowance>,
    /// What the ledger reader refused, carried through so one report prints every caveat.
    pub notices: Vec<String>,
}

impl Verdict {
    /// Put every current violation into one of the boxes.
    ///
    /// `all` is one revision's full assessment ([`crate::layering::engine::evaluate`]), `delta` is the
    /// episode's — the only source of *this turn did it*. A violation that is both new and baselined is
    /// debt: the whole point of the ledger is that a baselined fact is a different box, not a dimmer
    /// setting on the same one.
    ///
    /// `mode` is `[layering]`'s own, not the ratchet file's: the ratchet has a `mode` key too, for the
    /// cycle ledger, and two subsystems' volumes are not the same dial.
    #[must_use]
    #[allow(clippy::too_many_lines)] // one pass, four boxes, each with its own caveat to say
    pub fn assess(
        all: &[Violation],
        delta: &Delta,
        ratchet: &Ratchet,
        now: Timestamp,
        mode: Mode,
    ) -> Self {
        let mut verdict = Verdict {
            mode,
            baselined: ratchet.loaded,
            notices: ratchet.notices.clone(),
            ..Verdict::default()
        };
        for violation in all {
            let pattern = normalize_violation(violation);
            if let Some(allowance) = ratchet.covering_key(KIND, &pattern, now) {
                verdict.debt.push(Debt {
                    key: violation.key(),
                    rule: violation.rule.clone(),
                    owner: allowance.owner.clone(),
                    reason: allowance.reason.clone(),
                    expires: allowance.expires,
                    days_left: allowance.days_left(now),
                    lapsed: false,
                    body: violation.body.clone(),
                });
                if allowance.expires_soon(now) {
                    verdict.expiring.push(allowance.clone());
                }
                continue;
            }
            let lapsed = ratchet.lapsed_key(KIND, &pattern, now);
            let why = match lapsed {
                Some(allowance) => match allowance.expires {
                    Some(expires) => format!(
                        "allowance `{}` expired {expires}, owned by {}",
                        allowance.key,
                        if allowance.owner.is_empty() {
                            "nobody"
                        } else {
                            &allowance.owner
                        }
                    ),
                    None => format!("allowance `{}` no longer covers it", allowance.key),
                },
                None => format!(
                    "no {} allowance in {} covers `{pattern}`",
                    KIND,
                    ratchet.source.as_deref().unwrap_or("the baseline")
                ),
            };
            if let Some(allowance) = lapsed {
                verdict.back.push(Debt {
                    key: violation.key(),
                    rule: violation.rule.clone(),
                    owner: allowance.owner.clone(),
                    reason: allowance.reason.clone(),
                    expires: allowance.expires,
                    days_left: None,
                    lapsed: true,
                    body: violation.body.clone(),
                });
            }
            verdict.violations.push(Entry {
                violation: violation.clone(),
                why,
                introduced: delta.introduced.contains(&violation.key()),
            });
        }
        for allowance in ratchet.allowances().iter().filter(|row| row.is_layering()) {
            if !all
                .iter()
                .any(|violation| normalize_violation(violation) == allowance.pattern)
            {
                verdict.stale.push(allowance.clone());
            }
        }
        verdict.stale.sort_by(|a, b| a.pattern.cmp(&b.pattern));
        verdict.debt.sort_by(|a, b| a.key.cmp(&b.key));
        verdict.back.sort_by(|a, b| a.key.cmp(&b.key));
        verdict.violations.sort_by(|a, b| {
            b.introduced
                .cmp(&a.introduced)
                .then_with(|| a.violation.key().cmp(&b.violation.key()))
        });
        verdict
    }

    /// How many violations are debt — the number a scorecard must not also count as news.
    #[must_use]
    pub fn baselined_count(&self) -> usize {
        self.debt.len()
    }

    /// How many of the news this episode caused.
    #[must_use]
    pub fn introduced_count(&self) -> usize {
        self.violations
            .iter()
            .filter(|entry| entry.introduced)
            .count()
    }

    /// Whether anything here is news at the configured mode.
    #[must_use]
    pub fn alarms(&self) -> bool {
        self.mode != Mode::Off && !self.violations.is_empty()
    }

    /// The exit code the mode asks for, or `None` when the mode does not decide exit codes.
    #[must_use]
    pub fn exit_code(&self) -> Option<ExitCode> {
        if self.mode == Mode::Deny && !self.violations.is_empty() {
            return Some(ExitCode::CheckViolations);
        }
        None
    }

    /// The paragraph a pane prints.
    #[must_use]
    pub fn sentence(&self) -> String {
        let mut out = format!(
            "{} violation(s) new to the baseline ({} introduced this episode), {} baselined as debt",
            self.violations.len(),
            self.introduced_count(),
            self.baselined_count()
        );
        if !self.back.is_empty() {
            out.push_str(&format!(", {} back due", self.back.len()));
        }
        if !self.stale.is_empty() {
            out.push_str(&format!(
                ", {} allowance(s) could be removed as the violations went away",
                self.stale.len()
            ));
        }
        if !self.expiring.is_empty() {
            out.push_str(&format!(", {} expiring soon", self.expiring.len()));
        }
        if !self.baselined {
            out.push_str(" — no baseline was found, so every violation counts as new");
        }
        out
    }

    /// The machine-readable form.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::{Value, json};
        json!({
            "mode": self.mode.as_str(),
            "baselined": self.baselined,
            "violations": self.violations.iter().map(Entry::to_json).collect::<Vec<Value>>(),
            "debt": self.debt.iter().map(Debt::to_json).collect::<Vec<Value>>(),
            "back": self.back.iter().map(Debt::to_json).collect::<Vec<Value>>(),
            "stale": self.stale.iter().map(|a| json!({
                "key": a.key, "owner": a.owner, "reason": a.reason,
            })).collect::<Vec<Value>>(),
            "expiring": self.expiring.iter().map(|a| json!({
                "key": a.key, "owner": a.owner, "expires": a.expiry_day(),
            })).collect::<Vec<Value>>(),
            "alarms": self.alarms(),
            "exit": self.exit_code().map(|code| code.as_i32()),
        })
    }
}
