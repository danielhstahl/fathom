//! `e3.1` (`reco-zoz`) — the structural extractor's corpus harness.
//!
//! **This is the durable half of the ticket.** Spike `s1` closed with the instruction to "copy the
//! harness shape, not the extractors" (`docs/concepts/structure-extraction.md`, finding 8): corpus +
//! hand-authored `truth.json` + `thresholds.json` + a committed report + a `--check` mode that
//! refuses to bless a number it cannot reproduce. The argument for that shape is one sentence:
//! *a number nobody can re-run is a number nobody should plan around*, and ADR-004 built its whole
//! budget on numbers that came from a spike exactly like this one.
//!
//! ```text
//! # measure, and write the committed report
//! cargo run --release -p fathom-analyze --features lang-rust,lang-python,lang-typescript \
//!     --example structure_harness -- --repo . \
//!     --report benchmarks/reports/e3-1-structure-core.json
//!
//! # the gate: the committed report still matches a fresh run (relations, facts and verdicts — not timings)
//! cargo run -p fathom-analyze --features lang-rust,lang-python,lang-typescript \
//!     --example structure_harness -- --repo . \
//!     --report benchmarks/reports/e3-1-structure-core.json --check
//!
//! # the throughput floor: 1 000 files/s/core, measured on a replica of the corpus
//! cargo run --release -p fathom-analyze --features lang-rust,lang-python,lang-typescript \
//!     --example structure_harness -- --repo . --bench
//! ```
//!
//! Two debugging affordances, inherited from the spike because every "why did that not resolve"
//! question ended in one of them: `--dump <language>` (what the extractor concluded, per file) and
//! `--probe <language>` (symbol kinds and parse errors, file by file). Raw grammar archaeology —
//! s-expressions and node-kind histograms — stays spike `s1`'s job, and `s1_spike --sexp` still does
//! it against all six grammars.
//!
//! `--check` compares relation counts, the mechanism checks and the verdicts, and never compares
//! throughput: a busy runner is not a regression, and a debug build measures the allocator rather
//! than the grammar.

// The harness reports to a human, so stdout is its interface: `--check`'s verdict line is what CI
// reads. The lint that keeps a stray `println!` out of the shipped binary has nothing to do here.
#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::unwrap_used,
    clippy::expect_used
)]
// The report is one deeply nested `json!` per language and the macro's own expansion beats the
// default limit of 128 — a fact about `serde_json`'s macro, not about this report's structure.
#![recursion_limit = "256"]

#[path = "structure_harness/go_corpus.rs"]
mod go_corpus;
#[path = "structure_harness/java_corpus.rs"]
mod java_corpus;
#[path = "structure_harness/scoring.rs"]
mod scoring;
#[path = "structure_harness/truth.rs"]
mod truth;

use fathom_analyze::structure::{self, ExtractPolicy, Language, SourceFile};
use scoring::{LangScore, Score};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Every language the extractor ships: the three `e3.1` (`reco-zoz`) started with, and the three
/// `e3.1b` (`reco-dlp.1`) added. One table, one loop, one report: the tail's acceptance criterion is
/// that the three new languages *join* the harness rather than start a second one, because a second
/// harness is a second set of rules about what "measured" means.
const LANGUAGES: [(&str, &[&str]); 6] = [
    ("rust", &["rs"]),
    ("python", &["py"]),
    ("typescript", &["ts"]),
    ("go", &["go"]),
    ("ruby", &["rb"]),
    ("java", &["java"]),
];

/// What the report is called and where it lives.
const REPORT_PATH: &str = "benchmarks/reports/e3-1-structure-core.json";

/// How many Java files the same-package scan is re-measured on. `e0.12` wrote "at least 1 000" into
/// `e3.1b`'s acceptance criteria; the corpus generator rounds up from this.
const SAME_PACKAGE_CORPUS_FILES: usize = 1_200;

/// How many files the *repository-shaped* throughput corpus has, for the two languages whose replica
/// shape is a lie about their resolution (see `go_corpus`). Same order of magnitude as the real thing,
/// because a shape measured at a tenth of a repository is a shape measured at a tenth.
const REPOSITORY_SHAPE_FILES: usize = SAME_PACKAGE_CORPUS_FILES;

fn main() {
    let args = match Args::parse() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("e3.1 structure harness: {message}");
            std::process::exit(2);
        }
    };
    match run(&args) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("e3.1 structure harness: {error}");
            std::process::exit(1);
        }
    }
}

struct Args {
    repo: PathBuf,
    corpus: PathBuf,
    thresholds: PathBuf,
    report: Option<PathBuf>,
    write: bool,
    check: bool,
    bench: bool,
    snapshot: Option<PathBuf>,
    bless: bool,
    dump: Option<String>,
    probe: Option<String>,
    max_passes: Option<usize>,
    quiet: bool,
}

impl Args {
    fn parse() -> Result<Args, String> {
        let mut out = Args {
            repo: PathBuf::from("."),
            corpus: PathBuf::from("crates/fathom-analyze/structure/corpus"),
            thresholds: PathBuf::from("crates/fathom-analyze/structure/thresholds.json"),
            report: None,
            write: false,
            check: false,
            bench: false,
            snapshot: None,
            bless: false,
            dump: None,
            probe: None,
            max_passes: None,
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
                "--thresholds" => out.thresholds = PathBuf::from(take(&mut index)?),
                "--report" => {
                    out.report = Some(PathBuf::from(take(&mut index)?));
                    out.write = true;
                }
                "--check" => out.check = true,
                "--bench" => out.bench = true,
                "--snapshot" => out.snapshot = Some(PathBuf::from(take(&mut index)?)),
                "--bless" => out.bless = true,
                "--dump" => out.dump = Some(take(&mut index)?),
                "--probe" => out.probe = Some(take(&mut index)?),
                "--max-passes" => {
                    let value = take(&mut index)?;
                    out.max_passes = Some(
                        value
                            .parse::<usize>()
                            .map_err(|_| format!("--max-passes wants a number, got {value}"))?,
                    );
                }
                "--quiet" => out.quiet = true,
                other => return Err(format!("unknown argument {other}")),
            }
            index += 1;
        }
        if out.check {
            // The audit must not be able to fix the thing it is auditing.
            out.write = false;
            out.bless = false;
        }
        if out.report.is_none()
            && !out.check
            && !out.bench
            && out.snapshot.is_none()
            && out.dump.is_none()
            && out.probe.is_none()
        {
            return Err(
                "pass --report PATH, --check, --bench, --snapshot PATH, --dump LANG or --probe LANG"
                    .to_string(),
            );
        }
        Ok(out)
    }

    /// The policy the run obeys: the built-in defaults, overridden by the repository's own
    /// `[extract]` table if it has one, overridden again by `--max-passes` (which exists so the
    /// bound can be *demonstrated* to bite, not merely asserted).
    fn policy(&self) -> Result<(ExtractPolicy, Vec<String>), String> {
        let mut notices = Vec::new();
        let path = self.repo.join("fathom.toml");
        let mut policy = match std::fs::read_to_string(&path) {
            Ok(text) => {
                let (policy, mut notes) = ExtractPolicy::from_repo_config_text(&text);
                notices.append(&mut notes);
                policy
            }
            Err(_) => ExtractPolicy::default(),
        };
        if let Some(passes) = self.max_passes {
            policy.max_resolution_passes = passes;
            policy.max_resolution_hops = passes;
            notices.push(format!(
                "extract.resolution bound overridden to {passes} by --max-passes"
            ));
        }
        Ok((policy, notices))
    }
}

