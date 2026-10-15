//! The state the headless verbs report: the repository, the index, and what is wrong with either.
//!
//! `fathom stats`, `fathom show` and `fathom doctor` are three renderings of one read. That is the whole
//! reason this module exists: `fathom stats` is specified as "repo scale, index state, cache hit rate" and
//! `fathom doctor` as "diagnostics with remedies" — two verbs a bug report puts side by side, so they must
//! not be two implementations of "is the index OK?". ADR-004 §11 makes the same argument about the numbers
//! themselves, and ADR-006 §10.3 fixes the wording of the empty state for the same reason.
//!
//! # What "degraded" means, and what it costs
//!
//! Six states, in the order checked, each with the published exit code it maps to and the remedy the verb
//! prints. `docs/concepts/cli.md` reproduces this table; a test (`tests/test_cli_contract.py`) checks the
//! codes named here exist in `fathom_core::exit`:
//!
//! | State | Why it is a degradation | Code | Remedy |
//! |---|---|---|---|
//! | no store at all | every answer would be `unknown` | 12 | `fathom index` |
//! | store not at HEAD | the newest commit has no analysis, so the queue is yesterday's | 12 | `fathom index` |
//! | commits owed an analysis | a run was interrupted after it wrote commits | 12 | `fathom index` |
//! | a run never closed | the writer died holding the store | 12 | `fathom index` |
//! | epoch still `building` | a rebuild was started and not finished | 12 | `fathom index --rebuild` |
//! | store fails its checks | the file is not trustworthy | 4 | `fathom index --rebuild` |
//!
//! Note what is *not* here: an index with nothing in it because the repository has no commits is not
//! degraded, it is an empty repository, and the copy ADR-006 §10.3 fixes is the one printed.

use fathom_core::exit::ExitCode;
use fathom_ingest::repo::{Head, ObjectHash, RefFilter, Repository};
use serde_json::{Value, json};
use std::path::Path;

use crate::sanitize::Redactor;

/// The remedy sentence, shared so every verb prints the same words (ADR-006 §10.3: a shrug is not a
/// remedy — and neither are three near-identical sentences that drift).
#[cfg_attr(not(feature = "sqlite-bundled"), allow(dead_code))]
pub const REMEDY_INDEX: &str =
    "run `fathom index` in this repository, or `fathom index --rebuild` to throw the cache away";

/// The remedy for a store that cannot be trusted, which is the same sentence in both verbs that say it.
#[cfg_attr(not(feature = "sqlite-bundled"), allow(dead_code))]
pub const REMEDY_REBUILD: &str = "the store is disposable (ADR-004 §12): `fathom index --rebuild`";

/// One line of "here is what is wrong, and here is what to do about it".
///
/// A finding always carries its remedy, which is the difference between a diagnostic and a shrug. `exit` is
/// the *published* code the verb leaves with if it decides this finding is worth exiting for — that decision
/// belongs to the verb (`doctor` reports and exits 0 unless `--strict`), never to this struct.
#[derive(Debug, Clone)]
pub struct Finding {
    /// The stable machine name, e.g. `index_missing`.
    pub code: &'static str,
    /// Which section found it.
    pub section: &'static str,
    /// The human line.
    pub message: String,
    /// What to type.
    pub remedy: &'static str,
    /// The published code this finding maps to.
    pub exit: ExitCode,
}

impl Finding {
    #[must_use]
    pub fn new(
        code: &'static str,
        section: &'static str,
        message: impl Into<String>,
        remedy: &'static str,
        exit: ExitCode,
    ) -> Self {
        Finding {
            code,
            section,
            message: message.into(),
            remedy,
            exit,
        }
    }

    /// The finding as data — the shape `schemas/common.schema.json#/$defs/finding` pins.
    #[must_use]
    pub fn json(&self) -> Value {
        json!({
            "code": self.code,
            "section": self.section,
            "message": self.message,
            "remedy": self.remedy,
            "exit_code": self.exit.as_i32(),
            "exit_name": self.exit.name(),
        })
    }

