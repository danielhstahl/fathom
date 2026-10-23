//! The cycle-debt ledger: what the repository already owed, and what this episode put on the tab.
//!
//! The ticket's sentence is *"existing cycles baselined with owner and expiry so only new cycles
//! alarm"*, and the acceptance criterion is sharper than that: a baselined cycle is **shown as debt
//! with its expiry and never appears as a new violation**. That is the whole product argument for this
//! file. A repository with pre-existing cycles is the normal case; a tool that reports every one of them
//! on every episode is turned off in the second week, because it makes the same noise about a fact from
//! 2019 that it makes about the cycle introduced five minutes ago.
//!
//! # The file, which is the interface
//!
//! The authoritative copy is a committed TOML file at [`DEFAULT_BASELINE`] (config: `ratchet.baseline`),
//! class **M** in ADR-004 §12's sense — must-survive *because it is committed*, so a `git clean` cannot
//! delete the record of what was already allowed. The shape is the one `docs/concepts/ratchet.md`
//! publishes:
//!
//! ```toml
//! [[allowance]]
//! kind = "cycle"
//! key = "billing → invoicing → billing"
//! owner = "team-payments"
//! expires = "2026-03-31"
//! reason = "split lands with the ledger rewrite; see the plan item"
//! ```
//!
//! This module **reads** it. Writing it — the verb, the `--update-baseline` flag, the reason that has to
//! be typed — is `e3.10` (`reco-5ht`), and what it needs from here is [`Allowance::for_cycle`] and
//! [`Ratchet::render`], which are the two halves of "emit a baseline that is a diff a human can review".
//! The one thing this module refuses on `e3.10`'s behalf is a matching rule that a refactor can dodge by
//! accident: see [`normalize_key`].
//!
//! # Matching, and why it is a member set
//!
//! An allowance key may be written as a path (`a → b → a`, `a -> b -> a`) or as a set (`a|b`). Both
//! normalise to the sorted, deduplicated member list, which is exactly [`super::cycle::Cycle::key`], so
//! an allowance covers *the cycle between these modules*, whichever way the arrows run and whichever
//! member the writer started from. That is a deliberate loss of precision: a key that matched only one
//! traversal order would silently stop matching when somebody inverted a dependency, and a silently
//! unmatched allowance is a baselined cycle reappearing as a violation, which is the failure mode this
//! file exists to prevent.
//!
//! # Expiry
//!
//! An allowance whose `expires` date has passed is not a violation on the date itself: it is live
//! **throughout** its expiry day and expired from the first instant after it, so a reviewer who set
//! `2026-03-31` gets all of 31 March. An expired allowance whose cycle is still there is reported as
//! back (`Verdict::back`) with the date it lapsed, and an allowance whose cycle has *gone* is reported
//! as [`Verdict::stale`] — the auto-tightening half of "the ratchet only turns one way", surfaced as a
//! suggestion rather than performed silently, because deleting a line that documents somebody's decision
//! is a review act and not a background job.
//!
//! # No clock, as everywhere else
//!
//! `fathom_core::time`'s rule again: expiry is decided against a `now` the caller supplies, so a test can
//! ask what happens the day before an allowance lapses and the day after.

use std::path::Path;

use fathom_core::exit::ExitCode;
use fathom_core::time::{MICROS_PER_DAY, Timestamp, days_from_civil};
use toml_edit::Item as TomlItem;

/// Where the baseline lives when `fathom.toml` says nothing. `ratchet.baseline`'s default, restated here
/// because the config key and the code default have to be one number (ADR-004 §5.1).
pub const DEFAULT_BASELINE: &str = ".fathom/ratchet.toml";

/// The default `ratchet.mode`: a new violation is a finding, not a failing command.
pub const DEFAULT_MODE: Mode = Mode::Warn;

/// How soon before expiry an allowance starts asking to be looked at. The number is `e3.10`'s to change;
/// thirty days is one review cycle's notice, and the warning is a field on a debt rather than a decision
/// this module makes.
pub const EXPIRY_WARNING_DAYS: i64 = 30;

/// What a new violation does, which is `ratchet.mode` with the three spellings the key spec declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Report the ledger and alarm about nothing. A legitimate choice for a repository that is deciding
    /// whether it wants the rule at all.
    Off,
    /// A new violation is a finding. The default: a ratchet that blocks a day-one review teaches nothing
    /// about the review.
    #[default]
    Warn,
    /// A new violation fails `fathom check` ([`ExitCode::CheckViolations`]).
    Deny,
}

