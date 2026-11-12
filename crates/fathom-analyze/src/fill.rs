//! `fill.rs` — the index-time complexity fill (`e8.4`, `reco-l9bz.4`).
//!
//! ```text
//! the figures are counted while the bytes are in hand, and what is not counted says why
//! ```
//!
//! ADR-007 §9 made the per-file figure a *column* on `git_commit_file`, and `e8.3` built the shape, the
//! roll-up and the coverage. This is the pass that fills it, and the ticket that had to answer the two
//! questions the earlier ones left open: **where** the counting runs, and **what stops it**.
//!
//! # Where the measurement runs, and why not in the cache
//!
//! Counting decision points means reading file *contents*, and the spine keeps none: `git_commit_file`
//! carries a blob id per side and no bytes, and a file over `ingest.size_cap_mib` was never inflated at
//! all. So either the counting happens inside the index run, on the bytes that run is already reading, or
//! it is a derived analyzer row looked up through the `cache` table as `diff.per_file` is. **This pass is
//! the first, and three reasons decide it:**
//!
//! 1. **A second copy of a figure that already has a table is the thing ADR-004 §4.5 forbids.** The figure
//!    has a table — six columns of it, added by `e8.3` for exactly this. A `complexity.per_file` cache row
//!    would be written and read back only to be copied into those columns: a lookup per file row to move a
//!    number into the row it belongs to. And the memo the cache would exist to provide — *do not count this
//!    twice* — is already there: a row whose `complexity_basis` names this build's token has been counted,
//!    and [`Store::complexity_owed`] is the lookup. It costs one `PRIMARY KEY` prefix probe per commit, and
//!    unlike a second store of the figure it cannot disagree with the surface that prints it.
//! 2. **A budget would poison a cache row.** ADR-004 §5.5 digests into a cache key only the config keys an
//!    analyzer *declared*. A ceiling that decides whether a figure exists is exactly such a key: cache a
//!    `?` produced under a spent `extract.complexity.max_run_bytes` and that row outlives the raising of
//!    the ceiling, so the remedy the reason promises — raise it and re-index — would be false unless all
//!    three ceilings were declared and editing one evicted every figure in the store. As a column, a spent
//!    budget is **not a fill**: [`Store::complexity_owed`] keeps a budget-refused row owed, so the next run
//!    under a bigger ceiling finishes what this one declined. That is what makes a series of budgeted runs
//!    converge instead of calcifying, and it is why setting a ceiling low is safe.
//! 3. **The bytes are in hand here and nowhere else.** The analyses loop has just resolved the
//!    (path, before-blob, after-blob) triple for `diff.per_file`; a pass that ran later, off the store,
//!    would have to re-inflate every blob from the object database — and would find nothing to inflate for
//!    the files `ingest.size_cap_mib` had already refused. The figure for a monorepo's history is only ever
//!    computable at the moment its bytes are read.
//!
//! What that costs is measured rather than assumed: `benchmarks/reports/e8-4-index-time-complexity.json`
//! records the index time and the peak RSS of the same index with the pass off and on, on a real
//! repository, and the gate in `.github/workflows/complexity.yml` fails past the declared allowance.
//!
//! # What stops it, in the `reco-93w` vocabulary
//!
//! Three ceilings, each a `fathom.toml` key under `[extract]`, each with an observable outcome instead of a
//! slow run:
//!
//! | Key | Bounds | When spent |
//! |---|---|---|
//! | `extract.complexity.max_file_bytes` | one blob, decided from the object header so an oversized file is refused *without* being inflated | that side prints `?` with `too_large`, naming the limit |
//! | `extract.complexity.max_run_bytes` | one run's content reads | every remaining row prints `?` with `scan_budget`, and stays owed |
//! | `extract.complexity.max_run_ms` | one run's wall clock inside this pass | as above, naming the time key |
//!
//! The per-file ceiling refuses a *file*; the two per-run ceilings stop the *pass*, which is why they answer
//! `scan_budget` (ADR-007 §7.1's "a per-run cap spent, naming the key") and the per-file one answers
//! `too_large`. No new reason code was invented for any of it, which is the point of §7.1 being one
//! vocabulary. `fathom doctor` counts all three from the rows themselves — the reason tally is the count —
//! and [`Ledger`] carries the run's own numbers so `fathom index` can say what *this* run declined, in the
//! same breath as the progress row that reached 100 %.
//!
//! # The two things that make it cheap enough to run at all
//!
//! * **The base side is read from the store when the store already has it.** `complexity_before` *is* the
//!   figure at the first parent's tree (ADR-007 §5.2), and a parent that touched this path stored exactly
//!   that number under exactly this basis token. Reading it back is one index probe instead of an inflate
//!   and a parse, and it makes §9.2's telescoping identity true by construction rather than by two parses
//!   happening to agree.
//! * **Nothing is read for a file the pass was never going to count.** A binary row, a path whose extension
//!   names none of the six grammars, and a file over the per-file cap are all refused *before* the object
//!   database is asked — the same header-first read `reco-93w`'s decode cap depends on.
//!
//! # What this is not
//!
//! No threshold, no severity, no repository-wide total (ADR-007 §8): this module decides *presence*, and the
//! surfaces that print a figure own their own thresholds. No counting either — [`crate::structure::complexity`]
//! counts, and lint `M02` keeps it there. The one piece of arithmetic here is which §7.1 code a file with no
//! file-level reason gets, and [`dominant_reason`] explains why that is a tie-break rather than a new code.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use fathom_ingest::record::sha256_hex;
use fathom_ingest::repo::{BlobRead, Repository};
use fathom_store::complexity::{BUDGET_REASON, ComplexityWrite, SideWrite};
use fathom_store::spine::{FileRow, Store};

