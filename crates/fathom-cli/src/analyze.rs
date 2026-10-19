//! `fathom analyze <episode>` — the one versioned delta model, printed (`reco-tt4`, `e2.11`).
//!
//! This verb is the surface the other three read from. The document it prints is built by
//! [`fathom_analyze::insight`], the same façade the TUI card and the export handoff are restricted to
//! (the crate-boundary lint in `tools/check_workspace.py` is what makes that restriction a fact rather
//! than an intention), and it is printed here in all three of the forms a consumer can want:
//!
//! * **text** (default): [`fathom_analyze::insight::render`]'s lines. The renderer lives with the model
//!   for the same reason the model does — so that the words "the ranking cannot see 7 things" cannot
//!   become three slightly different sentences in three surfaces.
//! * **`--json`**: the document, one line, on stdout and nothing else there (ADR-002 §5.1). This is what
//!   `schemas/episode-delta.schema.json` describes, what `tests/delta_schema.rs` validates, and what an
//!   MCP tool result would carry verbatim.
//! * **`--schema`**: the JSON Schema itself, so a consumer can pin the contract without reading the
//!   repository out. `--schema --check` compares the checked-in file against what this build generates,
//!   which is the drift gate `e0.8` set up for the config schema (ADR-002 §6.1) applied to this one.
//!
//! The document is not a dump of the store. It is the analysis' answer to seven questions, and when the
//! analysis has no answer it says so and names the plan key that owns the missing piece — which is the
//! only reason a reader can tell "no tests changed" from "nobody looked".

// The store-backed read is the whole verb; a `minimal` build refuses it before it prints anything, and
// the schema half is deliberately kept out from under the gate so a gate binary can still pin the
// contract it is about to be fed.
#![cfg_attr(not(feature = "sqlite-bundled"), allow(dead_code))]

use crate::output;
use fathom_analyze::insight;
use fathom_core::exit::ExitCode;

/// `fathom analyze <episode> [--json]`: one episode's delta document.
///
/// The handle is anything [`fathom_ingest::revspec`] resolves — a commit, a rev, an oid prefix — plus an
/// episode id, because by now episodes have identities of their own and making a reviewer translate an
/// id into a commit to ask what it means is a ceremony with no purpose.
///
/// # Errors
///
/// Exit 1 when the handle names nothing in the repository (the caller's typo), exit 12 when it names a
/// commit the index has never seen (the index's gap, remedy `fathom index`), matching `fathom show`.
pub fn analyze(
    root: &str,
    target: Option<&str>,
    as_json: bool,
    schema: bool,
    check: bool,
    write: bool,
) -> eyre::Result<ExitCode> {
    if schema {
        return print_schema(check, write);
    }
    let Some(target) = target else {
        return Err(crate::verbs::pending(
            ExitCode::Usage,
            "fathom analyze needs an episode: a commit, a rev, an oid prefix or an episode id",
            // A usage error is still a user-facing error, and "you omitted an argument" is only half of it:
            // the reviewer needs the shape of what goes there and the verb that finds one (`e5.9`).
            Some(
                "the grammar is `HEAD`, `@`, `main~3`, `HEAD^2`, a full oid or a hex prefix; `fathom \
                 stats` prints how many episodes the index holds and `fathom show HEAD` the newest one"
                    .to_owned(),
            ),
        ));
    };

    #[cfg(not(feature = "sqlite-bundled"))]
    {
        let _ = (root, target, as_json);
        return Err(crate::verbs::feature_absent(
            "fathom analyze",
            "sqlite-bundled",
        ));
    }

    #[cfg(feature = "sqlite-bundled")]
    {
        use crate::state::{self, REMEDY_INDEX};
        use crate::verbs::{Pending, failed};
        use fathom_ingest::repo::Repository;
        use fathom_store::spine::Store;

        let root_path = std::path::Path::new(root);
        let snapshot = state::gather(root_path, false);
        let redactor = crate::sanitize::Redactor::for_root(root_path);
        let store_path = fathom_analyze::index::default_store_path(root_path);
        if !store_path.exists() {
            return Err(Pending::new(
                ExitCode::IndexDegraded,
                format!(
                    "no index at {} yet, so there is no delta to print",
                    redactor.path(&store_path)
                ),
                Some(REMEDY_INDEX.to_owned()),
            )
            .into());
        }
        let store = Store::open(&store_path).map_err(failed)?;

        // An episode id first, because it is the cheaper question and it cannot be ambiguous with a rev:
        // episode ids carry a prefix no revspec grammar produces.
        let delta = match store.episode(target) {
            Ok(Some(_)) => insight::for_episode(&store, target).map_err(|err| {
                Pending::new(
                    ExitCode::StoreCorrupt,
                    format!("reading the delta for {target}: {err}"),
                    Some("fathom index --rebuild".to_owned()),
                )
            })?,
            Ok(None) | Err(_) => None,
        };
        let mut blocked: Option<String> = None;
        let delta = match delta {
            Some(found) => Some(found),
            None => {
                let repository = Repository::discover(root_path).map_err(failed)?;
                let oid = repository.resolve(target).map_err(|err| {
                    Pending::new(
                        ExitCode::Error,
                        format!(
                            "`fathom analyze {target}` names no episode and no commit in this \
                             repository: {err}"
                        ),
                        Some(
                            "the grammar is an episode id, `HEAD`, `@`, `main~3`, `HEAD^2`, \
                             `v1.2^{}`, a full oid, or a hex prefix"
                                .to_owned(),
                        ),
                    )
                })?;
                let hex = oid.hex();
                // Whether the object database will serve this commit, remembered for the refusal below: an
                // index that could not read a commit and an index that has not seen it yet are different
                // failures and want different remedies.
                blocked = store.unreadable_commit(&hex).ok().flatten();
                insight::for_commit(&store, &hex).map_err(|err| {
                    Pending::new(
                        ExitCode::IndexDegraded,
                        format!("reading the delta for {}: {err}", short(&hex)),
                        Some(REMEDY_INDEX.to_owned()),
                    )
                })?
            }
        };
        let Some(delta) = delta else {
            return Err(match blocked {
                // The commit exists, the index tried it, and the object database would not serve it.
                Some(reason) => Pending::new(
                    ExitCode::IndexDegraded,
                    format!(
                        "{} could not be read out of the object database: {}",
                        short(target),
                        redactor.text(&reason)
                    ),
                    Some(state::REMEDY_UNREADABLE.to_owned()),
                ),
                None => Pending::new(
                    ExitCode::IndexDegraded,
                    format!(
                        "{} is a commit in this repository with no episode row in the index",
                        short(target)
                    ),
                    Some(REMEDY_INDEX.to_owned()),
                ),
            }
            .into());
        };

        if as_json {
            // One line, and the only thing on stdout: a consumer pipes this into `jq`, and a second
            // pretty-printed line would break the pipe (`e1.10`'s contract, asserted by
            // `tests/delta_schema.rs`).
            output::line(format_args!("{}", delta.to_json()));
        } else {
            for line in insight::render(&delta) {
                output::line(format_args!("{line}"));
            }
            // The answer above is complete for *this* commit. It is not a claim about the rest of the
            // repository: when the object database is withholding commits, the reader has to be told that
            // the history around this episode has holes, or a confident delta becomes the reason they assume
            // it does not.
            if let Ok(holes) = store.unreadable_commits() {
                if !holes.is_empty() {
                    let named = holes
                        .iter()
                        .take(3)
                        .map(|(oid, _, _)| format!("{}…", &oid[..12]))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let more = holes.len().saturating_sub(3);
                    output::line(format_args!(
                        "\nindex: {} commit(s) in this repository could not be read out of the object \\n         database ({named}{}), so they are in no answer here either",
                        holes.len(),
                        if more == 0 {
                            String::new()
                        } else {
                            format!(" and {more} more")
                        }
                    ));
                    output::line(format_args!("  remedy: {}", state::REMEDY_UNREADABLE));
                }
            }
        }
        Ok(snapshot.worst_code())
    }
}

