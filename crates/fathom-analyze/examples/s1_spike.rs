//! Spike `s1` (`reco-alj`) — how much of a codebase can tree-sitter actually tell `e3.1`?
//!
//! **This is a spike, not a library.** Six throwaway extractors, one per language, scored against
//! a hand-annotated corpus, and a report. Nothing here is the design `e3.1` will ship; the point
//! of writing them was to find out where each grammar lies to you, at what cost, and what the
//! fallback has to be. The written answer is `docs/concepts/structure-extraction.md`.
//!
//! ```text
//! cargo run -p fathom-analyze --features langs-all --example s1_spike -- \
//!     --repo . --report benchmarks/reports/s1-tree-sitter-accuracy.json
//! cargo run -p fathom-analyze --features langs-all --example s1_spike -- --repo . --check
//! cargo run -p fathom-analyze --features langs-all --example s1_spike -- --probe rust
//! ```
//!
//! `--check` is the gate CI runs: it re-measures and fails if the committed report's counts are
//! not what the corpus and the extractors produce, which is what stops the prose from drifting
//! from the measurement.

// Throwaway code keeps spare parts around: helpers written while probing a grammar that the final
// extractor did not need, and the kind vocabulary the report documents. Deleting them would make
// the spike harder to re-run, not easier, so the lint is off for this example alone. And a spike
// reports to a human, so stdout is its interface: `--check`'s verdict line is the thing CI reads.
#![allow(dead_code, missing_docs, clippy::print_stdout, clippy::print_stderr)]

// An example's own directory is not a module path rustc will guess, so the paths are spelled.
#[path = "s1_spike/common.rs"]
mod common;
#[path = "s1_spike/golang.rs"]
mod golang;
#[path = "s1_spike/javalang.rs"]
mod javalang;
#[path = "s1_spike/model.rs"]
mod model;
#[path = "s1_spike/python.rs"]
mod python;
#[path = "s1_spike/report.rs"]
mod report;
#[path = "s1_spike/ruby.rs"]
mod ruby;
#[path = "s1_spike/rustlang.rs"]
mod rustlang;
#[path = "s1_spike/ts.rs"]
mod ts;

use model::{Edge, Index, LangScore, Scan, Score, Truth};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Instant;
use tree_sitter::Parser;

/// What a language's export rule needs: every file's scan, every file's resolved edges, and the
/// corpus index. Named because the alternative was a type that took a paragraph to read.
type Exports = fn(&Scans, &Edges, &Index) -> BTreeMap<String, BTreeSet<String>>;
/// Per-file scans and per-file edge sets, keyed by corpus-relative path.
type Scans = BTreeMap<String, Scan>;
type Edges = BTreeMap<String, BTreeSet<Edge>>;
/// What one file produced: its declarations, its exported names, its resolved dependencies.
type PerFile = (BTreeSet<model::Def>, BTreeSet<String>, BTreeSet<Edge>);

/// A language, as far as the driver is concerned: a grammar, a scanner, a resolver, an export rule.
struct Lang {
    name: &'static str,
    beads_of_grammar: &'static str,
    exts: &'static [&'static str],
    language: fn() -> tree_sitter::Language,
    scan: fn(tree_sitter::Node, &[u8], &str, &mut Scan),
    resolve: fn(&str, &[model::RawImport], &mut Index) -> BTreeSet<Edge>,
    surface: fn(&str, &Scan, &BTreeSet<Edge>, &mut Index),
    same_package: fn(&str, &Scan, &Index) -> BTreeSet<Edge>,
    exports: Exports,
}

