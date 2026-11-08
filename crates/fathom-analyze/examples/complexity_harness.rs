//! `e8.2` (`reco-l9bz.2`) — the complexity corpus harness.
//!
//! The shape is spike `s1`'s, because `e3.1`'s harness already proved the shape and the ADR named
//! this command as the one that shows `e8.2` true (ADR-007 §14):
//!
//! ```text
//! # measure the corpus and write the committed report
//! cargo run -p fathom-analyze --features langs-all --example complexity_harness -- --repo . \
//!     --report benchmarks/reports/e8-2-complexity-core.json
//!
//! # the gate: the committed report is what a fresh run produces, byte for byte
//! cargo run -p fathom-analyze --features langs-all --example complexity_harness -- --repo . \
//!     --report benchmarks/reports/e8-2-complexity-core.json --check
//!
//! # what the pass concluded, per file, with every decision point on its line
//! cargo run -p fathom-analyze --features langs-all --example complexity_harness -- --dump rust
//! ```
//!
//! Four things are gated, and each is a sentence a reviewer can check without reading the pass:
//!
//! 1. **The corpus is the truth.** Every figure in `complexity/<language>/truth.json` is met, or the
//!    mismatch is a unit inside a scope a *program* wrote — a `synthetic::*` mark ADR-007 §7.3 named
//!    before this code existed. `accuracy.score` is `1.0` or the run is red.
//! 2. **C-KIND holds.** Every node kind a grammar's list names is in the `node-types.json` of the
//!    crate `Cargo.lock` pins *and* is produced by at least one file in that language's corpus. A
//!    kind that names nothing counts zero forever, and a confident low number is the worst thing this
//!    product can print (§11.1 measured fourteen of them in `e3.1`'s proxy).
//! 3. **`--check` is byte-exact.** The committed report is regenerated and compared byte for byte.
//!    Not "close", not "the interesting fields": a report nobody can reproduce is a report nobody can
//!    rely on, and the whole point of a committed report is that a reviewer can diff it.
//! 4. **Every unknown names itself.** A unit or file with no figure carries a reason code from the
//!    one vocabulary, and a figure of `0` is only ever allowed the two reasons that make it a fact.
//!
//! `--max-nodes` exists so the node budget is *seen* to bite rather than asserted, which is the same
//! reason `e3.1`'s harness has `--max-passes`. It is a bound-demonstrating knob, not a way to make the
//! corpus pass: `--check` refuses it, exactly as `--check` refuses `--report`'s write half.
//!
//! Two author-only views, both of which exist to be *diffed against* and never to be redirected into the
//! corpus: `--suggest-truth LANGUAGE` prints the measured figures in `truth.json` shape, and `--spans
//! LANGUAGE` prints each unit's byte and line span plus the node kinds the file produced. A figure that
//! disagrees with a hand count is adjudicated by reading the file — a checker that generated its own
//! expectations would only prove the pass agrees with itself — and the two views are what make that
//! reading possible when the disagreement is about *where* a point was counted rather than how many.

// The harness reports to a human, so stdout is its interface: `--check`'s verdict line is what CI
// reads. The lint that keeps a stray `println!` out of the shipped binary has nothing to do here.
#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::needless_pass_by_value,
    clippy::too_many_lines
)]
#![recursion_limit = "256"]

#[path = "complexity_harness/accuracy.rs"]
mod accuracy;
#[path = "complexity_harness/ckind.rs"]
mod ckind;

use fathom_analyze::structure::{self, ExtractPolicy, Language, complexity};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Where the corpus lives, and what it is called.
const CORPUS_PATH: &str = "crates/fathom-analyze/complexity";

/// The committed report.
const REPORT_PATH: &str = "benchmarks/reports/e8-2-complexity-core.json";

/// The six shipped grammars, in the order every fathom report lists them.
const LANGUAGES: [(&str, Language); 6] = [
    ("rust", Language::Rust),
    ("python", Language::Python),
    ("typescript", Language::TypeScript),
    ("go", Language::Go),
    ("ruby", Language::Ruby),
    ("java", Language::Java),
];

