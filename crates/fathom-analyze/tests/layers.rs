//! The layering rules (`e3.6`, `reco-cfp`): a configured repository, the violations it seeds, the
//! violations it must not invent, and the delta that separates this turn from what the repository
//! already owed.
//!
//! The store-facing half — two revisions in a lane, one episode's answer — is `tests/layers_episode.rs`.
//! What lives here needs no store, which is the point: the judgment is a pure function of a graph, a
//! naming, a rules table and the line evidence, and ADR-002 §6.1 says a `--no-default-features` build
//! has to be able to say so.
//!
//! The fixture is a *configured example repository*, because that is the first acceptance criterion:
//! `[layers]` and `[layering]` are read out of a `fathom.toml` text by the same code the CLI reads, the
//! files are built by the same `graph::Builder` the lane uses, and the assertions are made against the
//! whole pipeline rather than against a hand-made violation list. A test that hand-wrote a `Violation`
//! could not fail to find one, which is not the same as finding one.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_analyze::cycles::debt::{Mode, Ratchet};
use fathom_analyze::graph::layers::Layers;
use fathom_analyze::graph::{BuildOutcome, Builder, GraphPolicy, Interner, ModuleInput};
use fathom_analyze::layering::engine::evaluate;
use fathom_analyze::layering::locate::Locate;
use fathom_analyze::layering::report::Delta;
use fathom_analyze::layering::{Report, RuleKind, Rules, Violation};
use fathom_analyze::structure::{
    Edge, FileLevel, FileStructure, Granularity, Language, Resolution, Span, Structure,
};
use fathom_core::time::Timestamp;

/// The example repository's configuration, verbatim: a four-layer system, bottom first, with one broad
/// refusal, one narrow exception, one ignored region and two structural rules.
const CONFIG: &str = r#"
[layers]
l0_vocab = ["crates/vocab"]
l1_store = ["crates/store"]
l2_core = ["crates/core"]
l3_api = ["crates/api"]

[layering]
mode = "warn"
order = ["l0_vocab", "l1_store", "l2_core", "l3_api"]
no_upward = true
no_cycle_within_layer = true
forbid = ["door-through-the-store: l3_api -> l1_store"]
allow = ["the-read-path: l3_api -> crates/store/src/read.rs"]
ignore = ["crates/**/tests/**"]
"#;

/// One dependency as its file states it: the target, the symbol it names, the line the statement is on,
/// and the two ways an edge can be less certain than that.
#[derive(Clone)]
struct Dep {
    target: &'static str,
    symbol: &'static str,
    line: usize,
    star: bool,
    package: Option<&'static str>,
}

/// A named import, the ordinary case.
fn uses(target: &'static str, symbol: &'static str, line: usize) -> Dep {
    Dep {
        target,
        symbol,
        line,
        star: false,
        package: None,
    }
}

/// A dependency whose contents nobody read: a wildcard leg (spike `s1` finding 2).
fn star(target: &'static str, line: usize) -> Dep {
    Dep {
        target,
        symbol: "*",
        line,
        star: true,
        package: None,
    }
}

/// A Go package import: the language's dependency is package-to-package (`s1` finding 4), recorded
/// against one file because that is the granularity the store keys on.
fn go(target: &'static str, package: &'static str, line: usize) -> Dep {
    Dep {
        target,
        symbol: "*",
        line,
        star: true,
        package: Some(package),
    }
}

/// One file of the example repository.
#[derive(Clone)]
struct File {
    path: &'static str,
    deps: Vec<Dep>,
}

fn file(path: &'static str, deps: Vec<Dep>) -> File {
    File {
        path,
        deps: deps.into_iter().collect(),
    }
}

/// The example repository with every seeded defect in it, and every near miss beside it.
fn seeded() -> Vec<File> {
    vec![
        file("crates/vocab/src/word.rs", vec![]),
        file("crates/tools/src/script.rs", vec![]),
        // Legal, twice over: downhill, and outside every refusal.
        file(
            "crates/core/src/engine.rs",
            vec![
                uses("crates/store/src/sink.rs", "Sink", 3),
                // The target is untagged: counted as untagged, never guessed into a layer.
                uses("crates/tools/src/script.rs", "Script", 4),
            ],
        ),
        // SEEDED 1 — an upward reference: l1_store points at l2_core.
        file(
            "crates/store/src/sink.rs",
            vec![uses("crates/core/src/engine.rs", "Engine", 5)],
        ),
        // SEEDED 2 — a second upward reference, so the per-rule roll-up has something to roll up.
        file(
            "crates/store/src/read.rs",
            vec![uses("crates/core/src/invoicing.rs", "Invoicing", 7)],
        ),
        file(
            "crates/api/src/router.rs",
            vec![
                // SEEDED 3 — the door reaches the store directly, which `forbid` refuses.
                uses("crates/store/src/sink.rs", "Sink", 9),
                // The same pair a second time, through a wildcard leg: one pair, one violation, and
                // the named row is the one cited.
                star("crates/store/src/sink.rs", 11),
                // The near miss that matters: the broad rule covers this edge and the narrow `allow`
                // excepts it.
                uses("crates/store/src/read.rs", "Read", 12),
            ],
        ),
        // The ignored region: an upward reference from a test file, which `ignore` forgives.
        file(
            "crates/api/tests/smoke.rs",
            vec![uses("crates/core/src/engine.rs", "Engine", 1)],
        ),
        // SEEDED 4 — a cycle inside one layer. Two edges, one violation: the identity is the member
        // set, not the pair.
        file(
            "crates/core/src/billing.rs",
            vec![uses("crates/core/src/invoicing.rs", "Invoicing", 2)],
        ),
        file(
            "crates/core/src/invoicing.rs",
            vec![uses("crates/core/src/billing.rs", "Billing", 6)],
        ),
    ]
}

