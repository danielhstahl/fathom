//! The diff: two surfaces, one classified change set, per package.
//!
//! The pairing rule is the whole design, so it is said first. Items are paired by
//! *name, kind and — only where the language reaches a name through its file — path*. That choice does
//! most of the work the acceptance criteria ask for:
//!
//! * a **Go or Java** item that moved file within its package pairs with itself, digests identically,
//!   and produces no change at all, because in those languages nothing a caller compiled against moved;
//! * a **Rust, Python or TypeScript** item that moved file pairs as a *move*, and the move is breaking
//!   exactly when nothing still forwards the old path — which is the re-export-as-forwarding-fact rule
//!   [`crate::api::extract`] builds the reachability sets from, and the reason a barrel that still
//!   forwards a moved name keeps the release minor;
//! * an item whose kind changed in place pairs by name alone and is reported as
//!   [`Reason::KindChanged`] rather than as a removal and an
//!   unrelated addition, which is two rows where the reader wants one;
//! * a **Ruby** item is a file, so a renamed file is a removal and an addition, which is the honest
//!   answer when the file is the API.
//!
//! Then three rungs of cost, and the report always says which one answered ([`Answered`]):
//!
//! | Rung | What was compared | Cost |
//! |---|---|---|
//! | [`Answered::Bytes`] | the file bytes, via [`crate::api::surface::quick`] | one hash per file, no parse |
//! | [`Answered::Digest`] | the surface digests, after extraction | parse, no field comparison |
//! | [`Answered::Items`] | the item records, field by field | everything |
//!
//! Per package there is a fourth saving: a package whose digest is unchanged is skipped whole, so a
//! monorepo episode that touched one crate pays for one crate.

use crate::api::classify::{self, Class, Detail, Reason};
use crate::api::episode::Episode;
use crate::api::policy::ApiPolicy;
use crate::api::semver::{self, Bump, Counts, Driver, Recommendation};
use crate::api::surface::{Access, Identity, Item, ItemId, Origin, Package, Surface};
use serde::Serialize;
use std::collections::BTreeMap;

/// Which rung answered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Answered {
    /// Nothing surface-bearing changed, decided from the bytes. No grammar was opened.
    #[default]
    Bytes,
    /// Files changed and the public surface did not: the digests are equal.
    Digest,
    /// Items were compared and changes were found.
    Items,
    /// At least one side could not be read completely, so nothing here may be read as "no change".
    Incomplete,
}

impl Answered {
    /// The stable spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Answered::Bytes => "bytes",
            Answered::Digest => "digest",
            Answered::Items => "items",
            Answered::Incomplete => "incomplete",
        }
    }
}

/// What the comparison cost, so a report can say what it did not do. The architecture panes in this
/// repository always print the cost beside the answer (`e3.2`, `e3.3`), and this is the same rule.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Cost {
    /// Packages present on both sides.
    pub packages_compared: usize,
    /// Packages skipped because their digest was unchanged.
    pub packages_skipped_by_digest: usize,
    /// Packages that exist on only one side (a new or deleted crate/directory/gem).
    pub packages_one_sided: usize,
    /// Items whose records were compared field by field.
    pub items_compared: usize,
    /// Items whose records were identical, counted so "nothing changed" can be shown to have been
    /// checked rather than assumed.
    pub items_identical: usize,
    /// The rung that answered.
    pub answered: Answered,
}

impl Cost {
    /// The sentence beside the answer.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self.answered {
            Answered::Bytes => "cost: bytes only — no file was parsed".to_string(),
            Answered::Digest => format!(
                "cost: extraction only — {} package(s) digested identically, no item was compared",
                self.packages_skipped_by_digest
            ),
            Answered::Items => format!(
                "cost: {} package(s) compared, {} skipped on digest, {} item(s) compared, {} \
                 identical",
                self.packages_compared,
                self.packages_skipped_by_digest,
                self.items_compared,
                self.items_identical
            ),
            Answered::Incomplete => {
                "cost: as much as could be read, and that is the finding".to_string()
            }
        }
    }
}

