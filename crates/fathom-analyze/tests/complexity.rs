//! Per-function cyclomatic complexity (`e8.2`, `reco-l9bz.2`): the ADR as a contract, not as a prose
//! promise.
//!
//! The corpus harness (`examples/complexity_harness.rs`) owns *accuracy* — hand-counted figures, the
//! per-language figures a person derived from ADR-007 §4, and C-KIND against the pinned grammars. This
//! file owns the four things a harness with a `--check` flag structurally cannot own, because they are
//! claims about the *documents* rather than about a measurement:
//!
//! * **One vocabulary (§7.1).** The ADR's table, the `structure::UnavailableReason` enum and the wording
//!   a surface prints are three artefacts that a normal edit touches one at a time. Here they are
//!   compared line by line, including the sentence: a code whose printed wording drifted from the ADR is
//!   a card telling a different story from the contract.
//! * **Unknown is never zero (§7.1, D10).** Asserted on inputs the corpus does not contain, because the
//!   corpus is hand-authored to be right and this is the check for when it is not.
//! * **A spent bound changes the state and the code (§7.2).** The bug the ADR names — a discarded
//!   `truncated` flag — is small enough to reintroduce in an afternoon, and a figure that quietly stopped
//!   counting at node 8 000 is the one silent undercount §7.2 says it will not carry.
//! * **The arithmetic is the arithmetic (§3.2, §5.1).** `V(G) = 1 + decision points`, `Σ` and `max` over
//!   the units that had a figure, restated as code so a refactor cannot move the base from 1 to 0 and
//!   still print a plausible table.
//!
//! Nothing here re-scores the corpus. Two copies of a scoring rule drift, and the second one always
//! goes quiet first.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout)]
// The complexity module exists only where at least one grammar is compiled in (`ADR-002` §6.5), so the
// whole file goes with it. A grammar-less build has no counting to contract about, and the house pattern
// for a test whose subject is gated out is the file-level `cfg`, not a runtime skip — the same shape
// `tests/episodes.rs` and `tests/ledger.rs` use for the store-gated modules.
#![cfg(any(
    feature = "lang-rust",
    feature = "lang-python",
    feature = "lang-typescript",
    feature = "lang-go",
    feature = "lang-ruby",
    feature = "lang-java"
))]

use fathom_analyze::structure::{ExtractPolicy, Language, UnavailableReason, complexity};
use std::path::{Path, PathBuf};

/// True when this build can count every language the test needs.
///
/// The corpus-backed assertions are about counting, and a build with a grammar gated out (ADR-002
/// §6.5) cannot count that language at all — it refuses with `grammar_not_compiled`, which is the right
/// answer and the wrong subject. CI runs this file with `langs-all`; a developer running it with fewer
/// features gets a note naming what was skipped rather than a failure about a grammar they never asked
/// for. The vocabulary and arithmetic-of-the-ADR tests below are not gated: they read documents.
fn gated(languages: &[Language]) -> bool {
    let missing: Vec<&str> = languages
        .iter()
        .filter(|language| complexity::rules(**language).is_none())
        .map(|language| language.as_str())
        .collect();
    if missing.is_empty() {
        return true;
    }
    eprintln!(
        "note: skipped, this build has no grammar for: {} (run with `--features langs-all`)",
        missing.join(", ")
    );
    false
}

/// The code a report or unit carries, as a bare option of `&str`, so an assertion never moves the
/// reason out of the report it is checking.
fn code(reason: &Option<UnavailableReason>) -> Option<&str> {
    reason.as_ref().map(UnavailableReason::as_str)
}

fn adr_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/adr/0007-complexity.md")
}

fn adr() -> String {
    std::fs::read_to_string(adr_path()).expect("ADR-007 is part of the repository")
}