use crate::error::Error;
use crate::structure::complexity::{self, BASIS, FileComplexity, State};
use crate::structure::{ExtractPolicy, Language};

/// Which per-run ceiling stopped the pass, and so which key the reason sentences name.
///
/// A state rather than a `bool`, because the two have different remedies — one is a bigger number, the other
/// is a faster machine or a smaller repository — and because a run that would have hit both has to report the
/// one it hit *first*, which is the one its reader can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Declined {
    /// `extract.complexity.max_run_bytes` is spent.
    Bytes,
    /// `extract.complexity.max_run_ms` is spent.
    Millis,
}

impl Declined {
    /// The config key to raise, in its printed dotted form.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Declined::Bytes => "extract.complexity.max_run_bytes",
            Declined::Millis => "extract.complexity.max_run_ms",
        }
    }

    /// The words the progress row appends, short enough to fit beside a percentage.
    ///
    /// `Tick::note` is a `&'static str` because a tick is `Copy` and printed on a clock, and because the
    /// alternative — a formatted string with a count in it — is a row that changes width forty times a
    /// second. The counts belong to the run report, which has room for them.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Declined::Bytes => "complexity byte budget spent",
            Declined::Millis => "complexity time budget spent",
        }
    }
}

/// The three ceilings a pass is obeying, spent or not, so a report can say what bounded it instead of
/// leaving a reader to look the defaults up. `e8.4`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Ceilings {
    /// `extract.complexity.max_file_bytes`: the largest blob counted for one row.
    pub max_file_bytes: u64,
    /// `extract.complexity.max_run_bytes`: the run's byte budget.
    pub max_run_bytes: u64,
    /// `extract.complexity.max_run_ms`: the pass's own clock, in milliseconds.
    pub max_run_ms: u64,
}

impl Ceilings {
    /// The ceilings as one policy states them. A fill prints exactly these, because they are the three
    /// numbers its own comparisons use.
    #[must_use]
    pub fn of(policy: &ExtractPolicy) -> Self {
        Self {
            max_file_bytes: policy.complexity_max_file_bytes as u64,
            max_run_bytes: policy.complexity_max_run_bytes,
            max_run_ms: policy.complexity_max_run_ms,
        }
    }

    /// The human line: each ceiling under the last segment of its key, because the sentence is pointing at
    /// a config file and has no reason to spell `extract.complexity.` three times.
    #[must_use]
    pub fn as_text(&self) -> String {
        let cap = |n: u64| {
            if n == u64::MAX {
                "off".to_string()
            } else {
                human_bytes(n)
            }
        };
        let time = if self.max_run_ms.is_multiple_of(1000) && self.max_run_ms >= 1000 {
            format!("{} s", self.max_run_ms / 1000)
        } else {
            format!("{} ms", self.max_run_ms)
        };
        format!(
            "max_file_bytes {}, max_run_bytes {}, max_run_ms {time}",
            cap(self.max_file_bytes),
            cap(self.max_run_bytes)
        )
    }