impl Mode {
    /// The stored spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Mode::Off => "off",
            Mode::Warn => "warn",
            Mode::Deny => "deny",
        }
    }

    /// Parse the stored spelling, `None` for anything else, so a typo is a notice and a default rather
    /// than a silent change of policy.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "off" => Some(Mode::Off),
            "warn" => Some(Mode::Warn),
            "deny" => Some(Mode::Deny),
            _ => None,
        }
    }
}

/// One allowance, as written in the baseline file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allowance {
    /// What kind of violation this forgives. `cycle` is the only one this module acts on; the others are
    /// the same file's business (`layer`, `api`, `dep`, `hotspot` — ADR-004's `ratchet_entry` kind list).
    pub kind: String,
    /// The key exactly as the author wrote it, because the file is the record and a diff should show it.
    pub key: String,
    /// The normalised form used for matching: [`normalize_key`] of `key`.
    pub pattern: String,
    /// Who is on the hook. Required by the file format, and by the argument that an allowance nobody owns
    /// is an allowance nobody removes.
    pub owner: String,
    /// Why the allowance exists. Also required: loosening is a written act.
    pub reason: String,
    /// The instant *after* the last day the allowance is live, or `None` for a row with no expiry — which
    /// is accepted with a notice rather than refused, because refusing it would push the author to write
    /// a date one day further out.
    pub expires: Option<Timestamp>,
    /// When it was granted, when the file says.
    pub granted: Option<Timestamp>,
    /// The line it was read from, for the notice that names it.
    pub line: usize,
}

/// The calendar day an expiry instant ends: the last day it is live, printed as the ledger printed it.
fn day_of(expires: Timestamp) -> String {
    let (year, month, day) = Timestamp::from_unix_micros(expires.as_unix_micros() - 1).civil();
    format!("{year:04}-{month:02}-{day:02}")
}

impl Allowance {
    /// Whether this row is a cycle allowance.
    #[must_use]
    pub fn is_cycle(&self) -> bool {
        self.kind == "cycle"
    }

    /// Whether this row forgives a layering violation ([`crate::layering`]).
    ///
    /// One file, four kinds, one reader: the kind field is what keeps two engines' debts from
    /// cancelling each other out, and it is why [`Ratchet`] keeps every row it can read instead of only
    /// its own.
    #[must_use]
    pub fn is_layering(&self) -> bool {
        self.kind == "layer"
    }

    /// Whether the allowance still covers anything at `now`.
    #[must_use]
    pub fn is_live(&self, now: Timestamp) -> bool {
        self.expires.is_none_or(|expiry| now < expiry)
    }

    /// Whole days left on the allowance, `None` when it has no expiry or has already lapsed.
    #[must_use]
    pub fn days_left(&self, now: Timestamp) -> Option<i64> {
        let expiry = self.expires?;
        if now >= expiry {
            return None;
        }
        Some(
            ((expiry.as_unix_micros() - 1) - now.as_unix_micros())
                .div_euclid(MICROS_PER_DAY)
                .max(0),
        )
    }

    /// The last whole day the allowance covers, spelled the way the ledger spelled it.
    ///
    /// The stored instant is the first instant *after* the day the author wrote (see [`Allowance`]), so
    /// the day a reader recognises is the one before the instant: `expires = "2026-03-31"` is debt
    /// through the 31st and a violation on the 1st. Printing the instant prints the 1st, which reads as
    /// a date one day later than the one in the file, and a debt whose own due date the owner cannot
    /// recognise is a debt nobody argues about.
    #[must_use]
    pub fn expiry_day(&self) -> Option<String> {
        self.expires.map(day_of)
    }

    /// Whether it is live and about to stop being live.
    #[must_use]
    pub fn expires_soon(&self, now: Timestamp) -> bool {
        self.days_left(now)
            .is_some_and(|days| days <= EXPIRY_WARNING_DAYS)
    }