/// The §7.1 table, as `(code, printed sentence)` pairs — the two columns this crate is responsible for.
///
/// The last row of that table is not codes: it is `e3.1`'s ten scope marks, and the ADR says so out
/// loud ("The ten strings on the last row are not codes"). A row is a code row when its first cell
/// holds exactly one backticked token, which is also how a reader's eye separates the two.
fn adr_vocabulary() -> Vec<(String, String)> {
    let text = adr();
    let body = text
        .split("### 7.1 ")
        .nth(1)
        .and_then(|rest| rest.split("### 7.2 ").next())
        .expect("ADR-007 §7.1 exists");
    let mut out = Vec::new();
    for line in body.lines().filter(|line| line.starts_with("| `")) {
        let first = line.split('|').nth(1).unwrap_or_default();
        let codes: Vec<&str> = first
            .split('`')
            .skip(1)
            .step_by(2)
            .filter(|token| !token.trim().is_empty())
            .collect();
        if codes.len() != 1 {
            continue;
        }
        let sentence = line
            .split('|')
            .nth(5)
            .unwrap_or_default()
            .trim()
            .to_string();
        out.push((codes[0].to_string(), sentence));
    }
    out
}

/// Does the sentence the library prints fill the ADR's template?
///
/// The ADR's wording column is a template with holes — `{n}`, `{limit}`, `{key}`, and the grammar
/// feature `lang-x` — that the library fills with real values (ADR-007 §7.1 deliberately writes the
/// sentence in the library so "the phrase and the reason arrive together everywhere"). So the check is
/// not string equality: it is that every literal piece of the template appears, in order, in what gets
/// printed. Formatting goes first: the ADR italicises the sentence and backticks names, and neither is
/// part of what a card says.
fn fills(printed: &str, template: &str) -> bool {
    let strip = |text: &str| {
        text.replace(['*', '`'], "")
            .replace("lang-x", "{feature}")
            .trim()
            .to_string()
    };
    let printed = strip(printed);
    let template = strip(template);
    let mut rest = printed.as_str();
    let mut piece = String::new();
    let mut hole = false;
    let mut pieces: Vec<String> = Vec::new();
    for c in template.chars() {
        match c {
            '{' => hole = true,
            '}' => {
                hole = false;
                pieces.push(std::mem::take(&mut piece));
            }
            other if !hole => piece.push(other),
            _ => {}
        }
    }
    pieces.push(piece);
    for piece in pieces.iter().filter(|piece| !piece.trim().is_empty()) {
        let piece = piece.trim();
        match rest.find(piece) {
            Some(at) => rest = &rest[at + piece.len()..],
            None => return false,
        }
    }
    true
}

/// §7.1: the ADR's code list, the enum's `vocabulary()`, and nothing else.
#[test]
fn one_vocabulary_for_no_figure() {
    let from_adr = adr_vocabulary();
    assert!(
        from_adr.len() >= 17,
        "ADR-007 §7.1's table should carry at least the seventeen codes it counts itself; found {}",
        from_adr.len()
    );
    let adr_codes: Vec<&str> = from_adr.iter().map(|(code, _)| code.as_str()).collect();
    let enum_codes: Vec<&str> = UnavailableReason::vocabulary()
        .iter()
        .map(UnavailableReason::as_str)
        .collect();
    for code in enum_codes.iter() {
        assert!(
            adr_codes.contains(code),
            "`{code}` is in structure::UnavailableReason::vocabulary() but not in ADR-007 §7.1"
        );
    }
    for code in adr_codes.iter() {
        assert!(
            enum_codes.contains(code),
            "ADR-007 §7.1 names `{code}` and the vocabulary does not carry it"
        );
    }
    assert_eq!(
        enum_codes.len(),
        adr_codes.len(),
        "the vocabulary and the ADR disagree on how many codes there are: {:?} vs {:?}",
        enum_codes,
        adr_codes
    );
}

/// §7.1: exactly two codes mean "a `0` that is a fact", and every other code means "no figure".
#[test]
fn exactly_two_codes_are_measured_zeros() {
    let zeros: Vec<&str> = UnavailableReason::vocabulary()
        .iter()
        .filter(|reason| reason.is_measured_zero())
        .map(UnavailableReason::as_str)
        .collect();
    assert_eq!(
        zeros,
        vec!["absent_revision", "no_bodies"],
        "ADR-007 §7.1: fifteen codes mean no figure and two mean a zero that is a fact"
    );
}

