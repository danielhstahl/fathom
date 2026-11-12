//! ADR-007 §9's store half (`e8.3`, `reco-l9bz.3`): the per-file complexity columns, the episode roll-up
//! that is a join, and the coverage `fathom doctor` prints.
//!
//! Everything here is arithmetic over rows this crate already holds, which is the point: ADR-007 §5.2 makes
//! the episode figure "both are arithmetic over stored rows", and §9.2 makes the roll-up "a join, not a
//! column". Nothing in this module computes a complexity figure — the counting is `fathom-analyze`'s
//! (`structure::complexity`, ADR-007 §3–§4), and lint `M02` keeps it there. What lives here is the *shape*
//! a counted figure takes in the store, the shape a figure that was never counted takes, and the reads that
//! let a surface fill a delta document or a doctor line without walking files of its own.
//!
//! # The rule this module is written to make unrepresentable
//!
//! **A figure for a file that was never parsed.** ADR-007 §7.4 stores four shapes, and the two that are not
//! figures are distinguished from each other: a side with no figure *and* a reason is "this file could not
//! be read", and a side with no figure and *no* reason is "this build never looked". The first is repaired
//! by fixing the file or the build; the second is repaired by running `fathom index`, and only one of them
//! is checkable afterwards. So the store keeps them apart ([`Side::Unmeasurable`] and [`Side::NotMeasured`],
//! built from the row by [`Side::stored`]), and a reader that asks what to print is handed
//! [`Side::printed`] — which prints `?` for both, and a `?` that never carries a bare shrug, because
//! [`Side::reason`] substitutes `column_not_filled` for the second case on the way out. A [`SideWrite`]
//! cannot even be built with a missing figure and no reason: the CHECK constraints ADR-007 §9.1 names are
//! the backstop for rows written by anything else, and the type is the front stop for rows written here.
//!
//! # Invalidation, stated once, where the migration is
//!
//! ADR-007 §9.3's answer — **complexity evicts zero derived rows** — is written out on the column
//! definitions in [`crate::episode::ADDED_COLUMNS`] and asserted by
//! `spine`'s `the_complexity_columns_land_on_an_older_store_and_evict_nothing`. Short form: key field 9 is
//! [`crate::cache::ROW_SCHEMA_VERSION`], which these columns do not move, because field 9 versions the
//! derived-row tables (`cache`, `blob`, `blob_ref`) and these six live on `git_commit_file`. A store
//! written by an older fathom keeps every cached analysis it has; what it loses is nothing, and what it
//! lacks is the figures, which arrive as `NotMeasured` and are filled by the next run over that revision.
//!
//! # Who fills the columns
//!
//! Not this crate, and not on open. The pass that counts needs file *contents*, and deciding when it may
//! read them is `e8.4`'s budget (`scan_budget`, `too_large`, `contents_not_kept` — ADR-007 §7.1's codes,
//! already declared in `structure::UnavailableReason` for exactly that reason). [`Store::record_complexity`]
//! is the write half, and it is deliberately honest about progress: it returns how many rows it landed and
//! how many named no file row, so a fill in progress prints as a fraction ([`Store::complexity_coverage`])
//! rather than as a total over a third of the store. Until a writer is wired up, every column is empty and
//! every figure prints `?` with `column_not_filled`, which is the state ADR-007 §9.6 prefers to the
//! alternative.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{OptionalExtension as _, params};

use crate::error::Error;
use crate::spine::{Store, corrupt};

/// The code a reader must print for a side this build never measured (ADR-007 §7.1, §7.4 row 4).
///
/// The store writes `''` into the reason column for that state and never this string: a reason the pass
/// recorded and a reason a reader invented are two answers, and only the first can be traced to the thing
/// that happened. The substitution happens on the way *out*, in [`Side::reason`], which is the only place
/// a `?` gets its sentence.
///
/// The spelling belongs to `fathom_analyze::structure::UnavailableReason::ColumnNotFilled`; the store
/// cannot name that type (the dependency runs the other way) so the string is repeated here, and
/// `fathom-analyze`'s `tests/complexity.rs` asserts the two agree, which is what makes it a duplication
/// with a keeper rather than a fork.
pub const COLUMN_NOT_FILLED: &str = "column_not_filled";

/// The code for a comparison whose two sides were counted under different basis tokens (ADR-007 §9.5).
///
/// Raised by the roll-up rather than stored: no single row can know that the row it is being added to was
/// counted under other rules. §9.5's rule is that such a pair is `partial`, names both tokens and
/// contributes nothing to a sum, and [`Store::episode_complexity`] implements exactly that.
pub const MIXED_BASIS: &str = "mixed_basis";

/// The code a **spent run budget** leaves behind (ADR-007 §7.1's `scan_budget`, produced by `e8.4`'s two
/// per-run ceilings as well as by `e3.1b`'s Ruby scan caps).
///
/// The store names it because [`Store::complexity_owed`] treats it as the one refusal that re-opens a row:
/// every other §7.1 code is a fact about the bytes, and a budget is a fact about the run. The spelling
/// belongs to `fathom_analyze::structure::UnavailableReason::ScanBudget`, exactly as
/// [`COLUMN_NOT_FILLED`]'s does, and the same test in `fathom-analyze`'s `tests/complexity.rs` keeps them
/// equal.
pub const BUDGET_REASON: &str = "scan_budget";

/// The code a **per-file ceiling** leaves behind (ADR-007 §7.1's `too_large`, produced by
/// `extract.complexity.max_file_bytes` as well as by `e3.1`'s `extract.max_file_bytes`).
///
/// Named here for the same reason [`BUDGET_REASON`] is: raising the ceiling has to be able to change the
/// answer, so a size refusal re-opens a row in [`Store::complexity_owed`] instead of closing it. The §7.1
/// table already says as much in its "Filled by" column — "raise it, then `fathom index`" — and this is
/// that sentence made executable.
pub const SIZE_REASON: &str = "too_large";

/// One side of one file row's figure: what the row says about the base side or the head side.
///
/// The three variants are ADR-007 §7.4's rows 1–3 as read from one side. Row 4 — the side this build never
/// looked at — is [`Side::NotMeasured`], and it is a variant rather than an `Option` precisely because the
/// difference between "0 with a reason" and "nothing" is the difference between a measurement and a
/// refusal, which is the whole content of that ADR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Side {
    /// A figure. `reason` is `''` for a plain measurement and one of §7.1's two *measured zero* codes
    /// (`absent_revision`, `no_bodies`) when the zero is a fact rather than an omission — a path that did
    /// not exist on this side, or a file that parsed and declares nothing callable. Both feed a sum;
    /// neither is an unknown wearing a number.
    Measured {
        /// The figure: Σ V(G) over the path's functions on this side.
        figure: i64,
        /// The stable reason string, `''` when there is nothing to explain.
        reason: String,
    },
    /// No figure, and the row names why (`too_large`, `binary`, `parse_failed`, `scan_budget`, …).
    Unmeasurable {
        /// §7.1's code, as the pass wrote it.
        reason: String,
    },
    /// No figure and no reason: this row predates the columns, or no run has measured it since
    /// (ADR-007 §9.6). The fix is `fathom index`, which is why it is not the same variant as
    /// [`Side::Unmeasurable`].
    NotMeasured,
}

impl Side {
    /// Build a side from what the row holds.
    #[must_use]
    pub fn stored(figure: Option<i64>, reason: &str) -> Self {
        match figure {
            Some(figure) => Side::Measured {
                figure,
                reason: reason.to_owned(),
            },
            // A `NULL` figure with a reason is row 3; with no reason it is row 4. Both are `?`; only one of
            // them says `fathom index` will fix it, and `fathom doctor` counts them separately.
            None if reason.is_empty() => Side::NotMeasured,
            None => Side::Unmeasurable {
                reason: reason.to_owned(),
            },
        }
    }

