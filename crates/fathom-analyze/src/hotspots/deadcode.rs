//! Dead-code candidates, and the three ways a static reader can be wrong about them
//! (`e3.8`, `reco-wky`).
//!
//! The ticket names three kinds, and all three are listed here as *candidates* — a word chosen because
//! every one of them is reachable by at least one mechanism that makes it wrong:
//!
//! | kind | the fact that was computed | what it could still be |
//! |---|---|---|
//! | [`CandidateKind::Unreferenced`] | a symbol added within the window that no other module names after N episodes | reflection, `serde`, `#[derive]`, a trait object, a build script, a caller in another repository, a macro that writes the call site |
//! | [`CandidateKind::Gated`] | a declaration behind a flag this build does not resolve | the flag is on in the build that ships, in CI, or on the platform you are not on right now |
//! | [`CandidateKind::Duplicated`] | the same block of new text arrived in two places in one episode | a table-driven test, two implementations of one interface, a match arm per variant |
//!
//! So every candidate carries three things a reviewer can act on and check: **how long** it has been
//! without a reference ([`Candidate::age`], in episodes, with the window it was judged over — an age of
//! one episode in a window of one is not a finding), **what was checked** ([`Candidate::why`]), and
//! **the named mechanism that could make it wrong** ([`Candidate::uncertainty`], one line per
//! mechanism, empty only when nothing was withheld).
//!
//! # Verdicts, and the difference between them
//!
//! [`Confidence::Firm`] means every check this build could run ran and nothing contradicted it.
//! [`Confidence::Uncertain`] means a *named* mechanism could make it wrong — a wildcard that reaches the
//! module, a scope that cannot be enumerated, a flag nobody declared. [`Confidence::Unverified`] means
//! the history is too short to judge at all: `hotspots.dead_code_after` is 3 and the symbol arrived last
//! episode, so the correct answer is "not yet", not "dead". A surface that prints only the `firm` rows
//! is the honest subset; a surface that prints the count as a total is the bug this whole module exists
//! to avoid.
//!
//! # The blind spots are printed, not assumed away
//!
//! [`LIMITS`] is a permanent list of what no static graph over one repository can see. It travels with
//! every report because a dead-code report without it is read as a proof.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::detectors::text::{self, Dialect};
use crate::graph::query::{dependents_of, module_of};
use crate::graph::{Graph, Naming, RowKind};

use super::policy::Policy;

/// The shape tag a JSON reader checks first.
pub const CONTRACT: &str = "fathom.dead-code/1";

/// What no static reader sees, whatever this build gets better at.
pub const LIMITS: [&str; 6] = [
    "reflection, dependency injection containers and anything a build script or a proc-macro generates \
     at compile time: none of it appears as a call site in the source",
    "`serde`, `#[derive]`, `#[async_trait]`, `@dataclass`, a decorator or a metaclass: the framework \
     reaches the name through a string, and the string is not an edge",
    "dynamic dispatch: a call through a trait object, a vtable, an interface pointer or a monkeypatched \
     method has no static caller to point at the implementation",
    "callers outside this repository: a library's exported symbol with no in-repo reference is the \
     normal shape of an API, not evidence",
    "tests, benchmarks, examples and benches: excluded from the sweep by default, and excluded loudly \
     rather than silently",
    "a window shorter than `hotspots.dead_code_after` cannot show a symbol that is merely new; every \
     such row is listed as `unverified` and never counted as a candidate",
];

/// Which of the three questions produced the candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateKind {
    /// A symbol added inside the window that nothing has named since.
    Unreferenced,
    /// A declaration behind a flag that this read could not resolve as on.
    Gated,
    /// A block of new text that arrived in two places in the same episode.
    Duplicated,
}

impl CandidateKind {
    /// The stored spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            CandidateKind::Unreferenced => "unreferenced",
            CandidateKind::Gated => "gated",
            CandidateKind::Duplicated => "duplicated",
        }
    }

    /// The section heading a pane uses.
    #[must_use]
    pub const fn heading(self) -> &'static str {
        match self {
            CandidateKind::Unreferenced => "new symbols with no inbound reference",
            CandidateKind::Gated => "code behind a flag this build does not resolve as on",
            CandidateKind::Duplicated => "duplicated blocks that grew in the same episode",
        }
    }
}

/// How sure the read is allowed to be, and the reason it is allowed to be that sure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Every check ran; nothing contradicted it. Still a candidate: this is a static read.
    Firm,
    /// A named mechanism could make this wrong. The mechanism is in [`Candidate::uncertainty`].
    Uncertain,
    /// The history is too short, or the input that would decide it is missing. Not a finding.
    Unverified,
}