/// The four violations `seeded()` plants, spelled as the keys a baseline would pin.
const SEEDED_KEYS: [&str; 4] = [
    "door-through-the-store::crates/api/src/router.rs->crates/store/src/sink.rs",
    "no_cycle_within_layer::crates/core/src/billing.rs|crates/core/src/invoicing.rs",
    "no_upward::crates/store/src/read.rs->crates/core/src/invoicing.rs",
    "no_upward::crates/store/src/sink.rs->crates/core/src/engine.rs",
];

/// The whole pipeline for one revision: config in, graph, line evidence and rules out.
struct Repo {
    outcome: BuildOutcome,
    locate: Locate,
    rules: Rules,
    notices: Vec<String>,
}

impl Repo {
    fn new(files: &[File], config: &str) -> Self {
        Self::with_layers(files, config, config)
    }

    /// A repository whose `[layers]` and `[layering]` come from two different documents — the shape a
    /// user's config and a repository's config actually arrive in.
    fn with_layers(files: &[File], layers_text: &str, config: &str) -> Self {
        let (layers, mut notices) = Layers::from_repo_config_text(layers_text);
        let (rules, rule_notices) = fathom_analyze::layering::load(&layers, config);
        notices.extend(rule_notices);
        let structures: Vec<FileStructure> = files.iter().map(structure_of).collect();
        let blobs: Vec<String> = files.iter().map(|f| format!("blob:{}", f.path)).collect();
        let inputs: Vec<ModuleInput<'_>> = files
            .iter()
            .zip(&structures)
            .zip(&blobs)
            .map(|((f, structure), blob)| ModuleInput {
                path: f.path,
                blob: blob.as_str(),
                file: Some(structure),
            })
            .collect();
        let locate = Locate::from_inputs(&inputs);
        let mut interner = Interner::default();
        let outcome = Builder::new(GraphPolicy::default(), layers).build(&inputs, &mut interner);
        Self {
            outcome,
            locate,
            rules,
            notices,
        }
    }

    fn assess(&self) -> fathom_analyze::layering::Assessment {
        evaluate(
            &self.outcome.graph,
            &self.outcome.naming(),
            &self.rules,
            &self.locate,
        )
    }

    /// The violation keys, sorted — the comparison every assertion in this file is made of.
    fn keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self
            .assess()
            .violations
            .iter()
            .map(Violation::key)
            .collect();
        keys.sort();
        keys
    }

    fn violations(&self) -> Vec<Violation> {
        self.assess().violations
    }

    fn find(&self, needle: &str) -> Option<Violation> {
        self.violations()
            .into_iter()
            .find(|violation| violation.key().contains(needle))
    }

    /// The report the way a surface would read it, with no baseline and no episode.
    fn report(&self) -> Report {
        fathom_analyze::layering::check(
            &self.outcome.graph,
            &self.outcome.naming(),
            &self.rules,
            &self.locate,
            &Ratchet::empty(),
            now(),
        )
    }
}

/// One file's structural record, with the import statements and the lines they sit on.
fn structure_of(f: &File) -> FileStructure {
    let span = Span {
        start_byte: 0,
        end_byte: 1,
        start_line: 1,
        end_line: 1,
    };
    let stem = f
        .path
        .rsplit('/')
        .next()
        .unwrap_or(f.path)
        .trim_end_matches(".rs")
        .trim_end_matches(".go")
        .to_string();
    let exports = vec![stem.clone()];
    let symbols = vec![fathom_analyze::structure::Symbol {
        kind: fathom_analyze::structure::SymbolKind::Fn,
        name: stem.clone(),
        container: String::new(),
        span,
        visibility: fathom_analyze::structure::Visibility::Public,
        exported: true,
        complexity: fathom_analyze::structure::Complexity {
            body_lines: 1,
            branches: 0,
            loops: 0,
            params: 0,
            returns: 0,
            members: 0,
            nesting: 0,
        },
        modifiers: Vec::new(),
    }];
    let mut imports = Vec::new();
    let mut edges = Vec::new();
    for dep in &f.deps {
        imports.push(fathom_analyze::structure::Import {
            specifier: format!("{}::{}", dep.target, dep.symbol),
            symbol: dep.symbol.to_string(),
            form: if dep.star { "star_import" } else { "use" }.to_string(),
            reexport: false,
            span: Span {
                start_byte: dep.line * 10,
                end_byte: dep.line * 10 + 9,
                start_line: dep.line,
                end_line: dep.line,
            },
        });
        let mut edge = if dep.star {
            Edge::wildcard(dep.target)
        } else {
            Edge::resolved(dep.target, dep.symbol)
        };
        edge.form = if dep.star { "star_import" } else { "use" }.to_string();
        if let Some(package) = dep.package {
            edge.granularity = Granularity::Package;
            edge.package = Some(package.to_string());
        }
        edges.push(edge);
    }
    let structure = Structure {
        symbols,
        exports,
        imports,
        edges,
        supertypes: Vec::new(),
        synthetic_scopes: Vec::new(),
        notes: Vec::new(),
        resolution: Resolution {
            passes_used: 1,
            converged: true,
            bound: 4,
            truncated_chains: 0,
        },
        package: None,
    };
    FileStructure {
        path: f.path.to_string(),
        language: Language::Rust,
        file_level: FileLevel {
            path: f.path.to_string(),
            language: Language::Rust,
            bytes: 64,
            newlines: 12,
            blank_lines: 1,
            code_lines: 10,
            parse_errors: 0,
            refused: false,
        },
        structure: Some(structure),
        reason: None,
    }
}

/// A `now` for the ledger assertions. Nothing here reads a clock, so a test can ask about the day
/// before an allowance lapses and the day after.
fn now() -> Timestamp {
    Timestamp::from_unix_micros(1_700_000_000_000_000)
}

/// One day in microseconds, for the expiry arithmetic.
const DAY: i64 = 86_400_000_000;

/// An allowance row for a layering key, in the shape `docs/concepts/ratchet.md` publishes.
fn allowance(key: &str, owner: &str, expires_days: i64) -> String {
    let day = Timestamp::from_unix_micros(now().as_unix_micros() + expires_days * DAY);
    let (year, month, date) = day.civil();
    format!(
        "[[allowance]]\nkind = \"layer\"\nkey = \"{key}\"\nowner = \"{owner}\"\nexpires = \
         \"{year:04}-{month:02}-{date:02}\"\nreason = \"a written reason, as the format demands\"\n"
    )
}