/// §7.1, the wording column: what the library prints is what the contract says a surface prints.
///
/// Two codes are the exception by construction, and the ADR says so in the sentence it chose:
/// `body_not_visible` and `node_budget` print a *count* line, and the counts belong to the file rather
/// than to any one function, so the library builds them on `FileComplexity` rather than on the reason.
/// Those two are checked against a real file with a real count in it — the fixture below — rather than
/// against the reason in isolation.
#[test]
fn the_printed_wording_is_the_adrs_wording() {
    if !gated(&[Language::Rust]) {
        return;
    }
    let counted = complexity::measure(
        "src/wording.rs",
        b"pub trait Shape {\n    fn area(&self) -> f64;\n}\n\nimpl Shape for f64 {\n    \
         fn area(&self) -> f64 {\n        if *self < 0.0 { 0.0 } else { *self }\n    }\n}\n",
        &ExtractPolicy::default(),
    );
    assert_eq!(
        counted.unmeasured, 1,
        "the wording fixture must have one uncounted unit"
    );
    let starved = complexity::measure(
        "src/wording.rs",
        b"fn wide(x: i32) -> i32 {\n    let mut total = 0;\n    if x > 0 {\n        for i \
         in 0..x {\n            if i % 2 == 0 {\n                total += i;\n            }\n        \
         }\n    }\n    total\n}\n",
        &ExtractPolicy {
            max_complexity_nodes: 1,
            ..ExtractPolicy::default()
        },
    );
    assert_eq!(
        starved.unmeasured, 1,
        "the budget fixture must have one unit that ran out"
    );
    for (code, adr_sentence) in adr_vocabulary() {
        let Some(reason) = UnavailableReason::vocabulary()
            .into_iter()
            .find(|candidate| candidate.as_str() == code)
        else {
            continue;
        };
        let printed = match code.as_str() {
            "body_not_visible" => counted.unmeasured_sentence(),
            "node_budget" => starved.unmeasured_sentence(),
            _ => reason.phrase("complexity"),
        };
        if adr_sentence.starts_with("*(no sentence") || adr_sentence.starts_with("*(none") {
            assert!(
                printed.is_empty(),
                "`{code}` is a fact rather than a refusal, so it must print no sentence; got `{printed}`"
            );
            continue;
        }
        assert!(
            fills(&printed, &adr_sentence),
            "`{code}` prints `{printed}`, which does not fill ADR-007 §7.1's sentence `{adr_sentence}`"
        );
    }
}

/// §9.5 and §11.4: the basis token is the one the ADR registered, and it appears in the ADR itself.
#[test]
fn the_basis_token_is_the_registered_one() {
    assert_eq!(complexity::BASIS, "mccabe-1");
    assert!(
        adr().contains("mccabe-1"),
        "the ADR no longer spells the basis token this build stamps"
    );
    assert_eq!(complexity::BASE, 1, "ADR-007 §3.2: the base is one path");
}

/// §3.1 plus ADR-002 §6.5: the rule tables exist for exactly the grammars that ship, and a seventh
/// language has no table to be read with a grammar that was never gated.
#[test]
fn rule_tables_exist_for_the_shipped_grammars() {
    for language in complexity::LANGUAGES {
        let Some(rules) = complexity::rules(language) else {
            eprintln!(
                "note: {} has no table in this build (no grammar); skipped",
                language.as_str()
            );
            continue;
        };
        assert_eq!(rules.language, language);
        assert!(
            !rules.units.is_empty(),
            "{}'s table names no unit node kind",
            language.as_str()
        );
        let declared = rules.declared_kinds();
        assert!(
            !declared.is_empty(),
            "{}'s table declares no decision points at all",
            language.as_str()
        );
        for kind in declared.iter() {
            assert!(
                !kind.trim().is_empty(),
                "{}'s table declares a blank node kind",
                language.as_str()
            );
        }
    }
    assert!(
        !complexity::supported().is_empty(),
        "a build that gates in a grammar and still supports no language has a bug in its table wiring"
    );
    assert!(
        complexity::rules(Language::Unknown).is_none(),
        "an undetected language must have no table, so it refuses with a reason rather than counting 0"
    );
}