    fn as_json(&self) -> serde_json::Value {
        serde_json::json!({
            "max_file_bytes": self.max_file_bytes,
            "max_run_bytes": self.max_run_bytes,
            "max_run_ms": self.max_run_ms,
        })
    }
}

/// A size the way the rest of fathom's lines say one: the largest unit that still leaves a number a
/// person can compare without counting zeros. Byte counts in a ceiling are config values, so 128 MB written
/// in the config reads back as 122.1 MiB here and that is fine — the line says which one it is.
fn human_bytes(n: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    let n = n as f64;
    if n >= GIB {
        format!("{:.1} GiB", n / GIB)
    } else if n >= MIB {
        format!("{:.1} MiB", n / MIB)
    } else if n >= KIB {
        format!("{:.0} KiB", n / KIB)
    } else {
        format!("{n} B")
    }
}

/// What one run's fill did: the numbers behind `fathom index`'s complexity line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ledger {
    /// File rows the pass answered, including the ones it declined.
    pub rows: usize,
    /// Rows that ended with at least one figure.
    pub measured: usize,
    /// Refusals by §7.1 code, `scan_budget` among them. The durable count of the same names lives in the
    /// `counter` table; this is the run's own share of it.
    pub refused: BTreeMap<String, usize>,
    /// Bytes of blob content inflated for the figure — the quantity `max_run_bytes` bounds.
    pub bytes_read: u64,
    /// Base-side figures taken from the parent's stored row instead of from a blob read.
    pub base_reused: usize,
    /// Milliseconds this pass's own work took — summed over the commits it filled, and charging nothing it
    /// did not do. `max_run_ms` bounds the same quantity, so the number reported is the number bounded.
    pub ms: u128,
    /// File rows the columns were written on.
    pub written: usize,
    /// Writes that named no file row — a fill racing `fathom index --prune`.
    pub missing: usize,
    /// The ceiling that stopped the pass, if one did.
    pub declined: Option<Declined>,
    /// What the pass was working under, whether or not any of it was spent.
    pub ceilings: Ceilings,
}

impl Ledger {
    /// Rows that answered `?` on at least one side, and why.
    #[must_use]
    pub fn refusals(&self) -> usize {
        self.refused.values().sum()
    }

    /// One line, for the run report and for `fathom doctor`'s note list: what the pass read, what it
    /// declined, and which key would have let it read more.
    #[must_use]
    pub fn line(&self) -> String {
        use std::fmt::Write as _;
        let mut line = format!(
            "complexity pass: {} file row(s), {} MiB read, {} ms",
            self.rows,
            self.bytes_read / (1024 * 1024),
            self.ms
        );
        if self.base_reused > 0 {
            let _ = write!(
                line,
                ", {} base side(s) reused from the store",
                self.base_reused
            );
        }
        if let Some(declined) = self.declined {
            let _ = write!(
                line,
                "; stopped by {} — the rest print `?` with `scan_budget` and stay owed",
                declined.key()
            );
        }
        line
    }

    /// The JSON form `fathom index --json` prints.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "rows": self.rows,
            "measured": self.measured,
            "refused": self
                .refused
                .iter()
                .map(|(code, rows)| serde_json::json!({"code": code, "rows": rows}))
                .collect::<Vec<_>>(),
            "bytes_read": self.bytes_read,
            "base_reused": self.base_reused,
            "ms": self.ms,
            "written": self.written,
            "missing": self.missing,
            "declined": self.declined.map(Declined::key),
            "ceilings": self.ceilings.as_json(),
        })
    }

    /// Fold the run's numbers into the store's durable counters, so `fathom doctor` can answer "has this
    /// store ever run out of complexity budget?" without a report to compare against.
    ///
    /// Names follow `cache.evict.<analyzer>`'s precedent: a family, then the thing counted, then the code
    /// or key the count is keyed by.
    pub fn count_in(&self, store: &mut Store) {
        store.bump_counter(
            "complexity.rows_measured",
            i64::try_from(self.measured).unwrap_or(i64::MAX),
        );
        store.bump_counter(
            "complexity.bytes_read",
            i64::try_from(self.bytes_read).unwrap_or(i64::MAX),
        );
        store.bump_counter("complexity.ms", i64::try_from(self.ms).unwrap_or(i64::MAX));
        for (code, rows) in &self.refused {
            store.bump_counter(
                &format!("complexity.refused.{code}"),
                i64::try_from(*rows).unwrap_or(i64::MAX),
            );
        }
        if let Some(declined) = self.declined {
            store.bump_counter(&format!("complexity.budget_spent.{}", declined.key()), 1);
        }
    }
}

