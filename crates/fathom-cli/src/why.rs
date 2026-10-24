//! `fathom why <episode>` — the score, with its arithmetic turned inside out (`e4.4`, `reco-884`).
//!
//! The first acceptance criterion is a sentence: *"`fathom why <episode>` prints every feature, its
//! weight and its contribution to the total."* This file is the verb that says it, and it is a thin one:
//! the vector, the weights, the arithmetic and the renderer all live in
//! [`fathom_analyze::risk`], because the TUI's `w`, an exported artefact and an MCP result have to print
//! the same sentence and not three close translations of it (ADR-001 §7.10).
//!
//! # What a run does
//!
//! 1. resolve the handle — an episode id, a commit, a rev, an oid prefix — the same grammar `fathom show`
//!    and `fathom analyze` accept, because a reviewer should not have to know which verb wants which;
//! 2. read the repository's own `[risk.weights]`, `risk.tau` and `[detectors]` out of `fathom.toml`
//!    (ADR-002 §4.5: the config is read here and handed down as values, never opened below the CLI);
//! 3. apply any `--weight name=value` overrides, which is how a person *feels* a knob before deciding to
//!    write it down — and `--calibrate` is how they measure one;
//! 4. read the episode's feature vector ([`fathom_analyze::risk::collect`]), which runs the `e4.3`
//!    catalogue over it;
//! 5. score it and print: the number, the vector line, the feature table, the reasons in order, the
//!    features that could not be measured with the ticket that owns each, and what the model cannot see.
//!
//! # What it refuses
//!
//! A score with no denominator. An episode no feature could be measured for prints as a refusal, not as
//! `0`, because a zero on a review queue reads as "safe" and that is the one misreading this whole
//! feature exists to prevent (ADR-001 §7.7).
//!
//! ```text
//! $ fathom why ep-7f2a
//! ep-7f2a  "fix: drop the stale backup column"  ·  agent 0.95  ·  +340 −0  ·  3 files
//! features-1 49 · below τ 40
//!   9 of 12 features measured · weight denominator 0.76 · total 49.04 of 100.00 points
//!   features-1 49 = surface 17.11 + migration 11.84 + findings 7.89 + …
//! ```

use crate::output;
use fathom_core::exit::ExitCode;

/// `fathom why <episode> [--calibrate]`: one episode's risk score, or the calibration report.
///
/// # Errors
///
/// Exit 2 when neither an episode nor `--calibrate` was given, exit 12 when the index cannot answer for
/// the episode named, and exit 5-shaped notices are *not* errors: a weight this build could not honour
/// is printed with the score that ignores it, because the alternative is that a typo in a config stops a
/// reviewer from reading their queue.
#[allow(clippy::too_many_arguments)]
pub fn why(
    root: &str,
    target: Option<&str>,
    as_json: bool,
    calibrate: Option<Option<&str>>,
    overrides: &[String],
    tau: Option<u8>,
    cut: Option<f64>,
    extens: &crate::extens::Extensibility,
) -> eyre::Result<ExitCode> {
    if let Some(path) = calibrate {
        return calibration(root, path, as_json, cut);
    }
    let Some(target) = target else {
        return Err(crate::verbs::pending(
            ExitCode::Usage,
            "fathom why needs an episode: an episode id, a commit, a rev or an oid prefix",
            Some(
                "`fathom why HEAD`, or `fathom stats` for how many episodes the index holds, or \
                 `fathom why --calibrate` for the weight report"
                    .to_owned(),
            ),
        ));
    };

    #[cfg(not(feature = "sqlite-bundled"))]
    {
        let _ = (root, target, as_json, overrides, tau, extens);
        return Err(crate::verbs::feature_absent("fathom why", "sqlite-bundled"));
    }

    #[cfg(feature = "sqlite-bundled")]
    {
        score_one(root, target, as_json, overrides, tau, extens)
    }
}