/// §3.2 and §5.1, restated as code: `V(G) = 1 + points`, `Σ`, `max`, and a unit with nothing in it is
/// one path rather than zero paths.
#[test]
fn the_arithmetic_is_the_arithmetic() {
    if !gated(&[Language::Rust]) {
        return;
    }
    let source = br#"
fn quiet(x: i32) -> i32 {
    x
}

fn one(x: i32) -> i32 {
    if x > 0 { 1 } else { 0 }
}

fn three(x: i32) -> i32 {
    if x > 0 && x < 10 {
        for i in 0..x {
            let _ = i;
        }
    }
    x
}
"#;
    let file = complexity::measure("src/arithmetic.rs", source, &ExtractPolicy::default());
    assert_eq!(file.language, Language::Rust);
    assert_eq!(file.units.len(), 3, "three functions, three units");
    for unit in &file.units {
        let expected = match unit.figure {
            Some(figure) => figure,
            None => panic!("a countable body came back unknown: {}", unit.name),
        };
        assert!(
            expected > 0,
            "{} came back as 0, which is never a unit's figure",
            unit.name
        );
        assert_eq!(
            expected,
            complexity::BASE + unit.points.len(),
            "{}: the figure is not 1 + its decision points",
            unit.name
        );
    }
    let figures: Vec<usize> = file.units.iter().filter_map(|unit| unit.figure).collect();
    assert_eq!(
        file.sum,
        Some(figures.iter().sum()),
        "the file's sum is not the sum of its units"
    );
    assert_eq!(file.max, figures.iter().copied().max());
    assert_eq!(file.measured, figures.len());
    assert_eq!(file.unmeasured, 0);
    assert_eq!(file.state(), complexity::State::Measured);
    assert_eq!(
        file.units[0].figure,
        Some(1),
        "a function with no decision points is one path"
    );
}

/// §7.1 and D10: a declaration whose body lives elsewhere is an unknown with a code, on every grammar
/// that has a bodyless declaration form — and never a `0`, on any of them.
#[test]
fn an_unknown_never_arrives_as_zero() {
    // One bodyless form per grammar that has one. Go is deliberately absent: Go has no forward
    // declaration, so an `extern`-style bodyless Go function is not a thing the grammar emits — which
    // is itself the reason the corpus's Go unknown is written the way it is.
    let cases: [(&str, &str, Language); 3] = [
        (
            "src/sig.rs",
            "pub trait Shape {\n    fn area(&self) -> f64;\n}\n",
            Language::Rust,
        ),
        (
            "src/Decls.java",
            "interface Shape {\n    double area();\n}\n",
            Language::Java,
        ),
        (
            "src/shape.ts",
            "interface Shape {\n    area(): number;\n}\n",
            Language::TypeScript,
        ),
    ];
    for (path, text, language) in cases {
        if !gated(&[language]) {
            continue;
        }
        let file = complexity::measure(path, text.as_bytes(), &ExtractPolicy::default());
        for unit in &file.units {
            if let Some(figure) = unit.figure {
                assert_ne!(
                    figure, 0,
                    "{} in {path} printed a 0 where the ADR demands a `?`",
                    unit.name
                );
            } else {
                assert!(
                    unit.reason.is_some(),
                    "{} in {path} has no figure and no reason code, which prints a bare `?`",
                    unit.name
                );
            }
        }
        if let Some(sum) = file.sum {
            assert_ne!(
                sum, 0,
                "{path} rolled up to 0 without `absent_revision` or `no_bodies`"
            );
        } else {
            let spoken = file.reason.is_some()
                || (!file.units.is_empty() && file.units.iter().all(|unit| unit.reason.is_some()));
            assert!(
                spoken,
                "{path} has no file figure, no file reason and a unit with no reason"
            );
        }
    }
}

