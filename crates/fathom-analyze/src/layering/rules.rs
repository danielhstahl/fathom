//! The declared architecture, read once and refused loudly.
//!
//! `[layers]` says which paths *are* which layer ([`crate::graph::layers::Layers`], the tags that ride
//! on every stored row). `[layering]` says what is allowed to point at what, which is the part a
//! reviewer actually wants enforced: allowed and forbidden dependencies, no upward references, no
//! cycles, no cycles inside one layer, and boundary rules of the shape *only the api crate may import
//! the db layer*.
//!
//! # Validation happens here, at load, and a refused rule is counted
//!
//! The ticket's fourth acceptance criterion is *"rules are validated at config load with actionable
//! errors, not silent no-ops"*, and a rules engine is exactly where a silent no-op is easy to write: a
//! rule naming a layer that does not exist matches nothing, fires on nothing, and looks identical to a
//! repository with no violations. So every entry is parsed into a [`Rule`] or refused, every refusal
//! says what to type instead, and [`Rules::refused`] is a number a surface can exit on rather than a
//! log line. The four cases this refuses rather than shrugs at:
//!
//! * an endpoint that names no declared layer and is not a path pattern — the typo case, with the
//!   near-miss layer names printed;
//! * `no_upward = true` with an empty `order` — an upward reference is only upward relative to a
//!   declared order, and guessing one from the `[layers]` table's file order would be an architecture
//!   invented by the tool;
//! * two rules with the same kind and the same body — the second changes nothing and would move the
//!   automatic rule name out from under any baseline pinning it;
//! * a rule label with a space in it — the label is a name that goes into a violation and a baseline key,
//!   and a prefix this label-shaped is not the author's intent swallowed into a source selector;
//! * an `allow` that has nothing to except, or that names the refusal exactly — see
//!   `Rules::check_exceptions`.
//!
//! # The rule grammar, and why it is a string list
//!
//! The config system's key kinds are scalars and string lists (
//! [`docs/concepts/config.md`](../../../../docs/concepts/config.md) §6), so a rule is one line:
//!
//! ```toml
//! [layering]
//! mode = "warn"
//! order = ["l0_vocabulary", "l1_durable_state", "l2_outside_door", "l3_analysis"]
//! no_upward = true
//! no_cycle_within_layer = true
//! forbid = ["the-long-reach: l3_analysis -> l1_durable_state"]
//! allow  = ["the-read-path: l3_analysis -> crates/fathom-store/src/read.rs"]
//! only   = ["l1_durable_state <- l5_composition"]
//! ```
//!
//! The refusal and the exception are a pair by design: the exception names one file inside the region the
//! refusal covers, which is the shape a real way-through has. `Rules::check_exceptions` decides it, one
//! endpoint at a time, and `[layers]` is what says the file is inside the layer.
//!
//! An endpoint is a **layer name**, a **path pattern** (any endpoint with a `/` or a glob character:
//! no glob character means prefix, exactly as in `[layers]`), or `*`. That is what makes the ticket's
//! boundary sentence expressible — *only the api crate may import the db layer* names a crate on one
//! side and a layer on the other:
//!
//! ```toml
//! only = ["db <- crates/api"]        # only files under crates/api may depend on anything in db
//! ```
//!
//! Several boundary rules naming the same target are **one allow-list**, not a list of separate
//! accusations: `db <- api` beside `db <- db` says the api layer and the layer itself may reach `db`,
//! and nothing else may. Reading them one at a time would have each one fire on a source the other
//! permits, which is a contradiction no author wrote and no reader could spot.
//!
//! `->` reads as "may not depend on" in `forbid` and as "may depend on" in `allow`; `<-` reads as
//! "may be reached only from" in `only`. The arrow direction always points at the *dependency*, which
//! is the one rule a reader has to hold; which list it sits in decides whether it forbids or permits.
//!
//! # Rule names are content, not position
//!
//! A violation names the rule it broke, and the ratchet baseline is keyed on that name, so a name that
//! moves when an unrelated rule is inserted above it would silently un-baseline a debt (the failure
//! mode [`crate::cycles::debt`] exists to prevent). So the automatic name is the rule's own content —
//! `forbid:l4_surfaces->l1_durable_state` — and a label (`my-name: src -> dst`) is a name the author
//! owns. Both are stable under reordering; a label is what to write when the rule will be baselined.
//!
//! # Specificity, because `allow` is an exception
//!
//! `forbid` and `allow` are matched together and the **most specific** match wins, counted as pattern
//! length with `*` worth nothing, so `l4 -> db` is excepted by `crates/door -> crates/db/store.rs`
//! and not by `* -> db`. An exact tie between a permission and a refusal resolves to the refusal:
//! silence about a permission is the more expensive mistake. Ties are refused at load when both sides
//! are the same kind and body, so the tie that survives is a real narrowing, and it is asserted in
//! `tests/layers.rs` rather than trusted.

