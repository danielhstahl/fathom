//! The verbs: parse, dispatch, and leave with a published exit code (ADR-002 §4.3, §7.1, §7.3).
//!
//! Each verb here is a *stub that knows its own name*, and that is the design rather than a placeholder
//! for it: the verb surface, the refusal it returns before it has an answer, and the exit code it leaves
//! with are the contract `e1.10` fills in. A stub returns a typed failure and never a panic (§7.2, §9
//! rule 2), and the failure names the ticket that owns the work, so "not yet" is a pointer and not a
//! wall.

use crate::features;
use crate::output;
use clap::{Args, Parser, Subcommand};
use fathom_core::error::Diagnosable;
use fathom_core::exit::ExitCode;

/// The whole command line.
#[derive(Parser, Debug)]
#[command(
    name = "fathom",
    about = "Review an agent-driven repository before it merges.",
    version,
    propagate_version = true
)]
pub struct Cli {
    /// The verb to run. With no verb, fathom opens the terminal UI — if it was compiled in.
    #[command(subcommand)]
    pub verb: Option<Verb>,
}

/// The verbs fathom answers to. ADR-005 §6 owns the full list and `e1.10` ships it; the ones here are
/// the ones the exit-code contract and the feature gates need in order to be demonstrable at all.
#[derive(Subcommand, Debug)]
pub enum Verb {
    /// Index the repository so every other verb has something to read
    Index(IndexArgs),
    /// Show one episode in full
    Show(ShowArgs),
    /// Report what the index holds
    Stats,
    /// Run the checks CI cares about; findings above the threshold exit 7
    Check,
    /// Describe this build, its features and its privacy posture
    Doctor(DoctorArgs),
    /// Write a review artefact for a pull request or an audit
    Export(ExportArgs),
    /// Speak MCP to a coding agent
    Mcp(McpArgs),
    /// Watch the repository and index as it changes
    Watch(WatchArgs),
}

/// `fathom index`
#[derive(Args, Debug)]
pub struct IndexArgs {
    /// The repository to index.
    #[arg(default_value = ".")]
    pub root: String,
    /// Print the per-analyzer cache table (ADR-004 §4.8).
    #[arg(long)]
    pub stats: bool,
    /// Print the run report as JSON on stdout, and nothing else there.
    #[arg(long)]
    pub json: bool,
    /// Look up every stored unit, and recompute the ones that hit too.
    ///
    /// This is the pass that *measures* ADR-004 §6's invalidation matrix instead of trusting it: after a
    /// version or config bump it prints exactly the rows that moved, and after an unremarkable commit it
    /// prints 100% hits.
    #[arg(long)]
    pub refresh: bool,
    /// Do not analyse uncommitted paths (ADR-004 §6.4).
    #[arg(long = "committed-only")]
    pub committed_only: bool,
}

/// `fathom show`
#[derive(Args, Debug)]
pub struct ShowArgs {
    /// The episode to show.
    pub episode: String,
}

/// `fathom doctor`
#[derive(Args, Debug)]
pub struct DoctorArgs {
    /// Print the network-feature and consent table.
    #[arg(long)]
    pub privacy: bool,
    /// Print the same report as JSON, for a machine.
    #[arg(long)]
    pub json: bool,
    /// The repository to report the watcher for.
    #[arg(long, default_value = ".")]
    pub root: String,
}