/// One change to one item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    /// What changed.
    pub item: ItemId,
    /// The package, repeated on the change so a change can be printed on its own.
    pub package: String,
    /// Symbol-level or file-level.
    pub access: Access,
    /// The class, which is the highest-ranking reason.
    pub class: Class,
    /// Every reason, in the order found.
    pub reasons: Vec<Detail>,
    /// The `file:line` — the acceptance criterion, and it is the extractor's own span, so the line the
    /// reader clicks is the line the declaration is on. A removal carries the line it was last seen at.
    pub location: String,
    /// The episode that made it, when the caller said which episode this diff is about.
    pub episode: Option<String>,
    /// The revision pair, for a report that prints provenance.
    pub revision: Option<String>,
    /// The item lived in a scope that cannot be enumerated. Carried on the change because a reader
    /// scanning for `breaking` needs to see this next to it.
    pub synthetic: bool,
    /// How the item was established.
    pub origin: Origin,
}

impl Change {
    /// The line a report prints: class, name, location, reasons, episode.
    #[must_use]
    pub fn sentence(&self) -> String {
        let mut out = format!(
            "{} {} {} — {}",
            self.class.as_str(),
            self.item.name,
            self.location,
            if self.reasons.is_empty() {
                "(no reason recorded)".to_string()
            } else {
                self.reasons
                    .iter()
                    .map(Detail::sentence)
                    .collect::<Vec<_>>()
                    .join("; ")
            }
        );
        if let Some(episode) = &self.episode {
            out.push_str(&format!(" [episode {episode}]"));
        }
        if self.synthetic {
            out.push_str(" [scope cannot be enumerated]");
        }
        out
    }
}

/// One package's changes and its recommendation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackageDiff {
    /// The package.
    pub name: String,
    /// The language that decided the rules.
    pub language: crate::structure::Language,
    /// Symbol-level or file-level.
    pub access: Access,
    /// How names are reached.
    pub identity: Identity,
    /// The changes, sorted by class (worst first) then by identity.
    pub changes: Vec<Change>,
    /// The counts.
    pub counts: Counts,
    /// The recommendation, with its driver.
    pub recommendation: Recommendation,
    /// The package was skipped because its digest was unchanged.
    pub skipped_by_digest: bool,
    /// The package existed on one side only.
    pub one_sided: bool,
    /// What limits the claim for this package.
    pub caveat: Option<String>,
    /// The sentence saying what this language's surface can claim.
    pub surface_sentence: String,
}

/// Two revisions' difference, as data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diff {
    /// The schema version.
    pub schema_version: u32,
    /// The rung that answered.
    pub answered: Answered,
    /// What it cost.
    pub cost: Cost,
    /// The packages with changes or with a caveat, sorted by name.
    pub packages: Vec<PackageDiff>,
    /// The totals across every package.
    pub totals: Counts,
    /// The stated caveats, from both surfaces and from the policy: what was told about the read.
    pub caveats: Vec<String>,
    /// The holes in the read — a file that could not be parsed, paths the policy refused. Where
    /// [`Diff::caveats`] is context, this is the reason a "no change" answer here would not be sound,
    /// and it is what makes [`Answered::Incomplete`] what it means.
    pub gaps: Vec<String>,
    /// The episode this diff is attributed to, when it was.
    pub episode: Option<String>,
    /// Changes whose file is not in the episode's own changed-file list, counted and named. A
    /// non-empty list means the two surfaces did not come from the revision pair the episode says they
    /// did — which is a bug in the caller, and this is the only place it can be caught.
    pub attribution_gaps: Vec<String>,
}

impl Diff {
    /// Whether this is the "no public API change" answer, and legitimate.
    #[must_use]
    pub fn no_public_api_change(&self) -> bool {
        self.totals.total() == self.totals.neutral && self.answered != Answered::Incomplete
    }

    /// The packages with at least one change above [`Class::Neutral`].
    pub fn changed_packages(&self) -> impl Iterator<Item = &PackageDiff> {
        self.packages
            .iter()
            .filter(|package| !package.counts.empty())
    }

