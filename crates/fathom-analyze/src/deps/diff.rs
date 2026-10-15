//! The dependency delta: state before, state after, and the difference in the reviewer's terms
//! (e2.6 / reco-kvs).
//!
//! # Why state and not text
//!
//! A `pnpm-lock.yaml` hunk can add 140 lines and mean one new direct dependency; a one-line `Cargo.toml`
//! change can mean 60 new resolved packages; a mass refresh can move 150 packages that nobody asked for.
//! Those three look identical in text and completely different in review, so this module diffs the *graph*
//! [`super::state`] folded out of both revisions and reports eight separate facts:
//!
//! | fact | where it lives |
//!  |---|---|
//!  | new direct dependencies, removed ones, upgrades and downgrades | [`DepChange`] rows, by `attribution` |
//!  | transitive-only movement | the same rows, `attribution: transitive` |
//!  | resolution-graph size delta | [`DependencyDelta::graph`] |
//!  | new registries, git and vendored sources, source switches | [`DependencyDelta::sources`], `SourceSwitch` |
//!  | integrity/hash changes | `IntegrityChange` |
//!  | peer-dependency conflicts | [`DependencyDelta::peer_conflicts`] |
//!  | licence set delta | [`DependencyDelta::licenses`] |
//!  | what it means, in one sentence | [`DependencyDelta::headline`] |
//!
//! # Attribution is the whole ticket
//!
//! "One new direct dependency pulled in 140 transitive packages" is a claim, and the graph has to support
//! it: each transitive change is walked up to the root that asked for it
//! ([`super::state::State::who_asked`]). A transitive change whose root is *itself* a changed direct
//! dependency is **explained**; one whose root did not move is **not**. That is what separates a *bump
//! storm* — a lockfile regenerated, 150 packages moved, nobody asked — from a real new dependency, and it
//! is why [`Counts`] carries [`Counts::explained_transitive`] and [`Counts::unexplained_transitive`]
//! instead of one total. Where the walk cannot reach a root — no edges recorded, an ecosystem whose lock
//! states nothing, a manifest outside the changed set — the change stays `unknown`, the headline says
//! attribution is unavailable, and the pattern falls back to what the evidence supports rather than to the
//! sentence that reads better.
//!
//! # Every row carries its proof
//!
//! Each change carries the [`Proof`] — file and hunk — that makes it true, located by
//! [`super::hunk::locate`] in [`episode`]. A row with no hunk says why instead of going quiet.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::deps::hunk::{self, Evidence, FileDiff};
use crate::deps::model::Ecosystem;
use crate::deps::scan::{self, Delta};
use crate::deps::state::{Attribution, Basis, Declared, Node, State};
use crate::deps::version::{self, Direction, Satisfies};

/// Knobs for one delta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// How the two sides are parsed.
    pub scan: scan::Options,
    /// How the lockfile diffs the proofs come from are cut.
    pub hunks: hunk::Options,
    /// How many *unexplained* transitive changes make a mass refresh (a "bump storm").
    ///
    /// Ten separates a routine `cargo update` from a renovate-style sweep in every corpus measured here,
    /// and it is a knob precisely because that is a judgement, not a law.
    pub storm_min_changes: usize,
    /// How far up the resolved graph to walk when asking who asked for a package.
    pub attribution_depth: usize,
    /// Stop adding rows to [`DependencyDelta::changes`] after this many, and say so.
    pub max_changes: usize,
    /// Whether to compute hunk-level proofs at all — off for bulk passes that only want the counts.
    pub prove: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            scan: scan::Options::default(),
            hunks: hunk::Options::default(),
            storm_min_changes: 10,
            attribution_depth: 6,
            max_changes: 4000,
            prove: true,
        }
    }
}

/// What kind of difference one row records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    /// In the graph after, not before.
    Added,
    /// In the graph before, not after.
    Removed,
    /// The version went up.
    Upgraded,
    /// The version went down — the one a reviewer should be told about first.
    Downgraded,
    /// The version moved and the direction cannot be ordered (a commit, a tag, a wheel tag).
    VersionChanged,
    /// The same version — or the same name — now comes from somewhere else.
    SourceSwitch,
    /// The same version at the same source has a different integrity/hash.
    IntegrityChange,
    /// The same version declares a different licence.
    LicenseChanged,
    /// A declared range changed without the resolved version moving.
    RequirementChanged,
}

impl ChangeKind {
    /// The stable string, which is also the JSON spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ChangeKind::Added => "added",
            ChangeKind::Removed => "removed",
            ChangeKind::Upgraded => "upgraded",
            ChangeKind::Downgraded => "downgraded",
            ChangeKind::VersionChanged => "version_changed",
            ChangeKind::SourceSwitch => "source_switch",
            ChangeKind::IntegrityChange => "integrity_change",
            ChangeKind::LicenseChanged => "license_changed",
            ChangeKind::RequirementChanged => "requirement_changed",
        }
    }

    /// Whether a human scanning a table should stop on this row.
    #[must_use]
    pub const fn attention(self) -> bool {
        matches!(
            self,
            ChangeKind::Added
                | ChangeKind::Downgraded
                | ChangeKind::SourceSwitch
                | ChangeKind::IntegrityChange
        )
    }
}

/// The hunk that proves one row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proof {
    /// The file whose diff proves it — usually the lockfile the package was resolved in.
    pub path: String,
    /// The hunk header, `@@ -120,6 +120,11 @@`.
    pub header: String,
    /// The excerpt: ` `/`+`/`-` prefixed lines, cut from round the line that proves the row.
    pub lines: Vec<String>,
    /// Hunk lines before the excerpt.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub offset: usize,
    /// Hunk lines after the excerpt.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub tail: usize,
    /// Whether the line cap cut the hunk short.
    pub truncated: bool,
    /// Why there is no hunk, when there is none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub missing: Option<String>,
}

impl Proof {
    /// The proof of one located hunk: an excerpt centred on the line that proves `evidence`, not on the
    /// top of the hunk. A mass refresh packs a dozen packages into one hunk, and the top of it would prove
    /// nothing about this row.
    #[must_use]
    pub fn found(path: &str, hunk: &hunk::Hunk, evidence: &hunk::Evidence) -> Proof {
        const WINDOW: usize = 12;
        let centre = hunk.centre(evidence).unwrap_or(0);
        let start = centre.saturating_sub(WINDOW / 2);
        let lines: Vec<String> = hunk
            .lines
            .iter()
            .skip(start)
            .take(WINDOW)
            .cloned()
            .collect();
        let tail = hunk.lines.len().saturating_sub(start + lines.len());
        Proof {
            path: path.to_owned(),
            header: hunk.header.clone(),
            lines,
            offset: start,
            tail,
            truncated: hunk.truncated,
            missing: None,
        }
    }

    /// A row that could not be proved, saying why.
    #[must_use]
    pub fn absent(path: impl Into<String>, why: impl Into<String>) -> Proof {
        Proof {
            path: path.into(),
            header: String::new(),
            lines: Vec::new(),
            offset: 0,
            tail: 0,
            truncated: false,
            missing: Some(why.into()),
        }
    }