/// The violation keys of a report's news list.
fn news(report: &Report) -> Vec<String> {
    report
        .verdict
        .violations
        .iter()
        .map(|entry| entry.violation.key())
        .collect()
}

/// The debt keys of a report's ledger.
fn debts(report: &Report) -> Vec<String> {
    report
        .verdict
        .debt
        .iter()
        .map(|debt| debt.key.clone())
        .collect::<Vec<String>>()
}

/// The keys of a violation list, sorted: the comparison the delta tests are made of.
fn sorted(violations: &[Violation]) -> Vec<String> {
    let mut keys: Vec<String> = violations.iter().map(Violation::key).collect();
    keys.sort();
    keys
}

// ===========================================================================
// AC 1 — a configured example repo yields zero false positives and catches every
// seeded violation in the fixture
// ===========================================================================
#[test]
fn the_configured_repo_finds_exactly_what_was_seeded() {
    let repo = Repo::new(&seeded(), CONFIG);
    assert_eq!(
        repo.rules.refused, 0,
        "the fixture's own configuration must load clean: {:?}",
        repo.notices
    );
    assert_eq!(
        repo.keys(),
        SEEDED_KEYS
            .iter()
            .map(|key| key.to_string())
            .collect::<Vec<String>>(),
        "the finding set must be the seeded set, neither wider nor narrower"
    );
}

#[test]
fn the_near_misses_are_counted_as_judged_and_not_as_findings() {
    let assessment = Repo::new(&seeded(), CONFIG).assess();
    let counters = assessment.counters;
    assert_eq!(
        counters.permitted, 1,
        "the narrow `allow` excepts exactly the read path, and the broad refusal still fires on \
         the sink"
    );
    assert!(
        counters.skipped_ignored >= 1,
        "the ignored test file's edge is skipped and said: {counters:?}"
    );
    assert!(
        counters.untagged >= 1,
        "the untagged tools file is counted, not guessed into a layer: {counters:?}"
    );
    assert_eq!(
        counters.rules, 4,
        "forbid, allow, no_upward and no_cycle_within_layer are the four rules in force"
    );
    assert_eq!(
        counters.cycles, 2,
        "the within-layer pair and the cross-layer engine/sink loop are both cycles; only one of \
         them is a violation here"
    );
    assert_eq!(counters.refused, 0);
}

#[test]
fn the_cross_layer_cycle_is_not_a_within_layer_violation() {
    // `engine.rs → sink.rs → engine.rs` is a real cycle in the fixture. With `no_cycles` off and
    // `no_cycle_within_layer` on, it is a cycle the rules do not object to, and the two members sit
    // in different layers.
    let repo = Repo::new(&seeded(), CONFIG);
    assert!(
        !repo
            .keys()
            .iter()
            .any(|key| key.contains("engine.rs|crates/store/src/sink.rs")),
        "a cycle that spans two layers is not a within-one-layer violation: {:?}",
        repo.keys()
    );
}

#[test]
fn a_wildcard_leg_beside_a_named_one_is_one_violation_of_the_named_edge() {
    // `router.rs` reaches `sink.rs` twice: once by name, once through a wildcard. The violation is
    // the pair, so one finding, cited at the named row — and `unknown_contents` is false because the
    // citation is confident.
    let violations = Repo::new(&seeded(), CONFIG).violations();
    let hits: Vec<&Violation> = violations
        .iter()
        .filter(|violation| violation.dst.path == "crates/store/src/sink.rs")
        .collect();
    assert_eq!(hits.len(), 1, "one pair, one violation: {hits:?}");
    assert_eq!(hits[0].edge, "imports");
    assert!(!hits[0].unknown);
}

// ===========================================================================
// AC 3 — each violation names the rule, the offending edge with file:line and a fix hint
// ===========================================================================
#[test]
fn every_violation_names_its_rule_its_edge_line_and_fix() {
    let repo = Repo::new(&seeded(), CONFIG);
    let assessment = repo.assess();
    assert!(
        assessment.counters.statements_located > 0,
        "the line evidence was built from the same records as the graph"
    );
    for violation in &assessment.violations {
        assert!(!violation.rule.is_empty(), "{violation:?}");
        assert!(
            repo.rules.rule(&violation.rule).is_some(),
            "the violation names a rule that is actually in force: {}",
            violation.rule
        );
        assert!(violation.line.is_some(), "{violation:?} cites no line");
        assert!(
            violation.location().contains(':'),
            "a location without a line: {}",
            violation.location()
        );
        // An edge finding's reason names the far end; a cycle's names the layer its members share,
        // because the pair that closed the loop is not the fact about it.
        if violation.members.is_empty() {
            assert!(violation.why.contains(&violation.dst.path), "{violation:?}");
        } else {
            assert!(
                violation.why.contains(&violation.src.layer),
                "{violation:?}"
            );
        }
        assert!(!violation.fix.is_empty());
        assert!(
            violation
                .detail_lines()
                .iter()
                .any(|line| line.starts_with("fix: ")),
            "{violation:?}"
        );
    }
}

#[test]
fn the_line_named_is_the_line_the_statement_is_on() {
    let repo = Repo::new(&seeded(), CONFIG);
    let violation = repo
        .find("crates/store/src/sink.rs->crates/core/src/engine.rs")
        .expect("the seeded upward reference");
    assert_eq!(violation.line, Some(5), "sink.rs states it on line 5");
    assert_eq!(violation.location(), "crates/store/src/sink.rs:5");
    assert_eq!(violation.kind, RuleKind::NoUpward);
    assert!(
        violation.why.contains("l2_core") && violation.why.contains("l1_store"),
        "the reason names both layers, because that is the fact: {}",
        violation.why
    );
    assert!(
        violation.fix.contains("layering.order"),
        "the fix suggests the move that is usually right: {}",
        violation.fix
    );
}

