//! `e8.2` (`reco-l9bz.2`) — per-function cyclomatic complexity, counted the way
//! [`docs/adr/0007-complexity.md`](../../../../docs/adr/0007-complexity.md) says it is counted.
//!
//! ```text
//! a figure per named callable, and the decision points that earned it
//! ```
//!
//! The ADR decides everything this module obeys, so this page documents the *mechanisms* and
//! points at the section that decided each one rather than restating the definition — restating it
//! here would be a second copy of it, which is the failure ADR-007 §10 exists to prevent.
//!
//! * **The unit is a symbol the structure extractor already emits** with kind `fn` or `method`
//!   (D1, §3.1). The pass is driven by `Draft::symbols`, not by a second walk that decides for
//!   itself what a function is: two traversals, two opinions, ADR-001 §7.10. A nested declaration
//!   the extractor does not name is therefore not a unit, and its decision points fold into the
//!   enclosing unit exactly as §3.3 requires.
//! * **`V(G) = 1 + decision points`** over the unit's own span (D2). Every entry in §4's six lists
//!   is worth exactly one point, so a figure is a count and the `1` is the base — one path, no
//!   branches. A figure is never `0` (D2, §3.2).
//! * **Six lists, none of them invented here.** Each language module publishes its own
//!   [`Rules`] table, and every node kind in it was checked against the `node-types.json` of the
//!   exact crate `Cargo.lock` pins and against a real parse of a corpus file — the ADR's **C-KIND**,
//!   restated in §13 as this ticket's gate, and enforced by `examples/complexity_harness.rs`.
//!   Two rows of §4's tables named a node kind the pinned grammar does not emit (see
//!   [`RUST_GUARD_AND_LET_ELSE_NOTE`] below); the rules they encode are implemented and counted, and
//!   the spelling is recorded in the corpus report rather than passed over.
//! * **Unknown is unknown, with a reason** (D12, §7). A file this build could not read, or a unit
//!   whose body is not in the file, has `figure: None` beside an [`UnavailableReason`] from the one
//!   shared vocabulary. `0` is reserved for the two zeros that are facts, and
//!   [`UnavailableReason::is_measured_zero`] is what distinguishes them.
//! * **A spent bound is a stated state, never a bare number** (§7.2). The per-unit node budget is
//!   `extract.complexity.max_nodes`; when it runs out the unit comes back `unknown` with
//!   `node_budget`, and the file's figure becomes a floor printed `~`.
//!
//! # Cost
//!
//! No second parse of a file a caller has already parsed: [`measure_tree`] takes a tree the caller
//! holds and [`measure`] owns its own. The ADR refuses a second parse pass (§12.6) because the pass
//! is one kind-test per node inside a traversal the extractor already does, and because for Ruby —
//! the one language `s1` measured an order of magnitude slower than its neighbours — a second parse
//! would double `e8.4`'s whole problem. `e8.3`'s store pass calls [`measure_tree`] from inside the
//! extractor's own scan for exactly that reason.
//!
//! # What is *not* here
//!
//! No threshold, no severity, no colour, no repository-wide total, no cross-language normalisation
//! (§8). The figures that consume this — the risk axis, the ratchet, `fathom check` — each own their
//! own threshold and print it. Nothing here aggregates past one file, because the episode roll-up is
//! a join over stored rows (ADR-007 §9.2), not a second implementation of the same arithmetic.

use crate::structure::engine::{self, Draft};
use crate::structure::model::{Language, Symbol, SymbolKind, UnavailableReason};
use crate::structure::policy::ExtractPolicy;
use crate::structure::util::{self, all_kids};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::{Node, Parser, Tree};

/// The basis token of every figure this module produces (ADR-007 §9.5).
///
/// Any change to §4's six lists, to §3's unit rule or to §3.4's arm rule bumps it — in the same
/// commit as the change, with a row in the ADR's amendment note saying which figures move. A figure
/// is only comparable with a figure of the same token, which is what makes an old stored figure
/// honest rather than silently wrong.
pub const BASIS: &str = "mccabe-1";

/// `V(G)`'s base: a body with no decision points has exactly one path (ADR-007 D2).
///
/// It is a *measured* one, not a filler: printing `?` for a one-line function would be the same
/// lie as printing `0` for an unreadable one, pointed the other way.
pub const BASE: usize = 1;

/// The record of ADR-007 §4.1's two node-kind spellings that the pinned grammar does not emit.
///
/// The ADR's own §11.1 audit was written to catch exactly this disease in `e3.1`'s proxy, and it
/// caught two rows in the ADR's own table on the way out:
///
/// * an arm guard is not an anonymous `if` child of `match_arm` — `tree-sitter-rust` 0.24.2 puts
///   the guard in `match_pattern`'s `condition` field;
/// * a `let … else` does not produce a `let_condition` under a `let_declaration` — the `else` arm
///   is `let_declaration`'s `alternative` field, and `let_condition` appears only in
///   `if let` / `while let`, where the ADR counts the test on the `if`/`while` anyway.
///
/// Both *rules* are implemented and both count what the ADR says they count, so the basis token
/// stands: what §9.5 bumps is a change to what is counted, not a correction to which node the
/// counter reads. The corpus exercises both (`rust/awkward.rs`), and the corpus report names both
/// under `grammar_spelling` so the correction is a committed fact rather than a code comment.
pub const RUST_GUARD_AND_LET_ELSE_NOTE: &str = concat!(
    "ADR-007 §4.1 names an arm guard as `the anonymous if child of a match_arm` and `let … else` as ",
    "`let_condition belonging to a let_declaration`. tree-sitter-rust 0.24.2 emits neither: the ",
    "guard is `match_pattern`'s `condition` field and a `let … else` is a `let_declaration` with an ",
    "`alternative`. Both rules are counted on those real nodes, so every figure is what §4.1 says it ",
    "is, and `mccabe-1` still names it."
);

