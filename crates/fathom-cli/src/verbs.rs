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
    /// Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1).
    ///
    /// Off by default, because capturing the pointer takes the terminal's own text selection away for
    /// the length of the run. Every gesture it adds is also a key, and the status line says `mouse on`
    /// while it is armed.
    #[arg(long, global = true)]
    pub mouse: bool,
    /// With `--plain`: print the frame in reading order, one block per pane, instead of side by side.
    ///
    /// Two panes drawn next to each other is the right shape for eyes and the wrong shape for a log,
    /// a pipe or a screen reader (ADR-006 §6.7). Same facts, same order, one per line.
    #[arg(long, global = true)]
    pub lines: bool,
    /// Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`).
    ///
    /// `dark`, `light` and `ansi` are built in, and all three are measured — for contrast against the
    /// background they paint, and for how far the added / removed / violation pairs stay apart when
    /// protanopia and deuteranopia are simulated. Anything else is `<name>.toml` in `$FATHOM_THEME_DIR` or
    /// `~/.config/fathom/themes/`, or a path. `$FATHOM_THEME` and `[theme] name` in your own config are
    /// the same knob one layer down; a repository's `fathom.toml` is deliberately not a layer at all,
    /// because a clone does not get to repaint your terminal. With colour off — `NO_COLOR`, or a terminal
    /// that has none — the theme is not applied and every state keeps its shape and its word.
    /// `fathom tui --list-themes` lists what Fathom can see; `--show-theme` prints the one in force with
    /// its measurements, and `--show-theme --audit` exits 7 if a palette fails a floor.
    #[arg(long, global = true, value_name = "NAME|PATH")]
    pub theme: Option<String>,
    /// Review a miniature repository that ships inside this binary, then say what to do next (`e5.9`).
    ///
    /// The five-minute tour: Fathom unpacks a 21-commit repository into its cache directory, indexes it
    /// with the same `fathom index` you would run on your own history, and opens on it. Nothing in the
    /// directory you ran it from is read or written, no configuration is read, and deleting
    /// `$XDG_CACHE_HOME/fathom` is a complete undo. With a verb the verb runs against the miniature; with
    /// none you get the shell.
    #[arg(long)]
    pub demo: bool,
    /// Print the index's progress row even where stderr is not a terminal (`e5.9`).
    ///
    /// On a terminal the row is on by default, one row overwriting itself; this asks for it in a log, a CI
    /// step or a `watch -n 1` wrapper, where one line per update is the point. It applies to `fathom index`
    /// and to the indexing `fathom --demo` does on your behalf — no other verb has a row to print — and
    /// `--json` refuses it, because that stream belongs to the run report and a row beside a machine-readable
    /// document is how a parser dies.
    #[arg(
        long,
        global = true,
        conflicts_with = "no_progress",
        help = "print the index's progress row even when stderr is not a terminal (`index` and `--demo`)"
    )]
    pub progress: bool,
    /// Never print the index's progress row, even on a terminal (`e5.9`).
    ///
    /// The other half of the same choice: a script with its own progress line, or a run whose output is
    /// being tee'd somewhere, turns it off explicitly instead of discovering it on.
    #[arg(
        long = "no-progress",
        global = true,
        help = "never print the index's progress row, even on a terminal"
    )]
    pub no_progress: bool,
    /// The verb to run. With no verb, fathom opens the terminal UI — if it was compiled in.
    #[command(subcommand)]
    pub verb: Option<Verb>,
}

