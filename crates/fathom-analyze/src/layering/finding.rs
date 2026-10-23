//! The finding: one rule, one offending edge, one fix, and the reason they belong together.
//!
//! The ticket's third acceptance criterion is *"each violation names the rule, the offending edge with
//! file:line and a fix hint"*, which is a sentence about three separate pieces of evidence and is why
//! this is a type rather than a formatted string:
//!
//! * **the rule** — its name ([`super::rules::Rules`] resolved it at load, so the name is the one the
//!   author can find in `fathom.toml`), its kind, and where it was declared;
//! * **the offending edge** — both endpoints at the granularity the extractor guarantees, the row kind
//!   that carried it, and the line of the statement that made it when
//!   [`super::locate::Locate`] could say ([`crate::structure::Import::span`]);
//! * **the fix** — the usual one for *that kind* of rule, phrased with the concrete names in it, so a
//!   reviewer reading one line in a pane gets something to do rather than a category.
//!
//! # Why `why` is data and not a log line
//!
//! The same violation is printed in a TUI pane, in `fathom check`'s SARIF `message`, and in a markdown
//! job summary, and all three have to say the same thing. A surface composes [`Violation::sentence`]
//! for a one-liner and [`Violation::detail_lines`] for a body; neither re-derives a fact the other
//! does not have.
//!
//! # The caveats ride with the finding
//!
//! A layering rule that fires on an artefact of the scanner is a false positive with a good pedigree,
//! and `s1`'s findings 2 and 4 are the two that bite here, so they are fields on the violation and not
//! paragraphs in a README: [`Violation::package`] says the edge is a *package*-to-package fact drawn at
//! file granularity (Go), and [`Violation::unknown`] says one of its legs is a dependency whose
//! contents nobody read (a wildcard). Where the claim cannot be made at a defensible granularity the
//! edge is **not judged at all** — see [`super::engine`].

use serde_json::{Value, json};

use crate::cycles::cycle::Cycle;
use crate::layering::locate::Line;
use crate::layering::rules::{Rule, RuleKind};

/// What a judged endpoint is a fact about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// A file: every language but Go, and every Go edge the extractor called file-granular.
    File,
    /// A package: the Go case, where the language's dependency is `package → package` and the row is
    /// one file's rendering of it (`s1` finding 4).
    Package,
}

impl Unit {
    /// The stored spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Unit::File => "file",
            Unit::Package => "package",
        }
    }
}

/// One endpoint of a judged dependency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Side {
    /// The module path, or the package directory when [`Side::unit`] is [`Unit::Package`].
    pub path: String,
    /// Its `[layers]` tag; empty means **untagged**, which is "the repository did not say", not "has
    /// no layer".
    pub layer: String,
    /// The granularity the extractor guarantees for this endpoint.
    pub unit: Unit,
}

impl Side {
    /// A file-level endpoint.
    #[must_use]
    pub fn file(path: impl Into<String>, layer: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            layer: layer.into(),
            unit: Unit::File,
        }
    }

    /// A package-level endpoint (Go).
    #[must_use]
    pub fn package(path: impl Into<String>, layer: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            layer: layer.into(),
            unit: Unit::Package,
        }
    }

    /// `layer/name.rs`, with `(untagged)` when no rule claims it — the form that goes into a sentence.
    #[must_use]
    pub fn label(&self) -> String {
        if self.layer.is_empty() {
            return format!("{} (untagged)", self.path);
        }
        format!("{} ({})", self.path, self.layer)
    }

    /// The machine-readable form.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "path": self.path,
            "layer": self.layer,
            "unit": self.unit.as_str(),
            "tagged": !self.layer.is_empty(),
        })
    }
}

/// One violation of one rule, by one dependency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// The rule's name — a label the author wrote, or the rule's own content. Also the baseline key's
    /// first field, so it does not move when an unrelated rule is inserted.
    pub rule: String,
    /// Which family the rule belongs to.
    pub kind: RuleKind,
    /// The rule's own assertion, quoted: the violation repeats what the config said.
    pub body: String,
    /// Where the rule was declared (`layering.forbid (line 12)`), so the fix is a keystroke away.
    pub declared_in: String,
    /// The depending side.
    pub src: Side,
    /// The side depended on.
    pub dst: Side,
    /// The row kind that carried the dependency (`imports`, `uses`, `imports_package`, `wildcard`,
    /// `implements`, `inherits`).
    pub edge: &'static str,
    /// The edge's contents are unknown — a wildcard leg (`s1` finding 2).
    pub unknown: bool,
    /// The edge is a package-to-package fact drawn per file (Go, `s1` finding 4).
    pub package: bool,
    /// The line of the statement, when it could be named.
    pub line: Option<usize>,
    /// Why there is no line, when there is not. Stated, because a bare path invites the reader to
    /// assume the whole file.
    pub line_note: Option<String>,
    /// The members of a cycle, sorted — empty for every non-cycle rule.
    pub members: Vec<String>,
    /// The witness path that closes a cycle, first-to-last; empty for every non-cycle rule.
    pub witness: Vec<String>,
    /// Whether the witness is proven shortest ( [`crate::cycles::EXACT_WITNESS_MEMBER_CAP`]).
    pub exact: bool,
    /// The declared `layering.order`, carried for the rule whose whole meaning is the order. Empty for
    /// the others, so a report never prints an order that decided nothing.
    pub order: Vec<String>,
    /// Why this is wrong, in one sentence with the names in it.
    pub why: String,
    /// The usual fix, in one sentence with the names in it.
    pub fix: String,
}

