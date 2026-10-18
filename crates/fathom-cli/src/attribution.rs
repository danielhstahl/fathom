//! `fathom attribution`: who wrote it, and what to do when Fathom got it wrong (`e4.1`, `reco-w7q`).
//!
//! Three verbs, one purpose: make the authorship label *correctable*, and make the correction outlast the
//! command that made it. ADR-003 §8 H4 prices the evidence; only a human can overrule what the evidence
//! says, and the ticket's rule is that the overrule is *learned* — stored under the identity it corrects and
//! applied to every later commit carrying that signature, so the same mistake is made once.
//!
//! The write path is deliberately narrow: a correction is keyed on an identity (`identity:<address>`, or
//! `identity:<name>` when git recorded no address), never on an episode id. Episode ids are derived from
//! content and move when a lineage is rewritten; an identity is what the user was actually pointing at, and
//! it is the same key the classifier reads (see `fathom_ingest::attribute::identity_key`).
//!
//! Storing a correction re-runs the segmentation immediately, so the label moves in the same command that
//! asked for it, and the command prints which episodes moved. A correction that changed nothing says so
//! rather than pretending to have fixed something.

use std::path::Path;

use fathom_core::exit::ExitCode;
use fathom_ingest::attribute::AuthorKind;
use serde_json::json;

use crate::output;
use crate::verbs::{Pending, failed};

/// The contract id the `--json` payloads carry, in the same style as the other verbs (ADR-006 §10.1).
pub const CONTRACT: &str = "fathom.cli.attribution/1";

/// `fathom attribution correct [root] <target>`: store a human's verdict about an identity.
///
/// `target` is a rev or an oid — anything `fathom_ingest::revspec` resolves — and every identity that
/// commit carries (author, committer, and the identities named by its claim trailers) is corrected, which
/// is exactly the set `fathom_ingest::attribute::signatures` prints and the set a later commit can match
/// on. Passing `--identity` instead corrects one key directly, which is what the card's `correct` line
/// suggests when it refuses to guess.
///
/// # Errors
///
/// A `Pending` for a kind outside the vocabulary, an absent index, an unresolvable rev, or a store that
/// will not take the write.
#[allow(clippy::too_many_arguments)]
pub fn correct(
    root: &Path,
    target: &str,
    kind: &str,
    agent: Option<&str>,
    note: Option<&str>,
    identity: Option<&str>,
    as_json: bool,
) -> eyre::Result<ExitCode> {
    #[cfg(not(feature = "sqlite-bundled"))]
    {
        let _ = (root, target, kind, agent, note, identity, as_json);
        return Err(crate::verbs::feature_absent(
            "fathom attribution",
            "sqlite-bundled",
        ));
    }

    #[cfg(feature = "sqlite-bundled")]
    {
        use fathom_ingest::repo::Repository;
        use fathom_store::episode::AttributionCorrection;
        use fathom_store::spine::Store;

        let parsed = AuthorKind::parse(kind).ok_or_else(|| {
            Pending::new(
                ExitCode::Usage,
                format!("`{kind}` is not an author kind"),
                Some("the vocabulary is `agent`, `human` or `unknown`".to_owned()),
            )
        })?;

        let store_path = fathom_analyze::index::default_store_path(root);
        if !store_path.exists() {
            return Err(Pending::new(
                ExitCode::IndexDegraded,
                format!(
                    "no index at {} yet",
                    crate::sanitize::Redactor::for_root(root).path(&store_path)
                ),
                Some(crate::state::REMEDY_INDEX.to_owned()),
            )
            .into());
        }
        let mut store = Store::open(&store_path).map_err(failed)?;

        // Which identities to correct.
        let keys: Vec<String> = match identity {
            Some(key) => vec![key.to_owned()],
            None => {
                let repository = Repository::discover(root).map_err(failed)?;
                let oid = repository.resolve(target).map_err(|err| {
                    Pending::new(
                        ExitCode::Error,
                        format!("`{target}` names nothing in this repository: {err}"),
                        Some("pass a rev, an oid, or `--identity identity:<address>`".to_owned()),
                    )
                })?;
                let hex = oid.hex();
                let input = store
                    .episode_inputs()?
                    .into_iter()
                    .find(|row| row.oid == hex)
                    .ok_or_else(|| {
                        Pending::new(
                            ExitCode::IndexDegraded,
                            format!("{hex} has no row in the index"),
                            Some(crate::state::REMEDY_INDEX.to_owned()),
                        )
                    })?;
                let facts = fathom_analyze::episodes::to_facts(&input);
                let keys = fathom_ingest::attribute::signatures(std::slice::from_ref(&facts));
                if keys.is_empty() {
                    return Err(Pending::new(
                        ExitCode::Error,
                        format!("{hex} carries no identity to correct"),
                        Some("pass `--identity identity:<address>` to name one".to_owned()),
                    )
                    .into());
                }
                keys
            }
        };

        let now = crate::clock::now_millis();
        let before: Vec<(String, String)> = store
            .episodes(&fathom_store::episode::EpisodeQuery::default())?
            .iter()
            .map(|row| (row.id.clone(), row.author_kind.clone()))
            .collect();

        for key in &keys {
            store.set_attribution_correction(
                &AttributionCorrection {
                    identity: key.clone(),
                    kind: parsed.as_str().to_owned(),
                    agent: agent.unwrap_or_default().to_owned(),
                    note: note.unwrap_or_default().to_owned(),
                    created_utc: now,
                },
                now,
            )?;
        }

        // Re-segment now, with the corrections in the policy, so the label moves in this command.
        let resolved = crate::segment::from_repository(root);
        let report =
            fathom_analyze::episodes::rebuild(&mut store, &resolved.rules, now).map_err(|err| {
                Pending::new(
                    ExitCode::StoreCorrupt,
                    format!("re-segmenting after the correction: {err}"),
                    Some("fathom index".to_owned()),
                )
            })?;
        let after: Vec<(String, String)> = store
            .episodes(&fathom_store::episode::EpisodeQuery::default())?
            .iter()
            .map(|row| (row.id.clone(), row.author_kind.clone()))
            .collect();
        let changed: Vec<(String, String, String)> = before
            .iter()
            .filter_map(|(id, kind)| {
                after
                    .iter()
                    .find(|(after_id, after_kind)| after_id == id && after_kind != kind)
                    .map(|(_, after_kind)| (id.clone(), kind.clone(), after_kind.clone()))
            })
            .collect();

        if as_json {
            output::line(format_args!(
                "{}",
                json!({
                    "contract": CONTRACT,
                    "action": "correct",
                    "kind": parsed.as_str(),
                    "agent": agent,
                    "identities": keys,
                    "episodes_rewritten": report.updated + report.inserted,
                    "relabelled": changed.iter().map(|(id, from, to)| json!({
                        "episode": id, "from": from, "to": to
                    })).collect::<Vec<_>>(),
                })
            ));
            return Ok(ExitCode::Ok);
        }
        for key in &keys {
            output::line(format_args!(
                "corrected  {key} → {}{}",
                parsed.as_str(),
                agent.map(|name| format!(" · {name}")).unwrap_or_default()
            ));
        }
        if changed.is_empty() {
            output::line(format_args!(
                "no live episode changed label — the correction will apply to the next commit with that identity"
            ));
        }
        for (id, from, to) in &changed {
            output::line(format_args!("relabelled {id}: {from} → {to}"));
        }
        Ok(ExitCode::Ok)
    }
}