use std::collections::BTreeMap;

use toml_edit::Item as TomlItem;
use toml_edit::Table as TomlTable;

use crate::cycles::debt::Mode;
use crate::graph::layers::{Layers, glob_match};

/// The default `layering.mode`. Same three spells as `ratchet.mode`, same meaning: what a violation
/// does. `warn` because a rule that blocks a repository's first review teaches nothing about it.
pub const DEFAULT_MODE: Mode = Mode::Warn;

/// How many violations one report lists before it says it stopped. A bound that is reached is reported
/// as reached ([`super::report::Report::truncated`]), never silently shortened.
pub const DEFAULT_MAX_VIOLATIONS: usize = 200;

/// What a rule claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RuleKind {
    /// This dependency is forbidden.
    Forbid,
    /// This dependency is permitted, as an exception to a narrower-or-broader `forbid`.
    Allow,
    /// Only the named source may reach the named destination: a boundary rule.
    Only,
    /// No module may depend on a layer declared above its own.
    NoUpward,
    /// No dependency cycle, anywhere.
    NoCycles,
    /// No dependency cycle whose members all sit in one layer.
    NoCycleWithinLayer,
}

impl RuleKind {
    /// How the left-hand side of a rule of this kind is best named in an error message.
    #[must_use]
    pub const fn source_name(self) -> &'static str {
        match self {
            RuleKind::Only => "the layer that may be reached",
            RuleKind::Allow => "the permitted source",
            _ => "the source",
        }
    }

    /// How the right-hand side of a rule of this kind is best named in an error message.
    #[must_use]
    pub const fn target_name(self) -> &'static str {
        match self {
            RuleKind::Only => "the only source allowed to reach it",
            RuleKind::Allow => "the permitted target",
            _ => "the target",
        }
    }

    /// The stored spelling, used in rule names and in the JSON form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            RuleKind::Forbid => "forbid",
            RuleKind::Allow => "allow",
            RuleKind::Only => "only",
            RuleKind::NoUpward => "no_upward",
            RuleKind::NoCycles => "no_cycles",
            RuleKind::NoCycleWithinLayer => "no_cycle_within_layer",
        }
    }

    /// Whether the rule was written as a line in one of the three lists, as opposed to switched on by
    /// a boolean. Only the written ones carry a pair of selectors.
    #[must_use]
    pub const fn is_written(self) -> bool {
        matches!(self, RuleKind::Forbid | RuleKind::Allow | RuleKind::Only)
    }

    /// The `layering.` key a rule of this kind was read from, for an error message that a human can
    /// act on without opening this file.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            RuleKind::Forbid => "layering.forbid",
            RuleKind::Allow => "layering.allow",
            RuleKind::Only => "layering.only",
            RuleKind::NoUpward => "layering.no_upward",
            RuleKind::NoCycles => "layering.no_cycles",
            RuleKind::NoCycleWithinLayer => "layering.no_cycle_within_layer",
        }
    }

    /// The one-line statement of what the rule asserts, printed in `fathom config explain` and in the
    /// report header.
    #[must_use]
    pub const fn claim(self) -> &'static str {
        match self {
            RuleKind::Forbid => "a declared forbidden dependency",
            RuleKind::Allow => "a declared exception to a forbidden dependency",
            RuleKind::Only => "a boundary: only the named source may reach the named target",
            RuleKind::NoUpward => {
                "no upward reference: an arrow must point at or below its own layer"
            }
            RuleKind::NoCycles => "no dependency cycle anywhere in the graph",
            RuleKind::NoCycleWithinLayer => "no dependency cycle inside one layer",
        }
    }
}

/// One side of a rule, as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selector {
    /// `*`: any module, whatever its layer.
    Any,
    /// A name declared in `[layers]`.
    Layer(String),
    /// A path prefix, or a glob when the pattern has a glob character.
    Path(String),
}

impl Selector {
    /// The way this selector prints inside a rule name and in a message.
    #[must_use]
    pub fn display(&self) -> String {
        match self {
            Selector::Any => "*".to_string(),
            Selector::Layer(name) => name.clone(),
            Selector::Path(pattern) => pattern.clone(),
        }
    }

    /// What this selector is worth when two rules compete: longer means narrower, and `*` is worth
    /// nothing, which is what makes a named layer beat `*` and a named file beat a named layer.
    #[must_use]
    pub fn specificity(&self) -> usize {
        match self {
            Selector::Any => 0,
            // Ordered by kind, not by length: a layer names a set the config defines and can grow a
            // member whenever someone adds a file, a plain path names one region and cannot grow, and a
            // glob is the widest thing a path can be. So a file-level exception outranks a layer-level
            // refusal (that is what an exception is for) and a glob exception cannot outrank it.
            Selector::Layer(_) => 1,
            Selector::Path(pattern) => {
                if pattern.contains(['*', '?']) {
                    2
                } else {
                    3
                }
            }
        }
    }