    /// The figure, when there is one. A `0` that is a fact comes back as `Some(0)`; an unknown does not
    /// come back as `Some(0)` under any circumstances.
    #[must_use]
    pub fn figure(&self) -> Option<i64> {
        match self {
            Side::Measured { figure, .. } => Some(*figure),
            Side::Unmeasurable { .. } | Side::NotMeasured => None,
        }
    }
    /// Did this side get counted?
    #[must_use]
    pub fn measured(&self) -> bool {
        matches!(self, Side::Measured { .. })
    }

    /// The reason code to print beside the figure, never empty.
    ///
    /// ADR-007 §7: `?` never stands alone. A measured side with nothing to explain answers `''`, which a
    /// surface prints as nothing; an unmeasured side answers its own code; an unmeasured *and* unlooked-at
    /// side answers [`COLUMN_NOT_FILLED`], because "this index has not filled the column" is the true
    /// sentence about it and `fathom index` is the true remedy.
    #[must_use]
    pub fn reason(&self) -> &str {
        match self {
            Side::Measured { reason, .. } | Side::Unmeasurable { reason } => reason,
            Side::NotMeasured => COLUMN_NOT_FILLED,
        }
    }

    /// The cell: the number, or `?`.
    ///
    /// This is the minimum that keeps a surface from lying — an unfilled column cannot print a figure — and
    /// deliberately nothing more. The signed and floored forms ADR-007 §10.1 fixes for the rail (`+7`,
    /// `−3`, `±0`, `~41 (3 unmeasured)`) are a surface's formatting of a figure and a coverage, and
    /// ADR-002 §5.3 leaves formatting to the surface that has the width to render it.
    #[must_use]
    pub fn printed(&self) -> String {
        match self.figure() {
            Some(figure) => figure.to_string(),
            None => "?".to_owned(),
        }
    }
}

/// What one `git_commit_file` row says about complexity: the two sides, the head side's worst unit, and
/// the basis token that says which rules counted them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileComplexity {
    /// The figure at the row's base side.
    pub before: Side,
    /// The figure at the row's head side.
    pub after: Side,
    /// The largest single figure on the head side (ADR-007 §5.1: "what is the worst thing in here").
    /// `None` whenever [`FileComplexity::after`] has no figure — a maximum over nothing is not `0`.
    pub max: Option<i64>,
    /// The basis token the figures were counted under (`mccabe-1`), or `''` for a row nothing has filled.
    pub basis: String,
}

impl FileComplexity {
    /// Has any build that knows about these columns written this row?
    ///
    /// The basis token is the marker, because it is the thing a figure cannot travel without: a row with a
    /// basis and no figure names why it has none, and a row without a basis makes no claim at all.
    #[must_use]
    pub fn filled(&self) -> bool {
        !self.basis.is_empty()
    }
}

/// One side as a caller *writes* it — ADR-007 §9.1's CHECK, expressed as a type.
///
/// `?`-without-a-reason is not constructible here, which is the point of the enum: the constraint the ADR
/// puts in the schema would otherwise be the only thing standing between a careless caller and a column of
/// unexplained `?`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideWrite<'a> {
    /// A figure, plus the reason when the figure is a zero that is a fact (pass `''` for an ordinary one).
    Measured {
        /// The figure.
        figure: i64,
        /// §7.1's code, or `''`.
        reason: &'a str,
    },
    /// No figure, and the reason that says why. An empty `reason` here is what the CHECK refuses, and
    /// refusing it at the call site is cheaper than discovering it in a constraint message.
    Unmeasurable {
        /// §7.1's code.
        reason: &'a str,
    },
}

impl SideWrite<'_> {
    /// The figure to store, if any.
    #[must_use]
    const fn figure(&self) -> Option<i64> {
        match self {
            SideWrite::Measured { figure, .. } => Some(*figure),
            SideWrite::Unmeasurable { .. } => None,
        }
    }

    /// The reason string to store.
    #[must_use]
    fn reason(&self) -> &str {
        match self {
            SideWrite::Measured { reason, .. } | SideWrite::Unmeasurable { reason } => reason,
        }
    }
}

/// One row of a fill: "the pass measured this path in this commit, and here is what it found".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComplexityWrite<'a> {
    /// The commit whose file row is being filled.
    pub commit: &'a str,
    /// sha256 of the path's raw bytes, hex — the primary-key half [`crate::spine::FileRow::path_hash`].
    pub path_hash: &'a str,
    /// The basis token the figures were counted under (`structure::complexity::BASIS`).
    ///
    /// A figure arriving with an empty token is refused by the schema rather than normalised here: a figure
    /// that cannot name the rules that counted it cannot be compared with anything, and the `CHECK` on
    /// `complexity_before` is where that is decided — the same single enforcement point ADR-007 §9.1 asks
    /// for, not a second copy of it. A row with no figures at all may arrive with no basis, which is how a
    /// store says "nothing has measured this path yet".
    pub basis: &'a str,
    /// The base side.
    pub before: SideWrite<'a>,
    /// The head side.
    pub after: SideWrite<'a>,
    /// The head side's largest single figure; `None` when there is no head figure.
    pub max: Option<i64>,
}

/// What [`Store::record_complexity`] landed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ComplexityFill {
    /// File rows updated.
    pub written: usize,
    /// Writes that named no such file row.
    ///
    /// Reported, not fatal: a fill racing `fathom index --prune` can legitimately find the row gone, and a
    /// run that died with half its rows gone is a run whose own progress line has to say so. Counting them
    /// is also what makes a mis-keyed fill visible instead of silent — a `written` of 0 over 4 000 attempts
    /// is a bug, and it looks exactly like an empty repository otherwise.
    pub missing: usize,
}

impl ComplexityFill {
    /// The observable-progress form ADR-007 §9.6 asks of a fill in flight: `written` out of `written +
    /// missing`, as a percentage, or `None` when nothing was attempted.
    #[must_use]
    pub fn percent(&self) -> Option<f64> {
        let total = self.written + self.missing;
        if total == 0 {
            return None;
        }
        Some(
            f64::from(u32::try_from(self.written).unwrap_or(u32::MAX)) * 100.0
                / f64::from(u32::try_from(total).unwrap_or(u32::MAX)),
        )
    }
}

/// How complete an episode's complexity figure is — ADR-007 §5.1's three states at episode grain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completeness {
    /// Every path in scope was counted on both sides.
    Measured,
    /// Some were, and the figure is a floor: it is the sum of what was read and it must print with the
    /// count of what was not beside it (§5.1, §5.4).
    Partial,
    /// Nothing could be counted. The figure is `?` with its reason codes, and it is never a `0` (§5.4:
    /// "zero coverage is not a zero delta").
    Unavailable,
}

impl Completeness {
    /// The stable word, matching ADR-007 §5.1's table.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Completeness::Measured => "measured",
            Completeness::Partial => "partial",
            Completeness::Unavailable => "unavailable",
        }
    }
}

/// One path's contribution to an episode's figure, as ADR-007 §9.2 defines it: the base side read from the
/// **earliest** row for that path in the episode and the head side from the **latest**.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathComplexity {
    /// The path after the change, in the store's quoted form.
    pub path: String,
    /// `complexity_before` on the earliest row: the figure at `parent(first_oid)`, ADR-003 **X3**'s base.
    pub base: Side,
    /// `complexity_after` on the latest row: the figure at `last_oid`.
    pub head: Side,
    /// `head − base`, or `None` when the pair cannot be subtracted — one side uncounted, or the two sides
    /// counted under different bases ([`MIXED_BASIS`]).
    pub delta: Option<i64>,
    /// The basis tokens the two sides carry, `''` excluded. Two entries is a mixed pair.
    pub bases: BTreeSet<String>,
}

