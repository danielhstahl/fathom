//! The engine: one revision's rows in, the violations out, and a stated count of what could not be
//! judged.
//!
//! [`evaluate`] is a pure function of `(graph, naming, rules, locate)`. It walks a revision's
//! dependency rows once, asks the declared rules about each, and then asks the cycle rules once about
//! the whole graph. Nothing here is incremental and nothing here pretends to be: the per-episode half is
//! [`super::report`], which pays one evaluation at each end of the episode and diffs the *violations*,
//! and `super::episode` reads the stored row ops so the second end never has to be materialised.
//!
//! # The granularity rule, which is `e0.12`'s checkpoint on this ticket
//!
//! The checkpoint is explicit: *a layering rule that fires on a file-level Go edge or an unresolved
//! Java same-package reference is a rule that fires on an artefact of the scanner*. Two consequences,
//! both enforced here rather than commented about:
//!
//! * **a package-granular edge is judged at package granularity.** A Go `imports_package` row is one
//!   file's rendering of `package A → package B`, so the endpoints become the two *directories* and the
//!   layer asked of each is the layer that directory carries. If the files of one directory do not all
//!   carry the same tag — a `[layers]` rule like `**/main.go` slicing a package in half — the layer of
//!   the package is not knowable, so the edge is counted in
//!   [`Counters::indeterminate`] and **not judged**. A rule that fired there would be reporting the
//!   shape of the glob, not the shape of the architecture.
//! * **an edge that does not exist cannot fire.** The extractor's unresolved references — a third-party
//!   base, an internal name the corpus does not contain, and the Java same-package *guesses* that did
//!   not resolve ([`crate::structure`], `extract.java.same_package_scan`) — write no row
//!   ([`crate::graph::build`]), so they are not in the input to this function at all. That is not an
//!   omission discovered later; it is the reason a Java edge that *is* here is one the extractor
//!   resolved.
//!
//! # What is not judged either
//!
//! A row with an endpoint the dictionary cannot name is counted in [`Counters::skipped_unnamed`]; a row whose
//! endpoint no `[layers]` rule claims is counted in [`Counters::untagged`] and can still be matched by
//! a path-shaped selector, which is the case an untagged repository can use before it declares any
//! layers at all. Both numbers travel in the report, because "no violations" and "nothing could be
//! judged" have to be distinguishable from the payload and not from the author's memory.

use std::collections::BTreeMap;

use crate::cycles::debt::Mode;
use crate::cycles::flow::package_of;
use crate::graph::model::{Graph, RowKind};
use crate::graph::query::{Naming, module_of};
use crate::layering::finding::{Side, Unit, Violation};
use crate::layering::locate::{Line, Locate};
use crate::layering::rules::{Rule, RuleKind, Rules};

/// What one evaluation cost, and what it could not see.
///
/// Every field is a denominator for something: `checked` is what "no violations" is a claim about, and
/// the four `could_*` fields are the reasons a claim might be thinner than it reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counters {
    /// Dependency rows examined. `contains`, `module` and `symbol` rows are not dependencies and are
    /// not counted.
    pub checked: usize,
    /// Rows a rule was actually applied to.
    pub judged: usize,
    /// Rows an `allow` rule excepted out of a `forbid`.
    pub permitted: usize,
    /// Rows skipped because the source matched `layering.ignore`.
    pub skipped_ignored: usize,
    /// Rows with an endpoint the dictionary could not name.
    pub skipped_unnamed: usize,
    /// Rows with an endpoint no `[layers]` rule claims.
    pub untagged: usize,
    /// Rows whose layer is absent from `layering.order`, while `no_upward` is on.
    pub unranked: usize,
    /// Rows whose two endpoints carry **different** layer tags.
    ///
    /// This is the denominator that tells a reader whether the declared rules had anything to disagree
    /// about. A repository whose `[layers]` entries are crate directories and whose extractor cannot
    /// place a crate-boundary import checks thousands of same-layer rows, reports zero violations, and
    /// means neither of those numbers the way a reader hears them (**reco-842**): the rows were judged,
    /// and no rule could have fired on any of them. Zero here with rules in force says that out loud.
    pub cross_layer: usize,
    /// Package-granular rows whose package has no single layer tag, so the claim was not available.
    pub indeterminate: usize,
    /// Cycles looked at, when a cycle rule was on.
    pub cycles: usize,
    /// Rules in force when this was measured.
    pub rules: usize,
    /// Rules refused at load, so a report can refuse to look clean.
    pub refused: usize,
    /// Statements whose line was located, out of the records [`Locate`] was built from.
    pub statements_located: usize,
}