    /// Whether this endpoint is a path pattern rather than a layer or the wildcard.
    #[must_use]
    pub fn is_path(&self) -> bool {
        matches!(self, Selector::Path(_))
    }

    /// Whether `path`, tagged `layer`, is this endpoint.
    ///
    /// A path selector matches the same way `[layers]` matches: a pattern with no glob character is a
    /// prefix, so `crates/fathom-cli` covers everything under it and `crates/**/tests` picks out test
    /// directories at any depth.
    #[must_use]
    pub fn matches(&self, layer: &str, path: &str) -> bool {
        match self {
            Selector::Any => true,
            Selector::Layer(name) => layer == name,
            Selector::Path(pattern) => {
                if pattern.contains(['*', '?']) {
                    glob_match(pattern, path)
                } else {
                    path.starts_with(pattern.as_str())
                }
            }
        }
    }
}

/// One rule, as written or as switched on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// The name a violation carries and a baseline pins: the label if the author wrote one, otherwise
    /// the rule's own content (see this module's header).
    pub name: String,
    /// What it claims.
    pub kind: RuleKind,
    /// The source endpoint, or — for [`RuleKind::Only`] — the *only* source that may reach `dst`.
    pub src: Selector,
    /// The destination endpoint.
    pub dst: Selector,
    /// Whether the author named it. An automatic name is stable, but a rule that will be baselined
    /// should be named, so this drives a notice rather than a refusal.
    pub labelled: bool,
    /// The line the entry was read from, `0` when it was not read from a file.
    pub line: usize,
}

impl Rule {
    /// A rule as written in one of the three lists.
    #[must_use]
    pub fn written(
        kind: RuleKind,
        src: Selector,
        dst: Selector,
        label: Option<&str>,
        line: usize,
    ) -> Self {
        let labelled = label.is_some();
        let name = label.map_or_else(
            || match kind {
                RuleKind::Only => format!("only:{}<-{}", dst.display(), src.display()),
                other => format!("{}:{}->{}", other.as_str(), src.display(), dst.display()),
            },
            |label| label.to_string(),
        );
        Self {
            name,
            kind,
            src,
            dst,
            labelled,
            line,
        }
    }

    /// A built-in rule, switched on by a boolean: one name, no selectors.
    #[must_use]
    pub fn builtin(kind: RuleKind) -> Self {
        Self {
            name: kind.as_str().to_string(),
            kind,
            src: Selector::Any,
            dst: Selector::Any,
            labelled: true,
            line: 0,
        }
    }

    /// The rule as its author wrote it, minus any label — the part of the name that is the rule.
    #[must_use]
    pub fn body(&self) -> String {
        match self.kind {
            RuleKind::Only => format!("{} <- {}", self.dst.display(), self.src.display()),
            RuleKind::NoUpward | RuleKind::NoCycles | RuleKind::NoCycleWithinLayer => {
                self.kind.as_str().to_string()
            }
            _ => format!("{} -> {}", self.src.display(), self.dst.display()),
        }
    }

    /// The sentence that says which list to edit, or which key to set.
    #[must_use]
    pub fn declared_in(&self) -> String {
        if self.line == 0 {
            return self.kind.key().to_string();
        }
        format!("{} (line {})", self.kind.key(), self.line)
    }
}

/// The whole `[layering]` table, resolved against `[layers]`.
#[derive(Debug, Clone)]
pub struct Rules {
    /// What a violation does: nothing, a finding, or a failing check (exit 7).
    pub mode: Mode,
    /// The declared layer order, **bottom first** — the first entry is the layer everything may point
    /// at and nothing may be pointed at from. Required by `no_upward`.
    pub order: Vec<String>,
    /// Whether the no-upward rule is active. `false` after a refusal, so the state a surface reads is
    /// the state that was actually applied.
    pub no_upward: bool,
    /// Whether every cycle in the graph is a violation of this repository's own declaration.
    pub no_cycles: bool,
    /// Whether cycles inside one layer are.
    pub no_cycle_within_layer: bool,
    /// Path patterns whose *files* are not judged as sources: test code is allowed to reach uphill,
    /// and a rule that fires on a test double is a rule that will be switched off.
    pub ignore: Vec<String>,
    /// How many violations the report lists before it says it stopped.
    pub max_violations: usize,
    /// The written rules, in the order they were read.
    entries: Vec<Rule>,
    /// The built-ins that are switched on, so a violation can name its rule either way.
    builtins: Vec<Rule>,
    /// What was refused, and therefore is **not** in force. A surface that wants to fail on a
    /// half-read configuration exits on this number.
    pub refused: usize,
    /// What the load noticed, in the order it noticed it.
    pub notices: Vec<String>,
    /// How many `[layers]` names were available to match against, because a rule naming a layer when
    /// none exist is a different error than a typo among many.
    layers_declared: usize,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            mode: DEFAULT_MODE,
            order: Vec::new(),
            no_upward: false,
            no_cycles: false,
            no_cycle_within_layer: false,
            ignore: Vec::new(),
            max_violations: DEFAULT_MAX_VIOLATIONS,
            entries: Vec::new(),
            builtins: Vec::new(),
            refused: 0,
            notices: Vec::new(),
            layers_declared: 0,
        }
    }
}

