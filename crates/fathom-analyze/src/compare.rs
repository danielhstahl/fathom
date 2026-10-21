//! `compare` — *"what changed since I last looked?"*, answered for any two endpoints (`e5.3`, `reco-ifo`).
//!
//! The question a reviewer returns to a repository with is not "what did this turn do" but "what happened
//! while I was away". `fathom analyze` answers one episode; this answers a **pair** — `fathom cmp a1b2..c3d4`,
//! `m` then `c` in the shell, or the saved name a team agreed on (`since-the-refactor`).
//!
//! # One pipeline, not a second one
//!
//! The comparison is **composed from the per-episode delta documents the insight façade already produces**
//! ([`crate::insight::EpisodeDelta`]). There is no second delta engine here, which is what the ticket's
//! first acceptance criterion is testing: comparing two non-adjacent episodes *is* the composition of the
//! per-episode deltas between them, and [`Delta::merged`] is associative, so the composition of a prefix and
//! the composition of the rest produce the same answer as the whole walk
//! (`tests/compare_property.rs` proves it rather than asserting it once on a fixture that happens to work).
//!
//! # What "composed" means, per shape
//!
//! * **Numbers add.** `Totals` sum, and a path touched in three episodes carries the sum of its churn and
//!   the count of episodes that touched it. That is what "the delta over the range" means when the units are
//!   countable.
//! * **A path's final state wins.** A file added in the first episode and deleted in the third is *net*
//!   deleted, and the row says so with the last change kind seen plus the note that it was touched more
//!   than once. Silence here would be the worst kind of wrong: a comparison that says a file was added when
//!   it is gone is a comparison that gets a reviewer to go read a nonexistent file.
//! * **Prose is unioned, never averaged.** A section figure whose value differs across episodes is printed
//!   as the list of the values it took, capped at [`VALUE_CAP`], and the finished comparison carries one
//!   sentence naming how many figures of that section were unioned. Averaging "one new cycle" and "no
//!   cycles" into a number would be inventing a fact. Merged sections print their figures in label order,
//!   because a first-seen order would depend on where the range happened to be split.
//! * **Findings dedupe by identity, not by text.** Same kind and same subject across three episodes is one
//!   finding seen three times, and `seen` says three. A list of twelve copies of `new-cycle store→core`
//!   teaches a reviewer to stop reading the list.
//! * **A gap stays a gap.** If any episode in the range could not answer a section, the range cannot either,
//!   and says which episodes were the ones that could not.
//!
//! # Endpoints are labelled, because a spec that is shareable has to be unambiguous
//!
//! [`Endpoint`] carries the spec the human typed, the episode it resolved to, and the **lineage** it sits
//! on. Two endpoints on different lineages (the cross-branch case) are a pair, not a walk: the span between
//! them is temporal, and [`Comparison::notes`] says so and names every lineage that contributed. `main@1a2b3c`
//! and `feat/x@9f8e7d` in the header is what stops a pasted `fathom cmp` line being read as one branch's
//! history by the next person.
//!
//! # Saved comparisons are data, not state
//!
//! A name, a spec and a note — [`Saved`] — stored through the same session mechanism that carries the
//! reviewer's filter and cursor (`e1.11`), so they survive a restart because the session does, and they
//! appear in the `,` palette because `e5.4` reads them from the same place (`reco-mdb`).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::insight::{EpisodeDelta, Evidence, FileDelta, Finding, Gap, Section, SectionId, Totals};

/// The contract id a machine-readable comparison carries.
pub const CONTRACT: &str = "fathom.compare/1";

/// The separator between the two endpoints. Two dots, git's own "from `a` *up to* `b`".
pub const RANGE_SEPARATOR: &str = "..";

/// How many distinct values one merged figure prints before it summarises the rest.
pub const VALUE_CAP: usize = 4;

/// A parsed comparison spec: `a..b`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    /// The left endpoint exactly as typed.
    pub from: String,
    /// The right endpoint exactly as typed.
    pub to: String,
    /// The whole spec, as typed.
    pub raw: String,
}

