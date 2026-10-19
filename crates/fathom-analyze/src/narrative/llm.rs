//! `narrative::llm` — the seam spike `s5` measured, decided and did not wire (`reco-nv0`).
//!
//! `e4.7` as written in the plan floated an optional LLM polish behind `privacy.llm`, and ADR-002 §4.5
//! pre-funded it with a `net-llm` feature. Spike `s5` then built three arms over 20 real episodes from this
//! repository's own history, all three citing the same evidence set, and measured them
//! ([`docs/concepts/narrative-polish.md`](../../../../docs/concepts/narrative-polish.md)). The polished arm
//! bought readability by dropping figures — citation coverage 0.61 → 0.32 — scored *lower* on
//! trustworthiness (4.667 → 4.000), and re-sampled 0 of 6 times byte-identically. The decision recorded in
//! `reco-nv0` is: ship the template, ship nothing of the polish.
//!
//! # Why this module exists at all
//!
//! A decision that lives only in a markdown file is a decision that erodes. This module turns the three
//! halves of it into things a build can be asked about:
//!
//! * [`SHIPPED`] — `false`, printed by `fathom narrative --attribution` and by `fathom doctor --privacy`,
//!   so "was a model in the path of this artefact" is a query rather than a recollection;
//! * [`GATES`] — the five gates `reco-nv0` says must *all* be cleared before a polish path appears, in the
//!   order they were written;
//! * [`find_wired_paths`] — the mechanical half: given a crate's source, name every place that would put a
//!   model in the prose path. The `narrative` test target runs it over this crate's own `src/`, and a
//!   future `#[cfg(feature = "net-llm")]` consumer fails the build's contract rather than sliding in with a
//!   nice diff.
//!
//! The seam itself — `net-llm = ["dep:ureq"]` in `Cargo.toml` — stays declared, because ADR-002 §6.2
//! publishes the feature list and lint `F01` fails an unexplained removal. What must not exist is a
//! *consumer*, and that is what [`find_wired_paths`] looks for.

#![deny(missing_docs)]

/// Whether any part of the LLM narrative path ships. `false` is the decision, not a placeholder.
pub const SHIPPED: bool = false;

/// The feature that was reserved for the polish path and never given a consumer.
pub const RESERVED_FEATURE: &str = "net-llm";

/// The config key that was reserved for it and never given a surface.
pub const RESERVED_KEY: &str = "privacy.llm";

/// Why there is no polish, in one line, for a surface that has to say so without paraphrasing.
pub const REFUSAL: &str = "spike s5 measured the polish arm and it lost trustworthiness while halving \
                          citation coverage; the template ships alone (reco-nv0)";

/// The five gates `reco-nv0` says a polish arm must clear before it ships, in the order the decision
/// wrote them.
///
/// They are here rather than only in the decision text because a gate nobody can quote is a gate nobody
/// has to pass. `rejection_reasons` prints them with the current state of each.
pub const GATES: [&str; 5] = [
    "hallucinated claims at zero on >=100 episodes across >=3 repositories, including an arm prompted \
     for helpfulness instead of citation",
    "a measured win on trustworthiness as well as usefulness, from three human reviewers",
    "cost and latency measured against a real provider — bill and wall-clock — with a fallback path \
     byte-identical to the no-LLM path when the call fails, times out or is offline",
    "e5.7's byte-stable exports survive a polished narrative across reruns, or the guarantee is \
     dropped for polished episodes on purpose, in the open",
    "citation preservation enforced by the renderer, not instructed to the model",
];

/// What the measurement found, as the product states it.
#[must_use]
pub fn state() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "llm_narrative",
            if SHIPPED { "shipped" } else { "not shipped" },
        ),
        ("generator", "template"),
        ("reserved_feature", RESERVED_FEATURE),
        ("reserved_config_key", RESERVED_KEY),
        ("decided_by", "reco-nv0 (spike s5, reco-6p8)"),
        ("why", REFUSAL),
    ]
}

/// The gates, each with the state that gate is in right now.
///
/// `open` means the burden of proof has not been met; `not measured` means nobody has run the arm that
/// would meet it. Neither is `false` by neglect — the third and fourth are unreachable from a sandbox with
/// no provider, and saying which gate is waiting on a bill is the difference between a policy and an
/// excuse.
#[must_use]
pub fn rejection_reasons() -> Vec<(&'static str, &'static str)> {
    GATES
        .iter()
        .map(|gate| (*gate, gate_status(gate)))
        .collect()
}

fn gate_status(gate: &str) -> &'static str {
    if gate.starts_with("cost and latency") {
        "not measured — no provider endpoint was reachable from the sandbox, so the numbers in the spike \
         report are modelled"
    } else if gate.starts_with("hallucinated") {
        "open — measured on 20 episodes in one repository, from one disciplined arm"
    } else if gate.starts_with("a measured win") {
        "open — the panel was three rubric passes of one model, and trustworthiness moved the wrong way"
    } else {
        "open — nothing has been measured since the decision was taken"
    }
}

/// The HTTP clients whose use in this crate means an outbound call. Named one token at a time so this
/// file does not contain the call syntax it is looking for — a checker that flags its own table of
/// forbidden tokens is a checker that gets disabled.
pub const CLIENTS: [&str; 2] = ["ureq", "reqwest"];

/// One place in a source tree that would put a model in the prose path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WiredPath {
    /// The file, as the caller named it.
    pub file: String,
    /// The 1-based line.
    pub line: usize,
    /// The text that matched.
    pub text: String,
    /// What kind of wiring it is: `feature-gate` or `client-call`.
    pub kind: &'static str,
}

/// Find every line that would make an LLM call reachable from this crate.
///
/// Two shapes count, and both are named so a fix knows which way to go:
///
/// * a `cfg` naming the reserved feature — the seam would have a consumer again;
/// * a call into an HTTP client (`ureq`, `reqwest`) in a crate whose shipped binary is expected to make no
///   outbound call without an explicit `net-*` opt-in.
///
/// A doc comment is not a wiring, so lines that begin with a comment marker are skipped: the module docs
/// in this file discuss the feature they forbid, and a checker that flagged prose would be disabled within
/// a week.
#[must_use]
pub fn find_wired_paths(file: &str, source: &str) -> Vec<WiredPath> {
    let mut found = Vec::new();
    for (index, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//!")
            || trimmed.starts_with("///")
            || trimmed.starts_with("//")
            || trimmed.starts_with("//! ")
        {
            continue;
        }
        // The cfg spellings of the reserved feature, hyphen and underscore. Assembled rather than written
        // out, so that this file does not contain the pattern it is looking for — a checker that flags its
        // own table of forbidden tokens is a checker that gets disabled.
        let dashed = format!("feature = \"{RESERVED_FEATURE}\"");
        let underlined = format!("feature = \"{}\"", RESERVED_FEATURE.replace('-', "_"));
        let tight = format!("feature=\"{RESERVED_FEATURE}\"");
        if line.contains(&dashed) || line.contains(&underlined) || line.contains(&tight) {
            found.push(WiredPath {
                file: file.to_owned(),
                line: index + 1,
                text: trimmed.to_owned(),
                kind: "feature-gate",
            });
            continue;
        }
        for client in CLIENTS {
            if line.contains(client) && line.contains("::") {
                found.push(WiredPath {
                    file: file.to_owned(),
                    line: index + 1,
                    text: trimmed.to_owned(),
                    kind: "client-call",
                });
                break;
            }
        }
    }
    found
}