impl Rules {
    /// Read `[layering]` out of a `fathom.toml` document, against the `[layers]` tags it will be
    /// matched with.
    ///
    /// The two tables are read together on purpose: a rule that names a layer is only as good as the
    /// `[layers]` entry behind it, and validating one without the other is how a rule becomes the
    /// silent no-op this module's header is a refusal of.
    #[must_use]
    pub fn from_repo_config_text(layers: &Layers, text: &str) -> (Self, Vec<String>) {
        let mut rules = Self {
            layers_declared: layers.len(),
            ..Self::default()
        };
        let mut notices = Vec::new();
        let document = match text.parse::<toml_edit::DocumentMut>() {
            Ok(document) => document,
            Err(err) => {
                notices.push(
                    "fathom.toml does not parse, so no layering rule was applied: \
                     declare the rules or fix the file; nothing is being enforced"
                        .to_string(),
                );
                rules.refused += 1;
                let _ = err;
                rules.notices = notices.clone();
                return (rules, notices);
            }
        };
        let Some(table) = document.get("layering").and_then(TomlItem::as_table) else {
            notices.push(
                "fathom.toml declares no [layering], so nothing is enforced: the layer tags are read \
                 and reported, and every dependency is permitted"
                    .to_string(),
            );
            rules.notices = notices.clone();
            return (rules, notices);
        };
        rules.read_scalars(table, &mut notices);
        for (key, kind) in [
            ("forbid", RuleKind::Forbid),
            ("allow", RuleKind::Allow),
            ("only", RuleKind::Only),
        ] {
            rules.read_entries(text, table, key, kind, layers, &mut notices);
        }
        rules.finish(layers, &mut notices);
        rules.notices = notices.clone();
        (rules, notices)
    }

    /// The mode, the order, the three booleans, the ignore list and the report bound.
    fn read_scalars(&mut self, table: &TomlTable, notices: &mut Vec<String>) {
        if let Some(item) = table.get("mode").and_then(TomlItem::as_value) {
            match item.as_str().and_then(Mode::parse) {
                Some(mode) => self.mode = mode,
                None => notices.push(format!(
                    "layering.mode is {:?}, which is not `off`, `warn` or `deny`; kept `{}`",
                    item.as_str().unwrap_or_default(),
                    self.mode.as_str()
                )),
            }
        }
        self.order = string_list(table, "order");
        self.ignore = string_list(table, "ignore");
        self.no_upward = boolean(table, "no_upward");
        self.no_cycles = boolean(table, "no_cycles");
        self.no_cycle_within_layer = boolean(table, "no_cycle_within_layer");
        if let Some(item) = table.get("max_violations").and_then(TomlItem::as_value) {
            match item.as_integer() {
                Some(number) if number >= 1 => {
                    self.max_violations = usize::try_from(number).unwrap_or(usize::MAX);
                }
                Some(number) => notices.push(format!(
                    "layering.max_violations is {number}, which would bound the report to nothing; \
                     kept {DEFAULT_MAX_VIOLATIONS}"
                )),
                None => notices.push(
                    "layering.max_violations is not a whole number; kept the built-in bound"
                        .to_string(),
                ),
            }
        }
    }