    /// Whether this is a real hunk.
    #[must_use]
    pub const fn found_hunk(&self) -> bool {
        self.missing.is_none()
    }

    /// The hunk as text, for the drill-down view.
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::new();
        if !self.header.is_empty() {
            out.push_str(&self.header);
            out.push('\n');
        }
        for line in &self.lines {
            out.push_str(line);
            out.push('\n');
        }
        if let Some(why) = &self.missing {
            out.push_str(why);
            out.push('\n');
        }
        out
    }
}

/// One changed dependency file with both sides' bytes: path, before, after, and how it changed.
type Blob = (String, Option<Vec<u8>>, Option<Vec<u8>>, scan::ChangeKind);

/// True when a count is zero, for `skip_serializing_if`.
fn is_zero(value: &usize) -> bool {
    *value == 0
}

/// One difference between the two states.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepChange {
    /// What kind.
    pub kind: ChangeKind,
    /// Ecosystem.
    pub ecosystem: Ecosystem,
    /// Package name.
    pub name: String,
    /// The version before, when there was one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    /// The version after, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    /// Direct, transitive, local, unknown.
    pub attribution: Attribution,
    /// What that attribution rests on.
    pub basis: Basis,
    /// The file that resolved it — where the proof lives.
    pub scope: String,
    /// One line in the reviewer's terms: `18.2.0 → 18.3.1`, `registry → git:https://…`.
    pub detail: String,
    /// The chain from the root that asked for it down to this package; empty when there is nothing to say.
    pub asked_by: Vec<String>,
    /// Whether the lockfile marks it development-only.
    pub dev: bool,
    /// The hunk that proves it, or the reason there is none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof: Option<Proof>,
}

impl DepChange {
    /// `foo 2.1.0`, preferring the after version: the way a table prints the row.
    #[must_use]
    pub fn package(&self) -> String {
        package(
            &self.name,
            self.after
                .as_deref()
                .or(self.before.as_deref())
                .unwrap_or(""),
        )
    }
}

/// `foo`, `foo 2.1.0`.
fn package(name: &str, version: &str) -> String {
    if version.is_empty() {
        name.to_owned()
    } else {
        format!("{name} {version}")
    }
}

/// A new source of packages: a registry, a git URL, a vendored tree, a path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewSource {
    /// The identity compared on: `git:https://host/repo` without its ref, `registry:internal`,
    /// `vendored:vendor`, `path:../shared`.
    pub identity: String,
    /// The kind — `registry`, `git`, `path`, `url`, `vendored`, `system`, `unknown`.
    pub kind: String,
    /// A package that arrived from it, so the row is not an abstract claim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
    /// Ecosystem.
    pub ecosystem: Ecosystem,
    /// The hunk that proves it, where one was found.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof: Option<Proof>,
}

/// A peer dependency whose range the resolved graph contradicts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerConflict {
    /// Ecosystem.
    pub ecosystem: Ecosystem,
    /// The package that declared the peer dependency, when a manifest named one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declared_by: Option<String>,
    /// The peer package.
    pub peer: String,
    /// The range as written.
    pub required: String,
    /// The version the graph actually resolved.
    pub resolved: String,
    /// The one-line statement of the disagreement.
    pub detail: String,
    /// The hunk that proves it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof: Option<Proof>,
}

/// A licence that appeared in, or left, the resolved set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LicenseSeen {
    /// The licence as the lockfile spelled it, verbatim.
    pub license: String,
    /// A package carrying it.
    pub package: String,
    /// Ecosystem.
    pub ecosystem: Ecosystem,
}

/// The licence set difference.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LicenseDelta {
    /// Licences now in the graph that were not before.
    pub added: Vec<LicenseSeen>,
    /// Licences that left.
    pub removed: Vec<LicenseSeen>,
}

/// Resolved-package counts on both sides.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Size {
    /// Resolved packages before.
    pub before: usize,
    /// Resolved packages after.
    pub after: usize,
    /// `after - before`.
    pub net: i64,
}

/// The graph size delta, overall and per ecosystem.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SizeDelta {
    /// Every ecosystem together.
    pub total: Size,
    /// One row per ecosystem.
    pub per_ecosystem: BTreeMap<String, Size>,
}

/// The counts, in the order a table prints them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    /// New direct dependencies.
    pub direct_added: usize,
    /// Direct dependencies gone.
    pub direct_removed: usize,
    /// Direct dependencies upgraded.
    pub direct_upgraded: usize,
    /// Direct dependencies downgraded.
    pub direct_downgraded: usize,
    /// Direct version moves with no orderable direction.
    pub direct_unordered: usize,
    /// Direct ranges that moved without the resolution moving.
    pub direct_requalified: usize,
    /// New transitive packages.
    pub transitive_added: usize,
    /// Transitive packages gone.
    pub transitive_removed: usize,
    /// Transitive packages upgraded.
    pub transitive_upgraded: usize,
    /// Transitive packages downgraded.
    pub transitive_downgraded: usize,
    /// Transitive version moves with no orderable direction.
    pub transitive_unordered: usize,
    /// Changes to this repository's own packages.
    pub local_changed: usize,
    /// Changes the evidence could not attribute at all.
    pub unattributed: usize,
    /// Transitive changes explained by a changed direct dependency.
    pub explained_transitive: usize,
    /// Transitive changes with no changed direct dependency behind them.
    pub unexplained_transitive: usize,
    /// Source switches.
    pub source_switches: usize,
    /// Integrity changes.
    pub integrity_changes: usize,
    /// Licence changes on a package whose version stayed.
    pub license_changes: usize,
    /// Licences new to the graph.
    pub licenses_added: usize,
    /// Licences gone from the graph.
    pub licenses_removed: usize,
    /// New registries, git URLs, vendored or path sources.
    pub new_sources: usize,
    /// Peer-dependency conflicts this episode introduced.
    pub peer_conflicts: usize,
    /// Every downgrade, direct or transitive.
    pub downgrades: usize,
}

impl Counts {
    /// Every direct-dependency change together.
    #[must_use]
    pub fn direct(&self) -> usize {
        self.direct_added
            + self.direct_removed
            + self.direct_upgraded
            + self.direct_downgraded
            + self.direct_unordered
            + self.direct_requalified
    }

    /// Every transitive change together.
    #[must_use]
    pub fn transitive(&self) -> usize {
        self.transitive_added
            + self.transitive_removed
            + self.transitive_upgraded
            + self.transitive_downgraded
            + self.transitive_unordered
    }

    /// Every version move that is not an addition or a removal.
    #[must_use]
    pub fn refreshed(&self) -> usize {
        self.transitive_upgraded + self.transitive_downgraded + self.transitive_unordered
    }
}