impl Confidence {
    /// The stored spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Confidence::Firm => "firm",
            Confidence::Uncertain => "uncertain",
            Confidence::Unverified => "unverified",
        }
    }
}

/// How long a subject has had no reference, and how much history that judgement had.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Age {
    /// Episodes (or revisions) since the subject appeared.
    pub episodes: usize,
    /// How many episodes the read looked back over: an age equal to the window is an age the window
    /// cannot bound, and says so.
    pub window: usize,
    /// The revision it arrived in, when the caller knows one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_revision: Option<String>,
    /// Whether `episodes >= hotspots.dead_code_after`, i.e. whether the wait is long enough to mean
    /// anything. False is not a candidate; it is a row that is not due yet.
    pub due: bool,
    /// The bar `due` was decided against, printed beside it.
    pub after: usize,
}

impl Age {
    /// The sentence: the age, the denominator, and the bar. Never a bare number.
    #[must_use]
    pub fn sentence(&self) -> String {
        let base = format!(
            "unreferenced for {} episode(s){}",
            self.episodes,
            self.since_revision
                .as_ref()
                .map(|rev| format!(" since it appeared in {rev}"))
                .unwrap_or_default()
        );
        if self.due {
            format!("{base}, past the `dead_code_after` bar of {}", self.after)
        } else {
            format!(
                "{base}; `hotspots.dead_code_after` is {}, so this is too new to judge and is not \
                 claimed as dead",
                self.after
            )
        }
    }
}

/// One candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Candidate {
    /// Which question produced it.
    pub kind: CandidateKind,
    /// What it is about: `path::Symbol`, `path:line`, or a pair of spans for a duplicated block.
    pub subject: String,
    /// The module it belongs to, so a row can be read next to the ranking.
    pub module: String,
    /// How sure the read is.
    pub verdict: Confidence,
    /// How long it has been unreferenced. `None` for a duplicated block, which is about a moment
    /// rather than a wait.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub age: Option<Age>,
    /// The age, as the sentence a row prints. `None` when there is no age to state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    /// The fact that was computed, with its numbers.
    pub why: String,
    /// Every mechanism that could make this wrong, one line each. Empty only when the verdict is
    /// `firm` and nothing was withheld.
    pub uncertainty: Vec<String>,
    /// The evidence a reviewer clicks: `path:line`, a flag name, a revision.
    pub evidence: Vec<String>,
    /// The knob that decided it, when a knob decided it: `hotspots.dead_code_after = 3`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<String>,
}

/// A newly added symbol, as the caller's history read it.
///
/// The caller — a lane path, or a test — owns the question "when did this appear", because answering it
/// means walking `graph_delta` back through the chain, which is a store question with a cost of its
/// own. What this module owns is what to *say* once it is known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSymbol {
    /// The graph's symbol key: `path::Name`.
    pub key: String,
    /// How many episodes ago it first appeared. `0` is "this episode".
    pub added_episodes_ago: usize,
    /// The revision it appeared in, when the caller has one to name.
    pub revision: Option<String>,
}

/// One file's text, for the flag scan.
#[derive(Debug, Clone, Copy)]
pub struct Source<'a> {
    /// The path.
    pub path: &'a str,
    /// The file's text at the revision being reviewed.
    pub text: &'a str,
}

/// One file's added lines, for the duplication sweep.
#[derive(Debug, Clone)]
pub struct AddedLines<'a> {
    /// The path.
    pub path: &'a str,
    /// The lines the episode added, in file order, without their `+` markers.
    pub lines: &'a [String],
}

/// The whole answer: three lists that share one policy and one stated set of limits.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    /// The shape tag.
    pub contract: &'static str,
    /// The revision the read was asked about.
    pub revision: String,
    /// Every candidate, ordered by kind then by subject so two runs print identically.
    pub candidates: Vec<Candidate>,
    /// How many are `firm` — the subset a gate may act on.
    pub firm: usize,
    /// How many are `uncertain`, with their mechanisms attached.
    pub uncertain: usize,
    /// How many are `unverified`: too new, or missing the input that would decide them.
    pub unverified: usize,
    /// How many symbols the unreferenced sweep examined. A candidate list with no denominator is a
    /// list of anecdotes.
    pub symbols_checked: usize,
    /// How many new symbols the window contained.
    pub new_symbols: usize,
    /// Paths the `hotspots.ignore` list excluded, counted rather than hidden.
    pub ignored_paths: usize,
    /// The patterns that did the excluding, so a reader can check whether one of them swallowed the
    /// file they care about.
    pub ignored_patterns: Vec<String>,
    /// How many gated declarations were *not* reported because their flag is declared on, or because
    /// the flag is the build's to set rather than the repository's.
    pub gated_not_reported: usize,
    /// What the read could not do: a file it could not read, a graph it could not reach.
    pub notices: Vec<String>,
    /// The permanent blind spots ([`LIMITS`]), copied in so a serialised report carries its own limits.
    pub limits: Vec<String>,
    /// Why the lists are empty, when they are: "no new symbols in the window" and "every new symbol is
    /// named somewhere" are different empties.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub empty_reason: Option<String>,
}