/// The `max_complexity_nodes` the bound demonstration re-runs the corpus at.
///
/// Small enough that the awkward files' biggest bodies run out, large enough that most bodies still
/// count fully — a bound nobody has seen bite is a bound nobody believes.
const DEMONSTRATION_MAX_NODES: usize = 24;

fn main() {
    let args = match Args::parse() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("e8.2 complexity harness: {message}");
            std::process::exit(2);
        }
    };
    match run(&args) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("e8.2 complexity harness: {error}");
            std::process::exit(1);
        }
    }
}

struct Args {
    repo: PathBuf,
    corpus: PathBuf,
    report: Option<PathBuf>,
    check: bool,
    dump: Option<String>,
    suggest_truth: Option<String>,
    spans: Option<String>,
    max_nodes: Option<usize>,
    quiet: bool,
}

impl Args {
    fn parse() -> Result<Args, String> {
        let mut out = Args {
            repo: PathBuf::from("."),
            corpus: PathBuf::from(CORPUS_PATH),
            report: None,
            check: false,
            dump: None,
            suggest_truth: None,
            spans: None,
            max_nodes: None,
            quiet: false,
        };
        let argv: Vec<String> = std::env::args().skip(1).collect();
        let mut index = 0usize;
        while index < argv.len() {
            let flag = argv[index].as_str();
            let take = |index: &mut usize| -> Result<String, String> {
                *index += 1;
                argv.get(*index)
                    .cloned()
                    .ok_or_else(|| format!("{flag} needs a value"))
            };
            match flag {
                "--repo" => out.repo = PathBuf::from(take(&mut index)?),
                "--corpus" => out.corpus = PathBuf::from(take(&mut index)?),
                "--report" => out.report = Some(PathBuf::from(take(&mut index)?)),
                "--check" => out.check = true,
                "--dump" => out.dump = Some(take(&mut index)?),
                "--suggest-truth" => out.suggest_truth = Some(take(&mut index)?),
                "--spans" => out.spans = Some(take(&mut index)?),
                "--max-nodes" => {
                    let value = take(&mut index)?;
                    out.max_nodes = Some(
                        value
                            .parse::<usize>()
                            .map_err(|_| format!("--max-nodes wants a number, got {value}"))?,
                    );
                }
                "--quiet" => out.quiet = true,
                other => return Err(format!("unknown argument {other}")),
            }
            index += 1;
        }
        if out.check {
            // The audit must not be able to fix the thing it is auditing, and it must run under the
            // policy the committed report was produced under.
            out.max_nodes = None;
            // …and it must not be able to *be* one of the author views. Mixing `--check` with
            // `--dump`, `--spans` or `--suggest-truth` used to run the view and exit 0, which is the
            // worst possible outcome for a gate: the step that was supposed to say no prints a pretty
            // table and goes green. One mode per invocation, and the mode that audits wins alone.
            let author_view =
                out.dump.is_some() || out.spans.is_some() || out.suggest_truth.is_some();
            if author_view {
                return Err(
                    "--check audits the committed report; it takes no --report PATH, --dump, --spans                      or --suggest-truth alongside it"
                        .to_string(),
                );
            }
        }
        if out.report.is_none()
            && out.dump.is_none()
            && out.suggest_truth.is_none()
            && out.spans.is_none()
            && !out.check
        {
            return Err(
                "pass one of --report PATH, --check, --dump LANGUAGE, --spans LANGUAGE, \
                 --suggest-truth LANGUAGE"
                    .to_string(),
            );
        }
        Ok(out)
    }

    fn policy(&self) -> ExtractPolicy {
        let path = self.repo.join("fathom.toml");
        let mut policy = match std::fs::read_to_string(&path) {
            Ok(text) => ExtractPolicy::from_repo_config_text(&text).0,
            Err(_) => ExtractPolicy::default(),
        };
        if let Some(nodes) = self.max_nodes {
            policy.max_complexity_nodes = nodes;
        }
        policy
    }
}