/// What kind of episode this was, in one value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pattern {
    /// Nothing dependency-shaped changed.
    NoChange,
    /// New direct dependencies, plus the transitive movement they explain.
    NewDirect {
        /// How many.
        added: usize,
    },
    /// Direct dependencies removed.
    RemovedDirect {
        /// How many.
        removed: usize,
    },
    /// Direct dependencies moved version.
    DirectVersionChange {
        /// How many.
        changed: usize,
    },
    /// Only the resolved graph moved — a regenerated lockfile.
    TransitiveRefresh {
        /// How many transitive packages moved.
        changed: usize,
        /// Whether that is a mass refresh (a bump storm) rather than a couple of bumps.
        mass: bool,
    },
    /// A direct change *and* a mass refresh that is not explained by it.
    Mixed {
        /// Direct changes.
        direct: usize,
        /// Unexplained transitive changes.
        transitive: usize,
    },
    /// Only provenance moved: sources, integrity, licences, with no version movement.
    ProvenanceOnly,
    /// Changes happened and the evidence would not attribute them.
    Unattributed,
}

impl Pattern {
    /// The stable string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Pattern::NoChange => "no_change",
            Pattern::NewDirect { .. } => "new_direct",
            Pattern::RemovedDirect { .. } => "removed_direct",
            Pattern::DirectVersionChange { .. } => "direct_version_change",
            Pattern::TransitiveRefresh { .. } => "transitive_refresh",
            Pattern::Mixed { .. } => "mixed",
            Pattern::ProvenanceOnly => "provenance_only",
            Pattern::Unattributed => "unattributed",
        }
    }

    /// Whether this pattern is the bump storm the ticket names.
    #[must_use]
    pub const fn is_storm(self) -> bool {
        matches!(
            self,
            Pattern::TransitiveRefresh { mass: true, .. } | Pattern::Mixed { .. }
        )
    }
}

/// The whole answer for one episode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyDelta {
    /// Every difference found, in `(kind, ecosystem, name)` order.
    pub changes: Vec<DepChange>,
    /// The counts.
    pub counts: Counts,
    /// The graph size delta.
    pub graph: SizeDelta,
    /// New registries, git URLs, vendored and path sources.
    pub sources: Vec<NewSource>,
    /// Sources that left.
    pub sources_gone: Vec<String>,
    /// Peer-dependency conflicts.
    pub peer_conflicts: Vec<PeerConflict>,
    /// The licence set delta.
    pub licenses: LicenseDelta,
    /// What kind of episode this was.
    pub pattern: Pattern,
    /// The one-sentence summary.
    pub headline: String,
    /// Statements about what could not be determined.
    pub notes: Vec<String>,
    /// Whether both sides were read completely.
    pub complete: bool,
    /// Files folded in on either side.
    pub files: usize,
}

/// One episode's dependency review: the per-path diff, the graph delta, and the hunks behind both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Episode {
    /// The per-path rows from [`scan::delta`].
    pub paths: Delta,
    /// The graph delta — the answer this ticket exists for.
    pub model: DependencyDelta,
    /// The line-level diff of each changed dependency file, which is where proofs come from.
    pub files: BTreeMap<String, FileDiff>,
}

/// A conflict as found inside one state, before the "is it new" test.
struct Conflict {
    ecosystem: Ecosystem,
    peer: String,
    required: String,
    resolved: String,
    declared_by: Option<String>,
}

impl Conflict {
    fn key(&self) -> (String, String, String) {
        (
            self.ecosystem.as_str().to_owned(),
            self.peer.clone(),
            self.resolved.clone(),
        )
    }
}

/// The sample fields both sides of a name-group share, from whichever node exists.
struct Sample {
    attribution: Attribution,
    basis: Basis,
    dev: bool,
    id: usize,
}

impl Sample {
    fn of(node: &Node) -> Sample {
        Sample {
            attribution: node.attribution,
            basis: node.basis,
            dev: node.dev,
            id: node.id,
        }
    }
}

/// The declared requirement entries that differ between two states.
fn declared_changes(before: &State, after: &State) -> Vec<Declared> {
    let mut out: Vec<Declared> = Vec::new();
    for declared in &after.declared {
        match before.declaration(declared.ecosystem, &declared.name) {
            None => out.push(declared.clone()),
            Some(previous) if previous.requirement != declared.requirement => {
                out.push(declared.clone())
            }
            Some(_) => {}
        }
    }
    for declared in &before.declared {
        if after
            .declaration(declared.ecosystem, &declared.name)
            .is_none()
        {
            out.push(declared.clone());
        }
    }
    out.sort_by(|left, right| {
        (left.ecosystem.as_str(), &left.name).cmp(&(right.ecosystem.as_str(), &right.name))
    });
    out.dedup_by(|a, b| a.ecosystem == b.ecosystem && a.name == b.name);
    out
}