impl Report {
    /// The candidates of one kind, in the order they were produced.
    #[must_use]
    pub fn of_kind(&self, kind: CandidateKind) -> Vec<&Candidate> {
        self.candidates
            .iter()
            .filter(|candidate| candidate.kind == kind)
            .collect()
    }

    /// Whether there is nothing to print, which is a state with a reason and not a silence.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }
}

/// The unreferenced sweep: new symbols, and what the graph can and cannot say about them.
///
/// A reference is a dependency edge that arrives at the symbol's own node from a **different** module
/// — `uses`, `implements`, `inherits`, or a file-granular import that named it. Self-references do not
/// count: a module calling its own helper is not evidence anybody else needs it.
#[must_use]
pub fn unreferenced(
    graph: &Graph,
    naming: &Naming,
    new: &[NewSymbol],
    window: usize,
    policy: &Policy,
) -> Vec<Candidate> {
    let mut out = Vec::new();
    for symbol in new {
        if policy.ignores(&symbol.key) {
            continue;
        }
        let Some(id) = naming.id(&symbol.key) else {
            continue;
        };
        if !graph.nodes().contains_key(&id) {
            continue;
        }
        let home = module_of(&symbol.key);
        let mut inbound = 0_usize;
        for (kind, src) in graph.arrived(id, RowKind::is_dependency) {
            let Some(name) = naming.name(src) else {
                continue;
            };
            if module_of(name) == home {
                continue;
            }
            let _ = kind;
            inbound += 1;
        }
        if inbound > 0 {
            continue;
        }
        // A wildcard reaches the *module*, not the symbol node: a star import, a barrel `export *` or a
        // Go package import can name this symbol without the graph recording which one it meant, which
        // is `s1` finding 2 wearing a different hat. It is read at the module end for that reason.
        let module_fan = dependents_of(graph, naming, &home);
        let inbound_wildcard = module_fan.wildcard.len();
        let inbound_package = module_fan.package_granular.len();
        let age = Age {
            episodes: symbol.added_episodes_ago,
            window,
            since_revision: symbol.revision.clone(),
            due: symbol.added_episodes_ago >= policy.dead_code_after,
            after: policy.dead_code_after,
        };
        let mut uncertainty = Vec::new();
        if !age.due {
            uncertainty.push(format!(
                "it arrived {} episode(s) ago and the bar is {}, so the window has not given anything \
                 a chance to name it",
                symbol.added_episodes_ago, policy.dead_code_after
            ));
        }
        if inbound_wildcard > 0 || inbound_package > 0 {
            uncertainty.push(format!(
                "{inbound_wildcard} wildcard and {inbound_package} package-granular edge(s) reach \
                 `{home}` — a star import, a barrel `export *`, a Go package import — and any of them \
                 can name this symbol without the graph recording which one it meant"
            ));
        }
        let module_id = naming.id(&home);
        if module_id.is_some_and(|id| naming.incomplete(id)) {
            uncertainty.push(
                "the declaring module's export list may have shrunk rather than shrunk — a macro, a \
                 decorator or a generated scope — so the reference list this was computed from is a \
                 lower bound"
                    .to_string(),
            );
        }
        if naming
            .vertex(id)
            .is_some_and(|vertex| vertex.layer.is_empty() && vertex.incomplete)
        {
            uncertainty.push(String::from(
                "the symbol itself is inside a scope that could not be enumerated",
            ));
        }
        uncertainty.push(String::from(
            "no static read sees reflection, a derive, a serde field, a trait object or a caller in \
             another repository; an exported symbol of a library is the normal shape of an API",
        ));
        let verdict = if !age.due {
            Confidence::Unverified
        } else if inbound_wildcard + inbound_package > 0
            || module_id.is_some_and(|id| naming.incomplete(id))
        {
            Confidence::Uncertain
        } else {
            Confidence::Firm
        };
        out.push(Candidate {
            kind: CandidateKind::Unreferenced,
            subject: symbol.key.clone(),
            module: home.clone(),
            verdict,
            age: Some(age.clone()),
            since: Some(age.sentence()),
            why: format!(
                "no module other than `{home}` names it: the graph holds zero inbound reference edges \n                 for this symbol at this revision, and its own module never counts as one"
            ),
            uncertainty,
            evidence: match &symbol.revision {
                Some(rev) => vec![format!("appeared in {rev}"), symbol.key.clone()],
                None => vec![symbol.key.clone()],
            },
            decided_by: Some(format!("hotspots.dead_code_after = {}", policy.dead_code_after)),
        })
    }
    out
}