/// The scope marks that make a figure a floor, verbatim from `e3.1`'s `synthetic::*` (ADR-007 §7.3).
///
/// These are the marks that mean *a program wrote part of this scope, so a branch may live in the
/// part this file does not contain*. Two of the ten shipped marks are deliberately absent:
/// `wildcard_reexport` and `ambient_merge` say another *file* adds to this surface, which is a
/// completeness caveat on the file's export list and not a claim that a branch is missing from a
/// body — ADR-007 §4.3 says so about `ambient_merge` in as many words, and the same reasoning covers
/// the barrel. They are reported as file completeness notes instead.
pub const FLOOR_MARKS: &[&str] = &[
    crate::structure::model::synthetic::MACRO_DEFINITION,
    crate::structure::model::synthetic::MACRO_INVOCATION,
    crate::structure::model::synthetic::DECORATED,
    crate::structure::model::synthetic::MONKEYPATCH,
    crate::structure::model::synthetic::DYNAMIC_METHOD,
    crate::structure::model::synthetic::SYNTHESISED_ACCESSORS,
    crate::structure::model::synthetic::BUILD_CONSTRAINT,
    crate::structure::model::synthetic::UNKNOWN_SHAPE,
];

/// The sentence a floor carries (ADR-007 §7.1's last row).
pub const FLOOR_SENTENCE: &str =
    "a macro, decorator or the runtime may add or hide branches here — this figure may be a floor";

/// The three states ADR-007 §5.1 gives a figure at every level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Everything in scope was counted. Prints the number.
    Measured,
    /// Something in scope was not counted and something was. Prints `~n (k unmeasured)`.
    Partial,
    /// Nothing in scope could be counted. Prints `?` and a reason that arrives with it.
    Unavailable,
}

impl State {
    /// The stable string, which is also the JSON spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            State::Measured => "measured",
            State::Partial => "partial",
            State::Unavailable => "unavailable",
        }
    }
}

/// One decision point, as counted.
///
/// A figure is defensible line by line or it is not a figure, so this carries the line the point is
/// on and the grammar's own spelling of the node that earned it. Where the rule reads an operator
/// rather than a node (`&&`, `||`, `??`, `and`, `or`, `&.`) the kind *is* the operator spelling,
/// which is how the grammar spells it too — an anonymous token's `kind()` is its text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionPoint {
    /// The node kind, or the operator token, exactly as the pinned grammar emits it.
    pub kind: String,
    /// Which row of ADR-007 §4 this is, in a few words: `if / else if`, `match arm`, `arm guard`.
    pub rule: String,
    /// 1-based line the point sits on.
    pub line: usize,
}

impl DecisionPoint {
    fn new(kind: &str, rule: &str, line: usize) -> DecisionPoint {
        DecisionPoint {
            kind: kind.to_string(),
            rule: rule.to_string(),
            line,
        }
    }
}

/// One unit — one named callable — and its figure (ADR-007 §3.1).
///
/// `name` is the dotted name the *structure extractor* gives the symbol, so a complexity figure can
/// be traced to a graph node, a store row and a diff-tree leaf without a translation table (D1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitComplexity {
    /// Dotted from the file's own top level: `Shape.area`, `classify`, `Demo.run`.
    pub name: String,
    /// `fn` or `method` — the extractor's own two callable kinds, not a second vocabulary.
    pub kind: String,
    /// The unit's span, from the extractor's symbol.
    pub span: crate::structure::model::Span,
    /// `V(G)`. `None` is *unknown*: the reason beside it says why, and `0` is never the figure of a
    /// unit (D2). `Some` figures are always `>= 1`.
    pub figure: Option<usize>,
    /// Set exactly when `figure` is `None`. A unit's own reason is one of `body_not_visible` or
    /// `node_budget`; a file-level reason lives on [`FileComplexity::reason`].
    pub reason: Option<UnavailableReason>,
    /// The points that earned the figure, in source order. Empty and `figure: Some(1)` is a body
    /// with no forks, which is a measurement.
    pub points: Vec<DecisionPoint>,
    /// A scope mark or a spent bound makes this figure a floor: the true value may be higher and
    /// this build cannot enumerate the difference (ADR-007 §7.3).
    pub floor: bool,
    /// The `synthetic::*` marks that apply to this unit's scope, sorted and deduplicated.
    pub marks: Vec<String>,
}