fn run(args: &Args) -> Result<i32, String> {
    if let Some(language) = &args.probe {
        return probe(args, language);
    }
    if let Some(language) = &args.dump {
        return dump(args, language);
    }
    let (policy, policy_notices) = args.policy()?;
    let thresholds: Value = read_json(&args.thresholds)?;

    // One snapshot of the whole corpus, so the byte-identity half of the ticket is measured against
    // the same bytes the accuracy half scored.
    let mut all_owned: Vec<(String, Vec<u8>)> = Vec::new();
    let mut languages = Vec::new();
    let mut throughputs = Vec::new();
    let mut grammars = Vec::new();
    let lock = std::fs::read_to_string(args.repo.join("Cargo.lock")).unwrap_or_default();

    for (name, extensions) in LANGUAGES {
        let tongue = Language::detect(&format!("probe.{}", extensions[0]));
        let dir = args.corpus.join(name);
        if !dir.is_dir() {
            return Err(format!("no corpus for {name} at {}", dir.display()));
        }
        let annotation = truth::Truth::load(&dir)?;
        let files = corpus_files(&dir, extensions);
        let mut owned: Vec<(String, Vec<u8>)> = Vec::new();
        for path in &files {
            let body = std::fs::read(dir.join(path)).map_err(|e| format!("{path}: {e}"))?;
            owned.push((path.clone(), body));
        }
        let sources: Vec<(&str, &[u8])> = owned
            .iter()
            .map(|(path, body)| (path.as_str(), body.as_slice()))
            .collect();
        all_owned.extend(owned.iter().cloned());
        grammars.push(grammar_row(name, &lock));

        // -- Extract, score, and check the four mechanisms s1 handed over as non-negotiables.
        let started = Instant::now();
        let borrowed: Vec<SourceFile> = sources
            .iter()
            .map(|(path, body)| SourceFile::new(path, body))
            .collect();
        let extracted = structure::extract(&borrowed, &policy);
        let extract_elapsed = started.elapsed();
        let irreducible = irreducible_tags(&thresholds, name);
        let mut score = scoring::score(&annotation, &extracted, &irreducible);
        score.corpus_files = files.len();
        score.corpus_bytes = sources.iter().map(|(_, body)| body.len()).sum();

        // The bound, demonstrated rather than asserted: the same corpus, extracted again with one
        // pass allowed. A resolution limit nobody has seen bite is a limit nobody believes, and this
        // is the cheapest possible proof that the iteration is bounded per file and that the bound
        // is what stops it. On a corpus this small it bites on the barrel chains, not the fixture's
        // own depth — which is exactly what the file-level bound is for.
        let mut bound_policy = policy.clone();
        bound_policy.max_resolution_passes = 1;
        bound_policy.max_resolution_hops = 1;
        let forced = structure::extract(&borrowed, &bound_policy);
        let bound_demo = json!({
            "bound": 1,
            "non_converged": forced
                .files
                .iter()
                .filter(|record| record.structure.as_ref().is_some_and(|s| !s.resolution.converged))
                .map(|record| record.path.clone())
                .collect::<Vec<String>>(),
            "truncated_chains": forced
                .files
                .iter()
                .filter_map(|record| record.structure.as_ref())
                .map(|structure| structure.resolution.truncated_chains)
                .sum::<usize>(),
            "mean": "the same corpus, re-extracted with `extract.resolution.max_passes = 1`. One pass is not enough to *observe* stability for any file that resolved something — a fixpoint is proved by a pass that changes nothing — so the honest report names those files instead of calling them settled. A file with nothing to resolve converged on the spot and is not listed.",
        });
        let mechanisms = mechanisms(&annotation, &extracted, &policy, bound_demo);
        let fallback = fallback_battery(&policy);
        let granularity = granularity_block(&extracted);
        let surface = surface_block(name, annotation.root.as_deref().unwrap_or(""), &extracted);
        let throughput = throughput(name, &policy, &dir, &files, &sources);
        let throughput_floor = thresholds
            .get("throughput")
            .and_then(|row| row.get("files_per_second_per_core_floor"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let mib_floor = thresholds
            .get("throughput")
            .and_then(|row| row.get("mib_per_second_floor"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let measured_files_per_second = throughput
            .get("replica")
            .and_then(|row| row.get("files_per_second_per_core"))
            .and_then(Value::as_f64)
            .unwrap_or_default();
        let measured_mib_per_second = throughput
            .get("replica")
            .and_then(|row| row.get("mib_per_second"))
            .and_then(Value::as_f64)
            .unwrap_or_default();
        // The scan budget, shown biting on the fixture (cheap) and priced by arithmetic at the cost
        // the replica just measured — the expensive half is never measured twice.
        let scan_budget = scan_budget_block(
            tongue,
            &policy,
            &borrowed,
            measured_mib_per_second,
            &thresholds,
        );
        let same_package = (name == "java").then(|| same_package_block(&thresholds));
        // A debug build measures bounds checks and an unoptimised grammar walk, not the extractor,
        // so the floor is only enforced where the number means something — the same exemption spike
        // `s1` used, and for the same reason: `--check` runs in debug precisely so CI is fast.
        //
        // The other way a language may miss the floor without failing is a *named exemption* in
        // `thresholds.json`, and an exemption is never "this language is allowed to be slow". It is a
        // condition with a checkable consequence, of which there are two kinds:
        //
        // * `"via": "scan_budget"`, for a grammar that really costs more per byte (Ruby, which `s1`
        //   measured under 1 MiB/s against 11-16 for the rest): the per-run byte cap divided by the
        //   measured rate must fit inside ADR-004's cold-graph budget, which `scan_budget.cost`
        //   computes and the contract test re-checks. A slow grammar is a bound, not a free pass.
        // * `"via": "repository_shape"`, for a language the replica mis-measures (Go and Java, whose
        //   imports are package-granular and so fan out 200x wider in a corpus of 444 copies of an
        //   eleven-file fixture than in any repository): the exemption holds only if the
        //   machine-authored repository-shaped corpus in `throughput.repository` clears both floors.
        let exemption = thresholds
            .get("throughput")
            .and_then(|row| row.get("exempt"))
            .and_then(|row| row.get(name));
        let exempt_floor = exemption
            .as_ref()
            .and_then(|row| row.get("mib_per_second_floor"))
            .and_then(Value::as_f64);
        let budget_ok = scan_budget
            .get("cost")
            .and_then(|cost| cost.get("inside_graph_budget"))
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let repository = throughput.get("repository");
        let repository_files = repository
            .as_ref()
            .and_then(|row| row.get("files_per_second_per_core"))
            .and_then(Value::as_f64)
            .unwrap_or_default();
        let repository_mib = repository
            .as_ref()
            .and_then(|row| row.get("mib_per_second"))
            .and_then(Value::as_f64)
            .unwrap_or_default();
        let exempt_via = exemption
            .as_ref()
            .and_then(|row| row.get("via"))
            .and_then(Value::as_str);
        // The two floors are the same statement in two units only when a file is about a kilobyte:
        // 1 MiB/s at 1 kB per file *is* 1 000 files/s. A machine-authored corpus has its own mean file
        // size, so judging it against the canary as written would compare a 0.46 kB corpus with a 0.9 kB
        // one and call the difference a regression. The canary is therefore scaled by the corpus's own
        // mean file size, which is the only way the two floors agree on two corpora.
        let shape_mean_kib = repository
            .as_ref()
            .and_then(|row| Some((row.get("bytes")?.as_f64()?, row.get("files")?.as_f64()?)))
            .map(|(bytes, files)| {
                if files == 0.0 {
                    1.0
                } else {
                    bytes / files / 1_024.0
                }
            })
            .unwrap_or(1.0);
        let shape_canary_floor = round(mib_floor * shape_mean_kib);
        let exempt_ok = match exempt_via {
            Some("scan_budget") => {
                exempt_floor.is_some_and(|floor| measured_mib_per_second >= floor) && budget_ok
            }
            Some("repository_shape") => {
                repository_files >= throughput_floor && repository_mib >= shape_canary_floor
            }
            // An exemption with no `via` is a hole in the thresholds, so it exempts nothing.
            _ => false,
        };
        let basis = if measured_files_per_second >= throughput_floor
            && measured_mib_per_second >= mib_floor
        {
            "replica"
        } else if exempt_ok {
            exempt_via.unwrap_or("none")
        } else {
            "none"
        };
        let throughput_ok =
            (basis != "none") || cfg!(debug_assertions) || !args.bench && !args.write;
        let verdict = verdict_of(name, &score, &mechanisms, &thresholds, throughput_ok);
        if !args.quiet {
            print_one(name, &score, &mechanisms, &verdict);
        }
        languages.push(json!({
            "language": name,
            "grammar_crate": format!("tree-sitter-{name}"),
            "corpus": {
                "files": score.corpus_files,
                "bytes": score.corpus_bytes,
                "definitions": annotation.rows().0,
                "exports": annotation.rows().1,
                "import_edges": annotation.rows().2,
                "bases": annotation.rows().3,
                "synthetic_scopes": annotation.rows().4,
            },
            "relations": {
                "definitions": row(&score.definitions),
                "exports": row(&score.exports),
                "imports": row(&score.imports),
                "supertypes": row(&score.supertypes),
            },
            "tags": score
                .tags
                .iter()
                .map(|(tag, row)| json!({"tag": tag, "expected": row.tp + row.miss, "found": row.tp, "recall": round(row.recall())}))
                .collect::<Vec<Value>>(),
            "missing_definitions": score.missing_defs,
            "surprising_definitions": score.extra_defs,
            "missing_exports": score.missing_exports,
            "surprising_exports": score.extra_exports,
            "missing_edges": score.missing_edges,
            "surprising_edges": score.extra_edges,
            "missing_supertypes": score.missing_bases,
            "surprising_supertypes": score.extra_bases,
            "supertype_kind_mismatches": score.base_kind_mismatches,
            "truth_root": score.root,
            "truth_notes": score.notes,
            "truth_bases_notes": score.bases_notes,
            "truth_synthetic_notes": score.synthetic_notes,
            "missing_synthetic": score.missing_synthetic,
            "irreducible_rows": score.irreducible_rows,
            "blind_recovered": score.blind_recovered,
            "irreducible_tags": irreducible.iter().cloned().collect::<Vec<String>>(),
            "blind_spots": score
                .blind
                .iter()
                .map(|(kind, count)| json!({"kind": kind, "count": count}))
                .collect::<Vec<Value>>(),
            "mechanisms": mechanisms,
            "fallback": fallback,
            "granularity": granularity,
            "scan_budget": scan_budget,
            "surface": surface,
            "same_package": same_package,
            "extraction": {
                "milliseconds": round(extract_elapsed.as_secs_f64() * 1_000.0),
                "policy": policy.to_json(),
                "policy_notices": policy_notices,
            },
            "throughput": throughput,
            "throughput_check": {
                "floor_files_per_second_per_core": throughput_floor,
                "measured_files_per_second_per_core": round(measured_files_per_second),
                "floor_mib_per_second": mib_floor,
                "measured_mib_per_second": round(measured_mib_per_second),
                "exemption": exemption,
                "exemption_satisfied": exempt_ok,
                "basis": basis,
                "repository_shape_canary_floor": shape_canary_floor,
                "repository_shape": repository,
                "enforced": !cfg!(debug_assertions),
                "ok": throughput_ok,
                "mean": "the replica, measured: files per second per core is the contract because ADR-004's budgets are paid per changed file; MiB/s is reported next to spike s1's parse-only figure to show what resolution adds on top of parsing. A debug build exempts both, because a debug build measures the borrow checker.",
            },
            "verdict": verdict,
        }));
        throughputs.push(json!({"language": name, "corpora": throughput}));
    }

    let report = build_report(
        args,
        &thresholds,
        grammars,
        languages,
        &throughputs,
        &policy,
    );
    let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    let mut code = 0;

    if let Some(path) = &args.snapshot {
        // The byte-identity contract, written out. The Rust test compares against this file, so the
        // bytes here are the ones a reviewer can diff.
        let sources: Vec<SourceFile> = all_owned
            .iter()
            .filter(|(path, _)| {
                LANGUAGES.iter().any(|(language, exts)| {
                    exts.iter().any(|ext| path.ends_with(&format!(".{ext}")))
                        && language_matches(language, path)
                })
            })
            .map(|(path, body)| SourceFile::new(path, body))
            .collect();
        let corpus = structure::extract(&sources, &policy);
        let bytes = structure::canonical_json(&corpus);
        if args.bless {
            if let Some(parent) = Path::new(path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(path, bytes).map_err(|e| e.to_string())?;
            if !args.quiet {
                println!("wrote {}", path.display());
            }
        } else {
            let committed = std::fs::read(path).unwrap_or_default();
            if committed != bytes {
                code = 1;
                println!(
                    "e3.1 structure harness: the snapshot does NOT reproduce ({}): committed {} bytes, run {} bytes",
                    path.display(),
                    committed.len(),
                    bytes.len()
                );
            }
        }
    }

    if args.check {
        let Some(path) = &args.report else {
            return Err("--check needs --report to compare against".to_string());
        };
        let committed: Value = read_json(path)?;
        let failures = compare(&committed, &report);
        if failures.is_empty() {
            println!(
                "e3.1 structure harness: the committed report reproduces exactly ({})",
                path.display()
            );
        } else {
            code = 1;
            println!(
                "e3.1 structure harness: the committed report does NOT reproduce ({}):",
                path.display()
            );
            for line in failures {
                println!("  - {line}");
            }
        }
    }

    if args.write
        && let Some(path) = &args.report
    {
        if let Some(parent) = Path::new(path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(path, format!("{text}\n")).map_err(|e| e.to_string())?;
        if !args.quiet {
            println!("wrote {}", path.display());
        }
    }

    if args.bench {
        let floor = thresholds
            .get("throughput")
            .and_then(|row| row.get("files_per_second_per_core_floor"))
            .and_then(Value::as_f64)
            .unwrap_or(1_000.0);
        let mut failed = false;
        for row in &throughputs {
            let language = row["language"].as_str().unwrap_or("?");
            let measured = row["corpora"]["replica"]["files_per_second_per_core"]
                .as_f64()
                .unwrap_or_default();
            let line = format!(
                "{language}: {measured:.0} files/s/core (floor {floor:.0}) in a {} build",
                if cfg!(debug_assertions) {
                    "debug"
                } else {
                    "release"
                }
            );
            if measured < floor && !cfg!(debug_assertions) {
                failed = true;
                println!("BENCH FAIL {line}");
            } else {
                println!("BENCH ok     {line}");
            }
        }
        if failed {
            code = 1;
        }
    }

    if !args.quiet && !args.write && !args.check && !args.bench && args.snapshot.is_none() {
        println!("{text}");
    }
    Ok(code)
}

// ---------------------------------------------------------------------------
// the four mechanisms, measured
// ---------------------------------------------------------------------------

/// The four things `e0.12` handed over as non-negotiables, each turned into a number.
///
/// These are not extra accuracy metrics. They are the four ways a 100 % accuracy score can still be
/// a lie, which is the argument spike `s1` spent 900 minutes of planning making.
fn mechanisms(
    annotation: &truth::Truth,
    corpus: &structure::CorpusStructure,
    policy: &ExtractPolicy,
    bound_demo: Value,
) -> Value {
    let mut expected_wildcards = 0usize;
    let mut found_wildcards = 0usize;
    let mut wildcard_looks_resolved = Vec::new();
    let mut expected_synthetic = 0usize;
    let mut found_synthetic = 0usize;
    let mut emptiness_assertions = Vec::new();
    let mut over_marked = 0usize;
    let mut declared_empty = 0usize;
    let mut passes_used_max = 0usize;
    let mut non_converged = Vec::new();
    let mut truncated_chains = 0usize;
    let mut external_star_imports = 0usize;
    let mut expected_synthetic_keys: BTreeSet<String> = BTreeSet::new();

    for (path, file) in &annotation.files {
        for import in &file.imports {
            if import.s == "*" {
                expected_wildcards += 1;
            }
        }
        for scope in &file.synthetic {
            expected_synthetic += 1;
            expected_synthetic_keys.insert(synthetic_key(path, &scope.scope));
        }
    }

    for record in &corpus.files {
        let Some(structure) = &record.structure else {
            continue;
        };
        for edge in &structure.edges {
            if edge.is_wildcard() {
                found_wildcards += 1;
                // The one mislabel that matters: a dependency of unknown contents printed as if its
                // contents were known. A wildcard edge carrying a named symbol is that lie.
                if edge.symbol != "*" {
                    wildcard_looks_resolved.push(format!(
                        "{} -> {} named {} but the contents are unknown",
                        record.path, edge.target, edge.symbol
                    ));
                }
            } else if edge.kind == structure::EdgeKind::Resolved && edge.symbol == "*" {
                wildcard_looks_resolved.push(format!(
                    "{} -> {} resolved with no symbol named",
                    record.path, edge.target
                ));
            } else if edge.kind == structure::EdgeKind::Unresolved && edge.symbol == "*" {
                // Honest and correct: a star import of something outside the corpus. Counted, not
                // charged — there is no file to point at.
                external_star_imports += 1;
            }
        }
        for scope in &structure.synthetic_scopes {
            let key = synthetic_key(&record.path, &scope.scope);
            if expected_synthetic_keys.contains(&key) {
                found_synthetic += 1;
            } else {
                over_marked += 1;
            }
            // The state, not the absence: a marked scope must never be presented as "declares
            // nothing". `may_have_shrunk` is the only sentence a surface is allowed to print about it.
            if structure.may_have_shrunk().is_none() {
                emptiness_assertions.push(key);
            }
        }
        if structure.symbols.is_empty() && structure.synthetic_scopes.is_empty() {
            declared_empty += 1;
        }
        passes_used_max = passes_used_max.max(structure.resolution.passes_used);
        if !structure.resolution.converged {
            non_converged.push(record.path.clone());
        }
        truncated_chains += structure.resolution.truncated_chains;
    }

    let mut wildcard_query = json!([]);
    if let Some(question) = annotation.wildcard_question() {
        let answer = corpus.dependents_of(&question.0, question.1.as_deref());
        wildcard_query = json!({
            "file": question.0,
            "symbol": question.1,
            "named": answer.named,
            "unknown": answer.unknown,
            "says_unknown": !answer.unknown.is_empty(),
        });
    }

    json!({
        "synthetic_eligible": {
            "expected": expected_synthetic,
            "found": found_synthetic,
            "missing": expected_synthetic.saturating_sub(found_synthetic),
            "marked_beyond_the_annotation": over_marked,
            "scopes_that_assert_emptiness": emptiness_assertions,
            "empty_files_that_assert_completeness": declared_empty,
            "mean": "s1 finding 1: a scope a program wrote into is a scope that cannot be enumerated. `missing` is the number that matters; a marked scope never yields a 'declares nothing' assertion, which is what `scopes_that_assert_emptiness` proves."
        },
        "wildcard_edges": {
            "expected": expected_wildcards,
            "found": found_wildcards,
            "mislabelled_as_resolved": wildcard_looks_resolved,
            "external_star_imports": external_star_imports,
            "who_depends_on": wildcard_query,
            "mean": "s1 finding 2: a star import is a real dependency of unknown contents. `who_depends_on` is the AC made visible — the answer must name the unknown files, never say nobody."
        },
        "resolution": {
            "bound_passes": policy.max_resolution_passes,
            "bound_hops": policy.max_resolution_hops,
            "passes_used_max": passes_used_max,
            "non_converged": non_converged,
            "truncated_chains": truncated_chains,
            "forced_one_pass": bound_demo,
            "mean": "s1 finding 6: resolution is iterative, so it is bounded per file and the bound is a config key. `non_converged` names the files where the bound bit; on this corpus it is empty by construction, and `--max-passes 1` makes it non-empty on purpose."
        },
        "never_lost": {
            "records": corpus.summary.files,
            "extracted": corpus.summary.extracted,
            "unavailable": corpus.summary.unavailable,
            "mean": "AC 2: every file comes back, with a reason when there is no structure. `unavailable` here is 0 for a clean corpus; the `fallback` battery below is what proves the non-zero path works."
        }
    })
}

/// The tags whose truth rows are excluded from scoring, per `thresholds.json`.
fn irreducible_tags(thresholds: &Value, language: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for key in ["default", language] {
        if let Some(rows) = thresholds
            .get("irreducible_tags")
            .and_then(|row| row.get(key))
            .and_then(Value::as_array)
        {
            for tag in rows {
                if let Some(tag) = tag.as_str() {
                    out.insert(tag.to_string());
                }
            }
        }
    }
    out
}

fn synthetic_key(file: &str, scope: &str) -> String {
    format!("{file} \u{2022} {scope}")
}

/// The "unknown or broken language" acceptance criterion, exercised rather than asserted.
///
/// The corpus is deliberately well-behaved, so the fallback path needs its own inputs. These are
/// constructed in memory, extracted through the same code path, and reported: a fallback nobody has
/// seen fire is decoration.
fn fallback_battery(policy: &ExtractPolicy) -> Value {
    let cases: Vec<(&str, &[u8], &str)> = vec![
        (
            "notes.unknown-lang",
            b"just some text, not any language fathom reads\n",
            "language_unknown",
        ),
        (
            "broken.rs",
            b"fn oops( { let x = ; if { }]]] ",
            "parse_failed_or_parsed",
        ),
        (
            "not-python.py",
            b"\x00\x01\x02 not utf-8 at all \xff\xfe",
            "not_utf8",
        ),
        ("big.rs", b"fn small() {}\n", "too_large"),
    ];
    let mut rows = Vec::new();
    for (path, body, expect) in &cases {
        let mut small_policy = policy.clone();
        if *expect == "too_large" {
            // The limit is what is being tested, so the policy is told to refuse this size.
            small_policy.max_file_bytes = body.len().saturating_sub(1);
        }
        let record = structure::extract_one(path, body, &small_policy);
        rows.push(json!({
            "path": path,
            "language": record.language.as_str(),
            "extracted": record.structure.is_some(),
            "reason": record.reason.as_ref().map(|reason| reason.as_str().to_string()),
            "sentence": record.reason.as_ref().map(|reason| reason.sentence()),
            "file_level_bytes": record.file_level.bytes,
            "file_level_lines": record.file_level.newlines,
            "file_level_code_lines": record.file_level.code_lines,
            "parse_errors": record.file_level.parse_errors,
            "expect": expect,
        }));
    }
    let every_file_came_back = rows.len() == cases.len();
    let every_unreadable_has_file_level = rows.iter().all(|row| {
        row["file_level_bytes"].as_u64().unwrap_or(0) > 0 || row["extracted"] == json!(true)
    });
    json!({
        "cases": rows,
        "every_input_returned": every_file_came_back,
        "every_unreadable_still_measured": every_unreadable_has_file_level,
        "mean": "AC 2: an unknown or broken language yields structure unavailable plus file-level analysis, never a lost episode. `broken.rs` may parse — tree-sitter recovers — and that is the honest outcome, so it is reported rather than asserted."
    })
}

// ---------------------------------------------------------------------------
// the three things `e3.1b` added to the report
// ---------------------------------------------------------------------------

/// What the corpus's edges are *about*: files, or whole packages.
///
/// This is the acceptance criterion "Go edges are package-level, or every file-level Go edge is
/// labelled as one and the panes say so", made into a number and a sentence. The panes themselves are
/// `e3.5` and `e3.6`; what this ticket owes them is the label, the count and the wording, all of which
/// come out of the extractor rather than being typed into a report.
fn granularity_block(corpus: &structure::CorpusStructure) -> Value {
    let mut total = 0usize;
    let mut package_edges = 0usize;
    let mut files_with = 0usize;
    let mut packages: BTreeSet<String> = BTreeSet::new();
    let mut sentence: Option<String> = None;
    for record in &corpus.files {
        let Some(structure) = &record.structure else {
            continue;
        };
        total += structure.edges.len();
        let here = structure.package_edges().len();
        package_edges += here;
        if here > 0 && sentence.is_none() {
            sentence = structure.granularity_sentence();
        }
        files_with += usize::from(here > 0);
        if let Some(package) = &structure.package {
            packages.insert(package.clone());
        }
    }
    json!({
        "edges_total": total,
        "edges_package_granular": package_edges,
        "files_with_package_granular_edges": files_with,
        "declared_units": packages.into_iter().collect::<Vec<String>>(),
        "label_for_a_pane": sentence,
        "mean": "Go's dependency is package-to-package; the record is per file because that is what the store keys on. Every such edge carries `granularity: package` and the import path it came from, and `label_for_a_pane` is the sentence a surface prints instead of implying a file-to-file fact nobody declared. A language with none of these has edges that may be printed as file-level facts, which is most of them.",
    })
}

/// What a file's export list may be read as, and the question the language can answer instead.
///
/// Ruby's honest surface question is *which files does this entry point load* (s1 finding 3), so the
/// report asks it, over the corpus's own annotated entry point.
fn surface_block(language: &str, entry: &str, corpus: &structure::CorpusStructure) -> Value {
    let answer = (!entry.is_empty()).then(|| structure::loads(corpus, entry));
    json!({
        "entry_point": entry,
        "loads": answer.as_ref().map(|loads| json!({
            "sentence": loads.sentence(),
            "files": loads.files,
            "depth": loads.depth,
            "truncated": loads.truncated,
        })),
        "may_claim_public_symbols": language != "ruby",
        "mean": if language == "ruby" {
            "Ruby has no per-symbol access control, so `exports` here is every top-level name the file wrote and nothing more: a surface that prints it under the heading `public symbols` is lying about the language. The question Ruby can answer is the one above - what a require pulls in - and `e3.7` is told to print that instead."
        } else {
            "This language's export rule is a real access rule, so a surface may print its export list as public API."
        },
    })
}

/// The per-run scan budget: what it is set to, what it refuses when it bites, and what it costs.
///
/// The cap is shown rather than asserted — the same argument `--max-passes 1` makes for the resolution
/// bound. The cost half is arithmetic on the throughput the harness just measured: a byte cap divided
/// by a measured rate is a number of seconds, and ADR-004's cold-graph budget says whether that number
/// is affordable.
fn scan_budget_block(
    language: Language,
    policy: &ExtractPolicy,
    sources: &[SourceFile],
    measured_mib_per_second: f64,
    thresholds: &Value,
) -> Value {
    let graph_budget_ms = thresholds
        .get("throughput")
        .and_then(|row| row.get("graph_budget_ms"))
        .and_then(Value::as_f64)
        .unwrap_or(60_000.0);
    let Some(budget) = policy.scan_budget(language) else {
        return json!({
            "configured": null,
            "mean": "No per-run cap for this language, on purpose: s1 measured it inside the pack, and a cap that can never bind is decoration. The mechanism is here for the next grammar that costs what Ruby costs.",
        });
    };
    // The demonstration: the same corpus, with the count cap set to two files.
    let mut tiny = policy.clone();
    if language == Language::Ruby {
        tiny.ruby_scan.max_files = 2;
    }
    let squeezed = structure::extract(sources, &tiny);
    let refused: Vec<String> = squeezed
        .files
        .iter()
        .filter(|record| {
            record
                .reason
                .as_ref()
                .is_some_and(|reason| reason.as_str() == "scan_budget")
        })
        .map(|record| record.path.clone())
        .collect();
    let cost = if measured_mib_per_second > 0.0 && budget.max_bytes > 0 {
        let implied_ms = budget.max_bytes as f64 / (measured_mib_per_second * 1_048_576.0) * 1e3;
        json!({
            "measured_mib_per_second": round(measured_mib_per_second),
            "implied_ms_at_the_cap": round(implied_ms),
            "graph_budget_ms": graph_budget_ms,
            "inside_graph_budget": implied_ms <= graph_budget_ms,
            "mean": "the byte cap divided by the measured rate: the most a cold run can spend on this language, against ADR-004's `cold.graph.ms` small-class budget. This is why a slow grammar is a scan-policy question rather than a feature-gate argument.",
        })
    } else {
        json!(null)
    };
    json!({
        "configured": {
            "max_files_per_run": budget.max_files,
            "max_bytes_per_run": budget.max_bytes,
            "priority": budget.priority,
        },
        "demonstration": {
            "max_files": 2,
            "refused": refused.len(),
            "refused_paths": refused.iter().take(12).cloned().collect::<Vec<String>>(),
            "every_refused_file_still_measured": squeezed
                .files
                .iter()
                .filter(|record| record.structure.is_none())
                .all(|record| record.file_level.bytes > 0),
        },
        "cost": cost,
        "mean": "s1 finding 7: the feature gates stay and the scan policy is what changes. Ruby is the only language with a cap by default, because it is the only grammar measured an order of magnitude off the pack; files past the cap come back `scan_budget` with their file-level numbers, in the order `extract.ruby.priority` ranked them.",
    })
}

/// One measurement of the same-package scan on the machine-authored repository-scale corpus.
fn measure_same_package(
    generated: &java_corpus::Generated,
    scope: fathom_analyze::structure::ScanScope,
    policy: &ExtractPolicy,
) -> Value {
    let mut scoped_policy = policy.clone();
    scoped_policy.java_same_package_scan = scope;
    let sources: Vec<SourceFile> = generated
        .files
        .iter()
        .map(|(path, body)| SourceFile::new(path, body))
        .collect();
    let started = Instant::now();
    let corpus = structure::extract(&sources, &scoped_policy);
    let elapsed = started.elapsed().as_secs_f64().max(1e-9);
    let mut produced: BTreeSet<String> = BTreeSet::new();
    for record in &corpus.files {
        let Some(structure) = &record.structure else {
            continue;
        };
        for edge in &structure.edges {
            if edge.form == "same_package" {
                produced.insert(format!(
                    "{} \u{2022} {} \u{2022} {}",
                    record.path, edge.target, edge.symbol
                ));
            }
        }
    }
    let correct: BTreeSet<String> = produced.intersection(&generated.planted).cloned().collect();
    let false_positives: Vec<String> = produced.difference(&generated.planted).cloned().collect();
    let missed: Vec<String> = generated.planted.difference(&produced).cloned().collect();
    let claimed = produced.len();
    let expected = generated.planted.len();
    json!({
        "scope": scope.as_str(),
        "edges_claimed": claimed,
        "dependencies_in_the_corpus": expected,
        "correct": correct.len(),
        "false_positive": false_positives.len(),
        "missed": missed.len(),
        "false_positive_rate": round(if claimed == 0 { 0.0 } else { false_positives.len() as f64 / claimed as f64 }),
        "recall": round(if expected == 0 { 1.0 } else { correct.len() as f64 / expected as f64 }),
        "examples_false_positive": false_positives.into_iter().take(8).collect::<Vec<String>>(),
        "examples_missed": missed.into_iter().take(8).collect::<Vec<String>>(),
        "seconds": round(elapsed),
    })
}

/// The published false-positive rate and the stated budget, measured twice: once with the shipped
/// scoped scan and once with the wide scan `s1` ran, so the scoping decision is a number a later
/// ticket can re-run rather than a paragraph.
fn same_package_block(thresholds: &Value) -> Value {
    let wanted = thresholds
        .get("same_package")
        .and_then(|row| row.get("corpus_files"))
        .and_then(Value::as_u64)
        .map(|n| n as usize)
        .unwrap_or(SAME_PACKAGE_CORPUS_FILES);
    let fp_budget = thresholds
        .get("same_package")
        .and_then(|row| row.get("max_false_positive_rate"))
        .and_then(Value::as_f64)
        .unwrap_or(0.02);
    let recall_floor = thresholds
        .get("same_package")
        .and_then(|row| row.get("min_recall"))
        .and_then(Value::as_f64)
        .unwrap_or(0.95);
    let generated = java_corpus::author(wanted);
    let policy = ExtractPolicy::default();
    let scoped = measure_same_package(
        &generated,
        fathom_analyze::structure::ScanScope::ImportsPlusPackage,
        &policy,
    );
    let wide = measure_same_package(
        &generated,
        fathom_analyze::structure::ScanScope::AllTypeReferences,
        &policy,
    );
    let unbound_policy = ExtractPolicy::default().with_bound_name_rule_off();
    let unbound = measure_same_package(
        &generated,
        fathom_analyze::structure::ScanScope::ImportsPlusPackage,
        &unbound_policy,
    );
    let met = scoped["false_positive_rate"].as_f64().unwrap_or(1.0) <= fp_budget
        && scoped["recall"].as_f64().unwrap_or(0.0) >= recall_floor;
    // What the scoped scan does with the bound-name rule switched off: a third scan of the same
    // corpus, not arithmetic about a rule. Arithmetic about a rule you changed is the kind of number
    // that stops matching the code the day someone edits the rule; this one cannot drift, and it is
    // what makes the rule a measured decision rather than a preference.
    let counterfactual = json!({
        "without_the_bound_name_rule": {
            "false_positive": unbound["false_positive"],
            "false_positive_rate": unbound["false_positive_rate"],
            "edges_claimed": unbound["edges_claimed"],
            "recall": unbound["recall"],
            "added_edges": unbound["edges_claimed"].as_u64().unwrap_or(0)
                .saturating_sub(scoped["edges_claimed"].as_u64().unwrap_or(0)),
            "bounds_name_baits": generated.bound_baits,
            "over_budget": unbound["false_positive_rate"].as_f64().unwrap_or(1.0) > fp_budget,
            "mean": "the same scoped scan, re-run with the rule that treats a name the file binds itself as a mention switched off. Every added edge is a method type variable spelled like a type another file of the same package declares: the source cannot mean any of them, and a scan that only counts words would clear its recall floor while claiming them. Measured rather than inferred, because a counterfactual computed from the same assumption being tested proves nothing.",
        },
    });
    json!({
        "corpus": {
            "files": generated.files.len(),
            "packages": generated.packages,
            "planted_dependencies": generated.planted.len(),
            "shadowed_imports": generated.shadowed,
            "type_variable_baits": generated.bound_baits,
            "type_names_declared_in_more_than_one_package": generated.colliding_names,
            "authored_by": "crates/fathom-analyze/examples/structure_harness/java_corpus.rs",
            "mean": "machine-authored, and said out loud: nobody hand-annotates 1 200 Java files. What makes the number worth having is that the generator plants every dependency it expects and nothing else, and fills the type-name pool with the collisions (`Builder`, `Result`, `Config`) that a same-package scan can only get wrong when a name means several files.",
        },
        "budget": {
            "min_files": wanted,
            "max_false_positive_rate": fp_budget,
            "min_recall": recall_floor,
        },
        "counterfactual": counterfactual,
        "scoped": scoped,
        "wide": wide,
        "budget_met": met,
        "mean": "s1 finding 5, measured at repository scale instead of on ten files. Three rules make the scoped column what it is: a reference resolves against the file's own declared package and nowhere else; a single-type import of the same spelling shadows it (JLS 6.5.5); and a name the file itself binds as a type parameter is a mention, not a use. The wide column is the spike's version - every type reference, resolved against any package that declares the name - kept as a measurement so the cost of that reading stays a number. The coarse edge of the shipped scan is stated too: bound names are collected per file, not per scope, so a file that binds `<Result>` and also names the package's `Result` elsewhere loses that one edge, which is a hole rather than a wrong answer.",
    })
}

// ---------------------------------------------------------------------------
// throughput
// ---------------------------------------------------------------------------

/// One measured corpus: owned `(path, bytes)` pairs, because the replica has to outlive the borrow
/// the measurement runs over.
type Corpus = Vec<(String, Vec<u8>)>;

/// Parse-plus-extract throughput over the fixture and a deterministic replica of it.
///
/// The floor is stated in files per second per core, because that is the unit the acceptance
/// criterion is written in and the unit `e3.2`'s budgets are paid in: a repository is a number of
/// files, not a number of megabytes. The replica exists because 26 fixture files measure process
/// startup, not extraction.
fn throughput(
    language: &str,
    policy: &ExtractPolicy,
    dir: &Path,
    files: &[String],
    sources: &[(&str, &[u8])],
) -> Value {
    let mut out = json!({});
    let mut corpora: Vec<(&str, Corpus)> = vec![(
        "fixture",
        sources
            .iter()
            .map(|(path, body)| (path.to_string(), body.to_vec()))
            .collect(),
    )];
    let mut replica: Corpus = Vec::new();
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut rounds = 0usize;
    while replica.len() < 4_000 {
        for (path, body) in sources {
            let mut pad = String::new();
            let mut lines = 1 + (next(&mut seed) % 24) as usize;
            while lines > 0 {
                pad.push_str(&format!("// replica {:x}\n", next(&mut seed) % 4096));
                lines -= 1;
            }
            let mut text = pad.into_bytes();
            text.extend_from_slice(body);
            // The `.rs` has to stay the last suffix: the replica must still *be* the language, or the
            // measurement is a measurement of the refusal path and reports a speed nobody built.
            let name = match path.split_once('.') {
                Some((stem, rest)) => format!("{stem}.replica{rounds}.{rest}"),
                None => format!("{path}.replica{rounds}"),
            };
            replica.push((name, text));
        }
        rounds += 1;
    }
    corpora.push(("replica", replica));

    // A third shape, for the two languages whose *replica* is pathological. Replicating a fixture 444
    // times turns a package of four files into a package of eight hundred, and a package-granular
    // import then fans out 200× wider than the language's own dependency ever would: the replica was
    // measuring its own construction. These corpora are machine-authored at a believable
    // files-per-package instead, and a throughput exemption is only granted against them — see
    // `throughput_check.basis` in the report, and `thresholds.json`'s `throughput.exempt`.
    if language == "java" {
        corpora.push((
            "repository",
            java_corpus::author(REPOSITORY_SHAPE_FILES).files,
        ));
    }
    if language == "go" {
        corpora.push(("repository", go_corpus::author(REPOSITORY_SHAPE_FILES)));
    }

    for (label, corpus) in &corpora {
        // The caps are lifted for the measurement on purpose: the floor is a statement about the
        // extractor's cost, and measuring the refusal path would report a speed nobody built. What
        // the caps cost is computed from this number (`scan_budget.cost`), not hidden behind it.
        let mut throughput_policy = policy.clone();
        throughput_policy.ruby_scan = fathom_analyze::structure::ScanBudget {
            max_files: 0,
            max_bytes: 0,
            priority: policy.ruby_scan.priority.clone(),
        };
        let borrowed: Vec<SourceFile> = corpus
            .iter()
            .map(|(path, body)| SourceFile::new(path, body))
            .collect();
        let total: usize = corpus.iter().map(|(_, body)| body.len()).sum();
        if total == 0 {
            continue;
        }
        let started = Instant::now();
        let extracted = structure::extract(&borrowed, &throughput_policy);
        let elapsed = started.elapsed().as_secs_f64().max(1e-9);
        out[*label] = json!({
            "files": corpus.len(),
            "bytes": total,
            "files_per_second_per_core": round(corpus.len() as f64 / elapsed),
            "mib_per_second": round(total as f64 / 1_048_576.0 / elapsed),
            "per_file_us_p50": round(elapsed * 1e6 / corpus.len() as f64),
            "symbols_found": extracted.summary.symbols,
            "edges_found": extracted.summary.edges_resolved + extracted.summary.edges_wildcard,
        });
    }
    let _ = (language, dir, files);
    out
}

fn next(state: &mut u64) -> u64 {
    // xorshift64*, so a replica is the same corpus next week and on another machine.
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

// ---------------------------------------------------------------------------
// verdict, report, printing
// ---------------------------------------------------------------------------

/// `pass`, `conditions` or `fail`, decided by `thresholds.json` and nothing else — the same rule
/// shape spike `s1` used, so the two reports can be read side by side.
fn verdict_of(
    language: &str,
    score: &LangScore,
    mechanisms: &Value,
    thresholds: &Value,
    throughput_ok: bool,
) -> String {
    let rules = thresholds
        .get("relations")
        .and_then(|row| row.get("default"))
        .cloned()
        .unwrap_or(json!({}));
    let per_language = thresholds
        .get("relations")
        .and_then(|row| row.get(language))
        .cloned();
    let mut worst = "pass";
    let relations: [(&str, &Score); 4] = [
        ("definitions", &score.definitions),
        ("exports", &score.exports),
        ("imports", &score.imports),
        ("supertypes", &score.supertypes),
    ];
    for (relation, row) in &relations {
        let Some(rule) = per_language
            .as_ref()
            .and_then(|row| row.get(*relation))
            .or_else(|| rules.get(*relation))
        else {
            continue;
        };
        let min_recall = rule.get("recall").and_then(Value::as_f64).unwrap_or(1.0);
        let min_precision = rule.get("precision").and_then(Value::as_f64).unwrap_or(1.0);
        if row.recall() >= min_recall && row.precision() >= min_precision {
            continue;
        }
        worst = if rule.get("hard").and_then(Value::as_bool).unwrap_or(false) {
            "fail"
        } else {
            "conditions"
        };
        break;
    }
    // A mechanism that did not hold is a condition even when every floor passed, because the
    // mechanisms are the reasons a floor is not enough.
    let synthetic_missing = mechanisms["synthetic_eligible"]["missing"]
        .as_u64()
        .unwrap_or(0);
    let wildcards_mislabelled = mechanisms["wildcard_edges"]["mislabelled_as_resolved"]
        .as_array()
        .map(|rows| rows.len())
        .unwrap_or(0);
    let wildcard_query_says_unknown =
        mechanisms["wildcard_edges"]["who_depends_on"]["says_unknown"]
            .as_bool()
            .unwrap_or(true);
    let asserts_emptiness = mechanisms["synthetic_eligible"]["scopes_that_assert_emptiness"]
        .as_array()
        .map(|rows| rows.len())
        .unwrap_or(0);
    if synthetic_missing > 0
        || wildcards_mislabelled > 0
        || asserts_emptiness > 0
        || !wildcard_query_says_unknown
    {
        worst = "fail";
    }
    // Clearing the floors while still missing rows is `conditions`, with the irreducible categories
    // from the thresholds as the excuse — exactly s1's rule, kept so the two reports agree.
    if worst == "pass"
        && (relations
            .iter()
            .any(|(_, row)| row.miss > 0 || row.fp > 0)
            // A recorded blind spot is a condition even at a perfect score, and this is the sentence
            // that says why: the score is over what the corpus annotated, and the blind spot is a
            // thing the annotation had to admit the file cannot answer. `e3.2` should read the
            // condition, not infer it from an empty gap.
            || !score.blind.is_empty())
    {
        worst = "conditions";
    }
    if worst == "pass" && !throughput_ok {
        worst = "conditions";
    }
    worst.to_string()
}

fn build_report(
    args: &Args,
    thresholds: &Value,
    grammars: Vec<Value>,
    languages: Vec<Value>,
    throughput: &[Value],
    policy: &ExtractPolicy,
) -> Value {
    let verdicts: Vec<(String, String)> = languages
        .iter()
        .map(|row| {
            (
                row["language"].as_str().unwrap_or("?").to_string(),
                row["verdict"].as_str().unwrap_or("?").to_string(),
            )
        })
        .collect();
    let pick = |want: &str| -> Vec<String> {
        verdicts
            .iter()
            .filter(|(_, verdict)| verdict == want)
            .map(|(language, _)| language.clone())
            .collect()
    };
    let passing = pick("pass");
    let conditional = pick("conditions");
    let failing = pick("fail");
    json!({
        "ticket": {
            "id": "reco-zoz",
            "plan_key": "e3.1",
            "title": "structural extractor core: rust, python, typescript, and the corpus harness",
            "tail": {
                "id": "reco-dlp.1",
                "plan_key": "e3.1b",
                "title": "structural extractor tail: go, ruby and java, with the scan-policy knobs",
                "added": ["go", "ruby", "java"],
                "mechanisms": [
                    "package-granular Go edges, labelled",
                    "the Ruby per-run scan budget with per-file priority",
                    "the Java same-package scan, scoped and re-measured at repository scale",
                ],
            },
            "extractor": "crates/fathom-analyze/src/structure/",
            "harness": "crates/fathom-analyze/examples/structure_harness.rs",
            "corpus": "crates/fathom-analyze/structure/corpus",
            "thresholds": "crates/fathom-analyze/structure/thresholds.json",
            "inherited_from": "crates/fathom-analyze/spike/s1 (corpus and truth, extended with bases and synthetic)",
            "schema_version": structure::SCHEMA_VERSION,
            "reproduce": format!("cargo run --release -p fathom-analyze --features langs-all --example structure_harness -- --repo . --report {REPORT_PATH}"),
            "available_languages": structure::available_languages(),
        },
        "environment": {
            "rustc": rustc_version(),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
            "what_these_numbers_are": "Definitions, exports, import edges and supertypes are exact set comparisons against hand-authored truth, inherited from spike s1 and extended for the two relations only a shipped extractor needs. They are a statement about THIS extractor, not about the grammars: s1's report is the grammar measurement, this is the implementation's.",
        },
        "policy": policy.to_json(),
        "thresholds": thresholds,
        "grammars": grammars,
        "languages": languages,
        "throughput": throughput.to_vec(),
        "verdicts": verdicts
            .into_iter()
            .map(|(language, verdict)| (language, json!(verdict)))
            .collect::<serde_json::Map<String, Value>>(),
        "summary": {
            "pass": passing,
            "conditions": conditional,
            "fail": failing,
            "report": REPORT_PATH,
            "corpus_bytes": languages.iter().map(|row| row["corpus"]["bytes"].as_u64().unwrap_or(0)).sum::<u64>(),
            "corpus_files": languages.iter().map(|row| row["corpus"]["files"].as_u64().unwrap_or(0)).sum::<u64>(),
        },
        "_args": { "repo": args.repo.display().to_string() },
    })
}

/// Everything that must be identical for the committed report to still be true. Throughput is
/// deliberately excluded: wall-clock on a different runner is not a contract.
fn compare(committed: &Value, fresh: &Value) -> Vec<String> {
    let mut failures = Vec::new();
    let rows = |report: &Value| -> Vec<(String, String, Value)> {
        report["languages"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .flat_map(|row| {
                        let language = row["language"].as_str().unwrap_or("?").to_string();
                        ["definitions", "exports", "imports", "supertypes"]
                            .into_iter()
                            .map(move |relation| {
                                (
                                    language.clone(),
                                    relation.to_string(),
                                    row["relations"][relation].clone(),
                                )
                            })
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    for (language, relation, expected) in rows(committed) {
        let Some((_, _, actual)) = rows(fresh)
            .into_iter()
            .find(|(at, at_relation, _)| *at == language && *at_relation == relation)
        else {
            failures.push(format!("{language}/{relation}: missing from the fresh run"));
            continue;
        };
        if expected != actual {
            failures.push(format!(
                "{language}/{relation}: committed {} but the run produced {}",
                compact(&expected),
                compact(&actual)
            ));
        }
    }
    let mechanisms = |report: &Value| -> Vec<(String, String, Value)> {
        report["languages"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .flat_map(|row| {
                        let language = row["language"].as_str().unwrap_or("?").to_string();
                        [
                            "synthetic_eligible",
                            "wildcard_edges",
                            "resolution",
                            "never_lost",
                        ]
                        .into_iter()
                        .map(move |check| {
                            (
                                language.clone(),
                                check.to_string(),
                                row["mechanisms"][check].clone(),
                            )
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    for (language, check, expected) in mechanisms(committed) {
        let mut fresh = mechanisms(fresh).into_iter();
        let Some((_, _, actual)) =
            fresh.find(|(at, at_check, _)| *at == language && *at_check == check)
        else {
            failures.push(format!("{language}/{check}: missing from the fresh run"));
            continue;
        };
        if expected != actual {
            failures.push(format!(
                "{language}/{check}: committed {expected} but the run produced {actual}"
            ));
        }
    }
    let verdicts = |report: &Value| -> Vec<String> {
        report["languages"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .map(|row| format!("{}={}", row["language"], row["verdict"]))
                    .collect()
            })
            .unwrap_or_default()
    };
    if verdicts(committed) != verdicts(fresh) {
        failures.push(format!(
            "verdicts moved: committed {:?}, run {:?}",
            verdicts(committed),
            verdicts(fresh)
        ));
    }
    // The published false-positive rate is a measurement, so CI re-measures it too: the numbers
    // `e3.2` was told to wait for are exactly the ones that must not be able to drift. Timings and the
    // illustrative samples are stripped first — a busy runner is not a regression, and eight example
    // strings that sort differently are not either.
    let same_package = |report: &Value| -> Option<Value> {
        let block = report["languages"]
            .as_array()?
            .iter()
            .find(|row| row["language"] == "java")?
            .get("same_package")?
            .clone();
        if block.is_null() {
            return None;
        }
        fn strip(value: &Value) -> Value {
            match value {
                Value::Object(row) => Value::Object(
                    row.iter()
                        .filter(|(key, _)| *key != "seconds" && !key.starts_with("examples_"))
                        .map(|(key, value)| (key.clone(), strip(value)))
                        .collect(),
                ),
                Value::Array(rows) => Value::Array(rows.iter().map(strip).collect()),
                other => other.clone(),
            }
        }
        Some(strip(&block))
    };
    match (same_package(committed), same_package(fresh)) {
        (Some(before), Some(after)) => {
            if before != after {
                failures.push(format!(
                    "java/same_package: committed {} but the run produced {}",
                    before["scoped"]
                        .as_object()
                        .map(|row| format!(
                            "scoped {} claimed, {} false positive, recall {} (budget {})",
                            row["edges_claimed"],
                            row["false_positive"],
                            row["recall"],
                            before["budget"]["max_false_positive_rate"]
                        ))
                        .unwrap_or_default(),
                    after["scoped"]
                        .as_object()
                        .map(|row| format!(
                            "scoped {} claimed, {} false positive, recall {}",
                            row["edges_claimed"], row["false_positive"], row["recall"]
                        ))
                        .unwrap_or_default()
                ));
            }
        }
        (None, Some(_)) | (Some(_), None) => {
            failures.push("java/same_package: present in one report and not the other".to_string());
        }
        (None, None) => {}
    }
    for key in ["thresholds", "policy", "schema_version_check"] {
        if key == "schema_version_check" {
            let before = committed["ticket"]["schema_version"].clone();
            let after = fresh["ticket"]["schema_version"].clone();
            if before != after {
                failures.push(format!(
                    "structure::SCHEMA_VERSION moved: committed {before}, run {after}"
                ));
            }
            continue;
        }
        if committed[key] != fresh[key] {
            failures.push(format!("{key} differs from the committed report"));
        }
    }
    failures
}

fn row(score: &Score) -> Value {
    json!({
        "true_positive": score.tp,
        "false_positive": score.fp,
        "missed": score.miss,
        "precision": round(score.precision()),
        "recall": round(score.recall()),
        "f1": round(score.f1()),
    })
}

fn round(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

fn compact(value: &Value) -> String {
    let get = |key: &str| value[key].as_u64().unwrap_or(0);
    format!(
        "P{}/R{} ({}+ {}- {}m)",
        value["precision"].as_f64().unwrap_or(0.0),
        value["recall"].as_f64().unwrap_or(0.0),
        get("true_positive"),
        get("false_positive"),
        get("missed")
    )
}

fn print_one(language: &str, score: &LangScore, mechanisms: &Value, verdict: &str) {
    let fmt = |row: &Score| {
        format!(
            "P {:.3} R {:.3} ({} ok, {} wrong, {} missed)",
            row.precision(),
            row.recall(),
            row.tp,
            row.fp,
            row.miss
        )
    };
    println!("── {language} [{verdict}]");
    println!("   definitions  {}", fmt(&score.definitions));
    println!("   exports      {}", fmt(&score.exports));
    println!("   imports      {}", fmt(&score.imports));
    println!("   supertypes   {}", fmt(&score.supertypes));
    for miss in score.missing_defs.iter().take(12) {
        println!("     missed def      {miss}");
    }
    for extra in score.extra_defs.iter().take(12) {
        println!("     surprise def      {extra}");
    }
    for miss in score.missing_exports.iter().take(12) {
        println!("     missed export     {miss}");
    }
    for extra in score.extra_exports.iter().take(12) {
        println!("     surprise export   {extra}");
    }
    for miss in score.missing_edges.iter().take(12) {
        println!("     missed edge       {miss}");
    }
    for extra in score.extra_edges.iter().take(12) {
        println!("     surprise edge     {extra}");
    }
    for miss in score.missing_bases.iter().take(12) {
        println!("     missed base       {miss}");
    }
    for extra in score.extra_bases.iter().take(12) {
        println!("     surprise base     {extra}");
    }
    println!(
        "   blind spots: {} categories, {} rows, {} found anyway",
        score.blind.len(),
        score.irreducible_rows.len(),
        score.blind_recovered,
    );
    for (kind, count) in &score.blind {
        println!("     {kind} ({count})");
    }
    println!(
        "   synthetic: {}/{} scopes marked, {} beyond the annotation, {} asserting emptiness",
        mechanisms["synthetic_eligible"]["found"]
            .as_u64()
            .unwrap_or(0),
        mechanisms["synthetic_eligible"]["expected"]
            .as_u64()
            .unwrap_or(0),
        mechanisms["synthetic_eligible"]["marked_beyond_the_annotation"]
            .as_u64()
            .unwrap_or(0),
        mechanisms["synthetic_eligible"]["scopes_that_assert_emptiness"]
            .as_array()
            .map(|rows| rows.len())
            .unwrap_or(0),
    );
    for miss in mechanisms["synthetic_eligible"]["scopes_that_assert_emptiness"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .take(6)
    {
        println!("     ASSERTS EMPTY   {miss}");
    }
    println!(
        "   wildcards:   {}/{} found, {} mislabelled; who-depends-on unknown: {:?}",
        mechanisms["wildcard_edges"]["found"].as_u64().unwrap_or(0),
        mechanisms["wildcard_edges"]["expected"]
            .as_u64()
            .unwrap_or(0),
        mechanisms["wildcard_edges"]["mislabelled_as_resolved"]
            .as_array()
            .map(|rows| rows.len())
            .unwrap_or(0),
        mechanisms["wildcard_edges"]["who_depends_on"]["says_unknown"],
    );
    println!(
        "   resolution:  bound {}, used {}, non-converged {}, truncated chains {}",
        mechanisms["resolution"]["bound_passes"]
            .as_u64()
            .unwrap_or(0),
        mechanisms["resolution"]["passes_used_max"]
            .as_u64()
            .unwrap_or(0),
        mechanisms["resolution"]["non_converged"]
            .as_array()
            .map(|rows| rows.len())
            .unwrap_or(0),
        mechanisms["resolution"]["truncated_chains"]
            .as_u64()
            .unwrap_or(0),
    );
}

fn grammar_row(language: &str, lock: &str) -> Value {
    let crate_name = format!("tree-sitter-{language}");
    let mut row = json!({
        "language": language,
        "crate": crate_name,
        "version": null,
        "checksum": null,
    });
    let needle = format!("name = \"{crate_name}\"\n");
    if let Some(at) = lock.find(&needle) {
        let tail = &lock[at..];
        let version = tail
            .lines()
            .nth(1)
            .and_then(|line| {
                line.trim()
                    .strip_prefix("version = ")
                    .map(|value| value.trim_matches('"').to_string())
            })
            .unwrap_or_default();
        let checksum = tail
            .lines()
            .find(|line| line.trim().starts_with("checksum = "))
            .and_then(|line| line.trim().strip_prefix("checksum = "))
            .map(|value| value.trim_matches('"').to_string())
            .unwrap_or_default();
        row["version"] = json!(version);
        row["checksum"] = json!(checksum);
    }
    row
}

fn rustc_version() -> String {
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn language_matches(language: &str, path: &str) -> bool {
    Language::detect(path).as_str() == language
}

/// Every corpus file of a language, corpus-relative and `/`-separated.
fn corpus_files(dir: &Path, extensions: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = path
                .file_name()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_default();
            if name == "truth.json" {
                continue;
            }
            let relative = path
                .strip_prefix(dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if extensions
                .iter()
                .any(|extension| relative.ends_with(&format!(".{extension}")))
            {
                out.push(relative);
            }
        }
    }
    out.sort();
    out
}

// ---------------------------------------------------------------------------
// debugging affordances
// ---------------------------------------------------------------------------

fn load(
    args: &Args,
    language: &str,
) -> Result<(PathBuf, Vec<String>, Vec<u8>, ExtractPolicy), String> {
    let (policy, _) = args.policy()?;
    let (_, extensions) = LANGUAGES
        .iter()
        .find(|(name, _)| *name == language)
        .ok_or_else(|| format!("no language named {language}"))?;
    let dir = args.corpus.join(language);
    let files = corpus_files(&dir, extensions);
    let mut joined = Vec::new();
    for path in &files {
        joined.extend_from_slice(&std::fs::read(dir.join(path)).unwrap_or_default());
    }
    Ok((dir, files, joined, policy))
}

fn dump(args: &Args, language: &str) -> Result<i32, String> {
    let (dir, files, _, policy) = load(args, language)?;
    let mut owned: Vec<(String, Vec<u8>)> = Vec::new();
    for path in &files {
        owned.push((
            path.clone(),
            std::fs::read(dir.join(path)).unwrap_or_default(),
        ));
    }
    let borrowed: Vec<SourceFile> = owned
        .iter()
        .map(|(path, body)| SourceFile::new(path, body))
        .collect();
    let corpus = structure::extract(&borrowed, &policy);
    for record in &corpus.files {
        println!("== {}", record.path);
        let Some(structure) = &record.structure else {
            println!(
                "   unavailable  {:?}",
                record.reason.as_ref().map(|reason| reason.as_str())
            );
            continue;
        };
        println!(
            "   defs        {}",
            structure
                .symbols
                .iter()
                .map(|symbol| format!("{}:{}", symbol.kind.as_str(), symbol.name))
                .collect::<Vec<_>>()
                .join(" ")
        );
        println!("   unit        {:?}", structure.package);
        println!("   exports     {:?}", structure.exports);
        println!(
            "   imports     {}",
            structure
                .imports
                .iter()
                .map(|import| format!("{}#{}[{}]", import.specifier, import.symbol, import.form))
                .collect::<Vec<_>>()
                .join(" ")
        );
        println!(
            "   edges       {}",
            structure
                .edges
                .iter()
                .map(|edge| format!(
                    "{}#{}<{}>{}",
                    edge.target,
                    edge.symbol,
                    edge.kind.as_str(),
                    edge.via
                        .as_ref()
                        .map(|via| format!(" via {via}"))
                        .unwrap_or_default()
                ))
                .collect::<Vec<_>>()
                .join(" ")
        );
        println!(
            "   bases       {}",
            structure
                .supertypes
                .iter()
                .map(|base| format!("{} {}-> {}", base.of, base.kind.as_str(), base.base))
                .collect::<Vec<_>>()
                .join(" ")
        );
        println!(
            "   synthetic   {}",
            structure
                .synthetic_scopes
                .iter()
                .map(|scope| format!(
                    "{}[{}]",
                    if scope.scope.is_empty() {
                        "(file)"
                    } else {
                        &scope.scope
                    },
                    scope.reasons.join("+")
                ))
                .collect::<Vec<_>>()
                .join(" ")
        );
        println!(
            "   resolution  used {} converged {} bound {} truncated {}",
            structure.resolution.passes_used,
            structure.resolution.converged,
            structure.resolution.bound,
            structure.resolution.truncated_chains
        );
        if let Some(sentence) = structure.may_have_shrunk() {
            println!("   shrunk?     {sentence}");
        }
        if !structure.notes.is_empty() {
            println!(
                "   notes       {}",
                structure
                    .notes
                    .iter()
                    .map(|note| format!("{}:{}", note.kind, note.detail))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
    }
    Ok(0)
}

fn probe(args: &Args, language: &str) -> Result<i32, String> {
    let (dir, files, _, _) = load(args, language)?;
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for path in &files {
        let text = std::fs::read(dir.join(path)).unwrap_or_default();
        let record = structure::extract_one(path, &text, &ExtractPolicy::default());
        let Some(structure) = &record.structure else {
            println!("{}: UNAVAILABLE", record.path);
            continue;
        };
        println!(
            "{}: symbols={} parse_errors={}",
            record.path,
            structure.symbols.len(),
            record.file_level.parse_errors
        );
        for symbol in &structure.symbols {
            *kinds.entry(symbol.kind.as_str().to_string()).or_insert(0) += 1;
        }
    }
    for (kind, count) in kinds {
        println!("{count}\t{kind}");
    }
    Ok(0)
}