#[test]
fn the_rule_name_a_violation_carries_is_the_label_the_author_wrote() {
    let repo = Repo::new(&seeded(), CONFIG);
    let violation = repo
        .find("door-through-the-store")
        .expect("the labelled rule");
    assert_eq!(violation.rule, "door-through-the-store");
    assert!(
        violation.declared_in.contains("layering.forbid"),
        "the violation says where the rule was declared: {}",
        violation.declared_in
    );
    assert!(
        violation
            .fix
            .contains("allow = [\"crates/api/src/router.rs -> crates/store/src/sink.rs\"]"),
        "the fix offers the narrow exception with the concrete pair already filled in: {}",
        violation.fix
    );
}

#[test]
fn an_unnamed_rule_is_named_from_its_own_content_not_its_position() {
    // A baseline keyed on a rule name must survive an unrelated rule being inserted above it, so the
    // automatic name is the rule's body. This is the difference between a debt and a surprise.
    let config = r#"
[layers]
db = ["crates/db"]
ui = ["crates/ui"]

[layering]
forbid = ["ui -> db", "db -> ui"]
"#;
    let files = vec![
        file(
            "crates/ui/panel.rs",
            vec![uses("crates/db/store.rs", "Store", 2)],
        ),
        file(
            "crates/db/store.rs",
            vec![uses("crates/ui/panel.rs", "Panel", 3)],
        ),
    ];
    let repo = Repo::new(&files, config);
    let mut keys = repo.keys();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "forbid:db->ui::crates/db/store.rs->crates/ui/panel.rs",
            "forbid:ui->db::crates/ui/panel.rs->crates/db/store.rs",
        ],
        "the names are content-derived, so reordering the file changes nothing"
    );
}

// ===========================================================================
// AC 4 — rules are validated at config load with actionable errors, not silent no-ops
// ===========================================================================

/// The load result for one `[layering]` document: how many rules survived, and what was said.
fn loaded(config: &str) -> (usize, usize, Vec<String>) {
    let (layers, _) = Layers::from_repo_config_text(config);
    let (rules, notices) = fathom_analyze::layering::load(&layers, config);
    (rules.len(), rules.refused, notices)
}

#[test]
fn a_layer_name_that_does_not_exist_is_refused_with_the_near_miss() {
    let config = r#"
[layers]
core = ["crates/core"]
store = ["crates/store"]

[layering]
forbid = ["ui -> core"]
"#;
    let (active, refused, notices) = loaded(config);
    assert_eq!(refused, 1, "a rule that can match nothing is not a rule");
    assert_eq!(active, 0, "and it is not in force: {:?}", active);
    let notice = notices.join("\n");
    assert!(notice.contains("`ui`"), "{notice}");
    assert!(
        notice.contains("names no layer declared in [layers]"),
        "the message has to say what is wrong: {notice}"
    );
}

#[test]
fn a_layer_name_with_no_layers_table_at_all_says_that_is_the_problem() {
    let (active, refused, notices) = loaded("[layering]\nforbid = [\"ui -> db\"]\n");
    assert_eq!(refused, 1);
    assert_eq!(active, 0);
    assert!(
        notices
            .join("\n")
            .contains("No [layers] table is declared, so nothing is a layer name here"),
        "{notices:?}"
    );
}

#[test]
fn no_upward_without_an_order_is_refused_and_switched_off() {
    // An upward reference is only upward relative to a declared order. Guessing one from the order the
    // `[layers]` table happened to be written in would be inventing an architecture.
    let config = r#"
[layers]
core = ["crates/core"]
api = ["crates/api"]

[layering]
no_upward = true
"#;
    let (active, refused, notices) = loaded(config);
    assert_eq!(refused, 1);
    assert_eq!(
        active, 0,
        "the rule is OFF, not silently passing everything"
    );
    assert!(
        notices
            .join("\n")
            .contains("an upward reference is only upward relative to a declared order"),
        "{notices:?}"
    );
}

#[test]
fn a_rank_named_in_the_order_but_not_declared_is_refused() {
    let config = r#"
[layers]
core = ["crates/core"]

[layering]
order = ["core", "chapel"]
no_upward = true
"#;
    let (_, refused, notices) = loaded(config);
    assert_eq!(refused, 1);
    assert!(
        notices
            .join("\n")
            .contains("layering.order names `chapel`, which [layers] does not declare"),
        "{notices:?}"
    );
}

#[test]
fn a_layer_declared_but_not_ranked_is_stated_not_quietly_unjudged() {
    let config = r#"
[layers]
core = ["crates/core"]
side = ["crates/side"]

[layering]
order = ["core"]
no_upward = true
"#;
    let (_, refused, notices) = loaded(config);
    assert_eq!(
        refused, 0,
        "an unranked layer is a caveat, not a refused rule"
    );
    assert!(
        notices
            .join("\n")
            .contains("cannot be judged for upwardness and is counted as unranked"),
        "{notices:?}"
    );
}

#[test]
fn an_arrow_that_disagrees_with_the_list_it_is_in_is_refused() {
    // `only` means "may be reached only from". Reading `a -> b` in that list would enforce the exact
    // opposite of what the author wrote, which is the worst possible silent misunderstanding.
    let config = r#"
[layers]
core = ["crates/core"]
api = ["crates/api"]

[layering]
only = ["api -> core"]
"#;
    let (active, refused, notices) = loaded(config);
    assert_eq!(refused, 1);
    assert_eq!(active, 0);
    assert!(
        notices
            .join("\n")
            .contains("A boundary is written `target <- the only source that may reach it`"),
        "{notices:?}"
    );
}

#[test]
fn a_rule_with_no_arrow_at_all_names_both_spellings() {
    let config = r#"
[layers]
core = ["crates/core"]

[layering]
forbid = ["everything"]
"#;
    let (active, refused, notices) = loaded(config);
    assert_eq!(refused, 1);
    assert_eq!(active, 0);
    let notice = notices.join("\n");
    assert!(notice.contains("names no arrow"), "{notice}");
    assert!(
        notice.contains("<-"),
        "the fix has to show the other form: {notice}"
    );
}