    /// Every change, across every package, worst class first.
    #[must_use]
    pub fn changes(&self) -> Vec<&Change> {
        let mut out: Vec<&Change> = self.packages.iter().flat_map(|p| &p.changes).collect();
        out.sort_by(|a, b| {
            b.class
                .rank()
                .cmp(&a.class.rank())
                .then_with(|| a.item.package.cmp(&b.item.package))
                .then_with(|| a.item.location_key().cmp(&b.item.location_key()))
        });
        out
    }

    /// The lines a surface prints, in the order a reviewer reads them.
    #[must_use]
    pub fn sentences(&self) -> Vec<String> {
        let mut out = vec![crate::api::report::headline(self)];
        for package in &self.packages {
            out.push(package.recommendation.why.clone());
            if let Some(caveat) = &package.caveat {
                out.push(format!("  note: {caveat}"));
            }
            for change in &package.changes {
                out.push(format!("  {}", change.sentence()));
            }
        }
        for gap in &self.gaps {
            out.push(format!("gap: {gap}"));
        }
        for caveat in &self.caveats {
            out.push(format!("note: {caveat}"));
        }
        out
    }
}

/// The key an item pairs on. See the module header: the path is in the key exactly where the language
/// makes the path part of the name.
fn pair_key(item: &Item) -> (String, String, Option<String>) {
    (
        item.id.name.clone(),
        item.id.kind.as_str().to_string(),
        match item.identity {
            Identity::Unit => None,
            Identity::Path | Identity::File => Some(item.id.path.clone()),
        },
    )
}

/// The move-match key: name and kind, no path.
fn move_key(item: &Item) -> (String, String) {
    (item.id.name.clone(), item.id.kind.as_str().to_string())
}

/// Diff two revisions' surfaces.
///
/// `episode` is the episode this diff is attributed to (see [`crate::api::episode`]); pass `None` for a
/// whole-revision question with no episode behind it.
#[must_use]
pub fn diff(before: &Surface, after: &Surface, episode: Option<&Episode>) -> Diff {
    let mut caveats: Vec<String> = Vec::new();
    let mut gaps: Vec<String> = Vec::new();
    let mut cost = Cost::default();
    let answered = if !before.complete() || !after.complete() {
        Answered::Incomplete
    } else if before.digest == after.digest {
        Answered::Digest
    } else {
        Answered::Items
    };
    cost.answered = answered;
    for surface in [before, after] {
        caveats.extend(surface.caveats.iter().cloned());
        if let Some(gap) = surface.gap_sentence() {
            gaps.push(gap);
        }
    }
    caveats.sort();
    caveats.dedup();
    gaps.sort();
    gaps.dedup();
    let mut packages: Vec<PackageDiff> = Vec::new();
    let mut totals = Counts::default();
    let mut attribution_gaps: Vec<String> = Vec::new();

    // The union of package names, in one sorted order.
    let mut names: Vec<&str> = before
        .packages
        .iter()
        .map(|package| package.name.as_str())
        .chain(after.packages.iter().map(|package| package.name.as_str()))
        .collect();
    names.sort();
    names.dedup();

    for name in names {
        let previous = before.package(name);
        let current = after.package(name);
        let (Some(current), Some(previous)) = (current, previous) else {
            // A package that exists on one side only: everything in it is an addition or a removal, and
            // the per-item rules still decide each one's class.
            let Some(package) = current.or(previous) else {
                continue;
            };
            let (changes, counts) = diff_package(previous, current, episode, &mut cost);
            cost.packages_one_sided += 1;
            let recommendation = recommendation_of(name, &changes, counts, after, episode);
            if !counts.empty() || recommendation.caveat.is_some() {
                packages.push(PackageDiff {
                    name: name.to_string(),
                    language: package.language,
                    access: package.access,
                    identity: package.identity,
                    changes,
                    counts,
                    recommendation,
                    skipped_by_digest: false,
                    one_sided: true,
                    caveat: package.caveat(),
                    surface_sentence: package.surface_sentence.clone(),
                });
            }
            totals.add_class_of(&counts);
            continue;
        };
        if previous.digest == current.digest {
            cost.packages_skipped_by_digest += 1;
            // Still surfaced when the package has a gap to report, so an unreadable package is never
            // silently absent from the answer.
            if let Some(caveat) = current.caveat() {
                packages.push(PackageDiff {
                    name: name.to_string(),
                    language: current.language,
                    access: current.access,
                    identity: current.identity,
                    changes: Vec::new(),
                    counts: Counts::default(),
                    recommendation: Recommendation::build(
                        name,
                        Counts::default(),
                        None,
                        after.policy.is_private(name),
                        Some(caveat.clone()),
                    ),
                    skipped_by_digest: true,
                    one_sided: false,
                    caveat: Some(caveat),
                    surface_sentence: current.surface_sentence.clone(),
                });
            }
            continue;
        }
        cost.packages_compared += 1;
        let (mut changes, counts) = diff_package(Some(previous), Some(current), episode, &mut cost);
        changes.sort_by(|a, b| {
            b.class
                .rank()
                .cmp(&a.class.rank())
                .then_with(|| a.item.name.cmp(&b.item.name))
        });
        let recommendation = recommendation_of(name, &changes, counts, after, episode);
        if let Some(gap) = attribution_gap(&changes, episode) {
            attribution_gaps.push(gap);
        }
        if !counts.empty() || recommendation.caveat.is_some() {
            packages.push(PackageDiff {
                name: name.to_string(),
                language: current.language,
                access: current.access,
                identity: current.identity,
                changes,
                counts,
                recommendation,
                skipped_by_digest: false,
                one_sided: false,
                caveat: current.caveat(),
                surface_sentence: current.surface_sentence.clone(),
            });
        }
        totals.add_class_of(&counts);
    }

    let mut diff = Diff {
        schema_version: crate::api::surface::SCHEMA_VERSION,
        answered,
        cost,
        packages,
        totals,
        caveats,
        gaps,
        episode: episode.map(|episode| episode.id.clone()),
        attribution_gaps,
    };
    diff.packages.sort_by(|a, b| a.name.cmp(&b.name));
    if diff.answered == Answered::Digest {
        // The digest rung answered: the surfaces are equal, and the honest report says so rather than
        // printing an empty change set.
        diff.packages.clear();
    }
    diff
}