const LANGS: [Lang; 6] = [
    Lang {
        name: "rust",
        beads_of_grammar: "tree-sitter-rust",
        exts: &["rs"],
        language: rustlang::language,
        scan: rustlang::scan,
        resolve: rustlang::resolve,
        surface: rustlang::surface,
        same_package: rustlang::same_package,
        exports: rustlang::exports,
    },
    Lang {
        name: "python",
        beads_of_grammar: "tree-sitter-python",
        exts: &["py"],
        language: python::language,
        scan: python::scan,
        resolve: python::resolve,
        surface: python::surface,
        same_package: python::same_package,
        exports: python::exports,
    },
    Lang {
        name: "typescript",
        beads_of_grammar: "tree-sitter-typescript",
        exts: &["ts"],
        language: ts::language,
        scan: ts::scan,
        resolve: ts::resolve,
        surface: ts::surface,
        same_package: ts::same_package,
        exports: ts::exports,
    },
    Lang {
        name: "go",
        beads_of_grammar: "tree-sitter-go",
        exts: &["go"],
        language: golang::language,
        scan: golang::scan,
        resolve: golang::resolve,
        surface: golang::surface,
        same_package: golang::same_package,
        exports: golang::exports,
    },
    Lang {
        name: "ruby",
        beads_of_grammar: "tree-sitter-ruby",
        exts: &["rb"],
        language: ruby::language,
        scan: ruby::scan,
        resolve: ruby::resolve,
        surface: ruby::surface,
        same_package: ruby::same_package,
        exports: ruby::exports,
    },
    Lang {
        name: "java",
        beads_of_grammar: "tree-sitter-java",
        exts: &["java"],
        language: javalang::language,
        scan: javalang::scan,
        resolve: javalang::resolve,
        surface: javalang::surface,
        same_package: javalang::same_package,
        exports: javalang::exports,
    },
];

pub struct Args {
    pub repo: PathBuf,
    #[allow(dead_code)]
    pub corpus: PathBuf,
    pub thresholds: PathBuf,
    pub report: Option<PathBuf>,
    pub write: bool,
    pub check: bool,
    pub probe: Option<String>,
    pub dump: Option<String>,
    pub sexp: Option<String>,
    pub sexp_file: Option<String>,
    pub quiet: bool,
}

fn main() {
    let args = match Args::parse() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("s1 spike: {message}");
            std::process::exit(2);
        }
    };
    match run(&args) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("s1 spike: {error}");
            std::process::exit(1);
        }
    }
}

impl Args {
    fn parse() -> Result<Args, String> {
        let mut out = Args {
            repo: PathBuf::from("."),
            corpus: PathBuf::from("crates/fathom-analyze/spike/s1/corpus"),
            thresholds: PathBuf::from("crates/fathom-analyze/spike/s1/thresholds.json"),
            report: None,
            write: false,
            check: false,
            probe: None,
            dump: None,
            sexp: None,
            sexp_file: None,
            quiet: false,
        };
        let argv: Vec<String> = std::env::args().skip(1).collect();
        let mut i = 0;
        while i < argv.len() {
            let flag = argv[i].as_str();
            let take = |i: &mut usize| -> Result<String, String> {
                *i += 1;
                argv.get(*i)
                    .cloned()
                    .ok_or_else(|| format!("{flag} needs a value"))
            };
            match flag {
                "--repo" => out.repo = PathBuf::from(take(&mut i)?),
                "--corpus" => out.corpus = PathBuf::from(take(&mut i)?),
                "--thresholds" => out.thresholds = PathBuf::from(take(&mut i)?),
                "--report" => {
                    out.report = Some(PathBuf::from(take(&mut i)?));
                    out.write = true;
                }
                "--check" => out.check = true,
                "--probe" => out.probe = Some(take(&mut i)?),
                "--dump" => out.dump = Some(take(&mut i)?),
                "--sexp" => {
                    out.sexp = Some(take(&mut i)?);
                    out.sexp_file = Some(take(&mut i)?);
                }
                "--quiet" => out.quiet = true,
                other => return Err(format!("unknown argument {other}")),
            }
            i += 1;
        }
        if out.check {
            // The audit must not be able to fix the thing it is auditing.
            out.write = false;
        }
        if out.report.is_none()
            && !out.check
            && out.probe.is_none()
            && out.dump.is_none()
            && out.sexp.is_none()
        {
            return Err("pass --report PATH, --check, or --probe LANG".to_string());
        }
        Ok(out)
    }
}