/// An episode's complexity roll-up: [`Store::episode_complexity`]'s answer for one episode.
///
/// This is a *join*, not a stored column (ADR-007 §9.2 — "a second answer" is what ADR-004 §4.7 exists to
/// keep out), and it is the read `e8.5` fills a delta document from, so that the delta document does not
/// re-walk files that the spine already walked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpisodeComplexity {
    /// Every touched path, in path order.
    pub paths: Vec<PathComplexity>,
    /// Σ over the paths that could be subtracted. `None` means *no* path could be, which prints `?`; a
    /// `Some(0)` means it was measured and made no difference (`±0`).
    pub delta: Option<i64>,
    /// Σ of the head figures over the paths that have one: the absolute at head, over the paths this
    /// episode touched — never a repository figure (ADR-007 §8.1).
    pub absolute: Option<i64>,
    /// The largest head figure over the episode's paths (§5.1: an episode's max is a max, never a sum).
    pub max: Option<i64>,
    /// Paths whose pair could be subtracted.
    pub measured: usize,
    /// Paths that could not, and why each could not, counted into [`EpisodeComplexity::reasons`].
    pub unmeasured: usize,
    /// Why each unmeasured path could not be, by code — including [`COLUMN_NOT_FILLED`] for a row nothing
    /// has filled and [`MIXED_BASIS`] for a pair whose bases disagree.
    pub reasons: BTreeMap<String, usize>,
    /// Every basis token the episode's rows carry. More than one is a `mixed_basis` episode (§9.5).
    pub bases: BTreeSet<String>,
    /// How complete the figure is.
    pub state: Completeness,
}

impl EpisodeComplexity {
    /// Paths touched, which is the denominator of every coverage number here.
    #[must_use]
    pub fn paths_total(&self) -> usize {
        self.paths.len()
    }

    /// `measured / touched` (ADR-007 §5.4), or `None` when there is nothing to divide — `None` and `0 %`
    /// are different claims and the card prints them differently.
    #[must_use]
    pub fn coverage(&self) -> Option<f64> {
        if self.paths.is_empty() {
            return None;
        }
        Some(
            f64::from(u32::try_from(self.measured).unwrap_or(u32::MAX))
                / f64::from(u32::try_from(self.paths.len()).unwrap_or(u32::MAX)),
        )
    }
}

/// How much of the complexity column the index actually carries: ADR-007 §9.6's coverage line, as data.
///
/// A half-filled column is a figure that lies quietly, and the way to stop it lying is to print the
/// fraction rather than the total: the number, the denominator it was taken over, and the reason codes for
/// the rest. The threshold worth warning about is the *caller's* decision (ADR-007 §8.3) and lives with
/// the line that prints it, in `fathom-cli`'s `state`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComplexityCoverage {
    /// Every `git_commit_file` row in the store.
    pub rows: i64,
    /// Rows that could carry a figure: the ones git does not report as binary. A binary row can never have
    /// a figure, so counting it in the denominator would make a fully-measured store look under-filled.
    pub measurable: i64,
    /// Measurable rows some build has measured (`complexity_basis <> ''`), figure or no figure after that.
    pub filled: i64,
    /// Measurable rows whose head side holds a figure at all. Below [`ComplexityCoverage::filled`] wherever
    /// a file was looked at and refused — `language_unknown`, `too_large`, `binary`.
    pub with_figure: i64,
    /// The head-side reason tally, `column_not_filled` included, most frequent first. Empty when every
    /// measurable row has a figure.
    pub reasons: Vec<(String, i64)>,
    /// The basis tokens in use with their row counts, so a store mid-way through a re-count under a new
    /// token says so instead of printing one blended column.
    pub bases: Vec<(String, i64)>,
}

impl ComplexityCoverage {
    /// Measurable rows nothing has filled yet.
    #[must_use]
    pub fn unfilled(&self) -> i64 {
        (self.measurable - self.filled).max(0)
    }

    /// `filled / measurable`, or `None` when the index holds nothing measurable — the same
    /// `None`-not-`0 %` rule every coverage number in this crate follows.
    #[must_use]
    pub fn percent(&self) -> Option<f64> {
        if self.measurable == 0 {
            return None;
        }
        Some(self.filled as f64 * 100.0 / self.measurable as f64)
    }

    /// Whether the column is *half* filled: some rows measured, some not.
    ///
    /// This is the state that can lie, and the reason `fathom doctor` prints a warning line for it rather
    /// than a fact: a wholly empty column prints `?` everywhere and nobody mistakes it for an answer, while
    /// a column filled on two thirds of the rows will print a confident figure beside a `?` and invite a
    /// reader to average them. ADR-007 §9.6 wants partial progress observable *while it runs*, which is
    /// the same statement from the other side.
    #[must_use]
    pub fn half_filled(&self) -> bool {
        self.measurable > 0 && self.filled > 0 && self.filled < self.measurable
    }
}