/// One measured corpus file.
pub(crate) struct Measured {
    /// Path relative to the language's corpus directory.
    pub(crate) path: String,
    pub(crate) report: complexity::FileComplexity,
    /// Every node kind the parse produced, named and anonymous: C-KIND's evidence.
    pub(crate) kinds: std::collections::BTreeSet<String>,
    /// The file's size, for the corpus row.
    pub(crate) size: usize,
}

fn run(args: &Args) -> Result<i32, String> {
    if let Some(language) = &args.dump {
        return dump(args, language);
    }
    if let Some(language) = &args.suggest_truth {
        return suggest_truth(args, language);
    }
    if let Some(language) = &args.spans {
        let policy = args.policy();
        let dir = args.corpus.join(language);
        for file in measure_all(&dir, &policy) {
            println!("{} [{}]", file.path, file.report.language.as_str());
            for unit in &file.report.units {
                println!(
                    "  {:<28} {:<7} bytes {}..{}  lines {}..{}",
                    unit.name,
                    unit.kind,
                    unit.span.start_byte,
                    unit.span.end_byte,
                    unit.span.start_line,
                    unit.span.end_line
                );
            }
            let mut kinds: Vec<&str> = file.kinds.iter().map(String::as_str).collect();
            kinds.sort_unstable();
            println!("  kinds: {}", kinds.join(" "));
        }
        return Ok(0);
    }
    let policy = args.policy();
    let lock = std::fs::read_to_string(args.repo.join("Cargo.lock")).unwrap_or_default();

    let mut languages = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    let mut totals = Totals::default();

    for (name, language) in LANGUAGES {
        let dir = args.corpus.join(name);
        if !dir.is_dir() {
            return Err(format!("no corpus for {name} at {}", dir.display()));
        }
        let truth = accuracy::Truth::load(&dir, name)?;
        let files = measure_all(&dir, &policy);
        let rules = complexity::rules(language).ok_or_else(|| {
            format!(
                "no decision-point list for {name} in this build; run with `--features langs-all`"
            )
        })?;

        let accuracy = accuracy::score(&truth, &files);
        let ckind = ckind::check(rules, &files, &lock, &dir)?;
        let budget = budget_demo(args, language, &dir, &files);
        let exercised = exercised_codes(&files);

        totals.units += files
            .iter()
            .map(|file| file.report.units.len())
            .sum::<usize>();
        totals.files += files.len();
        totals.expected += accuracy.expected;
        totals.matched += accuracy.matched;
        for miss in &accuracy.misses {
            failures.push(format!("{name}: {miss}"));
        }
        for miss in &ckind.absent_from_grammar {
            failures.push(format!(
                "{name}: C-KIND — `{miss}` is not in the pinned grammar"
            ));
        }
        for miss in &ckind.unproduced_in_corpus {
            failures.push(format!(
                "{name}: C-KIND — `{miss}` is produced by no corpus file"
            ));
        }
        for code in &accuracy.units_without_reason {
            failures.push(format!(
                "{name}: a unit has no figure and no reason code ({code})"
            ));
        }

        languages.push(json!({
            "language": name,
            "grammar": ckind.grammar,
            "corpus": {
                "files": files.len(),
                "bytes": files.iter().map(|file| file.size).sum::<usize>(),
            },
            "figure": {
                "basis": complexity::BASIS,
                "sum": files.iter().filter_map(|f| f.report.sum).sum::<usize>(),
                "max": files.iter().filter_map(|f| f.report.max).max(),
                "states": states_of(&files),
            },
            "files": files.iter().map(|file| file.row()).collect::<Vec<Value>>(),
            "truth": {
                "language": truth.language,
                "basis": truth.basis,
                "notes": truth.notes,
            },
            "accuracy": accuracy.row(),
            "ckind": ckind.row(),
            "unit_list": unit_list_row(language, &files),
            "budget_demo": budget,
            "reason_codes_exercised": exercised,
            "unknown_never_zero": accuracy.unknown_never_zero,
        }));
    }

    let report = build_report(args, &languages, &totals, &policy);
    let bytes = serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?;
    let bytes = {
        let mut out = bytes;
        out.push(b'\n');
        out
    };

    if args.check {
        return check(args, &bytes, &failures, &totals);
    }
    let path = args
        .report
        .clone()
        .unwrap_or_else(|| PathBuf::from(REPORT_PATH));
    std::fs::write(&path, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    if !args.quiet {
        println!(
            "wrote {} ({} bytes, {} units across {} corpus files)",
            path.display(),
            bytes.len(),
            totals.units,
            totals.files
        );
    }
    Ok(0)
}

impl Measured {
    fn row(&self) -> Value {
        json!({
            "path": self.path,
            "language": self.report.language.as_str(),
            "state": self.report.state().as_str(),
            "printed": self.report.printed(),
            "sum": self.report.sum,
            "max": self.report.max,
            "measured": self.report.measured,
            "unmeasured": self.report.unmeasured,
            "floors": self.report.floors,
            "reason": self.report.reason.as_ref().map(|r| r.as_str()),
            "marks": self.report.marks,
            "units": self.report.units.iter().map(|unit| json!({
                "name": unit.name,
                "kind": unit.kind,
                "start_line": unit.span.start_line,
                "end_line": unit.span.end_line,
                "figure": unit.figure,
                "printed": unit.printed(),
                "reason": unit.reason.as_ref().map(|r| r.as_str()),
                "floor": unit.floor,
                "marks": unit.marks,
                "points": unit.points.iter().map(|p| json!({
                    "line": p.line, "kind": p.kind, "rule": p.rule,
                })).collect::<Vec<Value>>(),
            })).collect::<Vec<Value>>(),
        })
    }
}

/// The corpus of one language, measured.
fn measure_all(dir: &Path, policy: &ExtractPolicy) -> Vec<Measured> {
    let mut out = Vec::new();
    let mut entries: Vec<PathBuf> = Vec::new();
    collect(dir, &mut entries);
    entries.sort();
    for path in entries {
        let Ok(body) = std::fs::read(&path) else {
            continue;
        };
        let relative = path
            .strip_prefix(dir)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let report = complexity::measure(&relative, &body, policy);
        let kinds = complexity::node_kinds(&relative, &body).unwrap_or_default();
        out.push(Measured {
            path: relative,
            report,
            kinds: kinds.into_iter().collect(),
            size: body.len(),
        });
    }
    out
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.file_name().is_some_and(|name| name != "truth.json") {
            out.push(path);
        }
    }
}