fn run(args: &Args) -> Result<i32, String> {
    if let Some(lang) = &args.probe {
        return probe(args, lang);
    }
    if let Some(lang) = &args.dump {
        return dump(args, lang);
    }
    if let (Some(lang), Some(file)) = (&args.sexp, &args.sexp_file) {
        return sexp(args, lang, file);
    }
    let thresholds: Value = read_json(&args.thresholds)?;
    let mut languages = Vec::new();
    let mut throughput = Vec::new();
    let mut grammars = Vec::new();
    let lock = std::fs::read_to_string(args.repo.join("Cargo.lock")).unwrap_or_default();

    for lang in LANGS.iter() {
        let dir = args.corpus.join(lang.name);
        if !dir.is_dir() {
            return Err(format!("no corpus for {} at {}", lang.name, dir.display()));
        }
        let truth: Truth = read_json(&dir.join("truth.json"))?;
        let files = corpus_files(&dir, lang.exts);
        let mut parser = Parser::new();
        let language = (lang.language)();
        parser
            .set_language(&language)
            .map_err(|e| format!("{}: {e}", lang.name))?;
        grammars.push(grammar_row(lang, &language, &lock));

        // Pass 1: parse and scan everything, because resolution needs the whole corpus.
        let mut scans: BTreeMap<String, Scan> = BTreeMap::new();
        let mut index = Index::default();
        let mut sources: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        for path in &files {
            let text = std::fs::read(dir.join(path)).map_err(|e| format!("{path}: {e}"))?;
            let root = parser
                .parse(&text, None)
                .ok_or_else(|| format!("{path}: parse failed"))?;
            let mut scan = Scan::default();
            (lang.scan)(root.root_node(), &text, path, &mut scan);
            index.files.insert(path.clone());
            index.defs.insert(
                path.clone(),
                scan.defs.iter().map(|d| d.name.clone()).collect(),
            );
            if let Some(pkg) = scan.aux.get("package") {
                index.extra.insert(format!("pkg:{path}"), pkg.clone());
            }
            sources.insert(path.clone(), text);
            scans.insert(path.clone(), scan);
        }
        if lang.name == "go"
            && let Ok(manifest) = std::fs::read_to_string(dir.join("go.mod"))
        {
            let module = manifest
                .lines()
                .find_map(|l| l.trim().strip_prefix("module ").map(String::from))
                .unwrap_or_default();
            index.extra.insert("module".into(), module);
        }

        // Pass 2: resolve, repeatedly, because a re-export can only be followed once the file
        // that owns it has been resolved. Four rounds covers any chain the corpus contains.
        let mut edges: BTreeMap<String, BTreeSet<Edge>> = BTreeMap::new();
        for _ in 0..4 {
            for (path, scan) in scans.iter() {
                let mut set = (lang.resolve)(path, &scan.imports, &mut index);
                set.extend((lang.same_package)(path, scan, &index));
                (lang.surface)(path, scan, &set, &mut index);
                // A module that `use super::*` or `from . import x` its own file does not depend
                // on itself; self-edges are dropped so nothing downstream trips over them.
                set.retain(|e| e.target != *path);
                edges.insert(path.clone(), set);
            }
        }

        let exports = (lang.exports)(&scans, &edges, &index);
        let mut got: BTreeMap<String, PerFile> = BTreeMap::new();
        let mut blind: BTreeMap<String, usize> = BTreeMap::new();
        for (path, scan) in scans.iter() {
            for note in &scan.blind {
                *blind
                    .entry(note.split(':').next().unwrap_or(note).to_string())
                    .or_insert(0) += 1;
            }
            got.insert(
                path.clone(),
                (
                    scan.defs.iter().cloned().collect(),
                    exports.get(path).cloned().unwrap_or_default(),
                    edges.get(path).cloned().unwrap_or_default(),
                ),
            );
        }

        let mut score = model::score(&truth, &got);
        score.corpus_files = files.len();
        score.corpus_bytes = sources.values().map(|s| s.len()).sum();
        score.blind = blind;
        let rows = truth_rows(&truth);
        // Cost is part of the verdict, not a footnote: a grammar that is accurate and twenty times
        // slower than its neighbours is a feature gate decision (ADR-002 §6.5), not a shrug. The
        // corpus that represents the language is real files when the host repository has any,
        // otherwise the replica.
        let run = bench(lang, &dir, &files, &args.repo)?;
        let floor = thresholds
            .get("throughput")
            .and_then(|t| t.get("parse_mb_per_second_floor"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let (cost_corpus, cost) = match (run.get("repository"), run.get("replica")) {
            (Some(repo), _) => ("repository", Some(repo)),
            (None, Some(replica)) => ("replica", Some(replica)),
            _ => ("fixture", run.get("fixture")),
        };
        let parse_rate = cost
            .and_then(|c| c.get("parse_mib_per_second"))
            .and_then(Value::as_f64)
            .unwrap_or_default();
        // A debug build measures the allocator and the bounds checks, not the grammar, so the
        // cost floor is only enforced where the number means something. `--check` runs in debug
        // precisely so CI can be fast; it must not fail a floor it cannot measure.
        let throughput_ok = parse_rate >= floor || cfg!(debug_assertions);
        let mut verdict = verdict_of(lang.name, &score, &thresholds);
        if verdict == "go" && !throughput_ok {
            verdict = "go_with_conditions";
        }
        languages.push(json!({
            "language": lang.name,
            "grammar_crate": lang.beads_of_grammar,
            "corpus": {
                "files": score.corpus_files,
                "bytes": score.corpus_bytes,
                "definitions": rows.0,
                "exports": rows.1,
                "import_edges": rows.2,
            },
            "relations": {
                "definitions": row(&score.definitions),
                "exports": row(&score.exports),
                "imports": row(&score.imports),
            },
            "tags": score.tags.iter().map(|(tag, s)| json!({
                "tag": tag, "expected": s.tp + s.miss, "found": s.tp, "recall": round(s.recall()),
            })).collect::<Vec<_>>(),
            "missing_definitions": score.missing_defs,
            "surprising_definitions": score.extra_defs,
            "missing_exports": score.missing_exports,
            "surprising_exports": score.extra_exports,
            "missing_edges": score.missing_edges,
            "surprising_edges": score.extra_edges,
            "blind_spots": score.blind.iter().map(|(k, v)| json!({"kind": k, "count": v})).collect::<Vec<_>>(),
            "throughput_check": {
                "floor_mib_per_second": floor,
                "measured_mib_per_second": round(parse_rate),
                "measured_over": cost_corpus,
                "enforced": !cfg!(debug_assertions),
                "ok": throughput_ok,
            },
            "verdict": verdict,
        }));

        throughput.push(run);
        if !args.quiet {
            print_one(lang.name, &score, verdict);
        }
    }

    let report = report::build(args, grammars, languages, throughput, &thresholds);
    let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    let mut code = 0;
    if args.check {
        let Some(path) = &args.report else {
            return Err("--check needs --report to compare against".to_string());
        };
        let committed: Value = read_json(path)?;
        let failures = report::compare(&committed, &report);
        if failures.is_empty() {
            println!(
                "spike s1: the committed report reproduces exactly ({})",
                path.display()
            );
        } else {
            code = 1;
            println!(
                "spike s1: the committed report does NOT reproduce ({}):",
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
    if !args.quiet && !args.write && !args.check {
        println!("{text}");
    }
    Ok(code)
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn truth_rows(truth: &Truth) -> (usize, usize, usize) {
    let mut defs = 0;
    let mut exports = 0;
    let mut imports = 0;
    for file in truth.files.values() {
        defs += file.definitions.len();
        exports += file.exports.len();
        imports += file.imports.len();
    }
    (defs, exports, imports)
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

/// `go`, `go_with_conditions`, or `no_go`, decided by the thresholds file and nothing else: the
/// verdict has to be a function of the measurement, or it is an opinion with a table attached.
fn verdict_of(lang: &str, score: &LangScore, thresholds: &Value) -> &'static str {
    let rules = thresholds
        .get("relations")
        .and_then(|r| r.get("default"))
        .cloned()
        .unwrap_or(json!({}));
    let per = thresholds
        .get("relations")
        .and_then(|r| r.get(lang))
        .cloned();
    let mut worst = "go";
    for relation in ["definitions", "exports", "imports"] {
        let rule = per
            .as_ref()
            .and_then(|p| p.get(relation))
            .or_else(|| rules.get(relation));
        let Some(rule) = rule else { continue };
        let score = match relation {
            "definitions" => &score.definitions,
            "exports" => &score.exports,
            _ => &score.imports,
        };
        let min_recall = rule.get("recall").and_then(Value::as_f64).unwrap_or(1.0);
        let min_precision = rule.get("precision").and_then(Value::as_f64).unwrap_or(1.0);
        let ok = score.recall() >= min_recall && score.precision() >= min_precision;
        if !ok {
            let hard = rule.get("hard").and_then(Value::as_bool).unwrap_or(false);
            worst = if hard { "no_go" } else { "go_with_conditions" };
            break;
        }
    }
    // Numbers clearing the floor is not the same as nothing to carry into the design. A clean
    // verdict requires having found *everything* the corpus contained: a language that passes its
    // floors while still missing declarations is a `go` with conditions, and the conditions are the
    // categories named in the thresholds file's `irreducible_blind_spots`.
    if worst == "go"
        && [&score.definitions, &score.exports, &score.imports]
            .iter()
            .any(|s| s.miss > 0 || s.fp > 0)
    {
        worst = "go_with_conditions";
    }
    // Second, weaker signal in the same direction: an extractor that recorded an irreducible blind
    // spot has told us the limitation is a property of the language, not of the spike.
    if worst == "go" {
        let irreducible = thresholds
            .get("irreducible_blind_spots")
            .and_then(|m| m.get(lang))
            .or_else(|| {
                thresholds
                    .get("irreducible_blind_spots")
                    .and_then(|m| m.get("default"))
            })
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if score.blind.keys().any(|kind| {
            irreducible
                .iter()
                .any(|k| k.as_str() == Some(kind.as_str()))
        }) {
            worst = "go_with_conditions";
        }
    }
    worst
}

fn print_one(name: &str, score: &LangScore, verdict: &str) {
    let fmt = |s: &Score| {
        format!(
            "P {:.3} R {:.3} ({} ok, {} wrong, {} missed)",
            s.precision(),
            s.recall(),
            s.tp,
            s.fp,
            s.miss
        )
    };
    println!("── {name} [{verdict}]");
    println!("   definitions  {}", fmt(&score.definitions));
    println!("   exports      {}", fmt(&score.exports));
    println!("   imports      {}", fmt(&score.imports));
    for miss in score.missing_defs.iter().take(12) {
        println!("     missed def   {miss}");
    }
    for extra in score.extra_defs.iter().take(12) {
        println!("     surprise def   {extra}");
    }
    for miss in score.missing_edges.iter().take(12) {
        println!("     missed edge  {miss}");
    }
    for extra in score.extra_edges.iter().take(12) {
        println!("     surprise edge  {extra}");
    }
    for miss in score.missing_exports.iter().take(12) {
        println!("     missed export  {miss}");
    }
    for extra in score.extra_exports.iter().take(12) {
        println!("     surprise export {extra}");
    }
    for extra in score.extra_edges.iter().take(12) {
        println!("     surprise edge  {extra}");
    }
}

/// Every corpus file of a language, corpus-relative, `/` separated.
fn corpus_files(dir: &Path, exts: &[&str]) -> Vec<String> {
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
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            if name == "truth.json" || name == "go.mod" {
                continue;
            }
            let rel = path
                .strip_prefix(dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if exts.iter().any(|ext| rel.ends_with(&format!(".{ext}"))) {
                out.push(rel);
            }
        }
    }
    out.sort();
    out
}

/// Grammar provenance: the crate the lockfile pins, and the ABI the parser reports at run time.
fn grammar_row(lang: &Lang, language: &tree_sitter::Language, lock: &str) -> Value {
    let mut row = json!({
        "language": lang.name,
        "crate": lang.beads_of_grammar,
        "version": null,
        "checksum": null,
        "grammar_name": language.name().unwrap_or("?").to_string(),
        "grammar_abi": language.abi_version(),
    });
    let needle = format!("name = \"{}\"\n", lang.beads_of_grammar);
    if let Some(at) = lock.find(&needle) {
        let tail = &lock[at..];
        let version = tail
            .lines()
            .nth(1)
            .and_then(|l| {
                l.trim()
                    .strip_prefix("version = ")
                    .map(|s| s.trim_matches('"').to_string())
            })
            .unwrap_or_default();
        let checksum = tail
            .lines()
            .find(|l| l.trim().starts_with("checksum = "))
            .and_then(|l| l.trim().strip_prefix("checksum = "))
            .map(|s| s.trim_matches('"').to_string())
            .unwrap_or_default();
        row["version"] = json!(version);
        row["checksum"] = json!(checksum);
    }
    row
}

// ---------------------------------------------------------------------------
// throughput
// ---------------------------------------------------------------------------

/// One timed corpus: the fixtures, a byte-varied replica of them, and — where the repository has
/// real code in the language — the repository itself.
fn bench(lang: &Lang, dir: &Path, files: &[String], repo: &Path) -> Result<Value, String> {
    let mut parser = Parser::new();
    parser
        .set_language(&(lang.language)())
        .map_err(|e| format!("{}: {e}", lang.name))?;
    let mut out = json!({ "language": lang.name });
    let mut corpora = Vec::new();
    let mut fixture_bytes = Vec::new();
    for path in files {
        fixture_bytes.push((
            path.clone(),
            std::fs::read(dir.join(path)).unwrap_or_default(),
        ));
    }
    corpora.push(("fixture", fixture_bytes.clone()));
    let mut replica = Vec::new();
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut bytes = 0usize;
    while bytes < 2_000_000 && replica.len() < 4000 {
        for (path, body) in &fixture_bytes {
            let mut pad = String::new();
            let mut n = 1 + (next(&mut seed) % 40) as usize;
            while n > 0 {
                pad.push_str(&format!("// replica {:x}\n", next(&mut seed) % 4096));
                n -= 1;
            }
            let mut text = pad.into_bytes();
            text.extend_from_slice(body);
            bytes += text.len();
            replica.push((format!("{path}.{}", replica.len()), text));
        }
    }
    corpora.push(("replica", replica));
    if lang.name == "rust" || lang.name == "python" {
        let skip: &[&str] = &["target", ".git", "node_modules", ".venv", "spike"];
        let mut real = Vec::new();
        let ext = lang.exts[0];
        let mut stack = vec![repo.to_path_buf()];
        while let Some(current) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&current) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let name = path
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if path.is_dir() {
                    if !skip.iter().any(|s| name == *s) {
                        stack.push(path);
                    }
                } else if name.ends_with(&format!(".{ext}"))
                    && let Ok(body) = std::fs::read(&path)
                {
                    real.push((name, body));
                }
            }
        }
        if real.len() > 20 {
            corpora.push(("repository", real));
        }
    }

    for (label, corpus) in &corpora {
        let total: usize = corpus.iter().map(|(_, b)| b.len()).sum();
        if total == 0 {
            continue;
        }
        let mut parse_only = Vec::new();
        let mut parse_and_scan = Vec::new();
        let rss_before = rss_kib();
        for (_, body) in corpus {
            let start = Instant::now();
            let Some(tree) = parser.parse(body, None) else {
                continue;
            };
            parse_only.push(start.elapsed().as_secs_f64());
            let start = Instant::now();
            let mut scan = Scan::default();
            (lang.scan)(tree.root_node(), body, "x", &mut scan);
            parse_and_scan.push(start.elapsed().as_secs_f64());
        }
        let rss_after = rss_kib();
        let sum = |v: &Vec<f64>| v.iter().sum::<f64>();
        out[*label] = json!({
            "files": corpus.len(),
            "bytes": total,
            "parse_mib_per_second": round(total as f64 / 1_048_576.0 / sum(&parse_only).max(1e-9)),
            "extract_mib_per_second": round(total as f64 / 1_048_576.0 / sum(&parse_and_scan).max(1e-9)),
            "per_file_us_p50": round(percentile(&parse_only, 0.50) * 1e6),
            "per_file_us_p95": round(percentile(&parse_only, 0.95) * 1e6),
            "per_file_us_max": round(parse_only.iter().cloned().fold(0.0f64, f64::max) * 1e6),
            "rss_delta_kib": rss_after - rss_before,
        });
    }
    Ok(out)
}

fn next(state: &mut u64) -> u64 {
    // xorshift64*: deterministic, so a replica corpus is the same corpus next week.
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

fn percentile(values: &[f64], q: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let at = (((sorted.len() - 1) as f64) * q).round() as usize;
    sorted[at]
}

fn rss_kib() -> i64 {
    std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| {
            s.split_whitespace()
                .nth(1)
                .and_then(|v| v.parse::<i64>().ok())
        })
        .map(|pages| pages * 4)
        .unwrap_or(0)
}

/// The raw parse tree of one corpus file. Written because every "why did that not resolve"
/// question ended here anyway.
fn sexp(args: &Args, lang_name: &str, file: &str) -> Result<i32, String> {
    let lang = LANGS
        .iter()
        .find(|l| l.name == lang_name)
        .ok_or_else(|| format!("no language named {lang_name}"))?;
    let dir = args.corpus.join(lang.name);
    let mut parser = Parser::new();
    parser
        .set_language(&(lang.language)())
        .map_err(|e| e.to_string())?;
    let text = std::fs::read(dir.join(file)).map_err(|e| e.to_string())?;
    let root = parser
        .parse(&text, None)
        .ok_or_else(|| format!("{file}: parse failed"))?;
    println!("{}", root.root_node().to_sexp());
    Ok(0)
}

/// What the extractor said, file by file: definitions, exports, the imports as written and the
/// edges they resolved to. Nothing scores this; it exists so a reported miss can be looked at.
fn dump(args: &Args, lang_name: &str) -> Result<i32, String> {
    let lang = LANGS
        .iter()
        .find(|l| l.name == lang_name)
        .ok_or_else(|| format!("no language named {lang_name}"))?;
    let dir = args.corpus.join(lang.name);
    let mut parser = Parser::new();
    parser
        .set_language(&(lang.language)())
        .map_err(|e| e.to_string())?;
    let files = corpus_files(&dir, lang.exts);
    let mut scans: BTreeMap<String, Scan> = BTreeMap::new();
    let mut index = Index::default();
    for path in &files {
        let text = std::fs::read(dir.join(path)).map_err(|e| e.to_string())?;
        let root = parser
            .parse(&text, None)
            .ok_or_else(|| format!("{path}: parse failed"))?;
        let mut scan = Scan::default();
        (lang.scan)(root.root_node(), &text, path, &mut scan);
        index.files.insert(path.clone());
        index.defs.insert(
            path.clone(),
            scan.defs.iter().map(|d| d.name.clone()).collect(),
        );
        if let Some(pkg) = scan.aux.get("package") {
            index.extra.insert(format!("pkg:{path}"), pkg.clone());
        }
        scans.insert(path.clone(), scan);
    }
    if lang.name == "go"
        && let Ok(manifest) = std::fs::read_to_string(dir.join("go.mod"))
    {
        let module = manifest
            .lines()
            .find_map(|l| l.trim().strip_prefix("module ").map(String::from))
            .unwrap_or_default();
        index.extra.insert("module".into(), module);
    }
    let mut edges: BTreeMap<String, BTreeSet<Edge>> = BTreeMap::new();
    for _ in 0..4 {
        for (path, scan) in scans.iter() {
            let mut set = (lang.resolve)(path, &scan.imports, &mut index);
            set.extend((lang.same_package)(path, scan, &index));
            (lang.surface)(path, scan, &set, &mut index);
            edges.insert(path.clone(), set);
        }
    }
    let exports = (lang.exports)(&scans, &edges, &index);
    for (path, scan) in scans.iter() {
        println!("== {path}");
        println!(
            "   defs     {:?}",
            scan.defs
                .iter()
                .map(|d| format!("{}:{}", d.kind, d.name))
                .collect::<Vec<_>>()
        );
        println!(
            "   exports  {:?}",
            exports.get(path).cloned().unwrap_or_default()
        );
        println!(
            "   imports  {:?}",
            scan.imports
                .iter()
                .map(|i| format!("{}#{}[{}]", i.spec, i.symbol, i.form))
                .collect::<Vec<_>>()
        );
        println!(
            "   edges    {:?}",
            edges
                .get(path)
                .cloned()
                .unwrap_or_default()
                .iter()
                .map(|e| format!("{}#{}", e.target, e.symbol))
                .collect::<Vec<_>>()
        );
        if !scan.blind.is_empty() {
            println!("   blind    {:?}", scan.blind);
        }
    }
    Ok(0)
}

// ---------------------------------------------------------------------------
// --probe: what does the grammar actually call its nodes?
// ---------------------------------------------------------------------------

fn probe(args: &Args, lang_name: &str) -> Result<i32, String> {
    let lang = LANGS
        .iter()
        .find(|l| l.name == lang_name)
        .ok_or_else(|| format!("no language named {lang_name}"))?;
    let dir = args.corpus.join(lang.name);
    let mut parser = Parser::new();
    parser
        .set_language(&(lang.language)())
        .map_err(|e| e.to_string())?;
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for path in corpus_files(&dir, lang.exts) {
        let text = std::fs::read(dir.join(&path)).map_err(|e| e.to_string())?;
        let Some(tree) = parser.parse(&text, None) else {
            println!("{path}: PARSE FAILED");
            continue;
        };
        let root = tree.root_node();
        let mut errors = Vec::new();
        walk_kinds(root, &mut kinds, &mut errors);
        println!(
            "{path}: nodes={} errors={}",
            root.descendant_count(),
            errors.len()
        );
        for e in errors {
            println!("   ERROR {e}");
        }
    }
    for (kind, count) in kinds {
        println!("{count}\t{kind}");
    }
    Ok(0)
}

fn walk_kinds(
    node: tree_sitter::Node,
    kinds: &mut BTreeMap<String, usize>,
    errors: &mut Vec<String>,
) {
    if node.is_error() || node.is_missing() {
        errors.push(format!(
            "{}:{} {:?}",
            node.start_position().row + 1,
            node.start_position().column,
            node.kind()
        ));
    }
    if node.is_named() {
        *kinds.entry(node.kind().to_string()).or_insert(0) += 1;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_kinds(child, kinds, errors);
    }
}