#[test]
fn a_duplicated_rule_is_refused_because_it_would_move_a_name() {
    let config = r#"
[layers]
core = ["crates/core"]
api = ["crates/api"]

[layering]
forbid = ["api -> core", "api -> core"]
"#;
    let (active, refused, notices) = loaded(config);
    assert_eq!(active, 1, "the first one stands");
    assert_eq!(refused, 1);
    assert!(
        notices
            .join("\n")
            .contains("would move a baseline that pins it"),
        "{notices:?}"
    );
}

#[test]
fn an_allow_that_excepts_nothing_is_refused() {
    // The exact no-op the acceptance criterion names: a permission with no rule to except looks like
    // configuration and enforces nothing.
    let config = r#"
[layers]
core = ["crates/core"]
api = ["crates/api"]

[layering]
allow = ["api -> core"]
"#;
    let (active, refused, notices) = loaded(config);
    assert_eq!(refused, 1);
    assert_eq!(active, 0);
    assert!(
        notices.join("\n").contains("names an exception to nothing"),
        "{notices:?}"
    );
}

#[test]
fn an_allow_that_is_not_narrower_than_any_forbid_is_refused_but_a_path_exception_survives() {
    let broad = r#"
[layers]
core = ["crates/core"]
api = ["crates/api"]

[layering]
forbid = ["api -> core"]
allow = ["also: api -> core"]
"#;
    let (active, refused, notices) = loaded(broad);
    assert_eq!(
        refused, 1,
        "a rule and its exact exception are a contradiction"
    );
    assert_eq!(active, 1);
    assert!(
        notices.join("\n").contains("cannot except anything"),
        "{notices:?}"
    );

    let narrow = CONFIG;
    let (active, refused, notices) = loaded(narrow);
    assert_eq!(refused, 0, "{notices:?}");
    assert_eq!(active, 4, "the narrow path exception is in force");
}

#[test]
fn a_missing_layering_table_is_a_stated_state_and_not_a_clean_bill() {
    let (rules, notices) = fathom_analyze::layering::load(&Layers::default(), "");
    assert!(rules.is_inert());
    assert_eq!(rules.refused, 0, "declaring nothing is not an error");
    assert!(
        notices
            .join("\n")
            .contains("so nothing is enforced: the layer tags are read and reported"),
        "{notices:?}"
    );
}

#[test]
fn mode_off_reports_nothing_and_says_that_it_chose_not_to_look() {
    let config = CONFIG.replace("mode = \"warn\"", "mode = \"off\"");
    let repo = Repo::new(&seeded(), &config);
    let assessment = repo.assess();
    assert!(assessment.violations.is_empty());
    assert_eq!(assessment.counters.checked, 0, "nothing was examined");
    assert!(
        assessment
            .notices
            .iter()
            .any(|notice| notice.contains("none were applied")),
        "{:?}",
        assessment.notices
    );
}

#[test]
fn the_report_bound_stops_the_list_and_prints_the_number_behind_it() {
    let config = CONFIG.replace("mode = \"warn\"", "mode = \"warn\"\nmax_violations = 2");
    let repo = Repo::new(&seeded(), &config);
    let assessment = repo.assess();
    assert_eq!(assessment.violations.len(), 2);
    assert_eq!(
        assessment.found, 4,
        "four were found even though two are listed"
    );
    assert!(assessment.truncated);
    let report = repo.report();
    assert!(
        report
            .sentence()
            .contains("the report bound stopped the list")
    );
}

// ===========================================================================
// AC 2 — the per-episode delta, baseline-aware
// ===========================================================================

/// The repository as it stood before the turn under review: everything in `seeded()` except the two
/// upward references, which is to say two of the four violations are already there.
fn before_turn() -> Vec<File> {
    vec![
        file("crates/vocab/src/word.rs", vec![]),
        file("crates/tools/src/script.rs", vec![]),
        file(
            "crates/core/src/engine.rs",
            vec![
                uses("crates/store/src/sink.rs", "Sink", 3),
                uses("crates/tools/src/script.rs", "Script", 4),
            ],
        ),
        file("crates/store/src/sink.rs", vec![]),
        file("crates/store/src/read.rs", vec![]),
        file(
            "crates/api/src/router.rs",
            vec![
                uses("crates/store/src/sink.rs", "Sink", 9),
                star("crates/store/src/sink.rs", 11),
                uses("crates/store/src/read.rs", "Read", 12),
            ],
        ),
        file(
            "crates/api/tests/smoke.rs",
            vec![uses("crates/core/src/engine.rs", "Engine", 1)],
        ),
        file(
            "crates/core/src/billing.rs",
            vec![uses("crates/core/src/invoicing.rs", "Invoicing", 2)],
        ),
        file(
            "crates/core/src/invoicing.rs",
            vec![uses("crates/core/src/billing.rs", "Billing", 6)],
        ),
    ]
}

/// One episode's report: `before` is the revision the turn started at.
fn episode(before: &Repo, after: &Repo, ratchet: &Ratchet) -> Report {
    let start = before.assess();
    let end = after.assess();
    Report::build(
        &after.rules,
        end,
        Some(&start),
        &after.locate,
        ratchet,
        now(),
    )
}

/// A report with nothing to compare to, which is what a cold index produces.
fn snapshot(repo: &Repo, ratchet: &Ratchet) -> Report {
    Report::build(
        &repo.rules,
        repo.assess(),
        None,
        &repo.locate,
        ratchet,
        now(),
    )
}