/// `fathom watch`
///
/// The tuning flags are [`fathom_ingest::watch::Options`] fields with names a human can type, and their
/// defaults are the crate's rather than a second copy here: a flag exists because a repository on a laptop
/// and a repository in a container do not agree about what "quiet" means, not because a flag was cheap.
/// The rest are the output contract — what to print, where the heartbeat goes, and when to stop.
#[derive(Args, Debug)]
pub struct WatchArgs {
    /// The repository to watch.
    #[arg(default_value = ".")]
    pub root: String,
    /// Print one JSON object per state change on stdout, and nothing else there.
    #[arg(long)]
    pub json: bool,
    /// Index once, then exit: what a script or a CI check wants from `watch`.
    #[arg(long)]
    pub once: bool,
    /// Stop after this many looks at the filesystem.
    #[arg(long)]
    pub max_cycles: Option<u64>,
    /// Stop after this many seconds.
    #[arg(long)]
    pub deadline: Option<u64>,
    /// Which change source to use: `auto`, `notify` or `poll`.
    #[arg(long, default_value = "auto")]
    pub backend: String,
    /// The debounced polling interval in milliseconds.
    #[arg(long)]
    pub interval_ms: Option<u64>,
    /// The quiescence window in milliseconds: how long the filesystem must be quiet before a change is read.
    #[arg(long)]
    pub debounce_ms: Option<u64>,
    /// The liveness beat in milliseconds: how often the state is rewritten while nothing changes.
    #[arg(long)]
    pub heartbeat_ms: Option<u64>,
    /// Analysis units per index chunk — the watcher's slice of the CPU.
    #[arg(long, default_value_t = 2_000)]
    pub chunk_units: usize,
    /// Also re-index uncommitted paths when only the index moved.
    #[arg(long)]
    pub worktree: bool,
    /// Do not write `.fathom/watch/status.json`.
    #[arg(long)]
    pub no_status_file: bool,
    /// Print nothing to stdout but errors; the state machine still goes to the status file.
    #[arg(long)]
    pub quiet: bool,
}

/// `fathom export`
#[derive(Args, Debug)]
pub struct ExportArgs {
    /// The artefact format: markdown, html or json.
    #[arg(long, default_value = "markdown")]
    pub format: String,
    /// The file stem to write.
    #[arg(long, default_value = "fathom-review")]
    pub out: String,
}

/// `fathom mcp`
#[derive(Args, Debug)]
pub struct McpArgs {
    /// What to do with the MCP server.
    #[command(subcommand)]
    pub action: McpAction,
}

/// `fathom mcp` actions.
#[derive(Subcommand, Debug)]
pub enum McpAction {
    /// Serve the tool surface over stdio, which opens no socket.
    Serve,
}

/// Run one verb. The return type is the process contract, not a `Result` a caller forgets to check.
///
/// # Errors
///
/// [`Pending`], or anything a verb chose to raise, when the verb cannot be completed; `main` maps that to
/// an exit code at the edge, which is the only place the two are joined.
pub fn dispatch(verb: Option<&Verb>) -> eyre::Result<ExitCode> {
    match verb {
        None => open_terminal(),
        Some(Verb::Index(args)) => index(args),
        Some(Verb::Show(args)) => Err(not_implemented(
            &format!("fathom show {}", args.episode),
            "e1.6",
        )),
        Some(Verb::Stats) => Err(not_implemented("fathom stats", "e1.10")),
        Some(Verb::Check) => Err(not_implemented("fathom check", "e6.6")),
        Some(Verb::Doctor(args)) => doctor(args),
        Some(Verb::Export(args)) => export(args),
        Some(Verb::Mcp(args)) => mcp(args),
        Some(Verb::Watch(args)) => watch(args),
    }
}

fn open_terminal() -> eyre::Result<ExitCode> {
    if !cfg!(feature = "tui") {
        return Err(feature_absent("running with no verb", "tui"));
    }
    Err(pending(
        ExitCode::Error,
        "the terminal UI is compiled in but the shell is not implemented yet",
        Some("owned by e1.5 (reco-v6t); `fathom --help` lists what works today".to_owned()),
    ))
}