impl UnitComplexity {
    /// This unit's state: unavailable when it has no figure, partial when it is a floor.
    pub fn state(&self) -> State {
        match (self.figure, self.floor) {
            (None, _) => State::Unavailable,
            (Some(_), true) => State::Partial,
            (Some(_), false) => State::Measured,
        }
    }

    /// The figure as printed: `8`, `~8` or `?`. Never `-1`, never a bare `0` for an unknown
    /// (ADR-007 §10.1).
    pub fn printed(&self) -> String {
        match (self.figure, self.floor) {
            (Some(n), true) => format!("~{n}"),
            (Some(n), false) => n.to_string(),
            (None, _) => "?".to_string(),
        }
    }

    /// The reason sentence, when there is one: the unit's own, or the floor sentence, or nothing.
    pub fn sentence(&self) -> String {
        match &self.reason {
            Some(reason) => reason.phrase("complexity"),
            None if self.floor => FLOOR_SENTENCE.to_string(),
            None => String::new(),
        }
    }
}

/// One file at one revision: the units, and the two roll-ups ADR-007 D7 allows.
///
/// `sum` answers *what does this file ask a reader to hold*; `max` answers *what is the worst thing
/// in it*. There is no mean, and no `max` that is secretly a sum (D7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileComplexity {
    /// The path the source came from, verbatim.
    pub path: String,
    /// What [`Language::detect`] made of the extension.
    pub language: Language,
    /// Always [`BASIS`] for a figure this build produced. A stored figure may name an older token,
    /// and two figures of different tokens are not subtractable (ADR-007 §9.5).
    pub basis: String,
    /// The units, in source order.
    pub units: Vec<UnitComplexity>,
    /// Σ over the units that had a figure. `None` means nothing in the file was countable — which is
    /// `?`, not `0`.
    pub sum: Option<usize>,
    /// The largest single `V(G)` in the file. `None` with `sum: Some(0)` is the one shape of that:
    /// a file that declares no callables at all.
    pub max: Option<usize>,
    /// How many units produced a figure.
    pub measured: usize,
    /// How many did not, and each of them names why on itself.
    pub unmeasured: usize,
    /// How many units are floors (a mark or a spent bound).
    pub floors: usize,
    /// A file-level reason: why the whole file has no figure (`too_large`, `not_utf8`,
    /// `language_unknown`, `grammar_not_compiled`, `parse_failed`, …) or [`UnavailableReason::NoBodies`]
    /// when the zero is a fact.
    pub reason: Option<UnavailableReason>,
    /// Marks on the file's own scope, or on any scope inside it, sorted and deduplicated.
    pub marks: Vec<String>,
}

impl FileComplexity {
    /// The state of the file's figure (ADR-007 §5.1's table).
    pub fn state(&self) -> State {
        if self.units.is_empty() {
            // A `0` with a reason that is a fact is a measurement; anything else at this level is
            // an unknown, and `?` is the only honest glyph for it.
            return match &self.reason {
                Some(reason) if reason.is_measured_zero() => State::Measured,
                _ => State::Unavailable,
            };
        }
        if self.measured == 0 {
            return State::Unavailable;
        }
        if self.unmeasured > 0 || self.floors > 0 {
            return State::Partial;
        }
        State::Measured
    }

    /// The figure as printed for a reviewer: `41`, `~41 (3 unmeasured)`, or `? <sentence>`.
    ///
    /// The count of what was *not* read always travels beside the floor and is never folded into it
    /// (ADR-007 §5.1: adding the unread as zero is the specific act ADR-001 §7.7 forbids).
    pub fn printed(&self) -> String {
        match self.state() {
            State::Unavailable => match &self.reason {
                Some(reason) if !reason.phrase("complexity").is_empty() => {
                    format!("? {}", reason.phrase("complexity"))
                }
                _ => "?".to_string(),
            },
            State::Measured if self.units.is_empty() => "0".to_string(),
            State::Measured => self
                .sum
                .map_or_else(|| "?".to_string(), |sum| sum.to_string()),
            State::Partial => format!(
                "~{} ({} unmeasured)",
                self.sum.unwrap_or(0),
                self.unmeasured
            ),
        }
    }

    /// The `{n} of {m}` sentence ADR-007 §7.1 gives for uncounted units, when there are any.
    ///
    /// The reason codes are worded per unit; the counts live here, so this is where the ADR's
    /// `{n} of {m} functions …` phrasing is assembled.
    pub fn unmeasured_sentence(&self) -> String {
        let named = |code: &str| {
            self.units
                .iter()
                .filter(|u| u.reason.as_ref().is_some_and(|r| r.as_str() == code))
                .count()
        };
        let mut parts = Vec::new();
        let bodies = named("body_not_visible");
        if bodies > 0 {
            parts.push(format!(
                "{bodies} of {} functions have no body in this file (declared elsewhere, e.g. a \
                 trait signature)",
                self.units.len()
            ));
        }
        let budget = named("node_budget");
        if budget > 0 {
            parts.push(format!(
                "{budget} of {} functions were too large to count within \
                 `extract.complexity.max_nodes`",
                self.units.len()
            ));
        }
        parts.join("; ")
    }