    /// The row an episode's cycle would need to be baselined, for `e3.10`'s writer.
    ///
    /// The key is the cycle's own canonical key, so what gets written is what will match next quarter,
    /// and the reason is required rather than defaulted: an allowance whose reason is `""` is an
    /// allowance that will still be there in four years with nobody remembering.
    #[must_use]
    pub fn for_cycle(
        cycle: &super::cycle::Cycle,
        owner: &str,
        expires: Option<Timestamp>,
        reason: &str,
    ) -> Self {
        Self {
            kind: "cycle".to_string(),
            key: cycle.display(),
            pattern: cycle.key(),
            owner: owner.to_string(),
            reason: reason.to_string(),
            expires,
            granted: None,
            line: 0,
        }
    }

    /// The row that would baseline one layering violation, for `e3.10`'s writer.
    ///
    /// `key` is [`crate::layering::Violation::key`] — the rule name and the pair or member set — because
    /// a baseline written from a violation has to match the same violation after the next commit.
    #[must_use]
    pub fn for_layer(key: &str, owner: &str, expires: Option<Timestamp>, reason: &str) -> Self {
        Self {
            kind: "layer".to_string(),
            key: key.to_string(),
            pattern: pattern_for("layer", key),
            owner: owner.to_string(),
            reason: reason.to_string(),
            expires,
            granted: None,
            line: 0,
        }
    }

    /// The one line that says what it forgives and when it stops.
    #[must_use]
    pub fn sentence(&self, now: Timestamp) -> String {
        let expiry = match (self.expires, self.expiry_day()) {
            (None, _) => "no expiry".to_string(),
            (_, Some(day)) if self.is_live(now) => match self.days_left(now) {
                Some(days) => format!("expires {day} ({} day(s) left)", days),
                None => format!("expires {day}"),
            },
            (_, Some(day)) => format!("expired {day}"),
            (Some(expiry), None) => format!("expires {expiry}"),
        };
        format!("{} — owner {}, {}", self.key, self.owner, expiry)
    }

    /// The committed spelling of this row.
    #[must_use]
    pub fn to_toml(&self) -> String {
        let mut out = String::from("[[allowance]]\n");
        out.push_str(&format!("kind = \"{}\"\n", self.kind));
        out.push_str(&format!("key = \"{}\"\n", escape(&self.key)));
        out.push_str(&format!("owner = \"{}\"\n", escape(&self.owner)));
        if let Some(expires) = self.expires {
            // The instant stored is the first one *after* the live day, so the date written back is the
            // day before it: a round trip through `parse_date` has to print what the author wrote.
            let (year, month, day) =
                Timestamp::from_unix_micros(expires.as_unix_micros() - MICROS_PER_DAY).civil();
            out.push_str(&format!("expires = \"{year:04}-{month:02}-{day:02}\"\n"));
        }
        if let Some(granted) = self.granted {
            out.push_str(&format!("granted = \"{granted}\"\n"));
        }
        out.push_str(&format!("reason = \"{}\"\n", escape(&self.reason)));
        out
    }
}

/// The ledger itself: the allowances in force, the mode, and what was refused on the way in.
#[derive(Debug, Clone)]
pub struct Ratchet {
    /// What a new violation does.
    pub mode: Mode,
    /// Where this was read from, for the sentence that has to name it.
    pub source: Option<String>,
    allowances: Vec<Allowance>,
    /// What the reader refused or found odd, in the order it found it. A surface prints these.
    pub notices: Vec<String>,
    /// Rows of a kind this module does not act on (`layer`, `api`, `dep`, `hotspot`).
    pub other_kinds: usize,
    /// Rows it could not read at all.
    pub refused: usize,
    /// Whether a baseline file was actually found. `false` is not an error: a repository that has never
    /// run the ratchet has no baseline, and every cycle it has is then new — which is the honest reading
    /// and the one that makes the first `fathom ratchet --update-baseline` worth doing.
    pub loaded: bool,
}

impl Default for Ratchet {
    fn default() -> Self {
        Self::empty()
    }
}