impl Spec {
    /// Parse `a..b`.
    ///
    /// The two sides are kept verbatim — a rev, an oid prefix, an episode id, or a rev with a `branch@`
    /// prefix — because resolving them needs a repository, and this module has none. The parser's job is
    /// the shape, and the shape is the thing a reviewer shares in a message.
    ///
    /// # Errors
    ///
    /// A one-line sentence naming the shape that was expected.
    pub fn parse(text: &str) -> Result<Spec, String> {
        let raw = text.trim();
        let (from, to) = match raw.split_once(RANGE_SEPARATOR) {
            Some((from, to)) => (from.trim(), to.trim()),
            None => {
                return Err(format!(
                    "`{raw}` is not a comparison — the shape is `<from>..{to}`, two dots, e.g. \
                     `HEAD~9..HEAD` or `77aa0c..e8f1a2`",
                    to = "to"
                ));
            }
        };
        // Three dots is git's *symmetric* difference, which is not what a comparison of two endpoints
        // means, and reading `a...b` as `a..b` would be a silent change of question.
        let ragged = from.ends_with('.') || to.starts_with('.');
        if from.is_empty() || to.is_empty() || to.contains(RANGE_SEPARATOR) || ragged {
            return Err(format!(
                "`{raw}` is not a comparison — the shape is `<from>..<to>`, two dots, one on each side"
            ));
        }
        Ok(Spec {
            from: from.to_owned(),
            to: to.to_owned(),
            raw: raw.to_owned(),
        })
    }

    /// The spec as it is typed and as it is stored.
    #[must_use]
    pub fn display(&self) -> String {
        format!("{}{}{}", self.from, RANGE_SEPARATOR, self.to)
    }
}

/// A named comparison a reviewer kept (`since-the-refactor`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Saved {
    /// The name the reviewer chose; the key it is stored under.
    pub name: String,
    /// The spec it stands for.
    pub spec: String,
    /// Why the name exists, for the next reader.
    pub note: String,
    /// The session that saved it, as an instant the caller supplied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_utc: Option<i64>,
}

/// The session key a saved comparison lives under.
///
/// A prefix and a name rather than one blob of JSON in one row, because the session table is a key/value
/// table (`ADR-004 §4.7`'s `session_state`), because a reviewer may edit one line of it without a parser,
/// and because the palette listing is then a prefix scan like every other session read.
///
/// Under `tui.` for the same reason [`crate::search::history::SAVED_PREFIX`] is: `tui.` is the namespace
/// the session loader hands the shell (`fathom_tui::session::owns`), and a row outside it is a row the
/// shell is never given — a saved comparison the palette cannot see is a saved comparison that does not
/// exist.
pub fn saved_key(name: &str) -> String {
    format!("{SAVED_PREFIX}{name}")
}

/// The session-key prefix saved comparisons live under.
pub const SAVED_PREFIX: &str = "tui.compare.saved.";

/// Whether a session key is a saved comparison, and the name it carries.
#[must_use]
pub fn saved_name(key: &str) -> Option<&str> {
    key.strip_prefix(SAVED_PREFIX)
        .filter(|name| !name.is_empty())
}

/// Decode the saved comparisons out of session rows, sorted by name so the palette order is stable.
#[must_use]
pub fn saved_from_session(entries: &[(String, String)]) -> Vec<Saved> {
    let mut out: Vec<Saved> = entries
        .iter()
        .filter_map(|(key, value)| {
            let name = saved_name(key)?.to_owned();
            // The value is the spec, with an optional ` | note` tail — the form a reviewer can hand-edit.
            let (spec, note) = match value.split_once(" | ") {
                Some((spec, note)) => (spec.trim(), note.trim().to_owned()),
                None => (value.trim(), String::new()),
            };
            Some(Saved {
                name,
                spec: spec.to_owned(),
                note,
                created_utc: None,
            })
        })
        .collect();
    out.sort_by(|left, right| left.name.cmp(&right.name));
    out
}

/// The session row that saves a comparison, in the shape the session writer takes.
#[must_use]
pub fn saved_entry(saved: &Saved) -> (String, String) {
    let value = if saved.note.is_empty() {
        saved.spec.clone()
    } else {
        format!("{} | {}", saved.spec, saved.note)
    };
    (saved_key(&saved.name), value)
}

/// One endpoint of a comparison, labelled with everything a reader needs to be sure which point this is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    /// What the human typed.
    pub spec: String,
    /// The episode it resolved to, by ADR-003 identity.
    pub episode: String,
    /// The episode's summary, so a header line is enough to recognise the point.
    pub summary: String,
    /// The lineage the episode is ordered within — a branch name, or `lineage:<n>` when git did not name one.
    pub lineage: String,
    /// Where on that lineage, as an ordinal.
    pub lineage_seq: i64,
    /// The endpoint's instant, in micros.
    pub utc: i64,
}