/// One episode's score, printed.
#[cfg(feature = "sqlite-bundled")]
fn score_one(
    root: &str,
    target: &str,
    as_json: bool,
    overrides: &[String],
    tau: Option<u8>,
    extens: &crate::extens::Extensibility,
) -> eyre::Result<ExitCode> {
    use crate::state::{self, REMEDY_INDEX};
    use crate::verbs::{Pending, failed};
    use fathom_analyze::risk::collect::{self, Supply};
    use fathom_analyze::risk::feature::Feature;
    use fathom_analyze::risk::{score as risk_score, weights};
    use fathom_ingest::repo::Repository;
    use fathom_store::spine::Store;

    let root_path = std::path::Path::new(root);
    // The run's line on the event bus, opened before anything that can fail, so that a verb which gives up
    // half-way still leaves a terminal event behind (`e6.7`). A repository that switched events off pays
    // nothing: the bus has no subscribers, and every publish is a lock on an empty list.
    let (run, stream_notices) = crate::events::Run::start("why", root_path, extens);
    for notice in stream_notices {
        output::problem(format_args!("fathom why: {notice}"));
    }
    let snapshot = state::gather(root_path, false);
    let store_path = fathom_analyze::index::default_store_path(root_path);
    if !store_path.exists() {
        return Err(Pending::new(
            ExitCode::IndexDegraded,
            format!(
                "no index at {} yet, so there is nothing to score",
                crate::sanitize::Redactor::for_root(root_path).path(&store_path)
            ),
            Some(REMEDY_INDEX.to_owned()),
        )
        .into());
    }
    let store = Store::open(&store_path).map_err(failed)?;
    let repository = Repository::discover(root_path).map_err(failed)?;

    // The handle, in the order a reviewer types it: an id is unambiguous, a rev has to be resolved.
    let id = match store.episode(target).map_err(failed)? {
        Some(row) => row.id,
        None => {
            let oid = repository.resolve(target).map_err(|err| {
                Pending::new(
                    ExitCode::Error,
                    format!("`fathom why {target}` names no episode and no commit here: {err}"),
                    Some(
                        "the grammar is an episode id, `HEAD`, `@`, `main~3`, a full oid or a hex \
                         prefix; `fathom show` lists the newest ones"
                            .to_owned(),
                    ),
                )
            })?;
            let hex = oid.hex();
            match store.episode_for_commit(&hex).map_err(failed)? {
                Some(row) => row.id,
                None => {
                    return Err(Pending::new(
                        ExitCode::IndexDegraded,
                        format!(
                            "{} is a commit in this repository with no episode row in the index",
                            short(&hex)
                        ),
                        Some(REMEDY_INDEX.to_owned()),
                    )
                    .into());
                }
            }
        }
    };

    // Configuration, read here and handed down as values (ADR-002 §4.5).
    let config = std::fs::read_to_string(root_path.join("fathom.toml")).unwrap_or_default();
    let mut resolved = weights::weights_from_toml(&config);
    // Which detectors run, in the order the product resolves everything: the built-in default set, then
    // `fathom.toml`, then the command line (`e6.7`). A `detectors.disable` or a `--disable-detector` that
    // named nothing this build knows is a notice, never a failure — the same rule `detectors.enabled`
    // already follows, because a repository that upgrades should never be unable to ask a question.
    let resolved_policy = extens.apply_to(&config);
    let describe = resolved_policy.describe();
    let policy = resolved_policy.policy;
    for notice in &resolved_policy.notices {
        resolved.notice(notice.clone());
    }
    run.selection(&describe);
    for notice in extens.plugin_notices() {
        resolved.notice(notice);
    }
    for token in overrides {
        let (name, value) = weights::parse_weight_flag(token).map_err(|reason| {
            Pending::new(
                ExitCode::Usage,
                format!("--weight {reason}"),
                Some(format!("the features are {}", Feature::ids().join(", "))),
            )
        })?;
        resolved
            .weights
            .set(&name, value, weights::Origin::Flag)
            .map_err(|reason| {
                Pending::new(
                    ExitCode::Usage,
                    format!("--weight {name}: {reason}"),
                    Some(format!("the features are {}", Feature::ids().join(", "))),
                )
            })?;
        // A value outside `0.0..=1.0` is clamped rather than refused, exactly as a value in the file is,
        // and the notice says so: a flag typed as `--weight findings=2` means "a lot", and the reviewer
        // should be told what "a lot" cost, not be silently corrected.
        let clamped = resolved
            .weights
            .weights
            .iter()
            .find(|weight| weight.feature.id() == name)
            .and_then(|weight| weight.clamped_from.map(|written| (weight.value, written)));
        if let Some((standing, written)) = clamped {
            resolved.notice(format!(
                "--weight {name} = {written} is outside 0.0..=1.0, and was clamped to {standing}"
            ));
            continue;
        }
        resolved.notice(format!("--weight {name} = {value} overrides {}", name));
    }
    if let Some(tau) = tau {
        resolved.weights.tau = tau;
        resolved.notice(format!("--tau {tau} overrides risk.tau"));
    }

    let read =
        collect::read(&store, &repository, &id, &policy, &Supply::nothing()).map_err(|err| {
            Pending::new(
                ExitCode::StoreCorrupt,
                format!("reading the feature vector for {id}: {err}"),
                Some("fathom index --rebuild".to_owned()),
            )
        })?;
    let Some(read) = read else {
        return Err(Pending::new(
            ExitCode::IndexDegraded,
            format!("the index has no episode {id} any more"),
            Some(REMEDY_INDEX.to_owned()),
        )
        .into());
    };

    // The scan's own account of itself, on the bus: every detector that was asked, every finding that
    // survived, and every gap (`e6.7`). This is the stream an external subscriber reads, so it carries the
    // whole truth of the scan and not only the part the terminal prints.
    run.analysis_started(&id, read.report.runs.len());
    run.detectors(&read.report);
    run.findings(&read.report);
    for gap in &read.report.gaps {
        run.notice(&id, gap);
    }
    run.analysis_finished(&read.report);

    let scored = risk_score::score(&read.vector, &resolved.weights);
    let row = store.episode(&id).map_err(failed)?;
    if as_json {
        output::line(format_args!(
            "{}",
            payload(&scored, &resolved.weights, &read, row.as_ref())
        ));
    } else {
        if let Some(row) = row.as_ref() {
            output::line(format_args!(
                "{}  {:?}  ·  {} {}  ·  +{} −{}  ·  {} file(s)",
                row.id,
                row.summary,
                row.author_kind,
                row.author_confidence,
                row.n_add,
                row.n_del,
                row.n_files
            ));
        }
        for line in risk_score::render(&scored) {
            output::line(format_args!("{line}"));
        }
        if !read.report.findings.is_empty() {
            output::line(format_args!("\nfindings the score counted:"));
            for finding in read.report.findings.iter().take(12) {
                output::line(format_args!(
                    "  {} {} — {}",
                    finding.severity, finding.detector_id, finding.message
                ));
            }
        }
    }
    let code = snapshot.worst_code();
    run.finished(code.as_i32());
    Ok(code)
}