    /// The two-line form the text renderers print: the problem, then the remedy.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        vec![
            format!("{}: {}", self.section, self.message),
            format!("  remedy: {}", self.remedy),
        ]
    }
}

/// Everything one read of one repository produced.
pub struct Snapshot {
    /// The directory the verb was pointed at, redacted for printing.
    pub root_display: String,
    /// The repository section, or the reason there is none.
    pub repository: Value,
    /// The index section, or the reason there is none.
    pub index: Value,
    /// The cache section: `null` when there is no store to count.
    pub cache: Value,
    /// What is wrong, in the order checked.
    pub findings: Vec<Finding>,
}

impl Snapshot {
    /// The highest-numbered published code among the findings, or `OK`.
    ///
    /// "Worst" is by number rather than by an invented severity scale: ADR-002 §7.3's table is already
    /// ordered roughly by how much attention a code wants, and a second ordering would be a second contract
    /// to keep in sync.
    #[must_use]
    pub fn worst_code(&self) -> ExitCode {
        self.findings
            .iter()
            .map(|finding| finding.exit.as_i32())
            .filter(|code| *code != 0)
            .max()
            .and_then(|code| ExitCode::try_from(code).ok())
            .unwrap_or(ExitCode::Ok)
    }

    /// The findings as data.
    #[must_use]
    pub fn findings_json(&self) -> Vec<Value> {
        self.findings.iter().map(Finding::json).collect()
    }

    /// The findings as printed lines.
    #[must_use]
    pub fn finding_lines(&self) -> Vec<String> {
        self.findings.iter().flat_map(Finding::lines).collect()
    }
}

/// Read the repository at `root` and the index beside it.
///
/// `deep` is `fathom doctor --deep`'s full `integrity_check`; the other verbs pass `false`, because a
/// whole-database check on every `fathom stats` is a check nobody will run twice.
#[must_use]
pub fn gather(root: &Path, deep: bool) -> Snapshot {
    let redactor = Redactor::for_root(root);
    let mut findings = Vec::new();
    let repository = describe_repository(root, &redactor, &mut findings);
    #[allow(unused_mut)]
    let mut head_oid = repository
        .get("head")
        .and_then(|head| head.get("oid"))
        .and_then(Value::as_str)
        .map(str::to_owned);

    #[cfg(feature = "sqlite-bundled")]
    let (index, cache, index_findings) = describe_index(root, &redactor, head_oid.as_deref(), deep);
    #[cfg(not(feature = "sqlite-bundled"))]
    let (index, cache, index_findings) = {
        let _ = (&deep, &mut head_oid);
        (
            json!({ "present": false, "reason": "no SQLite backend in this build" }),
            Value::Null,
            vec![Finding::new(
                "feature_absent",
                "index",
                "this binary was built without a SQLite backend, so it cannot read an index",
                "rebuild with `--features sqlite-bundled`, or use the shipped default binary",
                ExitCode::FeatureAbsent,
            )],
        )
    };

    findings.extend(index_findings);
    Snapshot {
        root_display: redactor.path(root),
        repository,
        index,
        cache,
        findings,
    }
}