/// `fathom index`: the whole of ADR-004 §6 behind one verb.
///
/// The verb owns three things and no logic: the clock (this is the crate ADR-002 §4.4 lets read it, and
/// [`clock`] reads it once for the run), the store's location (§3: `.fathom/index.sqlite3` under the
/// repository), and what to print. The walk, the cache keys and the counters are `fathom-analyze`'s and
/// `fathom-store`'s, so `--stats` and `fathom stats` cannot tell different stories.
///
/// Ctrl-c is still the default disposition: the process dies where it stands, and the one thing `e1.9`
/// makes sure of is that a watcher killed that way is *reported* as stopped by `fathom doctor` rather than
/// as running (its heartbeat names a pid that is gone). A clean cancel — the writer lease released, the run
/// stamped cancelled, exit 6 — needs a signal handler, and this crate forbids `unsafe`, so the handler
/// belongs to the daemon process that owns the terminal (`reco-kvk`). The driver's cancel handle is there
/// and counted either way (`--stats` shows a cancelled run as a stated partial, exit 6).
#[cfg(feature = "sqlite-bundled")]
fn index(args: &IndexArgs) -> eyre::Result<ExitCode> {
    use fathom_analyze::index::{self, Scope, Worktree};
    use fathom_ingest::record::Unclassified;
    use fathom_ingest::repo::Repository;
    use fathom_store::spine::Store;

    let root = std::path::Path::new(&args.root);
    let repository = Repository::discover(root).map_err(failed)?;
    let store_path = index::default_store_path(root);
    if let Some(dir) = store_path.parent() {
        std::fs::create_dir_all(dir).map_err(|err| {
            Pending::new(
                ExitCode::Error,
                format!("cannot create {} for the store: {err}", dir.display()),
                None,
            )
        })?;
    }
    let mut store = Store::open(&store_path).map_err(failed)?;

    let options = index::Options {
        // The instant is an argument, not a read, all the way down (ADR-002 §4.4); this is the one place
        // it is read, and every row and counter this run writes carries this one value.
        now: Some(crate::clock::now_millis()),
        scope: if args.refresh {
            Scope::Everything
        } else {
            Scope::NewCommits
        },
        refresh: args.refresh,
        worktree: if args.committed_only {
            Worktree::Ignore
        } else {
            Worktree::Analyse
        },
        ..index::Options::head()
    };
    let report = index::run(&repository, &mut store, &Unclassified, &options).map_err(failed)?;

    if args.json {
        output::line(format_args!("{}", json_report(&store_path, &report)));
    } else if args.stats {
        print_stats(&store_path, &report);
    } else {
        let added = report.spine.commits_written;
        output::line(format_args!(
            "{}: {} commit(s) walked, {} new, {} analyse unit(s) computed in {} ms",
            store_path.display(),
            report.commits_walked,
            added,
            report.computed(),
            report.elapsed.as_millis()
        ));
        if !report.worktree_skipped.is_empty() {
            output::line(format_args!(
                "  {} uncommitted path(s) were not analysed; pass --stats to see why",
                report.worktree_skipped.len()
            ));
        }
        output::line(format_args!(
            "  pass --stats for the per-analyzer cache table, or --json for the same as data"
        ));
    }

    Ok(match report.reason {
        // Only a cancel is the user's act and therefore the user's exit code (§7.3). A run that stopped
        // because its unit budget ran out did what it was told: that is a stated partial, exit 0, and the
        // report says so.
        Some(index::Interrupted::Cancelled) => ExitCode::Cancelled,
        _ => ExitCode::Ok,
    })
}

/// The `--json` shape, assembled here rather than derived from [`index::Report`]'s own serde so that the
/// key names are the CLI's promise and not a struct's internals: a field renamed in `fathom-analyze` must
/// not silently rename `fathom index --json`'s contract.
#[cfg(feature = "sqlite-bundled")]
fn json_report(
    store: &std::path::Path,
    report: &fathom_analyze::index::Report,
) -> serde_json::Value {
    use serde_json::json;

    json!({
        "store": store.display().to_string(),
        "kind": report.kind.as_str(),
        "elapsed_ms": report.elapsed.as_millis(),
        "commits": {
            "walked": report.commits_walked,
            "new": report.commits_new,
            "looked_up": report.commits_considered,
            "written": report.spine.commits_written,
        },
        "interrupted": report.interrupted,
        "reason": report.reason.map(|reason| format!("{reason:?}").to_ascii_lowercase()),
        "analyzers": report
            .analyzers
            .iter()
            .map(|(id, stats)| {
                json!({
                    "id": id,
                    "lookups": stats.lookups,
                    "hits": stats.hits,
                    "negative_hits": stats.negative_hits,
                    "cold": stats.cold,
                    "bumped": stats.bumped,
                    "epoch_stale": stats.epoch_stale,
                    "puts": stats.puts,
                    "bytes": stats.bytes,
                    "cost_ms": stats.cost_ms,
                    "hit_rate": stats.hit_rate(),
                })
            })
            .collect::<Vec<_>>(),
        "worktree": report
            .worktree
            .iter()
            .map(|record| {
                json!({
                    "path": record.path,
                    "staged": record.staged,
                    "reason": record.reason.as_str(),
                })
            })
            .collect::<Vec<_>>(),
        "worktree_skipped": report
            .worktree_skipped
            .iter()
            .map(|(path, reason)| json!({"path": path, "reason": reason}))
            .collect::<Vec<_>>(),
        "failures": report.failures,
    })
}