/// A file that parses and declares nothing callable is `no_bodies`: a `0` that is a fact, with its
/// reason named (§6.1).
#[test]
fn a_file_with_no_callables_is_a_zero_with_a_reason() {
    if !gated(&[Language::Rust]) {
        return;
    }
    let file = complexity::measure(
        "src/constants.rs",
        b"pub const LIMIT: usize = 4096;\n",
        &ExtractPolicy::default(),
    );
    assert!(file.units.is_empty());
    assert_eq!(file.sum, Some(0));
    assert_eq!(code(&file.reason), Some("no_bodies"));
    assert_eq!(file.state(), complexity::State::Measured);
}

/// An unreadable file is a `?` with a reason and no figure at all — never a `0` that reads as simple.
#[test]
fn an_unreadable_file_refuses_with_a_reason() {
    let file = complexity::measure(
        "src/binary.rs",
        &[0u8, 159, 141, 149, 0, 0],
        &ExtractPolicy::default(),
    );
    assert_eq!(file.sum, None);
    assert_eq!(file.max, None);
    assert!(file.units.is_empty());
    assert_eq!(code(&file.reason), Some("not_utf8"));
    assert_eq!(file.state(), complexity::State::Unavailable);
}

/// §7.2, the rule the old implementation broke: a spent bound sets the state and names the code. It is
/// checked by *spending* the bound, not by asserting that some budget exists.
#[test]
fn a_spent_bound_changes_the_state_and_the_code() {
    if !gated(&[Language::Rust]) {
        return;
    }
    let source = br#"
fn wide(x: i32) -> i32 {
    let mut total = 0;
    if x > 0 {
        for i in 0..x {
            if i % 2 == 0 && i > 4 || i < 0 {
                total += i;
            }
            while total > 100 {
                total -= 1;
            }
        }
    }
    total
}
"#;
    let open = complexity::measure("src/bounded.rs", source, &ExtractPolicy::default());
    let counted = open.units[0]
        .figure
        .expect("with a normal bound the body is countable");
    assert!(
        counted > 1,
        "the sample should have decision points to lose"
    );

    let policy = ExtractPolicy {
        max_complexity_nodes: 1,
        ..ExtractPolicy::default()
    };
    let starved = complexity::measure("src/bounded.rs", source, &policy);
    let unit = &starved.units[0];
    assert_eq!(
        unit.figure, None,
        "a body that ran out of budget printed a figure ({:?}) anyway — the §7.2 bug",
        unit.figure
    );
    assert_eq!(
        code(&unit.reason),
        Some("node_budget"),
        "the bound was spent and the unit does not say which bound"
    );
    assert_eq!(
        starved.state(),
        complexity::State::Unavailable,
        "a file whose every unit ran out still prints a number"
    );
    assert!(
        starved.printed().contains('?'),
        "the dump prints `{}` where it must print `?`",
        starved.printed()
    );
}

/// §5.4: coverage is part of the figure. A file with units that could not be counted prints a floor
/// with the count beside it, and never the bare number.
#[test]
fn partial_coverage_prints_the_count() {
    if !gated(&[Language::Rust]) {
        return;
    }
    let source = br#"
pub trait Shape {
    fn area(&self) -> f64;
}

impl Shape for f64 {
    fn area(&self) -> f64 {
        if *self < 0.0 { 0.0 } else { *self }
    }
}
"#;
    let file = complexity::measure("src/partial.rs", source, &ExtractPolicy::default());
    assert_eq!(file.unmeasured, 1);
    assert_eq!(file.measured, 1);
    assert_eq!(file.state(), complexity::State::Partial);
    let printed = file.printed();
    assert!(
        printed.contains('~'),
        "a floor must print as a floor: {printed}"
    );
    assert!(
        printed.contains("1 of 2") || printed.contains("unmeasured"),
        "the count of uncounted units is missing from `{printed}`"
    );
}