/// One row's context, so [`Fill::side`] takes one argument that says what it means.
struct Row<'a> {
    /// The path, unquoted — what a language is decided from.
    path: String,
    /// The stored primary-key half, never re-derived.
    path_hash: &'a str,
    /// The first parent's oid, when the run has its row: the only commit whose stored figure *is* this
    /// row's base side.
    parent: Option<&'a str>,
    /// The base blob, as the spine stored it.
    blob_before: Option<&'a str>,
    /// The head blob.
    blob_after: Option<&'a str>,
    /// Whether the change kind says the path did not exist on the base side (`add`).
    absent_before: bool,
    /// …or on the head side (`delete`).
    absent_after: bool,
    /// Whether a rename moved this path: its base side lives under the *old* path hash, which this row
    /// does not carry, so nothing may be borrowed for it.
    renamed: bool,
    /// Git's verdict, already a column: such a row has no content to read at either side.
    binary: bool,
}

/// The pass, over one run.
///
/// Owns what has been *spent*, which is the only reason it is a struct rather than a function: the two
/// per-run ceilings are properties of the run, and a run is a sequence of commits.
pub struct Fill {
    policy: ExtractPolicy,
    /// Content bytes inflated so far — the quantity `max_run_bytes` bounds.
    spent_bytes: u64,
    /// Time this pass itself has spent working — the quantity `max_run_ms` bounds.
    ///
    /// Accumulated around each [`Fill::fill_commit`] rather than read off a clock started at the run's
    /// beginning, and that distinction is the difference between a budget and an accident. A deadline
    /// measured from the start of the analyses phase would charge this pass for every other analyzer that
    /// ran first, so on a repository where the structural pass is slow the complexity budget would be spent
    /// before a single blob was read — and the report would say the pass spent it, which would be a lie
    /// about a number nobody could reproduce. What is bounded is what this pass did.
    spent: Duration,
    /// Set the first time a ceiling refuses work; every row after it is answered with no read at all.
    declined: Option<Declined>,
    ledger: Ledger,
}

impl Fill {
    /// A fill obeying `policy`'s three complexity ceilings, with nothing spent yet.
    #[must_use]
    pub fn new(policy: ExtractPolicy) -> Self {
        let ceilings = Ceilings::of(&policy);
        Fill {
            policy,
            spent_bytes: 0,
            spent: Duration::ZERO,
            declined: None,
            ledger: Ledger {
                ceilings,
                ..Default::default()
            },
        }
    }

    /// Whether a per-run ceiling has been spent. The driver reads this to put the note on the progress row
    /// once per run rather than re-detecting it per commit.
    #[must_use]
    pub fn declined(&self) -> Option<Declined> {
        self.declined
    }

    /// The ceilings in force, for a report that prints them.
    #[must_use]
    pub fn policy(&self) -> &ExtractPolicy {
        &self.policy
    }

    /// Finish the run: close the clock, record which ceiling (if any) stopped it, and take the ledger.
    #[must_use]
    pub fn finish(mut self) -> Ledger {
        self.ledger.ms = self.spent.as_millis();
        self.ledger.declined = self.declined;
        self.ledger
    }