/// Fold a per-package count into a running total.
pub trait AddCounts {
    /// Add every class of `other` into `self`.
    fn add_class_of(&mut self, other: &Counts);
}

impl AddCounts for Counts {
    fn add_class_of(&mut self, other: &Counts) {
        self.breaking += other.breaking;
        self.additive += other.additive;
        self.deprecating += other.deprecating;
        self.indeterminate += other.indeterminate;
        self.neutral += other.neutral;
    }
}

/// One package's changes.
///
/// Four passes, in this order, and the order matters: an exact pair is compared before anything is
/// called a move, and a move is called before a removal, because the last two are what a move *looks
/// like* if you do not look for it.
fn diff_package(
    before: Option<&Package>,
    after: Option<&Package>,
    episode: Option<&Episode>,
    cost: &mut Cost,
) -> (Vec<Change>, Counts) {
    let mut changes: Vec<Change> = Vec::new();
    let mut counts = Counts::default();
    let empty: Vec<Item> = Vec::new();
    let previous_items = before.map(|package| &package.items).unwrap_or(&empty);
    let current_items = after.map(|package| &package.items).unwrap_or(&empty);

    let mut previous: BTreeMap<(String, String, Option<String>), &Item> = BTreeMap::new();
    for item in previous_items {
        previous.insert(pair_key(item), item);
    }
    let mut current: BTreeMap<(String, String, Option<String>), &Item> = BTreeMap::new();
    for item in current_items {
        current.insert(pair_key(item), item);
    }

    // Pass 1 — the exact pairs.
    let mut orphan_before: Vec<&Item> = Vec::new();
    let mut orphan_after: Vec<&Item> = Vec::new();
    for (key, item) in &previous {
        match current.get(key) {
            None => orphan_before.push(item),
            Some(other) => {
                cost.items_compared += 1;
                if item.record() == other.record() {
                    cost.items_identical += 1;
                    continue;
                }
                let reasons = classify::reasons(Some(item), Some(other));
                push(&mut changes, &mut counts, other, item, reasons, episode);
            }
        }
    }
    for (key, item) in &current {
        if !previous.contains_key(key) {
            orphan_after.push(item);
        }
    }

    // Pass 2 — moves: the same name and kind, declared in a different file. Consumed from both sides
    // so a move is never also a removal.
    let mut pool: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();
    for (index, item) in orphan_after.iter().enumerate() {
        pool.entry(move_key(item)).or_default().push(index);
    }
    let mut consumed_before = vec![false; orphan_before.len()];
    let mut consumed_after = vec![false; orphan_after.len()];
    for (index, item) in orphan_before.iter().enumerate() {
        let Some(candidates) = pool.get_mut(&move_key(item)) else {
            continue;
        };
        let Some(candidate) = candidates
            .iter()
            .position(|position| !consumed_after[*position])
            .map(|position| candidates.remove(position))
        else {
            continue;
        };
        consumed_before[index] = true;
        consumed_after[candidate] = true;
        push_move(
            &mut changes,
            &mut counts,
            item,
            orphan_after[candidate],
            episode,
        );
    }

    // Pass 3 — the additions, and the kind changes hiding among them: the same name and file under a
    // different kind is one row that says `kind changed`, not two rows that say it twice.
    for (index, item) in orphan_after.iter().enumerate() {
        if consumed_after[index] {
            continue;
        }
        let pair = orphan_before
            .iter()
            .enumerate()
            .find(|(position, other)| {
                !consumed_before[*position]
                    && other.id.name == item.id.name
                    && other.id.path == item.id.path
            })
            .map(|(position, other)| (position, *other));
        match pair {
            Some((position, other)) => {
                consumed_before[position] = true;
                consumed_after[index] = true;
                let mut reasons = vec![Detail {
                    reason: Reason::KindChanged,
                    note: format!(
                        "was {}, now {}",
                        other.id.kind.as_str(),
                        item.id.kind.as_str()
                    ),
                }];
                reasons.extend(classify::reasons(Some(other), Some(item)));
                push(&mut changes, &mut counts, item, other, reasons, episode);
            }
            None => {
                let reasons = classify::reasons(None, Some(item));
                push(&mut changes, &mut counts, item, item, reasons, episode);
            }
        }
    }

    // Pass 4 — the removals, which is where `may have shrunk` is decided.
    for (index, item) in orphan_before.iter().enumerate() {
        if consumed_before[index] {
            continue;
        }
        let reasons = classify::reasons(Some(item), None);
        push(&mut changes, &mut counts, item, item, reasons, episode);
    }
    (changes, counts)
}