    /// The plain-text per-function dump a reviewer diffs a report against (ADR-007 §9.4, D1's
    /// traceability). Deterministic by construction: same bytes, same text.
    pub fn dump(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("file      {}\n", self.path));
        out.push_str(&format!("language  {}\n", self.language.as_str()));
        out.push_str(&format!("basis     {}\n", self.basis));
        out.push_str(&format!("state     {}\n", self.state().as_str()));
        out.push_str(&format!("figure    {}\n", self.printed()));
        out.push_str(&format!(
            "sum       {}\n",
            self.sum.map_or_else(|| "?".to_string(), |n| n.to_string())
        ));
        out.push_str(&format!(
            "max       {}\n",
            self.max.map_or_else(|| "-".to_string(), |n| n.to_string())
        ));
        out.push_str(&format!(
            "units     {} measured, {} unmeasured, {} floor\n",
            self.measured, self.unmeasured, self.floors
        ));
        out.push_str(&format!(
            "marks     {}\n",
            if self.marks.is_empty() {
                "-".to_string()
            } else {
                self.marks.join(", ")
            }
        ));
        if let Some(reason) = &self.reason {
            let phrase = reason.phrase("complexity");
            out.push_str(&format!(
                "reason    {}{}\n",
                reason.as_str(),
                if phrase.is_empty() {
                    String::new()
                } else {
                    format!(" — {phrase}")
                }
            ));
        }
        let unmeasured = self.unmeasured_sentence();
        if !unmeasured.is_empty() {
            out.push_str(&format!("unmeasured {unmeasured}\n"));
        }
        for unit in &self.units {
            let header = format!(
                "{}  {:<44} L{}-L{}  V(G)={}",
                unit.kind,
                unit.name,
                unit.span.start_line,
                unit.span.end_line,
                unit.printed()
            );
            out.push('\n');
            out.push_str(&format!("{header}\n"));
            for point in &unit.points {
                out.push_str(&format!(
                    "    L{:<5} {:<24} {}\n",
                    point.line, point.kind, point.rule
                ));
            }
            let sentence = unit.sentence();
            if !sentence.is_empty() && unit.points.is_empty() {
                out.push_str(&format!("    {sentence}\n"));
            }
            if !unit.marks.is_empty() {
                out.push_str(&format!("    marks   {}\n", unit.marks.join(", ")));
            }
        }
        out
    }
}

/// The predicate half of a [`Rules`] entry: does this node earn the point?
///
/// Needed because three of the six lists have a rule that is not a bare kind test: a Rust arm guard
/// is a *field* on a node that is already a container, a Rust `let … else` is a `let_declaration`
/// with an `alternative`, and nothing else in the six needs it. A `fn` pointer, so the tables stay
/// `const` data rather than code that runs a match over kinds.
pub type Condition = fn(&Node, &[u8]) -> bool;

/// Always-true predicate, for a rule that is just the kind.
pub const ALWAYS: Condition = |_node, _src| true;

/// One language's decision-point list, as data (ADR-007 §4).
///
/// Every field is a *closed list of grammar spellings*, which is what makes C-KIND checkable: the
/// harness collects every kind named here and refuses one that the pinned grammar never emits or
/// that no corpus file produces.
pub struct Rules {
    /// Which language's list this is.
    pub language: Language,
    /// The node kinds that open a unit (§4's "Units:" line), for the report and for
    /// [`FileComplexity`]'s body test. The *set* of units comes from the extractor's symbols.
    pub units: &'static [&'static str],
    /// Node kinds that declare no body, so the unit is `unknown` with `body_not_visible` (§3.1).
    pub bodyless: &'static [&'static str],
    /// `true` for a node that is a unit *kind* but has no readable body — Java's abstract and
    /// interface methods are the case that cannot be answered from the kind alone.
    pub no_body: fn(&Node) -> bool,
    /// Named node kinds worth one point each: `(kind, rule)`.
    pub kinds: &'static [(&'static str, &'static str)],
    /// Node kinds worth one point *when the predicate holds*: `(kind, rule, predicate)`.
    pub conditions: &'static [(&'static str, &'static str, Condition)],
    /// Operator-valued rules: `(host node kind, [(operator token, rule)])`. An operator is read as
    /// an operator — a rule that counted the host would count every comparison in the file (ADR-007
    /// §4's first bullet), which is the mistake `e3.1`'s proxy made in the other direction.
    pub operators: &'static [(&'static str, &'static [(&'static str, &'static str)])],
    /// Anonymous token kinds worth one point wherever they appear: `(token, rule)`. Ruby's `&.` and
    /// TypeScript's `?.` live here: a test wearing an exit's clothes (§4's third bullet).
    pub tokens: &'static [(&'static str, &'static str)],
}

impl Rules {
    /// Every node kind and token this list counts, sorted and deduplicated — the C-KIND input.
    pub fn declared_kinds(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = self
            .units
            .iter()
            .chain(self.bodyless.iter())
            .chain(self.kinds.iter().map(|(kind, _)| kind))
            .chain(self.conditions.iter().map(|(kind, _, _)| kind))
            .chain(self.operators.iter().map(|(host, _)| host))
            .chain(self.tokens.iter().map(|(token, _)| token))
            .chain(
                self.operators
                    .iter()
                    .flat_map(|(_, ops)| ops.iter().map(|(token, _)| token)),
            )
            .copied()
            .collect::<BTreeSet<&'static str>>()
            .into_iter()
            .collect();
        out.sort_unstable();
        out
    }
}

