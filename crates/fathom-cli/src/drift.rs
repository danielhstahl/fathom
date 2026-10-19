//! `fathom drift` — what the plan said, measured against what the session did (`e4.8`, `reco-cpr`).
//!
//! [`fathom_analyze::drift`] is the analysis and this file is the read: open the index, take the episodes in
//! session order, lift the recorded intent into a plan, lift the ledger into sign-offs, hand the whole lot
//! over, and print what came back. That split is the reason every finding in the report has a fixture
//! (ADR-002 §4.5).
//!
//! The default scope is the last [`DEFAULT_LIMIT`] episodes, grouped by the harness session each one names
//! and falling back to the lineage ref when none was named. `--session` narrows to one session; `--all`
//! widens past the limit. Both bound the *read*, not the rules: the window, the thresholds and the caps are
//! the policy's ([`fathom_analyze::drift::Policy`]) and the report prints them beside its verdict.
//!
//! ```text
//! fathom drift                        # the last sessions, both sides of every contradiction
//! fathom drift --session s-7f2        # one session
//! fathom drift --window 2 --json      # a tighter look-ahead, as one line
//! fathom drift --dismiss drift:plan-unimplemented:9c1… # a contradiction already argued about
//! ```

use crate::output;
use fathom_core::exit::ExitCode;

/// How many episodes a default `fathom drift` reads. A session is a handful of turns, and a read that
/// swallowed the whole repository would spend its report on history nobody is reviewing.
pub const DEFAULT_LIMIT: i64 = 200;

/// `fathom drift [--session S] [--all] [--window N] [--min-overlap F] [--max N] [--dismiss KEY]…
/// [--off KIND]… [--json] [--fail-on-findings]`.
///
/// # Errors
///
/// Exit 12 (`INDEX_DEGRADED`) when there is no index or it holds no episodes yet, with the remedy on the
/// next line. A report with no findings is exit 0 — "nothing contradicted" is an answer, not a failure —
/// and so is a report that could compare nothing, which says so in its first line: the verb reports.
/// `--fail-on-findings` is the gate, and it exits 7 (`FINDINGS`) the way `fathom check` does, because a CI
/// step wants one bit.
#[allow(clippy::too_many_arguments)]
pub fn drift(
    root: &str,
    session: Option<&str>,
    everything: bool,
    window: Option<usize>,
    min_overlap: Option<f64>,
    max: Option<usize>,
    dismiss: &[String],
    off: &[String],
    as_json: bool,
    fail_on_findings: bool,
) -> eyre::Result<ExitCode> {
    #[cfg(not(feature = "sqlite-bundled"))]
    {
        let _ = (
            root,
            session,
            everything,
            window,
            min_overlap,
            max,
            dismiss,
            off,
            as_json,
            fail_on_findings,
        );
        return Err(crate::verbs::feature_absent(
            "fathom drift",
            "sqlite-bundled",
        ));
    }

    #[cfg(feature = "sqlite-bundled")]
    {
        run(
            root,
            session,
            everything,
            window,
            min_overlap,
            max,
            dismiss,
            off,
            as_json,
            fail_on_findings,
        )
    }
}

/// The `[drift]` table of `fathom.toml`. A file that does not parse is no table at all — `fathom index`
/// already said it does not parse, and a second verb saying so is noise.
#[derive(Debug, Default)]
struct Config {
    window: Option<usize>,
    min_overlap: Option<f64>,
    max_per_kind: Option<usize>,
    max_findings: Option<usize>,
    cross_session: bool,
    dismiss: Vec<String>,
    off: Vec<String>,
}