impl Store {
    /// Which file rows of one commit still owe a complexity figure (`e8.4`).
    ///
    /// Three shapes owe one, and the third is the one a run budget makes unavoidable:
    ///
    /// * `complexity_basis <> basis` covers both "nothing has ever looked at this row" (`''`, ADR-007 §7.4's
    ///   fourth row) and "this build counts under a different token" (§9.5's re-count) — one comparison, so
    ///   long as an unfilled row and a stale row are both rows whose claim this build cannot defend;
    /// * a row with **no figure on either side** whose reason is a ceiling — a spent per-run budget
    ///   ([`BUDGET_REASON`]) or a file over the per-file cap ([`SIZE_REASON`]) — owes one again, because
    ///   such a refusal is a fact about a number somebody set and not about the file: raise the key and the
    ///   next index has to be allowed to answer, which is only true if the refusal did not close the row.
    ///   A `language_unknown`, a `binary` or a `parse_failed` *does* close it — those are properties of the
    ///   bytes, and re-reading them answers the same way, which is why re-opening them would be a warm run
    ///   that never converges.
    ///
    /// The predicate is a `commit_oid` prefix scan of the `PRIMARY KEY`, which is what makes it cheap enough
    /// to ask of every commit of every warm run: a store whose column is filled pays one index probe per
    /// commit and reads nothing else.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn complexity_owed(&self, commit: &str, basis: &str) -> Result<BTreeSet<String>, Error> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT path_hash FROM git_commit_file \
                  WHERE commit_oid = ?1 \
                    AND (complexity_basis <> ?2 \
                       OR complexity_before_reason IN (?3, ?4) \
                       OR complexity_after_reason IN (?3, ?4))",
            )
            .map_err(|err| corrupt("asking which file rows owe a complexity figure", &err))?;
        let rows = statement
            .query_map(params![commit, basis, BUDGET_REASON, SIZE_REASON], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|err| corrupt("asking which file rows owe a complexity figure", &err))?;
        rows.collect::<Result<BTreeSet<String>, _>>()
            .map_err(|err| corrupt("reading which file rows owe a complexity figure", &err))
    }

    /// Write the complexity columns for the named file rows, in one transaction.
    ///
    /// A fill, not an insert: it only ever updates a row the spine already wrote, because a complexity
    /// figure for a file row that does not exist is a claim about a file this index has not seen. Rows that
    /// name no file row are counted in [`ComplexityFill::missing`] rather than failing the batch — a fill
    /// racing a prune is a normal thing a repository does, and the run's own progress line is where it
    /// belongs.
    ///
    /// The CHECK constraints ADR-007 §9.1 names are enforced here by SQLite rather than by a re-statement:
    /// a write that would leave a figure without a basis, or a side without a figure *and* without a reason,
    /// fails and rolls the whole batch back, so a half-written figure cannot be the state a reader finds.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if a statement fails, including a `CHECK` refusal — which is the store saying
    /// it will not hold a figure it cannot account for.
    pub fn record_complexity(
        &mut self,
        rows: &[ComplexityWrite<'_>],
    ) -> Result<ComplexityFill, Error> {
        let mut fill = ComplexityFill::default();
        if rows.is_empty() {
            return Ok(fill);
        }
        let tx = self
            .conn_mut()
            .unchecked_transaction()
            .map_err(|err| corrupt("beginning a complexity fill", &err))?;
        {
            let mut statement = tx
                .prepare(
                    "UPDATE git_commit_file
                        SET complexity_before = ?3, complexity_before_reason = ?4,
                            complexity_after  = ?5, complexity_after_reason  = ?6,
                            complexity_max    = ?7, complexity_basis         = ?8
                      WHERE commit_oid = ?1 AND path_hash = ?2",
                )
                .map_err(|err| corrupt("preparing a complexity fill", &err))?;
            for row in rows {
                let changed = statement
                    .execute(params![
                        row.commit,
                        row.path_hash,
                        row.before.figure(),
                        row.before.reason(),
                        row.after.figure(),
                        row.after.reason(),
                        row.max,
                        row.basis
                    ])
                    .map_err(|err| corrupt("writing a complexity figure", &err))?;
                if changed == 0 {
                    fill.missing += 1;
                } else {
                    fill.written += 1;
                }
            }
        }
        tx.commit()
            .map_err(|err| corrupt("committing a complexity fill", &err))?;
        Ok(fill)
    }

    /// One file row's complexity, by commit and path hash.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn file_complexity(
        &self,
        commit: &str,
        path_hash: &str,
    ) -> Result<Option<FileComplexity>, Error> {
        let row = self
            .connection()
            .query_row(
                "SELECT complexity_before, complexity_before_reason, complexity_after,
                        complexity_after_reason, complexity_max, complexity_basis
                 FROM git_commit_file WHERE commit_oid = ?1 AND path_hash = ?2",
                params![commit, path_hash],
                |row| {
                    Ok((
                        row.get::<_, Option<i64>>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()
            .map_err(|err| corrupt("reading a complexity row", &err))?;
        Ok(row.map(
            |(before, before_reason, after, after_reason, max, basis)| FileComplexity {
                before: Side::stored(before, &before_reason),
                after: Side::stored(after, &after_reason),
                max,
                basis,
            },
        ))
    }

    /// Every file row of one commit with its complexity, ordered as [`Store::read_files`] orders them:
    /// `(new_path, path_hash, complexity)`.
    ///
    /// The read behind a per-file table (the diff tree sorts on the per-file delta with `max` second,
    /// ADR-007 §5.3), and deliberately separate from [`Store::read_files`]: a spine read that also dragged
    /// six complexity columns through would make every caller of the spine's own shape pay for a column
    /// most of them do not print.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the read fails.
    pub fn commit_complexity(
        &self,
        commit: &str,
    ) -> Result<Vec<(String, String, FileComplexity)>, Error> {
        let mut statement = self
            .connection()
            .prepare(
                "SELECT new_path, path_hash, complexity_before, complexity_before_reason,
                        complexity_after, complexity_after_reason, complexity_max, complexity_basis
                 FROM git_commit_file WHERE commit_oid = ?1 ORDER BY new_path, path_hash",
            )
            .map_err(|err| corrupt("reading a commit's complexity rows", &err))?;
        let rows = statement
            .query_map(params![commit], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    FileComplexity {
                        before: Side::stored(
                            row.get::<_, Option<i64>>(2)?,
                            &row.get::<_, String>(3)?,
                        ),
                        after: Side::stored(
                            row.get::<_, Option<i64>>(4)?,
                            &row.get::<_, String>(5)?,
                        ),
                        max: row.get::<_, Option<i64>>(6)?,
                        basis: row.get::<_, String>(7)?,
                    },
                ))
            })
            .map_err(|err| corrupt("reading a commit's complexity rows", &err))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|err| corrupt("reading a commit's complexity rows", &err))
    }

    /// The complexity roll-up for the given episodes, as `episode id → [`EpisodeComplexity`].
    ///
    /// One statement over `episode_member ⋈ git_commit_file` for the whole page of rails, in the shape
    /// [`Store::episode_paths`] and [`Store::episode_files`] already use: a read whose cost grew with the
    /// episodes it describes is a read the first frame cannot afford. It is the same join with two more
    /// columns and an ordering, which is what §9.2 means by "a join, not a column", and it is read beside
    /// the line counts so `e8.5` fills its delta document without walking a file twice.
    ///
    /// The definition, which is ADR-007 §5.2's and not this function's to invent:
    ///
    /// * `base(path)` is `complexity_before` on the **earliest** row for that path in the episode — the row
    ///   of `parent(first_oid)`, ADR-003 **X3**'s base, which is why the ordering is by commit time with
    ///   the member ordinal to break a tie;
    /// * `head(path)` is `complexity_after` on the **latest** row;
    /// * `delta` is Σ over the paths whose two sides are both counted *and share a basis*, and a pair whose
    ///   bases differ contributes nothing and is tallied as [`MIXED_BASIS`] (§9.5);
    /// * `absolute` is Σ of the head figures that exist; `max` is a max over them, never a sum (§5.1).
    ///
    /// Because the definition reads the first and the last row rather than summing the middle, it survives
    /// an episode whose commits are not contiguous in the index — and the telescoping identity a contiguous
    /// episode satisfies (Σ per-commit `after − before` == `head − base`) is asserted on the fixture
    /// (`the_rollup_telescopes_when_the_episode_is_contiguous_and_does_not_pretend_otherwise_when_it_is_not`)
    /// instead of being assumed by the arithmetic.
    ///
    /// An episode with no file rows is *absent* from the map, not present and empty, for the same reason
    /// [`Store::episode_paths`] gives: "the index recorded nothing for it" and "this episode touches
    /// nothing" are different claims.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if any read fails.
    pub fn episode_complexity(
        &self,
        ids: &[String],
    ) -> Result<BTreeMap<String, EpisodeComplexity>, Error> {
        const CHUNK: usize = 500;
        // Per episode, per path: the base side and its basis, the head side and its basis, and whether the
        // row seen so far was the one that carries them (an episode may hold one row, which is both).
        let mut seen: BTreeMap<String, BTreeMap<String, Sides>> = BTreeMap::new();
        for chunk in ids.chunks(CHUNK) {
            let marks = vec!["?"; chunk.len()].join(",");
            let mut statement = self
                .connection()
                .prepare(&format!(
                    "SELECT episode_id, new_path, rn_first, rn_last, figure_before, before_reason, \
                            figure_after, after_reason, basis \
                     FROM ( \
                       SELECT m.episode_id AS episode_id, f.new_path AS new_path, \
                              ROW_NUMBER() OVER (PARTITION BY m.episode_id, f.new_path \
                                                  ORDER BY c.author_utc, m.ordinal) AS rn_first, \
                              ROW_NUMBER() OVER (PARTITION BY m.episode_id, f.new_path \
                                                  ORDER BY c.author_utc DESC, m.ordinal DESC) AS rn_last, \
                              f.complexity_before AS figure_before, \
                              f.complexity_before_reason AS before_reason, \
                              f.complexity_after AS figure_after, \
                              f.complexity_after_reason AS after_reason, \
                              f.complexity_basis AS basis \
                       FROM episode_member m \
                            JOIN git_commit c ON c.oid = m.commit_oid \
                            JOIN git_commit_file f ON f.commit_oid = m.commit_oid \
                       WHERE m.episode_id IN ({marks}) \
                     ) \
                     WHERE rn_first = 1 OR rn_last = 1 \
                     ORDER BY episode_id, new_path"
                ))
                .map_err(|err| corrupt("reading episode complexity", &err))?;
            let rows = statement
                .query_map(rusqlite::params_from_iter(chunk.iter()), |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        Side::stored(row.get::<_, Option<i64>>(4)?, &row.get::<_, String>(5)?),
                        Side::stored(row.get::<_, Option<i64>>(6)?, &row.get::<_, String>(7)?),
                        row.get::<_, String>(8)?,
                    ))
                })
                .map_err(|err| corrupt("reading episode complexity", &err))?;
            for row in rows {
                let (episode, path, first, last, base, head, basis) =
                    row.map_err(|err| corrupt("reading episode complexity", &err))?;
                let entry = seen
                    .entry(episode)
                    .or_default()
                    .entry(path)
                    .or_insert((None, None));
                if first == 1 {
                    entry.0 = Some((base, basis.clone()));
                }
                if last == 1 {
                    entry.1 = Some((head, basis));
                }
            }
        }

        let mut out = BTreeMap::new();
        for (episode, paths) in seen {
            out.insert(episode, roll_up(paths));
        }
        Ok(out)
    }

    /// How much of the complexity column the index carries (ADR-007 §9.6).
    ///
    /// One pass over `git_commit_file`, because this is a status read: `fathom doctor` and `fathom stats`
    /// print it on every invocation and it must not cost the size of the index more than a scan already
    /// does. The denominators are stated on the fields they divide.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if a read fails.
    pub fn complexity_coverage(&self) -> Result<ComplexityCoverage, Error> {
        let mut coverage = ComplexityCoverage::default();
        // `COALESCE` on every total: `SUM` over no rows answers `NULL`, and an index with no file rows has
        // *zero* of each — the store has to answer rather than shrug, because `ComplexityCoverage::percent`
        // distinguishes "nothing measurable" from "nothing measured" and needs its denominator.
        let (rows, measurable, filled, with_figure) = self
            .connection()
            .query_row(
                "SELECT count(*), \
                        COALESCE(SUM(CASE WHEN is_binary = 0 AND change_kind <> 'binary' \
                                          THEN 1 ELSE 0 END), 0), \
                        COALESCE(SUM(CASE WHEN is_binary = 0 AND change_kind <> 'binary' \
                                          AND complexity_basis <> '' THEN 1 ELSE 0 END), 0), \
                        COALESCE(SUM(CASE WHEN is_binary = 0 AND change_kind <> 'binary' \
                                          AND complexity_after IS NOT NULL THEN 1 ELSE 0 END), 0) \
                 FROM git_commit_file",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .map_err(|err| corrupt("counting complexity column coverage", &err))?;
        coverage.rows = rows;
        coverage.measurable = measurable;
        coverage.filled = filled;
        coverage.with_figure = with_figure;

        // The head-side tally. `''` — "there is a figure and nothing to explain" — is not a reason and is
        // left out; an empty basis becomes `column_not_filled`, which is the row-out semantics of
        // [`Side::reason`] applied to a whole index.
        let mut statement = self
            .connection()
            .prepare(
                "SELECT code, count(*) FROM ( \
                   SELECT CASE \
                            WHEN complexity_after IS NOT NULL THEN '' \
                            WHEN complexity_basis <> '' THEN complexity_after_reason \
                            ELSE 'column_not_filled' END AS code \
                   FROM git_commit_file \
                   WHERE is_binary = 0 AND change_kind <> 'binary' \
                 ) WHERE code <> '' GROUP BY code ORDER BY count(*) DESC, code",
            )
            .map_err(|err| corrupt("tallying complexity reason codes", &err))?;
        let reasons = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|err| corrupt("tallying complexity reason codes", &err))?;
        coverage.reasons = reasons
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| corrupt("tallying complexity reason codes", &err))?;

        let mut statement = self
            .connection()
            .prepare(
                "SELECT complexity_basis, count(*) FROM git_commit_file \
                 WHERE complexity_basis <> '' \
                 GROUP BY complexity_basis ORDER BY complexity_basis",
            )
            .map_err(|err| corrupt("reading complexity basis tokens", &err))?;
        let bases = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|err| corrupt("reading complexity basis tokens", &err))?;
        coverage.bases = bases
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| corrupt("reading complexity basis tokens", &err))?;
        Ok(coverage)
    }
}