/// A flag the scan recognised, and whose decision it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagKind {
    /// `feature = "x"` and the like: the repository's own knob, and the only kind
    /// `hotspots.flags` can speak to.
    Repository,
    /// `test`, `unix`, `windows`, `debug_assertions`, `docsrs`: set by the build and the host, not by
    /// anybody's `fathom.toml`, so a report that called them dead would be reporting the platform.
    Build,
    /// An environment or config gate in a language with no cfg: `TYPE_CHECKING`, `if False`,
    /// `process.env.LAKE_PROXY`.
    Dynamic,
}

impl FlagKind {
    /// The stored spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            FlagKind::Repository => "repository",
            FlagKind::Build => "build",
            FlagKind::Dynamic => "dynamic",
        }
    }
}

/// One gated region the scan found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gate {
    /// The flag as written.
    pub flag: String,
    /// Whose knob it is.
    pub kind: FlagKind,
    /// The line the gate is on, one-based.
    pub line: usize,
    /// The gated declaration's own line, when the scan could tell it from the attribute.
    pub declaration_line: usize,
    /// The gate as it appears in the source, verbatim.
    pub spelling: String,
}

/// Find the gates in one file's text.
///
/// Six spellings, chosen as the ones that appear in this repository's own languages, and each one
/// matched on a line that *is* a gate (an attribute, a build tag, an `if` on a known condition) rather
/// than by any fuzzier guess. It reads text, not a build graph, and [`gated`] says so in every row.
#[must_use]
pub fn gates(text: &str) -> Vec<Gate> {
    let mut out = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut index = 0_usize;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        if let Some(gate) = gate_on(trimmed) {
            let mut gate_line = index + 1;
            let mut declaration_line = index + 1;
            // A Rust attribute stack belongs to the next non-attribute line; a Go build tag or an `if`
            // belongs to the line it is on or the block below it.
            let mut cursor = index + 1;
            if trimmed.starts_with("#[") || trimmed.starts_with("#[cfg") {
                while cursor < lines.len() && lines[cursor].trim().starts_with("#[") {
                    cursor += 1;
                }
                if cursor < lines.len() {
                    declaration_line = cursor + 1;
                }
            } else if cursor < lines.len() && !lines[cursor].trim().is_empty() {
                declaration_line = cursor + 1;
            }
            gate_line = gate_line.max(1);
            out.push(Gate {
                declaration_line: declaration_line.max(gate_line),
                ..gate
            });
        }
        index += 1;
    }
    out
}