impl Config {
    fn read(root: &std::path::Path) -> Self {
        let Ok(text) = std::fs::read_to_string(root.join("fathom.toml")) else {
            return Self::default();
        };
        let Ok(document) = text.parse::<toml_edit::DocumentMut>() else {
            return Self::default();
        };
        let Some(table) = document.get("drift").and_then(|item| item.as_table()) else {
            return Self::default();
        };
        let number = |key: &str| table.get(key).and_then(toml_edit::Item::as_integer);
        let flag = |key: &str| {
            table
                .get(key)
                .and_then(toml_edit::Item::as_bool)
                .unwrap_or_default()
        };
        let words = |key: &str| {
            table
                .get(key)
                .and_then(toml_edit::Item::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(toml_edit::Value::as_str)
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        };
        Config {
            window: number("window").map(|value| value.clamp(1, 24) as usize),
            min_overlap: table.get("min-overlap").and_then(toml_edit::Item::as_float),
            max_per_kind: number("max-per-kind").map(|value| value.clamp(1, 50) as usize),
            max_findings: number("max-findings").map(|value| value.clamp(1, 200) as usize),
            cross_session: flag("cross-session"),
            dismiss: words("dismiss"),
            off: words("kinds-off"),
        }
    }
}

/// Read the sessions out of the index and print what the comparison found.
#[cfg(feature = "sqlite-bundled")]
#[allow(clippy::too_many_arguments)]
fn run(
    root: &str,
    session: Option<&str>,
    everything: bool,
    window: Option<usize>,
    min_overlap: Option<f64>,
    max: Option<usize>,
    dismiss: &[String],
    off: &[String],
    as_json: bool,
    fail_on_findings: bool,
) -> eyre::Result<ExitCode> {
    use crate::state::{self, REMEDY_INDEX};
    use crate::verbs::{Pending, failed};
    use fathom_analyze::drift::{self, Episode, Policy, Session};
    use fathom_store::episode::{EpisodeFile, EpisodeQuery};
    use fathom_store::spine::Store;

    let root_path = std::path::Path::new(root);
    let _ = state::gather(root_path, false);
    let store_path = fathom_analyze::index::default_store_path(root_path);
    if !store_path.exists() {
        return Err(Pending::new(
            ExitCode::IndexDegraded,
            format!(
                "no index at {} yet, so there is no session history to compare",
                crate::sanitize::Redactor::for_root(root_path).path(&store_path)
            ),
            Some(REMEDY_INDEX.to_owned()),
        )
        .into());
    }
    let mut store = Store::open(&store_path).map_err(failed)?;
    let redact = crate::sanitize::Redactor::for_root(root_path);

    let query = EpisodeQuery {
        limit: if everything { 0 } else { DEFAULT_LIMIT },
        include_dead: true,
        ..EpisodeQuery::default()
    };
    let rows = store.episodes(&query).map_err(failed)?;
    if rows.is_empty() {
        return Err(Pending::new(
            ExitCode::IndexDegraded,
            "the index holds no episodes yet, so there is no plan to compare against a change",
            Some(REMEDY_INDEX.to_owned()),
        )
        .into());
    }

    // One read for every episode's files, then split by episode: the store caps that read and says whether
    // it did, and a capped read is a fact the report has to carry rather than a hole in the paths.
    let ids: Vec<String> = rows.iter().map(|row| row.id.clone()).collect();
    let files = store.episode_files(&ids).map_err(failed)?;
    if files.truncated {
        output::problem(format_args!(
            "note: the file read was capped, so an episode's touched paths may be incomplete — a path \
             drift detector will not see what this read did not return"
        ));
    }
    let signoffs = ledger_signoffs(&mut store)?;

    // Group in the order the index returned them (`episodes` orders newest first), then reverse each group:
    // the drift window looks *forward*, so it needs the oldest turn first.
    let mut sessions: Vec<Session> = Vec::new();
    for row in &rows {
        let key = row
            .session_id
            .clone()
            .unwrap_or_else(|| format!("lineage:{}", row.lineage_ref));
        if let Some(wanted) = session
            && key != wanted
            && row.session_id.as_deref() != Some(wanted)
        {
            continue;
        }
        let empty: Vec<EpisodeFile> = Vec::new();
        let touched = files.by_episode.get(&row.id).unwrap_or(&empty);
        let paths: Vec<String> = touched.iter().map(|file| file.path.clone()).collect();
        push(
            &mut sessions,
            key,
            Episode {
                id: row.id.clone(),
                summary: row.summary.clone(),
                ordinal: 0,
                plan: plan_of(row),
                touched: paths.clone(),
                tests_touched: tests_touched(touched),
                // The summed file rows keep the *new* path after a rename, so a move is not knowable from
                // this read. `None` is the honest answer, and the refactor detector reads it as "cannot
                // rule a move out" rather than as "no move happened".
                moved_paths: if paths.is_empty() { Some(false) } else { None },
                manifests: manifests(touched),
                signoff: signoffs.get(&row.id).cloned(),
            },
        );
    }
    for one in &mut sessions {
        one.episodes.reverse();
        for (index, episode) in one.episodes.iter_mut().enumerate() {
            episode.ordinal = index;
        }
    }

    let config = Config::read(root_path);
    let mut dismissed: Vec<String> = config.dismiss.clone();
    dismissed.extend_from_slice(dismiss);
    let mut kinds_off: Vec<String> = config.off.clone();
    kinds_off.extend_from_slice(off);
    let mut policy = Policy::from_values(
        window.or(config.window),
        min_overlap.or(config.min_overlap),
        Some(
            config
                .max_per_kind
                .unwrap_or(fathom_analyze::drift::DEFAULT_MAX_PER_KIND),
        ),
        max.or(config.max_findings),
        Some(config.cross_session),
        &dismissed,
        &kinds_off,
    );
    policy.max_per_kind = policy.max_per_kind.max(1);

    let report = drift::analyse(&sessions, &policy);
    if as_json {
        output::line(format_args!("{}", redact.text(&report.to_json())));
    } else {
        for line in report.render() {
            output::line(format_args!("{}", redact.text(&line)));
        }
    }
    if fail_on_findings && !report.findings.is_empty() {
        return Ok(ExitCode::CheckViolations);
    }
    Ok(ExitCode::Ok)
}

/// Append to the session with this key, creating it when it is new.
#[cfg(feature = "sqlite-bundled")]
fn push(
    sessions: &mut Vec<fathom_analyze::drift::Session>,
    key: String,
    episode: fathom_analyze::drift::Episode,
) {
    if let Some(found) = sessions
        .iter_mut()
        .find(|session| session.id.as_deref() == Some(key.as_str()))
    {
        found.episodes.push(episode);
        return;
    }
    sessions.push(fathom_analyze::drift::Session {
        id: Some(key),
        episodes: vec![episode],
    });
}

/// The plan a row recorded, in drift's shape. `None` is the honest "nothing stated a plan".
#[cfg(feature = "sqlite-bundled")]
fn plan_of(row: &fathom_store::episode::EpisodeRow) -> Option<fathom_analyze::drift::Plan> {
    use fathom_analyze::drift::{Plan, PlanItem, Rung};
    use fathom_ingest::intent::{Intent, Source};

    if row.intent_state != "recorded" || row.intent.trim().is_empty() {
        return None;
    }
    let intent: Intent = serde_json::from_str(row.intent.trim()).ok()?;
    if intent.plan.is_empty() && intent.text.is_none() {
        return None;
    }
    let source = Source::parse(&intent.source);
    Some(Plan {
        items: intent
            .plan
            .iter()
            .map(|item| PlanItem {
                text: item.text.clone(),
                done: item.is_done(),
            })
            .collect(),
        scope: intent.scope.clone(),
        text: intent.text.clone().or_else(|| Some(row.summary.clone())),
        rung: Rung {
            source: intent.source.clone(),
            confidence: if intent.confidence > 0.0 {
                intent.confidence
            } else {
                source.confidence()
            },
            stated: source.is_stated(),
        },
    })
}

/// Was a test surface touched? `None` when the index could not tell — which is never read as "no".
#[cfg(feature = "sqlite-bundled")]
fn tests_touched(files: &[fathom_store::episode::EpisodeFile]) -> Option<bool> {
    let mut known = false;
    for file in files {
        match file.path_class.as_str() {
            "test" | "tests" => return Some(true),
            "unknown" | "" => {}
            _ => known = true,
        }
    }
    known.then_some(false)
}

/// The dependency manifests among an episode's paths.
#[cfg(feature = "sqlite-bundled")]
fn manifests(files: &[fathom_store::episode::EpisodeFile]) -> Vec<String> {
    files
        .iter()
        .filter(|file| {
            let name = file
                .path
                .rsplit('/')
                .next()
                .unwrap_or(&file.path)
                .to_lowercase();
            name.contains("cargo.toml")
                || name.contains("package.json")
                || name.ends_with(".lock")
                || name.starts_with("requirements")
                || name.contains("pyproject")
                || name.contains("go.mod")
        })
        .map(|file| file.path.clone())
        .collect()
}

/// The reviewer's own words per episode, for the sign-off contradiction. Only `accepted` entries count: a
/// flagged or dismissed entry is a reviewer who already disagreed, and agreeing to disagree is not a
/// contradiction.
#[cfg(feature = "sqlite-bundled")]
fn ledger_signoffs(
    store: &mut fathom_store::spine::Store,
) -> eyre::Result<std::collections::BTreeMap<String, String>> {
    use std::collections::BTreeMap;

    let mut out: BTreeMap<String, String> = BTreeMap::new();
    for row in store.ledger_rows().map_err(crate::verbs::failed)? {
        let Some(episode) = row.episode_id else {
            continue;
        };
        if row.decision != "accepted" {
            continue;
        }
        let note = if row.note.trim().is_empty() {
            format!("{} signed it off as accepted", row.actor)
        } else {
            row.note.trim().to_owned()
        };
        if let Some(existing) = out.get_mut(&episode) {
            *existing = format!("{existing}; {note}");
        } else {
            out.insert(episode, note);
        }
    }
    Ok(out)
}