    /// Answer one commit's owed file rows and write what was found.
    ///
    /// `owed` is [`Store::complexity_owed`] for this commit — the rows whose stored claim this build cannot
    /// defend. Nothing here may *assume* a row is owed: that predicate is the only memo this pass has, so a
    /// row already measured is never measured twice and a warm run costs one probe.
    ///
    /// The write is one call, so one transaction per commit: a fill that dies halfway leaves the rows it
    /// wrote written and the rest owed, which is the resume property the spine's batching gives the commit
    /// rows themselves.
    ///
    /// # Errors
    ///
    /// [`Error::Store`] if the read or the write fails. A *blob* the object database would not serve is not
    /// an error: it is a `contents_not_kept` row, and the run continues — ADR-007 refuses a degraded run
    /// that reports itself as a failed one.
    pub fn fill_commit(
        &mut self,
        repo: &Repository,
        store: &mut Store,
        commit: &str,
        parent: Option<&str>,
        files: &[FileRow],
        owed: &BTreeSet<String>,
    ) -> Result<(), Error> {
        if owed.is_empty() {
            return Ok(());
        }
        let began = Instant::now();
        let out = self.fill_commit_inner(repo, store, commit, parent, files, owed);
        // The clock stops with the commit's write, so what `max_run_ms` bounds is this pass's own work and
        // not the wall clock of everything that happened around it.
        self.spent += began.elapsed();
        out
    }

    fn fill_commit_inner(
        &mut self,
        repo: &Repository,
        store: &mut Store,
        commit: &str,
        parent: Option<&str>,
        files: &[FileRow],
        owed: &BTreeSet<String>,
    ) -> Result<(), Error> {
        let mut writes: Vec<ComplexityWrite<'_>> = Vec::with_capacity(owed.len());
        for file in files {
            if !owed.contains(&file.path_hash) {
                continue;
            }
            self.ledger.rows += 1;
            let row = Row {
                // The path, unquoted: the spine stores `new_path` in git's quoted form, and a path that
                // needed quoting is a path whose escapes sit exactly where an extension would be read.
                path: fathom_ingest::paths::from_quoted(&file.new_path)
                    .to_string_lossy()
                    .into_owned(),
                path_hash: &file.path_hash,
                parent,
                blob_before: file.blob_old.as_deref(),
                blob_after: file.blob_new.as_deref(),
                absent_before: file.change_kind == "add",
                absent_after: file.change_kind == "delete",
                renamed: file.old_path.is_some(),
                binary: file.is_binary || file.change_kind == "binary",
            };
            let (before, _) = self.side(repo, store, &row, Side::Base)?;
            let (after, max) = self.side(repo, store, &row, Side::Head)?;
            for side in [before, after] {
                match side {
                    SideWrite::Measured { .. } => {}
                    SideWrite::Unmeasurable { reason } => {
                        *self.ledger.refused.entry(reason.to_owned()).or_insert(0) += 1;
                    }
                }
            }
            if row_measured(&before, &after) {
                self.ledger.measured += 1;
            }
            writes.push(ComplexityWrite {
                commit,
                path_hash: row.path_hash,
                basis: BASIS,
                before,
                after,
                max,
            });
        }
        let fill = store.record_complexity(&writes)?;
        self.ledger.written += fill.written;
        self.ledger.missing += fill.missing;
        Ok(())
    }