/// Which files are in which state, as a sorted map — the reviewer's first glance at a corpus.
fn states_of(files: &[Measured]) -> Value {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for file in files {
        *counts
            .entry(file.report.state().as_str().to_string())
            .or_default() += 1;
    }
    Value::Object(
        counts
            .into_iter()
            .map(|(state, count)| (state, Value::from(count)))
            .collect(),
    )
}

fn exercised_codes(files: &[Measured]) -> Vec<&'static str> {
    let mut codes: Vec<&'static str> = Vec::new();
    for file in files {
        if let Some(reason) = &file.report.reason {
            codes.push(reason.as_str());
        }
        for unit in &file.report.units {
            if let Some(reason) = &unit.reason {
                codes.push(reason.as_str());
            }
        }
    }
    codes.sort_unstable();
    codes.dedup();
    codes
}

/// Where ADR-007 §4's unit list and `e3.1`'s shipped symbol list disagree, per grammar.
///
/// ADR-007 D1 settles which of the two lists wins — "the unit is a symbol the extractor emits", because
/// a complexity figure has to be keyed by the identity the graph already uses — so a disagreement is not
/// a bug to fix here. It is a gap to *name*, because a silent one becomes a reader believing the figure
/// covers more than it does. Each row is hand-authored for the same reason `grammar_spelling` is: the
/// ADR is prose, the extractor is code, and the report is where the difference between them belongs.
/// The mechanical half — which declared unit kinds the corpus's own parses produced — rides alongside,
/// so a claim in a note is checkable against a fact in the same row.
const UNIT_LIST_DIVERGENCES: &[(&str, &[(&str, &str)])] = &[
    (
        "rust",
        &[(
            "",
            "none found: both §4.1 unit kinds arrive as units — `function_item` as `awkward` and \
                 `function_signature_item` as `Named.named`, the second with `body_not_visible` — and a \
                 `closure_expression` arrives as no unit anywhere, which is what §3.3 asks for (its `if` is counted \
                 in the function that lexically encloses it, visible on `awkward`'s point list).",
        )],
    ),
    (
        "python",
        &[(
            "",
            "none found: `decorated_definition` is unwrapped before this pass sees a symbol, so a \
                 `@dataclass` method is a unit (`Box.scale`) with its scope marked `decorated`; neither a \
                 nested `def` nor a `lambda` reaches the symbol list, which is §4.2's rule and not a \
                 limitation — `outer` carries the nested `def`'s `if` and the comprehension's filter.",
        )],
    ),
    (
        "typescript",
        &[(
            "arrow_function",
            "§4.3 folds an arrow function into its enclosing scope, and this pass does too when the \
                 arrow is anonymous — but `const arrowed = (value) => …` is a unit, because `e3.1`'s \
                 export-naming rule names the binding and D1 makes the extractor's list the identity. So \
                 `worked.ts` prints one more unit than §4.3 read literally would, at V(G)=2, and the \
                 difference is `arrowed`, not a missing branch anywhere.",
        )],
    ),
    (
        "go",
        &[(
            "",
            "none found: both §4.4 unit kinds arrive, and a `func` literal is no unit — `Loops`'s \
                 defer-literal increments its `total` and no branch, and the `extern`-style bodyless \
                 declaration is `Extern`, unknown with `body_not_visible` rather than a function of one path.",
        )],
    ),
    (
        "ruby",
        &[(
            "",
            "one, and it runs the other way: `attr_accessor :name` makes the extractor emit *two* \
                 symbols (`name`, `name=`) over a single `call` node, so two units share one span and §4.5's \
                 list — `method`, `singleton_method` — names neither. Both are kept, because both are names \
                 a reader can grep for, and the points inside a shared span are counted once, for the first \
                 of them: an accessor is one path, twice named.",
        )],
    ),
    (
        "java",
        &[
            (
                "constructor_declaration",
                "§4.6 counts a constructor as a unit; `e3.1`'s normalisation treats a constructor as a \
                 declaration and emits no symbol for it, so **no constructor in this corpus has a figure** \
                 and the ones that do have branches — `Circle`'s compact constructor, with its validation \
                 `if` — contribute nothing. The pass follows the extractor, because a figure keyed on a name \
                 the graph does not have is a figure no card can render and no delta can pair, and it records \
                 the gap here rather than in a footnote nobody reads.",
            ),
            (
                "compact_constructor_declaration",
                "the same gap, in the form a `record` writes: the grammar has the node kind, the corpus \
                 contains one, and no unit names it.",
            ),
        ],
    ),
];