/// Classify one line as a gate, if it is one.
fn gate_on(line: &str) -> Option<Gate> {
    let spelling = line.to_string();
    let lower = line.to_ascii_lowercase();
    // Rust: `#[cfg(feature = "x")]`, `#[cfg(feature = "x", not(test))]`, `#[cfg(test)]`,
    // `#[cfg_attr(feature = "x", …)]`.
    if line.starts_with("#[cfg(") || line.starts_with("#[cfg_attr(") {
        if let Some(flag) = quoted_after(line, "feature") {
            return Some(Gate {
                flag,
                kind: FlagKind::Repository,
                line: 0,
                declaration_line: 0,
                spelling,
            });
        }
        let build = [
            "test",
            "unix",
            "windows",
            "linux",
            "macos",
            "docsrs",
            "debug_assertions",
        ];
        if build
            .iter()
            .any(|flag| lower.contains(&format!("cfg({flag}")))
            || build
                .iter()
                .any(|flag| lower.contains(&format!("not({flag}")))
        {
            return Some(Gate {
                flag: build
                    .iter()
                    .find(|flag| {
                        lower.contains(&format!("cfg({flag}"))
                            || lower.contains(&format!("not({flag}"))
                    })
                    .map(|flag| (*flag).to_string())
                    .unwrap_or_default(),
                kind: FlagKind::Build,
                line: 0,
                declaration_line: 0,
                spelling,
            });
        }
        return None;
    }
    // Go: `//go:build !js`, `//go:build js && wasm`.
    if let Some(rest) = line.strip_prefix("//go:build ") {
        let flag = rest
            .split([' ', '(', '!', '|', '&'])
            .find(|token| !token.is_empty())
            .unwrap_or("build-tag")
            .to_string();
        return Some(Gate {
            flag,
            kind: FlagKind::Build,
            line: 0,
            declaration_line: 0,
            spelling,
        });
    }
    // C-shaped: `#if defined(X)`, `#ifdef X`.
    if let Some(rest) = line
        .strip_prefix("#ifdef ")
        .or_else(|| line.strip_prefix("#if defined("))
    {
        let flag = rest
            .trim()
            .trim_start_matches('(')
            .trim_end_matches(')')
            .split([' ', ')'])
            .next()
            .unwrap_or("macro")
            .to_string();
        return Some(Gate {
            flag,
            kind: FlagKind::Repository,
            line: 0,
            declaration_line: 0,
            spelling,
        });
    }
    // Python and TypeScript: a runtime condition the interpreter or the bundler decides.
    for (needle, flag, kind) in [
        ("if TYPE_CHECKING:", "TYPE_CHECKING", FlagKind::Dynamic),
        ("if False:", "always-false branch", FlagKind::Dynamic),
        ("if (process.env.", "process.env", FlagKind::Dynamic),
        ("if process.env.", "process.env", FlagKind::Dynamic),
    ] {
        if line.contains(needle) {
            let named = if flag == "process.env" {
                env_name(line).unwrap_or_else(|| "process.env".to_string())
            } else {
                flag.to_string()
            };
            return Some(Gate {
                flag: named,
                kind,
                line: 0,
                declaration_line: 0,
                spelling,
            });
        }
    }
    None
}

