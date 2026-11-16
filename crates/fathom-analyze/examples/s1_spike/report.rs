//! Spike `s1` — assembling and diffing the report.

use crate::Args;
use serde_json::{Value, json};

pub fn build(
    _args: &Args,
    grammars: Vec<Value>,
    languages: Vec<Value>,
    throughput: Vec<Value>,
    thresholds: &Value,
) -> Value {
    let verdicts: Vec<(&str, String)> = languages
        .iter()
        .map(|l| {
            (
                l["language"].as_str().unwrap_or("?"),
                l["verdict"].as_str().unwrap_or("?").to_string(),
            )
        })
        .collect();
    let pick = |want: &str| -> Vec<String> {
        verdicts
            .iter()
            .filter(|(_, v)| v == want)
            .map(|(l, _)| (*l).to_string())
            .collect()
    };
    json!({
        "spike": {
            "id": "reco-alj",
            "plan_key": "s1",
            "title": "tree-sitter extraction accuracy across six languages",
            "harness": "crates/fathom-analyze/examples/s1_spike.rs",
            "corpus": "crates/fathom-analyze/spike/s1/corpus",
            "thresholds": "crates/fathom-analyze/spike/s1/thresholds.json",
            "reproduce": "cargo run -p fathom-analyze --features langs-all --example s1_spike -- --repo . --report benchmarks/reports/s1-tree-sitter-accuracy.json",
        },
        "environment": {
            "rustc": rustc_version(),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
            "what_these_numbers_are": "Definitions, exports and import edges are exact set comparisons against hand-authored truth, so they carry over to `fathom-analyze` only as well as the spike's extractors do — they are an upper bound on what the grammar can be made to say, not a promise about `e3.1`'s implementation. Throughput is one thread, one process, debug profile unless stated: a floor, not a prediction.",
        },
        "grammars": grammars,
        "thresholds": thresholds,
        "languages": languages,
        "throughput": throughput,
        "verdicts": verdicts
            .iter()
            .map(|(lang, verdict)| (lang.to_string(), json!(verdict)))
            .collect::<serde_json::Map<String, Value>>(),
        "recommendation": {
            "go": pick("go"),
            "go_with_conditions": pick("go_with_conditions"),
            "no_go": pick("no_go"),
        }
    })
}

/// Everything that must be identical for the committed report to still be true. Deliberately no
/// throughput comparison: wall-clock on a different runner is not a contract.
pub fn compare(committed: &Value, fresh: &Value) -> Vec<String> {
    let mut failures = Vec::new();
    let rows = |report: &Value| -> Vec<(String, String, String, Value)> {
        report["languages"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .flat_map(|row| {
                        let lang = row["language"].as_str().unwrap_or("?").to_string();
                        ["definitions", "exports", "imports"]
                            .into_iter()
                            .map(move |rel| {
                                (
                                    lang.clone(),
                                    rel.to_string(),
                                    "counts".to_string(),
                                    row["relations"][rel].clone(),
                                )
                            })
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let before = rows(committed);
    let after = rows(fresh);
    for (lang, relation, _, expected) in &before {
        let Some((_, _, _, actual)) = after.iter().find(|(l, r, _, _)| l == lang && r == relation)
        else {
            failures.push(format!("{lang}/{relation}: missing from the fresh run"));
            continue;
        };
        if expected != actual {
            failures.push(format!(
                "{lang}/{relation}: committed {} but the run produced {}",
                compact(expected),
                compact(actual)
            ));
        }
    }
    let verdicts = |report: &Value| -> Vec<String> {
        report["languages"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .map(|r| format!("{}={}", r["language"], r["verdict"]))
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
    let key = "thresholds";
    if committed[key] != fresh[key] {
        failures.push(format!("{key} differs from the committed report"));
    }
    failures
}

fn compact(value: &Value) -> String {
    let get = |k: &str| value[k].as_u64().unwrap_or(0);
    format!(
        "P{}/R{} ({}+ {}- {}m)",
        value["precision"].as_f64().unwrap_or(0.0),
        value["recall"].as_f64().unwrap_or(0.0),
        get("true_positive"),
        get("false_positive"),
        get("missed")
    )
}

fn rustc_version() -> String {
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}