impl Counters {
    /// The machine-readable form, with the denominator named rather than implied.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::json;
        json!({
            "checked": self.checked,
            "judged": self.judged,
            "permitted": self.permitted,
            "skipped_ignored": self.skipped_ignored,
            "skipped_unnamed": self.skipped_unnamed,
            "untagged": self.untagged,
            "unranked": self.unranked,
            "cross_layer": self.cross_layer,
            "indeterminate": self.indeterminate,
            "cycles": self.cycles,
            "rules": self.rules,
            "refused": self.refused,
            "statements_located": self.statements_located,
        })
    }

    /// The one sentence that says the rules had nothing to disagree about, when that is the state.
    ///
    /// [`Rules::is_inert`] covers "no rules", and [`Counters::checked`] == 0 covers "no rows". This is the
    /// third state, the one neither of them catches: rules in force, rows judged, and not one row whose
    /// endpoints sit in different layers — so the zero violations is a fact about the corpus's shape and
    /// not about the architecture holding (**reco-842**, where the whole graph of a Cargo workspace held
    /// no crate-boundary edge and the row still read clean). A surface that prints the row should print
    /// this beside it.
    #[must_use]
    pub fn blind_spot(&self) -> Option<String> {
        if self.rules > 0 && self.checked > 0 && self.cross_layer == 0 {
            return Some(
                "no row crossed a layer boundary, so every rule here ran on rows it could not have \
                 disagreed with — check that the corpus can show the edges the rules name"
                    .to_string(),
            );
        }
        None
    }

    /// The one line that says what was looked at.
    #[must_use]
    pub fn sentence(&self) -> String {
        let mut out = format!(
            "{} dependency row(s) checked, {} judged against {} rule(s), {} permitted by an exception",
            self.checked, self.judged, self.rules, self.permitted
        );
        let mut caveats: Vec<String> = Vec::new();
        if let Some(blind) = self.blind_spot() {
            caveats.push(blind);
        }
        if self.untagged > 0 {
            caveats.push(format!("{} row(s) touch an untagged module", self.untagged));
        }
        if self.unranked > 0 {
            caveats.push(format!(
                "{} row(s) involve an unranked layer",
                self.unranked
            ));
        }
        if self.indeterminate > 0 {
            caveats.push(format!(
                "{} package-granular row(s) had no single layer tag and were not judged",
                self.indeterminate
            ));
        }
        if self.skipped_unnamed > 0 {
            caveats.push(format!(
                "{} row(s) name a module the dictionary does not",
                self.skipped_unnamed
            ));
        }
        if self.skipped_ignored > 0 {
            caveats.push(format!(
                "{} row(s) came from a `layering.ignore` path",
                self.skipped_ignored
            ));
        }
        if self.refused > 0 {
            caveats.push(format!("{} rule(s) were refused at load", self.refused));
        }
        if !caveats.is_empty() {
            out.push_str(&format!("; {}", caveats.join("; ")));
        }
        out
    }
}

/// One judged dependency: the two endpoints as the extractor guarantees them, and the row that carried
/// it.
#[derive(Debug, Clone)]
pub struct Judged {
    /// The depending side.
    pub src: Side,
    /// The side depended on.
    pub dst: Side,
    /// The stored row kind.
    pub kind: &'static str,
    /// The endpoints' layer tags, kept apart from the sides for the selectors' convenience.
    tags: (String, String),
}

impl Judged {
    /// The depending side's path.
    #[must_use]
    pub fn src_path(&self) -> &str {
        &self.src.path
    }

    /// The depending side's layer tag.
    #[must_use]
    pub fn src_layer(&self) -> &str {
        &self.tags.0
    }

    /// The target's path.
    #[must_use]
    pub fn dst_path(&self) -> &str {
        &self.dst.path
    }

    /// The target's layer tag.
    #[must_use]
    pub fn dst_layer(&self) -> &str {
        &self.tags.1
    }