/// One path's two sides as the episode read found them: `(base, head)`, each the side plus the basis token
/// of the row it came from, each `None` when the episode holds no row on that end for this path.
///
/// Named because both halves of the roll-up carry it, and because four nested anonymous levels in a
/// signature is not a thing a reviewer can hold in their head (clippy's `type_complexity` agrees).
type Sides = (Option<(Side, String)>, Option<(Side, String)>);

/// Fold one episode's per-path first/last rows into its roll-up (ADR-007 §5.1, §5.2, §5.4).
///
/// The state is decided by *pairs that could be subtracted*, not by rows seen: a path whose base is a
/// `too_large` refusal and whose head measured is a path the figure cannot describe, so it is `unmeasured`
/// here and it makes the episode a floor printed with its count beside it (§5.4: coverage travels with the
/// number it describes). Reasons are tallied **once per path**, because every other number on this page
/// counts paths, and a tally that counted sides would make a file unreadable on both sides two missing
/// files.
fn roll_up(paths: BTreeMap<String, Sides>) -> EpisodeComplexity {
    let mut rows = Vec::with_capacity(paths.len());
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut bases: BTreeSet<String> = BTreeSet::new();
    let mut measured = 0_usize;
    let mut unmeasured = 0_usize;
    let mut delta_sum = 0_i64;
    let mut absolute_sum = 0_i64;
    let mut any_head = false;
    let mut max: Option<i64> = None;
    for (path, (base_row, head_row)) in paths {
        let (base, base_basis) = base_row.unwrap_or((Side::NotMeasured, String::new()));
        let (head, head_basis) = head_row.unwrap_or((Side::NotMeasured, String::new()));
        let mut pair: BTreeSet<String> = BTreeSet::new();
        for token in [&base_basis, &head_basis] {
            if !token.is_empty() {
                pair.insert(token.clone());
                bases.insert(token.clone());
            }
        }
        // §9.5: a pair counted under two bases contributes nothing to a sum. It is named, not adjusted.
        let mixed = pair.len() > 1;
        let delta = match (base.figure(), head.figure()) {
            (Some(before), Some(after)) if !mixed => Some(after - before),
            _ => None,
        };
        let mut tallied: BTreeSet<String> = BTreeSet::new();
        if mixed {
            tallied.insert(MIXED_BASIS.to_owned());
        }
        for side in [&base, &head] {
            if !side.measured() {
                tallied.insert(side.reason().to_owned());
            }
        }
        if tallied.is_empty() {
            measured += 1;
            if let Some(d) = delta {
                delta_sum += d;
            }
        } else {
            unmeasured += 1;
            for reason in tallied {
                *reasons.entry(reason).or_insert(0) += 1;
            }
        }
        if let Some(head_figure) = head.figure() {
            absolute_sum += head_figure;
            any_head = true;
            max = Some(max.map_or(head_figure, |current| current.max(head_figure)));
        }
        rows.push(PathComplexity {
            path,
            base,
            head,
            delta,
            bases: pair,
        });
    }
    // The state answers "how much was counted"; `delta` answers "what could be added". They disagree in one
    // case, and it is worth naming: an episode whose only paths are mixed-basis pairs *was* counted — twice,
    // under two rule sets — so §9.5 calls it `partial` rather than `unavailable`, while §9.5's other half
    // ("contributes nothing to a sum") leaves `delta` empty. A surface reading `Partial` with no delta prints
    // the reason list and the two tokens, which is the sentence the ADR wants there.
    let counted = rows
        .iter()
        .any(|row| row.base.figure().is_some() && row.head.figure().is_some());
    EpisodeComplexity {
        paths: rows,
        // `None`, not `0`, when nothing could be subtracted: ADR-007 §5.4 refuses to let an episode that was
        // never measured print as an episode that changed nothing.
        delta: (measured > 0).then_some(delta_sum),
        absolute: any_head.then_some(absolute_sum),
        max,
        measured,
        unmeasured,
        reasons,
        bases,
        state: if measured > 0 && unmeasured == 0 {
            Completeness::Measured
        } else if measured > 0 || counted {
            Completeness::Partial
        } else {
            Completeness::Unavailable
        },
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::spine::{CommitRow, FileRow, RunStats, Trigger};
    use rusqlite::params;

    const NOW: i64 = 1_700_000_000_000;
    const BASIS: &str = "mccabe-1";

    /// One file row, with the shape the spine writes and the complexity columns untouched — which is the
    /// state every row in a store written before `e8.3` is in, and the state this module is built around.
    fn file(path: &str) -> FileRow {
        FileRow {
            path_hash: hash(path),
            new_path: path.to_owned(),
            old_path: None,
            change_kind: "modify".into(),
            mode_old: Some(0o100644),
            mode_new: Some(0o100644),
            is_binary: false,
            n_add: 10,
            n_del: 2,
            hunk_headers: vec!["@@ -1,3 +1,11 @@".into()],
            blob_old: Some("a".repeat(40)),
            blob_new: Some("b".repeat(40)),
            path_class: "source".into(),
            taxonomy_version: 1,
            noise_class: "informative".into(),
        }
    }

    /// A path hash the way the spine keys one: sha256 of the raw bytes, hex. The store never recomputes it,
    /// so a test may use any stable digest-shaped string — this one is reproducible without a hash crate.
    fn hash(path: &str) -> String {
        let mut out = String::new();
        for byte in path.bytes() {
            out.push_str(&format!("{byte:02x}"));
        }
        while out.len() < 64 {
            out.push('0');
        }
        out
    }

    fn commit(oid: &str, at: i64, paths: &[&str]) -> CommitRow {
        CommitRow {
            oid: oid.to_owned(),
            tree_oid: "c".repeat(40),
            parent_oids: vec!["d".repeat(40)],
            author_name: "Ada Lovelace".into(),
            author_email: "ada@example.invalid".into(),
            author_utc: at,
            committer_utc: at,
            committer_name: "Ada Lovelace".into(),
            committer_email: "ada@example.invalid".into(),
            message_head: format!("commit {oid}"),
            message_hash: "e".repeat(64),
            trailers: BTreeMap::new(),
            n_files: paths.len() as i64,
            n_add: 10,
            n_del: 2,
            n_binary: 0,
            oversized: false,
            files: paths.iter().map(|path| file(path)).collect(),
        }
    }

    fn indexed(commits: &[CommitRow]) -> Store {
        let mut store = Store::open_in_memory().expect("store");
        let run = store.begin_run("warm", Trigger::Manual, NOW).expect("run");
        store.write_commits(run, NOW, commits).expect("write");
        store
            .finish_run(run, NOW, &RunStats::default())
            .expect("finish");
        store
    }

    /// A minimal episode over `oids`, in order. The `episode` row is written with raw SQL because the
    /// segmentation that produces one is another ticket's; the roll-up only ever joins through
    /// `episode_member`, and this is the smallest thing that makes that join answer.
    fn episode(store: &mut Store, id: &str, oids: &[&str]) {
        store
            .conn_mut()
            .execute(
                "INSERT INTO episode(id, kind, fingerprint, lineage_ref, lineage_seq, created_utc,
                                     created_source, created_confidence, boundary_rule,
                                     boundary_confidence, author_kind, author_confidence, epoch,
                                     fallback_fingerprint, first_oid, last_oid, n_commits)
                 VALUES (?1,'committed',?2,'refs/heads/main',1,?3,'commit',1.0,'commit',1.0,'agent',1.0,
                         1,?2,?4,?5,?6)",
                params![id, format!("fp-{id}"), NOW, oids[0], oids[oids.len() - 1], oids.len() as i64],
            )
            .expect("episode row");
        for (ordinal, oid) in oids.iter().enumerate() {
            store
                .conn_mut()
                .execute(
                    "INSERT INTO episode_member(episode_id, ordinal, commit_oid) VALUES (?1,?2,?3)",
                    params![id, i64::try_from(ordinal).unwrap_or_default(), *oid],
                )
                .expect("member row");
        }
    }

    /// Fill one commit's rows, all with the same figures for every path.
    fn fill(store: &mut Store, commit_oid: &str, paths: &[&str], before: i64, after: i64) {
        let hashes: Vec<String> = paths.iter().map(|path| hash(path)).collect();
        let rows: Vec<ComplexityWrite<'_>> = hashes
            .iter()
            .map(|path_hash| ComplexityWrite {
                commit: commit_oid,
                path_hash,
                basis: BASIS,
                before: SideWrite::Measured {
                    figure: before,
                    reason: "",
                },
                after: SideWrite::Measured {
                    figure: after,
                    reason: "",
                },
                max: Some(after),
            })
            .collect();
        store.record_complexity(&rows).expect("fill");
    }

    #[test]
    fn a_row_nobody_measured_reads_as_absent_and_prints_a_question_mark() {
        // The acceptance criterion, in its plainest form: a store written by an older fathom — six `NULL`
        // / `''` columns, which is what the probe leaves behind — prints `?` and never a figure, and the `?`
        // arrives with the one code that is true about it.
        let store = indexed(&[commit("a".repeat(40).as_str(), NOW, &["src/a.rs"])]);
        let oid = "a".repeat(40);
        let row = store
            .file_complexity(&oid, &hash("src/a.rs"))
            .expect("read")
            .expect("the row is there");
        assert!(!row.filled(), "no basis token means nothing measured it");
        assert_eq!(row.before, Side::NotMeasured);
        assert_eq!(row.after, Side::NotMeasured);
        assert_eq!(row.max, None, "a maximum over nothing is not 0");
        assert_eq!(row.after.printed(), "?");
        assert_eq!(row.after.reason(), COLUMN_NOT_FILLED);
        assert_eq!(row.after.figure(), None, "an unknown is not a zero");
    }

    #[test]
    fn the_constraints_refuse_a_silent_question_mark_and_an_anonymous_figure() {
        // ADR-007 §9.1's constraints, tested through SQLite rather than through a re-statement of them: a
        // side with no figure must name why, and a figure must name its basis. Both are reached by raw SQL,
        // because the point is what the *schema* refuses, not what this crate's types decline.
        let mut store = indexed(&[commit("a".repeat(40).as_str(), NOW, &["src/a.rs"])]);
        let oid = "a".repeat(40);
        let mut set = |sql: &str| {
            store.conn_mut().execute(
                &format!(
                    "UPDATE git_commit_file SET {sql} WHERE commit_oid = ?1 AND path_hash = ?2"
                ),
                params![oid, hash("src/a.rs")],
            )
        };

        // A figure with a reason of its own and its basis: the ordinary row (§7.4 row 1).
        set("complexity_before = 2, complexity_after = 7, complexity_basis = 'mccabe-1'")
            .expect("an ordinary measured row");
        // A side with neither a figure nor a reason, once a basis says somebody looked: not representable.
        let silent = set("complexity_after = NULL");
        assert!(
            silent.is_err(),
            "`?`-without-a-reason must not be writable: {}",
            silent.err().map(|err| err.to_string()).unwrap_or_default()
        );
        // Returning the row to the state every legacy row is in — no figures, no basis, no reasons — has to
        // be writable, or no store an older fathom wrote could ever open.
        set(
            "complexity_before = NULL, complexity_after = NULL, complexity_before_reason = '', \
             complexity_after_reason = '', complexity_basis = ''",
        )
        .expect("the never-looked state must be representable");
        // And a figure that cannot say which rules counted it is the same class of lie (§9.5).
        let anonymous = set("complexity_after = 9, complexity_basis = ''");
        assert!(
            anonymous.is_err(),
            "a figure without a basis must not be writable: {}",
            anonymous
                .err()
                .map(|err| err.to_string())
                .unwrap_or_default()
        );
    }

    #[test]
    fn a_fill_lands_both_sides_and_names_what_it_could_not_count() {
        let mut store = indexed(&[commit(
            "a".repeat(40).as_str(),
            NOW,
            &["src/a.rs", "src/b.rs"],
        )]);
        let oid = "a".repeat(40);
        let fill = store
            .record_complexity(&[
                ComplexityWrite {
                    commit: &oid,
                    path_hash: &hash("src/a.rs"),
                    basis: BASIS,
                    before: SideWrite::Measured {
                        figure: 12,
                        reason: "",
                    },
                    after: SideWrite::Measured {
                        figure: 19,
                        reason: "",
                    },
                    max: Some(9),
                },
                ComplexityWrite {
                    commit: &oid,
                    path_hash: &hash("src/b.rs"),
                    basis: BASIS,
                    before: SideWrite::Measured {
                        figure: 0,
                        reason: "absent_revision",
                    },
                    after: SideWrite::Unmeasurable {
                        reason: "too_large",
                    },
                    max: None,
                },
            ])
            .expect("fill");
        assert_eq!(fill.written, 2);
        assert_eq!(fill.missing, 0);
        assert_eq!(fill.percent(), Some(100.0));

        let all = store.commit_complexity(&oid).expect("read");
        assert_eq!(all.len(), 2, "the per-commit read comes back in path order");
        assert_eq!(all[0].0, "src/a.rs");
        let a = &all[0].2;
        assert!(a.filled());
        assert_eq!(a.before.printed(), "12");
        assert_eq!(a.after.printed(), "19");
        assert_eq!(a.max, Some(9));
        assert_eq!(
            a.after.reason(),
            "",
            "a plain figure has nothing to explain"
        );
        let b = &all[1].2;
        // An added file's base side is a zero that is a fact (§6.1): it prints `0` and feeds a sum.
        assert_eq!(b.before.printed(), "0");
        assert_eq!(b.before.reason(), "absent_revision");
        assert_eq!(b.after.printed(), "?");
        assert_eq!(b.after.reason(), "too_large");
    }

    #[test]
    fn a_fill_that_names_no_file_row_says_so_instead_of_reading_as_a_clean_run() {
        // A mis-keyed fill and an empty repository look identical otherwise, and only one of them is a bug.
        let mut store = indexed(&[commit("a".repeat(40).as_str(), NOW, &["src/a.rs"])]);
        let fill = store
            .record_complexity(&[ComplexityWrite {
                commit: &"f".repeat(40),
                path_hash: &hash("src/gone.rs"),
                basis: BASIS,
                before: SideWrite::Unmeasurable { reason: "not_utf8" },
                after: SideWrite::Measured {
                    figure: 3,
                    reason: "",
                },
                max: Some(3),
            }])
            .expect("fill");
        assert_eq!((fill.written, fill.missing), (0, 1));
        assert_eq!(fill.percent(), Some(0.0));
    }

    #[test]
    fn the_rollup_reads_base_from_the_first_row_and_head_from_the_last() {
        // ADR-007 §9.2's definition, on the fixture §9.2 asks for: three commits over one path, the figures
        // of each commit's row contiguous, and the episode's base/head therefore the *ends* rather than a
        // sum over the middle.
        let paths = ["src/pricing.rs"];
        let (c1, c2, c3) = ("a".repeat(40), "b".repeat(40), "c".repeat(40));
        let mut store = indexed(&[
            commit(&c1, NOW, &paths),
            commit(&c2, NOW + 1, &paths),
            commit(&c3, NOW + 2, &paths),
        ]);
        episode(&mut store, "ep-1", &[&c1, &c2, &c3]);
        fill(&mut store, &c1, &paths, 10, 14);
        fill(&mut store, &c2, &paths, 14, 20);
        fill(&mut store, &c3, &paths, 20, 23);

        let rollups = store
            .episode_complexity(&["ep-1".to_owned()])
            .expect("roll-up");
        let ep = &rollups["ep-1"];
        assert_eq!(ep.paths_total(), 1);
        assert_eq!(ep.paths[0].base.figure(), Some(10), "parent(first_oid)");
        assert_eq!(ep.paths[0].head.figure(), Some(23), "last_oid");
        assert_eq!(ep.delta, Some(13), "23 - 10");
        assert_eq!(ep.absolute, Some(23));
        assert_eq!(ep.max, Some(23));
        assert_eq!(ep.state, Completeness::Measured);
        assert_eq!(ep.coverage(), Some(1.0));
        assert_eq!(
            ep.bases.len(),
            1,
            "one basis token, so a comparable episode"
        );
        assert!(
            ep.reasons.is_empty(),
            "nothing was refused: {:?}",
            ep.reasons
        );
    }

    #[test]
    fn the_rollup_telescopes_when_the_episode_is_contiguous_and_does_not_pretend_otherwise_when_it_is_not()
     {
        // Σ per-commit (`after − before`) == `head − base`, which is the check ADR-007 §9.2 asks for rather
        // than the definition it forbids: a definition that *summed* the per-commit deltas would silently
        // print the wrong figure the moment an episode's commits were not contiguous in the index — a gapped
        // episode after a prune, a relink that moved a commit, a run that died between commits.
        let paths = ["src/pricing.rs"];
        let (c1, c2, c3) = ("a".repeat(40), "b".repeat(40), "c".repeat(40));
        let mut store = indexed(&[
            commit(&c1, NOW, &paths),
            commit(&c2, NOW + 1, &paths),
            commit(&c3, NOW + 2, &paths),
        ]);
        fill(&mut store, &c1, &paths, 10, 14);
        fill(&mut store, &c2, &paths, 14, 20);
        fill(&mut store, &c3, &paths, 20, 23);

        episode(&mut store, "whole", &[&c1, &c2, &c3]);
        let whole = &store
            .episode_complexity(&["whole".to_owned()])
            .expect("roll-up")["whole"];
        let walked: i64 = store
            .connection()
            .query_row(
                "SELECT SUM(COALESCE(f.complexity_after,0) - COALESCE(f.complexity_before,0)) \
                 FROM episode_member m JOIN git_commit_file f ON f.commit_oid = m.commit_oid \
                 WHERE m.episode_id = 'whole'",
                [],
                |row| row.get(0),
            )
            .expect("walk");
        assert_eq!(
            whole.delta,
            Some(walked),
            "the join and the walk agree on a contiguous episode"
        );
        assert_eq!(walked, 13);

        // Now the same three rows with the middle commit gone from the episode: the ends still answer the
        // question the reviewer is asking, and the sum of what is present does not.
        episode(&mut store, "gapped", &[&c1, &c3]);
        let gapped = &store
            .episode_complexity(&["gapped".to_owned()])
            .expect("roll-up")["gapped"];
        assert_eq!(
            gapped.delta,
            Some(13),
            "base and head, not a sum of the middle"
        );
        assert_ne!(
            gapped.delta,
            Some(7),
            "7 is Σ over the two rows present, and it is the wrong answer to 'what did this episode do'"
        );
    }

    #[test]
    fn a_path_nobody_measured_makes_the_episode_partial_and_says_which_one() {
        let paths = ["src/a.rs", "src/b.rs"];
        let oid = "a".repeat(40);
        let mut store = indexed(&[commit(&oid, NOW, &paths)]);
        episode(&mut store, "ep-1", &[&oid]);
        fill(&mut store, &oid, &["src/a.rs"], 12, 19);
        // `src/b.rs` is left untouched: a store mid-way through its first complexity run, which is the state
        // ADR-007 §9.6 refuses to let print as a total.

        let ep = &store
            .episode_complexity(&["ep-1".to_owned()])
            .expect("roll-up")["ep-1"];
        assert_eq!(ep.paths_total(), 2);
        assert_eq!(ep.measured, 1);
        assert_eq!(ep.unmeasured, 1);
        assert_eq!(ep.delta, Some(7), "the figure is the sum of what was read");
        assert_eq!(ep.state, Completeness::Partial);
        assert_eq!(ep.reasons.get(COLUMN_NOT_FILLED), Some(&1));
        assert_eq!(
            ep.coverage(),
            Some(0.5),
            "coverage is part of the figure, never a footnote (§5.4)"
        );
        assert_eq!(
            ep.paths[1].head.reason(),
            COLUMN_NOT_FILLED,
            "and the unmeasured path names the one remedy that fixes it"
        );
    }

    #[test]
    fn nothing_measured_is_a_question_mark_and_never_a_zero_delta() {
        let oid = "a".repeat(40);
        let mut store = indexed(&[commit(&oid, NOW, &["src/a.rs"])]);
        episode(&mut store, "ep-1", &[&oid]);
        let ep = &store
            .episode_complexity(&["ep-1".to_owned()])
            .expect("roll-up")["ep-1"];
        assert_eq!(
            ep.delta, None,
            "`±0` means measured-and-unchanged, not unmeasured"
        );
        assert_eq!(ep.state, Completeness::Unavailable);
        assert_eq!(ep.absolute, None);
        assert_eq!(ep.reasons.get(COLUMN_NOT_FILLED), Some(&1));
    }

    #[test]
    fn a_pair_counted_under_two_bases_contributes_nothing_and_names_both() {
        // ADR-007 §9.5, on the rolling re-index it predicts: base counted under `mccabe-1`, head under
        // `mccabe-2`. Subtracting them is arithmetic on unlike units, so the pair is named instead.
        let paths = ["src/a.rs"];
        let (c1, c2) = ("a".repeat(40), "b".repeat(40));
        let mut store = indexed(&[commit(&c1, NOW, &paths), commit(&c2, NOW + 1, &paths)]);
        episode(&mut store, "ep-1", &[&c1, &c2]);
        fill(&mut store, &c1, &paths, 10, 14);
        fill(&mut store, &c2, &paths, 15, 23);
        store
            .conn_mut()
            .execute(
                "UPDATE git_commit_file SET complexity_basis = 'mccabe-2' WHERE commit_oid = ?1",
                params![c2],
            )
            .expect("re-count under the new basis");

        let ep = &store
            .episode_complexity(&["ep-1".to_owned()])
            .expect("roll-up")["ep-1"];
        assert_eq!(ep.delta, None, "the pair adds to nothing");
        assert_eq!(ep.bases.len(), 2, "both tokens are named: {:?}", ep.bases);
        assert_eq!(ep.reasons.get(MIXED_BASIS), Some(&1));
        assert_eq!(
            ep.state,
            Completeness::Partial,
            "it was counted, twice — §9.5 calls that partial, not unavailable"
        );
        assert_eq!(
            ep.absolute,
            Some(23),
            "the head figure still prints on its own"
        );
        assert_eq!(ep.max, Some(23));
    }

    #[test]
    fn coverage_counts_the_rows_that_could_have_a_figure_and_splits_the_rest_by_reason() {
        let oid = "a".repeat(40);
        let mut store = indexed(&[commit(&oid, NOW, &["src/a.rs", "src/b.rs", "img/x.png"])]);
        store
            .conn_mut()
            .execute(
                "UPDATE git_commit_file SET is_binary = 1, change_kind = 'binary' WHERE new_path = 'img/x.png'",
                [],
            )
            .expect("a binary row");
        fill(&mut store, &oid, &["src/a.rs"], 4, 6);
        store
            .record_complexity(&[ComplexityWrite {
                commit: &oid,
                path_hash: &hash("src/b.rs"),
                basis: BASIS,
                before: SideWrite::Unmeasurable {
                    reason: "language_unknown",
                },
                after: SideWrite::Unmeasurable {
                    reason: "language_unknown",
                },
                max: None,
            }])
            .expect("fill");

        let coverage = store.complexity_coverage().expect("coverage");
        assert_eq!(coverage.rows, 3);
        assert_eq!(
            coverage.measurable, 2,
            "a binary row can never carry a figure"
        );
        assert_eq!(coverage.filled, 2, "refused-and-labelled is still filled");
        assert_eq!(
            coverage.with_figure, 1,
            "one row has a figure, one has a reason"
        );
        assert_eq!(coverage.unfilled(), 0);
        assert_eq!(coverage.percent(), Some(100.0));
        assert!(!coverage.half_filled(), "nothing is waiting to be filled");
        assert_eq!(coverage.reasons, vec![("language_unknown".to_owned(), 1)]);
        assert_eq!(coverage.bases, vec![(BASIS.to_owned(), 2)]);
    }

    #[test]
    fn a_half_filled_column_is_the_one_that_warrants_the_warning_line() {
        let oid = "a".repeat(40);
        let mut store = indexed(&[commit(&oid, NOW, &["src/a.rs", "src/b.rs"])]);
        let coverage = store.complexity_coverage().expect("coverage");
        assert_eq!(coverage.filled, 0);
        assert_eq!(coverage.unfilled(), 2);
        assert_eq!(coverage.percent(), Some(0.0));
        assert!(
            !coverage.half_filled(),
            "an empty column prints `?` everywhere; the line says so and nothing is being averaged"
        );
        assert_eq!(
            coverage.reasons.first(),
            Some(&(COLUMN_NOT_FILLED.to_owned(), 2)),
            "and the reason tally is the coverage line's own detail"
        );

        fill(&mut store, &oid, &["src/a.rs"], 4, 6);
        let coverage = store.complexity_coverage().expect("coverage");
        assert!(
            coverage.half_filled(),
            "one filled, one waiting: this is the state that lies quietly"
        );
        assert_eq!(coverage.percent(), Some(50.0));
        assert_eq!(coverage.reasons, vec![(COLUMN_NOT_FILLED.to_owned(), 1)]);
    }

    #[test]
    fn an_empty_index_has_no_coverage_to_report() {
        // `None`, not `0 %`: an index with no rows has not failed to be measured, it has nothing to measure.
        let store = Store::open_in_memory().expect("store");
        let coverage = store.complexity_coverage().expect("coverage");
        assert_eq!(
            (coverage.rows, coverage.measurable, coverage.filled),
            (0, 0, 0)
        );
        assert_eq!(coverage.percent(), None);
        assert!(!coverage.half_filled());
    }

    #[test]
    fn an_episode_the_index_has_nothing_for_is_absent_rather_than_empty() {
        let store = Store::open_in_memory().expect("store");
        let rollups = store
            .episode_complexity(&["no-such-episode".to_owned()])
            .expect("roll-up");
        assert!(
            rollups.is_empty(),
            "no file rows is not the same claim as zero paths"
        );
    }
}