/// The table acceptance criterion four asks for: one row per analyzer, read out of the store's own counters
/// for this run, with the hit rate the run earned rather than the one the store has averaged all week.
#[cfg(feature = "sqlite-bundled")]
fn print_stats(store: &std::path::Path, report: &fathom_analyze::index::Report) {
    output::line(format_args!(
        "{} — {} run, {} commit(s) walked, {} looked up, {} ms",
        store.display(),
        report.kind.as_str(),
        report.commits_walked,
        report.commits_considered,
        report.elapsed.as_millis()
    ));
    output::line(format_args!(
        "{:<20} {:>8} {:>7} {:>6} {:>6} {:>7}  {:>9} {}",
        "ANALYZER", "LOOKUPS", "HITS", "COLD", "BUMPED", "PUTS", "HIT %", "NOTES"
    ));
    for (id, stats) in &report.analyzers {
        let rate = stats
            .hit_rate()
            .map_or_else(|| "     n/a".to_owned(), |rate| format!("{rate:6.1}%"));
        output::line(format_args!(
            "{:<20} {:>8} {:>7} {:>6} {:>6} {:>7}  {} {}",
            id,
            stats.lookups,
            stats.hits,
            stats.cold,
            stats.bumped,
            stats.puts,
            rate,
            if stats.negative_hits > 0 {
                format!("{} stored failure(s)", stats.negative_hits)
            } else if stats.epoch_stale > 0 {
                format!("{} epoch-stale row(s)", stats.epoch_stale)
            } else {
                String::new()
            }
        ));
    }
    for record in &report.worktree {
        output::line(format_args!(
            "  uncommitted {}: {} ({})",
            if record.staged { "staged" } else { "dirty" },
            record.path,
            record.reason.as_str()
        ));
    }
    for (path, reason) in &report.worktree_skipped {
        output::line(format_args!("  not analysed {path}: {reason}"));
    }
    for failure in &report.failures {
        output::problem(format_args!("  analysis failed: {failure}"));
    }
    if let Some(reason) = report.reason {
        output::problem(format_args!(
            "the run stopped early: {}",
            match reason {
                fathom_analyze::index::Interrupted::Cancelled => "cancelled",
                fathom_analyze::index::Interrupted::Budget => "out of its unit budget",
            }
        ));
    }
}

/// The same verb in a binary that was built without SQLite: the feature gate is the point, and the answer
/// is the exit code that says which flag to rebuild with (ADR-002 §6.1's `minimal` binary).
#[cfg(not(feature = "sqlite-bundled"))]
fn index(args: &IndexArgs) -> eyre::Result<ExitCode> {
    let _ = args;
    Err(feature_absent("fathom index", "sqlite-bundled"))
}

/// `fathom watch`: the state machine, run by [`crate::watch`] when the feature is compiled in.
///
/// The gate is a compile-time one rather than a runtime check because the watcher is a *dependency*, not a
/// boolean: a binary built without `watch` does not contain `notify` at all, so the honest answer is the
/// exit code that means "rebuild differently" (ADR-002 §6.1, §7.3).
#[cfg(feature = "watch")]
fn watch(args: &WatchArgs) -> eyre::Result<ExitCode> {
    crate::watch::run(args)
}