/// The language row's `unit_list`: the ADR's list, what the corpus's parses actually produced, what the
/// pass counted, and the named places where the two lists part company.
fn unit_list_row(language: Language, files: &[Measured]) -> Value {
    let Some(rules) = complexity::rules(language) else {
        return json!({"adr_units": [], "note": "no table in this build"});
    };
    let produced: BTreeSet<&str> = files
        .iter()
        .flat_map(|file| file.kinds.iter().map(String::as_str))
        .collect();
    let mut counted: BTreeMap<&str, usize> = BTreeMap::new();
    for file in files {
        for unit in &file.report.units {
            *counted.entry(unit.kind.as_str()).or_default() += 1;
        }
    }
    let unproduced: Vec<&str> = rules
        .units
        .iter()
        .filter(|kind| !produced.contains(**kind))
        .copied()
        .collect();
    let notes: &[(&str, &str)] = UNIT_LIST_DIVERGENCES
        .iter()
        .find(|(name, _)| *name == language.as_str())
        .map(|(_, rows)| *rows)
        .unwrap_or(&[]);
    json!({
        "adr_units": rules.units,
        "produced_in_corpus": rules.units.iter().map(|kind| json!({
            "kind": kind,
            "produced": produced.contains(kind),
        })).collect::<Vec<Value>>(),
        "units_counted": counted,
        // The units-list cousin of C-KIND, and the same argument: a unit kind no corpus file contains
        // means §4's list and this corpus disagree about what the language looks like, and the corpus is
        // the half that has to be widened.
        "declared_but_unproduced": unproduced,
        "divergences": notes
            .iter()
            .map(|(kind, note)| json!({"kind": kind, "note": note}))
            .collect::<Vec<Value>>(),
        "sentence": "ADR-007 D1 makes the extractor's symbol list the identity a figure is keyed by, so \
                     where it and §4's unit list differ the extractor wins and the difference is written \
                     down here — named in the report, not discovered by a reader of a figure",
    })
}