#[cfg(feature = "lang-rust")]
pub(crate) fn rules_rust() -> &'static Rules {
    &crate::structure::rustlang::COMPLEXITY
}
#[cfg(feature = "lang-python")]
pub(crate) fn rules_python() -> &'static Rules {
    &crate::structure::pythonlang::COMPLEXITY
}
#[cfg(feature = "lang-typescript")]
pub(crate) fn rules_typescript() -> &'static Rules {
    &crate::structure::tslang::COMPLEXITY
}
#[cfg(feature = "lang-go")]
pub(crate) fn rules_go() -> &'static Rules {
    &crate::structure::golang::COMPLEXITY
}
#[cfg(feature = "lang-ruby")]
pub(crate) fn rules_ruby() -> &'static Rules {
    &crate::structure::rubylang::COMPLEXITY
}
#[cfg(feature = "lang-java")]
pub(crate) fn rules_java() -> &'static Rules {
    &crate::structure::javalang::COMPLEXITY
}

/// The decision-point list for a language, or `None` when this build has no grammar for it.
///
/// The gate is the ADR-002 §6.5 feature the grammar already ships behind: the pass adds no new
/// feature and no new grammar dependency, so a `--no-default-features` build counts nothing and
/// says which feature would (ADR-007 §7.1, `grammar_not_compiled`).
pub fn rules(language: Language) -> Option<&'static Rules> {
    match language {
        #[cfg(feature = "lang-rust")]
        Language::Rust => Some(rules_rust()),
        #[cfg(feature = "lang-python")]
        Language::Python => Some(rules_python()),
        #[cfg(feature = "lang-typescript")]
        Language::TypeScript => Some(rules_typescript()),
        #[cfg(feature = "lang-go")]
        Language::Go => Some(rules_go()),
        #[cfg(feature = "lang-ruby")]
        Language::Ruby => Some(rules_ruby()),
        #[cfg(feature = "lang-java")]
        Language::Java => Some(rules_java()),
        _ => None,
    }
}

/// The six shipped grammars, in the order the report lists them (the same order as
/// `examples/structure_harness.rs`'s table, so a reader who has seen one has seen both).
pub const LANGUAGES: [Language; 6] = [
    Language::Rust,
    Language::Python,
    Language::TypeScript,
    Language::Go,
    Language::Ruby,
    Language::Java,
];

/// Every language this build can count.
pub fn supported() -> Vec<Language> {
    LANGUAGES
        .into_iter()
        .filter(|language| rules(*language).is_some())
        .collect()
}

/// Count one file's complexity from its bytes.
///
/// This parses, so a caller that already holds a tree should call [`measure_tree`] instead — the ADR
/// refuses a second parse (§12.6) and this function is only the honest one when there is no tree to
/// borrow (a corpus harness, a one-off `fathom show` on a working copy).
pub fn measure(path: &str, text: &[u8], policy: &ExtractPolicy) -> FileComplexity {
    let language = Language::detect(path);
    let base = FileComplexity {
        path: path.to_string(),
        language,
        basis: BASIS.to_string(),
        units: Vec::new(),
        sum: None,
        max: None,
        measured: 0,
        unmeasured: 0,
        floors: 0,
        reason: None,
        marks: Vec::new(),
    };
    // The same four refusals the structural extractor makes, in the same order, so a file that has
    // no structure never acquires a complexity figure that contradicts it (ADR-007 §7.1: one
    // vocabulary, and one build's limits).
    if text.len() > policy.max_file_bytes {
        return base.refuse(UnavailableReason::TooLarge {
            limit: policy.max_file_bytes,
        });
    }
    if std::str::from_utf8(text).is_err() {
        return base.refuse(UnavailableReason::NotUtf8);
    }
    if !language.is_known() {
        return base.refuse(UnavailableReason::LanguageUnknown);
    }
    let Some(lang) = engine::lang_for(language) else {
        return base.refuse(if engine::grammar_compiled(language) {
            UnavailableReason::ExtractorNotShipped
        } else {
            UnavailableReason::GrammarNotCompiled {
                feature: language.feature().unwrap_or("langs-all").to_string(),
            }
        });
    };
    let mut parser = Parser::new();
    if parser.set_language(&(lang.grammar)()).is_err() {
        return base.refuse(UnavailableReason::Parser);
    }
    let Some(tree) = parser.parse(text, None) else {
        return base.refuse(UnavailableReason::Parser);
    };
    let root = tree.root_node();
    if util::parse_is_unusable(&root, text.len(), policy.max_complexity_nodes) {
        return base.refuse(UnavailableReason::ParseFailed {
            errors: util::error_nodes(&root, policy.max_complexity_nodes),
        });
    }
    measure_tree(&tree, text, path, policy)
}