#[cfg(not(feature = "watch"))]
fn watch(_args: &WatchArgs) -> eyre::Result<ExitCode> {
    Err(feature_absent("fathom watch", "watch"))
}

fn mcp(args: &McpArgs) -> eyre::Result<ExitCode> {
    if !cfg!(feature = "mcp") {
        return Err(feature_absent("fathom mcp serve", "mcp"));
    }
    match args.action {
        McpAction::Serve => Err(pending(
            ExitCode::Error,
            "the MCP server is compiled in but not implemented yet",
            Some(
                "owned by e6.4 (reco-p3i); stdio is the transport that will not need consent"
                    .to_owned(),
            ),
        )),
    }
}

fn export(args: &ExportArgs) -> eyre::Result<ExitCode> {
    // The format list is the facade's, so `--format` in the CLI, the TUI's export key and the MCP tool
    // cannot drift apart (§5.3), and its error reaches the right exit code through `Diagnosable`.
    let format = fathom_analyze::export::Format::parse(&args.format).map_err(failed)?;
    let rendered = fathom_analyze::export::render(format, &args.out);
    Err(match rendered {
        Ok(text) => pending(
            ExitCode::Ok,
            format!("wrote {} to {}", args.out, format.file_name(&args.out)),
            Some(format!("{} bytes", text.len())),
        ),
        Err(err) => failed(err),
    })
}

/// The one verb with a real answer at the scaffold: it reports *this build*, which is the thing a scaffold
/// is able to know (ADR-002 §6.4: a bug report should say what was compiled).
fn doctor(args: &DoctorArgs) -> eyre::Result<ExitCode> {
    let status = fathom_analyze::insight::status();
    let rows = features::privacy_table();
    let net_compiled: Vec<&str> = rows
        .iter()
        .filter(|row| row.compiled_in)
        .map(|row| row.feature)
        .collect();

    if args.json {
        let report = serde_json::json!({
            "version": features::VERSION,
            "features": features::compiled(),
            "schema_version": status.schema_version,
            "views_ready": status.views_ready,
            "sqlite": sqlite_provenance(),
            "watch": watch_json(std::path::Path::new(&args.root)),
            "network_features": rows
                .iter()
                .map(|row| {
                    serde_json::json!({
                        "feature": row.feature,
                        "owner": row.owner,
                        "compiled_in": row.compiled_in,
                        "consent_key": row.consent_key,
                        "ticket": row.ticket,
                    })
                })
                .collect::<Vec<_>>(),
        });
        output::line(format_args!("{report}"));
        return Ok(ExitCode::Ok);
    }

    output::line(format_args!("fathom {}", features::version_line()));
    output::line(format_args!(
        "review model: schema {}, views ready: {}",
        status.schema_version, status.views_ready
    ));
    output::line(format_args!("sqlite: {}", sqlite_provenance()));
    for line in watch_report(std::path::Path::new(&args.root)) {
        output::line(format_args!("{line}"));
    }
    tracing::debug!(features = ?features::compiled(), "build report");

    if args.privacy {
        output::line(format_args!(
            "{:<20} {:<16} {:<10} {:<28} {}",
            "FEATURE", "OWNER", "COMPILED", "CONSENT KEY", "OWNER TICKET"
        ));
        for row in &rows {
            output::line(format_args!(
                "{:<20} {:<16} {:<10} {:<28} {}",
                row.feature,
                row.owner,
                if row.compiled_in { "yes" } else { "no" },
                row.consent_key,
                row.ticket
            ));
        }
        output::line(format_args!(
            "outbound features compiled in: {}",
            if net_compiled.is_empty() {
                "none — no HTTP client and no listener in this binary (ADR-002 R6)".to_owned()
            } else {
                net_compiled.join(", ")
            }
        ));
    } else {
        output::line(format_args!(
            "pass --privacy for the network-feature table, or --json for the same report as data"
        ));
    }
    Ok(ExitCode::Ok)
}