/// The `--json` payload: the envelope the other verbs use, and the score's own structure inside it.
#[cfg(feature = "sqlite-bundled")]
fn payload(
    scored: &fathom_analyze::risk::score::Score,
    weights: &fathom_analyze::risk::weights::Weights,
    read: &fathom_analyze::risk::collect::Read,
    row: Option<&fathom_store::episode::EpisodeRow>,
) -> String {
    use serde_json::{Value, json};

    let mut body = scored.to_json();
    let object = match body {
        Value::Object(ref mut map) => map,
        _ => return body.to_string(),
    };
    object.insert("contract".to_owned(), json!(CONTRACT));
    // The weights that were actually used, with where each came from: a score a reviewer can audit has to
    // say which numbers came from their own `fathom.toml` and which from the build.
    object.insert(
        "weights".to_owned(),
        serde_json::to_value(&weights.weights).unwrap_or(Value::Null),
    );
    object.insert(
        "episode".to_owned(),
        json!({
            "id": row.map(|row| row.id.clone()).unwrap_or_else(|| scored.episode.clone()),
            "summary": row.map(|row| row.summary.clone()).unwrap_or_default(),
            "author_kind": row.map(|row| row.author_kind.clone()).unwrap_or_default(),
            "author_confidence": row.map(|row| row.author_confidence).unwrap_or_default(),
            "n_add": row.map(|row| row.n_add).unwrap_or_default(),
            "n_del": row.map(|row| row.n_del).unwrap_or_default(),
            "n_files": row.map(|row| row.n_files).unwrap_or_default(),
        }),
    );
    object.insert(
        "findings".to_owned(),
        json!({
            "total": read.report.findings.len(),
            "detectors_ran": read.report.runs.iter().filter(|run| run.outcome == fathom_analyze::detectors::Outcome::Ran).count(),
            "detectors": read.report.runs.len(),
            "ceiling": read.report.ceiling().map(|severity| severity.as_str()),
        }),
    );
    serde_json::to_string(&body).unwrap_or_else(|_| format!("{{\"contract\":\"{CONTRACT}\"}}"))
}

/// The contract id the payload carries, in the `fathom.cli.<verb>/<major>` shape every published verb
/// uses (`query.rs` keeps the others). A consumer pins this; a change to the payload's meaning bumps the
/// number, and the payload prints the score's own structure rather than a re-shaping of it.
pub const CONTRACT: &str = "fathom.cli.why/1";