impl Ratchet {
    /// No baseline, default mode: every cycle is new. This is the state of a repository on its first run.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            mode: DEFAULT_MODE,
            source: None,
            allowances: Vec::new(),
            notices: Vec::new(),
            other_kinds: 0,
            refused: 0,
            loaded: false,
        }
    }

    /// Read the two `[ratchet]` keys out of a `fathom.toml` document.
    ///
    /// `ratchet.` is this analyzer's declared config input (ADR-004 §5.1), so reading it is the ticket's
    /// job and not the CLI's: a caller that forgets to read `[ratchet]` would silently run in `warn` when
    /// the repository asked for `deny`.
    #[must_use]
    pub fn config(text: &str) -> (Mode, String, Vec<String>) {
        let mut mode = DEFAULT_MODE;
        let mut baseline = DEFAULT_BASELINE.to_string();
        let mut notices = Vec::new();
        let Ok(document) = text.parse::<toml_edit::DocumentMut>() else {
            notices.push(
                "fathom.toml does not parse, so the ratchet ran at its built-in defaults: mode \
                 `warn`, baseline `.` + `ratchet.toml`"
                    .to_string(),
            );
            return (mode, baseline, notices);
        };
        let Some(table) = document.get("ratchet").and_then(TomlItem::as_table) else {
            return (mode, baseline, notices);
        };
        if let Some(item) = table.get("mode").and_then(TomlItem::as_value) {
            match item.as_str().and_then(Mode::parse) {
                Some(parsed) => mode = parsed,
                None => notices.push(format!(
                    "ratchet.mode is {:?}, which is not `off`, `warn` or `deny`; kept `{}`",
                    item.as_str().unwrap_or_default(),
                    mode.as_str()
                )),
            }
        }
        if let Some(item) = table.get("baseline").and_then(TomlItem::as_value) {
            if let Some(path) = item.as_str() {
                if path.is_empty() {
                    notices.push(
                        "ratchet.baseline is empty, which would make the ledger unreadable; kept \
                         the default"
                            .to_string(),
                    );
                } else {
                    baseline = path.to_string();
                }
            } else {
                notices.push("ratchet.baseline is not a path string; kept the default".to_string());
            }
        }
        (mode, baseline, notices)
    }

    /// Read the baseline document at `path`, resolving `ratchet.baseline` against `root`.
    ///
    /// A missing file is a `Ratchet` with `loaded: false` and a notice, never an error: the first run on
    /// a real repository has no baseline, and the answer it prints — "these N cycles are all new, here is
    /// the baseline you could write" — is the onboarding, not a failure.
    #[must_use]
    pub fn read(root: &Path, config_text: &str) -> (Self, Vec<String>) {
        let (mode, baseline, mut notices) = Self::config(config_text);
        let path = root.join(&baseline);
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let mut ratchet = Self::from_text(&text, &baseline);
                ratchet.mode = mode;
                notices.extend(std::mem::take(&mut ratchet.notices));
                (ratchet, notices)
            }
            Err(_) => {
                notices.push(format!(
                    "no ratchet baseline at {} — every cycle in this revision counts as new. Write one \
                     with the ratchet verb when you want the pre-existing ones treated as debt.",
                    path.display()
                ));
                let mut ratchet = Self::empty();
                ratchet.mode = mode;
                ratchet.notices = notices.clone();
                (ratchet, notices)
            }
        }
    }

    /// Parse a baseline document.
    #[must_use]
    pub fn from_text(text: &str, source: &str) -> Self {
        let mut ratchet = Self {
            source: Some(source.to_string()),
            loaded: true,
            ..Self::empty()
        };
        let Ok(document) = text.parse::<toml_edit::DocumentMut>() else {
            ratchet.notices.push(format!(
                "{source} does not parse as TOML, so no allowance was read"
            ));
            ratchet.loaded = false;
            return ratchet;
        };
        let Some(items) = document
            .get("allowance")
            .and_then(TomlItem::as_array_of_tables)
        else {
            ratchet.notices.push(format!(
                "{source} holds no [[allowance]] rows, so nothing is baselined"
            ));
            return ratchet;
        };
        for (index, entry) in items.iter().enumerate() {
            let line = entry
                .decor()
                .prefix()
                .and_then(|prefix| prefix.as_str())
                .map_or(0, |prefix| prefix.matches('\n').count() + 1);
            let get = |key: &str| {
                entry
                    .get(key)
                    .and_then(TomlItem::as_value)
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            };
            let kind = get("kind").unwrap_or_default();
            let key = get("key").unwrap_or_default();
            let owner = get("owner").unwrap_or_default();
            let reason = get("reason").unwrap_or_default();
            if kind.is_empty() || key.is_empty() {
                ratchet.refused += 1;
                ratchet.notices.push(format!(
                    "{source}:{line} [[allowance]] #{index} names no kind or no key, so it forgives \
                     nothing and was skipped"
                ));
                continue;
            }
            if kind != "cycle" {
                // One file, four kinds. A `layer` row belongs to `layering::debt` and an `api`, `dep`
                // or `hotspot` row to whoever claims them next; they are counted, kept and matched by
                // their own kind. Dropping them here would make one engine's reader lose another's rows
                // on a round trip through `render`, which is the same mistake as a baseline that
                // quietly rewrites itself.
                ratchet.other_kinds += 1;
            }
            if owner.is_empty() {
                ratchet.notices.push(format!(
                    "{source}:{line} allowance for `{key}` names no owner; kept it, but an \
                     allowance nobody owns is an allowance nobody removes"
                ));
            }
            if reason.is_empty() {
                ratchet.notices.push(format!(
                    "{source}:{line} allowance for `{key}` gives no reason"
                ));
            }
            let expires = read_date(entry.get("expires").and_then(TomlItem::as_value));
            if entry.get("expires").is_some() && expires.is_none() {
                ratchet.notices.push(format!(
                    "{source}:{line} allowance for `{key}` has an expiry that is not a `YYYY-MM-DD` \
                     date, so it never expires — say so on purpose or fix the date"
                ));
            } else if expires.is_none() {
                ratchet.notices.push(format!(
                    "{source}:{line} allowance for `{key}` has no expiry"
                ));
            }
            let pattern = pattern_for(&kind, &key);
            ratchet.allowances.push(Allowance {
                kind,
                pattern,
                key,
                owner,
                reason,
                expires,
                granted: read_date(entry.get("granted").and_then(TomlItem::as_value)),
                line,
            });
        }
        ratchet
    }

    /// The rows, in file order.
    #[must_use]
    pub fn allowances(&self) -> &[Allowance] {
        &self.allowances
    }

    /// How many cycle allowances the ledger holds.
    #[must_use]
    pub fn cycle_allowances(&self) -> usize {
        self.allowances.iter().filter(|a| a.is_cycle()).count()
    }

    /// How many layering allowances the ledger holds.
    #[must_use]
    pub fn layering_allowances(&self) -> usize {
        self.allowances.iter().filter(|a| a.is_layering()).count()
    }

    /// The live allowance of `kind` whose key matches `pattern`.
    #[must_use]
    pub fn covering_key(&self, kind: &str, pattern: &str, now: Timestamp) -> Option<&Allowance> {
        self.allowances
            .iter()
            .find(|allowance| allowance.kind == kind && allowance.pattern == pattern)
            .filter(|allowance| allowance.is_live(now))
    }

    /// The allowance of `kind` that matches `pattern` but has lapsed by `now`.
    #[must_use]
    pub fn lapsed_key(&self, kind: &str, pattern: &str, now: Timestamp) -> Option<&Allowance> {
        self.allowances
            .iter()
            .find(|allowance| allowance.kind == kind && allowance.pattern == pattern)
            .filter(|allowance| !allowance.is_live(now))
    }

    /// The live layering allowance that covers a violation key, if any.
    #[must_use]
    pub fn covering_layer(&self, pattern: &str, now: Timestamp) -> Option<&Allowance> {
        self.covering_key("layer", pattern, now)
    }

    /// The layering allowance that matches a violation key but has lapsed by `now`.
    #[must_use]
    pub fn lapsed_layer(&self, pattern: &str, now: Timestamp) -> Option<&Allowance> {
        self.lapsed_key("layer", pattern, now)
    }

    /// The live cycle allowance that covers `cycle`, if any.
    #[must_use]
    pub fn covering(&self, cycle: &super::cycle::Cycle, now: Timestamp) -> Option<&Allowance> {
        let key = cycle.key();
        self.allowances
            .iter()
            .find(|allowance| allowance.is_cycle() && allowance.pattern == key)
            .filter(|allowance| allowance.is_live(now))
    }

    /// The cycle allowance that matches `cycle` but has lapsed by `now`, if any — which is how a
    /// violation gets to say *when* it stopped being forgiven rather than merely that it is new.
    #[must_use]
    pub fn lapsed(&self, cycle: &super::cycle::Cycle, now: Timestamp) -> Option<&Allowance> {
        let key = cycle.key();
        self.allowances
            .iter()
            .find(|allowance| allowance.is_cycle() && allowance.pattern == key)
            .filter(|allowance| !allowance.is_live(now))
    }

    /// The baseline document these rows would write, header included.
    ///
    /// Sorted by key, so the same facts written twice produce the same file and a re-run that changed
    /// nothing is an empty diff. The header is part of the contract: the file is reviewed by humans, and
    /// a human should not have to know this module to read it.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::from(
            "# Fathom ratchet baseline: violations that already existed, each with an owner and an \
             expiry.\n# Only tightening is automatic. Adding or widening a row is a deliberate edit, \
             and the diff is the review.\n",
        );
        let mut rows = self.allowances.clone();
        rows.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.pattern.cmp(&b.pattern)));
        for row in rows {
            out.push_str(&row.to_toml());
        }
        out
    }
}