/// Compute the dependency delta between two already-built states.
///
/// Every claim here is comparative: nothing is reported as *added* because a file appeared, only because
/// the graph after has a node the graph before did not. Proofs are attached later, in [`episode`], because
/// they need the raw text and this function does not take it.
#[must_use]
pub fn build(before: &State, after: &State, options: &Options) -> DependencyDelta {
    let mut changes: Vec<DepChange> = Vec::new();
    let mut changed_direct: BTreeSet<String> = BTreeSet::new();

    // ---- resolved-graph movement, one (scope, ecosystem, name) at a time -------------------------
    for (scope, ecosystem, name) in merged_scopes(before, after) {
        let before_nodes = before.at(&scope, ecosystem, &name);
        let after_nodes = after.at(&scope, ecosystem, &name);
        let Some(sample_node) = after_nodes.first().or(before_nodes.first()).copied() else {
            continue;
        };
        let sample = Sample::of(sample_node);
        let ask_state = if after_nodes.is_empty() {
            before
        } else {
            after
        };
        let asked_by = if sample.attribution == Attribution::Transitive {
            ask_state.who_asked(sample.id, options.attribution_depth)
        } else {
            Vec::new()
        };
        let before_versions: BTreeSet<&str> = before_nodes
            .iter()
            .map(|node| node.version.as_str())
            .collect();
        let after_versions: BTreeSet<&str> = after_nodes
            .iter()
            .map(|node| node.version.as_str())
            .collect();
        let added: Vec<&str> = after_versions
            .difference(&before_versions)
            .copied()
            .collect();
        let gone: Vec<&str> = before_versions
            .difference(&after_versions)
            .copied()
            .collect();
        let same: Vec<&str> = before_versions
            .intersection(&after_versions)
            .copied()
            .collect();
        // The same version on both sides can still have moved source, integrity or licence, and each of
        // those is a fact the ticket asks for separately from any version change.
        for version in &same {
            let (Some(before_node), Some(after_node)) = (
                before.node(&scope, ecosystem, &name, version),
                after.node(&scope, ecosystem, &name, version),
            ) else {
                continue;
            };
            for (kind, detail, from, to) in [
                (
                    crate::deps::diff::ChangeKind::SourceSwitch,
                    "source",
                    join(&before_node.sources),
                    join(&after_node.sources),
                ),
                (
                    crate::deps::diff::ChangeKind::IntegrityChange,
                    "integrity",
                    join(&before_node.integrity),
                    join(&after_node.integrity),
                ),
                (
                    crate::deps::diff::ChangeKind::LicenseChanged,
                    "licence",
                    join(&before_node.license),
                    join(&after_node.license),
                ),
            ] {
                if from == to {
                    continue;
                }
                changes.push(DepChange {
                    kind,
                    ecosystem,
                    name: name.clone(),
                    before: Some((*version).to_owned()),
                    after: Some((*version).to_owned()),
                    attribution: sample.attribution,
                    basis: sample.basis,
                    scope: scope.clone(),
                    detail: format!("{detail} {from} → {to}"),
                    asked_by: asked_by.clone(),
                    dev: sample.dev,
                    proof: None,
                });
            }
        }
        let mut push = |change: DepChange, changes: &mut Vec<DepChange>| {
            if change.attribution == Attribution::Direct {
                changed_direct.insert(change.name.clone());
            }
            changes.push(change);
        };
        match (gone.is_empty(), added.is_empty()) {
            (true, true) => {}
            (false, true) => push(
                DepChange {
                    kind: ChangeKind::Removed,
                    ecosystem,
                    name: name.clone(),
                    before: Some(gone.join(",")),
                    after: None,
                    attribution: sample.attribution,
                    basis: sample.basis,
                    scope: scope.clone(),
                    detail: format!("{} resolved version(s) gone", gone.len()),
                    asked_by: asked_by.clone(),
                    dev: sample.dev,
                    proof: None,
                },
                &mut changes,
            ),
            (true, false) => push(
                DepChange {
                    kind: ChangeKind::Added,
                    ecosystem,
                    name: name.clone(),
                    before: None,
                    after: Some(added.join(",")),
                    attribution: sample.attribution,
                    basis: sample.basis,
                    scope: scope.clone(),
                    detail: format!("new in {scope}"),
                    asked_by: asked_by.clone(),
                    dev: sample.dev,
                    proof: None,
                },
                &mut changes,
            ),
            (false, false) => {
                let from = gone.first().copied().unwrap_or_default();
                let to = added.first().copied().unwrap_or_default();
                let kind = match version::direction(from, to) {
                    Direction::Up => ChangeKind::Upgraded,
                    Direction::Down => ChangeKind::Downgraded,
                    Direction::Same | Direction::Unknown => ChangeKind::VersionChanged,
                };
                push(
                    DepChange {
                        kind,
                        ecosystem,
                        name: name.clone(),
                        before: Some(gone.join(",")),
                        after: Some(added.join(",")),
                        attribution: sample.attribution,
                        basis: sample.basis,
                        scope: scope.clone(),
                        detail: format!("{} → {}", gone.join(","), added.join(",")),
                        asked_by: asked_by.clone(),
                        dev: sample.dev,
                        proof: None,
                    },
                    &mut changes,
                );
                // A source switch that arrived with a version change is still a source switch: "the new
                // version also changed where it comes from" is the sentence a reviewer needs.
                let before_sources: BTreeSet<String> = before_nodes
                    .iter()
                    .flat_map(|node| node.sources.iter().cloned())
                    .collect();
                let after_sources: BTreeSet<String> = after_nodes
                    .iter()
                    .flat_map(|node| node.sources.iter().cloned())
                    .collect();
                if before_sources != after_sources {
                    push(
                        DepChange {
                            kind: ChangeKind::SourceSwitch,
                            ecosystem,
                            name: name.clone(),
                            before: Some(from.to_owned()),
                            after: Some(to.to_owned()),
                            attribution: sample.attribution,
                            basis: sample.basis,
                            scope: scope.clone(),
                            detail: format!("{} → {}", join(&before_sources), join(&after_sources)),
                            asked_by: asked_by.clone(),
                            dev: sample.dev,
                            proof: None,
                        },
                        &mut changes,
                    );
                }
            }
        }
    }

    // ---- declared requirements no resolution shows -----------------------------------------------
    let reported: BTreeSet<(Ecosystem, String)> = changes
        .iter()
        .map(|change| (change.ecosystem, change.name.clone()))
        .collect();
    for declared in declared_changes(before, after) {
        if reported.contains(&(declared.ecosystem, declared.name.clone()))
            || reported.contains(&(declared.ecosystem, declared.resolved_name().to_owned()))
        {
            // The graph already reported this package; a range change on top of a version change would be
            // the same fact twice.
            continue;
        }
        let existed_before = before
            .declaration(declared.ecosystem, &declared.name)
            .is_some();
        let existed_after = after
            .declaration(declared.ecosystem, &declared.name)
            .is_some();
        let resolved_name = declared.resolved_name().to_owned();
        let resolved_somewhere = after
            .nodes
            .iter()
            .any(|node| node.ecosystem == declared.ecosystem && node.name == resolved_name);
        let (kind, detail) = match (existed_before, existed_after) {
            (false, true) if resolved_somewhere => (
                ChangeKind::RequirementChanged,
                format!(
                    "first declared here in {}, already resolved by {}",
                    join(&declared.scopes),
                    scope_of(after, declared.ecosystem, &resolved_name)
                ),
            ),
            (false, true) => (
                ChangeKind::Added,
                format!(
                    "declared in {}, resolved by no lockfile here",
                    join(&declared.scopes)
                ),
            ),
            (true, false) => (
                ChangeKind::Removed,
                format!(
                    "declared requirement removed from {}",
                    join(&declared.scopes)
                ),
            ),
            _ => (ChangeKind::RequirementChanged, declared.requirement.clone()),
        };
        push_declared(
            &mut changes,
            &mut changed_direct,
            DepChange {
                kind,
                ecosystem: declared.ecosystem,
                name: declared.name.clone(),
                before: None,
                after: None,
                attribution: Attribution::Direct,
                basis: Basis::Manifest,
                scope: declared
                    .scopes
                    .iter()
                    .next()
                    .cloned()
                    .unwrap_or_else(|| "this repository".to_owned()),
                detail,
                asked_by: Vec::new(),
                dev: declared.kinds.iter().any(|kind| kind == "dev"),
                proof: None,
            },
        );
    }

    // ---- explained, or not ----------------------------------------------------------------------
    let mut explained = 0usize;
    let mut unexplained = 0usize;
    for change in &changes {
        if change.attribution != Attribution::Transitive {
            continue;
        }
        if change
            .asked_by
            .first()
            .is_some_and(|root| changed_direct.contains(root))
        {
            explained += 1;
        } else {
            unexplained += 1;
        }
    }

    let sources = source_delta(before, after);
    let licenses = license_delta(before, after);
    let peer_conflicts = peer_conflicts(before, after);
    let counts = count(
        &changes,
        &licenses,
        &sources,
        &peer_conflicts,
        explained,
        unexplained,
    );
    let mut notes: Vec<String> = Vec::new();
    changes.sort_by(|left, right| {
        (
            left.kind,
            left.ecosystem.as_str(),
            &left.name,
            &left.before,
            &left.after,
            &left.scope,
        )
            .cmp(&(
                right.kind,
                right.ecosystem.as_str(),
                &right.name,
                &right.before,
                &right.after,
                &right.scope,
            ))
    });
    if changes.len() > options.max_changes {
        notes.push(format!(
            "{} of {} changes were dropped from the row list; the counts still cover all of them",
            changes.len() - options.max_changes,
            changes.len()
        ));
        changes.truncate(options.max_changes);
    }
    let mut delta = DependencyDelta {
        changes,
        counts,
        graph: size(before, after),
        sources: sources.clone(),
        sources_gone: sources_gone(before, after),
        peer_conflicts,
        licenses,
        pattern: Pattern::NoChange,
        headline: String::new(),
        notes,
        complete: before.complete && after.complete,
        files: before.files + after.files,
    };
    delta.pattern = pattern(
        &delta.counts,
        delta.counts.unexplained_transitive >= options.storm_min_changes,
    );
    delta.notes.extend(before.notes.iter().cloned());
    delta.notes.extend(after.notes.iter().cloned());
    delta.headline = headline(&delta);
    delta
}