#[test]
fn the_delta_separates_this_turn_from_what_was_already_there() {
    let before = Repo::new(&before_turn(), CONFIG);
    let after = Repo::new(&seeded(), CONFIG);
    let report = episode(&before, &after, &Ratchet::empty());
    assert_eq!(
        report.delta.introduced,
        vec![
            "no_upward::crates/store/src/read.rs->crates/core/src/invoicing.rs",
            "no_upward::crates/store/src/sink.rs->crates/core/src/engine.rs",
        ]
    );
    assert!(report.delta.resolved.is_empty());
    assert_eq!(report.delta.persisting, 2);
    assert_eq!(
        report.new_violations(),
        2,
        "the reviewer's number is the turn's"
    );
    assert_eq!(
        report.violations_count(),
        4,
        "the repository's number is all four"
    );
    let introduced: Vec<bool> = report
        .verdict
        .violations
        .iter()
        .map(|entry| entry.introduced)
        .collect();
    assert_eq!(introduced.iter().filter(|flag| **flag).count(), 2);
    assert!(
        report
            .verdict
            .violations
            .iter()
            .any(|entry| !entry.introduced && entry.sentence().contains("pre-existing")),
        "a violation that was already there says so in the line a pane prints"
    );
}

#[test]
fn an_episode_that_fixes_a_violation_reports_it_resolved() {
    let before = Repo::new(&seeded(), CONFIG);
    let after = Repo::new(&before_turn(), CONFIG);
    let report = episode(&before, &after, &Ratchet::empty());
    assert_eq!(report.resolved_violations(), 2);
    assert_eq!(report.new_violations(), 0);
    assert_eq!(report.delta.persisting, 2);
    assert!(
        report
            .sentence()
            .contains("0 violation(s) introduced, 2 resolved")
    );
}

#[test]
fn a_baselined_violation_is_debt_and_never_news() {
    let key = "no_upward::crates/store/src/sink.rs->crates/core/src/engine.rs";
    let ratchet = Ratchet::from_text(&allowance(key, "team-store", 30), "ratchet.toml");
    let report = snapshot(&Repo::new(&seeded(), CONFIG), &ratchet);
    assert_eq!(debts(&report), vec![key.to_string()]);
    assert_eq!(report.baselined_violations(), 1);
    assert_eq!(
        report.violations_count(),
        3,
        "the debt is a different box, not a dimmer setting"
    );
    assert!(!news(&report).iter().any(|found| found == key));
    let debt = &report.verdict.debt[0];
    assert_eq!(debt.owner, "team-store");
    assert_eq!(debt.days_left, Some(30));
    assert!(debt.sentence().contains("expires"), "{}", debt.sentence());
}

#[test]
fn a_lapsed_allowance_comes_back_due_with_the_day_it_lapsed() {
    let key = "no_upward::crates/store/src/sink.rs->crates/core/src/engine.rs";
    let ratchet = Ratchet::from_text(&allowance(key, "team-store", -5), "ratchet.toml");
    let report = snapshot(&Repo::new(&seeded(), CONFIG), &ratchet);
    assert_eq!(report.verdict.back.len(), 1);
    assert!(report.verdict.debt.is_empty());
    let entry = report
        .verdict
        .violations
        .iter()
        .find(|entry| entry.violation.key() == key)
        .expect("the lapsed violation is news again");
    assert!(entry.why.contains("expired"), "{}", entry.why);
}

#[test]
fn an_allowance_whose_violation_went_away_is_offered_and_never_applied() {
    let ratchet = Ratchet::from_text(
        &allowance("no_upward::gone.rs->also_gone.rs", "team-gone", 30),
        "ratchet.toml",
    );
    let report = snapshot(&Repo::new(&seeded(), CONFIG), &ratchet);
    assert_eq!(report.verdict.stale.len(), 1);
    assert!(
        report
            .verdict
            .sentence()
            .contains("could be removed as the violations went away"),
        "{}",
        report.verdict.sentence()
    );
    assert_eq!(
        ratchet.allowances().len(),
        1,
        "the file is the record of a decision; this module does not delete it"
    );
}

#[test]
fn a_missing_baseline_is_stated_rather_than_treated_as_an_empty_ledger() {
    let (ratchet, notices) = Ratchet::read(std::path::Path::new("/nonexistent-layers"), "");
    assert!(!ratchet.loaded);
    assert!(
        notices
            .iter()
            .any(|notice| notice.contains("counts as new")),
        "{notices:?}"
    );
    let report = snapshot(&Repo::new(&seeded(), CONFIG), &ratchet);
    assert_eq!(report.violations_count(), 4);
    assert!(
        report.verdict.sentence().contains("no baseline was found"),
        "{}",
        report.verdict.sentence()
    );
}

#[test]
fn deny_mode_asks_for_the_check_exit_code_and_warn_does_not() {
    let denying = CONFIG.replace("mode = \"warn\"", "mode = \"deny\"");
    let repo = Repo::new(&seeded(), &denying);
    let report = snapshot(&repo, &Ratchet::empty());
    assert!(report.alarms());
    assert_eq!(report.exit_code().map(|code| code.as_i32()), Some(7));
    let warning = snapshot(&Repo::new(&seeded(), CONFIG), &Ratchet::empty());
    assert!(warning.alarms());
    assert_eq!(warning.exit_code(), None);
    assert_eq!(
        Mode::Warn,
        Mode::default(),
        "and warn is the default the key spec declares"
    );
}

// ===========================================================================
// the key, which is the whole matching contract
// ===========================================================================
#[test]
fn an_authored_key_meets_the_generated_one_in_every_spelling() {
    use fathom_analyze::layering::normalize_key;
    let canonical = "no_upward::a.rs->b.rs";
    for spelling in [
        "no_upward::a.rs->b.rs",
        "no_upward:: a.rs -> b.rs ",
        "no_upward::a.rs → b.rs",
    ] {
        assert_eq!(normalize_key(spelling), canonical, "{spelling}");
    }
    // A cycle key is a member set, so the order it was written in cannot matter — the same promise
    // the cycle ledger makes about `a → b → a` and `a|b`.
    assert_eq!(
        normalize_key("no_cycles::c.rs → b.rs → a.rs → c.rs"),
        normalize_key("no_cycles::a.rs|b.rs|c.rs")
    );
    // And a key with no rule separator matches nothing, because guessing which rule a bare pair
    // forgives would be a baseline that quietly widened itself.
    assert_eq!(normalize_key("just/a/path.rs"), "just/a/path.rs");
}