/// The change for an item that moved file.
fn push_move(
    changes: &mut Vec<Change>,
    counts: &mut Counts,
    before: &Item,
    after: &Item,
    episode: Option<&Episode>,
) {
    let mut reasons = classify::reasons(Some(before), Some(after));
    let shared: Vec<&String> = before
        .reachable_from
        .iter()
        .filter(|path| after.reachable_from.contains(path))
        .collect();
    let unreachable = after.identity == Identity::Path && shared.is_empty();
    reasons.insert(
        0,
        Detail {
            reason: if unreachable {
                Reason::ItemMovedUnreachable
            } else {
                Reason::ItemMoved
            },
            note: if unreachable {
                format!(
                    "was reachable from {}, now from {}; nothing forwards the old path",
                    before.reachable_from.join(", "),
                    after.reachable_from.join(", ")
                )
            } else {
                format!(
                    "{} → {}; still reachable from {}",
                    before.id.path,
                    after.id.path,
                    if shared.is_empty() {
                        "the package, which is the unit here".to_string()
                    } else {
                        shared
                            .iter()
                            .map(|value| value.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                )
            },
        },
    );
    push(changes, counts, after, before, reasons, episode);
}

fn push(
    changes: &mut Vec<Change>,
    counts: &mut Counts,
    where_item: &Item,
    about: &Item,
    reasons: Vec<Detail>,
    episode: Option<&Episode>,
) {
    if reasons.is_empty() {
        return;
    }
    let class = classify::class_of(&reasons);
    counts.add(class);
    changes.push(Change {
        item: about.id.clone(),
        package: where_item.id.package.clone(),
        access: about.access,
        class,
        reasons,
        // The location a reader clicks is the declaration *now*, which for a modification is the after
        // item's line — the before item's line points into a file that no longer looks like this. A
        // removal has no after, and carries the line it was last seen at.
        location: if where_item.line == 0 {
            where_item.id.path.clone()
        } else {
            where_item.location()
        },
        episode: episode.map(|episode| episode.id.clone()),
        revision: episode.map(|episode| episode.revision.clone()),
        synthetic: about.synthetic || where_item.synthetic,
        origin: about.origin,
    });
}

/// The package's recommendation, with its driver: the highest-ranking change's highest-ranking reason.
fn recommendation_of(
    name: &str,
    changes: &[Change],
    counts: Counts,
    surface: &Surface,
    episode: Option<&Episode>,
) -> Recommendation {
    let driver = driver_of(changes, episode);
    let caveat = if counts.indeterminate > 0 {
        Some(semver::indeterminate_sentence(counts.indeterminate))
    } else {
        None
    };
    Recommendation::build(
        name,
        counts,
        driver,
        surface.policy.is_private(name),
        caveat,
    )
}

/// The change and reason that decided the bump.
#[must_use]
pub fn driver_of(changes: &[Change], _episode: Option<&Episode>) -> Option<Driver> {
    let change = changes.iter().max_by_key(|change| change.class.rank())?;
    let detail = semver::driver_detail(&change.reasons)?;
    Some(Driver {
        item: if change.access == Access::File {
            change.item.path.clone()
        } else {
            change.item.name.clone()
        },
        location: change.location.clone(),
        reason: detail.reason,
        note: detail.note.clone(),
        episode: change.episode.clone(),
    })
}

/// The changes that do not belong to the episode they were attributed to.
fn attribution_gap(changes: &[Change], episode: Option<&Episode>) -> Option<String> {
    let episode = episode?;
    let offenders: Vec<&Change> = changes
        .iter()
        .filter(|change| !episode.covers(&change.item.path))
        .collect();
    if offenders.is_empty() {
        return None;
    }
    Some(format!(
        "{} change(s) in {} name a file this episode did not touch ({}): the two surfaces did not \
         come from the revision pair this episode says they did",
        offenders.len(),
        episode.id,
        offenders
            .iter()
            .take(4)
            .map(|change| change.item.path.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

/// The empty diff, for the rung that found nothing to report.
#[must_use]
pub fn no_change(answered: Answered, cost: Cost, policy: &ApiPolicy) -> Diff {
    let _ = policy;
    Diff {
        schema_version: crate::api::surface::SCHEMA_VERSION,
        answered,
        cost,
        packages: Vec::new(),
        totals: Counts::default(),
        caveats: Vec::new(),
        gaps: Vec::new(),
        episode: None,
        attribution_gaps: Vec::new(),
    }
}

/// Every package's own bump, whether or not it changed. A caller building a release note wants this; a
/// caller answering "what did this episode do" wants [`Diff::changed_packages`].
#[must_use]
pub fn bumps(after: &Surface, before: &Surface) -> Vec<(String, Bump)> {
    let diff = diff(before, after, None);
    let mut out: Vec<(String, Bump)> = after
        .packages
        .iter()
        .map(|package| {
            let counts = diff
                .packages
                .iter()
                .find(|entry| entry.name == package.name)
                .map(|entry| entry.counts)
                .unwrap_or_default();
            let recommendation = recommendation_of(&package.name, &[], counts, after, None);
            (package.name.clone(), recommendation.bump)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// The identity an item's location is keyed on, for the sorted change list.
trait LocationKey {
    fn location_key(&self) -> String;
}

impl LocationKey for ItemId {
    fn location_key(&self) -> String {
        format!("{}:{}", self.path, self.name)
    }
}