/// Push a change and remember that a direct dependency moved.
fn push_declared(
    changes: &mut Vec<DepChange>,
    changed_direct: &mut BTreeSet<String>,
    change: DepChange,
) {
    if change.attribution == Attribution::Direct {
        changed_direct.insert(change.name.clone());
    }
    changes.push(change);
}

/// The file that resolves a name, for a sentence that has to name its evidence.
fn scope_of(state: &State, ecosystem: Ecosystem, name: &str) -> String {
    state
        .nodes
        .iter()
        .find(|node| node.ecosystem == ecosystem && node.name == name)
        .map_or_else(|| "no lockfile".to_owned(), |node| node.scope.clone())
}

/// `(scope, ecosystem, name)` present on either side, in sorted order.
fn merged_scopes(before: &State, after: &State) -> Vec<(String, Ecosystem, String)> {
    let mut out: BTreeSet<(String, Ecosystem, String)> = BTreeSet::new();
    out.extend(before.scopes_and_names());
    out.extend(after.scopes_and_names());
    out.into_iter().collect()
}

/// Join a set for one line of prose, with the empty case saying so instead of printing nothing.
fn join(values: &BTreeSet<String>) -> String {
    if values.is_empty() {
        return "(none)".to_owned();
    }
    values
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(",")
}

/// The size delta, overall and per ecosystem.
fn size(before: &State, after: &State) -> SizeDelta {
    let mut per_ecosystem: BTreeMap<String, Size> = BTreeMap::new();
    for (ecosystem, count) in before.by_ecosystem() {
        per_ecosystem.entry(ecosystem).or_default().before += count;
    }
    for (ecosystem, count) in after.by_ecosystem() {
        per_ecosystem.entry(ecosystem).or_default().after += count;
    }
    for entry in per_ecosystem.values_mut() {
        entry.net = entry.after as i64 - entry.before as i64;
    }
    SizeDelta {
        total: Size {
            before: before.len(),
            after: after.len(),
            net: after.len() as i64 - before.len() as i64,
        },
        per_ecosystem,
    }
}

/// A source's identity, which ignores the ref: a git package moving commit is not a new source.
fn identity(source: &str) -> String {
    match source.split_once(':') {
        Some(("git", rest)) => format!("git:{}", rest.split('@').next().unwrap_or(rest)),
        _ => source.to_owned(),
    }
}

fn kind_of(source: &str) -> String {
    source
        .split_once(':')
        .map_or_else(|| source.to_owned(), |(kind, _)| kind.to_owned())
}

/// Sources present on one side and not the other.
fn source_delta(before: &State, after: &State) -> Vec<NewSource> {
    let before_set: BTreeSet<String> = before
        .nodes
        .iter()
        .flat_map(|node| node.sources.iter())
        .map(|source| identity(source))
        .collect();
    let mut added: BTreeMap<String, NewSource> = BTreeMap::new();
    for node in &after.nodes {
        for source in &node.sources {
            let key = identity(source);
            if before_set.contains(&key) {
                continue;
            }
            let entry = added.entry(key.clone()).or_insert_with(|| NewSource {
                identity: key,
                kind: kind_of(source),
                via: None,
                ecosystem: node.ecosystem,
                proof: None,
            });
            if entry.via.is_none() {
                entry.via = Some(package(&node.name, &node.version));
            }
        }
    }
    let mut out: Vec<NewSource> = added.into_values().collect();
    out.sort_by(|left, right| {
        (&left.identity, left.ecosystem.as_str()).cmp(&(&right.identity, right.ecosystem.as_str()))
    });
    out
}

/// Sources that were there before and are not now.
fn sources_gone(before: &State, after: &State) -> Vec<String> {
    let after_set: BTreeSet<String> = after
        .nodes
        .iter()
        .flat_map(|node| node.sources.iter())
        .map(|source| identity(source))
        .collect();
    let mut gone: BTreeSet<String> = BTreeSet::new();
    for node in &before.nodes {
        for source in &node.sources {
            let key = identity(source);
            if !after_set.contains(&key) {
                gone.insert(key);
            }
        }
    }
    gone.into_iter().collect()
}

/// The licence set delta, with one carrying package per licence.
fn license_delta(before: &State, after: &State) -> LicenseDelta {
    let mut before_set: BTreeMap<String, (String, Ecosystem)> = BTreeMap::new();
    let mut after_set: BTreeMap<String, (String, Ecosystem)> = BTreeMap::new();
    for node in &before.nodes {
        for license in &node.license {
            before_set
                .entry(license.clone())
                .or_insert_with(|| (package(&node.name, &node.version), node.ecosystem));
        }
    }
    for node in &after.nodes {
        for license in &node.license {
            after_set
                .entry(license.clone())
                .or_insert_with(|| (package(&node.name, &node.version), node.ecosystem));
        }
    }
    let mut delta = LicenseDelta::default();
    for (license, (package, ecosystem)) in &after_set {
        if !before_set.contains_key(license) {
            delta.added.push(LicenseSeen {
                license: license.clone(),
                package: package.clone(),
                ecosystem: *ecosystem,
            });
        }
    }
    for (license, (package, ecosystem)) in &before_set {
        if !after_set.contains_key(license) {
            delta.removed.push(LicenseSeen {
                license: license.clone(),
                package: package.clone(),
                ecosystem: *ecosystem,
            });
        }
    }
    delta
}

