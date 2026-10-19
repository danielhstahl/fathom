//! `fathom narrative <episode>` — the prose a human skims, with a citation on every claim
//! (`e4.7`, `reco-2rx`).
//!
//! [`fathom_analyze::narrative`] composes it and this file prints it, which is the whole split: the
//! composer is a pure function of the delta document so that the TUI, an exported artefact and an MCP tool
//! result can all carry the same bytes, and the verb owns only the store read and the formatting choice.
//!
//! Three renderings, one text:
//!
//! * **default** — the plain-text narrative, for a terminal;
//! * **`--markdown`** — the artefact body, the exact bytes `fathom export` will carry when `e5.7` lands;
//! * **`--json`** — the structure: claims, citations, weights and the self-check, one line on stdout.
//!
//! `--explain` puts the ranking beside the prose rather than inside it: a weight printed inside a sentence
//! would be a number that sentence's own citations do not license, and the audit is the point.
//! `--attribution` prints the LLM answer instead of an episode's — whether a model was in the path of
//! anything this build writes — which is the question spike `s5` decided and the one ADR-001 §8 non-goal 6
//! asks a build to keep answering.

use crate::output;
use fathom_core::exit::ExitCode;

/// `fathom narrative <episode> [--markdown] [--json] [--explain] [--top N]`.
///
/// # Errors
///
/// Exit 2 when neither an episode nor `--attribution` was given; exit 12 (`INDEX_DEGRADED`) when the index
/// has not seen that episode, with the remedy on the next line. A narrative that fails its own audit is not
/// an error exit: the verdict is printed as the last line of the prose, because the prose is still the most
/// useful thing on the screen and the reader should see both.
pub fn narrate(
    root: &str,
    target: Option<&str>,
    as_json: bool,
    markdown: bool,
    explain: bool,
    top: Option<usize>,
    attribution: bool,
) -> eyre::Result<ExitCode> {
    if attribution {
        return llm_answer(as_json);
    }
    let Some(target) = target else {
        return Err(crate::verbs::pending(
            ExitCode::Usage,
            "fathom narrative needs an episode: an episode id, a commit, a rev or an oid prefix",
            Some(
                "`fathom narrative HEAD`, or `--attribution` for whether a model writes any of this"
                    .to_owned(),
            ),
        ));
    };

    #[cfg(not(feature = "sqlite-bundled"))]
    {
        let _ = (root, target, as_json, markdown, explain, top);
        return Err(crate::verbs::feature_absent(
            "fathom narrative",
            "sqlite-bundled",
        ));
    }

    #[cfg(feature = "sqlite-bundled")]
    {
        one(root, target, as_json, markdown, explain, top.unwrap_or(3))
    }
}

/// Read one episode's delta and print its narrative.
#[cfg(feature = "sqlite-bundled")]
fn one(
    root: &str,
    target: &str,
    as_json: bool,
    markdown: bool,
    explain: bool,
    top: usize,
) -> eyre::Result<ExitCode> {
    use crate::state::{self, REMEDY_INDEX};
    use crate::verbs::{Pending, failed};
    use fathom_analyze::insight;
    use fathom_analyze::narrative::{self, Options};
    use fathom_ingest::repo::Repository;
    use fathom_store::spine::Store;

    let root_path = std::path::Path::new(root);
    let snapshot = state::gather(root_path, false);
    let store_path = fathom_analyze::index::default_store_path(root_path);
    if !store_path.exists() {
        return Err(Pending::new(
            ExitCode::IndexDegraded,
            format!(
                "no index at {} yet, so there is no narrative to print",
                crate::sanitize::Redactor::for_root(root_path).path(&store_path)
            ),
            Some(REMEDY_INDEX.to_owned()),
        )
        .into());
    }
    let store = Store::open(&store_path).map_err(failed)?;

    // The same handle grammar as `fathom analyze` and `fathom why`: an episode id first, because it is
    // unambiguous, then a rev.
    let mut delta = match store.episode(target) {
        Ok(Some(_)) => insight::for_episode(&store, target).map_err(|err| {
            Pending::new(
                ExitCode::StoreCorrupt,
                format!("reading the delta for {target}: {err}"),
                Some("fathom index --rebuild".to_owned()),
            )
        })?,
        Ok(None) | Err(_) => None,
    };
    if delta.is_none() {
        let repository = Repository::discover(root_path).map_err(failed)?;
        let oid = repository.resolve(target).map_err(|err| {
            Pending::new(
                ExitCode::Error,
                format!("`fathom narrative {target}` names no episode and no commit here: {err}"),
                Some(
                    "the grammar is an episode id, `HEAD`, `@`, `main~3`, a full oid or a hex prefix; \
                     `fathom show` lists the newest ones"
                        .to_owned(),
                ),
            )
        })?;
        delta = insight::for_commit(&store, &oid.hex()).map_err(|err| {
            Pending::new(
                ExitCode::IndexDegraded,
                format!("reading the delta for {}: {err}", oid.hex()),
                Some(REMEDY_INDEX.to_owned()),
            )
        })?;
    }
    let Some(delta) = delta else {
        return Err(Pending::new(
            ExitCode::IndexDegraded,
            format!("{target} has no episode row in the index, so nothing was measured for it"),
            Some(REMEDY_INDEX.to_owned()),
        )
        .into());
    };

    let options = Options::new().with_top(top);
    let composed = narrative::compose(&delta, &options);
    if as_json {
        // One line, the only thing on stdout — the same contract every machine-facing verb keeps.
        output::line(format_args!("{}", composed.to_json()));
        return Ok(snapshot.worst_code());
    }
    let body = if markdown {
        // Byte-identical to what the export artefact will carry, because it is the same call.
        fathom_analyze::export::narrative_markdown(&delta)
    } else {
        composed.to_text()
    };
    for line in body.lines() {
        output::line(format_args!("{line}"));
    }
    if explain {
        output::line(format_args!(""));
        output::line(format_args!(
            "Ranking — why each sentence sits where it does:"
        ));
        for line in composed.explain_lines() {
            output::line(format_args!("  {line}"));
        }
    }
    Ok(snapshot.worst_code())
}

/// The answer to "was a model in the path of this?", printed instead of an episode (`s5`, `reco-nv0`).
fn llm_answer(as_json: bool) -> eyre::Result<ExitCode> {
    use fathom_analyze::narrative::llm;

    if as_json {
        let document: Vec<(&str, &str)> = llm::state().into_iter().collect();
        let gates: Vec<&str> = llm::GATES.to_vec();
        let payload = serde_json::json!({
            "contract": "fathom-narrative-llm/1",
            "shipped": llm::SHIPPED,
            "state": document,
            "gates": gates,
        });
        output::line(format_args!("{payload}"));
        return Ok(ExitCode::Ok);
    }
    for (key, value) in llm::state() {
        output::line(format_args!("{key:<18} {value}"));
    }
    output::line(format_args!(""));
    output::line(format_args!(
        "To reopen the polish path, every gate below has to clear. They are the decision's, \
         quoted, in order:"
    ));
    for (index, (gate, status)) in llm::rejection_reasons().iter().enumerate() {
        output::line(format_args!("  {}. {gate}", index + 1));
        output::line(format_args!("      now: {status}"));
    }
    Ok(ExitCode::Ok)
}