/// The repository section, built from `fathom-ingest`'s own health report.
fn describe_repository(root: &Path, redactor: &Redactor, findings: &mut Vec<Finding>) -> Value {
    let repo = match Repository::discover(root) {
        Ok(repo) => repo,
        Err(err) => {
            findings.push(Finding::new(
                "not_a_repository",
                "repository",
                redactor.text(&format!(
                    "{} is not a git repository: {err}",
                    redactor.path(root)
                )),
                "run this inside a git repository, or point --root at one",
                ExitCode::NotARepository,
            ));
            return json!({ "present": false, "error": redactor.text(&err.to_string()) });
        }
    };
    let health = match repo.health() {
        Ok(health) => health,
        Err(err) => {
            findings.push(Finding::new(
                "repository_unreadable",
                "repository",
                redactor.text(&format!("the repository could not be read: {err}")),
                "check that `.git` is readable, or point --root at the work tree",
                ExitCode::NotARepository,
            ));
            return json!({ "present": false, "error": redactor.text(&err.to_string()) });
        }
    };
    let head = &health.head;
    let head_oid = head.oid().map(|oid| oid.hex());
    let refs = repo.refs(RefFilter::All).unwrap_or_default();
    let mut remote_names: Vec<String> = refs
        .iter()
        .filter_map(|reference| reference.name.strip_prefix("refs/remotes/"))
        .map(|rest| rest.split('/').next().unwrap_or(rest).to_owned())
        .collect();
    remote_names.sort();
    remote_names.dedup();
    let remotes: Vec<Value> = remote_names
        .into_iter()
        .map(|name| {
            // A remote URL is the one git config value worth printing, and the one most likely to carry a
            // credential, so it goes through the redactor rather than onto the page.
            let url = repo
                .config_string(&format!("remote.{name}.url"))
                .map(|url| redactor.secret(&url));
            json!({ "name": name, "url": url })
        })
        .collect();

    json!({
        "present": true,
        "root": redactor.path(root),
        "kind": repo.kind().as_str(),
        "bare": repo.is_bare(),
        "work_dir": repo.work_dir().map(|path| redactor.path(path)),
        "git_dir": redactor.path(repo.git_dir()),
        "object_hash": match repo.object_hash() {
            ObjectHash::Sha1 => "sha1",
            ObjectHash::Sha256 => "sha256",
            ObjectHash::Other(_) => "other",
        },
        "head": {
            "kind": match head {
                Head::Branch { .. } => "branch",
                Head::Unborn { .. } => "unborn",
                Head::Detached { .. } => "detached",
            },
            "branch": head.branch_name(),
            "oid": head_oid,
            "summary": redactor.text(&head.summary()),
        },
        "refs": {
            "total": refs.len(),
            "branches": health.refs.branches,
            "remote_branches": health.refs.remote_branches,
            "tags": health.refs.tags,
            "other": health.refs.other,
        },
        "remotes": remotes,
        "shallow": health.shallow.is_shallow,
        "partial_clone": health.partial.is_partial,
        "submodules": health.submodules,
        "notices": health
            .notices
            .iter()
            .map(|notice| format!("{notice:?}"))
            .collect::<Vec<_>>(),
    })
}