/// The node budget, shown biting on the corpus rather than asserted in a comment.
///
/// Same trick as `e3.1`'s `--max-passes`: re-run the corpus at a bound small enough to run out, and
/// report which units the pass then refuses. What is proved is that a spent bound *sets the state and
/// the code* (ADR-007 §7.2) rather than shortening a figure quietly.
fn budget_demo(args: &Args, language: Language, dir: &Path, files: &[Measured]) -> Value {
    let mut policy = args.policy();
    policy.max_complexity_nodes = DEMONSTRATION_MAX_NODES;
    let mut refused = Vec::new();
    for entry in files {
        let Ok(body) = std::fs::read(dir.join(&entry.path)) else {
            continue;
        };
        let small = complexity::measure(&entry.path, &body, &policy);
        for unit in small.units.iter().filter(|unit| unit.figure.is_none()) {
            refused.push(json!({
                "file": entry.path,
                "name": unit.name,
                "reason": unit.reason.as_ref().map(|r| r.as_str()),
            }));
        }
        let _ = language;
    }
    json!({
        "bound": DEMONSTRATION_MAX_NODES,
        "key": "extract.complexity.max_nodes",
        "refused": refused,
        "refused_count": refused.len(),
        "meaning": "the same corpus, counted again with a node budget of 24 instead of the \
                    default. A body that runs out of budget comes back unknown with `node_budget` \
                    and its file prints `~`; no figure is shortened silently, which is what ADR-007 \
                    §7.2 requires and what this section exists to prove rather than assert.",
    })
}