/// The verdict a surface prints: what is debt, what is new, and what the file still claims.
#[derive(Debug, Clone, Default)]
pub struct Verdict {
    /// The mode that decided how loudly the new ones are said.
    pub mode: Mode,
    /// Baselined cycles, with their owner and expiry. **Never** a violation, by acceptance criterion.
    pub debt: Vec<Debt>,
    /// Cycles with no live allowance. These are the ones that may alarm.
    pub violations: Vec<Violation>,
    /// Allowances that lapsed while their cycle stayed: debt that came back due.
    pub back: Vec<Debt>,
    /// Cycle allowances whose cycle is gone. The auto-tightening candidates — offered, never applied
    /// here.
    pub stale: Vec<Allowance>,
    /// Allowances about to lapse, whether or not their cycle is still there.
    pub expiring: Vec<Allowance>,
    /// Whether the baseline file was found at all.
    pub baselined: bool,
    /// The notices the reader raised, including the analyzer's own.
    pub notices: Vec<String>,
}

/// A cycle that is owed rather than new.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Debt {
    /// The cycle.
    pub cycle: super::cycle::Cycle,
    /// Who owns the allowance.
    pub owner: String,
    /// When it stops being forgiven.
    pub expires: Option<Timestamp>,
    /// Whole days left, `None` when it has no expiry or has lapsed.
    pub days_left: Option<i64>,
    /// Why it was forgiven.
    pub reason: String,
    /// Whether this debt is back because the allowance lapsed rather than never having been forgiven.
    pub lapsed: bool,
}