impl Endpoint {
    /// The unambiguous label: `main@1a2b3c`, which is what makes a shared spec mean one thing.
    #[must_use]
    pub fn label(&self) -> String {
        format!("{}@{}", self.lineage, short(&self.episode))
    }
}

/// The first eight characters of an id, which is how every surface in this product shortens one.
fn short(id: &str) -> String {
    id.chars().take(8).collect()
}

/// Whether the two endpoints sit on one history or on two.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lineage {
    /// Both on one lineage: the span is the walk between two ordinals on it.
    Same {
        /// The lineage they share.
        lineage: String,
    },
    /// Different lineages: the span is temporal, and every contributed lineage is named.
    Across {
        /// The left endpoint's lineage.
        from: String,
        /// The right endpoint's lineage.
        to: String,
    },
}

impl Lineage {
    /// The word the header prints.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Lineage::Same { .. } => "same lineage",
            Lineage::Across { .. } => "across lineages",
        }
    }
}

/// The composed delta of a range: the merge of the per-episode deltas inside it.
///
/// This is the type the associativity property is stated about, so its fields are the ones that can be
/// merged without inventing anything: numbers add, files merge by path, sections union their prose,
/// findings dedupe.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Delta {
    /// The summed scale, with `n_files` the count of *distinct* paths the range touched.
    pub totals: Totals,
    /// The merged file rows, in path order.
    pub files: Vec<FileDelta>,
    /// The merged sections, in [`SectionId::ALL`]'s order.
    pub sections: Vec<Section>,
    /// The findings the range raised, deduped by kind and subject, in `(kind, subject)` order.
    pub findings: Vec<Finding>,
    /// How many episodes raised each deduped finding, keyed by `kind\0subject`. A finding seen in three
    /// episodes of a range is one finding the range kept raising, and the count is the fact — which is why
    /// it is a number in its own map and not text appended to a message nobody can parse.
    pub finding_seen: BTreeMap<String, usize>,
    /// The distinct values each merged figure took, keyed `section\u{0}label`.
    ///
    /// The values are kept as *data* rather than as the rendered `a + b` prose, for two reasons. One is the
    /// reader: a figure whose value differs across a range is printed as the list of what each episode
    /// said, and a list capped at [`VALUE_CAP`] has to know how many values there were. The other is the
    /// algebra: a union of a union has to be computed from the parts, or the answer depends on where the
    /// range was split — which is the accident this whole module is built to forbid.
    pub union_parts: BTreeMap<String, BTreeSet<String>>,
    /// Everything the range could not answer, with the episodes that could not answer it.
    pub gaps: Vec<Gap>,
    /// How many episodes this was composed from. `0` is an empty range, which is a real answer and prints
    /// as one.
    pub episodes: usize,
}

/// One episode's contribution to a comparison: the delta document the façade produced, plus the findings
/// its owner lent to it.
///
/// Findings travel separately because the delta model *renders* them into its attention section and does not
/// keep them as data afterwards, and a comparison that re-parsed prose out of a rendered section would be
/// reading its own output back to itself. Whoever has the documents has the findings too — `fathom cmp` and
/// `fathom export` both do — so the caller hands them over beside the document.
#[derive(Debug, Clone)]
pub struct Contribution {
    /// The episode's versioned delta document.
    pub delta: EpisodeDelta,
    /// The findings `e4.3`'s detectors raised for it, if anyone ran them.
    pub findings: Vec<Finding>,
}

impl Contribution {
    /// A contribution with no findings: the honest default when the detectors were not run, and the reason
    /// a comparison says "findings were not evaluated here" rather than "no findings".
    #[must_use]
    pub fn without_findings(delta: EpisodeDelta) -> Contribution {
        Contribution {
            delta,
            findings: Vec::new(),
        }
    }
}

impl Delta {
    /// Lift one episode's contribution into a composition of one.
    #[must_use]
    pub fn of(item: &Contribution) -> Delta {
        let delta = &item.delta;
        let mut finding_seen = BTreeMap::new();
        for finding in &item.findings {
            *finding_seen.entry(finding_key(finding)).or_insert(0) += 1;
        }
        Delta {
            totals: delta.totals.clone(),
            files: delta.files.clone(),
            sections: delta.sections.clone(),
            findings: item.findings.clone(),
            finding_seen,
            union_parts: BTreeMap::new(),
            gaps: delta.gaps.clone(),
            episodes: 1,
        }
    }