/// The peer-dependency conflicts one state contradicts.
///
/// A peer dependency nothing installed is normal for a library — the consumer supplies it — so absence is
/// not a conflict. Only a resolved version that provably sits outside the written range is one, and a
/// range this module cannot read ([`crate::deps::version`]) never accuses anybody.
fn conflicts(state: &State) -> Vec<Conflict> {
    let mut out: Vec<Conflict> = Vec::new();
    for declared in state.declared.iter().filter(|declared| declared.is_peer) {
        let peer = declared.resolved_name().to_owned();
        let versions: Vec<String> = state
            .nodes
            .iter()
            .filter(|node| node.ecosystem == declared.ecosystem && node.name == peer)
            .map(|node| node.version.clone())
            .collect();
        if versions.is_empty() {
            continue;
        }
        for requirement in declared.requirement.split(" | ") {
            let verdicts: Vec<Satisfies> = versions
                .iter()
                .map(|version| version::satisfies(requirement, version))
                .collect();
            if verdicts.contains(&Satisfies::Yes) || verdicts.contains(&Satisfies::Unknown) {
                continue;
            }
            out.push(Conflict {
                ecosystem: declared.ecosystem,
                peer: peer.clone(),
                required: requirement.to_owned(),
                resolved: versions[0].clone(),
                declared_by: declared.member.clone(),
            });
        }
    }
    out
}

/// Conflicts this episode introduced: the same triple must not have existed before.
fn peer_conflicts(before: &State, after: &State) -> Vec<PeerConflict> {
    let existed: BTreeSet<(String, String, String)> =
        conflicts(before).iter().map(Conflict::key).collect();
    let mut out: Vec<PeerConflict> = Vec::new();
    for conflict in conflicts(after) {
        if existed.contains(&conflict.key()) {
            continue;
        }
        out.push(PeerConflict {
            ecosystem: conflict.ecosystem,
            declared_by: conflict.declared_by.clone(),
            peer: conflict.peer.clone(),
            required: conflict.required.clone(),
            resolved: conflict.resolved.clone(),
            detail: format!(
                "{} requires {peer} {required}; the graph resolves {resolved}",
                conflict.declared_by.as_deref().unwrap_or("this package"),
                peer = conflict.peer,
                required = conflict.required,
                resolved = conflict.resolved
            ),
            proof: None,
        });
    }
    out.sort_by(|left, right| {
        (
            left.ecosystem.as_str(),
            &left.peer,
            &left.required,
            &left.resolved,
        )
            .cmp(&(
                right.ecosystem.as_str(),
                &right.peer,
                &right.required,
                &right.resolved,
            ))
    });
    out
}

/// Fold the rows into counts. Recomputed after the row cap so the two cannot disagree silently — the cap
/// notes that it dropped rows rather than quietly lowering a total.
fn count(
    changes: &[DepChange],
    licenses: &LicenseDelta,
    sources: &[NewSource],
    peers: &[PeerConflict],
    explained: usize,
    unexplained: usize,
) -> Counts {
    let mut counts = Counts {
        explained_transitive: explained,
        unexplained_transitive: unexplained,
        licenses_added: licenses.added.len(),
        licenses_removed: licenses.removed.len(),
        new_sources: sources.len(),
        peer_conflicts: peers.len(),
        ..Counts::default()
    };
    // One row per kind, one column per attribution: the table is the count, so a row cannot be missed the
    // way a long `match` with four arms per kind misses one.
    let mut rows: BTreeMap<ChangeKind, [usize; 4]> = BTreeMap::new();
    let mut downgrades = 0usize;
    for change in changes {
        rows.entry(change.kind).or_insert([0; 4])[change.attribution.index()] += 1;
        if change.kind == ChangeKind::Downgraded {
            downgrades += 1;
        }
    }
    counts.downgrades = downgrades;
    let at = |kind: ChangeKind, attribution: Attribution| {
        rows.get(&kind).map_or(0, |row| row[attribution.index()])
    };
    counts.direct_added = at(ChangeKind::Added, Attribution::Direct);
    counts.transitive_added = at(ChangeKind::Added, Attribution::Transitive);
    counts.local_changed += at(ChangeKind::Added, Attribution::Local);
    counts.direct_removed = at(ChangeKind::Removed, Attribution::Direct);
    counts.transitive_removed = at(ChangeKind::Removed, Attribution::Transitive);
    counts.local_changed += at(ChangeKind::Removed, Attribution::Local);
    counts.direct_upgraded = at(ChangeKind::Upgraded, Attribution::Direct);
    counts.transitive_upgraded = at(ChangeKind::Upgraded, Attribution::Transitive);
    counts.local_changed += at(ChangeKind::Upgraded, Attribution::Local);
    counts.direct_downgraded = at(ChangeKind::Downgraded, Attribution::Direct);
    counts.transitive_downgraded = at(ChangeKind::Downgraded, Attribution::Transitive);
    counts.local_changed += at(ChangeKind::Downgraded, Attribution::Local);
    counts.direct_unordered = at(ChangeKind::VersionChanged, Attribution::Direct);
    counts.transitive_unordered = at(ChangeKind::VersionChanged, Attribution::Transitive);
    counts.local_changed += at(ChangeKind::VersionChanged, Attribution::Local);
    counts.unattributed = [
        ChangeKind::Added,
        ChangeKind::Removed,
        ChangeKind::Upgraded,
        ChangeKind::Downgraded,
        ChangeKind::VersionChanged,
    ]
    .iter()
    .map(|kind| at(*kind, Attribution::Unknown))
    .sum();
    counts.source_switches = at(ChangeKind::SourceSwitch, Attribution::Direct)
        + at(ChangeKind::SourceSwitch, Attribution::Transitive)
        + at(ChangeKind::SourceSwitch, Attribution::Local)
        + at(ChangeKind::SourceSwitch, Attribution::Unknown);
    counts.integrity_changes = at(ChangeKind::IntegrityChange, Attribution::Direct)
        + at(ChangeKind::IntegrityChange, Attribution::Transitive)
        + at(ChangeKind::IntegrityChange, Attribution::Local)
        + at(ChangeKind::IntegrityChange, Attribution::Unknown);
    counts.license_changes = at(ChangeKind::LicenseChanged, Attribution::Direct)
        + at(ChangeKind::LicenseChanged, Attribution::Transitive)
        + at(ChangeKind::LicenseChanged, Attribution::Local)
        + at(ChangeKind::LicenseChanged, Attribution::Unknown);
    counts.direct_requalified = at(ChangeKind::RequirementChanged, Attribution::Direct)
        + at(ChangeKind::RequirementChanged, Attribution::Transitive)
        + at(ChangeKind::RequirementChanged, Attribution::Local)
        + at(ChangeKind::RequirementChanged, Attribution::Unknown);
    counts
}