/// Count one file's complexity from a tree the caller already parsed.
///
/// This is the entry point an index-time producer uses, because it is the one that satisfies
/// ADR-007 §12.6: one parse per file, one traversal of the tree, a kind-test per node.
pub fn measure_tree(
    tree: &Tree,
    text: &[u8],
    path: &str,
    policy: &ExtractPolicy,
) -> FileComplexity {
    let language = Language::detect(path);
    let root = tree.root_node();
    let (Some(lang), Some(rules)) = (engine::lang_for(language), rules(language)) else {
        // The tree exists, so *a* grammar was compiled; what this build cannot do is name the
        // language of this path and count it, which is the same refusal `measure` gives.
        return FileComplexity {
            path: path.to_string(),
            language,
            basis: BASIS.to_string(),
            units: Vec::new(),
            sum: None,
            max: None,
            measured: 0,
            unmeasured: 0,
            floors: 0,
            reason: Some(if engine::grammar_compiled(language) {
                UnavailableReason::ExtractorNotShipped
            } else if language.is_known() {
                UnavailableReason::GrammarNotCompiled {
                    feature: language.feature().unwrap_or("langs-all").to_string(),
                }
            } else {
                UnavailableReason::LanguageUnknown
            }),
            marks: Vec::new(),
        };
    };

    // The extractor's own scan of the same tree: that is where the unit list, the dotted names and
    // the `synthetic::*` marks come from, and using it is what makes "one reality about what a
    // function is" true rather than aspirational (ADR-007 D1).
    let mut draft = Draft::default();
    (lang.scan)(root, text, path, &mut draft, policy);

    let units = units_of(&draft, rules);
    let regions: Vec<(usize, usize)> = units
        .iter()
        .map(|unit| (unit.span.start_byte, unit.span.end_byte))
        .collect();

    // One walk: every decision point in the file, in source order, each tagged with the unit it
    // belongs to. Nodes inside no unit are module-level or container-level code and are visited and
    // discarded, exactly as §3.1 requires of a class body or a module scope.
    let budget = policy.max_complexity_nodes;
    let mut points: Vec<Vec<DecisionPoint>> = units.iter().map(|_| Vec::new()).collect();
    let mut visited: Vec<usize> = units.iter().map(|_| 0usize).collect();
    let mut truncated: Vec<bool> = units.iter().map(|_| false).collect();
    let mut stack: Vec<(Node, Option<usize>)> = Vec::new();
    let root_unit = containing(&regions, root.start_byte(), root.end_byte());
    stack.push((root, root_unit));
    while let Some((node, unit)) = stack.pop() {
        if let Some(index) = unit {
            if truncated[index] {
                // Past the bound for this body: stop descending. The figure is refused below, and a
                // figure that stopped halfway is the one silent undercount ADR-007 §7.2 refuses to
                // carry, so the walk does not pretend to have finished.
                continue;
            }
            visited[index] += 1;
            if visited[index] > budget {
                truncated[index] = true;
                continue;
            }
        }
        if let Some(index) = unit {
            count(&node, text, rules, &mut points[index]);
        }
        let mut kids = all_kids(&node);
        kids.reverse();
        for child in kids {
            // Containment is decided per child, by the whole span, every time. Inheriting the parent's
            // unit is a tempting shortcut and it is wrong: a Ruby class body starts at the same byte as
            // the `attr_accessor :name` whose span is eight bytes wide, so a start-byte-only test tags
            // the entire class body with the accessor and every branch in the class lands on the
            // accessor's figure. The corpus caught that in one run too.
            let child_unit = containing(&regions, child.start_byte(), child.end_byte());
            stack.push((child, child_unit));
        }
    }

    // The marks, and which units they reach. A mark on `Shape` floors `Shape.area`; a mark on the
    // file's own scope (`""`) floors everything in the file.
    let file_marks: Vec<String> = draft
        .synthetic
        .values()
        .flat_map(|reasons| reasons.iter())
        .filter(|reason| FLOOR_MARKS.contains(&reason.as_str()))
        .cloned()
        .collect::<BTreeSet<String>>()
        .into_iter()
        .collect();
    let body_nodes = body_nodes_of(root, rules);

    let mut out: Vec<UnitComplexity> = Vec::with_capacity(units.len());
    for (index, symbol) in units.iter().enumerate() {
        let marks = marks_for(&draft.synthetic, &symbol.name);
        let floor = marks
            .iter()
            .any(|mark| FLOOR_MARKS.contains(&mark.as_str()));
        let node = body_nodes.get(&(symbol.span.start_byte, symbol.span.end_byte));
        let bodyless =
            node.is_some_and(|node| rules.bodyless.contains(&node.kind()) || (rules.no_body)(node));
        let earned = &points[index];
        let unit = if bodyless {
            UnitComplexity {
                name: symbol.name.clone(),
                kind: symbol.kind.as_str().to_string(),
                span: symbol.span,
                figure: None,
                reason: Some(UnavailableReason::BodyNotVisible),
                points: Vec::new(),
                floor,
                marks,
            }
        } else if truncated[index] {
            UnitComplexity {
                name: symbol.name.clone(),
                kind: symbol.kind.as_str().to_string(),
                span: symbol.span,
                figure: None,
                reason: Some(UnavailableReason::NodeBudget { limit: budget }),
                points: earned.clone(),
                // `figure: None` already puts this unit in `unmeasured`; the floor flag is for a
                // figure that exists and may still be low, so it is not set here.
                floor,
                marks,
            }
        } else {
            UnitComplexity {
                name: symbol.name.clone(),
                kind: symbol.kind.as_str().to_string(),
                span: symbol.span,
                figure: Some(BASE + earned.len()),
                reason: None,
                points: earned.clone(),
                floor,
                marks,
            }
        };
        out.push(unit);
    }

    let mut report = FileComplexity {
        path: path.to_string(),
        language,
        basis: BASIS.to_string(),
        units: out,
        sum: None,
        max: None,
        measured: 0,
        unmeasured: 0,
        floors: 0,
        reason: None,
        marks: file_marks,
    };
    if report.units.is_empty() {
        // Read fine, declares nothing callable: a zero that is a fact, with its reason (D10, §6.1).
        report.reason = Some(UnavailableReason::NoBodies);
    }
    report.roll_up();
    if report.sum.is_none() && report.reason.is_none() {
        // A file whose every unit is unmeasurable has no figure, and §7.1 will not let a `?` print
        // bare. No new code is invented for it, because none is needed: the units already named why,
        // and when they all name the same thing, that is the file's reason too. A `.d.ts` in which
        // every declaration lives in another file prints `? (body_not_visible)` on the strength of
        // its units, not on the strength of a code this pass made up. Where the units disagree the
        // file stays silent and each line carries its own reason, which the corpus checker accepts
        // as the same guarantee one level down.
        report.reason = shared_reason(&report.units);
    }
    report
}