/// `feature = "x"` → `Some("x")`.
fn quoted_after(line: &str, key: &str) -> Option<String> {
    let (_, rest) = line.split_once(&format!("{key} = \""))?;
    let (value, _) = rest.split_once('"')?;
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// The variable inside `process.env.LAKE_PROXY` or `process.env["LAKE_PROXY"]`.
fn env_name(line: &str) -> Option<String> {
    let (_, rest) = line.split_once("process.env.")?;
    let name: String = rest
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect();
    if name.is_empty() {
        None
    } else {
        Some(format!("process.env.{name}"))
    }
}

/// The gated-code sweep.
///
/// A gate whose flag is declared on by `hotspots.flags` is not reported; a gate whose flag belongs to
/// the build (`cfg(test)`, a Go build tag) is not reported either, and both are counted in
/// [`Report::gated_not_reported`] rather than vanishing. Everything else is a candidate, and none of
/// them is ever `firm`: whether a flag is on is a fact about a build this tool is not running.
#[must_use]
pub fn gated(revision: &str, files: &[Source<'_>], policy: &Policy) -> Report {
    let mut candidates = Vec::new();
    let mut not_reported = 0_usize;
    let mut ignored_paths = 0_usize;
    let notices: Vec<String> = Vec::new();
    for file in files {
        if policy.ignores(file.path) {
            ignored_paths += 1;
            continue;
        }
        let found = gates(file.text);
        if found.is_empty() {
            continue;
        }
        let mut by_flag: BTreeMap<String, (FlagKind, Vec<usize>, String)> = BTreeMap::new();
        for gate in found {
            let entry = by_flag.entry(gate.flag.clone()).or_insert((
                gate.kind,
                Vec::new(),
                gate.spelling.clone(),
            ));
            entry.1.push(gate.declaration_line.max(gate.line));
            entry.2 = gate.spelling.clone();
        }
        for (flag, (kind, lines, spelling)) in by_flag {
            if kind == FlagKind::Build || policy.flag_on(&flag) {
                not_reported += 1;
                continue;
            }
            let line = lines.into_iter().min().unwrap_or(1);
            let uncertainty = if policy.flags.is_empty() {
                vec![
                    format!(
                        "`hotspots.flags` declares no flags as on, so this read cannot tell a flag \
                         that is off from a flag nobody declared: `{spelling}` may well be on in the \
                         build that ships"
                    ),
                    "the scan reads text, not a build graph: it knows the attribute says the flag, not \
                     whether the compiler was told to compile it"
                        .to_string(),
                ]
            } else {
                vec![
                    format!(
                        "`hotspots.flags` declares [{}] as on and `{flag}` is not among them — that \
                         list is a claim the repository owner wrote into `fathom.toml`, not a resolved \
                         build configuration, and a feature enabled on the command line is invisible \
                         to it",
                    policy.flags.join(", ")
                    ),
                    "the scan reads text, not a build graph: it knows the attribute says the flag, not \
                     whether the compiler was told to compile it"
                        .to_string(),
                ]
            };
            candidates.push(Candidate {
                kind: CandidateKind::Gated,
                subject: format!("{}:{line}", file.path),
                module: file.path.to_string(),
                verdict: Confidence::Uncertain,
                age: None,
                since: None,
                why: format!(
                    "declared behind `{spelling}`; the repository declares {} flag(s) on and this is \
                     not one of them",
                    policy.flags.len()
                ),
                uncertainty,
                evidence: vec![format!("{}:{line}", file.path), format!("flag: {flag}")],
                decided_by: Some(format!(
                    "hotspots.flags = [{}]",
                    policy.flags.join(", ")
                )),
            });
        }
    }
    let _ = revision;
    build_report(
        revision,
        candidates,
        ReportCounts {
            symbols_checked: 0,
            new_symbols: 0,
            ignored_paths,
            gated_not_reported: not_reported,
        },
        notices,
        if ignored_paths > 0 {
            Some(format!(
                "{ignored_paths} path(s) were excluded from the gated sweep by `hotspots.ignore`"
            ))
        } else {
            None
        },
        policy,
    )
}

/// The duplication sweep: blocks of new text that arrived in two places in one episode.
///
/// Four refusals, inherited from [`crate::detectors::copy_paste`] because they are the same fact and
/// the same false positives: prose and configuration are not code; tests are repetition *by design*; a
/// window of `} else {` means nothing; and an import block is what the top of a big file looks like.
/// Every path a refusal dropped is counted in the report rather than vanishing from it.
///
/// Overlapping windows of one repeated run are one block: a 20-line copy produces nine matching
/// twelve-line windows, and printing nine candidates for one copy is the difference between a count and
/// a noise machine. Runs are merged per side, so the span printed is the copy's, not the window's.
#[must_use]
pub fn duplicated_growth(revision: &str, files: &[AddedLines<'_>], policy: &Policy) -> Report {
    let block = policy.duplicate_block_lines.max(2);
    let mut ignored_paths = 0_usize;
    let mut refused: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut scanned: Vec<Scanned<'_>> = Vec::new();
    for file in files {
        if policy.ignores(file.path) {
            ignored_paths += 1;
            continue;
        }
        if let Some(reason) = refused_path(file.path) {
            *refused.entry(reason).or_default() += 1;
            continue;
        }
        let lines: Vec<String> = file
            .lines
            .iter()
            .map(|line| text::normalise(line))
            .filter(|line| !line.is_empty())
            .collect();
        if lines.len() >= block {
            scanned.push(Scanned {
                path: file.path,
                lines,
                dialect: text::dialect(file.path).unwrap_or(Dialect::C),
            });
        }
    }

    // Hash every window that is worth hashing, keyed by the hash so a match is one lookup.
    let mut windows: BTreeMap<usize, Vec<Place>> = BTreeMap::new();
    for (index, file) in scanned.iter().enumerate() {
        for start in 0..=(file.lines.len() - block) {
            let window = &file.lines[start..start + block];
            if !text::substantive(window, file.dialect) || looks_like_imports(window) {
                continue;
            }
            windows
                .entry(hash(window))
                .or_default()
                .push(Place { file: index, start });
        }
    }

    // A hash in two distinct places is a repeated block. Overlapping windows of one run are collapsed
    // before pairing, so a copy yields one block per pair of copies rather than one per window.
    let mut pairs: Vec<(Place, Place)> = Vec::new();
    let mut widest = 0_usize;
    for places in windows.values() {
        let distinct = distinct_places(places, block);
        if distinct.len() < 2 {
            continue;
        }
        widest = widest.max(distinct.len());
        for (left, index) in distinct.iter().enumerate() {
            for right in distinct.iter().skip(left + 1) {
                pairs.push(ordered(*index, *right));
            }
        }
    }
    let candidates = merge_runs(pairs, block)
        .into_iter()
        .map(|group| {
            let left = &scanned[group.a.file];
            let right = &scanned[group.b.file];
            let mut uncertainty = vec![
                "repetition is a fact and a judgement is not: a table-driven test, two implementations \
                 of one interface, a match arm per variant and a struct of column definitions all look \
                 exactly like this to a text scan"
                    .to_string(),
                "the offsets are positions among the *added, folded* lines, not file lines: blank lines \
                 and comments were dropped to make the copies matchable, so add them back to find the \
                 line in the file"
                    .to_string(),
            ];
            if widest > 2 {
                uncertainty.push(format!(
                    "{widest} copies of this block arrived in this episode; only the first pair is \
                     printed, and the block is the same one"
                ));
            }
            Candidate {
                kind: CandidateKind::Duplicated,
                subject: format!(
                    "{}@{} and {}@{}",
                    left.path, group.a.start, right.path, group.b.start
                ),
                module: left.path.to_string(),
                verdict: Confidence::Uncertain,
                age: None,
                since: None,
                why: format!(
                    "a block of {} added lines appears in both `{}` and `{}`, and both copies arrived \
                     in this episode",
                    group.lines.max(block),
                    left.path,
                    right.path
                ),
                uncertainty,
                evidence: vec![
                    format!("{}@{} (+{} added lines)", left.path, group.a.start, group.lines),
                    format!("{}@{} (+{} added lines)", right.path, group.b.start, group.lines),
                ],
                decided_by: Some(format!(
                    "hotspots.duplicate_block_lines = {}",
                    policy.duplicate_block_lines
                )),
            }
        })
        .collect();
    let mut notices: Vec<String> = refused
        .into_iter()
        .map(|(reason, count)| {
            format!("duplication sweep: {count} path(s) were not read, because {reason}")
        })
        .collect();
    if ignored_paths > 0 {
        notices.push(format!(
            "{ignored_paths} path(s) were excluded by `hotspots.ignore`"
        ));
    }
    build_report(
        revision,
        candidates,
        ReportCounts {
            symbols_checked: scanned.len(),
            new_symbols: 0,
            ignored_paths,
            gated_not_reported: 0,
        },
        notices,
        Some(format!(
            "{} file(s) carried enough new code to contain a block and none of their windows matched \
             another file's",
            scanned.len()
        )),
        policy,
    )
}

/// One file's folded added lines.
struct Scanned<'a> {
    path: &'a str,
    lines: Vec<String>,
    dialect: Dialect,
}

/// One occurrence of a hashed window: which scanned file, and where among its folded lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Place {
    file: usize,
    start: usize,
}

/// A merged run: two places and how many lines the copy spans.
struct Run {
    a: Place,
    b: Place,
    lines: usize,
}

/// Collapse a run's overlapping windows into one place per copy.
fn distinct_places(places: &[Place], block: usize) -> Vec<Place> {
    let mut sorted: Vec<Place> = places.to_vec();
    sorted.sort();
    let mut out: Vec<Place> = Vec::new();
    for place in sorted {
        if let Some(last) = out.last()
            && last.file == place.file
            && place.start.saturating_sub(last.start) < block
        {
            continue;
        }
        out.push(place);
    }
    out
}

/// Order a pair so `(a, b)` and `(b, a)` are the same candidate.
fn ordered(a: Place, b: Place) -> (Place, Place) {
    if a <= b { (a, b) } else { (b, a) }
}

/// Merge the pairs that describe the same copy: same two files, and both starts within one window of
/// each other. Sorted input, so comparing against the previous group is enough.
fn merge_runs(mut pairs: Vec<(Place, Place)>, block: usize) -> Vec<Run> {
    pairs.sort();
    let mut out: Vec<Run> = Vec::new();
    for (a, b) in pairs {
        if let Some(last) = out.last_mut()
            && last.a.file == a.file
            && last.b.file == b.file
            && a.start.saturating_sub(last.a.start) < block
            && b.start.saturating_sub(last.b.start) < block
        {
            last.lines = block
                + (a.start.saturating_sub(last.a.start)).max(b.start.saturating_sub(last.b.start));
            continue;
        }
        out.push(Run { a, b, lines: block });
    }
    out
}

/// Which refusal — if any — keeps this path out of the duplication sweep, in the words the report
/// prints.
///
/// Three families, each refused for a stated reason and counted: a table-driven test is repetition by
/// design, generated and vendored text is a copy of what a generator wrote, and prose carries blocks
/// that repeat on purpose.
#[must_use]
fn refused_path(path: &str) -> Option<&'static str> {
    let segments: Vec<&str> = path.split('/').collect();
    let file = segments.last().copied().unwrap_or(path);
    let lower = file.to_ascii_lowercase();
    let test_dir = ["tests", "test", "benches", "examples", "fixtures", "spec"];
    if segments
        .iter()
        .any(|segment| test_dir.contains(&segment.to_ascii_lowercase().as_str()))
        || lower.starts_with("test_")
        || lower.ends_with("_test.go")
        || lower.ends_with("_test.rs")
        || lower.ends_with(".test.ts")
        || lower.ends_with(".test.tsx")
        || lower.ends_with(".spec.ts")
        || lower.ends_with("_spec.rb")
    {
        return Some(
            "a table-driven test is repetition by design, and `e2.9` owns what a test case is",
        );
    }
    if segments
        .iter()
        .any(|segment| ["node_modules", "vendor", "generated", "gen"].contains(segment))
        || lower.ends_with(".pb.go")
        || lower.ends_with(".generated.rs")
        || lower.ends_with(".min.js")
        || lower.ends_with(".lock")
        || lower.starts_with("cargo.")
    {
        return Some("generated and vendored text is a copy of what a generator wrote");
    }
    for suffix in [
        ".md", ".rst", ".txt", ".json", ".yaml", ".yml", ".toml", ".svg", ".png", ".csv",
    ] {
        if lower.ends_with(suffix) {
            return Some("prose and configuration repeat on purpose and are not code");
        }
    }
    None
}

