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
        let row = store.read_commit(&hex).map_err(|_| {
            Pending::new(
                ExitCode::IndexDegraded,
                format!(
                    "{} ({}) is a commit in this repository but has no row in the index",
                    short(&hex),
                    target
                ),
                Some(REMEDY_INDEX.to_owned()),
            )
        })?;

        if as_json {
            output::line(format_args!(
                "{}",
                show_json(&snapshot, &redactor, &store_path, &row, target)
            ));
            return Ok(snapshot.worst_code());
        }
        for line in show_text(&snapshot, &redactor, &store_path, &row, target) {
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
        },
        "degraded": snapshot.worst_code() != ExitCode::Ok,
        "diagnostics": snapshot.findings_json(),
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
    asked: &str,
) -> Vec<String> {
    let mut out = vec![format!("episode {} — {asked}", short(&row.oid),)];
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
    out.push(format!("  store     {}", redactor.path(store_path)));
    out.push(format!("  files     {}", row.files.len()));
    for file in &row.files {
        out.push(format!(
            "    {:<8} {:>5} {:>5}  {:<12} {}{}",
            file.change_kind,
            format!("+{}", file.n_add),
            format!("-{}", file.n_del),
            file.noise_class,
            file.new_path,
            file.old_path
                .as_ref()
                .map(|old| format!("  (from {old})"))
                .unwrap_or_default()
        ));
    }
    for line in snapshot.finding_lines() {
        out.push(line);
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