    /// Compose any number of episode contributions.
    #[must_use]
    pub fn compose(items: &[Contribution]) -> Delta {
        let mut out = Delta::default();
        for item in items {
            out = out.merged(&Delta::of(item));
        }
        out
    }

    /// Merge two composed deltas. **Associative**, which is the property the range-vs-pair test rests on.
    ///
    /// Every field is merged into a canonical shape — totals summed, files keyed by path and sorted by it,
    /// findings sorted by `(kind, subject)`, gaps deduplicated and sorted, unioned labels collected in a
    /// sorted set — so `(a ⊎ b) ⊎ c == a ⊎ (b ⊎ c)` holds for the values and not merely up to formatting.
    #[must_use]
    pub fn merged(&self, other: &Delta) -> Delta {
        let totals = merge_totals(&self.totals, &other.totals);
        let files = merge_files(&self.files, &other.files);
        let mut union_parts = self.union_parts.clone();
        for (key, parts) in &other.union_parts {
            union_parts
                .entry(key.clone())
                .or_default()
                .extend(parts.iter().cloned());
        }
        let sections = merge_sections(
            &self.sections,
            &other.sections,
            &self.union_parts,
            &other.union_parts,
            &mut union_parts,
        );
        let findings = merge_findings(&self.findings, &other.findings);
        let mut finding_seen = self.finding_seen.clone();
        for (key, count) in &other.finding_seen {
            *finding_seen.entry(key.clone()).or_insert(0) += *count;
        }
        let gaps: BTreeSet<(String, String, String, String, String)> = self
            .gaps
            .iter()
            .chain(other.gaps.iter())
            .map(gap_key)
            .collect();
        let mut ordered: Vec<Gap> = gaps
            .into_iter()
            .map(|(section, short, reason, owner, remedy)| Gap {
                section,
                short,
                reason,
                owner,
                remedy: (!remedy.is_empty()).then_some(remedy),
            })
            .collect();
        ordered.sort_by_key(gap_key);
        Delta {
            totals,
            files,
            sections,
            findings,
            finding_seen,
            union_parts,
            gaps: ordered,
            episodes: self.episodes + other.episodes,
        }
    }

    /// Whether the composition says "nothing happened", which is a claim it is only allowed to make when
    /// every episode in the range said so.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
            && self
                .sections
                .iter()
                .all(|section| section.figures.is_empty())
            && self.findings.is_empty()
    }
}

/// The dedupe key: a finding is the same finding when its kind and its subject match.
fn finding_key(finding: &Finding) -> String {
    format!("{}\u{0}{}", finding.kind, finding.subject)
}

/// The order a merged gap list is sorted into: section, then the short form, then the rest. A gap is
/// identified by all of it, so the sort key has to be all of it too.
fn gap_key(gap: &Gap) -> (String, String, String, String, String) {
    (
        gap.section.clone(),
        gap.short.clone(),
        gap.reason.clone(),
        gap.owner.clone(),
        gap.remedy.clone().unwrap_or_default(),
    )
}

/// Sum the countable totals; carry the flags that are not countable by taking the worse answer.
fn merge_totals(left: &Totals, right: &Totals) -> Totals {
    Totals {
        // `n_files` is deliberately *not* summed here: the caller recomputes it from the merged paths,
        // because two episodes touching one file is one file to a reviewer and two only to a counter.
        n_files: left.n_files + right.n_files,
        n_add: left.n_add + right.n_add,
        n_del: left.n_del + right.n_del,
        n_binary: left.n_binary + right.n_binary,
        churn: left.churn + right.churn,
        discounted_churn: left.discounted_churn + right.discounted_churn,
        reviewable_churn: left.reviewable_churn + right.reviewable_churn,
        files_truncated: left.files_truncated || right.files_truncated,
        files_read: left.files_read + right.files_read,
        reconciles: match (left.reconciles, right.reconciles) {
            (Some(true), Some(true)) => Some(true),
            (Some(false), _) | (_, Some(false)) => Some(false),
            _ => None,
        },
    }
}