    /// One of the three rule lists.
    fn read_entries(
        &mut self,
        text: &str,
        table: &TomlTable,
        key: &str,
        kind: RuleKind,
        layers: &Layers,
        notices: &mut Vec<String>,
    ) {
        let Some(item) = table.get(key) else {
            return;
        };
        let line = key_line(text, key);
        let Some(list) = item.as_array() else {
            self.refused += 1;
            notices.push(format!(
                "layering.{key} (line {line}) is not a list of rules, so it enforced nothing"
            ));
            return;
        };
        for entry in list.iter() {
            let Some(text) = entry.as_str() else {
                self.refused += 1;
                notices.push(format!(
                    "layering.{key} has an entry that is not a string; it was skipped"
                ));
                continue;
            };
            match parse_rule(kind, text, layers) {
                Ok(rule) => {
                    if self
                        .entries
                        .iter()
                        .any(|seen| seen.kind == rule.kind && seen.name == rule.name)
                    {
                        self.refused += 1;
                        notices.push(format!(
                            "layering.{key} declares `{}` twice; the second one changes nothing and \
                             would move a baseline that pins it, so it was dropped",
                            rule.name
                        ));
                        continue;
                    }
                    self.entries.push(Rule { line, ..rule });
                }
                Err(problem) => {
                    self.refused += 1;
                    notices.push(format!("layering.{key} (line {line}) {problem}"));
                }
            }
        }
    }

    /// The order's validity, the built-ins, and the exceptions that except nothing.
    fn finish(&mut self, layers: &Layers, notices: &mut Vec<String>) {
        let names: Vec<String> = layers.names();
        // The order: every name must be a declared layer, and one name must not appear twice.
        let mut seen: Vec<String> = Vec::new();
        for name in std::mem::take(&mut self.order) {
            if !names.contains(&name) {
                self.refused += 1;
                notices.push(format!(
                    "layering.order names `{name}`, which [layers] does not declare; {}",
                    near(&name, &names)
                ));
                continue;
            }
            if seen.contains(&name) {
                self.refused += 1;
                notices.push(format!(
                    "layering.order names `{name}` twice; one rank each, so the second was dropped"
                ));
                continue;
            }
            seen.push(name);
        }
        self.order = seen;
        if self.no_upward && self.order.is_empty() {
            self.no_upward = false;
            self.refused += 1;
            notices.push(
                "layering.no_upward is true but layering.order names no layers, and an upward \
                 reference is only upward relative to a declared order; the rule is OFF. List the \
                 layers bottom-first, e.g. order = [\"db\", \"core\", \"api\"]."
                    .to_string(),
            );
        }
        if self.no_upward {
            let unranked: Vec<String> = names
                .iter()
                .filter(|name| !self.order.contains(name))
                .cloned()
                .collect();
            if !unranked.is_empty() {
                notices.push(format!(
                    "layering.order ranks {} of the {} declared layers; an edge into or out of {} \
                     cannot be judged for upwardness and is counted as unranked, not as allowed: {}",
                    self.order.len(),
                    names.len(),
                    if unranked.len() == 1 { "it" } else { "them" },
                    unranked.join(", ")
                ));
            }
        }
        self.builtins = Vec::new();
        for kind in [
            RuleKind::NoUpward,
            RuleKind::NoCycles,
            RuleKind::NoCycleWithinLayer,
        ] {
            let on = match kind {
                RuleKind::NoUpward => self.no_upward,
                RuleKind::NoCycles => self.no_cycles,
                RuleKind::NoCycleWithinLayer => self.no_cycle_within_layer,
                _ => false,
            };
            if on {
                self.builtins.push(Rule::builtin(kind));
            }
        }
        self.check_exceptions(layers, notices);
        for rule in self.entries.iter().filter(|rule| !rule.labelled) {
            notices.push(format!(
                "layering rule `{}` is unnamed; it will be named `{}` in a violation and in the \
                 baseline, so write a label (`my-name: {}`) if anything pins it",
                rule.kind.key(),
                rule.name,
                rule.body()
            ));
        }
    }

