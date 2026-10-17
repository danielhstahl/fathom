//! The verbs: parse, dispatch, and leave with a published exit code (ADR-002 §4.3, §7.1, §7.3).
//!
//! Each verb here is a *stub that knows its own name*, and that is the design rather than a placeholder
//! for it: the verb surface, the refusal it returns before it has an answer, and the exit code it leaves
//! with are the contract `e1.10` fills in. A stub returns a typed failure and never a panic (§7.2, §9
//! rule 2), and the failure names the ticket that owns the work, so "not yet" is a pointer and not a
//! wall.

use crate::diagnose;
use crate::output;
use crate::query;
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
    /// Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7).
    ///
    /// Chosen automatically when stdout is not a terminal, so a redirected run degrades by itself.
    #[arg(long, global = true)]
    pub plain: bool,
    /// Draw in place instead of taking the alternate screen, which keeps the shell's scrollback.
    #[arg(long = "no-alt-screen", global = true)]
    pub no_alt_screen: bool,
    /// Ignore the session this repository has stored, and start from defaults (`e1.11`).
    ///
    /// The escape hatch for a session that came back wrong: a filter that hides the episode you are
    /// looking for, a cursor on a row that moved. The stored state is not destroyed — a run that changes
    /// nothing writes nothing — it is simply not applied to this one.
    #[arg(long, global = true)]
    pub fresh: bool,
    /// The verb to run. With no verb, fathom opens the terminal UI — if it was compiled in.
    #[command(subcommand)]
    pub verb: Option<Verb>,
}

/// The flags that are not a verb: how the process is to meet the terminal (ADR-006 §6.7).
#[derive(Debug, Clone, Copy)]
pub struct Globals {
    /// Render the frame as text and write nothing else.
    pub plain: bool,
    /// Draw inline rather than in the alternate screen.
    pub no_alt_screen: bool,
    /// Do not apply the repository's stored session (`e1.11`).
    pub fresh: bool,
}

/// The verbs fathom answers to. ADR-005 §6 owns the full list and `e1.10` ships the headless half of it;
/// the ones here are the ones the exit-code contract, the JSON schemas and the feature gates have to cover.
#[derive(Subcommand, Debug)]
pub enum Verb {
    /// Index the repository so every other verb has something to read
    Index(IndexArgs),
    /// Show one episode in full
    Show(ShowArgs),
    /// Report repo scale, index state and cache hit rate
    Stats(StatsArgs),
    /// Run the checks CI cares about; findings above the threshold exit 7
    Check,
    /// Describe this build, its environment and its index, with a remedy per finding
    Doctor(DoctorArgs),
    /// Write a review artefact for a pull request or an audit
    Export(ExportArgs),
    /// Speak MCP to a coding agent
    Mcp(McpArgs),
    /// Watch the repository and index as it changes
    Watch(WatchArgs),
    /// Print the terminal UI's keymap, or open the UI itself
    Tui(TuiArgs),
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
    /// Throw the epoch away and rebuild every analysis from scratch.
    ///
    /// The old epoch's rows stay on disk and are marked dead (ADR-004 §6.3), so a rebuild that is interrupted
    /// is recoverable by definition; the staging file and atomic swap of §9.3 arrive with the daemon that
    /// has a concurrent reader to protect.
    #[arg(long)]
    pub rebuild: bool,
    /// Index once, then keep watching and index as the repository changes.
    ///
    /// The one-shot form a script wants, and the same thing as `fathom watch` with an initial index.
    #[arg(long)]
    pub watch: bool,
    /// Do not analyse uncommitted paths (ADR-004 §6.4).
    #[arg(long = "committed-only")]
    pub committed_only: bool,
}

/// `fathom show`
#[derive(Args, Debug)]
pub struct ShowArgs {
    /// The episode to show: a commit, a rev (`HEAD~3`, `main`, `v1.2^{}`) or an oid prefix.
    pub episode: String,
    /// The repository to read.
    #[arg(long, default_value = ".")]
    pub root: String,
    /// Print the episode as JSON on stdout, and nothing else there.
    #[arg(long)]
    pub json: bool,
}