/// The flags that are not a verb: how the process is to meet the terminal (ADR-006 §6.7).
#[derive(Debug, Clone)]
pub struct Globals {
    /// Render the frame as text and write nothing else.
    pub plain: bool,
    /// Draw inline rather than in the alternate screen.
    pub no_alt_screen: bool,
    /// Do not apply the repository's stored session (`e1.11`).
    pub fresh: bool,
    /// Capture the pointer (`e5.5`).
    pub mouse: bool,
    /// Print the plain frame in reading order rather than as a grid (`e5.5`).
    pub lines: bool,
    /// `--progress`: the index's progress row, forced on (`e5.9`).
    pub progress: bool,
    /// `--no-progress`: the index's progress row, forced off (`e5.9`).
    pub no_progress: bool,
    /// Review the bundled miniature instead of the repository underfoot (`e5.9`).
    ///
    /// Resolved before the verb runs, because the answer is a directory to `chdir` into: every verb after
    /// it reads the demo through its own ordinary path, which is what makes the demo the product rather
    /// than a picture of it.
    pub demo: bool,
    /// The theme the reviewer chose, if any (`e5.6`): a built-in name, a theme-directory name, or a path.
    ///
    /// Resolved to a palette by `crate::theme`, which is also where the precedence over the environment
    /// and the user config lives; nothing downstream of that reads a name.
    pub theme: Option<String>,
}

/// The verbs fathom answers to. ADR-005 §6 owns the full list and `e1.10` ships the headless half of it;
/// the ones here are the ones the exit-code contract, the JSON schemas and the feature gates have to cover.
#[derive(Subcommand, Debug)]
pub enum Verb {
    /// Index the repository so every other verb has something to read
    Index(IndexArgs),
    /// Show one episode in full
    Show(ShowArgs),
    /// Report or correct who Fathom thinks wrote a commit (`e4.1`)
    Attribution(AttributionArgs),
    /// The review ledger: sign-offs that outlive the history they were written about (`e4.6`)
    Review(ReviewArgs),
    /// Print one episode's delta document — the model the TUI, the export and an agent all read
    Analyze(AnalyzeArgs),
    /// Print one episode's risk score with every feature, its weight and its contribution (`e4.4`)
    Why(WhyArgs),
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
    /// Report on the harness event stream: lag, offset, rejections, replay (`e6.1`)
    Hooks(HooksArgs),
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
    /// Send the run report to stderr instead of stdout.
    ///
    /// Not a flag a person types: it is what `fathom --demo` sets, because on that run stdout belongs to
    /// the frame the shell is about to draw and a summary line printed first is a corrupt first frame.
    /// `fathom watch --quiet` is the same promise, spelled the same way.
    #[arg(skip)]
    pub quiet: bool,
}

/// `fathom attribution`: the authorship label, and the way to overrule it.
///
/// A correction is keyed on an identity, not on an episode: episode ids are derived from content and move
/// when a lineage is rewritten, and the identity is what the user meant (`e4.1`).
#[derive(Args, Debug)]
pub struct AttributionArgs {
    /// What to do.
    #[command(subcommand)]
    pub action: AttributionAction,
}

/// `fathom review …` — the ledger's verbs (`e4.6`, `reco-rlf`).
///
/// The root comes from the global `--root` rather than a per-verb flag, because the ledger is a property of
/// the repository: `fathom --root X review sign` and `fathom --root X episodes` must be able to disagree
/// about nothing.
#[derive(Args, Debug)]
pub struct ReviewArgs {
    /// The repository to read.
    #[arg(long, default_value = ".")]
    pub root: String,
    /// Print the verdict and every entry as one JSON line (machine contract `fathom.cli.review/1`).
    ///
    /// Global so `fathom review --json sign …` and `fathom review sign … --json` are the same command;
    /// a reviewer should not have to remember which side of the subverb a flag lives on.
    #[arg(long, global = true)]
    pub json: bool,
    /// What to do with the ledger; bare `fathom review` is `review status`.
    #[command(subcommand)]
    pub action: Option<ReviewAction>,
}

/// What to do with the ledger.
#[derive(Subcommand, Debug)]
pub enum ReviewAction {
    /// The verdict: is every episode covered by a sign-off that still matches (exit `7` when it is not)
    Status,
    /// Sign an episode off, appending one entry to `.fathom/reviews/ledger.md`
    Sign(ReviewSignArgs),
    /// Render the queue's acts that the ledger has never seen
    Sync(ReviewSyncArgs),
    /// List the ledger's entries with their verdicts
    List(ReviewListArgs),
    /// Re-hash every entry against its own `digest:` line — tampering, and nothing else
    Verify,
}