/// Classify the episode from its counts. See [`Pattern`].
fn pattern(counts: &Counts, mass: bool) -> Pattern {
    let direct = counts.direct();
    let transitive = counts.transitive();
    if direct + transitive + counts.unattributed == 0 {
        return if counts.source_switches + counts.integrity_changes + counts.license_changes > 0
            || counts.licenses_added + counts.licenses_removed > 0
            || counts.new_sources > 0
        {
            Pattern::ProvenanceOnly
        } else {
            Pattern::NoChange
        };
    }
    if direct > 0 && mass {
        return Pattern::Mixed {
            direct,
            transitive: counts.unexplained_transitive,
        };
    }
    if transitive > 0 && direct == 0 {
        return Pattern::TransitiveRefresh {
            changed: transitive,
            mass,
        };
    }
    if counts.direct_added > 0 {
        return Pattern::NewDirect {
            added: counts.direct_added,
        };
    }
    if counts.direct_removed > 0 {
        return Pattern::RemovedDirect {
            removed: counts.direct_removed,
        };
    }
    if transitive == 0 && counts.unattributed > 0 {
        return Pattern::Unattributed;
    }
    Pattern::DirectVersionChange { changed: direct }
}

/// The one sentence, built from the counts so it cannot disagree with the rows above it.
fn headline(delta: &DependencyDelta) -> String {
    if delta.pattern == Pattern::NoChange {
        return "no dependency change".to_owned();
    }
    let counts = &delta.counts;
    let example = |attribution: Attribution, kind: ChangeKind| -> Vec<String> {
        let mut out: Vec<String> = delta
            .changes
            .iter()
            .filter(|change| change.attribution == attribution && change.kind == kind)
            .map(|change| match (&change.before, &change.after) {
                (Some(from), Some(to)) if from != to => format!("{} {from} → {to}", change.name),
                (Some(from), _) => package(&change.name, from),
                (None, Some(to)) => package(&change.name, to),
                (None, None) => change.name.clone(),
            })
            .collect();
        out.sort();
        out
    };
    let listed = |list: &[String]| -> String {
        let shown: Vec<&str> = list.iter().take(3).map(String::as_str).collect();
        let mut text = shown.join(", ");
        if list.len() > shown.len() {
            text.push_str(&format!(" +{}", list.len() - shown.len()));
        }
        text
    };
    let noun = |count: usize, one: &str, many: &str| -> String {
        format!("{count} {}", if count == 1 { one } else { many })
    };
    let mut parts: Vec<String> = Vec::new();
    let net = delta.graph.total.net;
    let direct_new = example(Attribution::Direct, ChangeKind::Added);
    let direct_gone = example(Attribution::Direct, ChangeKind::Removed);
    let plural = |one: usize| if one == 1 { "y" } else { "ies" };
    if counts.direct_added > 0 {
        let mut clause = if net != 0 {
            format!(
                "net {net:+} packages from {} new direct dependenc{}",
                counts.direct_added,
                plural(counts.direct_added)
            )
        } else {
            format!(
                "{} new direct dependenc{}",
                counts.direct_added,
                plural(counts.direct_added)
            )
        };
        if !direct_new.is_empty() {
            clause.push_str(&format!(" ({})", listed(&direct_new)));
        }
        parts.push(clause);
    } else if net != 0 {
        parts.push(format!("net {net:+} packages"));
    }
    if counts.direct_removed > 0 {
        parts.push(format!(
            "{} direct dependenc{} removed{}",
            counts.direct_removed,
            plural(counts.direct_removed),
            if direct_gone.is_empty() {
                String::new()
            } else {
                format!(" ({})", listed(&direct_gone))
            }
        ));
    }
    if counts.direct_upgraded > 0 {
        parts.push(format!(
            "{} direct dependenc{} upgraded",
            counts.direct_upgraded,
            plural(counts.direct_upgraded)
        ));
    }
    let downgrades = delta
        .changes
        .iter()
        .filter(|change| change.kind == ChangeKind::Downgraded)
        .collect::<Vec<_>>();
    if !downgrades.is_empty() {
        let names = listed(
            &downgrades
                .iter()
                .map(|change| format!("{} {}", change.name, change.after.as_deref().unwrap_or("")))
                .collect::<Vec<String>>(),
        );
        parts.push(format!(
            "{} dependenc{} downgraded ({names})",
            downgrades.len(),
            plural(downgrades.len())
        ));
    }
    match delta.pattern {
        Pattern::Mixed { transitive, .. } => parts.push(format!(
            "plus a mass refresh of {transitive} transitive packages nobody asked for"
        )),
        Pattern::TransitiveRefresh { changed, mass: true } => parts.push(format!(
            "{changed} transitive packages refreshed — a mass lockfile refresh, no direct dependency \
             changed"
        )),
        Pattern::TransitiveRefresh { changed, .. } => {
            parts.push(format!(
                "{} refreshed, transitive only",
                noun(changed, "package", "packages")
            ));
        }
        _ => {}
    }
    if counts.transitive_added + counts.transitive_removed > 0
        && !matches!(
            delta.pattern,
            Pattern::NewDirect { .. } | Pattern::TransitiveRefresh { .. } | Pattern::Mixed { .. }
        )
    {
        let mut moved = Vec::new();
        if counts.transitive_added > 0 {
            moved.push(format!(
                "{} added",
                noun(counts.transitive_added, "package", "packages")
            ));
        }
        if counts.transitive_removed > 0 {
            moved.push(format!(
                "{} gone",
                noun(counts.transitive_removed, "package", "packages")
            ));
        }
        parts.push(format!("transitive only: {}", moved.join(", ")));
    }
    if counts.source_switches > 0 {
        parts.push(format!(
            "source switch{}: {}",
            if counts.source_switches == 1 {
                ""
            } else {
                "es"
            },
            listed(
                &delta
                    .changes
                    .iter()
                    .filter(|change| change.kind == ChangeKind::SourceSwitch)
                    .map(|change| change.package())
                    .collect::<Vec<String>>()
            )
        ));
    }
    if counts.integrity_changes > 0 {
        parts.push(format!(
            "{} changed integrity",
            noun(counts.integrity_changes, "package", "packages")
        ));
    }
    if counts.new_sources > 0 {
        let _ = &noun;
        let kinds = listed(
            &delta
                .sources
                .iter()
                .map(|source| source.identity.clone())
                .collect::<Vec<_>>(),
        );
        parts.push(format!(
            "{} new package source{}: {kinds}",
            counts.new_sources,
            if counts.new_sources == 1 { "" } else { "s" }
        ));
    }
    if counts.licenses_added + counts.licenses_removed > 0 {
        let mut sides = Vec::new();
        if counts.licenses_added > 0 {
            sides.push(format!("+{}", counts.licenses_added));
        }
        if counts.licenses_removed > 0 {
            sides.push(format!("-{}", counts.licenses_removed));
        }
        parts.push(format!("licence set {}", sides.join(" / ")));
    }
    if counts.peer_conflicts > 0 {
        parts.push(format!(
            "{} new peer-dependency {}",
            counts.peer_conflicts,
            if counts.peer_conflicts == 1 {
                "conflict"
            } else {
                "conflicts"
            }
        ));
    }
    if counts.unattributed > 0 {
        parts.push(format!(
            "{} could not be attributed to a direct or transitive cause",
            noun(counts.unattributed, "package", "packages")
        ));
    }
    if parts.is_empty() {
        return "dependency changes: see the rows below".to_owned();
    }
    let text = parts.join("; ");
    let mut chars = text.chars();
    let head = chars.next().unwrap_or(' ').to_ascii_uppercase();
    format!("{head}{}.", chars.as_str())
}