impl Debt {
    /// The day the allowance was written to expire through, if it has one. See
    /// [`Allowance::expiry_day`] — the pane and the ledger must show the same date.
    #[must_use]
    pub fn expiry_day(&self) -> Option<String> {
        self.expires.map(day_of)
    }

    /// The line for the pane: the debt, who owns it, and its date.
    #[must_use]
    pub fn sentence(&self) -> String {
        let expiry = match (self.expires, self.expiry_day()) {
            (None, _) => "no expiry".to_string(),
            (Some(_), Some(day)) => match self.days_left {
                Some(days) => format!("expires {day}, {days} day(s) left"),
                None => format!("expired {day}"),
            },
            (Some(expires), None) => format!("expires {expires}"),
        };
        format!(
            "{} — debt owned by {}, {}",
            self.cycle.display(),
            if self.owner.is_empty() {
                "(nobody)"
            } else {
                &self.owner
            },
            expiry
        )
    }

    /// The machine-readable form.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "cycle": self.cycle.to_json(),
            "owner": self.owner,
            "expires": self.expires.map(|t| t.to_string()),
            "days_left": self.days_left,
            "reason": self.reason,
            "lapsed": self.lapsed,
        })
    }
}

/// A cycle that is news.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// The cycle.
    pub cycle: super::cycle::Cycle,
    /// Why it is news, in the sentence a reviewer reads.
    pub why: String,
    /// Whether this episode introduced it, as opposed to it being old and simply unbudgeted. The
    /// distinction is the difference between "the agent did this" and "nobody ever wrote it down".
    pub introduced: bool,
}

impl Violation {
    /// The line for the pane.
    #[must_use]
    pub fn sentence(&self) -> String {
        format!(
            "{} — {}{}",
            self.cycle.display(),
            self.why,
            if self.introduced {
                ", introduced by this episode"
            } else {
                ""
            }
        )
    }

    /// The machine-readable form.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "cycle": self.cycle.to_json(),
            "why": self.why,
            "introduced": self.introduced,
        })
    }
}