impl FileComplexity {
    /// Attach a file-level refusal.
    fn refuse(mut self, reason: UnavailableReason) -> FileComplexity {
        self.reason = Some(reason);
        self
    }

    /// Compute `sum`, `max` and the three counts. Called once, at the end of a pass.
    fn roll_up(&mut self) {
        let figures: Vec<usize> = self.units.iter().filter_map(|unit| unit.figure).collect();
        self.measured = figures.len();
        self.unmeasured = self.units.len() - figures.len();
        self.floors = self.units.iter().filter(|unit| unit.floor).count();
        if figures.is_empty() {
            self.sum = if self.units.is_empty() {
                // `no_bodies` is the zero that is a fact; any other reason at this level leaves the
                // figure unknown, and the two must not print the same way (§7.4).
                if self
                    .reason
                    .as_ref()
                    .is_some_and(UnavailableReason::is_measured_zero)
                {
                    Some(0)
                } else {
                    None
                }
            } else {
                None
            };
            self.max = None;
            return;
        }
        self.sum = Some(figures.iter().sum());
        self.max = figures.iter().max().copied();
    }
}

/// The units of a file: the extractor's `fn` and `method` symbols, with nested callables folded
/// into their enclosing unit (ADR-007 §3.3).
fn units_of(draft: &Draft, _rules: &Rules) -> Vec<Symbol> {
    let mut callables: Vec<Symbol> = draft
        .symbols
        .iter()
        .filter(|symbol| matches!(symbol.kind, SymbolKind::Fn | SymbolKind::Method))
        .cloned()
        .collect();
    callables.sort_by(|a, b| {
        (a.span.start_byte, a.span.end_byte, &a.name).cmp(&(
            b.span.start_byte,
            b.span.end_byte,
            &b.name,
        ))
    });
    // Fold: a callable strictly inside another callable's span is not a unit, and its points belong
    // to the enclosing one. Equal spans are *not* folded — Ruby's `attr_accessor :a, :b` declares
    // two named members over one node, and each of them is a real answer to "which functions are
    // here". For point assignment the first of an equal-span pair wins, so nothing is counted twice.
    let mut outer: Vec<Symbol> = Vec::with_capacity(callables.len());
    for symbol in callables {
        let nested = outer.iter().any(|keep| {
            keep.span.start_byte <= symbol.span.start_byte
                && symbol.span.end_byte <= keep.span.end_byte
                && (keep.span.start_byte, keep.span.end_byte)
                    != (symbol.span.start_byte, symbol.span.end_byte)
        });
        if !nested {
            outer.push(symbol);
        }
    }
    outer
}

/// The one reason code every unit in the list carries, if there is one.
///
/// The file-level voice of [`UnavailableReason`]: used only when nothing in the file was countable,
/// so that the file's `?` is never a bare glyph (ADR-007 §7.1).
fn shared_reason(units: &[UnitComplexity]) -> Option<UnavailableReason> {
    if units.is_empty() {
        return None;
    }
    let first = units.first()?.reason.clone()?;
    units
        .iter()
        .all(|unit| unit.reason.as_ref() == Some(&first))
        .then_some(first)
}

/// Which disjoint region wholly contains `start..end`, if any. Regions are sorted by start byte.
///
/// Wholly, not "starts inside": the regions are the units' spans, and a node that begins inside a
/// function and reaches past it is not that function's node. The first region whose start is at or
/// before `start` is the only candidate, because the outermost units' spans do not overlap.
fn containing(regions: &[(usize, usize)], start: usize, end: usize) -> Option<usize> {
    let index = regions.partition_point(|(region_start, _)| *region_start <= start);
    if index == 0 {
        return None;
    }
    let (region_start, region_end) = regions[index - 1];
    if start >= region_start && end <= region_end {
        Some(index - 1)
    } else {
        None
    }
}