/// Attach the hunk that proves each row, and say where one could not be found.
pub fn attach_proofs(model: &mut DependencyDelta, files: &BTreeMap<String, FileDiff>) {
    let missing = |path: &str| {
        files.get(path).map_or_else(
            || "no line-level diff was computed for this file".to_owned(),
            |diff| {
                if diff.notes.is_empty() {
                    "no hunk in this file's diff mentions the package".to_owned()
                } else {
                    diff.notes.join("; ")
                }
            },
        )
    };
    for change in &mut model.changes {
        let evidence = match change.kind {
            ChangeKind::Added => Evidence::added(&change.name, change.after.as_deref()),
            ChangeKind::Removed => Evidence::removed(&change.name, change.before.as_deref()),
            _ => Evidence::changed(&change.name, change.after.as_deref()),
        };
        change.proof = files
            .get(&change.scope)
            .and_then(|diff| hunk::locate(diff, evidence))
            .map(|found| Proof::found(&change.scope, found, &evidence))
            .or_else(|| Some(Proof::absent(change.scope.clone(), missing(&change.scope))));
    }
    for source in &mut model.sources {
        let Some(via) = &source.via else { continue };
        let (name, version) = via.split_once(' ').unwrap_or((via.as_str(), ""));
        let evidence = Evidence::added(name, Some(version));
        let found = files.iter().find_map(|(path, diff)| {
            hunk::locate(diff, evidence)
                .cloned()
                .map(|hunk| (path.clone(), hunk))
        });
        source.proof = found
            .map(|(path, found)| Proof::found(&path, &found, &evidence))
            .or_else(|| {
                Some(Proof::absent(
                    "(any)",
                    "no changed lockfile mentions this source",
                ))
            });
    }
    for conflict in &mut model.peer_conflicts {
        let evidence = Evidence::changed(&conflict.peer, Some(&conflict.resolved));
        let found = files.iter().find_map(|(path, diff)| {
            hunk::locate(diff, evidence)
                .cloned()
                .map(|hunk| (path.clone(), hunk))
        });
        conflict.proof = found
            .map(|(path, found)| Proof::found(&path, &found, &evidence))
            .or_else(|| {
                Some(Proof::absent(
                    "(any)",
                    "no changed lockfile mentions the peer package",
                ))
            });
    }
}

/// One side of a changed file as text: `None` when the path was not there, `Some(None)` when it was there
/// and was not text.
fn side_text(bytes: Option<&[u8]>) -> Option<Option<String>> {
    bytes.map(|bytes| {
        crate::deps::text::decode(bytes)
            .map(std::borrow::Cow::into_owned)
            .ok()
    })
}

/// Line-diff every changed dependency file, so a row can be proved.
fn diff_files<'a>(
    changes: &[scan::Change<'a>],
    options: &hunk::Options,
) -> BTreeMap<String, FileDiff> {
    let mut out: BTreeMap<String, FileDiff> = BTreeMap::new();
    // `None` means the path was not on this side; `Some(None)` means it was there and was not text.
    for change in changes {
        let (before, after) = (side_text(change.before), side_text(change.after));
        let path = change.path.to_owned();
        match (before, after) {
            (None, None) => {}
            (Some(None), Some(None)) => {
                out.insert(
                    path.clone(),
                    FileDiff::none(
                        path,
                        "neither side of this file is text, so no hunk can be shown",
                    ),
                );
            }
            (before, after) => {
                let (before, after) = (before.flatten(), after.flatten());
                out.insert(
                    path.clone(),
                    hunk::diff(&path, before.as_deref(), after.as_deref(), options),
                );
            }
        }
    }
    out
}

/// Run the whole per-episode answer over one diff's changed paths.
///
/// This is the warm path: cost is the changed paths, one blob read each, and nothing else
/// (ADR-004 §6.6 R2). The states are folded from the changed files alone, which is sound for a lockfile
/// and conservative for a manifest-only change: what cannot be seen becomes an unknown attribution and a
/// note, never a guess.
#[must_use]
pub fn episode<'a>(
    changes: impl IntoIterator<Item = scan::Change<'a>>,
    options: &Options,
) -> Episode {
    let changes: Vec<scan::Change<'a>> = changes.into_iter().collect();
    let paths = scan::delta(changes.iter().cloned(), &options.scan);
    let (before, after) = State::of_delta(&paths);
    let files = if options.prove {
        diff_files(&changes, &options.hunks)
    } else {
        BTreeMap::new()
    };
    let mut model = build(&before, &after, options);
    if options.prove {
        attach_proofs(&mut model, &files);
    }
    Episode {
        paths,
        model,
        files,
    }
}

/// The git adapter: one episode between two trees, read through `fathom-ingest`.
///
/// The cost is the changed-path walk plus one blob per changed dependency file; a commit that touches only
/// source files produces no rows and costs one tree comparison.
///
/// # Errors
///
/// As [`scan::delta_repository`]: only for failures that are not a file's fault.
pub fn episode_repository(
    repo: &fathom_ingest::repo::Repository,
    old_tree: Option<fathom_ingest::oid::Oid>,
    new_tree: Option<fathom_ingest::oid::Oid>,
    options: &Options,
) -> Result<Episode, crate::error::Error> {
    use fathom_ingest::repo::BlobRead;
    let changes = repo.changed_paths(old_tree, new_tree)?;
    let mut blobs: Vec<Blob> = Vec::new();
    for change in changes {
        let path = change.path.to_string_lossy().replace('\\', "/");
        if matches!(
            crate::deps::recognize(&path),
            crate::deps::Recognition::NotDependency
        ) {
            continue;
        }
        let kind = match change.kind {
            fathom_ingest::diff::PathChangeKind::Added => scan::ChangeKind::Added,
            fathom_ingest::diff::PathChangeKind::Deleted => scan::ChangeKind::Deleted,
            fathom_ingest::diff::PathChangeKind::ModeChanged => scan::ChangeKind::ModeOnly,
            fathom_ingest::diff::PathChangeKind::Modified
            | fathom_ingest::diff::PathChangeKind::TypeChanged => scan::ChangeKind::Modified,
        };
        let read = |entry: Option<fathom_ingest::diff::EntryRef>| {
            let entry = entry?;
            match repo
                .blob(entry.oid, options.scan.max_blob_bytes as usize)
                .ok()?
            {
                BlobRead::Present { bytes, .. } => Some(bytes),
                BlobRead::Oversized { .. } | BlobRead::WrongKind { .. } => Some(Vec::new()),
            }
        };
        blobs.push((path, read(change.old), read(change.new), kind));
    }
    let inputs = blobs
        .iter()
        .map(|(path, before, after, kind)| scan::Change {
            path,
            before: before.as_deref(),
            after: after.as_deref(),
            kind: Some(*kind),
        });
    Ok(episode(inputs, options))
}