/// The index and cache sections, plus the findings they earned.
///
/// Opening the store is the only side effect, and the `exists()` check is why `fathom stats` on an
/// unindexed repository reports *that* instead of minting an empty store and reporting "indexed, 0 commits".
#[cfg(feature = "sqlite-bundled")]
#[allow(clippy::too_many_lines)]
fn describe_index(
    root: &Path,
    redactor: &Redactor,
    head_oid: Option<&str>,
    deep: bool,
) -> (Value, Value, Vec<Finding>) {
    use fathom_store::spine::Store;

    let mut findings = Vec::new();
    let path = fathom_analyze::index::default_store_path(root);
    if !path.exists() {
        findings.push(Finding::new(
            "index_missing",
            "index",
            format!("there is no index at {} yet", redactor.path(&path)),
            REMEDY_INDEX,
            ExitCode::IndexDegraded,
        ));
        return (
            json!({ "present": false, "path": redactor.path(&path) }),
            Value::Null,
            findings,
        );
    }

    let store = match Store::open(&path) {
        Ok(store) => store,
        Err(err) => {
            findings.push(Finding::new(
                "store_unusable",
                "store",
                redactor.text(&err.to_string()),
                REMEDY_REBUILD,
                ExitCode::StoreCorrupt,
            ));
            return (
                json!({
                    "present": true,
                    "path": redactor.path(&path),
                    "error": redactor.text(&err.to_string()),
                }),
                Value::Null,
                findings,
            );
        }
    };

    let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
    let wal = std::fs::metadata(path.with_extension("sqlite3-wal"))
        .map(|meta| meta.len())
        .unwrap_or(0);
    let scale = store.scale().ok();
    let runs = store.run_summary().ok();
    let inventory = store.cache_inventory().ok();
    let counters = store.counters("cache.").unwrap_or_default();
    let pending = store.pending_derivation().unwrap_or_default();
    let integrity = store.integrity_check(deep).unwrap_or_default();
    let tips = store.ref_tips().unwrap_or_default();
    let head_indexed = match head_oid {
        Some(oid) => store
            .indexed_oids(&[oid.to_owned()])
            .map(|found| !found.is_empty())
            .unwrap_or(true),
        None => true,
    };
    let epoch = store.epoch().unwrap_or(0);
    let epoch_status = store
        .meta("epoch_status")
        .ok()
        .flatten()
        .unwrap_or_else(|| "unknown".to_owned());
    let commits = scale.as_ref().map_or(0, |scale| scale.commits);

    if !integrity.is_empty() {
        findings.push(Finding::new(
            "store_corrupt",
            "store",
            format!(
                "the store failed its integrity checks: {}",
                redactor.text(&integrity.join("; "))
            ),
            REMEDY_REBUILD,
            ExitCode::StoreCorrupt,
        ));
    }
    if epoch_status == "building" {
        findings.push(Finding::new(
            "index_building",
            "index",
            format!("epoch {epoch} is still marked `building`"),
            "finish the rebuild: `fathom index --rebuild`",
            ExitCode::IndexDegraded,
        ));
    }
    if !head_indexed {
        findings.push(Finding::new(
            "index_stale",
            "index",
            format!(
                "HEAD ({}) has no commit row, so the newest work is in no answer",
                head_oid.unwrap_or("unknown")
            ),
            REMEDY_INDEX,
            ExitCode::IndexDegraded,
        ));
    }
    if !pending.is_empty() {
        findings.push(Finding::new(
            "index_incomplete",
            "index",
            format!("{} commit(s) still owe an analysis", pending.len()),
            REMEDY_INDEX,
            ExitCode::IndexDegraded,
        ));
    }
    let unclean = runs
        .as_ref()
        .map_or(0, |runs| runs.open + runs.crashed + runs.aborted);
    if unclean > 0 {
        let counts = runs.as_ref();
        findings.push(Finding::new(
            "index_run_unclean",
            "index",
            format!(
                "{} index run(s) did not finish cleanly ({} still open, {} crashed, {} aborted)",
                unclean,
                counts.map_or(0, |runs| runs.open),
                counts.map_or(0, |runs| runs.crashed),
                counts.map_or(0, |runs| runs.aborted)
            ),
            REMEDY_INDEX,
            ExitCode::IndexDegraded,
        ));
    }
    if commits == 0 && head_oid.is_some() {
        findings.push(Finding::new(
            "index_empty",
            "index",
            "the repository has commits and the index has none",
            REMEDY_INDEX,
            ExitCode::IndexDegraded,
        ));
    }

    let index = json!({
        "present": true,
        "path": redactor.path(&path),
        "size_bytes": size,
        "wal_bytes": wal,
        "schema_version": store.meta("schema_version").ok().flatten(),
        "record_schema": store.meta("key_schema").ok().flatten(),
        "store_id": store.meta("store_id").ok().flatten(),
        "epoch": epoch,
        "epoch_status": epoch_status,
        "integrity": if integrity.is_empty() { "ok" } else { "failed" },
        "integrity_problems": integrity,
        "commits": commits,
        "head_indexed": head_indexed,
        "pending_derivation": pending.len(),
        "ref_tips": tips
            .iter()
            .map(|tip| json!({"name": tip.name, "oid": tip.oid, "kind": tip.kind}))
            .collect::<Vec<_>>(),
        "last_run": runs
            .as_ref()
            .and_then(|runs| runs.last.as_ref())
            .map(|last| json!({
                "id": last.id,
                "kind": last.kind,
                "trigger": last.trigger,
                "status": last.status,
                "started_utc": last.started_utc,
                "finished_utc": last.finished_utc,
                "commits_seen": last.commits_seen,
                "commits_added": last.commits_added,
                "rows_written": last.rows_written,
                "interrupted": last.interrupted,
                "error": last.error,
            })),
        "runs": runs.as_ref().map(|runs| json!({"total": runs.total, "unclean": unclean})),
        "scale": scale.map(|scale| json!({
            "commits": scale.commits,
            "files": scale.files,
            "authors": scale.authors,
            "lines_added": scale.lines_added,
            "lines_removed": scale.lines_removed,
            "binary_files": scale.binary_files,
            "oversized_commits": scale.oversized_commits,
            "first_commit_utc": scale.first_commit_utc,
            "last_commit_utc": scale.last_commit_utc,
            "noise": scale.noise,
            "kinds": scale.kinds,
            "top_authors": scale
                .top_authors
                .iter()
                .map(|(author, count)| json!({"author": author, "commits": count}))
                .collect::<Vec<_>>(),
        })),
    });

    (index, cache_json(&counters, inventory.as_ref()), findings)
}