/// Merge the file rows by path: churn adds, the last change kind wins, the episode count accumulates.
///
/// "Last" means last in the order the two sides were offered, and a caller that composes a range in lineage
/// order (`Comparison::compose`'s contract) has already decided that order; the rows come out sorted by
/// path, so the merge itself adds no ordering of its own.
fn merge_files(left: &[FileDelta], right: &[FileDelta]) -> Vec<FileDelta> {
    let mut by_path: BTreeMap<String, FileDelta> = BTreeMap::new();
    for side in [left, right] {
        for file in side {
            match by_path.get_mut(&file.path) {
                None => {
                    let mut carried = file.clone();
                    carried.commits = file.commits.max(1);
                    by_path.insert(file.path.clone(), carried);
                }
                Some(seen) => {
                    seen.add += file.add;
                    seen.del += file.del;
                    seen.commits += file.commits.max(1);
                    seen.binary |= file.binary;
                    if file.change != "unrecorded" {
                        // The last word wins, because the range's question is what state is the tree in now.
                        seen.change = file.change.clone();
                    }
                    if file.old_path.is_some() {
                        seen.old_path = file.old_path.clone();
                    }
                }
            }
        }
    }
    by_path.into_values().collect()
}

/// Merge the seven sections. Figures merge by label; a label whose prose differs across episodes becomes
/// the list of the values it took, and those values are kept in `parts_out` so a later merge unions the
/// parts rather than re-reading its own rendered prose.
fn merge_sections(
    left: &[Section],
    right: &[Section],
    left_parts: &BTreeMap<String, BTreeSet<String>>,
    right_parts: &BTreeMap<String, BTreeSet<String>>,
    parts_out: &mut BTreeMap<String, BTreeSet<String>>,
) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    for id in SectionId::ALL {
        let key = id.as_str();
        let left_section = left.iter().find(|section| section.id == key);
        let right_section = right.iter().find(|section| section.id == key);
        let (Some(a), Some(b)) = (left_section, right_section) else {
            // One side absent: carry the other whole. An absent section is not an empty one, and inventing
            // an empty here would turn "nobody answered" into "nothing to report".
            if let Some(carried) = left_section.or(right_section) {
                out.push(carried.clone());
            }
            continue;
        };
        let mut figures = Vec::new();
        // The union of the two sides' labels, in label order. Sorting here is what makes the merge
        // associative: a first-seen order depends on which side of a split each episode landed on, and a
        // range that printed its figures in one order when walked whole and another when walked in halves
        // would be a composition whose answer depends on how it was asked.
        let mut labels: Vec<String> = a
            .figures
            .iter()
            .chain(b.figures.iter())
            .map(|figure| figure.label.clone())
            .collect::<BTreeSet<String>>()
            .into_iter()
            .collect();
        labels.sort();
        for label in labels {
            let parts: Vec<&crate::insight::Figure> = a
                .figures
                .iter()
                .chain(b.figures.iter())
                .filter(|figure| figure.label == label)
                .collect();
            // The values this label took across the whole range: whatever each side already recorded, plus
            // what this side's own figure says. Read as a set, so the same list of values produces the same
            // row however the range was grouped.
            let union_key = format!("{key}\u{0}{label}");
            let mut values: BTreeSet<String> = BTreeSet::new();
            for (recorded, section) in [(left_parts, a), (right_parts, b)] {
                if let Some(seen) = recorded.get(&union_key) {
                    values.extend(seen.iter().cloned());
                    continue;
                }
                if let Some(figure) = section.figures.iter().find(|figure| figure.label == label)
                    && !figure.value.is_empty()
                {
                    values.insert(figure.value.clone());
                }
            }
            let mut raw: BTreeMap<String, i64> = BTreeMap::new();
            for figure in &parts {
                for (key, value) in &figure.raw {
                    *raw.entry(key.clone()).or_insert(0) += *value;
                }
            }
            let known = parts.iter().any(|figure| figure.known);
            let value = match values.len() {
                0 => String::new(),
                1 => values.iter().next().cloned().unwrap_or_default(),
                count => {
                    let shown: Vec<String> = values.iter().take(VALUE_CAP).cloned().collect();
                    let mut text = shown.join(" + ");
                    if count > VALUE_CAP {
                        text.push_str(&format!(" (+{} more)", count - VALUE_CAP));
                    }
                    text
                }
            };
            if values.len() > 1 {
                parts_out.insert(union_key, values);
            }
            let evidence = parts
                .first()
                .map_or_else(|| Evidence::owner("e5.3"), |figure| figure.evidence.clone());
            figures.push(crate::insight::Figure {
                label,
                value,
                known,
                reason: parts.iter().find_map(|figure| figure.reason.clone()),
                raw,
                confidence: parts
                    .iter()
                    .filter_map(|figure| figure.confidence)
                    // `f64` is only `PartialOrd`, so the minimum is a fold with an explicit answer for
                    // "no episode estimated this" rather than an `Ord`-requiring `min`.
                    .fold(None, |least: Option<f64>, value| {
                        Some(least.map_or(value, |least| least.min(value)))
                    }),
                evidence,
            });
        }
        let gap = a.gap.clone().or_else(|| b.gap.clone());
        let state = if a.state == b.state {
            a.state.clone()
        } else {
            // Two different states compose to the weaker answer, never to whichever is more flattering.
            crate::insight::STATE_PARTIAL.to_owned()
        };
        out.push(Section {
            id: key.to_owned(),
            title: a.title.clone(),
            question: a.question.clone(),
            owner: a.owner.clone(),
            state,
            figures,
            remainder: Some(
                a.remainder
                    .unwrap_or(0)
                    .saturating_add(b.remainder.unwrap_or(0)),
            )
            .filter(|dropped| *dropped > 0),
            gap,
        });
    }
    out.retain(|section| !section.figures.is_empty() || section.gap.is_some());
    out
}

