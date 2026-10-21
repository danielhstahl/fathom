//! `fathom show` and `fathom stats`: the two verbs whose job is to answer, not to fix.
//!
//! Both are thin renderers over [`crate::state`] plus one store read each, and that thinness is the point.
//! ADR-002 §4.3 gives the composition root the argument parsing and the exit code, and gives the answer to
//! the crate that owns the data — so `fathom stats` prints the counters `fathom index --stats` wrote, and
//! `fathom show` prints the rows `fathom index` stored, with no re-derivation anywhere. Where an episode's
//! own identity will come from (`e1.6`) the code says so rather than inventing a grouping that the episode
//! ticket will have to undo.
//!
//! Every figure printed here is *stated*: a missing store is `present: false` with a remedy, not a zero; a
//! cache that was never asked is `hit_rate: null`, not `0%`. ADR-006 §10.3 calls that "a value, not a
//! blank", and the headless surface is where it matters most because a script cannot notice a blank.

// A binary built without SQLite refuses these verbs on the first line of each one, so every shape behind
// that refusal is dead code in that build — which is what the feature gate is for (ADR-002 §6.1), not an
// oversight to delete.
#![cfg_attr(not(feature = "sqlite-bundled"), allow(dead_code))]

// `output` is only reached through the store-backed paths; a `minimal` build refuses those verbs before
// it prints anything, so the import is deliberately conditional rather than the whole verb being duplicated.
#[cfg(feature = "sqlite-bundled")]
use crate::output;
use crate::state::Snapshot;
use fathom_core::exit::ExitCode;
use serde_json::json;

/// The contract id each `--json` payload carries, so a consumer can switch on the shape it got rather than
/// on the shape it hoped for. Mirrored by `schemas/<verb>.schema.json`, and checked by the round-trip test.
pub const SHOW_CONTRACT: &str = "fathom.cli.show/1";
/// The `fathom stats` contract id.
pub const STATS_CONTRACT: &str = "fathom.cli.stats/1";
/// The `fathom doctor` contract id.
pub const DOCTOR_CONTRACT: &str = "fathom.cli.doctor/1";
/// The `fathom index --json` contract id.
pub const INDEX_CONTRACT: &str = "fathom.cli.index/1";
/// The `fathom export --json` contract id.
pub const EXPORT_CONTRACT: &str = "fathom.cli.export/1";
/// The record `fathom tui` leaves in `.fathom/frame-report.json`, and the object `fathom tui --stats`
/// prints (`reco-08r`).
///
/// Not a verb payload — no schema file pins it, because it is the shell's own measurement rather than a
/// query answer — but the same discipline applies: a consumer switches on the shape it got.
pub const FRAME_CONTRACT: &str = "fathom.cli.frame/1";