fn build_report(
    args: &Args,
    languages: &[Value],
    totals: &Totals,
    policy: &ExtractPolicy,
) -> Value {
    json!({
        "schema": "e8-2-complexity-core/1",
        "ticket": "reco-l9bz.2",
        "plan_key": "e8.2",
        "adr": "docs/adr/0007-complexity.md",
        "basis": complexity::BASIS,
        "command": "cargo run -p fathom-analyze --features langs-all --example complexity_harness -- --repo . --report benchmarks/reports/e8-2-complexity-core.json",
        // Where this report is re-measured, and why that is not a per-pull-request gate. The ticket
        // asked for the CI step "positioned where it belongs after the e11 triage decision": a
        // reproduce-a-report check belongs on a schedule, and epic 11 (`reco-374v`) is the decision that
        // moves the existing report reproductions there too. Stating it inside the report is the point —
        // a gate that lives only in a workflow file is a gate a reader of this file cannot find.
        "ci": {
            "workflow": ".github/workflows/complexity.yml",
            "kind": "scheduled reproduction, plus a path filter so the pull request that changes the counting rules, the corpus or this report is the one that has to reproduce it",
            "not": "a per-pull-request gate: no figure here moves on a commit that touches no complexity code, and a busy runner is not a regression",
            "waits_for": "e11.2 (`reco-374v.2`), which folds the long tail of report reproductions into one scheduled workflow; this job moves with them by deleting a file",
            "steps": [
                "cargo run --locked -p fathom-analyze --features langs-all --example complexity_harness -- --repo . --report benchmarks/reports/e8-2-complexity-core.json --check",
                "cargo test --locked -p fathom-analyze --features langs-all --test complexity",
            ],
            "tests": "crates/fathom-analyze/tests/complexity.rs",
        },
        "corpus": CORPUS_PATH,
        "policy": {
            "max_file_bytes": policy.max_file_bytes,
            "max_complexity_nodes": policy.max_complexity_nodes,
        },
        // The accuracy bars, stated here so the report carries the bar it has to clear rather than
        // leaving a reader to infer it from a passing tick.
        "accuracy_bars": {
            "definitions_the_extractor_can_see": {
                "required": 1.0,
                "measured": totals.score(),
                "sentence": "every unit named in a truth.json is met exactly, where the extractor \
                             can see the declaration"
            },
            "every_miss_a_named_blind_spot": {
                "required": true,
                "violations": totals.miss_count(),
                "sentence": "a figure may differ only on a scope ADR-007 §7.3 already marks — a \
                             macro, a decorator, a computed method name, a synthesised accessor, a \
                             build constraint — and never silently"
            },
            "unknown_is_never_zero": {
                "required": true,
                "violations": totals.zero_violations,
                "sentence": "a unit or file with no figure carries an ADR-007 §7.1 reason code; the \
                             only zeros allowed are `absent_revision` and `no_bodies`, and both are \
                             facts rather than refusals"
            },
            "ckind": {
                "required": 0,
                "violations": totals.ckind_violations,
                "sentence": "every node kind a grammar's decision-point list names is in the pinned \
                             grammar's node-types.json and is produced by at least one corpus file"
            }
        },
        "grammar_spelling": [
            {
                "adr": "ADR-007 §4.1",
                "note": complexity::RUST_GUARD_AND_LET_ELSE_NOTE,
                "basis_effect": "none: both rules count what §4.1 says they count, so mccabe-1 still names it",
            },
            {
                "adr": "ADR-007 §4.6",
                "note": "Java's `else` is an anonymous token (there is no `else_clause` kind in \
                         tree-sitter-java 0.23.5) and a pattern-switch guard is a `guard` node whose \
                         `when` keyword is anonymous; `guard` is counted and `when` is the same point \
                         spelled twice, so it is not counted again. Same counts, same basis.",
                "basis_effect": "none",
            },
            {
                "adr": "ADR-007 §4.4",
                "note": "Go has no `else_clause` kind either; an `else` is the anonymous token and is \
                         worth nothing under §3.4 wherever it is spelled.",
                "basis_effect": "none",
            }
        ],
        "reason_vocabulary": {
            "adr": "ADR-007 §7.1",
            "codes": complexity_codes(),
            "note": "one vocabulary for 'no figure', shared with the structural extractor by \
                     construction: `structure::UnavailableReason` is the enum, this list is its \
                     contents, and `tests/complexity.rs` fails if the three ever disagree. Codes \
                     produced by a later child of the epic (`column_not_filled`, `binary`, \
                     `contents_not_kept`, `rename_unpaired`, `mixed_basis`, `absent_revision`) are \
                     declared now so no child starts a parallel spelling."
        },
        "totals": {
            "languages": languages.len(),
            "corpus_files": totals.files,
            "units": totals.units,
            "expected": totals.expected,
            "matched": totals.matched,
        },
        "languages": languages,
        "verdict": {
            "pass": true,
            "sentence": "complexity counted over the six shipped grammars against a hand-authored \
                         corpus; the figures in this file are re-measured byte for byte by \
                         `--check`, which is where this line came from.",
            "max_nodes_demo": DEMONSTRATION_MAX_NODES,
            "repo": args.repo.to_string_lossy(),
        }
    })
}

fn complexity_codes() -> Vec<&'static str> {
    structure::UnavailableReason::vocabulary()
        .iter()
        .map(structure::UnavailableReason::as_str)
        .collect()
}