impl Verdict {
    /// Put every current cycle into one of the two boxes, and read the file back against the graph.
    ///
    /// The classification is **baseline-relative**, not episode-relative: a cycle is new when no live
    /// allowance covers it, whether it appeared in this episode or in 2019. The episode delta is carried
    /// along as `introduced`, which is what lets a surface say *this turn did it* rather than *there are
    /// cycles here*.
    #[must_use]
    pub fn assess(
        cycles: &[super::cycle::Cycle],
        delta: &super::cycle::CycleDelta,
        ratchet: &Ratchet,
        now: Timestamp,
    ) -> Self {
        let mut verdict = Verdict {
            mode: ratchet.mode,
            baselined: ratchet.loaded,
            notices: ratchet.notices.clone(),
            ..Verdict::default()
        };
        let introduced: Vec<String> = delta
            .introduced
            .iter()
            .map(super::cycle::Cycle::key)
            .collect();
        for cycle in cycles {
            if let Some(allowance) = ratchet.covering(cycle, now) {
                verdict.debt.push(Debt {
                    cycle: cycle.clone(),
                    owner: allowance.owner.clone(),
                    expires: allowance.expires,
                    days_left: allowance.days_left(now),
                    reason: allowance.reason.clone(),
                    lapsed: false,
                });
                if allowance.expires_soon(now) {
                    verdict.expiring.push(allowance.clone());
                }
                continue;
            }
            let lapsed = ratchet.lapsed(cycle, now);
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
                    "no allowance in {} for this cycle",
                    ratchet.source.as_deref().unwrap_or("the baseline")
                ),
            };
            if lapsed.is_some() {
                verdict.back.push(Debt {
                    cycle: cycle.clone(),
                    owner: lapsed.map(|a| a.owner.clone()).unwrap_or_default(),
                    expires: lapsed.and_then(|a| a.expires),
                    days_left: None,
                    reason: lapsed.map(|a| a.reason.clone()).unwrap_or_default(),
                    lapsed: true,
                });
            }
            verdict.violations.push(Violation {
                cycle: cycle.clone(),
                why,
                introduced: introduced.contains(&cycle.key()),
            });
        }
        // An allowance whose cycle is gone is the ratchet tightening itself: reported as a suggestion,
        // because deleting the record of a decision is a review act.
        for allowance in ratchet
            .allowances
            .iter()
            .filter(|allowance| allowance.is_cycle())
        {
            if !cycles.iter().any(|cycle| cycle.key() == allowance.pattern) {
                verdict.stale.push(allowance.clone());
            }
        }
        verdict.stale.sort_by(|a, b| a.pattern.cmp(&b.pattern));
        verdict.debt.sort_by_key(|debt| debt.cycle.key());
        verdict.violations.sort_by(|a, b| {
            b.introduced
                .cmp(&a.introduced)
                .then_with(|| a.cycle.key().cmp(&b.cycle.key()))
        });
        verdict.back.sort_by_key(|debt| debt.cycle.key());
        verdict
    }

    /// Whether anything here is news at all: `off` suppresses, otherwise a violation is news.
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

    /// The count of cycles that are debt, which is the number the scorecard's acyclicity dimension must
    /// not double-count as new violations.
    #[must_use]
    pub fn baselined_count(&self) -> usize {
        self.debt.len()
    }

    /// The paragraph a surface prints, in the order a reviewer acts on it.
    #[must_use]
    pub fn sentence(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !self.violations.is_empty() {
            parts.push(format!(
                "{} new cycle violation(s){}",
                self.violations.len(),
                match self.mode {
                    Mode::Off => ", suppressed by ratchet.mode = off",
                    Mode::Warn => ", warning",
                    Mode::Deny => ", and ratchet.mode = deny fails the check",
                }
            ));
        }
        if !self.debt.is_empty() {
            parts.push(format!("{} baselined as debt", self.debt.len()));
        }
        if !self.back.is_empty() {
            parts.push(format!(
                "{} came back due on an expired allowance",
                self.back.len()
            ));
        }
        if !self.expiring.is_empty() {
            parts.push(format!(
                "{} allowance(s) expiring soon",
                self.expiring.len()
            ));
        }
        if !self.stale.is_empty() {
            parts.push(format!(
                "{} allowance(s) no longer match a cycle and could be removed",
                self.stale.len()
            ));
        }
        if parts.is_empty() {
            return if self.baselined {
                String::from("no cycles in this revision, and nothing owed against the baseline")
            } else {
                String::from("no cycles in this revision")
            };
        }
        parts.join("; ")
    }

    /// The machine-readable form.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "mode": self.mode.as_str(),
            "baselined": self.baselined,
            "alarms": self.alarms(),
            "exit": self.exit_code().map(|code| code.as_i32()),
            "violations": self.violations.iter().map(Violation::to_json).collect::<Vec<_>>(),
            "debt": self.debt.iter().map(Debt::to_json).collect::<Vec<_>>(),
            "back": self.back.iter().map(Debt::to_json).collect::<Vec<_>>(),
            "stale": self.stale.iter().map(|a| serde_json::json!({
                "key": a.key, "owner": a.owner, "expires": a.expires.map(|t| t.to_string()),
            })).collect::<Vec<_>>(),
            "notices": self.notices,
        })
    }
}