    /// One side of one row: a figure, or `?` carrying the §7.1 code that says why — plus the head side's
    /// largest single unit, which only a parse can supply.
    ///
    /// The order is the cost order, cheapest refusal first, because on a monorepo most rows are refused and
    /// the pass must not pay for a refusal it could have seen for nothing:
    ///
    /// 1. the path did not exist on this side (`add`'s base, `delete`'s head) — `absent_revision`, a zero
    ///    that is a fact (ADR-007 §6.1);
    /// 2. the spine measured the file binary — `binary`, and no object is asked for;
    /// 3. the extension names none of the six grammars — `language_unknown`, still no object read;
    /// 4. the store already holds this side's figure — one index probe, no inflate;
    /// 5. a per-run ceiling is spent — `scan_budget`, no read, and the row stays owed;
    /// 6. the blob is over `extract.complexity.max_file_bytes`, missing, or not a blob — `too_large` /
    ///    `contents_not_kept`, all three decided *without* inflating anything.
    fn side(
        &mut self,
        repo: &Repository,
        store: &Store,
        row: &Row<'_>,
        which: Side,
    ) -> Result<(SideWrite<'static>, Option<i64>), Error> {
        let (blob, absent) = match which {
            Side::Base => (row.blob_before, row.absent_before),
            Side::Head => (row.blob_after, row.absent_after),
        };
        if absent {
            return Ok((
                SideWrite::Measured {
                    figure: 0,
                    reason: "absent_revision",
                },
                None,
            ));
        }
        if row.binary {
            return Ok((SideWrite::Unmeasurable { reason: "binary" }, None));
        }
        if !Language::detect(&row.path).is_known() {
            return Ok((
                SideWrite::Unmeasurable {
                    reason: "language_unknown",
                },
                None,
            ));
        }
        if let Some(borrowed) = self.borrowed(store, row, which) {
            return Ok((borrowed, None));
        }
        if let Some(declined) = self.ceiling() {
            self.declined = Some(declined);
            return Ok((
                SideWrite::Unmeasurable {
                    reason: BUDGET_REASON,
                },
                None,
            ));
        }
        let Some(oid) = blob else {
            // A side that should have a blob and names none: nothing was kept to read. This is `e8.4`'s own
            // code — the one whose remedy is a re-index rather than a repair — and it is reached by an
            // `unexamined` spine row, not by an add or a delete, which were answered above.
            return Ok((
                SideWrite::Unmeasurable {
                    reason: "contents_not_kept",
                },
                None,
            ));
        };
        let Ok(oid) = oid.parse() else {
            return Ok((
                SideWrite::Unmeasurable {
                    reason: "contents_not_kept",
                },
                None,
            ));
        };
        let read = match repo.blob(oid, self.policy.complexity_max_file_bytes) {
            Ok(read) => read,
            // A blob the object database will not serve — a shallow graft, a gc'd loose object — is the
            // spine's news, not this pass's. The figure is absent and the reason is that the contents are
            // not here; `fathom doctor` already names the commits the object database is hiding.
            Err(_) => {
                return Ok((
                    SideWrite::Unmeasurable {
                        reason: "contents_not_kept",
                    },
                    None,
                ));
            }
        };
        let bytes = match read {
            BlobRead::Present { bytes, .. } => bytes,
            // Decided from the object header: a 400 MB vendored file cost this pass a header read, which is
            // the whole point of the per-file ceiling being this pass's own key.
            BlobRead::Oversized { .. } => {
                return Ok((
                    SideWrite::Unmeasurable {
                        reason: "too_large",
                    },
                    None,
                ));
            }
            BlobRead::WrongKind { .. } => {
                return Ok((
                    SideWrite::Unmeasurable {
                        reason: "contents_not_kept",
                    },
                    None,
                ));
            }
        };
        self.spent_bytes += bytes.len() as u64;
        self.ledger.bytes_read += bytes.len() as u64;
        let counted = count_file(&row.path, &bytes, &self.policy);
        let Some(counted) = counted else {
            // A binary built without the grammars cannot count anything, and ADR-007 §7.1 has the code for
            // it. The rows still get filled — with a reason — because "this build never looked" and "this
            // build cannot look" are different claims and only one of them is fixed by `fathom index`.
            return Ok((
                SideWrite::Unmeasurable {
                    reason: "grammar_not_compiled",
                },
                None,
            ));
        };
        let figure = self.stored(&counted);
        // ADR-007 §5.1: `complexity_max` is the head side's largest single unit, and it is `None` whenever
        // there is no head figure — a maximum over nothing is not `0`.
        let max = match figure {
            SideWrite::Measured { .. } => counted.max.and_then(|max| i64::try_from(max).ok()),
            SideWrite::Unmeasurable { .. } => None,
        };
        Ok((figure, max))
    }