#[test]
fn a_key_written_with_a_unicode_arrow_still_forgives_the_violation() {
    let ratchet = Ratchet::from_text(
        &allowance(
            "no_upward:: crates/store/src/sink.rs → crates/core/src/engine.rs",
            "team-store",
            10,
        ),
        "ratchet.toml",
    );
    let report = snapshot(&Repo::new(&seeded(), CONFIG), &ratchet);
    assert_eq!(
        report.baselined_violations(),
        1,
        "the arrow a human types is not the arrow the code prints, and both must match: {:?}",
        report.verdict.debt
    );
}

// ===========================================================================
// the granularity rule from `e0.12`'s checkpoint on this ticket
// ===========================================================================

/// The Go corpus: two packages, and dependencies that are package-to-package facts recorded per file.
fn go_repo(config: &str) -> Repo {
    Repo::new(
        &[
            file("cmd/main.go", vec![go("shape/circle.go", "shape", 3)]),
            file("shape/shape.go", vec![]),
            file("shape/circle.go", vec![go("cmd/main.go", "cmd", 4)]),
        ],
        config,
    )
}

#[test]
fn a_package_granular_edge_is_judged_at_package_granularity() {
    let repo = go_repo(
        r#"
[layers]
shape = ["shape"]
cmd = ["cmd"]

[layering]
forbid = ["shape -> cmd"]
"#,
    );
    assert_eq!(repo.rules.refused, 0);
    let violations = repo.violations();
    assert_eq!(violations.len(), 1, "{:?}", violations);
    let violation = &violations[0];
    // The finding is about the *packages*. `shape/circle.go → cmd/main.go` is a file-level rendering of
    // a dependency the language does not have (`s1` finding 4), and printing it as file-to-file would
    // be reporting a claim nobody made.
    assert_eq!(violation.src.path, "shape");
    assert_eq!(violation.dst.path, "cmd");
    assert!(violation.package);
    assert_eq!(violation.edge, "imports_package");
    assert!(
        violation
            .detail_lines()
            .iter()
            .any(|line| line.contains("names a *package*, not a file")),
        "{:?}",
        violation.detail_lines()
    );
}

#[test]
fn a_package_whose_files_disagree_is_not_judged_and_says_so() {
    // `[layers]` globs can cut a Go package in half. Where they do, the package has no single layer, so
    // the only honest answer is that the claim was not available — not a violation invented out of a
    // pattern's shape.
    let repo = go_repo(
        r#"
[layers]
circle = ["shape/circle.go"]
support = ["shape/shape.go"]
cmd = ["cmd"]

[layering]
forbid = ["circle -> cmd"]
"#,
    );
    assert_eq!(repo.rules.refused, 0);
    assert!(repo.violations().is_empty(), "{:?}", repo.violations());
    assert_eq!(
        repo.assess().counters.indeterminate,
        2,
        "both package rows involve the split package"
    );
    assert!(
        repo.assess()
            .counters
            .sentence()
            .contains("no single layer tag and were not judged"),
        "{}",
        repo.assess().counters.sentence()
    );
}

// ===========================================================================
// boundary rules: only the api crate may import the db layer
// ===========================================================================

const BOUNDARY_CONFIG: &str = r#"
[layers]
api = ["crates/api"]
store = ["crates/store"]

[layering]
only = ["store <- crates/api"]
"#;

#[test]
fn a_boundary_rule_names_who_may_reach_the_layer() {
    let files = vec![
        file("crates/store/keeper.rs", vec![]),
        file(
            "crates/store/sink.rs",
            vec![uses("crates/store/keeper.rs", "Keeper", 2)],
        ),
        file(
            "crates/other/worker.rs",
            vec![uses("crates/store/sink.rs", "Sink", 6)],
        ),
        file(
            "crates/api/router.rs",
            vec![uses("crates/store/sink.rs", "Sink", 1)],
        ),
    ];
    let repo = Repo::new(&files, BOUNDARY_CONFIG);
    assert_eq!(repo.rules.refused, 0);
    let violations = repo.violations();
    assert_eq!(violations.len(), 2, "{:?}", violations);
    let sources: Vec<&str> = violations
        .iter()
        .map(|violation| violation.src.path.as_str())
        .collect();
    assert_eq!(
        sources,
        vec!["crates/other/worker.rs", "crates/store/sink.rs"]
    );
    assert!(
        !sources.contains(&"crates/api/router.rs"),
        "the named source is permitted, which is the whole point of the rule"
    );
    // Intra-layer reach is reach: `store` is not one of the permitted sources unless the author says
    // it is, and the next test says it, which is the other half of the contract.
    let why = &violations[0].why;
    assert!(why.contains("may be reached only from"), "{why}");
}

#[test]
fn a_boundary_rule_forgives_intra_layer_reach_only_when_told_to() {
    let files = vec![
        file("crates/store/keeper.rs", vec![]),
        file(
            "crates/store/sink.rs",
            vec![uses("crates/store/keeper.rs", "Keeper", 2)],
        ),
        file(
            "crates/api/router.rs",
            vec![uses("crates/store/keeper.rs", "Keeper", 1)],
        ),
    ];
    let strict = Repo::new(&files, BOUNDARY_CONFIG);
    assert_eq!(
        strict.violations().len(),
        1,
        "store reaching store is reach"
    );
    let forgiving = Repo::new(
        &files,
        &BOUNDARY_CONFIG.replace(
            "only = [\"store <- crates/api\"]",
            "only = [\"store <- crates/api\", \"store <- store\"]",
        ),
    );
    assert!(
        forgiving.violations().is_empty(),
        "the companion rule says what the author meant: {:?}",
        forgiving.violations()
    );
}