/// `fathom review sign <EPISODE>`.
#[derive(Args, Debug)]
pub struct ReviewSignArgs {
    /// The episode to sign off, or an unambiguous prefix of its id
    pub episode: String,
    /// What the decision is; `accepted` is the one that means *reviewed*
    #[arg(long, default_value = "accepted")]
    pub decision: String,
    /// A note, committed with the entry and folded onto one line
    #[arg(long)]
    pub note: Option<String>,
    /// Who is signing, when git config and `fathom.toml` do not say
    #[arg(long)]
    pub identity: Option<String>,
    /// Record the act as an agent's verdict on someone's behalf, not as a human acceptance
    #[arg(long)]
    pub agent: Option<String>,
}

/// `fathom review sync`.
#[derive(Args, Debug)]
pub struct ReviewSyncArgs {
    /// Report what would be rendered, write nothing
    #[arg(long)]
    pub dry_run: bool,
}

/// `fathom review list`.
#[derive(Args, Debug)]
pub struct ReviewListArgs {
    /// Show only entries in this state: `fresh`, `stale`, `orphan` or `unknown`
    #[arg(long)]
    pub state: Option<String>,
}

/// The `fathom attribution` subcommands.
#[derive(Subcommand, Debug)]
pub enum AttributionAction {
    /// Store a correction: these identities were written by an agent / by a person
    Correct(AttributionCorrectArgs),
    /// List the stored corrections, which are the keys the classifier honours
    List(AttributionListArgs),
    /// Forget one stored correction
    Forget(AttributionForgetArgs),
}

/// `fathom attribution correct`
#[derive(Args, Debug)]
pub struct AttributionCorrectArgs {
    /// The commit whose identities are being corrected: a rev, an oid prefix, or `-` for nothing.
    pub target: Option<String>,
    /// The repository to write to.
    #[arg(long, default_value = ".")]
    pub root: String,
    /// `agent`, `human` or `unknown`.
    #[arg(long)]
    pub kind: String,
    /// Which agent the identity is, when `--kind agent`.
    #[arg(long)]
    pub agent: Option<String>,
    /// Why the label was overruled, for whoever reads it next.
    #[arg(long)]
    pub note: Option<String>,
    /// Correct one identity key directly (`identity:<address>`), instead of deriving them from a commit.
    #[arg(long)]
    pub identity: Option<String>,
    /// Print the result as JSON on stdout, and nothing else there.
    #[arg(long)]
    pub json: bool,
}

/// `fathom attribution list`
#[derive(Args, Debug)]
pub struct AttributionListArgs {
    /// The repository to read.
    #[arg(long, default_value = ".")]
    pub root: String,
    /// Print the corrections as JSON on stdout, and nothing else there.
    #[arg(long)]
    pub json: bool,
}