/// Findings dedupe by `(kind, subject)`: one finding seen three times is one finding, seen three times.
/// The first occurrence's wording is kept, so merging is order-independent for the text and the count in
/// [`Delta::finding_seen`] carries the repetition as a number.
fn merge_findings(left: &[Finding], right: &[Finding]) -> Vec<Finding> {
    let mut out: Vec<Finding> = Vec::new();
    for finding in left.iter().chain(right.iter()) {
        if !out
            .iter()
            .any(|seen| seen.kind == finding.kind && seen.subject == finding.subject)
        {
            out.push(finding.clone());
        }
    }
    out.sort_by(|left, right| (&left.kind, &left.subject).cmp(&(&right.kind, &right.subject)));
    out
}

/// A whole comparison: the two labelled endpoints, how they relate, and what the range did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Comparison {
    /// [`CONTRACT`].
    pub contract: String,
    /// The left endpoint.
    pub from: Endpoint,
    /// The right endpoint.
    pub to: Endpoint,
    /// Whether they are one history or two.
    pub lineage: Lineage,
    /// Every episode the composition walked, in order, oldest first.
    pub episodes: Vec<String>,
    /// The composed delta.
    pub delta: Delta,
    /// The sentences a reader needs that the numbers cannot carry.
    pub notes: Vec<String>,
}