    /// The base-side figure the store already holds, when the first parent measured this very path.
    ///
    /// `complexity_before` is defined as the figure at the parent's tree (ADR-007 §5.2), so a parent that
    /// touched this path has already stored this number. Taking it back is cheaper than recomputing it by
    /// the entire cost of a blob read and a parse, and it makes the two columns agree by definition rather
    /// than by luck — which is what `e8.3`'s telescoping assertion is about. A rename is excluded: the base
    /// side of a rename lives under the *old* path hash, which this row does not carry.
    fn borrowed(
        &mut self,
        store: &Store,
        row: &Row<'_>,
        which: Side,
    ) -> Option<SideWrite<'static>> {
        if which != Side::Base || row.renamed {
            return None;
        }
        let parent = row.parent?;
        let stored = store
            .file_complexity(parent, row.path_hash)
            .ok()
            .flatten()
            .filter(|side| side.filled())?;
        let figure = stored.after.figure()?;
        self.ledger.base_reused += 1;
        Some(SideWrite::Measured {
            figure,
            // The parent's reason travels with its figure: a base side that was a measured zero stays a
            // measured zero, and a borrowed floor does not silently become an exact number.
            // `column_not_filled` is a reader's substitution, never a stored one, so it cannot travel.
            reason: static_code(stored.after.reason()),
        })
    }

    /// Which ceiling, if any, the next read would cross.
    ///
    /// Checked *before* the read, never after: a budget discovered by overspending is a budget that bounds
    /// nothing. The byte ceiling is a `>=` rather than a `>` so that a budget of `0` declines the pass
    /// outright, which is the CI spelling the config reference documents.
    fn ceiling(&self) -> Option<Declined> {
        if let Some(declined) = self.declined {
            return Some(declined);
        }
        if self.spent_bytes >= self.policy.complexity_max_run_bytes {
            return Some(Declined::Bytes);
        }
        if self.spent >= Duration::from_millis(self.policy.complexity_max_run_ms) {
            return Some(Declined::Millis);
        }
        None
    }

    /// A counted file, narrowed to one side's stored shape (ADR-007 §7.4's four rows).
    fn stored(&self, counted: &FileComplexity) -> SideWrite<'static> {
        let code = counted
            .reason
            .as_ref()
            .map(|reason| reason.as_str())
            .or_else(|| dominant_reason(counted));
        match counted.state() {
            State::Measured => SideWrite::Measured {
                figure: i64::try_from(counted.sum.unwrap_or(0)).unwrap_or(i64::MAX),
                // A zero that is a fact keeps its reason, because `0` and `0 because the path was not there`
                // are different claims and §7.4 stores them as different rows. An ordinary figure carries no
                // reason at all: §7.4's row 1 is `figure = n, reason = ''`, and `''` is what every surface
                // reads as "nothing to explain".
                reason: match code {
                    Some("absent_revision") | Some("no_bodies") => code.unwrap_or(""),
                    _ => "",
                },
            },
            // A floor: the sum of what could be counted, with the reason for the rest travelling beside it,
            // because §7.2 refuses a spent bound that prints bare.
            State::Partial => SideWrite::Measured {
                figure: i64::try_from(counted.sum.unwrap_or(0)).unwrap_or(i64::MAX),
                reason: code.unwrap_or(""),
            },
            State::Unavailable => SideWrite::Unmeasurable {
                reason: code.unwrap_or("parse_failed"),
            },
        }
    }
}

/// Count one file, or say that this build has no grammar to count it with.
///
/// The `langs-*` gates (ADR-002 §6.5) make this a build property rather than a per-file one, so the two
/// halves live here rather than inside [`Fill::side`]: a `minimal` binary still fills every row it reaches,
/// and what it writes is `grammar_not_compiled` on each side, which is a §7.1 code and not a new one.
#[cfg(any(
    feature = "lang-rust",
    feature = "lang-python",
    feature = "lang-typescript",
    feature = "lang-go",
    feature = "lang-ruby",
    feature = "lang-java"
))]
fn count_file(path: &str, bytes: &[u8], policy: &ExtractPolicy) -> Option<FileComplexity> {
    Some(complexity::measure(path, bytes, policy))
}

/// The no-grammar build's half of [`count_file`]: nothing is counted, and every row says which feature was
/// missing rather than being left blank.
#[cfg(not(any(
    feature = "lang-rust",
    feature = "lang-python",
    feature = "lang-typescript",
    feature = "lang-go",
    feature = "lang-ruby",
    feature = "lang-java"
)))]
fn count_file(_path: &str, _bytes: &[u8], _policy: &ExtractPolicy) -> Option<FileComplexity> {
    None
}

/// Whether a row ended with anything a roll-up may add: one figure on either side.
#[must_use]
fn row_measured(before: &SideWrite<'_>, after: &SideWrite<'_>) -> bool {
    [before, after]
        .iter()
        .any(|side| matches!(side, SideWrite::Measured { .. }))
}