/// `fathom attribution forget`
#[derive(Args, Debug)]
pub struct AttributionForgetArgs {
    /// The identity key to forget, as `fathom attribution list` prints it.
    pub identity: String,
    /// The repository to write to.
    #[arg(long, default_value = ".")]
    pub root: String,
    /// Print the result as JSON on stdout, and nothing else there.
    #[arg(long)]
    pub json: bool,
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

/// `fathom analyze`
#[derive(Args, Debug)]
pub struct AnalyzeArgs {
    /// The episode to analyse: an episode id, a commit, a rev (`HEAD~3`, `main`, `v1.2^{}`) or an oid
    /// prefix. Omitted with `--schema`, which prints the contract instead of an answer.
    pub episode: Option<String>,
    /// The repository to read.
    #[arg(long, default_value = ".")]
    pub root: String,
    /// Print the delta document as JSON on stdout, and nothing else there.
    #[arg(long)]
    pub json: bool,
    /// Print `schemas/episode-delta.schema.json`, the JSON Schema for the document, and stop.
    #[arg(long)]
    pub schema: bool,
    /// Check the checked-in schema against what this build generates; the CI form of `--schema`.
    #[arg(long, requires = "schema")]
    pub check: bool,
    /// Write the generated schema to `schemas/`, replacing the checked-in copy.
    #[arg(long, requires = "schema", conflicts_with = "check")]
    pub write: bool,
}

/// `fathom why`
#[derive(Args, Debug)]
pub struct WhyArgs {
    /// The episode to score: an episode id, a commit, a rev (`HEAD~3`, `main`, `v1.2^{}`) or an oid
    /// prefix. Omitted with `--calibrate`, which reports on the weights instead of on an episode.
    pub episode: Option<String>,
    /// The repository to read.
    #[arg(long, default_value = ".")]
    pub root: String,
    /// Print the same answer as JSON, for a machine.
    #[arg(long)]
    pub json: bool,
    /// Print the calibration report — precision and recall at the top of the queue, as shipped and as
    /// retuned — instead of a score. With a path, that JSON set is measured; without one, the set shipped
    /// with the binary.
    #[arg(long, num_args = 0..=1, default_missing_value = "", value_name = "SET")]
    pub calibrate: Option<String>,
    /// Override one weight for this run only: `--weight findings=0.25`. Repeatable. Nothing is written to
    /// `fathom.toml`; the score says which weights were overridden.
    #[arg(long = "weight", value_name = "NAME=VALUE")]
    pub weight: Vec<String>,
    /// Override the flag line for this run only, 0 to 100.
    #[arg(long)]
    pub tau: Option<u8>,
    /// How deep the queue is, as a fraction, for `--calibrate`. Defaults to 0.1 — the top ten percent.
    #[arg(long, value_name = "FRACTION")]
    pub cut: Option<f64>,
    /// Print the feature names this build can score, and stop.
    #[arg(long)]
    pub weights: bool,
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

/// `fathom hooks`
///
/// The stream is the harness's, and this verb only reads it (or rebuilds its own offset): nothing here
/// writes to `events.jsonl`, which is append-only by contract (ADR-005 §3.6). The actions are exclusive and
/// `--check` is the default, because the question asked most often is "is the stream healthy", and answering
/// it must not consume anything.
#[derive(Args, Debug)]
pub struct HooksArgs {
    /// The repository whose `.fathom/events.jsonl` to read.
    #[arg(default_value = ".")]
    pub root: String,
    /// Read this events file instead of the configured one (ADR-005 §3.1's first choice).
    #[arg(long)]
    pub events: Option<String>,
    /// Report the stream and change nothing. This is the default; the flag exists so a script can say what
    /// it means.
    #[arg(long, conflicts_with_all = ["replay", "tail", "trailer"])]
    pub check: bool,
    /// Rebuild everything from byte 0 and rewrite the offset: what an index rebuild does first.
    ///
    /// Replay is deterministic by contract — the same file yields the same turns and the same commit
    /// linkage — which is what makes it safe to key a review to a turn id across a rebuild.
    #[arg(long)]
    pub replay: bool,
    /// Read forward from the durable offset, advancing it, and print what arrived.
    #[arg(long)]
    pub tail: bool,
    /// With `--tail`: print at most this many records.
    #[arg(long, requires = "tail")]
    pub max: Option<usize>,
    /// Print the `Agent-Session:` / `Agent-Turn:` trailer pair for `<session>:<turn-index>`.
    ///
    /// The turn id is session-qualified on the way out, which is what keeps two sessions' turn 3 from
    /// reading as one turn (ADR-003 §9.1, §18.2 **D4**).
    #[arg(long, value_name = "SESSION:TURN")]
    pub trailer: Option<String>,
    /// Print the same report as JSON on stdout, and nothing else there.
    #[arg(long)]
    pub json: bool,
    /// Exit 7 when the stream is not healthy — the form a CI gate wants.
    #[arg(long)]
    pub strict: bool,
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
    /// Print the theme this run would paint with, and exit, taking no terminal (`e5.6`).
    ///
    /// The face table, the measured contrast of every face against the background the theme paints, and
    /// how far the added/removed/violation pairs stay apart when protanopia and deuteranopia are
    /// simulated. `--audit` makes a failed floor an error exit, so a palette can be gated the way a
    /// codebase is.
    #[arg(long)]
    pub show_theme: bool,
    /// With `--show-theme`: treat a failed contrast or separation floor as an error exit (exit 7).
    #[arg(long, requires = "show_theme")]
    pub audit: bool,
    /// Print every theme Fathom can see — the built-ins and the files in your theme directories — and
    /// exit, taking no terminal (`e5.6`).
    #[arg(long)]
    pub list_themes: bool,
}

/// Run one verb. The return type is the process contract, not a `Result` a caller forgets to check.
///
/// # Errors
///
/// [`Pending`], or anything a verb chose to raise, when the verb cannot be completed; `main` maps that to
/// an exit code at the edge, which is the only place the two are joined.
pub fn dispatch(verb: Option<&Verb>, globals: &Globals) -> eyre::Result<ExitCode> {
    // First, before any verb resolves a root: the miniature is unpacked, indexed, and becomes the working
    // directory, so that `--root`'s default of `.` is the demo and nothing below here knows the difference
    // (`e5.9`).
    if globals.demo {
        enter_demo(globals)?;
    }
    match verb {
        None => open_terminal(globals),
        Some(Verb::Index(args)) => index(args, globals),
        Some(Verb::Show(args)) => query::show(&args.root, &args.episode, args.json),
        Some(Verb::Attribution(args)) => match &args.action {
            AttributionAction::Correct(args) => {
                let Some(target) = args.target.as_deref().or(args.identity.as_deref()) else {
                    return Err(crate::verbs::Pending::new(
                        ExitCode::Usage,
                        "`fathom attribution correct` needs a commit or an --identity key"
                            .to_owned(),
                        Some("fathom attribution correct HEAD --kind human --root .".to_owned()),
                    )
                    .into());
                };
                crate::attribution::correct(
                    std::path::Path::new(&args.root),
                    target,
                    &args.kind,
                    args.agent.as_deref(),
                    args.note.as_deref(),
                    args.identity.as_deref(),
                    args.json,
                )
            }
            AttributionAction::List(args) => {
                crate::attribution::list(std::path::Path::new(&args.root), args.json)
            }
            AttributionAction::Forget(args) => crate::attribution::forget(
                std::path::Path::new(&args.root),
                &args.identity,
                args.json,
            ),
        },
        Some(Verb::Review(args)) => crate::review::run(args),
        Some(Verb::Analyze(args)) => crate::analyze::analyze(
            &args.root,
            args.episode.as_deref(),
            args.json,
            args.schema,
            args.check,
            args.write,
        ),
        Some(Verb::Why(args)) => {
            if args.weights {
                return crate::why::print_weights(args.json);
            }
            let set = args.calibrate.as_deref().filter(|set| !set.is_empty());
            crate::why::why(
                &args.root,
                args.episode.as_deref(),
                args.json,
                args.calibrate.as_ref().map(|_| set),
                &args.weight,
                args.tau,
                args.cut,
            )
        }
        Some(Verb::Stats(args)) => query::stats(&args.root, args.json),
        Some(Verb::Check) => Err(not_implemented("fathom check", "e6.6")),
        Some(Verb::Doctor(args)) => {
            diagnose::doctor(&args.root, args.privacy, args.json, args.deep, args.strict)
        }
        Some(Verb::Export(args)) => export(args),
        Some(Verb::Mcp(args)) => mcp(args),
        Some(Verb::Watch(args)) => watch(args),
        Some(Verb::Hooks(args)) => crate::harness::run(args),
        Some(Verb::Tui(args)) => tui(args, globals),
    }
}

/// Unpack the miniature, index it, and move the process into it (`e5.9`).
///
/// The index runs through [`index`] — the same verb, the same sink, the same report — because the thing
/// under review in the first five minutes is Fathom's own behaviour, and a demo that fakes the index would
/// demonstrate nothing about the wait a real repository costs.
fn enter_demo(globals: &Globals) -> eyre::Result<ExitCode> {
    let demo = crate::demo::prepare()?;
    crate::demo::intro(&demo);
    crate::demo::index(&demo, globals)?;
    crate::demo::handoff();
    std::env::set_current_dir(&demo.root).map_err(|err| {
        Pending::new(
            ExitCode::Error,
            format!(
                "the demo repository at {} could not be made the working directory: {err}",
                demo.root.display()
            ),
            Some("run `fathom --demo` from a directory you can read".to_owned()),
        )
    })?;
    // `Ok` is never looked at: `dispatch` continues into the verb the reviewer asked for.
    Ok(ExitCode::Ok)
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
    // `--list-themes` and `--show-theme` take no terminal for the same reason `--show-keys` takes none:
    // the question is answerable from the theme table and the environment, and a reviewer asking it is
    // often doing so precisely because taking a terminal is the thing that went wrong.
    if args.list_themes {
        return Ok(crate::theme::list());
    }
    if args.show_theme {
        let mut probe = fathom_tui::terminal::Probe::from_env_with(|key| std::env::var(key).ok());
        probe.tty = std::io::IsTerminal::is_terminal(&std::io::stdout());
        return crate::theme::show(globals, &probe, args.audit);
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
pub(crate) fn index(args: &IndexArgs, globals: &Globals) -> eyre::Result<ExitCode> {
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
    // The harness event stream, read before the run so the step that captures intent has its evidence
    // (`e4.2`). Absence is the normal case and prints as one, not as a failure.
    let now = crate::clock::now_millis();
    let stream = crate::harness::stream(root, now);
    // Created before the store is opened and finished after the run closes, so the row a terminal is
    // left with is always ended (`e5.9`).
    let guide = crate::guide::Guide::new(crate::guide::Request::decide(
        // The overrides are globals, so the same two flags reach the run `fathom --demo` performs on the
        // reviewer's behalf — a flag that only worked when you had already indexed something yourself would
        // be a flag nobody discovered in time to use.
        globals.progress,
        globals.no_progress,
        args.json,
    ));

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
        now: Some(now),
        // The guided first index (`e5.9`): a row on stderr while the run goes, so a minute of walking
        // a real history is not indistinguishable from a hang. `--json` and `--no-progress` silence it,
        // and a run indexing for a pipe never starts one.
        progress: Some(guide.sink()),
        scope: if args.refresh || args.rebuild {
            Scope::Everything
        } else {
            Scope::NewCommits
        },
        segment: segment.rules.clone(),
        // The harness stream, read for the intent and for nothing else (`e4.2`). Reading it does not
        // consume it: the durable cursor stays where the watcher left it.
        hooks: stream.ledger.clone(),
        refresh: args.refresh || args.rebuild,
        worktree: if args.committed_only {
            Worktree::Ignore
        } else {
            Worktree::Analyse
        },
        ..index::Options::head()
    };
    let report = index::run(&repository, &mut store, &Unclassified, &options);
    // The in-place row ends here, on every path out of the run: a run that left a bare `\r` behind would
    // print its own error over the middle of the last progress row (`e5.9`).
    guide.finish();
    let report = report.map_err(failed)?;
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
            json_report(
                root,
                &store,
                &store_path,
                &report,
                args.rebuild,
                epoch,
                &stream
            )
        ));
    } else if args.stats {
        print_stats(&store_path, &report);
    } else {
        // `--demo` sets `quiet` because stdout is about to carry the frame; every other caller owns
        // stdout and gets the report there (ADR-002 §7.5).
        let quiet = args.quiet;
        let say = move |line: std::fmt::Arguments| {
            if quiet {
                output::problem(line);
            } else {
                output::line(line);
            }
        };
        let added = report.spine.commits_written;
        say(format_args!(
            "{}: {}{} commit(s) walked, {} new, {} analyse unit(s) computed in {} ms",
            store_path.display(),
            if args.rebuild { "rebuild: " } else { "" },
            report.commits_walked,
            added,
            report.computed(),
            report.elapsed.as_millis()
        ));
        // The intent tally, on the same line the run reports (`e4.2`). Every bucket is printed, including
        // the empty one and the "nothing recorded" one: the ticket's rule is that missing intent is a state,
        // and a state that only appears when it is non-empty is a state a reviewer never learns to read.
        {
            let redactor = crate::sanitize::Redactor::for_root(root);
            let episodes = &report.episodes;
            let harness = if !stream.events.exists() {
                format!("no stream at {}", redactor.path(&stream.events))
            } else if stream.turns == 0 && stream.refused > 0 {
                format!(
                    "unreadable stream at {} ({} line(s) refused)",
                    redactor.path(&stream.events),
                    stream.refused
                )
            } else {
                format!(
                    "{} turn(s) from {}",
                    stream.turns,
                    redactor.path(&stream.events)
                )
            };
            say(format_args!(
                "  intent: {} recorded ({} from the harness), {} digest-only, {} none recorded · harness: {harness}",
                episodes.intent_recorded,
                episodes.intent_from_hook,
                episodes.intent_digest,
                episodes.intent_absent
            ));
            // Whatever was read and could not be used, on its own line (`e4.2`). A stream that exists but
            // cannot be read must not print as "no stream": the first is a fault to fix, the second is a
            // repository with no adapter, and the intent tally means something different in each.
            for notice in &stream.notices {
                say(format_args!("  stream: {notice}"));
            }
        }
        if !report.worktree_skipped.is_empty() {
            say(format_args!(
                "  {} uncommitted path(s) were not analysed; pass --stats to see why",
                report.worktree_skipped.len()
            ));
        }
        // A run that could not read some of the history says so here, in the plain report as well as in
        // `--stats` and `--json`: "degrade, never block" is only honest if the degradation is said out loud
        // on the path a human actually reads (ADR-001 §7.6). Named by commit, because "one commit failed"
        // is a fact the user cannot act on.
        if !report.failures.is_empty() {
            for failure in report.failures.iter().take(3) {
                say(format_args!("  could not read {failure}"));
            }
            if report.failures.len() > 3 {
                say(format_args!(
                    "  {} more commit(s) could not be read",
                    report.failures.len() - 3
                ));
            }
            say(format_args!(
                "  {} commit(s) are missing from the index: the objects are the problem, not the index — \
                 `git fsck --full` in the repository (fathom never fetches or repairs), then `fathom index`, \
                 and `fathom doctor` repeats this until it stops",
                report.failures.len()
            ));
        }
        say(format_args!(
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
        // Commits this run could not read are INDEX_DEGRADED in the published sense — the index is
        // incomplete, and the line above names the remedy. A script that runs `fathom index` in a
        // pre-flight check should not have to parse prose to learn that part of the history is missing.
        _ if !report.failures.is_empty() => ExitCode::IndexDegraded,
        _ => ExitCode::Ok,
    })
}

/// The `--json` shape, assembled here rather than derived from [`index::Report`]'s own serde so that the
/// key names are the CLI's promise and not a struct's internals: a field renamed in `fathom-analyze` must
/// not silently rename `fathom index --json`'s contract.
#[cfg(feature = "sqlite-bundled")]
fn json_report(
    root: &std::path::Path,
    store: &fathom_store::spine::Store,
    store_path: &std::path::Path,
    report: &fathom_analyze::index::Report,
    rebuild: bool,
    epoch: i64,
    stream: &crate::harness::Stream,
) -> serde_json::Value {
    use serde_json::json;

    let redactor = crate::sanitize::Redactor::for_root(root);
    json!({
        "contract": query::INDEX_CONTRACT,
        // The store is named *relative to the repository it belongs to*: an absolute path in a machine-
        // readable report is a path the reader cannot use and a fact about the machine that produced it.
        "store": redactor.path(store_path),
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
        // The intent capture's tallies, and what stream they came from (`e4.2`). `events_file` is printed
        // even when it does not exist, so "no intent" can be told apart from "no hooks" without a second
        // command.
        "intent": {
            "recorded": report.episodes.intent_recorded,
            "from_hook": report.episodes.intent_from_hook,
            "digest_only": report.episodes.intent_digest,
            "none": report.episodes.intent_absent,
            "harness": {
                "events_file": redactor.path(&stream.events),
                "exists": stream.events.exists(),
                "turns": stream.turns,
                "events": stream.accepted,
                "refused": stream.refused,
                // One line per thing the reader found and could not use, including a stream that exists but
                // would not parse: "no stream" and "unreadable stream" are different reasons for the same
                // empty intent tally, and only one of them is the repository's fault.
                "notices": stream.notices,
            },
        },
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
        // Every commit this store cannot read, named by id: the machine-readable twin of the
        // `object_unreadable` finding `fathom doctor` and `fathom stats` print, and the reason a script does
        // not have to scrape stderr to find out which commit is missing.
        "unreadable": store
            .unreadable_commits()
            .unwrap_or_default()
            .into_iter()
            .map(|(commit, reason, seen_utc)| {
                json!({"commit": commit, "reason": reason, "seen_utc": seen_utc})
            })
            .collect::<Vec<_>>(),
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
pub(crate) fn index(args: &IndexArgs, globals: &Globals) -> eyre::Result<ExitCode> {
    let _ = (args, globals);
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
    // cannot drift apart (§5.1), and its error reaches the right exit code through `Diagnosable`.
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
                "from": short_sha(from.as_deref()),
                "to": short_sha(to.as_deref()),
                "commits": if same { 1 } else { 0 },
                "resolved": same,
                // The artefact will be written from the delta model, and says which one it read. This is
                // the half of `e5.7` that can be checked before the rendering exists: an export that
                // quietly invented its own section list is the second reality ADR-001 §7.10 forbids, so
                // the scope names the contract, the version and every section's state instead.
                "delta": delta_scope(resolved, to.as_deref()),
            })
        }
        Err(_) => json!({"asked": episodes, "resolved": false}),
    }
}