    /// The `file:line` pair a violation prints, given the statement evidence.
    #[must_use]
    pub fn line(&self, locate: &Locate) -> Line {
        locate.at(&self.src.path, &self.dst.path)
    }
}

/// One revision's answer, before any baseline is applied to it.
#[derive(Debug, Clone, Default)]
pub struct Assessment {
    /// Every violation found, sorted by [`Violation::key`] — including the ones a baseline will later
    /// call debt. This is "what the revision does", not "what may alarm"; the two are different
    /// questions and only [`super::debt`] is allowed to conflate them.
    pub violations: Vec<Violation>,
    /// What was looked at and what could not be.
    pub counters: Counters,
    /// What this pass refused or could not do, in the order it found it.
    pub notices: Vec<String>,
    /// Whether [`Rules::max_violations`] stopped the list. The count of *what was found* is kept in
    /// [`Assessment::found`], so a bound never silently changes a number.
    pub truncated: bool,
    /// How many violations were found, before the bound.
    pub found: usize,
}

impl Assessment {
    /// How many violations this revision shows.
    #[must_use]
    pub fn len(&self) -> usize {
        self.violations.len()
    }

    /// Whether nothing was found. Distinct from [`Rules::is_inert`]: this one means the rules ran.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.violations.is_empty()
    }
}

/// Evaluate one revision against the declared rules.
///
/// `locate` may be [`Locate::empty`] when the caller has a stored graph and not the substrate: every
/// violation then cites a file and says the line was not recorded, which is the honest degradation and
/// not a failure.
#[must_use]
pub fn evaluate(graph: &Graph, naming: &Naming, rules: &Rules, locate: &Locate) -> Assessment {
    let mut out = Assessment::default();
    out.counters.rules = rules.len();
    out.counters.refused = rules.refused;
    out.counters.statements_located = locate.located;
    if rules.mode == Mode::Off {
        out.notices.push(
            "layering.mode is `off`: the rules were read and none were applied. `off` is a choice to \
             report nothing, not a statement that the architecture is clean"
                .to_string(),
        );
        return out;
    }
    if rules.is_empty() {
        out.notices.push(
            "no layering rule is in force, so nothing was judged. A clean report from here is a \
             report about nothing"
                .to_string(),
        );
        return out;
    }
    if rules.refused > 0 {
        out.notices.push(format!(
            "{} rule(s) were refused when fathom.toml was read: this is not the architecture the file \
             asked for, and the notices name each one",
            rules.refused
        ));
    }
    let packages = package_tags(naming);
    let mut edges: Vec<Judged> = Vec::new();
    for row in graph.edges() {
        let (kind, src, dst) = *row;
        if !kind.is_dependency() {
            continue;
        }
        out.counters.checked += 1;
        let Some(judged) = judge(naming, rules, &packages, kind, src, dst, &mut out.counters)
        else {
            continue;
        };
        out.counters.judged += 1;
        edges.push(judged);
    }
    for judged in &edges {
        judge_written(rules, judged, locate, &mut out);
        judge_upward(rules, judged, locate, &mut out);
    }
    if rules.no_cycles || rules.no_cycle_within_layer {
        judge_cycles(rules, graph, naming, locate, &mut out);
    }
    out.violations.sort_by_key(Violation::key);
    out.violations.dedup_by(|a, b| a.key() == b.key());
    out.found = out.violations.len();
    if out.violations.len() > rules.max_violations {
        out.violations.truncate(rules.max_violations);
        out.truncated = true;
        out.notices.push(format!(
            "layering.max_violations = {} stopped the list; {} violation(s) were found in total",
            rules.max_violations, out.found
        ));
    }
    out
}

/// The tag of every directory, and `Split` where its files disagree.
///
/// Built once per revision because a Go edge's claim is a package's claim, and a package's layer has to
/// be *the* layer of the package or the claim is not available (`e0.12`'s checkpoint on this ticket).
/// An untagged file makes its package *untagged*, not split: an untagged repository is saying "not yet",
/// which is a different state from "my globs cut a package in half".
fn package_tags(naming: &Naming) -> BTreeMap<String, PackageTag> {
    let mut out: BTreeMap<String, PackageTag> = BTreeMap::new();
    for vertex in naming.vertices() {
        if vertex.kind != RowKind::Module {
            continue;
        }
        let package = package_of(&vertex.key).to_string();
        let entry = out.entry(package).or_insert(PackageTag::Untagged);
        if vertex.layer.is_empty() {
            continue;
        }
        match entry {
            PackageTag::Untagged => *entry = PackageTag::Known(vertex.layer.clone()),
            PackageTag::Known(seen) if *seen == vertex.layer => {}
            _ => *entry = PackageTag::Split,
        }
    }
    out
}