/// A window that is mostly `use`/`import`/`from` lines, which is the top of a big file and not a
/// duplicated block.
fn looks_like_imports(window: &[String]) -> bool {
    let imports = window
        .iter()
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            lower.starts_with("use ")
                || lower.starts_with("import ")
                || lower.starts_with("from ")
                || lower.starts_with("#[")
                || lower.starts_with("pub use ")
        })
        .count();
    imports.saturating_mul(2) >= window.len()
}

/// A stable, order-independent-per-line hash of a window. Not a cryptographic digest: a duplicate is
/// found by two windows agreeing, and a hostile file should not be able to make that cheap to fake —
/// hence FNV-1a over the folded lines rather than a checksum of the pointer order.
fn hash(window: &[String]) -> usize {
    let mut value: usize = 0xcbf2_9ce4_8422_2325;
    for line in window {
        for byte in line.as_bytes() {
            value ^= *byte as usize;
            value = value.wrapping_mul(0x0000_0100_0000_01b3);
        }
        value ^= b'\n' as usize;
        value = value.wrapping_mul(0x0000_0100_0000_01b3);
    }
    value
}

/// The counts a report carries, so the three sweeps share one constructor.
#[derive(Debug, Clone, Copy, Default)]
struct ReportCounts {
    symbols_checked: usize,
    new_symbols: usize,
    ignored_paths: usize,
    gated_not_reported: usize,
}