/// `fathom stats`
#[derive(Args, Debug)]
pub struct StatsArgs {
    /// The repository to report on.
    #[arg(default_value = ".")]
    pub root: String,
    /// Print the same report as JSON, for a machine.
    #[arg(long)]
    pub json: bool,
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
    /// Run the whole-database `integrity_check` rather than `quick_check` (ADR-004 §9.6).
    #[arg(long)]
    pub deep: bool,
    /// Exit with the code of the worst finding instead of 0 — what CI and a pre-flight check want.
    #[arg(long)]
    pub strict: bool,
    /// The repository to report on.
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
    /// The episode range to export, e.g. `HEAD~9..HEAD`.
    #[arg(long, default_value = "HEAD")]
    pub episodes: String,
    /// The repository to read.
    #[arg(long, default_value = ".")]
    pub root: String,
    /// Describe what the artefact *would* contain, as JSON, and write nothing.
    #[arg(long)]
    pub json: bool,
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

/// `fathom tui`
#[derive(Args, Debug)]
pub struct TuiArgs {
    /// Print the merged keymap — defaults plus your `keys.overrides` — and exit, taking no terminal.
    #[arg(long)]
    pub show_keys: bool,
    /// Print the session state stored for this repository, and exit, taking no terminal (`e1.11`).
    ///
    /// The rows, when each was written, and what this build would apply — which is the only way to see
    /// what a resumed session is resuming *from*, and the reader for the keys no landed view draws yet.
    #[arg(long)]
    pub show_session: bool,
    /// With `--show-session`: the rows as JSON on stdout, and nothing else there.
    #[arg(long, requires = "show_session")]
    pub json: bool,
}

/// Run one verb. The return type is the process contract, not a `Result` a caller forgets to check.
///
/// # Errors
///
/// [`Pending`], or anything a verb chose to raise, when the verb cannot be completed; `main` maps that to
/// an exit code at the edge, which is the only place the two are joined.
pub fn dispatch(verb: Option<&Verb>, globals: &Globals) -> eyre::Result<ExitCode> {
    match verb {
        None => open_terminal(globals),
        Some(Verb::Index(args)) => index(args),
        Some(Verb::Show(args)) => query::show(&args.root, &args.episode, args.json),
        Some(Verb::Stats(args)) => query::stats(&args.root, args.json),
        Some(Verb::Check) => Err(not_implemented("fathom check", "e6.6")),
        Some(Verb::Doctor(args)) => {
            diagnose::doctor(&args.root, args.privacy, args.json, args.deep, args.strict)
        }
        Some(Verb::Export(args)) => export(args),
        Some(Verb::Mcp(args)) => mcp(args),
        Some(Verb::Watch(args)) => watch(args),
        Some(Verb::Tui(args)) => tui(args, globals),
    }
}

/// `fathom tui --show-keys`: the merged keymap as text, no terminal taken.
///
/// The overlay inside the shell prints this table; the flag prints it where the shell cannot run —
/// a pipe, CI, a screen reader — which is what makes "your override changed the table" checkable
/// without a pty (§8.3, and ADR-006 §7.3's cheatsheet).
#[cfg(feature = "tui")]
fn tui(args: &TuiArgs, globals: &Globals) -> eyre::Result<ExitCode> {
    if args.show_keys {
        return crate::tui::show_keys();
    }
    if args.show_session {
        return crate::session::show(&std::path::PathBuf::from("."), args.json);
    }
    crate::tui::dispatch(globals)
}

#[cfg(not(feature = "tui"))]
fn tui(_args: &TuiArgs, _globals: &Globals) -> eyre::Result<ExitCode> {
    Err(feature_absent("fathom tui", "tui"))
}

fn open_terminal(globals: &Globals) -> eyre::Result<ExitCode> {
    // The compile-time answer first, because a binary built `--no-default-features` does not contain
    // the shell at all; the runtime answer (no tty, `--plain`) is `tui`'s to make.
    #[cfg(feature = "tui")]
    {
        crate::tui::dispatch(globals)
    }
    #[cfg(not(feature = "tui"))]
    {
        let _ = globals;
        Err(feature_absent("running with no verb", "tui"))
    }
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
    // The boundary knobs come from the repository's own `fathom.toml`. This is the crate ADR-002 §4.5 lets
    // read configuration, and the resolution happens here so that segmentation receives a value.
    let segment = crate::segment::from_repository(root);

    // `--rebuild` is `advance_epoch` plus a walk, which is what `fathom-store`'s own documentation says it
    // is (ADR-004 §6.3, §9.3). The staging file and the atomic rename are deliberately not attempted here:
    // their whole purpose is that a *concurrent reader* never sees a half-rebuilt epoch, and until the
    // daemon and the TUI open this store while a run writes it there is no concurrent reader to protect.
    // An interrupted rebuild is recoverable by construction — the old epoch's rows are still on disk and
    // marked dead — and `fathom stats` reports a parked epoch as `building`, with the remedy.
    if args.rebuild {
        store.advance_epoch().map_err(failed)?;
    }
    let options = index::Options {
        // The instant is an argument, not a read, all the way down (ADR-002 §4.4); this is the one place
        // it is read, and every row and counter this run writes carries this one value.
        now: Some(crate::clock::now_millis()),
        scope: if args.refresh || args.rebuild {
            Scope::Everything
        } else {
            Scope::NewCommits
        },
        segment: segment.rules.clone(),
        refresh: args.refresh || args.rebuild,
        worktree: if args.committed_only {
            Worktree::Ignore
        } else {
            Worktree::Analyse
        },
        ..index::Options::head()
    };
    let report = index::run(&repository, &mut store, &Unclassified, &options).map_err(failed)?;
    // A key that was read and could not be honoured is said out loud, on the JSON path too: "the file
    // changed nothing" and "the file was not applied" are different statements and only one of them is
    // true of a silently dropped `fathom.toml` entry (ADR-002 §7.1).
    for notice in &segment.notices {
        output::problem(format_args!("config: {notice}"));
    }
    if args.rebuild {
        // The other half of §9.3's state machine: the epoch this run filled is the one readers may now see.
        store.settle_epoch().map_err(failed)?;
    }
    let epoch = store.epoch().map_err(failed)?;

    if args.json {
        output::line(format_args!(
            "{}",
            json_report(root, &store_path, &report, args.rebuild, epoch)
        ));
    } else if args.stats {
        print_stats(&store_path, &report);
    } else {
        let added = report.spine.commits_written;
        output::line(format_args!(
            "{}: {}{} commit(s) walked, {} new, {} analyse unit(s) computed in {} ms",
            store_path.display(),
            if args.rebuild { "rebuild: " } else { "" },
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

    // `--watch` is "and then keep doing this": the same state machine `fathom watch` runs, rooted where the
    // index was rooted, with the same output contract. A binary built without the watcher says so with the
    // feature code rather than exiting 0 after indexing and pretending a watch had started.
    if args.watch {
        return watch_after_index(args);
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
    root: &std::path::Path,
    store: &std::path::Path,
    report: &fathom_analyze::index::Report,
    rebuild: bool,
    epoch: i64,
) -> serde_json::Value {
    use serde_json::json;

    let redactor = crate::sanitize::Redactor::for_root(root);
    json!({
        "contract": query::INDEX_CONTRACT,
        // The store is named *relative to the repository it belongs to*: an absolute path in a machine-
        // readable report is a path the reader cannot use and a fact about the machine that produced it.
        "store": redactor.path(store),
        "rebuild": rebuild,
        "epoch": epoch,
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
                    "computes": misses(stats),
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

/// Lookups that found nothing usable: the misses, which is the work the repository made the analyzers do.
///
/// `puts` counts rows *written*, and a `--refresh` recompute writes a row after a hit, so `puts` is not
/// "misses" — it is "writes". The distinction is the difference between a warm cache that looks broken and
/// a warm cache that was asked to re-prove itself (ADR-004 §5.6, §6.1).
#[cfg(feature = "sqlite-bundled")]
fn misses(stats: &fathom_analyze::index::AnalyzerStats) -> i64 {
    (stats.lookups - stats.hits - stats.negative_hits).max(0)
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

/// `fathom watch`, run by `fathom index --watch` with the index verb's own root and output choice.
///
/// The flags are the watcher's defaults rather than a second copy of them: `--watch` exists so a script has
/// one command to run, not so it can set twenty tuning knobs through `index`.
#[cfg(feature = "watch")]
fn watch_after_index(args: &IndexArgs) -> eyre::Result<ExitCode> {
    watch(&WatchArgs {
        root: args.root.clone(),
        json: args.json,
        once: false,
        max_cycles: None,
        deadline: None,
        backend: "auto".to_owned(),
        interval_ms: None,
        debounce_ms: None,
        heartbeat_ms: None,
        chunk_units: 2_000,
        worktree: false,
        no_status_file: false,
        quiet: false,
    })
}

#[cfg(not(feature = "watch"))]
#[cfg_attr(not(feature = "sqlite-bundled"), allow(dead_code))]
fn watch_after_index(_args: &IndexArgs) -> eyre::Result<ExitCode> {
    Err(feature_absent("fathom index --watch", "watch"))
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

/// `fathom export`: the skeleton `e5.7` fills in.
///
/// What is real here is the *decision surface*: the format list is the facade's, the out stem and the
/// episode range are parsed and validated, and `--json` answers what the artefact would be built from. What
/// is not here is the rendering, which is `e5.7`'s, and the verb says so and leaves with `ERROR` rather
/// than writing a file nobody asked for (ADR-002 §7.2: a stub is a typed failure, never a panic and never a
/// silently-empty artefact).
fn export(args: &ExportArgs) -> eyre::Result<ExitCode> {
    // The format list is the facade's, so `--format` in the CLI, the TUI's export key and the MCP tool
    // cannot drift apart (§5.3), and its error reaches the right exit code through `Diagnosable`.
    let format = fathom_analyze::export::Format::parse(&args.format).map_err(failed)?;
    let artefact = format.file_name(&args.out);
    let scope = export_scope(&args.root, &args.episodes);

    if args.json {
        output::line(format_args!(
            "{}",
            serde_json::json!({
                "contract": query::EXPORT_CONTRACT,
                "status": "not_implemented",
                "owner_ticket": "e5.7",
                "format": format.file_name("x").rsplit('.').next().unwrap_or("md"),
                "artifact": artefact,
                "out": args.out,
                "scope": scope,
                "writes_file": false,
                "exit_code": ExitCode::Error.as_i32(),
                "exit_name": ExitCode::Error.name(),
            })
        ));
        return Ok(ExitCode::Error);
    }

    let rendered = fathom_analyze::export::render(format, &args.out);
    Err(match rendered {
        Ok(text) => pending(
            ExitCode::Error,
            "`fathom export` is the skeleton: the format, the destination and the scope are \
             validated; the rendering is owned by e5.7"
                .to_owned(),
            Some(format!(
                "scope {} ({} commit(s)); artefact would be {artefact} ({} bytes once e5.7 renders it)",
                scope["asked"].as_str().unwrap_or("HEAD"),
                scope["commits"].as_u64().unwrap_or(0),
                text.len()
            )),
        ),
        Err(err) => failed(err),
    })
}

/// The episode range an export would cover, resolved against the repository if there is one.
///
/// Resolving the range is worth doing in the skeleton because it is the half that can be wrong: an export
/// that reported a scope it had not resolved would be a preview of nothing.
fn export_scope(root: &str, episodes: &str) -> serde_json::Value {
    use serde_json::json;

    let (start, end) = match episodes.split_once("..=").or(episodes.split_once("..")) {
        Some((from, to)) => (from, to),
        None => (episodes, episodes),
    };
    let resolved = std::path::Path::new(root);
    match fathom_ingest::repo::Repository::discover(resolved) {
        Ok(repo) => {
            let one = |spec: &str| {
                fathom_ingest::revspec::resolve(&repo, spec)
                    .ok()
                    .map(|oid| oid.hex())
            };
            let from = one(start);
            let to = one(end);
            let same = from.is_some() && from == to;
            json!({
                "asked": episodes,
                "from": from.map(|oid| oid.chars().take(7).collect::<String>()),
                "to": to.map(|oid| oid.chars().take(7).collect::<String>()),
                "commits": if same { 1 } else { 0 },
                "resolved": same,
            })
        }
        Err(_) => json!({"asked": episodes, "resolved": false}),
    }
}

/// What this build's SQLite story is, in the one sentence `fathom doctor` prints.
#[must_use]
pub(crate) fn sqlite_provenance_string() -> &'static str {
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
#[must_use]
pub(crate) fn watch_lines(root: &std::path::Path) -> Vec<String> {
    #[cfg(feature = "watch")]
    {
        crate::watch::doctor_lines(root, fathom_ingest::watch::Options::default().poll_interval)
    }
    #[cfg(not(feature = "watch"))]
    {
        let _ = root;
        vec![
            "watch: not compiled into this build; rebuild with `--features watch` to get `fathom watch`"
                .to_owned(),
        ]
    }
}

/// The same, as data.
#[must_use]
pub(crate) fn watch_report_json(root: &std::path::Path) -> serde_json::Value {
    #[cfg(feature = "watch")]
    {
        crate::watch::doctor_json(root, fathom_ingest::watch::Options::default().poll_interval)
    }
    #[cfg(not(feature = "watch"))]
    {
        let _ = root;
        serde_json::json!({ "compiled_in": false })
    }
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
    #[cfg_attr(not(feature = "watch"), allow(dead_code))]
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

pub(crate) fn pending(
    code: ExitCode,
    message: impl Into<String>,
    hint: Option<String>,
) -> eyre::Report {
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

pub(crate) fn feature_absent(verb: &str, feature: &str) -> eyre::Report {
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