impl Violation {
    /// A violation of a written or built-in rule, on one judged edge.
    #[must_use]
    pub fn from_edge(
        rule: &Rule,
        src: Side,
        dst: Side,
        edge: &'static str,
        line: Line,
        order: &[String],
    ) -> Self {
        let unknown = edge == "wildcard";
        let package = edge == "imports_package" || src.unit == Unit::Package;
        let mut out = Self {
            rule: rule.name.clone(),
            kind: rule.kind,
            body: rule.body(),
            declared_in: rule.declared_in(),
            src,
            dst,
            edge,
            unknown,
            package,
            line: line.number(),
            line_note: line.note(),
            members: Vec::new(),
            witness: Vec::new(),
            exact: true,
            order: if rule.kind == RuleKind::NoUpward {
                order.to_vec()
            } else {
                Vec::new()
            },
            why: String::new(),
            fix: String::new(),
        };
        out.why = out.explain();
        out.fix = out.usual_fix();
        out
    }

    /// A cycle reported through one of the cycle rules.
    ///
    /// The offending *edge* is the one that closes the loop — the witness's last member depending on
    /// its first — because a violation that named only a set of five files could not print a line, and
    /// the members are carried alongside so the identity is still the set
    /// ([`crate::cycles::Cycle::key`], and the same argument the cycle ledger makes about identity).
    /// The two sides come from the caller, which owns the naming and therefore the layer tags.
    #[must_use]
    pub fn from_cycle(rule: &Rule, cycle: &Cycle, src: Side, dst: Side, line: Line) -> Self {
        let package_edge = dst.unit == Unit::Package;
        let mut out = Self {
            rule: rule.name.clone(),
            kind: rule.kind,
            body: rule.body(),
            declared_in: rule.declared_in(),
            src,
            dst,
            edge: if package_edge {
                "imports_package"
            } else {
                "imports"
            },
            unknown: cycle.unknown_edges > 0,
            package: cycle.package_edges > 0 || package_edge,
            line: line.number(),
            line_note: line.note(),
            members: cycle.members.clone(),
            witness: cycle.path.clone(),
            exact: cycle.exact,
            order: Vec::new(),
            why: String::new(),
            fix: String::new(),
        };
        out.why = out.explain();
        out.fix = out.usual_fix();
        out
    }

    /// The violation's key: `rule::src->dst` for an edge rule, `rule::member|set` for a cycle rule.
    ///
    /// The separator is the same double colon the cycle ledger uses, and for the same reason: an
    /// unnamed rule's own name already contains a colon (`forbid:ui->db`), and a key whose halves could
    /// not be told apart could not be matched, printed or debugged. Canonical through
    /// [`crate::cycles::debt::normalize_key`].
    #[must_use]
    pub fn key(&self) -> String {
        match self.kind {
            RuleKind::NoCycles | RuleKind::NoCycleWithinLayer => format!(
                "{}::{}",
                self.rule,
                crate::cycles::debt::normalize_key(&self.members.join("|"))
            ),
            _ => format!("{}::{}->{}", self.rule, self.src.path, self.dst.path),
        }
    }

    /// `path:line`, or the bare path with its reason in [`Violation::line_note`].
    #[must_use]
    pub fn location(&self) -> String {
        match self.line {
            Some(line) => format!("{}:{}", self.src.path, line),
            None => self.src.path.clone(),
        }
    }

    /// The one line a pane prints.
    #[must_use]
    pub fn sentence(&self) -> String {
        let arrow = if self.package { "⇢" } else { "→" };
        let mut out = format!(
            "{} · {} {} {} — {}",
            self.rule,
            self.location(),
            arrow,
            self.dst.label(),
            self.kind.claim()
        );
        if self.kind == RuleKind::NoCycles || self.kind == RuleKind::NoCycleWithinLayer {
            out = format!(
                "{} · {} ({} {} member(s)) — {}",
                self.rule,
                self.witness.join(" → "),
                self.members.len(),
                if self.package { "package" } else { "file" },
                self.kind.claim()
            );
        }
        out
    }