/// `fathom attribution list`: every stored correction, oldest key first.
///
/// # Errors
///
/// A `Pending` when there is no index to read.
pub fn list(root: &Path, as_json: bool) -> eyre::Result<ExitCode> {
    #[cfg(not(feature = "sqlite-bundled"))]
    {
        let _ = (root, as_json);
        return Err(crate::verbs::feature_absent(
            "fathom attribution",
            "sqlite-bundled",
        ));
    }

    #[cfg(feature = "sqlite-bundled")]
    {
        use fathom_store::spine::Store;
        let store_path = fathom_analyze::index::default_store_path(root);
        if !store_path.exists() {
            return Err(Pending::new(
                ExitCode::IndexDegraded,
                format!(
                    "no index at {} yet",
                    crate::sanitize::Redactor::for_root(root).path(&store_path)
                ),
                Some(crate::state::REMEDY_INDEX.to_owned()),
            )
            .into());
        }
        let store = Store::open(&store_path).map_err(failed)?;
        let rows = store.attribution_corrections()?;
        if as_json {
            output::line(format_args!(
                "{}",
                json!({
                    "contract": CONTRACT,
                    "action": "list",
                    "corrections": rows.iter().map(|row| json!({
                        "identity": row.identity,
                        "kind": row.kind,
                        "agent": (!row.agent.is_empty()).then_some(row.agent.as_str()),
                        "note": (!row.note.is_empty()).then_some(row.note.as_str()),
                        "created_utc": row.created_utc,
                    })).collect::<Vec<_>>(),
                })
            ));
            return Ok(ExitCode::Ok);
        }
        if rows.is_empty() {
            output::line(format_args!(
                "no corrections stored — `fathom attribution correct <rev> --kind human` makes one"
            ));
            return Ok(ExitCode::Ok);
        }
        for row in &rows {
            output::line(format_args!(
                "{:<40} {:<8}{}{}",
                row.identity,
                row.kind,
                if row.agent.is_empty() {
                    String::new()
                } else {
                    format!(" {}", row.agent)
                },
                if row.note.is_empty() {
                    String::new()
                } else {
                    format!("  ({})", row.note)
                }
            ));
        }
        Ok(ExitCode::Ok)
    }
}

/// `fathom attribution forget <identity>`: drop one correction.
///
/// # Errors
///
/// A `Pending` when there is no index, or when nothing was stored under that identity.
pub fn forget(root: &Path, identity: &str, as_json: bool) -> eyre::Result<ExitCode> {
    #[cfg(not(feature = "sqlite-bundled"))]
    {
        let _ = (root, identity, as_json);
        return Err(crate::verbs::feature_absent(
            "fathom attribution",
            "sqlite-bundled",
        ));
    }

    #[cfg(feature = "sqlite-bundled")]
    {
        use fathom_store::spine::Store;
        let store_path = fathom_analyze::index::default_store_path(root);
        if !store_path.exists() {
            return Err(Pending::new(
                ExitCode::IndexDegraded,
                format!(
                    "no index at {} yet",
                    crate::sanitize::Redactor::for_root(root).path(&store_path)
                ),
                Some(crate::state::REMEDY_INDEX.to_owned()),
            )
            .into());
        }
        let mut store = Store::open(&store_path).map_err(failed)?;
        let removed = store.forget_attribution_correction(identity)?;
        if !removed {
            return Err(Pending::new(
                ExitCode::Error,
                format!("no correction is stored under `{identity}`"),
                Some("fathom attribution list shows the keys".to_owned()),
            )
            .into());
        }
        let resolved = crate::segment::from_repository(root);
        let now = crate::clock::now_millis();
        fathom_analyze::episodes::rebuild(&mut store, &resolved.rules, now).map_err(failed)?;
        if as_json {
            output::line(format_args!(
                "{}",
                json!({"contract": CONTRACT, "action": "forget", "identity": identity, "removed": true})
            ));
        } else {
            output::line(format_args!(
                "forgot {identity}; the episodes were re-segmented"
            ));
        }
        Ok(ExitCode::Ok)
    }
}