/// Print the published schema, or check the checked-in copy against this build's generator.
///
/// The generator lives in the crate that owns the model, so a schema that has drifted from the struct is
/// impossible by construction; what this closes is the other half — a checked-in file edited by hand to
/// make a test pass. `--check` is the CI invocation, and it prints the first real difference rather than
/// a checksum, because whoever broke it has to fix it.
fn print_schema(check: bool, write: bool) -> eyre::Result<ExitCode> {
    let generated = insight::schema_to_text();
    let path = std::path::Path::new(insight::SCHEMA_FILE);
    if write {
        // The blessed way to change the published file: the bytes still come from the generator, so the
        // only thing a `--write` can smuggle in is a change to the *model*. What it cannot do is bump the
        // version — `tools/check_delta_compat.py` is what decides whether the change was allowed.
        std::fs::write(path, &generated).map_err(|err| {
            eyre::eyre!("cannot write {file}: {err}", file = insight::SCHEMA_FILE)
        })?;
        output::line(format_args!(
            "wrote {} ({} v{}, {} bytes)",
            insight::SCHEMA_FILE,
            insight::CONTRACT,
            insight::SCHEMA_VERSION,
            generated.len()
        ));
        return Ok(ExitCode::Ok);
    }
    if !check {
        output::line(format_args!("{generated}"));
        return Ok(ExitCode::Ok);
    }
    match std::fs::read_to_string(path) {
        Ok(checked_in) if checked_in == generated => {
            output::line(format_args!(
                "{} is what this build generates ({} v{})",
                insight::SCHEMA_FILE,
                insight::CONTRACT,
                insight::SCHEMA_VERSION
            ));
            Ok(ExitCode::Ok)
        }
        Ok(checked_in) => {
            let (line, detail) = first_difference(&checked_in, &generated);
            Err(eyre::eyre!(
                "{file} is not what this build generates: {detail} (first difference at line {line}) \
                 — regenerate with `fathom analyze --schema > {file}`; a *breaking* difference is \
                 allowed only with a `schema_version` bump, and \
                 `tools/check_delta_compat.py check` is what refuses the other kind",
                file = insight::SCHEMA_FILE
            ))
        }
        Err(err) => Err(eyre::eyre!(
            "{file} cannot be read to be checked: {err}",
            file = insight::SCHEMA_FILE
        )),
    }
}

/// The first line the two texts part on, and a short description of it.
fn first_difference(checked_in: &str, generated: &str) -> (usize, String) {
    let old: Vec<&str> = checked_in.lines().collect();
    let new: Vec<&str> = generated.lines().collect();
    for index in 0..old.len().max(new.len()) {
        let left = old.get(index).copied().unwrap_or("<end of file>");
        let right = new.get(index).copied().unwrap_or("<end of file>");
        if left != right {
            return (
                index + 1,
                format!("checked-in `{left}` against generated `{right}`"),
            );
        }
    }
    (0, "the two texts differ in length only".to_owned())
}

/// A short handle for a message, so a 40-character oid does not push the remedy off the line.
fn short(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 12 {
        return value.to_owned();
    }
    chars[..12].iter().collect::<String>() + "…"
}