fn sqlite_provenance() -> &'static str {
    if cfg!(feature = "sqlite-bundled") {
        "bundled, compiled in (ADR-004 D2: STRICT, `->>`, FTS5)"
    } else {
        "not compiled into this build (no SQLite feature was enabled)"
    }
}

/// What `fathom doctor` says about watching this repository: the backend the probe would choose here, and
/// the heartbeat of any watcher already running.
///
/// This is `e1.9`'s fourth acceptance criterion made inspectable — "the polling fallback kicks in
/// automatically and is reported in `fathom doctor`". A prediction read off the mount table is only half of
/// that, so the running watcher's own backend is printed too: it may have *reactively* fallen back after
/// the OS refused to notify, and it carries the reason the OS gave.
#[cfg(feature = "watch")]
fn watch_report(root: &std::path::Path) -> Vec<String> {
    crate::watch::doctor_lines(root, fathom_ingest::watch::Options::default().poll_interval)
}

#[cfg(not(feature = "watch"))]
fn watch_report(_root: &std::path::Path) -> Vec<String> {
    vec![
        "watch: not compiled into this build; rebuild with `--features watch` to get `fathom watch`"
            .to_owned(),
    ]
}

/// The same, as data.
#[cfg(feature = "watch")]
fn watch_json(root: &std::path::Path) -> serde_json::Value {
    crate::watch::doctor_json(root, fathom_ingest::watch::Options::default().poll_interval)
}

#[cfg(not(feature = "watch"))]
fn watch_json(_root: &std::path::Path) -> serde_json::Value {
    serde_json::json!({ "compiled_in": false })
}

/// A failure that knows the exit code the process should leave with.
///
/// `main` downcasts to this at the edge (ADR-002 §7.1): the message the human reads, the hint under it
/// and the number CI branches on come from one value, so they cannot disagree.
#[derive(Debug)]
pub struct Pending {
    message: String,
    hint: Option<String>,
    code: ExitCode,
}

impl Pending {
    pub(crate) fn new(code: ExitCode, message: impl Into<String>, hint: Option<String>) -> Self {
        Pending {
            message: message.into(),
            hint,
            code,
        }
    }

    /// A rejected argument that is not clap's to catch: a flag whose *value* is wrong. `USAGE` is the code
    /// clap itself uses (§7.3), so `fathom watch --backend sideways` and `fathom --nope` fail alike.
    pub(crate) fn usage(message: impl Into<String>, detail: impl Into<String>) -> Self {
        Pending::new(ExitCode::Usage, message, Some(detail.into()))
    }

    /// The code to leave with.
    #[must_use]
    pub fn code(&self) -> ExitCode {
        self.code
    }

    /// The line to print under the message, if there is one.
    #[must_use]
    pub fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }

    /// The message itself, for `main`'s error path.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for Pending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Pending {}

/// Turn any crate error into a failure carrying its published exit code (§7.1).
pub(crate) fn failed(err: impl Diagnosable + std::fmt::Display) -> eyre::Report {
    Pending::new(
        err.code().exit(),
        err.to_string(),
        err.hint().map(str::to_owned),
    )
    .into()
}

fn pending(code: ExitCode, message: impl Into<String>, hint: Option<String>) -> eyre::Report {
    Pending::new(code, message, hint).into()
}

fn not_implemented(verb: &str, ticket: &str) -> eyre::Report {
    Pending::new(
        ExitCode::Error,
        format!("`{verb}` is not implemented yet"),
        Some(format!(
            "owned by {ticket}; the ticket map is docs/adr/0002-workspace.md §11"
        )),
    )
    .into()
}

fn feature_absent(verb: &str, feature: &str) -> eyre::Report {
    Pending::new(
        ExitCode::FeatureAbsent,
        format!("`{verb}` needs the `{feature}` feature, which this binary was not built with"),
        Some(
            "rebuild with `--features <name>`; `fathom doctor` lists what is compiled in"
                .to_owned(),
        ),
    )
    .into()
}