/// What the tags of one package add up to.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PackageTag {
    /// Every tagged file in it carries this tag.
    Known(String),
    /// Nothing in it is tagged: unjudged by a layer rule, not misjudged.
    Untagged,
    /// Its files carry more than one tag, so the package has no single layer and a package-granular
    /// edge into it cannot be judged at all.
    Split,
}

/// Turn one stored row into a [`Judged`], or count the reason it could not be one.
fn judge(
    naming: &Naming,
    rules: &Rules,
    packages: &BTreeMap<String, PackageTag>,
    kind: RowKind,
    src: i64,
    dst: i64,
    counters: &mut Counters,
) -> Option<Judged> {
    let src_key = naming.name(src)?;
    let dst_key = naming.name(dst)?;
    let src_module = module_of(src_key);
    let dst_module = module_of(dst_key);
    if rules.ignored(&src_module) {
        counters.skipped_ignored += 1;
        return None;
    }
    let package = kind == RowKind::ImportsPackage;
    let (src_path, dst_path) = if package {
        (
            package_of(&src_module).to_string(),
            package_of(&dst_module).to_string(),
        )
    } else {
        (src_module.clone(), dst_module.clone())
    };
    let src_tag = if package {
        match packages.get(&src_path) {
            Some(PackageTag::Known(tag)) => tag.clone(),
            Some(PackageTag::Untagged) | None => String::new(),
            Some(PackageTag::Split) => {
                counters.indeterminate += 1;
                return None;
            }
        }
    } else {
        tag_of(naming, &src_module, src)
    };
    let dst_tag = if package {
        match packages.get(&dst_path) {
            Some(PackageTag::Known(tag)) => tag.clone(),
            Some(PackageTag::Untagged) | None => String::new(),
            Some(PackageTag::Split) => {
                counters.indeterminate += 1;
                return None;
            }
        }
    } else {
        tag_of(naming, &dst_module, dst)
    };
    if src_tag.is_empty() || dst_tag.is_empty() {
        counters.untagged += 1;
    }
    if !src_tag.is_empty() && !dst_tag.is_empty() && src_tag != dst_tag {
        counters.cross_layer += 1;
    }
    let unit = if package { Unit::Package } else { Unit::File };
    Some(Judged {
        src: Side {
            path: src_path,
            layer: src_tag.clone(),
            unit,
        },
        dst: Side {
            path: dst_path,
            layer: dst_tag.clone(),
            unit,
        },
        kind: kind.as_str(),
        tags: (src_tag, dst_tag),
    })
}

/// The layer a module carries: its own dictionary tag, falling back to the endpoint's own tag when the
/// module node was never declared (a symbol whose module is only named, not declared).
fn tag_of(naming: &Naming, module: &str, fallback_id: i64) -> String {
    naming.id(module).map_or_else(
        || naming.layer(fallback_id).to_string(),
        |id| naming.layer(id).to_string(),
    )
}

/// The written rules: `forbid`, `allow` and the `only` boundaries.
fn judge_written(rules: &Rules, judged: &Judged, locate: &Locate, out: &mut Assessment) {
    if let Some(rule) = rules.verdict_of(judged) {
        match rule.kind {
            RuleKind::Allow => out.counters.permitted += 1,
            RuleKind::Forbid => out.violations.push(Violation::from_edge(
                rule,
                judged.src.clone(),
                judged.dst.clone(),
                judged.kind,
                judged.line(locate),
                &rules.order,
            )),
            _ => {}
        }
    }
    // Every boundary rule that speaks about this edge's target is one allow-list, not a list of
    // separate accusations: `only = ["store <- api", "store <- store"]` says two sources may reach the
    // store, and a rule that fired on a source its companion permits would be a contradiction nobody
    // could read back out of the config. The first match names the violation, so the fix hint points at
    // a rule the author actually wrote.
    let reaching: Vec<&Rule> = rules
        .boundaries()
        .filter(|rule| rule.dst.matches(judged.dst_layer(), judged.dst_path()))
        .collect();
    if !reaching.is_empty()
        && !reaching
            .iter()
            .any(|rule| rule.src.matches(judged.src_layer(), judged.src_path()))
    {
        out.violations.push(Violation::from_edge(
            reaching[0],
            judged.src.clone(),
            judged.dst.clone(),
            judged.kind,
            judged.line(locate),
            &rules.order,
        ));
    }
}