/// The calibration report: the shipped weights, the retuned ones, and the difference between them.
///
/// With no path the shipped set is used — the one the tests bless, compiled in — so the command answers
/// on a machine that has never seen a calibration file. With a path, that file is the corpus, and a
/// repository is measuring its own dispositions instead of agreeing with someone else's.
fn calibration(
    root: &str,
    path: Option<&str>,
    as_json: bool,
    cut: Option<f64>,
) -> eyre::Result<ExitCode> {
    use crate::state;
    use crate::verbs::Pending;
    use fathom_analyze::risk::{calibrate, weights};

    let root_path = std::path::Path::new(root);
    let snapshot = state::gather(root_path, false);
    let fraction = cut.unwrap_or(calibrate::DEFAULT_FRACTION);
    if !(0.01..=1.0).contains(&fraction) {
        return Err(Pending::new(
            ExitCode::Usage,
            format!("--cut {fraction} is not a fraction between 0.01 and 1.0"),
            Some("the shape is --cut 0.1 for the top ten percent".to_owned()),
        )
        .into());
    }

    let config = std::fs::read_to_string(root_path.join("fathom.toml")).unwrap_or_default();
    let resolved = weights::weights_from_toml(&config);
    let set = match path {
        None => calibrate::parse(calibrate::EMBEDDED).map_err(|err| {
            Pending::new(
                ExitCode::Error,
                format!("the shipped calibration set does not parse: {err}"),
                Some("it is compiled in, so this is a build fault: `cargo test -p fathom-analyze --test risk_score`".to_owned()),
            )
        })?,
        Some(file) => {
            let text = std::fs::read_to_string(file).map_err(|err| {
                Pending::new(
                    ExitCode::Error,
                    format!("the calibration set at {file} cannot be read: {err}"),
                    Some("the shape is {\"cases\": [{\"id\", \"summary\", \"label\", \"features\"}]}".to_owned()),
                )
            })?;
            calibrate::parse(&text).map_err(|err| {
                Pending::new(
                    ExitCode::Error,
                    format!("{file}: {err}"),
                    Some("the feature names are the ones `fathom why --weights` prints".to_owned()),
                )
            })?
        }
    };
    let comparison = calibrate::calibrate(&set, &resolved.weights, fraction);
    if as_json {
        output::line(format_args!(
            "{}",
            serde_json::to_string(&calibrate::to_json(&comparison)).unwrap_or_default()
        ));
    } else {
        if path.is_none() {
            output::line(format_args!(
                "the shipped calibration set — `fathom why --calibrate <file>` measures your own"
            ));
        }
        for line in calibrate::render(&comparison) {
            output::line(format_args!("{line}"));
        }
    }
    Ok(snapshot.worst_code())
}

/// `fathom why --weights`: the feature table, so a person can write `[risk.weights]` without reading Rust.
///
/// This is the discoverability half of the knob: the names, the unit each is measured in, the ruler that
/// turns the unit into 0-1, the shipped weight, and whether this build can measure it at all.
pub fn print_weights(as_json: bool) -> eyre::Result<ExitCode> {
    use fathom_analyze::risk::feature::{Feature, SPECS};
    use fathom_analyze::risk::weights::Weights;
    use serde_json::json;

    let defaults = Weights::defaults();
    if as_json {
        let rows: Vec<_> = SPECS
            .iter()
            .map(|spec| {
                json!({
                    "feature": spec.id,
                    "weight": Feature::parse(spec.id).map(|f| defaults.weight(f)).unwrap_or(spec.default),
                    "unit": spec.unit,
                    "ruler": spec.ruler,
                    "source": spec.source,
                    "owner": spec.owner,
                    "asks": spec.asks,
                })
            })
            .collect();
        output::line(format_args!(
            "{}",
            json!({"contract": CONTRACT, "model": fathom_analyze::risk::MODEL, "features": rows})
        ));
        return Ok(ExitCode::Ok);
    }
    output::line(format_args!(
        "features-1 — the weight sum is the denominator, so relative weight is the whole knob"
    ));
    for spec in SPECS {
        output::line(format_args!(
            "{:<12} {:>5}   {}\n{:<12}        ruler: {}\n{:<12}        source: {} (owner {})",
            spec.id, spec.default, spec.asks, "", spec.ruler, "", spec.source, spec.owner
        ));
    }
    output::line(format_args!(
        "\nset one in fathom.toml, e.g.\n[risk.weights]\nfindings = 0.25\nsize = 0.02\ntau lives beside them: risk.tau = 55"
    ));
    Ok(ExitCode::Ok)
}

/// A short handle for a message, so a 40-character oid does not push the remedy off the line.
fn short(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 12 {
        return value.to_owned();
    }
    chars[..12].iter().collect::<String>() + "…"
}