    /// An `allow` that no `forbid` can reach is a rule that enforces nothing — the no-op the ticket's
    /// fourth criterion names.
    fn check_exceptions(&mut self, layers: &Layers, notices: &mut Vec<String>) {
        let forbids: Vec<Rule> = self
            .entries
            .iter()
            .filter(|rule| rule.kind == RuleKind::Forbid)
            .cloned()
            .collect();
        if forbids.is_empty() {
            let dangling: Vec<String> = self
                .entries
                .iter()
                .filter(|rule| rule.kind == RuleKind::Allow)
                .map(|rule| rule.name.clone())
                .collect();
            if !dangling.is_empty() {
                self.refused += 1;
                notices.push(format!(
                    "layering.allow {} names an exception to nothing — layering.forbid is empty, so \
                     there is no rule for it to except. Put a `forbid` entry in, or drop the \
                     allow: {}",
                    if dangling.len() == 1 {
                        "names an exception to nothing"
                    } else {
                        "name exceptions to nothing"
                    },
                    dangling.join(", ")
                ));
                self.entries.retain(|rule| rule.kind != RuleKind::Allow);
            }
            return;
        }
        let mut useless: Vec<String> = Vec::new();
        for rule in self.entries.iter() {
            if rule.kind != RuleKind::Allow {
                continue;
            }
            if !forbids.iter().any(|forbid| covers(layers, forbid, rule)) {
                useless.push(rule.name.clone());
            }
        }
        if useless.is_empty() {
            return;
        }
        self.refused += 1;
        notices.push(format!(
            "layering.allow {} cannot except anything: no layering.forbid rule is at least as broad \
             as {}, so {} enforces nothing and {} dropped: {}",
            if useless.len() == 1 {
                ""
            } else {
                "some entries"
            },
            if useless.len() == 1 {
                format!("`{}`", useless[0])
            } else {
                "any of them".to_string()
            },
            if useless.len() == 1 {
                "this entry"
            } else {
                "these entries"
            },
            if useless.len() == 1 {
                "was"
            } else {
                "were"
            },
            useless
                .iter()
                .map(|name| format!("`{name}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        self.entries
            .retain(|seen| !useless.contains(&seen.name) || seen.kind != RuleKind::Allow);
    }

    /// The rules in force: the written ones and the switched-on built-ins, sorted by name so a report
    /// and a test can enumerate them without caring how the table was written.
    #[must_use]
    pub fn active(&self) -> Vec<&Rule> {
        let mut all: Vec<&Rule> = self.entries.iter().chain(self.builtins.iter()).collect();
        all.sort_by(|a, b| a.name.cmp(&b.name));
        all
    }

    /// The written rules only, in file order — what `fathom config explain` prints beside the keys.
    #[must_use]
    pub fn written(&self) -> &[Rule] {
        &self.entries
    }

    /// The rule with this name, if it is in force. A violation names a rule; a reader should be able to
    /// go and find it.
    #[must_use]
    pub fn rule(&self, name: &str) -> Option<&Rule> {
        self.entries
            .iter()
            .chain(self.builtins.iter())
            .find(|rule| rule.name == name)
    }

    /// How many rules are in force. Zero with `refused == 0` means the table was empty, which is a
    /// state, not a failure — and [`Rules::is_inert`] says it out loud.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len() + self.builtins.len()
    }

    /// Whether there is nothing to enforce. Distinct from "no violations": a repository that declared
    /// nothing and a repository that passes are two different answers.
    #[must_use]
    pub fn is_inert(&self) -> bool {
        self.mode == Mode::Off || self.is_empty()
    }

    /// How many layers were declared when these rules were read.
    #[must_use]
    pub fn layers_declared(&self) -> usize {
        self.layers_declared
    }

    /// The rank of a layer in the declared order: `0` is the bottom. `None` is *unranked*, which is a
    /// stated state and never a silent pass.
    #[must_use]
    pub fn rank(&self, layer: &str) -> Option<usize> {
        self.order.iter().position(|name| name == layer)
    }

    /// Whether a path is excluded from being judged as a dependency's source.
    #[must_use]
    pub fn ignored(&self, path: &str) -> bool {
        self.ignore
            .iter()
            .any(|pattern| Selector::Path(pattern.clone()).matches("", path))
    }

    /// The written rule that decides this edge, if one does: the most specific match among `forbid`
    /// and `allow`, with a refusal winning an exact tie.
    ///
    /// `Only` rules are not consulted here — a boundary rule can be violated *alongside* a permission,
    /// so [`Rules::boundaries`] returns them separately.
    #[must_use]
    pub fn verdict_of(&self, edge: &crate::layering::engine::Judged) -> Option<&Rule> {
        let mut best: Option<(usize, &Rule)> = None;
        for rule in self
            .entries
            .iter()
            .filter(|rule| matches!(rule.kind, RuleKind::Forbid | RuleKind::Allow))
        {
            if !rule.src.matches(edge.src_layer(), edge.src_path())
                || !rule.dst.matches(edge.dst_layer(), edge.dst_path())
            {
                continue;
            }
            let score = rule.src.specificity() + rule.dst.specificity();
            let taken = match best {
                None => true,
                // A tie goes to the refusal: excepting something by accident is the more expensive
                // mistake, and a tie between two rules of the *same* kind is refused at load.
                Some((score_best, winner)) => {
                    score > score_best
                        || (score == score_best
                            && winner.kind == RuleKind::Allow
                            && rule.kind == RuleKind::Forbid)
                }
            };
            if taken {
                best = Some((score, rule));
            }
        }
        best.map(|(_, rule)| rule)
    }

    /// The boundary rules whose *target* side matches, so the caller can check each one's source.
    pub fn boundaries(&self) -> impl Iterator<Item = &Rule> {
        self.entries
            .iter()
            .filter(|rule| rule.kind == RuleKind::Only)
    }

    /// Whether no rule is in force at all — the state a repository with no `[layering]` table is in.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The machine-readable form of what is in force: mode, order, and every rule with its name, body,
    /// declaration and claim. A report prints this so a reader can tell an unenforced rule from an
    /// enforced one without opening `fathom.toml`.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::{Value, json};
        json!({
            "mode": self.mode.as_str(),
            "order": self.order,
            "no_upward": self.no_upward,
            "no_cycles": self.no_cycles,
            "no_cycle_within_layer": self.no_cycle_within_layer,
            "ignore": self.ignore,
            "max_violations": self.max_violations,
            "inert": self.is_inert(),
            "refused": self.refused,
            "rules": self.active().iter().map(|rule| json!({
                "name": rule.name,
                "kind": rule.kind.as_str(),
                "body": rule.body(),
                "claim": rule.kind.claim(),
                "declared_in": rule.declared_in(),
                "labelled": rule.labelled,
            })).collect::<Vec<Value>>(),
        })
    }
}

/// Does the forbid cover everything the allow could except?
///
/// "Cover" is deliberately conservative: both sides of the forbid must be at least as broad as the
/// allow's, where a layer matches only itself and a path matches its own prefix set. A path selector
/// covers another when one is a prefix of the other, or when either has glob characters and proving
/// containment is not possible — in which case the allow is *kept* rather than refused, because
/// refusing a real exception is worse than keeping a useless one.
fn covers(layers: &Layers, forbid: &Rule, allow: &Rule) -> bool {
    // A refusal and its exception: the kinds are different by construction, and the comparison that
    // matters is between their *regions*.
    if forbid.kind != RuleKind::Forbid || allow.kind != RuleKind::Allow {
        return false;
    }
    // An allow that names the refusal exactly cancels it rather than narrowing it, and the engine's
    // tie-break sends an exact tie to the refusal — so it would enforce nothing either way, and saying
    // so is kinder than printing a violation the reader cannot get rid of.
    if forbid.src == allow.src && forbid.dst == allow.dst {
        return false;
    }
    broad(layers, &forbid.src, &allow.src) && broad(layers, &forbid.dst, &allow.dst)
}

/// Whether the wide side's region contains the narrow side's.
///
/// The case that matters — and the one an equality test misses — is a refusal that names a layer and an
/// exception that names one file inside it: `l3_api -> l1_store` refused, `l3_api ->
/// crates/store/read.rs` allowed. `[layers]` is what says the file is inside the layer.
fn broad(layers: &Layers, wide: &Selector, narrow: &Selector) -> bool {
    match (wide, narrow) {
        (Selector::Any, _) => true,
        (_, Selector::Any) => false,
        (Selector::Layer(left), Selector::Layer(right)) => left == right,
        (Selector::Layer(layer), Selector::Path(path)) => layers
            .patterns_for(layer)
            .iter()
            .any(|pattern| Selector::Path(pattern.clone()).matches("", path)),
        (Selector::Path(left), Selector::Path(right)) => {
            left.contains(['*', '?'])
                || right.contains(['*', '?'])
                || right.starts_with(left.as_str())
        }
        // A single path cannot cover a whole layer, and one layer cannot cover another from the config
        // alone — that would need the corpus, which is not what a config-load check has.
        (Selector::Path(_), Selector::Layer(_)) => false,
    }
}

/// Parse one rule line: an optional `label: `, then `src -> dst` (forbid/allow) or `dst <- src`
/// (only). The arrow has to agree with the list it was written in: `layering.only = ["a -> b"]` reads
/// as a boundary and means the opposite of what it says, so it is refused rather than guessed at.
fn parse_rule(kind: RuleKind, text: &str, layers: &Layers) -> Result<Rule, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("is empty".to_string());
    }
    let (label, body) = match text.split_once(": ") {
        Some((label, rest)) if !rest.is_empty() && is_label(label) => (Some(label), rest),
        // A prefix that is label-shaped except for the one thing a label may not have — a space — gets
        // told so, rather than being swallowed into the source side and reported as a bad layer name.
        Some((label, _)) if !label.is_empty() && label_shaped(label) => {
            return Err(format!(
                "`{label}` is not a rule label: use letters, digits and `.` `_` `-`, with no spaces \
                 — the label is printed in every violation and pinned by a baseline"
            ));
        }
        _ => (None, text),
    };
    let normalised = body.replace('→', " -> ").replace('←', " <- ");
    let (src, dst) = match (
        kind,
        normalised.split_once("<-"),
        normalised.split_once("->"),
    ) {
        (RuleKind::Only, Some((dst, src)), _) => (src, dst),
        (RuleKind::Only, None, Some(_)) => {
            return Err(format!(
                "`{text}` points with `->`. A boundary is written `target <- the only source that \
                 may reach it`, e.g. `db <- crates/api`"
            ));
        }
        (_, _, Some((src, dst))) => (src, dst),
        (_, Some(_), _) => {
            return Err(format!(
                "`{text}` points with `<-`. That arrow is the boundary form and belongs in \
                 `layering.only`; a {} is written `source -> target`",
                kind.as_str()
            ));
        }
        _ => {
            return Err(format!(
                "`{text}` names no arrow. Write `{} -> {}` for a {}, or `target <- source` in \
                 layering.only for a boundary; a `label: ` may go before either",
                kind.source_name(),
                kind.target_name(),
                kind.as_str()
            ));
        }
    };
    if src.trim().is_empty() || dst.trim().is_empty() {
        return Err(format!(
            "`{text}` has an empty side. Both sides name a layer, a path pattern, or `*`"
        ));
    }
    let names: Vec<String> = layers.names();
    let src = parse_selector(src.trim(), &names, text)?;
    let dst = parse_selector(dst.trim(), &names, text)?;
    Ok(Rule::written(kind, src, dst, label, 0))
}