    /// The body lines: the reason, then the fix, then every caveat the finding carries.
    #[must_use]
    pub fn detail_lines(&self) -> Vec<String> {
        let mut out = vec![format!("why: {}", self.why), format!("fix: {}", self.fix)];
        if self.package {
            out.push(
                "note: this edge names a *package*, not a file (Go): the language's dependency is \
                 package-to-package and the layer tags are read at that granularity"
                    .to_string(),
            );
        }
        if self.unknown {
            out.push(
                "note: one leg of this dependency has contents nobody read (a wildcard import), so \
                 what it reaches is bounded by the wildcard, not by the name"
                    .to_string(),
            );
        }
        if let Some(note) = &self.line_note {
            out.push(format!("note: {note}"));
        }
        if !self.exact {
            out.push(
                "note: the witness is the shortest cycle through its first member, not proven \
                 shortest through the component"
                    .to_string(),
            );
        }
        out
    }

    /// The machine-readable form, with every field the sentences are built from.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "rule": self.rule,
            "kind": self.kind.as_str(),
            "body": self.body,
            "declared_in": self.declared_in,
            "key": self.key(),
            "location": self.location(),
            "line": self.line,
            "line_note": self.line_note,
            "edge": self.edge,
            "unknown_contents": self.unknown,
            "package_granular": self.package,
            "src": self.src.to_json(),
            "dst": self.dst.to_json(),
            "members": self.members,
            "witness": self.witness,
            "exact": self.exact,
            "order": self.order,
            "why": self.why,
            "fix": self.fix,
        })
    }

    /// The rule, the endpoints and the rank or selector that made it a violation.
    fn explain(&self) -> String {
        match self.kind {
            RuleKind::Forbid => format!(
                "`{}` depends on `{}`, and `{}` — declared in {} — forbids `{}`",
                self.src.label(),
                self.dst.label(),
                self.rule,
                self.declared_in,
                self.body
            ),
            RuleKind::Only => format!(
                "`{}` may be reached only from `{}`; `{}` is not one of the sources that rule names",
                self.dst.label(),
                self.rule_selector_side(),
                self.src.label()
            ),
            RuleKind::NoUpward => format!(
                "`{}` points uphill at `{}`: the declared order, bottom first, is {}, and {} sits \
                 above {}",
                self.src.label(),
                self.dst.label(),
                self.order
                    .iter()
                    .map(|name| format!("`{name}`"))
                    .collect::<Vec<_>>()
                    .join(", "),
                self.dst.layer,
                self.src.layer
            ),
            RuleKind::NoCycles => format!(
                "{} close a dependency loop and this repository declares `no_cycles`; the closing \
                 edge is `{}` → `{}`",
                self.members.len(),
                self.src.path,
                self.dst.path
            ),
            RuleKind::NoCycleWithinLayer => format!(
                "{} modules inside the `{}` layer close a dependency loop; a loop within one layer \
                 means the layer is doing more than one job",
                self.members.len(),
                if self.src.layer.is_empty() {
                    self.dst.layer.as_str()
                } else {
                    self.src.layer.as_str()
                }
            ),
            RuleKind::Allow => format!(
                "`{}` depends on `{}`, which `{}` permits — a permission is never a violation, so \
                 this line means an exception is load-bearing",
                self.src.label(),
                self.dst.label(),
                self.rule
            ),
        }
    }

    /// The usual fix for this kind of rule, with the concrete names already in it.
    fn usual_fix(&self) -> String {
        match self.kind {
            RuleKind::Forbid => format!(
                "point it the other way (declare the seam in `{}` and let `{}` implement it), or \
                 move the part both need into a layer both may reach. If the edge is intended, say \
                 so narrowly: allow = [\"{} -> {}\"]",
                self.src.path, self.dst.path, self.src.path, self.dst.path
            ),
            RuleKind::Only => format!(
                "reach `{}` through the module the boundary names — that indirection is the point of \
                 a boundary — or widen it on purpose and with a reason: only = [\"{} <- {}\"]",
                self.dst.path, self.dst.path, self.src.path
            ),
            RuleKind::NoUpward => format!(
                "move what `{}` needs into `{}`'s layer or below it, or pass it in from a caller \
                 both already share; if `{}` really is the lower layer, say so in layering.order",
                self.dst.path, self.src.layer, self.dst.layer
            ),
            RuleKind::NoCycles => format!(
                "pull the part every member needs into a module all of them may depend on — the \
                 witness is the shortest loop to break. If it is debt and not news, baseline it \
                 with an owner and a date: kind = \"layer\", key = \"{}\"",
                self.key()
            ),
            RuleKind::NoCycleWithinLayer => String::from(
                "split the interface half out of the members and point both halves at it, or move \
                 the members apart in layering.order if they are really two layers wearing one name",
            ),
            RuleKind::Allow => String::from(
                "nothing to fix: this is the exception the rule above was written to have",
            ),
        }
    }

    /// The allowed-source side of a boundary rule, for the `why` sentence.
    fn rule_selector_side(&self) -> String {
        self.body
            .split_once("<-")
            .map_or_else(|| self.body.clone(), |(_, src)| src.trim().to_string())
    }
}