/// The no-upward rule, which needs the order and nothing else.
fn judge_upward(rules: &Rules, judged: &Judged, locate: &Locate, out: &mut Assessment) {
    if !rules.no_upward {
        return;
    }
    let (Some(from), Some(to)) = (
        rules.rank(judged.src_layer()),
        rules.rank(judged.dst_layer()),
    ) else {
        if !judged.src_layer().is_empty() && !judged.dst_layer().is_empty() {
            out.counters.unranked += 1;
        }
        return;
    };
    if from >= to {
        return;
    }
    let Some(rule) = rules.rule(RuleKind::NoUpward.as_str()) else {
        return;
    };
    out.violations.push(Violation::from_edge(
        rule,
        judged.src.clone(),
        judged.dst.clone(),
        judged.kind,
        judged.line(locate),
        &rules.order,
    ));
}

/// The cycle rules, asked once per revision.
///
/// The machinery is [`crate::cycles`] — this does not find cycles a second way, it *declares* them,
/// which is a different ticket's object: a cycle the cycle ledger already baselines is not a second
/// debt here, and the two reports are joined in [`super::debt`] rather than deduplicated by accident.
fn judge_cycles(
    rules: &Rules,
    graph: &Graph,
    naming: &Naming,
    locate: &Locate,
    out: &mut Assessment,
) {
    let found = crate::cycles::detect(graph, naming, crate::cycles::Options::default());
    out.counters.cycles = found.len();
    for cycle in &found {
        let (src, dst) = cycle_sides(cycle, naming);
        let line = locate.at(&src.path, &dst.path);
        if rules.no_cycles
            && let Some(rule) = rules.rule(RuleKind::NoCycles.as_str())
        {
            out.violations.push(Violation::from_cycle(
                rule,
                cycle,
                src.clone(),
                dst.clone(),
                line,
            ));
        }
        if rules.no_cycle_within_layer
            && cycle.members.len() > 1
            && let Some(layer) = single_layer(cycle, naming)
            && let Some(rule) = rules.rule(RuleKind::NoCycleWithinLayer.as_str())
        {
            let src = Side {
                layer: layer.clone(),
                ..src.clone()
            };
            let dst = Side {
                layer,
                ..dst.clone()
            };
            out.violations
                .push(Violation::from_cycle(rule, cycle, src, dst, line));
        }
    }
}

/// The layer every member shares, or `None` when they do not all carry one.
///
/// `Cycle::layers` lists the tags that appeared; an untagged member contributes nothing to that list,
/// so a cycle of four with one tag is *not* a within-one-layer cycle and this checks each member.
fn single_layer(cycle: &crate::cycles::Cycle, naming: &Naming) -> Option<String> {
    let mut shared: Option<String> = None;
    for member in &cycle.members {
        let id = naming.id(member)?;
        let tag = naming.layer(id);
        if tag.is_empty() {
            return None;
        }
        match &shared {
            None => shared = Some(tag.to_string()),
            Some(seen) if seen == tag => {}
            Some(_) => return None,
        }
    }
    shared
}

/// The two endpoints a cycle violation cites: the closing edge, in the cycle's own claim.
fn cycle_sides(cycle: &crate::cycles::Cycle, naming: &Naming) -> (Side, Side) {
    let unit = if cycle.claim == crate::cycles::Claim::Package {
        Unit::Package
    } else {
        Unit::File
    };
    let side = |name: &str| Side {
        path: name.to_string(),
        layer: naming
            .id(name)
            .map(|id| naming.layer(id).to_string())
            .unwrap_or_default(),
        unit,
    };
    match (cycle.path.first(), cycle.path.last()) {
        (Some(first), Some(last)) if cycle.path.len() > 1 => (side(last), side(first)),
        (Some(only), _) => (side(only), side(only)),
        (None, _) => {
            let joined = cycle.members.join("|");
            (side(&joined), side(&joined))
        }
    }
}