/// Assemble one report: the sort that makes two runs print identically, and the counts.
fn build_report(
    revision: &str,
    mut candidates: Vec<Candidate>,
    counts: ReportCounts,
    notices: Vec<String>,
    empty_reason: Option<String>,
    policy: &Policy,
) -> Report {
    candidates.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.subject.cmp(&b.subject)));
    let firm = candidates
        .iter()
        .filter(|candidate| candidate.verdict == Confidence::Firm)
        .count();
    let uncertain = candidates
        .iter()
        .filter(|candidate| candidate.verdict == Confidence::Uncertain)
        .count();
    let unverified = candidates
        .iter()
        .filter(|candidate| candidate.verdict == Confidence::Unverified)
        .count();
    let _ = policy;
    Report {
        contract: CONTRACT,
        revision: revision.to_string(),
        firm,
        uncertain,
        unverified,
        symbols_checked: counts.symbols_checked,
        new_symbols: counts.new_symbols,
        ignored_paths: counts.ignored_paths,
        ignored_patterns: policy.ignore.clone(),
        gated_not_reported: counts.gated_not_reported,
        candidates,
        notices,
        limits: LIMITS.iter().map(|line| (*line).to_string()).collect(),
        empty_reason,
    }
}

/// Run the unreferenced sweep and wrap it: the counts this sweep alone can answer.
#[must_use]
pub fn unreferenced_report(
    revision: &str,
    graph: &Graph,
    naming: &Naming,
    new: &[NewSymbol],
    window: usize,
    policy: &Policy,
    notices: Vec<String>,
) -> Report {
    let candidates = unreferenced(graph, naming, new, window, policy);
    let ignored = new
        .iter()
        .filter(|symbol| policy.ignores(&symbol.key))
        .count();
    build_report(
        revision,
        candidates,
        ReportCounts {
            symbols_checked: new.len(),
            new_symbols: new.len(),
            ignored_paths: ignored,
            gated_not_reported: 0,
        },
        notices,
        Some(if new.is_empty() {
            format!(
                "no symbol appeared in the last {window} revision(s), so there is nothing to judge"
            )
        } else {
            format!(
                "{} new symbol(s) were examined and every one of them is named by another module",
                new.len()
            )
        }),
        policy,
    )
}