// ===========================================================================
// the cycle rules: declared here, detected by `e3.5`
// ===========================================================================
#[test]
fn turning_on_no_cycles_adds_the_cross_layer_cycle_and_no_second_copy_of_the_other() {
    let config = CONFIG.replace("no_upward = true", "no_upward = true\nno_cycles = true");
    let repo = Repo::new(&seeded(), &config);
    let violations = repo.violations();
    let cycle_rules: Vec<&Violation> = violations
        .iter()
        .filter(|violation| violation.kind == RuleKind::NoCycles)
        .collect();
    assert_eq!(
        cycle_rules.len(),
        2,
        "both cycles are now violations, and the within-layer one is *also* a within-layer finding: \
         {:?} of {violations:?}",
        cycle_rules
    );
    assert_eq!(
        repo.assess().counters.cycles,
        2,
        "detected once, by the cycle engine"
    );
    let one = cycle_rules
        .iter()
        .find(|violation| {
            violation.members.len() == 2
                && violation
                    .members
                    .iter()
                    .any(|member| member.contains("store"))
                && violation
                    .members
                    .iter()
                    .any(|member| member.contains("core"))
        })
        .expect("the cross-layer cycle");
    assert_eq!(
        one.src.path, "crates/core/src/engine.rs",
        "the offending edge is the one that closes the loop, so the finding can cite a line"
    );
    assert_eq!(one.dst.path, "crates/store/src/sink.rs");
    assert!(one.line.is_some());
    assert!(
        one.fix.contains("kind = \"layer\""),
        "the fix says how to make it debt, with the key to write: {}",
        one.fix
    );
}

#[test]
fn a_cycle_violation_carries_its_members_and_witness_alongside_the_closing_edge() {
    let repo = Repo::new(&seeded(), CONFIG);
    let cycle = repo
        .find("no_cycle_within_layer")
        .expect("the seeded within-layer cycle");
    assert_eq!(
        cycle.members,
        vec!["crates/core/src/billing.rs", "crates/core/src/invoicing.rs"]
    );
    assert_eq!(cycle.witness.len(), 2);
    assert!(cycle.exact, "a two-member component is proven shortest");
    assert!(
        cycle
            .detail_lines()
            .iter()
            .any(|line| line.starts_with("why: ") && line.contains("l2_core")),
        "the reason names the layer all the members share: {:?}",
        cycle.detail_lines()
    );
}

// ===========================================================================
// the report as the export boundary
// ===========================================================================
#[test]
fn the_exported_numbers_are_there_and_rederivable_from_the_lists() {
    let report = snapshot(&Repo::new(&seeded(), CONFIG), &Ratchet::empty());
    let json = report.to_json();
    assert_eq!(json["violations"].as_u64(), Some(4));
    assert_eq!(
        json["new_violations"].as_u64(),
        Some(0),
        "a snapshot has no episode"
    );
    assert_eq!(json["baselined_violations"].as_u64(), Some(0));
    assert_eq!(json["resolved_violations"].as_u64(), Some(0));
    assert!(json["edges_checked"].as_u64().unwrap() > 8);
    assert_eq!(json["rules_refused"].as_u64(), Some(0));
    assert_eq!(json["violation_list"].as_array().unwrap().len(), 4);
    assert_eq!(json["rules"]["order"].as_array().unwrap().len(), 4);
    assert!(
        json["violation_list"][0]["fix"].is_string(),
        "every exported violation carries its fix: {}",
        json["violation_list"][0]
    );
    assert!(
        json["by_rule"][0]["violations"].is_number(),
        "the roll-up a card prints: {}",
        json["by_rule"]
    );
}

#[test]
fn the_delta_document_names_what_moved() {
    let delta = Delta::of(
        &Repo::new(&before_turn(), CONFIG).violations(),
        &Repo::new(&seeded(), CONFIG).violations(),
    );
    let json = delta.to_json();
    assert_eq!(json["introduced"].as_array().unwrap().len(), 2);
    assert_eq!(json["before"].as_u64(), Some(2));
    assert_eq!(json["after"].as_u64(), Some(4));
    assert_eq!(json["persisting"].as_u64(), Some(2));
    assert!(delta.sentence().starts_with("2 violation(s) introduced"));
}

#[test]
fn with_no_line_evidence_a_violation_says_which_line_it_cannot_give() {
    let repo = Repo::new(&seeded(), CONFIG);
    let assessment = evaluate(
        &repo.outcome.graph,
        &repo.outcome.naming(),
        &repo.rules,
        &Locate::empty(),
    );
    assert_eq!(
        assessment.violations.len(),
        4,
        "the findings do not depend on the lines"
    );
    for violation in &assessment.violations {
        assert_eq!(violation.line, None);
        assert!(violation.line_note.is_some(), "{violation:?}");
        assert_eq!(violation.location(), violation.src.path);
        assert!(
            violation
                .detail_lines()
                .iter()
                .any(|line| line.contains("only the file can be cited")),
            "{:?}",
            violation.detail_lines()
        );
    }
}

#[test]
fn the_per_rule_rollup_counts_the_news_and_not_the_debt() {
    let key = "no_upward::crates/store/src/sink.rs->crates/core/src/engine.rs";
    let ratchet = Ratchet::from_text(&allowance(key, "team-store", 30), "ratchet.toml");
    let report = snapshot(&Repo::new(&seeded(), CONFIG), &ratchet);
    let rolled = report.by_rule();
    assert_eq!(
        rolled,
        vec![
            ("door-through-the-store".to_string(), 1),
            ("no_cycle_within_layer".to_string(), 1),
            ("no_upward".to_string(), 1),
        ],
        "sorted by count then by name, so the same facts print in the same order twice"
    );
    assert!(!report.summary_lines(2).is_empty());
}

#[test]
fn the_keys_of_a_violation_list_are_sorted_and_deduplicated() {
    let violations = Repo::new(&seeded(), CONFIG).violations();
    let keys = sorted(&violations);
    assert_eq!(keys.len(), 4);
    assert_eq!(
        keys.iter().collect::<Vec<_>>(),
        SEEDED_KEYS.iter().collect::<Vec<_>>(),
        "the assessment's order is the sorted key order, not the row-walk order"
    );
}