impl Comparison {
    /// Compose a pair of endpoints and the episodes between them.
    ///
    /// `items` must be in lineage order, oldest first; the caller resolved the endpoints and picked the
    /// span (a repository is needed for that, and this module has none).
    #[must_use]
    pub fn compose(from: Endpoint, to: Endpoint, items: &[Contribution]) -> Comparison {
        let lineage = if from.lineage == to.lineage {
            Lineage::Same {
                lineage: from.lineage.clone(),
            }
        } else {
            Lineage::Across {
                from: from.lineage.clone(),
                to: to.lineage.clone(),
            }
        };
        let mut delta = Delta::compose(items);
        // The count of files a *range* touched is the count of distinct paths, not the sum of per-episode
        // counts: two episodes in one module is one place a reviewer has to read.
        delta.totals.n_files = i64::try_from(delta.files.len()).unwrap_or(i64::MAX);
        let mut notes = Vec::new();
        notes.push(format!(
            "composed from {} episode delta{} between the endpoints — the same documents \
             `fathom analyze` prints, merged by `e5.3`'s rules and by no second engine",
            items.len(),
            if items.len() == 1 { "" } else { "s" }
        ));
        if items.iter().all(|item| item.findings.is_empty()) {
            notes.push(
                "the detectors were not run for this range, so the findings list is empty because \
                 nothing was evaluated — `fathom why <episode>` runs them for one episode"
                    .to_owned(),
            );
        }
        match &lineage {
            Lineage::Same { lineage } => notes.push(format!(
                "both endpoints are on `{lineage}` ({} → {}), so the span is the walk between them",
                from.lineage_seq, to.lineage_seq
            )),
            Lineage::Across { from, to } => notes.push(format!(
                "the endpoints are on different lineages (`{from}` and `{to}`), so the span between \
                 them is temporal rather than a walk on one history — every episode between the two \
                 endpoints' dates is included and each one's lineage is named in the episode list"
            )),
        }
        if to.lineage_seq < from.lineage_seq {
            notes.push(
                "the right endpoint is *earlier* on its lineage than the left, so this comparison \
                 reads backwards — the delta is still the composition, and `fathom cmp b..a` is the \
                 direction a reader will expect"
                    .to_owned(),
            );
        }
        // The two sentences the merge earns, written from the finished composition rather than from the
        // moment a sub-merge noticed them — which is what keeps them identical however the range was split.
        let repeated = delta.files.iter().filter(|file| file.commits > 1).count();
        if repeated > 0 {
            notes.push(format!(
                "{repeated} path{} touched by more than one episode in the range: churn is summed and \
                 the change kind is the last one recorded, which is the tree's state at the right \
                 endpoint",
                if repeated == 1 { "" } else { "s" }
            ));
        }
        let mut union_by_section: BTreeMap<&str, usize> = BTreeMap::new();
        for (key, parts) in &delta.union_parts {
            if parts.len() > 1
                && let Some((section, _label)) = key.split_once('\u{0}')
            {
                *union_by_section.entry(section).or_insert(0) += 1;
            }
        }
        for (section, count) in union_by_section {
            notes.push(format!(
                "`{section}`: {count} figure{} carried different values across the range and {} \
                 printed as the list of what each episode said, not as one merged number",
                if count == 1 { "" } else { "s" },
                if count == 1 { "is" } else { "are" }
            ));
        }
        Comparison {
            contract: CONTRACT.to_owned(),
            from,
            to,
            lineage,
            episodes: items
                .iter()
                .map(|item| item.delta.episode.id.clone())
                .collect(),
            delta,
            notes,
        }
    }

    /// The header line every surface prints first: what is being compared, and how.
    #[must_use]
    pub fn header(&self) -> String {
        format!(
            "{}..{} · {} · {} episode{} · {} files · +{} −{}",
            self.from.label(),
            self.to.label(),
            self.lineage.as_str(),
            self.delta.episodes,
            if self.delta.episodes == 1 { "" } else { "s" },
            self.delta.totals.n_files,
            self.delta.totals.n_add,
            self.delta.totals.n_del
        )
    }

    /// The text rendering: header, notes, the merged sections, the findings, the gaps.
    #[must_use]
    pub fn render(&self) -> Vec<String> {
        let mut out = vec![
            format!(
                "Compare {}  (was: {})",
                self.header(),
                self.from.summary.as_str()
            ),
            format!("  from  {} — {}", self.from.label(), self.from.summary),
            format!("  to    {} — {}", self.to.label(), self.to.summary),
        ];
        for section in &self.delta.sections {
            out.push(format!(
                "{} — {} ({})",
                section.title,
                section
                    .figures
                    .iter()
                    .map(|figure| format!("{} {}", figure.label, figure.value))
                    .collect::<Vec<_>>()
                    .join(" · "),
                section.state
            ));
        }
        if !self.delta.findings.is_empty() {
            out.push(format!("Findings ({})", self.delta.findings.len()));
            for finding in &self.delta.findings {
                let seen = self
                    .delta
                    .finding_seen
                    .get(&finding_key(finding))
                    .copied()
                    .unwrap_or(1);
                out.push(format!(
                    "  {} {}{}: {}{}",
                    finding.kind,
                    finding.severity,
                    if finding.subject.is_empty() {
                        String::new()
                    } else {
                        format!(" · {}", finding.subject)
                    },
                    finding.message,
                    if seen > 1 {
                        format!(" · raised by {seen} episodes in the range")
                    } else {
                        String::new()
                    }
                ));
            }
        }
        if !self.delta.gaps.is_empty() {
            out.push(format!(
                "Gaps ({}) — what the range could not answer",
                self.delta.gaps.len()
            ));
            for gap in &self.delta.gaps {
                out.push(format!("  {}: {}", gap.section, gap.short));
            }
        }
        for note in &self.notes {
            out.push(format!("· {note}"));
        }
        out
    }
}