/// The matching form of a key, which is a per-`kind` contract.
///
/// A cycle key names a *set* of members, so sorting and deduplicating its tokens is the whole
/// normalisation. A layering key names a rule and a *directed* edge (`rule::src->dst`), where sorting
/// the tokens would fold `a->b` into `b->a` and bury the rule name in the pile — so that grammar
/// belongs to `crate::layering::debt` and this dispatches to it. One ledger, two kinds, each matched
/// by the rules its own keys are made of.
#[must_use]
pub(crate) fn pattern_for(kind: &str, key: &str) -> String {
    if kind == crate::layering::debt::KIND {
        return crate::layering::debt::normalize_key(key);
    }
    normalize_key(key)
}

/// The one matching rule: sorted, deduplicated, `|`-joined members.
///
/// Accepts the two spellings an author reaches for — an arrow-joined path (`billing → invoicing →
/// billing`, or `->`, in either direction) and a separated set (`billing, invoicing`) — and normalises
/// both to the cycle key the graph produces. The closing node of a path spelling is a repeat and the
/// dedup drops it, which is what makes `a → b → a` and `a|b` the same allowance rather than two.
///
/// This is the *cycle* grammar. A layering key carries a rule name and a direction and is normalised by
/// [`crate::layering::debt::normalize_key`]; `pattern_for` (crate-private) picks the right one per row.
#[must_use]
pub fn normalize_key(text: &str) -> String {
    // One separator first, so `->` (two characters) and `→` (one) split the same way, then the ordinary
    // list separators. `-` is deliberately not a separator on its own: `my-module.go` is one name.
    let text = text.replace(['→', '⇒'], "\u{0}").replace("->", "\u{0}");
    let mut parts: Vec<String> = Vec::new();
    for chunk in text.split(['\u{0}', '|', ',', ';', ' ', '\t', '\n']) {
        let part = chunk.trim().trim_matches('"').trim();
        if !part.is_empty() {
            parts.push(part.to_string());
        }
    }
    parts.sort();
    parts.dedup();
    parts.join("|")
}

/// Read a date out of a TOML value: a `YYYY-MM-DD` string, a bare TOML date, or nothing.
///
/// The published example quotes the date and TOML also has a date type, so both are accepted rather
/// than one of them being a silent parse failure that turns an expiring allowance into a permanent one.
fn read_date(value: Option<&toml_edit::Value>) -> Option<Timestamp> {
    let value = value?;
    if let Some(text) = value.as_str() {
        return parse_date(text);
    }
    if let Some(date) = value.as_datetime() {
        let text = date.to_string();
        return parse_date(&text);
    }
    None
}

/// `YYYY-MM-DD` to the first instant *after* that day, so an allowance is live for all of its expiry day.
#[must_use]
pub fn parse_date(text: &str) -> Option<Timestamp> {
    let text = text.trim();
    let bytes = text.as_bytes();
    if bytes.len() < 10
        || !bytes[..10]
            .iter()
            .enumerate()
            .all(|(index, byte)| match index {
                4 | 7 => *byte == b'-',
                _ => byte.is_ascii_digit(),
            })
    {
        return None;
    }
    let year: i64 = text[0..4].parse().ok()?;
    let month: u32 = text[5..7].parse().ok()?;
    let day: u32 = text[8..10].parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some(Timestamp::from_unix_micros((days + 1) * MICROS_PER_DAY))
}

/// Escape the three things that can break a quoted TOML string, which is all this module writes.
fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', " ")
}