/// Seven characters, because a scope line is not a place to print an oid.
fn short_sha(oid: Option<&str>) -> Option<String> {
    oid.map(|value| value.chars().take(7).collect::<String>())
}

/// The delta model an export of this range would be written from.
///
/// `fathom analyze <episode> --json` is the source; this is that same document, reduced to what an
/// artefact's preamble has room for: the contract, the version, and the state of each of the seven
/// sections with the owner of every gap. When there is no index the answer is a stated absence with the
/// remedy, not an empty section list — an export that printed nothing about a section and nothing about
/// the gap would be reporting zero where nobody looked.
fn delta_scope(root: &std::path::Path, oid: Option<&str>) -> serde_json::Value {
    #[cfg(not(feature = "sqlite-bundled"))]
    {
        let _ = (root, oid);
        return serde_json::json!({
            "available": false,
            "contract": fathom_analyze::insight::CONTRACT,
            "why": "this binary was built without a SQLite feature, so no index can be read",
        });
    }

    #[cfg(feature = "sqlite-bundled")]
    {
        use fathom_analyze::index::default_store_path;
        use fathom_store::spine::Store;
        let Some(oid) = oid else {
            return serde_json::json!({"available": false, "why": "the range resolved to no commit"});
        };
        let store_path = default_store_path(root);
        if !store_path.exists() {
            return serde_json::json!({
                "available": false,
                "contract": fathom_analyze::insight::CONTRACT,
                "why": format!("no index at {} yet", store_path.display()),
                "remedy": "fathom index",
            });
        }
        let Ok(store) = Store::open(&store_path) else {
            return serde_json::json!({
                "available": false,
                "contract": fathom_analyze::insight::CONTRACT,
                "why": "the store would not open",
                "remedy": "fathom doctor",
            });
        };
        match fathom_analyze::insight::for_commit(&store, oid) {
            Ok(Some(delta)) => serde_json::json!({
                "available": true,
                "contract": delta.contract,
                "schema_version": delta.schema_version,
                "episode": fathom_analyze::insight::short_id(&delta.episode.id),
                "sections": delta
                    .sections
                    .iter()
                    .map(|section| {
                        serde_json::json!({
                            "id": section.id,
                            "state": section.state,
                            "gap_owner": section.gap.as_ref().map(|gap| gap.owner.clone()),
                        })
                    })
                    .collect::<Vec<_>>(),
                "gaps": delta.gaps.len(),
                "verified": delta.integrity.verified,
            }),
            Ok(None) => serde_json::json!({
                "available": false,
                "contract": fathom_analyze::insight::CONTRACT,
                "why": "this commit has no episode row in the index",
                "remedy": "fathom index",
            }),
            Err(err) => serde_json::json!({
                "available": false,
                "contract": fathom_analyze::insight::CONTRACT,
                "why": format!("reading the delta failed: {err}"),
                "remedy": "fathom index --rebuild",
            }),
        }
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