/// §8.6: a figure carries a pointer to its evidence. The dump is that pointer, so it has to name the
/// basis, the language and the line of every decision point it counted.
#[test]
fn the_dump_defends_every_figure() {
    if !gated(&[Language::Rust]) {
        return;
    }
    let source = b"fn f(x: i32) -> i32 {\n    if x > 0 { 1 } else { 0 }\n}\n";
    let file = complexity::measure("src/dump.rs", source, &ExtractPolicy::default());
    let dump = file.dump();
    assert!(
        dump.contains("mccabe-1"),
        "the dump does not name its basis:\n{dump}"
    );
    assert!(
        dump.contains("rust"),
        "the dump does not name its language:\n{dump}"
    );
    assert!(
        dump.contains("if_expression"),
        "the dump does not name the decision point it counted:\n{dump}"
    );
}

/// §10.1, the part of it that is about determinism rather than timing: the same bytes measure the same
/// way twice, with no clock and no path leakage into the numbers.
#[test]
fn measuring_twice_is_the_same_measurement() {
    if !gated(&[Language::Rust]) {
        return;
    }
    let source = b"fn f(x: i32) -> i32 {\n    if x > 0 { 1 } else { 0 }\n}\n";
    let policy = ExtractPolicy::default();
    let first = complexity::measure("src/twice.rs", source, &policy);
    let second = complexity::measure("src/twice.rs", source, &policy);
    assert_eq!(
        serde_json::to_string(&first).expect("a report is serialisable"),
        serde_json::to_string(&second).expect("a report is serialisable")
    );
}

/// A file that parses and holds nothing gets a reason anyway: in a build that gates the grammar out,
/// `grammar_not_compiled`, and in a build that has it, the fact that there was nothing callable. An
/// empty answer with no reason at all is the thing this forbids (§7.1).
#[test]
fn every_language_answers_an_empty_file() {
    for language in [
        Language::Rust,
        Language::Python,
        Language::TypeScript,
        Language::Go,
        Language::Ruby,
        Language::Java,
    ] {
        let extension = match language {
            Language::Rust => "rs",
            Language::Python => "py",
            Language::TypeScript => "ts",
            Language::Go => "go",
            Language::Ruby => "rb",
            Language::Java => "java",
            _ => continue,
        };
        let file = complexity::measure(
            &format!("src/probe.{extension}"),
            b"",
            &ExtractPolicy::default(),
        );
        let spoken = code(&file.reason).unwrap_or_else(|| {
            panic!(
                "{} answered an empty file with no reason and no figure",
                language.as_str()
            )
        });
        assert!(
            matches!(spoken, "grammar_not_compiled" | "no_bodies"),
            "{} answered an empty file with `{}`, which is neither a missing grammar nor an empty file",
            language.as_str(),
            spoken
        );
    }
}

/// The store's `?` and this crate's vocabulary are the same code (ADR-007 §7.1), and the store cannot say
/// so for itself: `fathom-store` holds the string it prints for "this column has not been filled" and
/// `column_not_filled` belongs to [`UnavailableReason`], which lives here because the dependency runs this
/// way round. So the two ends of that duplication are asserted where both are in scope — which is the only
/// way a string a store prints and an enum that names it can be kept from becoming two answers to one
/// question (`ADR-001 §7.10`).
#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
#[test]
fn the_store_prints_the_same_unknown_as_the_vocabulary_names() {
    assert_eq!(
        fathom_store::complexity::COLUMN_NOT_FILLED,
        UnavailableReason::ColumnNotFilled.as_str(),
        "an unfilled column must print the code whose sentence says `fathom index` will fix it"
    );
    assert_eq!(
        fathom_store::complexity::MIXED_BASIS,
        UnavailableReason::MixedBasis {
            bases: String::new()
        }
        .as_str(),
        "and a roll-up that refuses a mixed pair must name it with the §7.1 code, not a store-local word"
    );
    // Both are codes that mean *no figure*, and neither is one of the two codes that mean a `0` that is a
    // fact — a reader who printed either as `0` would be doing the thing §7 exists to stop.
    for code in [
        fathom_store::complexity::COLUMN_NOT_FILLED,
        fathom_store::complexity::MIXED_BASIS,
    ] {
        assert!(
            UnavailableReason::vocabulary()
                .iter()
                .any(|reason| reason.as_str() == code && !reason.is_measured_zero()),
            "`{code}` is not in the §7.1 vocabulary as an unknown"
        );
    }
}