/// One endpoint: `*`, a declared layer name, or a path pattern.
fn parse_selector(text: &str, names: &[String], whole: &str) -> Result<Selector, String> {
    if text.is_empty() {
        return Err(format!("`{whole}` has an empty side"));
    }
    if text == "*" {
        return Ok(Selector::Any);
    }
    let shaped = text.contains(['/', '\\']) || text.contains(['*', '?']);
    if names.iter().any(|name| name == text) {
        return Ok(Selector::Layer(text.to_string()));
    }
    if shaped {
        return Ok(Selector::Path(crate::graph::delta::module_key(text)));
    }
    // A bare word that is not a layer: almost always a typo, and the case a silent no-op would hide.
    Err(format!(
        "`{text}` names no layer declared in [layers] and is not a path pattern. {}",
        if names.is_empty() {
            "No [layers] table is declared, so nothing is a layer name here — declare `[layers]`, \
             or write a path pattern like `crates/api/**`"
                .to_string()
        } else {
            near(text, names)
        }
    ))
}

/// The near-miss list, printed because "no such layer" without candidates sends a reader back to
/// guessing.
fn near(text: &str, names: &[String]) -> String {
    let lower = text.to_lowercase();
    let hits: Vec<&String> = names
        .iter()
        .filter(|name| {
            let name_lower = name.to_lowercase();
            name_lower.contains(&lower) || lower.contains(&name_lower)
        })
        .collect();
    if hits.is_empty() {
        return format!(
            "the layers declared here are: {}",
            if names.is_empty() {
                "(none)".to_string()
            } else {
                names.join(", ")
            }
        );
    }
    format!(
        "did you mean {}?",
        hits.iter()
            .map(|name| format!("`{name}`"))
            .collect::<Vec<_>>()
            .join(" or ")
    )
}