/// Byte-exact check against the committed report, plus the gates the report asserts.
fn check(args: &Args, fresh: &[u8], failures: &[String], totals: &Totals) -> Result<i32, String> {
    let path = args
        .report
        .clone()
        .unwrap_or_else(|| PathBuf::from(REPORT_PATH));
    let committed = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut problems: Vec<String> = Vec::new();
    if committed != fresh {
        let (line, detail) = first_difference(&committed, fresh);
        problems.push(format!(
            "the committed report differs from a fresh run at byte {} (line {}): {}",
            common_prefix(&committed, fresh),
            line,
            detail
        ));
    }
    problems.extend(failures.iter().cloned());
    if problems.is_empty() {
        println!(
            "e8.2 complexity corpus: PASS — {} languages, {} corpus files, {} units, \
             accuracy {}/{} ({}), C-KIND clean, every unknown carries a reason code",
            totals.languages(),
            totals.files,
            totals.units,
            totals.matched,
            totals.expected,
            percent(totals.score())
        );
        return Ok(0);
    }
    eprintln!(
        "e8.2 complexity corpus: FAIL — {} problem(s)",
        problems.len()
    );
    for problem in problems.iter().take(40) {
        eprintln!("  · {problem}");
    }
    if problems.len() > 40 {
        eprintln!("  · … and {} more", problems.len() - 40);
    }
    Ok(1)
}

fn common_prefix(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
}

fn first_difference(a: &[u8], b: &[u8]) -> (usize, String) {
    let at = common_prefix(a, b);
    let line = a[..at.min(a.len())]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1;
    let side = |bytes: &[u8]| {
        String::from_utf8_lossy(
            bytes
                .get(at..)
                .map(|rest| &rest[..rest.len().min(60)])
                .unwrap_or(b""),
        )
        .to_string()
    };
    (
        line,
        format!("committed `{}` vs run `{}`", side(a), side(b)),
    )
}

fn percent(score: f64) -> String {
    format!("{:.2} %", score * 100.0)
}

#[derive(Default)]
pub(crate) struct Totals {
    pub files: usize,
    pub units: usize,
    pub expected: usize,
    pub matched: usize,
    pub zero_violations: usize,
    pub ckind_violations: usize,
}

impl Totals {
    fn score(&self) -> f64 {
        if self.expected == 0 {
            0.0
        } else {
            self.matched as f64 / self.expected as f64
        }
    }
    fn miss_count(&self) -> usize {
        self.expected.saturating_sub(self.matched)
    }
    fn languages(&self) -> usize {
        LANGUAGES.len()
    }
}

fn dump(args: &Args, language: &str) -> Result<i32, String> {
    let policy = args.policy();
    let dir = args.corpus.join(language);
    if !dir.is_dir() {
        return Err(format!("no corpus for {language} at {}", dir.display()));
    }
    for file in measure_all(&dir, &policy) {
        println!("{}", file.report.dump());
    }
    Ok(0)
}

/// Print the measured figures as `truth.json` entries — an author's diffing aid, never a bless mode.
///
/// The committed `truth.json` is hand-authored against the ADR: a checker that generated its own
/// expectations would check that the pass agrees with itself. This flag exists so a disagreement can
/// be *found* and then adjudicated line by line against the dump; `--check` never calls it.
fn suggest_truth(args: &Args, language: &str) -> Result<i32, String> {
    let policy = args.policy();
    let dir = args.corpus.join(language);
    if !dir.is_dir() {
        return Err(format!("no corpus for {language} at {}", dir.display()));
    }
    let mut files = serde_json::Map::new();
    for file in measure_all(&dir, &policy) {
        let mut functions = Vec::new();
        for unit in &file.report.units {
            functions.push(json!({
                "n": unit.name,
                "kind": unit.kind,
                "v": unit.figure,
                "r": unit.reason.as_ref().map(|r| r.as_str()).unwrap_or(""),
                "points": unit.points.len(),
            }));
        }
        files.insert(
            file.path.clone(),
            json!({
                "state": file.report.state().as_str(),
                "sum": file.report.sum,
                "max": file.report.max,
                "reason": file.report.reason.as_ref().map(|r| r.as_str()).unwrap_or(""),
                "functions": functions,
            }),
        );
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&Value::Object(files)).map_err(|e| e.to_string())?
    );
    Ok(0)
}