/// The nodes a body test needs, keyed by the span and kind of the symbol that came from them.
fn body_nodes_of<'a>(root: Node<'a>, rules: &Rules) -> BTreeMap<(usize, usize), Node<'a>> {
    let mut out: BTreeMap<(usize, usize), Node<'a>> = BTreeMap::new();
    let wanted = rules
        .units
        .iter()
        .chain(rules.bodyless.iter())
        .copied()
        .collect::<Vec<&str>>();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        // First writer wins at an identical span, in traversal order, so the map is a function of the
        // input and of nothing else.
        if wanted.contains(&node.kind()) {
            let _ = out.insert((node.start_byte(), node.end_byte()), node);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }
    out
}

/// The marks that reach one dotted name: its own scope, any ancestor scope, or the file's own.
fn marks_for(synthetic: &BTreeMap<String, Vec<String>>, name: &str) -> Vec<String> {
    let mut scopes: Vec<String> = vec![String::new()];
    let mut prefix = String::new();
    for part in name.split('.') {
        if !prefix.is_empty() {
            prefix.push('.');
        }
        prefix.push_str(part);
        scopes.push(prefix.clone());
    }
    let mut marks: Vec<String> = Vec::new();
    for scope in &scopes {
        if let Some(reasons) = synthetic.get(scope.as_str()) {
            marks.extend(reasons.iter().cloned());
        }
    }
    marks.sort();
    marks.dedup();
    marks
}

/// Every node kind a file's parse produces, named and anonymous together.
///
/// This is the C-KIND probe, public because the thing it checks is public: ADR-007 §4's C-KIND says a
/// decision-point list may not name a kind the pinned grammar never emits, and the only evidence of
/// what a grammar emits is a real parse of a real file. A list entry that names a kind nothing emits
/// counts zero forever and prints as a confident low number — the defect §11.1 measured fourteen
/// times in `e3.1`'s proxy. The corpus harness (`examples/complexity_harness.rs`) calls this per
/// corpus file, intersects the result with [`Rules::declared_kinds`], and CI refuses the difference.
///
/// `None` when the file cannot be parsed at all (no grammar for it, in this build or in any).
#[must_use]
pub fn node_kinds(path: &str, text: &[u8]) -> Option<BTreeSet<String>> {
    let language = Language::detect(path);
    let lang = engine::lang_for(language)?;
    let mut parser = Parser::new();
    if parser.set_language(&(lang.grammar)()).is_err() {
        return None;
    }
    let tree = parser.parse(text, None)?;
    let mut out = BTreeSet::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        out.insert(node.kind().to_string());
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }
    Some(out)
}

/// The node kinds a corpus file *must* produce for a grammar's list to pass C-KIND: the declared
/// kinds that are named nodes, i.e. the ones `node-types.json` lists at its top level.
///
/// Anonymous operator tokens are excluded here because a grammar's `node-types.json` does not list
/// them as types — they are checked against the corpus instead, which is the half of C-KIND that
/// catches a kind no file ever exercises.
#[must_use]
pub fn named_declared_kinds(rules: &Rules) -> Vec<&'static str> {
    rules
        .declared_kinds()
        .into_iter()
        .filter(|kind| !kind.contains(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '.'))
        .collect()
}

/// Count the points one node earns, appending to `out` in the order the rules are listed.
fn count(node: &Node, src: &[u8], rules: &Rules, out: &mut Vec<DecisionPoint>) {
    let kind = node.kind();
    let line = node.start_position().row + 1;
    // Named kinds first, and only on named nodes. This is not tidiness: in `tree-sitter-ruby`, the
    // keyword token inside `if_modifier` has the kind string `if`, so a rule that matched anonymous
    // nodes counted Ruby's `x if y` twice — and `elsif`, `while`, `until` and `for` each arrive in
    // the file as a named node *and* an anonymous keyword of the same spelling. The corpus caught it
    // in one run, which is what a corpus is for.
    if node.is_named() {
        for (listed, rule) in rules.kinds {
            if *listed == kind {
                out.push(DecisionPoint::new(listed, rule, line));
            }
        }
        for (listed, rule, test) in rules.conditions {
            if *listed == kind && test(node, src) {
                out.push(DecisionPoint::new(listed, rule, line));
            }
        }
    }
    // The host of an operator rule is a named node; the operator itself is anonymous, and it is the
    // operator's spelling that decides, never the host (`&&`, not `binary_expression`).
    if node.is_named() {
        for (host, operators) in rules.operators {
            if *host != kind {
                continue;
            }
            for child in all_kids(node) {
                if child.is_named() {
                    continue;
                }
                for (token, rule) in operators.iter() {
                    if child.kind() == *token {
                        out.push(DecisionPoint::new(
                            token,
                            rule,
                            child.start_position().row + 1,
                        ));
                    }
                }
            }
        }
    }
    // Token rules are the anonymous half of the six lists, and the only place a token kind counts:
    // TypeScript's `?.` is a named node here but Ruby's `&.` is an anonymous token, and ADR-007 §4
    // counts each of them exactly once.
    if !node.is_named() {
        for (token, rule) in rules.tokens {
            if kind == *token {
                out.push(DecisionPoint::new(token, rule, line));
            }
        }
    }
}