/// A label is a name, not a sentence: it goes into rule keys and baselines.
fn is_label(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_lowercase() || first.is_ascii_digit())
        && text.len() <= 64
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'))
}

/// Whether a prefix reads as a label the author meant, even though it is not one: anything without an
/// arrow, a wildcard or a path separator in it.
fn label_shaped(text: &str) -> bool {
    !text.contains(['<', '>', '*', '/', ':'])
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '_' | '-'))
}

/// The line a `[layering]` key is defined on, for a notice a reader can jump to.
///
/// `toml_edit` clears an array's span once the document is built, so the offset the parser had is not
/// available here. What is available is the document, and one conservative scan finds the definition:
/// a miss prints no line, and a wrong line is never printed.
fn key_line(text: &str, key: &str) -> usize {
    let mut inside = false;
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            inside = trimmed == "[layering]" || trimmed.starts_with("[layering]\n");
            continue;
        }
        if !inside || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix(key)
            && rest.trim_start().starts_with('=')
        {
            return index + 1;
        }
    }
    0
}

/// A string list, with a notice-free read: the shape is checked by the config system's schema, and a
/// value that is not a list reads as absent here rather than as an empty list.
fn string_list(table: &TomlTable, key: &str) -> Vec<String> {
    table
        .get(key)
        .and_then(TomlItem::as_array)
        .map(|array| {
            array
                .iter()
                .filter_map(|entry| entry.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn boolean(table: &TomlTable, key: &str) -> bool {
    table
        .get(key)
        .and_then(TomlItem::as_value)
        .and_then(toml_edit::Value::as_bool)
        .unwrap_or(false)
}

/// A table of the rules by name, for a caller that wants to print them beside their keys.
#[must_use]
pub fn by_name(rules: &Rules) -> BTreeMap<String, Rule> {
    rules
        .active()
        .into_iter()
        .map(|rule| (rule.name.clone(), rule.clone()))
        .collect()
}