/// `fathom show <episode|rev>`: one episode, in full, from the index.
///
/// The handle is a revision or an object id — anything [`fathom_ingest::revspec`] resolves — because that
/// is what exists before `e1.6` gives episodes their own identity. The failure modes are separated on
/// purpose: a rev that names nothing in the repository is the user's typo (exit 1), while a rev that names
/// a commit the index has never heard of is the index's problem (exit 12, remedy `fathom index`).
pub fn show(root: &str, target: &str, as_json: bool) -> eyre::Result<ExitCode> {
    #[cfg(not(feature = "sqlite-bundled"))]
    {
        let _ = (root, target, as_json);
        return Err(crate::verbs::feature_absent(
            "fathom show",
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
        let repository = Repository::discover(root_path).map_err(failed)?;
        let oid = repository.resolve(target).map_err(|err| {
            Pending::new(
                ExitCode::Error,
                format!("`fathom show {target}` names nothing in this repository: {err}"),
                Some(
                    "the grammar is `HEAD`, `@`, `main~3`, `HEAD^2`, `v1.2^{}`, a full oid, or a hex prefix"
                        .to_owned(),
                ),
            )
        })?;
        let hex = oid.hex();
        let store_path = fathom_analyze::index::default_store_path(root_path);
        if !store_path.exists() {
            return Err(Pending::new(
                ExitCode::IndexDegraded,
                format!(
                    "no index at {} yet, so nothing can be shown",
                    redactor.path(&store_path)
                ),
                Some(REMEDY_INDEX.to_owned()),
            )
            .into());
        }
        let store = Store::open(&store_path).map_err(failed)?;
        // Which episode this commit belongs to, if any. `e1.3` put the grouping, the boundary rule and the
        // confidence in the store, and this is the surface that shows them: a reviewer who disagrees with a
        // boundary has to be able to see which rule drew it before they can go and override it.
        let episode = store.episode_for_commit(&hex).map_err(|err| {
            Pending::new(
                ExitCode::StoreCorrupt,
                format!("reading the episode for {target}: {err}"),
                Some("fathom index --rebuild".to_owned()),
            )
        })?;
        let row = store.read_commit(&hex).map_err(|_| {
            // "No row" has two causes and they want different sentences: this commit was never indexed, or
            // the index tried and the object database would not serve it. The second is what a damaged or
            // filter-cloned repository looks like from the reader's side, and telling that reviewer to run
            // `fathom index` — the command that just failed for the same reason — is the dead end this branch
            // exists to close.
            match store.unreadable_commit(&hex).ok().flatten() {
                Some(reason) => Pending::new(
                    ExitCode::IndexDegraded,
                    format!(
                        "{} ({}) could not be read out of the object database: {}",
                        short(&hex),
                        target,
                        redactor.text(&reason)
                    ),
                    Some(state::REMEDY_UNREADABLE.to_owned()),
                ),
                None => Pending::new(
                    ExitCode::IndexDegraded,
                    format!(
                        "{} ({}) is a commit in this repository but has no row in the index",
                        short(&hex),
                        target
                    ),
                    Some(REMEDY_INDEX.to_owned()),
                ),
            }
        })?;

        if as_json {
            output::line(format_args!(
                "{}",
                show_json(&snapshot, &redactor, &store_path, &row, &episode, target)
            ));
            return Ok(snapshot.worst_code());
        }
        // Paths and the commit go through the link printer (`e5.8`): clickable where the terminal
        // advertised, byte-identical to these same lines where it did not.
        let links = crate::links::Printer::for_root(root_path);
        for line in show_text(
            &snapshot,
            &redactor,
            &store_path,
            &row,
            &episode,
            target,
            &links,
        ) {
            output::line(format_args!("{line}"));
        }
        Ok(snapshot.worst_code())
    }
}

/// The short oid every human-facing line uses, in the seven-character form git uses.
fn short(hex: &str) -> String {
    hex.chars().take(7).collect()
}

/// The episode as data.
#[cfg(feature = "sqlite-bundled")]
fn show_json(
    snapshot: &Snapshot,
    redactor: &crate::sanitize::Redactor,
    store_path: &std::path::Path,
    row: &fathom_store::spine::CommitRow,
    episode: &Option<fathom_store::episode::EpisodeRow>,
    asked: &str,
) -> serde_json::Value {
    let files = row
        .files
        .iter()
        .map(|file| {
            json!({
                "path": file.new_path,
                "old_path": file.old_path,
                "kind": file.change_kind,
                "add": file.n_add,
                "del": file.n_del,
                "binary": file.is_binary,
                "path_class": file.path_class,
                "taxonomy_version": file.taxonomy_version,
                "noise_class": file.noise_class,
                "hunk_headers": file.hunk_headers,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "contract": SHOW_CONTRACT,
        "asked": asked,
        "repository": snapshot.repository,
        "store": redactor.path(store_path),
        "episode": {
            "oid": row.oid,
            "short_oid": short(&row.oid),
            "tree_oid": row.tree_oid,
            "parents": row.parent_oids,
            "author": {
                "name": row.author_name,
                "email": row.author_email,
                "utc": row.author_utc,
            },
            "committer_utc": row.committer_utc,
            "message_head": row.message_head,
            "message_hash": row.message_hash,
            "trailers": row.trailers,
            "scale": {
                "files": row.n_files,
                "add": row.n_add,
                "del": row.n_del,
                "binary": row.n_binary,
                "oversized": row.oversized,
            },
            "files": files,
            "noise": noise_split(row),
            "identity": identity_json(episode),
        },
        "degraded": snapshot.worst_code() != ExitCode::Ok,
        "diagnostics": snapshot.findings_json(),
    })
}

/// The episode's own identity: which grouping this commit landed in, which rule drew its boundary, and
/// how much the tool trusts that call. `null` when the store has no episode for the commit — an index
/// written before `e1.3`, or a commit the walk can no longer reach — because "we do not have that" and
/// "the boundary is weak" are different statements (ADR-006 §10.3).
#[cfg(feature = "sqlite-bundled")]
fn identity_json(episode: &Option<fathom_store::episode::EpisodeRow>) -> serde_json::Value {
    use fathom_store::episode::EpisodeRow;
    let Some(EpisodeRow {
        id,
        fingerprint,
        boundary_rule,
        boundary_confidence,
        author_kind,
        author_confidence,
        session_id,
        turn_id,
        overridden,
        live,
        lineage_ref,
        first_oid,
        last_oid,
        members,
        n_files,
        n_add,
        n_del,
        first_utc,
        last_utc,
        ..
    }) = episode
    else {
        return serde_json::Value::Null;
    };
    json!({
        "id": id,
        "fingerprint": fingerprint,
        "boundary_rule": boundary_rule,
        "boundary_confidence": boundary_confidence,
        "author_kind": author_kind,
        "author_confidence": author_confidence,
        "attribution": attribution_json(episode),
        "intent": intent_json(episode),
        "session_id": session_id,
        "turn_id": turn_id,
        // ADR-003 §9.1 / §18.2 D4: the id is `<session_id>:<turn_index>`, and the number is printed as
        // well so a consumer does not re-split a string it has no business parsing. `null` when there is
        // no turn, or when the transport carried something that is not an index — `r6` is a ref, not a
        // turn number, and pretending otherwise would be inventing a fact.
        "turn_index": fathom_ingest::segment::turn_index(turn_id.as_deref()),
        "overridden": overridden,
        "live": live,
        "lineage_ref": lineage_ref,
        "first_oid": first_oid,
        "last_oid": last_oid,
        "commits": members.len(),
        "members": members,
        "files": n_files,
        "add": n_add,
        "del": n_del,
        "first_utc": first_utc,
        "last_utc": last_utc,
    })
}

/// How much of this episode the taxonomy did not trust, as one line of numbers.
#[cfg(feature = "sqlite-bundled")]
fn noise_split(row: &fathom_store::spine::CommitRow) -> serde_json::Value {
    let mut out: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
    for file in &row.files {
        *out.entry(file.noise_class.clone()).or_insert(0) += 1;
    }
    serde_json::Value::Object(
        out.into_iter()
            .map(|(class, count)| (class, serde_json::Value::from(count)))
            .collect(),
    )
}

/// The episode as text: a card, in the order a reviewer reads one.
#[cfg(feature = "sqlite-bundled")]
fn show_text(
    snapshot: &Snapshot,
    redactor: &crate::sanitize::Redactor,
    store_path: &std::path::Path,
    row: &fathom_store::spine::CommitRow,
    episode: &Option<fathom_store::episode::EpisodeRow>,
    asked: &str,
    links: &crate::links::Printer,
) -> Vec<String> {
    let mut out = vec![format!("episode {} — {asked}", links.commit(&row.oid),)];
    out.push(format!("  message   {}", row.message_head));
    out.push(format!(
        "  author    {} <{}>  {}",
        row.author_name,
        row.author_email,
        format_utc(row.author_utc)
    ));
    out.push(format!("  tree      {}", row.tree_oid));
    out.push(format!(
        "  parents   {}",
        if row.parent_oids.is_empty() {
            "(root commit)".to_owned()
        } else {
            row.parent_oids
                .iter()
                .map(|oid| short(oid))
                .collect::<Vec<_>>()
                .join(" ")
        }
    ));
    if !row.trailers.is_empty() {
        let trailers = row
            .trailers
            .iter()
            .map(|(key, value)| format!("{key}: {value}"))
            .collect::<Vec<_>>()
            .join(", ");
        out.push(format!("  trailers  {trailers}"));
    }
    out.push(format!(
        "  scale     {} file(s), +{} -{}, {} binary",
        row.n_files, row.n_add, row.n_del, row.n_binary
    ));
    out.extend(identity_lines(episode));
    out.push(format!("  store     {}", redactor.path(store_path)));
    out.push(format!("  files     {}", row.files.len()));
    for file in &row.files {
        out.push(format!(
            "    {:<8} {:>5} {:>5}  {:<12} {}{}",
            file.change_kind,
            format!("+{}", file.n_add),
            format!("-{}", file.n_del),
            file.noise_class,
            // The path last and unaligned on purpose: a link's escape sequences are zero-width, and a
            // column that has to survive them is a column worth not padding.
            links.path(&file.new_path),
            file.old_path
                .as_ref()
                .map(|old| format!("  (from {})", links.path(old)))
                .unwrap_or_default()
        ));
    }
    for line in snapshot.finding_lines() {
        out.push(line);
    }
    out
}

/// The intent block for `--json` (`e4.2`, `reco-c54`): the stored document, verbatim.
///
/// `null` when the row carries no intent document at all — a store written before migration 5. That is
/// deliberately *not* the same JSON as an episode whose intent was looked for and not found, which carries
/// `{"state": "none", …}`: a consumer must be able to tell "Fathom never asked" from "nothing answered",
/// because only the second one is a fact about the work.
fn intent_json(episode: &Option<fathom_store::episode::EpisodeRow>) -> serde_json::Value {
    let Some(row) = episode.as_ref() else {
        return serde_json::Value::Null;
    };
    if row.intent.is_empty() {
        return serde_json::Value::Null;
    }
    serde_json::from_str(&row.intent).unwrap_or_else(|_| {
        json!({
            "schema": "fathom-intent/1",
            "state": row.intent_state,
            "source": row.intent_source,
            "confidence": row.intent_confidence,
            "text": row.intent_text,
            "plan": [],
            "scope": [],
            "reason": "the stored intent document could not be read; these are the scalars beside it",
        })
    })
}

/// The attribution block for `--json` (`e4.1`, `reco-w7q`): the case behind `author_kind`.
///
/// `null` when the row carries no attribution detail — a store written before migration 4, which stored
/// the label and nothing else. That is a different statement from an empty evidence list, in the same way
/// [`identity_json`]'s `null` is a different statement from a weak boundary.
#[cfg(feature = "sqlite-bundled")]
fn attribution_json(episode: &Option<fathom_store::episode::EpisodeRow>) -> serde_json::Value {
    use fathom_store::episode::EpisodeRow;
    let Some(EpisodeRow {
        author_kind,
        author_confidence,
        attribution_source,
        attribution_agent,
        attribution_evidence,
        attribution_reason,
        ..
    }) = episode
    else {
        return serde_json::Value::Null;
    };
    let evidence: Vec<fathom_analyze::insight::AttributionEvidence> =
        serde_json::from_str(attribution_evidence).unwrap_or_default();
    if attribution_source.is_empty() && evidence.is_empty() {
        return serde_json::Value::Null;
    }
    json!({
        "kind": author_kind,
        "source": attribution_source,
        "agent": (!attribution_agent.is_empty()).then_some(attribution_agent.as_str()),
        "confidence": author_confidence,
        "reason": (!attribution_reason.is_empty()).then_some(attribution_reason.as_str()),
        "evidence": evidence,
    })
}

/// The identity lines of the card, in the order a reviewer reads them: what is this unit, what says so,
/// how much do we trust it. Empty when the store has no episode for the commit, with one line saying so —
/// an absent block is a blank, and a blank is what this surface is not allowed to print.
#[cfg(feature = "sqlite-bundled")]
fn identity_lines(episode: &Option<fathom_store::episode::EpisodeRow>) -> Vec<String> {
    use fathom_store::episode::EpisodeRow;
    let Some(row) = episode else {
        return vec![
            "  grouping  no episode in the index for this commit; run `fathom index`".to_owned(),
        ];
    };
    let EpisodeRow {
        id,
        boundary_rule,
        boundary_confidence,
        author_kind,
        author_confidence,
        overridden,
        session_id,
        turn_id,
        members,
        ..
    } = row;
    let mut out = vec![format!(
        "  grouping  {id} · {} commit(s) · boundary {boundary_rule} {:.2} · author {author_kind} {:.2}",
        members.len(),
        boundary_confidence,
        author_confidence
    )];
    // The intent block (`e4.2`, `reco-c54`) — through the same renderer the delta document uses, so the
    // three places an intent appears say it the same way. A row with no document says that too: the ticket's
    // rule is that a missing intent is shown, not blanked.
    match fathom_ingest::intent::Intent::from_json(&row.intent) {
        Some(intent) => {
            for line in fathom_analyze::insight::intent_block(&intent) {
                out.push(format!("  {line}"));
            }
        }
        None if row.intent.is_empty() => out.push(
            "  intent      no intent document on this row — `fathom index` captures it from the hooks or \
             the `Intent:`/`Task:` trailers"
                .to_owned(),
        ),
        None => out.push(format!(
            "  intent      {} · the stored document could not be read",
            row.intent_state
        )),
    }
    // The attribution block (`e4.1`, `reco-w7q`): the rung that decided, the agent it named, and — when the
    // label was refused — the reason for refusing it. A card that printed `?` with no reason is the silent
    // bucket the ticket forbids, and a card that printed `agent` with no evidence could not be argued with.
    let evidence: Vec<fathom_analyze::insight::AttributionEvidence> =
        serde_json::from_str(&row.attribution_evidence).unwrap_or_default();
    if !row.attribution_source.is_empty() || !evidence.is_empty() {
        let agent = if row.attribution_agent.is_empty() {
            "—"
        } else {
            row.attribution_agent.as_str()
        };
        out.push(format!(
            "  attribution  {author_kind} · {agent} · {author_confidence:.2} via {}",
            if row.attribution_source.is_empty() {
                "(rung not recorded)"
            } else {
                &row.attribution_source
            }
        ));
        for item in evidence.iter().take(3) {
            out.push(format!(
                "  evidence    {} {:.2} · {}",
                item.source, item.confidence, item.detail
            ));
        }
        if !row.attribution_reason.is_empty() {
            out.push(format!("  why         {}", row.attribution_reason));
            out.push(
                "  correct     fathom attribution correct <identity> --kind agent|human — `fathom attribution list` prints the keys stored so far".to_owned(),
            );
        }
    }
    if *overridden {
        out.push(
            "  provenance  this boundary was forced from fathom.toml, not derived from the history"
                .to_owned(),
        );
    }
    if let Some(session) = session_id {
        out.push(format!(
            "  session   {session}{}",
            turn_id
                .as_deref()
                .map(|turn| format!(" · turn {turn}"))
                .unwrap_or_default()
        ));
    }
    out
}

/// Unix millis as an ISO-8601 UTC stamp, without pulling in a date crate.
///
/// The civil-date arithmetic is the standard days-from-civil algorithm; it is here rather than in
/// `fathom-core` because formatting is a presentation concern and the CLI is the only crate allowed to
/// care about how a number looks.
#[must_use]
pub fn format_utc(millis: i64) -> String {
    let seconds = millis.div_euclid(1000);
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        (rest % 3600) / 60,
        rest % 60
    )
}

/// Days-from-civil (Howard Hinnant's algorithm), the inverse of `days_from_civil`.
fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `fathom stats`: repo scale, index state, cache hit rate.
pub fn stats(root: &str, as_json: bool) -> eyre::Result<ExitCode> {
    #[cfg(not(feature = "sqlite-bundled"))]
    {
        let _ = (root, as_json);
        return Err(crate::verbs::feature_absent(
            "fathom stats",
            "sqlite-bundled",
        ));
    }

    #[cfg(feature = "sqlite-bundled")]
    {
        use crate::state;
        let snapshot = state::gather(std::path::Path::new(root), false);
        if as_json {
            output::line(format_args!("{}", stats_json(&snapshot)));
            return Ok(snapshot.worst_code());
        }
        for line in stats_text(&snapshot) {
            output::line(format_args!("{line}"));
        }
        Ok(snapshot.worst_code())
    }
}

/// The `fathom stats --json` payload.
pub fn stats_json(snapshot: &Snapshot) -> serde_json::Value {
    json!({
        "contract": STATS_CONTRACT,
        "root": snapshot.root_display,
        "repository": snapshot.repository,
        "index": snapshot.index,
        "cache": snapshot.cache,
        "degraded": snapshot.worst_code() != ExitCode::Ok,
        "exit_code": snapshot.worst_code().as_i32(),
        "exit_name": snapshot.worst_code().name(),
        "diagnostics": snapshot.findings_json(),
    })
}

/// The text rendering, one block per section, in the order a person asks about them.
pub fn stats_text(snapshot: &Snapshot) -> Vec<String> {
    let mut out = Vec::new();
    let repo = &snapshot.repository;
    if repo["present"].as_bool() == Some(true) {
        out.push(format!(
            "repository {}",
            repo["root"].as_str().unwrap_or(".")
        ));
        out.push(format!(
            "  head      {} ({} refs)",
            repo["head"]["summary"].as_str().unwrap_or("unknown"),
            repo["refs"]["total"].as_u64().unwrap_or(0)
        ));
        out.push(format!(
            "  shape     {} repo, {} submodule(s), shallow {}, partial {}",
            repo["kind"].as_str().unwrap_or("unknown"),
            repo["submodules"].as_u64().unwrap_or(0),
            yes_no(repo["shallow"].as_bool()),
            yes_no(repo["partial_clone"].as_bool()),
        ));
        // What the repository says about itself that is not a finding — a hundred branches, an object store
        // on another filesystem. Said once here so the status line is not the only place to read it.
        for note in repo["notes"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
            if let Some(note) = note.as_str() {
                out.push(format!("  note      {note}"));
            }
        }
    } else {
        out.push(format!(
            "repository {}",
            repo["error"].as_str().unwrap_or("not a git repository")
        ));
    }

    let index = &snapshot.index;
    if index["present"].as_bool() == Some(true) {
        let scale = &index["scale"];
        out.push(format!("index {}", index["path"].as_str().unwrap_or("?")));
        out.push(format!(
            "  scale     {} commit(s), {} file row(s), {} author(s)",
            scale["commits"].as_u64().unwrap_or(0),
            scale["files"].as_u64().unwrap_or(0),
            scale["authors"].as_u64().unwrap_or(0)
        ));
        // The other half of the one read `state.rs` makes: what the scale line did *not* count, and the verb
        // that reclaims it. A rewrite leaves rows behind by design (ADR-004 §6.2); what `reco-218` was is a
        // report that added them to the repository's size and called the difference nothing.
        let dead = index["dead_commits"].as_u64().unwrap_or(0);
        let unjudged = index["dead_unjudged"].as_u64().unwrap_or(0);
        if dead > 0 {
            out.push(format!(
                "  dead      {dead} of {} commit row(s) are rows for commits no ref reaches \
                 ({} file row(s)) — `fathom index --prune` removes them",
                index["commits_retained"].as_u64().unwrap_or(0),
                index["dead_files"].as_u64().unwrap_or(0),
            ));
        }
        if unjudged > 0 {
            // Not a claim about dead rows — the opposite: the store could not tell. Usually a shallow clone
            // or an unreadable object, each with its own finding below.
            out.push(format!(
                "  reach     {} commit row(s) the store could not place: {}",
                unjudged,
                index["dead_unjudged_reason"]
                    .as_str()
                    .unwrap_or("the stored parent chain has a break in it")
            ));
        }
        out.push(format!(
            "  churn     +{} -{} over {} to {}",
            scale["lines_added"].as_u64().unwrap_or(0),
            scale["lines_removed"].as_u64().unwrap_or(0),
            scale["first_commit_utc"]
                .as_i64()
                .map_or_else(|| "—".to_owned(), format_utc),
            scale["last_commit_utc"]
                .as_i64()
                .map_or_else(|| "—".to_owned(), format_utc),
        ));
        out.push(format!(
            "  state     epoch {} {}, integrity {}, {} commit(s) owed an analysis",
            index["epoch"].as_u64().unwrap_or(0),
            index["epoch_status"].as_str().unwrap_or("?"),
            index["integrity"].as_str().unwrap_or("?"),
            index["pending_derivation"].as_u64().unwrap_or(0),
        ));
        if let Some(last) = index["last_run"].as_object() {
            out.push(format!(
                "  last run  {} {} ({}), {} commit(s) added, {}",
                last["kind"].as_str().unwrap_or("?"),
                last["trigger"].as_str().unwrap_or("?"),
                last["status"].as_str().unwrap_or("?"),
                last["commits_added"].as_u64().unwrap_or(0),
                last["finished_utc"]
                    .as_i64()
                    .map_or_else(|| "still open".to_owned(), format_utc),
            ));
        } else {
            out.push("  last run  none recorded".to_owned());
        }
        if let Some(noise) = index["scale"]["noise"].as_object()
            && !noise.is_empty()
        {
            let parts = noise
                .iter()
                .map(|(class, count)| format!("{class} {}", count.as_u64().unwrap_or(0)))
                .collect::<Vec<_>>()
                .join(", ");
            out.push(format!("  noise     {parts}"));
        }
    } else if index.get("reason").is_some() {
        out.push(format!(
            "index {}",
            index["reason"].as_str().unwrap_or("unavailable")
        ));
    } else {
        out.push(format!(
            "index {} — no index yet",
            index["path"].as_str().unwrap_or(".fathom/index.sqlite3")
        ));
    }

    if let Some(cache) = snapshot.cache.as_object() {
        out.push("cache".to_owned());
        out.push(format!(
            "  hit rate  {} over {} lookups, {} cache row(s), {} blob(s)",
            cache["hit_rate"].as_f64().map_or_else(
                || "n/a (nothing asked)".to_owned(),
                |rate| format!("{rate:.1}%")
            ),
            cache["lookups"].as_u64().unwrap_or(0),
            cache["rows"].as_u64().unwrap_or(0),
            cache["blobs"].as_u64().unwrap_or(0),
        ));
        for row in cache["analyzers"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            out.push(format!(
                "  {:<16} {:>7} {:>7} {:>6} {:>7} {}",
                row["id"].as_str().unwrap_or("?"),
                format!("{} lookups", row["lookups"].as_u64().unwrap_or(0)),
                format!("{} hits", row["hits"].as_u64().unwrap_or(0)),
                format!("{} cold", row["cold"].as_u64().unwrap_or(0)),
                format!("{} puts", row["puts"].as_u64().unwrap_or(0)),
                row["hit_rate"].as_f64().map_or_else(
                    || "hit rate n/a".to_owned(),
                    |rate| format!("hit rate {rate:.1}%")
                ),
            ));
        }
    }

    out.extend(snapshot.finding_lines());
    if snapshot.findings.is_empty() {
        out.push("nothing to fix — the index describes HEAD and every run closed".to_owned());
    }
    out
}

fn yes_no(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "yes",
        Some(false) => "no",
        None => "unknown",
    }
}