/// The reason codes §7.1 makes static, so a stored code can be handed back as a `&'static str` without an
/// allocation. Anything unrecognised answers `""` — "nothing to explain" — rather than inventing a code.
#[must_use]
fn static_code(reason: &str) -> &'static str {
    match reason {
        "absent_revision" => "absent_revision",
        "no_bodies" => "no_bodies",
        "node_budget" => "node_budget",
        "body_not_visible" => "body_not_visible",
        "too_large" => "too_large",
        "not_utf8" => "not_utf8",
        "binary" => "binary",
        "language_unknown" => "language_unknown",
        "grammar_not_compiled" => "grammar_not_compiled",
        "extractor_not_shipped" => "extractor_not_shipped",
        "parse_failed" => "parse_failed",
        "parser" => "parser",
        "contents_not_kept" => "contents_not_kept",
        "scan_budget" => "scan_budget",
        "rename_unpaired" => "rename_unpaired",
        "mixed_basis" => "mixed_basis",
        _ => "",
    }
}

/// The reason most of an unmeasured file's units agree on.
///
/// ADR-007 §7.1 leaves a file whose units *disagree* with no file-level code — each line carries its own —
/// but a stored column has one cell, so a tie-break is unavoidable. It is made by count, then by code name:
/// deterministic, and it coins nothing. Where even that finds nothing, the caller's fallback is the one code
/// that is true of a file with units but no figures and no reasons, which is `parse_failed`.
fn dominant_reason(counted: &FileComplexity) -> Option<&'static str> {
    let mut tally: BTreeMap<&'static str, usize> = BTreeMap::new();
    for unit in &counted.units {
        if let Some(reason) = &unit.reason {
            *tally.entry(reason.as_str()).or_insert(0) += 1;
        }
    }
    tally
        .into_iter()
        .max_by(|(left_code, left), (right_code, right)| {
            left.cmp(right).then_with(|| left_code.cmp(right_code))
        })
        .map(|(code, _)| code)
}

/// Which side of a row is being answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    /// The base side: `complexity_before`.
    Base,
    /// The head side: `complexity_after`, and the side `complexity_max` describes.
    Head,
}

/// The sha256 the spine keys a path by, from the stored quoted form.
///
/// The fill itself never calls this — it takes the hash off the row it is filling, because re-deriving a
/// key is forming a second opinion about it (ADR-001 §7.10). Tests and out-of-band callers use it to name a
/// row the same way the store does.
#[must_use]
pub fn path_hash(quoted_path: &str) -> String {
    sha256_hex(fathom_ingest::paths::from_quoted(quoted_path).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ceiling_line_names_all_three_in_the_units_the_config_key_promises() {
        let ceilings = Ceilings {
            max_file_bytes: 2_000_000,
            max_run_bytes: 128_000_000,
            max_run_ms: 60_000,
        };
        assert_eq!(
            ceilings.as_text(),
            "max_file_bytes 1.9 MiB, max_run_bytes 122.1 MiB, max_run_ms 60 s"
        );
    }

    #[test]
    fn an_uncapped_file_ceiling_reads_as_off_rather_than_as_a_size() {
        // Only reachable from a hand-made policy — the config schema refuses `0` — but a line that printed
        // "18446744073709551615 B" would be a line lying about a limit that is not there.
        let ceilings = Ceilings {
            max_file_bytes: u64::MAX,
            max_run_bytes: 0,
            max_run_ms: 1,
        };
        assert_eq!(
            ceilings.as_text(),
            "max_file_bytes off, max_run_bytes 0 B, max_run_ms 1 ms"
        );
    }

    #[test]
    fn the_policy_the_fill_is_built_from_is_the_policy_the_line_prints() {
        // The line is the only place a reader learns what bounded the run, so it had better be the same
        // three numbers the pass compared against.
        let policy = ExtractPolicy {
            complexity_max_file_bytes: 4_096,
            complexity_max_run_bytes: 8_192,
            complexity_max_run_ms: 250,
            ..Default::default()
        };
        assert_eq!(
            Ceilings::of(&policy).as_text(),
            "max_file_bytes 4 KiB, max_run_bytes 8 KiB, max_run_ms 250 ms"
        );
    }
}