/// The cache section: the counters the store keeps, in the shape `fathom index --stats` prints.
///
/// [`fathom_analyze::index::stats_of`] is the only reader of the counter names, which is what lets
/// `fathom index --stats`, `fathom stats` and `fathom doctor --json` quote one hit rate rather than three
/// approximations of it (ADR-004 §11's argument for a single set of numbers).
#[cfg(feature = "sqlite-bundled")]
#[must_use]
pub fn cache_json(
    counters: &std::collections::BTreeMap<String, i64>,
    inventory: Option<&fathom_store::inventory::CacheInventory>,
) -> Value {
    use fathom_analyze::index::{ANALYZED, stats_of};

    let rows: Vec<Value> = ANALYZED
        .iter()
        .map(|id| {
            let stats = stats_of(counters, id);
            json!({
                "id": id,
                "lookups": stats.lookups,
                "hits": stats.hits,
                "negative_hits": stats.negative_hits,
                "cold": stats.cold,
                "bumped": stats.bumped,
                "epoch_stale": stats.epoch_stale,
                "puts": stats.puts,
                "computes": (stats.lookups - stats.hits - stats.negative_hits).max(0),
                "bytes": stats.bytes,
                "cost_ms": stats.cost_ms,
                "hit_rate": stats.hit_rate(),
            })
        })
        .collect();
    let lookups: i64 = rows.iter().filter_map(|row| row["lookups"].as_i64()).sum();
    let hits: i64 = rows
        .iter()
        .filter_map(|row| {
            row["hits"]
                .as_i64()
                .map(|hit| hit + row["negative_hits"].as_i64().unwrap_or(0))
        })
        .sum();
    // `None` when nothing was asked, not `0%`: `AnalyzerStats::hit_rate` makes that distinction, and a
    // status command that flattened it would make a warm cache look broken.
    let hit_rate = if lookups > 0 {
        Some(
            f64::from(u32::try_from(hits).unwrap_or(u32::MAX)) * 100.0
                / f64::from(u32::try_from(lookups).unwrap_or(u32::MAX)),
        )
    } else {
        None
    };

    json!({
        "hit_rate": hit_rate,
        "lookups": lookups,
        "hits": hits,
        // `computed` is the misses: the work this repository made the analyzers do over its life, which is
        // the number a slow warm run actually explains. Summing `puts` would not be it — a `--refresh`
        // recompute writes a row after a hit, so `puts` counts writes and not misses (§5.6).
        "computed": lookups - hits,
        "analyzers": rows,
        "rows": inventory.map_or(0, |inventory| inventory.rows),
        "stale_rows": inventory.map_or(0, |inventory| inventory.stale_epoch_rows),
        "stored_failures": inventory.map_or(0, |inventory| inventory.stored_failures),
        "registered_analyzers": inventory.map_or(0, |inventory| inventory.analyzers),
        "unknown_analyzer_rows": inventory.map_or(0, |inventory| inventory.unknown_analyzers),
        "blobs": inventory.map_or(0, |inventory| inventory.blobs),
        "blob_bytes": inventory.map_or(0, |inventory| inventory.blob_bytes),
        "blobs_inline": inventory.map_or(0, |inventory| inventory.blobs_inline),
        "blobs_on_disk": inventory.map_or(0, |inventory| inventory.blobs_on_disk),
        "served_bytes": inventory.map_or(0, |inventory| inventory.served_bytes),
    })
}
