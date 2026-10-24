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
    /// Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`).
    ///
    /// An open shell watches its repository by default, so a commit made in another terminal arrives by
    /// itself. This turns that off, and the frame then says `snapshot` and what would move it rather than
    /// letting a still rail be read as a quiet repository. The other way to get the same picture, cheaper,
    /// is to run `fathom watch` in a second terminal and let it own the indexing.
    #[arg(long = "no-watch", global = true)]
    pub no_watch: bool,
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
    /// `--no-watch`: an open frame does not watch the repository (`reco-bpc`).
    pub no_watch: bool,
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
    /// List the episodes — the rail's rows, headless (`reco-11b`)
    Episodes(EpisodesArgs),
    /// Report or correct who Fathom thinks wrote a commit (`e4.1`)
    Attribution(AttributionArgs),
    /// The review ledger: sign-offs that outlive the history they were written about (`e4.6`)
    Review(ReviewArgs),
    /// Print one episode's delta document — the model the TUI, the export and an agent all read
    Analyze(AnalyzeArgs),
    /// Read one episode: the narrative every claim in it cites its evidence from (`e4.7`)
    Narrative(NarrativeArgs),
    /// What the plan said against what the session did — the contradictions, both sides linked (`e4.8`)
    Drift(DriftArgs),
    /// Print one episode's risk score with every feature, its weight and its contribution (`e4.4`)
    Why(WhyArgs),
    /// Report repo scale, index state and cache hit rate
    Stats(StatsArgs),
    /// Run the checks CI cares about; findings above the threshold exit 7
    Check(CheckArgs),
    /// Describe this build, its environment and its index, with a remedy per finding
    Doctor(DoctorArgs),
    /// Write a review artefact for a pull request or an audit
    Export(ExportArgs),
    /// Speak MCP to a coding agent
    Mcp(McpArgs),
    /// Watch the repository and index as it changes
    Watch(WatchArgs),
    /// The glance pane for the terminal beside the agent: latest episode, queue, index, one reason (`reco-8bx`)
    Ambient(AmbientArgs),
    /// Report on the harness event stream: lag, offset, rejections, replay (`e6.1`)
    Hooks(HooksArgs),
    /// The one process per repository that watches, tails the harness stream and indexes (`e6.3`)
    Daemon(DaemonArgs),
    /// Who is keeping this index current, what it costs, and what is left (`e6.3`)
    Status(StatusArgs),
    /// Print the terminal UI's keymap, or open the UI itself
    Tui(TuiArgs),
    /// Open an episode's file at its line in `$EDITOR` — jump out, do not read around it (`e5.8`)
    Open(OpenArgs),
    /// Show one file's two sides in your own difftool — difftastic, meld, kdiff3, `vim -d` (`e5.8`)
    Difftool(DifftoolArgs),
    /// Compare any two episodes or revisions: the composed delta over a range (`e5.3`)
    Cmp(CmpArgs),
    /// Search the whole index from one box, in the shell's own ranking (`e5.2`)
    Search(SearchArgs),
    /// The architecture scorecard: five dimensions, one number, and the drift since your last review
    /// (`e3.9`)
    Architecture(ArchitectureArgs),
    /// The ratchet: the committed allowance ledger, which tightens on its own and widens only when told to
    /// (`e3.10`)
    Ratchet(RatchetArgs),
    /// What this repository knows about a path or an episode: the recorded decisions and the open questions
    /// (`e6.5`)
    Context(ContextArgs),
    /// The review memory: notes, decisions, questions and follow-ups, committed under `.fathom/reviews/`
    /// (`e6.5`)
    Note(NoteArgs),
}

/// `fathom cmp` — compare two endpoints (`e5.3`, `reco-ifo`).
///
/// One argument, which is either a spec — `<from>..<to>`, git's own "from `a` up to `b`" — or a name saved
/// with `--save`. The spec is deliberately shareable: it is the line a reviewer pastes into a message, and
/// `branch@rev` on either side is what makes a cross-branch comparison mean one thing to the next reader.
#[derive(Args, Debug)]
pub struct CmpArgs {
    /// The comparison: `HEAD~9..HEAD`, `77aa0c..e8f1a2`, `main@1a2b3c..feat/x@9f8e7d`, or a saved name.
    pub spec: Option<String>,
    /// The repository to read.
    #[arg(long, default_value = ".")]
    pub root: String,
    /// Print the comparison as one JSON line on stdout (`fathom.compare/1`).
    #[arg(long)]
    pub json: bool,
    /// Keep this comparison under a name, so the next person says `since-the-refactor` instead of typing
    /// the endpoints.
    #[arg(long, value_name = "NAME")]
    pub save: Option<String>,
    /// Why the name exists, kept with it.
    #[arg(long, requires = "save")]
    pub note: Option<String>,
    /// List the saved comparisons and exit.
    #[arg(long = "list")]
    pub list: bool,
}

/// `fathom search` — the one search box, headless (`e5.2`, `reco-mkc`).
///
/// The same corpus, the same scorer and the same filter grammar the `/` overlay runs, for the two cases the
/// overlay cannot serve: a script that wants hits as JSON, and a reviewer in a terminal with no index
/// open yet who wants the answer without taking a screen. `/` and this verb cannot drift, because they are
/// the same [`fathom_analyze::search::Index`] question.
#[derive(Args, Debug)]
pub struct SearchArgs {
    /// The query, with the same filter tokens `/` takes (`author:`, `since:`, `risk>`, `path:`, `dep:`, …).
    pub query: Vec<String>,
    /// The repository to read.
    #[arg(long, default_value = ".")]
    pub root: String,
    /// Print the report as one JSON line on stdout (`fathom.search/1`).
    #[arg(long)]
    pub json: bool,
    /// How many hits to print.
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
    /// Keep this query under a name, so the `,` palette can offer it again.
    #[arg(long, value_name = "NAME")]
    pub save: Option<String>,
    /// List the saved searches and exit.
    #[arg(long)]
    pub list: bool,
}

/// `fathom architecture` — the scorecard, headless (`e3.9`, `reco-al1`).
///
/// The five dimensions, their arithmetic, the trend across what the index holds and the delta since the
/// reviewer's last review, in one document. `fathom check` (**reco-ngk**) and the exported artefact
/// (**e5.7**) read the same `fathom-architecture/1` payload this prints with `--json`, so the CI log and the
/// dashboard cannot disagree with the terminal about what 61 means.
#[derive(Args, Debug)]
pub struct ArchitectureArgs {
    /// The repository to read.
    #[arg(long, default_value = ".")]
    pub root: String,
    /// Print the report as one JSON line on stdout (`fathom-architecture/1`).
    #[arg(long)]
    pub json: bool,
    /// Score this revision rather than the newest the graph holds: a full tree oid or an unambiguous prefix.
    #[arg(long, value_name = "TREE")]
    pub revision: Option<String>,
    /// Score this many unscored revisions first, newest first, before reporting (`0`: report what is stored).
    ///
    /// Each one costs a graph materialisation, which is why the default is none: a trend that recomputed
    /// every revision on every run would be a trend nobody runs.
    #[arg(long, default_value_t = 0, value_name = "N")]
    pub backfill: usize,
    /// How many scored revisions the trend reads; bare, or `0`, is every row the index holds.
    #[arg(
        long,
        default_value_t = 0,
        default_missing_value = "0",
        num_args = 0..=1,
        value_name = "N"
    )]
    pub history: i64,
    /// Forget every stored scorecard first, and re-derive what this run needs.
    ///
    /// The recovery path for a row that disagrees with its revision — the honest fix for a derived fact is to
    /// re-derive it, not to patch it.
    #[arg(long)]
    pub refresh: bool,
    /// Do not write anything: read the stored rows and score only what this run needs.
    ///
    /// A dashboard polling on a schedule, or anyone reading a store another process is writing.
    #[arg(long)]
    pub read_only: bool,
    /// Print the knobs in force — the weights, the curves and their published constants — and exit.
    #[arg(long)]
    pub explain: bool,
}

/// `fathom ratchet` — the committed baseline and the act of turning it (`e3.10`, `reco-5ht`).
///
/// Bare, it reports: what is forgiven, what is about to stop being forgiven, what is back due, and what has
/// no allowance at all — and it removes the rows whose violation has gone away, because tightening is the
/// one direction the file may move without being asked (`--dry-run` watches instead of doing). Widening is
/// the other direction and needs both `--update-baseline` and `--reason`: the reason, the owner and the
/// expiry land in the file, in the printed diff and in the review memory, which is what makes a loosening a
/// decision with a name on it instead of a line that appeared.
#[derive(Args, Debug)]
pub struct RatchetArgs {
    /// The repository to read.
    #[arg(long, default_value = ".")]
    pub root: String,
    /// Print the report as one JSON line on stdout (`fathom-ratchet/1`).
    #[arg(long)]
    pub json: bool,
    /// Raise the baseline: write the violations that have no allowance as new rows.
    ///
    /// The one flag that makes the file looser, which is why it asks for a reason in the same breath.
    #[arg(long, requires = "reason")]
    pub update_baseline: bool,
    /// Why the baseline is being raised — required by `--update-baseline`, recorded in the file, the diff and
    /// the review memory.
    #[arg(long, value_name = "REASON")]
    pub reason: Option<String>,
    /// Who owns the new allowances; bare, the reviewer's own identity (`--identity`, `$FATHOM_REVIEWER`,
    /// `git config user.name`).
    #[arg(long, value_name = "OWNER")]
    pub owner: Option<String>,
    /// Who is asking, recorded beside the owner as `granted_by`; bare, the same identity as `--owner`.
    #[arg(long, value_name = "WHO")]
    pub granted_by: Option<String>,
    /// The last day an allowance is live, `YYYY-MM-DD`, or `never` for a row with no expiry.
    ///
    /// Bare, `+90` days: an allowance with no end is what the file exists to refuse, so the default is a
    /// quarter and saying `never` is a sentence a reviewer gets to read.
    #[arg(long, value_name = "DATE|never")]
    pub expires: Option<String>,
    /// Grant only this key (repeatable): a violation key as the report prints it, or `kind=key`.
    #[arg(long = "key", value_name = "KEY")]
    pub key: Vec<String>,
    /// Report what would change and write nothing — not even the automatic tightening.
    #[arg(long)]
    pub dry_run: bool,
    /// Judge this graph revision rather than the newest the index holds: an oid or an unambiguous prefix.
    #[arg(long, value_name = "TREE")]
    pub revision: Option<String>,
    /// Do not write a `decision` note into the review memory when the baseline is raised.
    ///
    /// The interlock is on by default: the reason a cycle is allowed is exactly the knowledge a TOML row
    /// cannot carry on its own (`e6.5`'s memory, `e3.10`'s write).
    #[arg(long)]
    pub no_note: bool,
}

/// `fathom context <path-or-episode>` — what this repository knows about one thing (`e6.5`, `reco-552`).
///
/// The question this answers is the one a review leaves behind: *why is it like this, and what did we say we
/// would come back to?* The ask is either an episode (`ep-…`, or a prefix) or a path (a file or a directory,
/// matched the way the rail's `--path` filter matches it). The answer is the recorded decisions and the open
/// questions, each with its author, its date and the evidence it cites — including the ledger's sign-offs,
/// which are the formal half of "what was decided" and are printed as themselves, labelled `ledger`.
#[derive(Args, Debug)]
pub struct ContextArgs {
    /// The path or episode to ask about. A path may be a file or a directory fragment.
    pub target: String,
    /// The repository to read.
    #[arg(long, default_value = ".")]
    pub root: String,
    /// Print the answer as one JSON line on stdout (`fathom.cli.context/1`).
    #[arg(long)]
    pub json: bool,
    /// Include answered and withdrawn notes in the lists, not just the live ones.
    #[arg(long)]
    pub all: bool,
}

/// `fathom note …` — the review memory's verbs (`e6.5`, `reco-552`).
#[derive(Args, Debug)]
pub struct NoteArgs {
    /// The repository to read.
    #[arg(long, default_value = ".")]
    pub root: String,
    /// Print the report as one JSON line on stdout (`fathom.cli.note/1`).
    #[arg(long, global = true)]
    pub json: bool,
    /// What to do with the memory; bare `fathom note` is `note list`.
    #[command(subcommand)]
    pub action: Option<NoteAction>,
}

/// What to do with the review memory.
#[derive(Subcommand, Debug)]
pub enum NoteAction {
    /// Record a note, decision, question or follow-up against an episode or a path
    Add(NoteAddArgs),
    /// List what is recorded, oldest first
    List(NoteListArgs),
    /// Show one note with its evidence and its link verdict
    Show(NoteShowArgs),
    /// Answer a question or close a follow-up by appending the note that closes it
    Answer(NoteAnswerArgs),
    /// Take a recorded decision back, by appending the note that retracts it
    Withdraw(NoteWithdrawArgs),
    /// Re-hash every note against its own `digest:` line — tampering, and nothing else
    Verify,
}

/// `fathom note add`.
#[derive(Args, Debug)]
pub struct NoteAddArgs {
    /// The text of the note. One line in the file either way: paragraph breaks are folded, never wrapped.
    pub text: Option<String>,
    /// The episode to attach it to, or an unambiguous prefix of its id.
    #[arg(long, conflicts_with = "path")]
    pub episode: Option<String>,
    /// The path to attach it to, optionally `path:line`. Resolved to the newest episode that touched it when
    /// the index knows one.
    #[arg(long)]
    pub path: Option<String>,
    /// What kind of note this is: `decision`, `question`, `follow-up`, `annotation` (`note` and `flag` are
    /// accepted as `annotation`).
    #[arg(long, default_value = "annotation")]
    pub kind: String,
    /// Who is writing, when git config and `fathom.toml` do not say.
    #[arg(long)]
    pub author: Option<String>,
    /// Record the note as an agent's act on someone's behalf, not as a human's.
    #[arg(long)]
    pub agent: Option<String>,
    /// The note this one answers, by `nt-…` id.
    #[arg(long)]
    pub about: Option<String>,
}

/// `fathom note list`.
#[derive(Args, Debug)]
pub struct NoteListArgs {
    /// Only notes attached to this episode or touching this path.
    pub target: Option<String>,
    /// Only this kind: `decision`, `question`, `follow-up`, `annotation`.
    #[arg(long)]
    pub kind: Option<String>,
    /// Only notes in this status: `open`, `answered`, `withdrawn`.
    #[arg(long)]
    pub status: Option<String>,
    /// How many to print, oldest first; `0` is every note.
    #[arg(long, default_value_t = 0)]
    pub limit: usize,
}

/// `fathom note show`.
#[derive(Args, Debug)]
pub struct NoteShowArgs {
    /// The note id (`nt-…`).
    pub id: String,
}

/// `fathom note answer` — close a question by appending, which is how a memory stays honest.
#[derive(Args, Debug)]
pub struct NoteAnswerArgs {
    /// The question or follow-up being answered (`nt-…`).
    pub id: String,
    /// The answer.
    pub text: String,
    /// Who is answering.
    #[arg(long)]
    pub author: Option<String>,
    /// Record the answer as an agent's act on someone's behalf.
    #[arg(long)]
    pub agent: Option<String>,
    /// The kind of the answering note: a `decision` is an answer that settles it, an `annotation` is an
    /// answer that informs it.
    #[arg(long, default_value = "decision")]
    pub kind: String,
}

/// `fathom note withdraw` — retract a recorded decision without editing it.
#[derive(Args, Debug)]
pub struct NoteWithdrawArgs {
    /// The note being retracted (`nt-…`).
    pub id: String,
    /// Why it no longer holds.
    pub text: String,
    /// Who is retracting it.
    #[arg(long)]
    pub author: Option<String>,
    /// Record the retraction as an agent's act on someone's behalf.
    #[arg(long)]
    pub agent: Option<String>,
}

/// `fathom episodes` — the reviewer's main list, for a script or CI (`reco-11b`).
///
/// `fathom show` answers a question about one episode; this answers the question the rail asks about the
/// whole index: how many episodes are there, which of them are unread, who wrote each, how big was it, and
/// what is left. ADR-006 §6.7 refuses a fact that only exists on a screen, and until now the rail's rows
/// were exactly such a fact — an acceptance script could not put a number on "the rail shows 213
/// episodes". The filters are the rail's own (`fathom_store::episode::EpisodeQuery`, the shape the shell
/// filters with), so a filtered count here is the same count the shell drew.
#[derive(Args, Debug)]
pub struct EpisodesArgs {
    /// The repository to read.
    #[arg(long, default_value = ".")]
    pub root: String,
    /// Print the list as one JSON value on stdout (`fathom.cli.episodes/1`), and nothing else there.
    #[arg(long)]
    pub json: bool,
    /// How many rows to print, newest first; `0` is every row the filters select.
    ///
    /// The rail can only ever hold 500 rows at a time; this verb is not a rail, so a larger page is allowed,
    /// and `page.truncated` says when the index held more than the page.
    #[arg(long, default_value_t = 500, value_name = "N")]
    pub limit: i64,
    /// Skip this many rows before printing, for paging with `--limit`.
    #[arg(long, default_value_t = 0, value_name = "N")]
    pub offset: i64,
    /// Only episodes by this author, exactly as git recorded the name; repeatable.
    #[arg(long, value_name = "NAME")]
    pub author: Vec<String>,
    /// Only episodes whose author kind is this — `agent`, `human` or `unknown`; repeatable.
    #[arg(long = "kind", value_name = "KIND")]
    pub kinds: Vec<String>,
    /// Only episodes in this review state: `unread`, `read`, `accepted`, `flagged`, `dismissed`, `snoozed`.
    #[arg(long, value_name = "STATE")]
    pub state: Option<String>,
    /// Only episodes that touched a path containing this fragment.
    #[arg(long, value_name = "FRAGMENT")]
    pub path: Option<String>,
    /// Only episodes whose last commit is at or after this date — `7d`, `2w`, `today`, `yesterday`,
    /// `2026-10-01`: the same grammar `fathom search since:` takes.
    #[arg(long, value_name = "WHEN")]
    pub since: Option<String>,
    /// Only episodes whose last commit is before this date, in the same grammar as `--since`.
    #[arg(long, value_name = "WHEN")]
    pub until: Option<String>,
    /// Include episodes no live commit belongs to — the ghosts a rebase, an amend or a force-push left.
    #[arg(long)]
    pub include_dead: bool,
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
    /// Remove the commit rows no ref can reach — what a rebase, an `--amend` or a force-push leaves behind.
    ///
    /// The only verb that deletes anything (ADR-004 §8.1). It runs after this run, against the ref tips this
    /// run just read, and it refuses rather than guesses: rows the store cannot prove are dead stay, and so
    /// does any row a review or a sign-off points at. `fathom stats` prints the count and names this flag.
    #[arg(long)]
    pub prune: bool,
    /// Do not write the module graph (`e3.11`).
    ///
    /// The escape hatch for a run that cannot afford it: one revision's graph costs one structural pass
    /// over its live source corpus, which is the expensive thing this index does. `[graph] enabled = false`
    /// in the repository's own `fathom.toml` is the durable form of the same refusal, and `graph.window` /
    /// `graph.max_corpus_mib` are the knobs that make it affordable instead of off. With either, the
    /// architecture surfaces say they have no graph rather than guessing without one.
    #[arg(long = "no-graph")]
    pub no_graph: bool,
    /// With `--prune`: print what would be removed and remove nothing.
    #[arg(long, requires = "prune")]
    pub dry_run: bool,
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

/// `fathom narrative` — the prose half of an episode (`e4.7`, `reco-2rx`).
///
/// The same handle grammar as `analyze`, `why` and `show`, because a reviewer who has an episode should not
/// have to learn which verb wants which spelling of it.
#[derive(Args, Debug)]
pub struct NarrativeArgs {
    /// The episode to read: an episode id, a commit, a rev (`HEAD~3`, `main`, `v1.2^{}`) or an oid prefix.
    /// Omitted with `--attribution`, which answers a question about the build instead of an episode.
    pub episode: Option<String>,
    /// The repository to read.
    #[arg(long, default_value = ".")]
    pub root: String,
    /// Print the claims, citations and weights as JSON on stdout, and nothing else there.
    #[arg(long)]
    pub json: bool,
    /// Print the Markdown artefact body — the exact text `fathom export` will carry.
    #[arg(long)]
    pub markdown: bool,
    /// Print the ranking beside the prose: every claim's weight, so the order can be argued with.
    #[arg(long)]
    pub explain: bool,
    /// How many claims the *Read this first* paragraph holds (1 to 6).
    #[arg(long, value_name = "N")]
    pub top: Option<usize>,
    /// Print whether a model wrote any of this, and the five gates a polish path must clear. Needs no
    /// index and no episode.
    #[arg(long = "attribution")]
    pub attribution: bool,
}

/// `fathom drift` — plan versus execution across a session (`e4.8`, `reco-cpr`).
///
/// Every flag here is a bound on the read or on the report, never on what is true: the window, the
/// threshold and the caps are the analysis's own and the report prints them beside its verdict.
#[derive(Args, Debug)]
pub struct DriftArgs {
    /// The repository to read.
    #[arg(long, default_value = ".")]
    pub root: String,
    /// Compare one harness session only — its id, or `lineage:refs/heads/main` for the episodes no session
    /// was named for.
    #[arg(long, value_name = "S")]
    pub session: Option<String>,
    /// Read every episode in the index instead of the newest ones. A long read, and a longer report.
    #[arg(long)]
    pub all: bool,
    /// How many episodes past a plan item to look for the change that did it.
    #[arg(long, value_name = "N")]
    pub window: Option<usize>,
    /// The token overlap at which a plan item counts as addressed (0.05 to 1.0).
    #[arg(long, value_name = "F")]
    pub min_overlap: Option<f64>,
    /// The cap on the report.
    #[arg(long, value_name = "N")]
    pub max: Option<usize>,
    /// A suppression key already argued about; its contradiction is counted, not re-reported. Repeatable.
    #[arg(long = "dismiss", value_name = "KEY")]
    pub dismiss: Vec<String>,
    /// Switch one kind off, in the published spelling (`plan-unimplemented`, …). Repeatable.
    #[arg(long = "off", value_name = "KIND")]
    pub off: Vec<String>,
    /// Print the report as JSON on stdout, and nothing else there.
    #[arg(long)]
    pub json: bool,
    /// Exit 7 when anything was found, so a CI step can gate on it.
    #[arg(long)]
    pub fail_on_findings: bool,
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

/// `fathom check` — the CI gate (`e6.6`, `reco-ngk`).
///
/// Six checks over one range, one exit code, five formats. The policy lives in `[check]` in the
/// repository's own `fathom.toml`; every flag here is a statement about *this run* and overrides it, so a
/// pipeline can say "this job fails on warnings" without rewriting the file the whole team reads.
#[derive(Args, Debug)]
pub struct CheckArgs {
    /// The repository to check.
    #[arg(long, default_value = ".")]
    pub root: String,
    /// The far end of the range: what the change is compared against.
    ///
    /// Defaults to `[check] base` in `fathom.toml`, and to `HEAD~1` when the file says nothing. A shallow
    /// clone that does not have it is not an error: the two checks that need both trees report themselves
    /// blocked, with the fetch command that fixes them.
    #[arg(long, value_name = "REV")]
    pub base: Option<String>,
    /// The near end of the range. Defaults to `[check] head`, then to `HEAD`.
    #[arg(long, value_name = "REV")]
    pub head: Option<String>,
    /// The range as one argument, `base..head` — what a reviewer pastes into a message.
    #[arg(long, value_name = "BASE..HEAD")]
    pub range: Option<String>,
    /// The output format: `human`, `json`, `sarif`, `github` (step annotations) or `markdown` (job summary).
    #[arg(long, value_name = "FMT", default_value = "human")]
    pub format: String,
    /// Print the JSON document — the same thing as `--format json`.
    #[arg(long)]
    pub json: bool,
    /// Write the artefact to this file instead of to stdout.
    #[arg(long, value_name = "PATH")]
    pub out: Option<String>,
    /// Where to append the markdown job summary. Defaults to `$GITHUB_STEP_SUMMARY` when the runner set one.
    #[arg(long, value_name = "PATH")]
    pub summary: Option<String>,
    /// Do not write a job summary at all, even with `$GITHUB_STEP_SUMMARY` set.
    #[arg(long = "no-summary")]
    pub no_summary: bool,
    /// Do not print GitHub step annotations beside the report.
    #[arg(long = "no-annotations")]
    pub no_annotations: bool,
    /// The severity at or above which the run exits 7: `note`, `warning`, `error` or `never`.
    ///
    /// Overrides `[check] fail_on`. `never` makes the gate a reporter: findings are listed, nothing fails.
    #[arg(long, value_name = "LEVEL")]
    pub fail_on: Option<String>,
    /// Set one check's volume for this run, e.g. `--severity tests=off`. Repeatable.
    #[arg(long, value_name = "CHECK=LEVEL")]
    pub severity: Vec<String>,
    /// Run only these checks. Repeatable; the others are reported as off, not omitted.
    #[arg(long, value_name = "CHECK")]
    pub only: Vec<String>,
    /// Do not run these checks. Repeatable.
    #[arg(long, value_name = "CHECK")]
    pub skip: Vec<String>,
    /// Checks that must *run*: one that cannot is a failure rather than a notice. Repeatable.
    #[arg(long, value_name = "CHECK")]
    pub require: Vec<String>,
    /// Silence one finding or family for this run: `tests`, or `layering:no_upward`. Repeatable.
    ///
    /// The command-line twin of `[check] ignore`, and it obeys the same rule — the family half must name a
    /// check, because a pattern that silences nothing is discovered at review time otherwise.
    #[arg(long, value_name = "CHECK[:RULE]")]
    pub ignore: Vec<String>,
    /// Cap the findings listed per check (the counts stay whole). Overrides `[check] max_findings`.
    #[arg(long, value_name = "N")]
    pub max: Option<usize>,
    /// Print the policy in force — every check's volume, every threshold — and exit 0 without checking.
    #[arg(long)]
    pub explain: bool,
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

/// `fathom ambient` — the glance pane, in a process of its own (`reco-8bx`).
///
/// The flags are the ones a second terminal needs and nothing else: how often to look, how long to run, how
/// wide to draw, and who is doing the indexing. There is no `--quiet`, because the pane's quiet is designed
/// rather than configured, and no `--verbose`, because a glance pane that grew a seventh column stopped
/// being a glance.
#[derive(Args, Debug)]
pub struct AmbientArgs {
    /// The repository to read.
    #[arg(long, default_value = ".")]
    pub root: String,
    /// Print the pane as one JSON object per change (machine contract `fathom.cli.ambient/1`).
    #[arg(long)]
    pub json: bool,
    /// Print the pane once and exit; never take the screen.
    #[arg(long)]
    pub once: bool,
    /// Draw the pane as text with no terminal taken (ADR-006 §6.7).
    #[arg(long)]
    pub plain: bool,
    /// How often to look, in milliseconds. 400 is what meets the "under a second" promise at near-zero cost.
    #[arg(long, default_value_t = 400)]
    pub interval_ms: u64,
    /// Stop after this many seconds.
    #[arg(long)]
    pub deadline: Option<u64>,
    /// Never index from the pane: read whatever the last index run left, whoever ran it.
    #[arg(long)]
    pub no_index: bool,
    /// Episodes to index per slice when this pane is the one keeping the index current.
    #[arg(long, default_value_t = 200)]
    pub chunk_units: usize,
    /// Render at this width instead of the terminal's — for a pipe, a test, or a tmux status line.
    #[arg(long)]
    pub width: Option<usize>,
    /// Run to the deadline and print what the pane cost: polls, redraws and this process's own CPU share.
    /// Exits `11` if it blew the budget of 1 % of a core.
    #[arg(long)]
    pub self_check: bool,
}

/// `fathom daemon` — the single-instance watcher, started, stopped or asked (`e6.3`, ADR-004 §7.1).
///
/// The subverbs are the three things a person does with a daemon. With none, `fathom daemon` is
/// `fathom daemon status`, because the question asked most often is "is it running", and a verb that made
/// you remember the subverb would be answered by `ps` instead.
#[derive(Args, Debug)]
pub struct DaemonArgs {
    /// What to do with the daemon; bare `fathom daemon` reports on it.
    #[command(subcommand)]
    pub action: Option<DaemonAction>,
}

/// What to do with the daemon.
#[derive(Subcommand, Debug)]
pub enum DaemonAction {
    /// Take the writer lease and watch this repository from this process, or from a detached one
    Start(DaemonStartArgs),
    /// Ask the daemon to finish its pass, checkpoint and release the lease
    Stop(DaemonStopArgs),
    /// The daemon's own health report (`fathom status` is the same question at the top level)
    Status(StatusArgs),
}

/// `fathom daemon start`.
///
/// Every budget flag is a *ceiling*, not a target, and the daemon enforces it by measuring its own process
/// CPU and sleeping when it has spent more than it is allowed (`fathom_ingest::daemon::Budget`). The
/// defaults are ADR-004 §7.2's: a quarter of one core for the background writer, with a work slice small
/// enough that a TUI attached to the same store gets its input back in well under the ticket's 150 ms.
#[derive(Args, Debug)]
pub struct DaemonStartArgs {
    /// The repository to watch and index.
    #[arg(long, default_value = ".")]
    pub root: String,
    /// Print the health document as JSON instead of lines, and print it once the first pass has run.
    #[arg(long)]
    pub json: bool,
    /// Print nothing but the health document; the heartbeat file is still written.
    #[arg(long)]
    pub quiet: bool,
    /// Run in this process, so a supervisor can supervise it (`e6.3`, ADR-004 §7.1).
    ///
    /// This is the form systemd, a container `init` and a CI step want: stdout and stderr stay attached, and
    /// the process leaves on SIGTERM the same way it leaves on `fathom daemon stop`. Without it, `start`
    /// spawns the detached form and waits for its socket to answer.
    #[arg(long)]
    pub foreground: bool,
    /// Detach into a new session (`setsid`), so a terminal's Ctrl-C cannot reach the daemon later.
    ///
    /// Not a flag a person types: `fathom daemon start` sets it when it re-executes itself in the
    /// background, and it only means anything together with `--foreground`.
    #[arg(long, hide = true)]
    pub detach: bool,
    /// Where to listen, instead of the endpoint derived from the repository (ADR-004 §7.2).
    #[arg(long, value_name = "PATH")]
    pub socket: Option<String>,
    /// Refuse to start rather than run with no IPC endpoint.
    ///
    /// Off by default, because a filesystem that cannot host a socket (an SMB share, some container bind
    /// mount) is a reason to publish less, not a reason to have no daemon: the lease and the heartbeat file
    /// still answer `fathom status`.
    #[arg(long)]
    pub require_socket: bool,
    /// Who is supervising this daemon, recorded in the health document (`systemd`, `docker`, `ci`).
    #[arg(long)]
    pub supervisor: Option<String>,
    /// The CPU ceiling, as a percent of one core. 25 is ADR-004 §7.2's background default.
    #[arg(long, default_value_t = 25)]
    pub cpu_share_pct: u32,
    /// Analysis units per pass while nothing is talking to the daemon.
    #[arg(long, default_value_t = 2_000)]
    pub slice: usize,
    /// Analysis units per pass while a foreground client has just been heard — the yield slice.
    #[arg(long, default_value_t = 50)]
    pub yield_slice: usize,
    /// How many distinct index requests may wait before new ones are refused rather than queued.
    #[arg(long, default_value_t = 64)]
    pub queue_depth: usize,
    /// Harness-stream bytes per pass — the back-pressure knob of ADR-005 §6.5.
    #[arg(long, default_value_t = 1_048_576)]
    pub hook_bytes: u64,
    /// Which change source to use: `auto`, `notify` or `poll`. The daemon is a watcher, so it takes the
    /// watcher's flags for the same meaning.
    #[arg(long, default_value = "auto")]
    pub backend: String,
    /// The debounced polling interval in milliseconds.
    #[arg(long)]
    pub interval_ms: Option<u64>,
    /// The quiescence window in milliseconds.
    #[arg(long)]
    pub debounce_ms: Option<u64>,
    /// The liveness beat in milliseconds, for the heartbeat file and the lease.
    #[arg(long)]
    pub heartbeat_ms: Option<u64>,
    /// Also re-index uncommitted paths when only git's index moved.
    #[arg(long)]
    pub worktree: bool,
    /// Do not write `.fathom/watch/status.json`.
    #[arg(long)]
    pub no_status_file: bool,
    /// Stop after this many seconds — the supervisor's `TimeoutStopSec` seam and a test's.
    #[arg(long)]
    pub deadline: Option<u64>,
    /// Stop after this many loop passes. A test seam, and the way to use the daemon as a one-shot indexer.
    #[arg(long)]
    pub max_cycles: Option<u64>,
}

/// `fathom daemon stop`.
#[derive(Args, Debug)]
pub struct DaemonStopArgs {
    /// The repository whose daemon to stop.
    #[arg(long, default_value = ".")]
    pub root: String,
    /// Print the outcome as one JSON line (`fathom.cli.daemon.stop/1`).
    #[arg(long)]
    pub json: bool,
    /// Ask the daemon at this endpoint instead of the derived one.
    #[arg(long, value_name = "PATH")]
    pub socket: Option<String>,
    /// Who is asking, recorded in the daemon's last line.
    #[arg(long, default_value = "fathom daemon stop")]
    pub reason: String,
}

/// `fathom status` — the daemon's health, and the store's state, in one report (`e6.3`).
///
/// The read-only verb: it never indexes unless asked (`--index`), never takes a lease, and exits with the
/// code the *store* half of the report deserves (ADR-004 §7.4's degraded-read codes), so a monitoring script
/// can branch on the index being stale without parsing prose.
#[derive(Args, Debug)]
pub struct StatusArgs {
    /// The repository to report on.
    #[arg(default_value = ".")]
    pub root: String,
    /// Print the report as one JSON line (machine contract `fathom.cli.status/1`).
    #[arg(long)]
    pub json: bool,
    /// Ask the daemon at this endpoint instead of the derived one.
    #[arg(long, value_name = "PATH")]
    pub socket: Option<String>,
    /// Ask for a pass and wait for it, then report the state after it — the way to make `fathom status`
    /// mean "up to date" rather than "as of whenever".
    #[arg(long)]
    pub index: bool,
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
    /// The file stem to write, or a file name that already carries the format's extension.
    #[arg(long, default_value = "fathom-review")]
    pub out: String,
    /// The episode range to export, e.g. `HEAD~9..HEAD`, one commit, or one episode id.
    #[arg(long, default_value = "HEAD")]
    pub episodes: String,
    /// The repository to read.
    #[arg(long, default_value = ".")]
    pub root: String,
    /// Print the report as JSON on stdout, and nothing else there.
    #[arg(long)]
    pub json: bool,
    /// Write the artefact to stdout instead of a file; the report goes to stderr.
    #[arg(long, conflicts_with = "json")]
    pub stdout: bool,
    /// Exit 7 while the artefact's checklist still has an open item — the form a CI gate wants.
    #[arg(long)]
    pub strict: bool,
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
    /// Serve the tool surface over stdio, which opens no socket (`e6.4`).
    Serve,
    /// Print the tool surface and its bounds as this repository's config resolves them.
    Tools {
        /// The repository whose `[mcp]` config and index to read.
        #[arg(long, default_value = ".")]
        root: String,
        /// The surface as one JSON document on stdout (`fathom.mcp.surface/1`).
        #[arg(long)]
        json: bool,
    },
    /// Mint the token that authorises `mark_reviewed` for exactly one change.
    ApprovalToken {
        /// The change to bind the token to: an episode id or a commit.
        change_id: String,
        /// The repository whose `[mcp]` config holds the approval secret.
        #[arg(long, default_value = ".")]
        root: String,
    },
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
    ///
    /// Without it, `--json` is the same as `--stats`: the run's own frame numbers, as one object
    /// (`reco-08r`).
    #[arg(long, conflicts_with_all = ["show_keys", "show_theme", "list_themes"])]
    pub json: bool,
    /// Print the run's own frame numbers — draws, events, p99, worst, over-budget — as one JSON object
    /// on stdout when the shell gives the screen back (`reco-08r`).
    ///
    /// The same numbers the shell prints as one line on stderr on the way out, and the same ones it
    /// leaves in `.fathom/frame-report.json` for `fathom doctor`; a script that wants them as data asks
    /// for them this way instead of measuring the process from outside a pty.
    #[arg(
        long,
        conflicts_with_all = ["show_keys", "show_theme", "list_themes", "show_session"]
    )]
    pub stats: bool,
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

/// `fathom open <EPISODE>` — the exit from reading to editing (`e5.8`, `reco-dmw`).
///
/// Fathom is a reader, not an editor (ADR-001 §8 non-goal 2). This verb is the door out: it finds the
/// file an episode changed and the line its first hunk touches, works out the argument *your* editor
/// takes for a line, and says what it decided before it runs it.
#[derive(Args, Debug)]
pub struct OpenArgs {
    /// The episode to jump into: an episode id, a commit, a rev (`HEAD~3`, `main`, `v1.2^{}`) or an oid
    /// prefix — the same grammar `fathom show`, `analyze` and `why` accept. With no argument, `HEAD`.
    pub episode: Option<String>,
    /// The repository to read.
    #[arg(long, default_value = ".")]
    pub root: String,
    /// Which of the episode's files to open, by path or by a unique suffix of it.
    ///
    /// With more than one file in the episode and no `--path`, the biggest non-noise change is chosen —
    /// and printed, so a surprise is at least a visible one.
    #[arg(long, value_name = "PATH")]
    pub path: Option<String>,
    /// Which line, overriding the line the first `@@` header says.
    #[arg(long, value_name = "N")]
    pub line: Option<u32>,
    /// Which editor to use for this run, overriding `$FATHOM_EDITOR`, `$VISUAL` and `$EDITOR`.
    ///
    /// The whole spec is used, arguments included, so `--editor "nvim -p"` splits a tab open across your
    /// command line and this flag.
    #[arg(long, value_name = "PROGRAM [ARGS…]")]
    pub editor: Option<String>,
    /// Print the command and the reason for it, and run nothing.
    ///
    /// The door that works with no editor installed, which is what makes "no `$EDITOR`" an explainable
    /// state rather than a broken one.
    #[arg(long)]
    pub print: bool,
    /// List every file the episode touched with the line each would jump to, and open nothing.
    #[arg(long)]
    pub list: bool,
    /// Print the decision as JSON on stdout, and nothing else there.
    #[arg(long)]
    pub json: bool,
}

/// `fathom difftool <EPISODE>` — the heavyweight diff, in the tool you already like (`e5.8`).
///
/// Fathom's own diff pane is the one that needs nothing installed. This is for the times a reviewer wants
/// the other kind: the two sides of one file written out and handed to difftastic, meld, kdiff3,
/// `vim -d` or `code --diff`. The tool comes from `--tool`, `$FATHOM_DIFFTOOL` or `[diff] difftool` in
/// your own config — never from a clone's `fathom.toml`, which is told as much when it tries.
#[derive(Args, Debug)]
pub struct DifftoolArgs {
    /// The episode whose file to show: the same handle grammar as every other verb. With no argument,
    /// `HEAD`.
    pub episode: Option<String>,
    /// The repository to read.
    #[arg(long, default_value = ".")]
    pub root: String,
    /// Which of the episode's files to show, by path or by a unique suffix of it.
    #[arg(long, value_name = "PATH")]
    pub path: Option<String>,
    /// Which difftool to use, overriding `$FATHOM_DIFFTOOL` and your user config.
    ///
    /// A name from the table (`difftastic`, `meld`, `kdiff3`, `nvim`, `vim`, `vscode`, …) or any program
    /// on `PATH`, which gets the two files as its last two arguments.
    #[arg(long, value_name = "NAME")]
    pub tool: Option<String>,
    /// Print the command and the reason for it, and run nothing. The two files stay on disk.
    #[arg(long)]
    pub print: bool,
    /// List every file the episode touched with the line each would jump to, and show nothing.
    #[arg(long)]
    pub list: bool,
    /// Leave the two staged files behind and say where they are.
    #[arg(long)]
    pub keep: bool,
    /// Print the decision as JSON on stdout, and nothing else there.
    #[arg(long)]
    pub json: bool,
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
        Some(Verb::Episodes(args)) => crate::episodes::run(args),
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
        Some(Verb::Narrative(args)) => crate::narrative::narrate(
            &args.root,
            args.episode.as_deref(),
            args.json,
            args.markdown,
            args.explain,
            args.top,
            args.attribution,
        ),
        Some(Verb::Drift(args)) => crate::drift::drift(
            &args.root,
            args.session.as_deref(),
            args.all,
            args.window,
            args.min_overlap,
            args.max,
            &args.dismiss,
            &args.off,
            args.json,
            args.fail_on_findings,
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
        Some(Verb::Check(args)) => crate::check::run(args),
        Some(Verb::Doctor(args)) => {
            diagnose::doctor(&args.root, args.privacy, args.json, args.deep, args.strict)
        }
        Some(Verb::Export(args)) => crate::export::run(args),
        Some(Verb::Mcp(args)) => mcp(args),
        Some(Verb::Watch(args)) => watch(args),
        Some(Verb::Ambient(args)) => ambient(args, globals),
        Some(Verb::Hooks(args)) => crate::harness::run(args),
        Some(Verb::Daemon(args)) => daemon(args),
        Some(Verb::Status(args)) => status(args),
        Some(Verb::Tui(args)) => tui(args, globals),
        Some(Verb::Open(args)) => crate::open::open(
            &args.root,
            args.episode.as_deref(),
            args.path.as_deref(),
            args.line,
            args.editor.as_deref(),
            args.print,
            args.list,
            args.json,
        ),
        Some(Verb::Difftool(args)) => crate::open::difftool(
            &args.root,
            args.episode.as_deref(),
            args.path.as_deref(),
            args.tool.as_deref(),
            args.print,
            args.list,
            args.keep,
            args.json,
        ),
        Some(Verb::Cmp(args)) => crate::compare::run(args),
        Some(Verb::Search(args)) => crate::search::run(args),
        Some(Verb::Architecture(args)) => crate::scorecard::run(args),
        Some(Verb::Ratchet(args)) => crate::ratchet::run(args),
        Some(Verb::Context(args)) => crate::notes::context(args),
        Some(Verb::Note(args)) => crate::notes::note(args),
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
    // `--json` and `--stats` both ask for the run's numbers as data, and mean the same thing once no
    // `--show-session` has claimed `--json` for the session rows (`reco-08r`).
    let numbers = if args.json || args.stats {
        crate::tui::Numbers::Json
    } else {
        crate::tui::Numbers::Line
    };
    crate::tui::dispatch(globals, numbers)
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
        // No verb means no `--stats` either: the flag belongs to `fathom tui`, and running with no verb
        // gets the ordinary one line on the way out (`reco-08r`).
        crate::tui::dispatch(globals, crate::tui::Numbers::Line)
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
    // One process per repository does the watching and the indexing (`e6.3`, ADR-004 §7.1): when a daemon is
    // serving this repository, this command asks it for the pass and reports what that pass did, rather than
    // a second process writing a second copy of the same work for no reader's benefit. Four flags are
    // exempt, and each exemption is a different reason: `--rebuild` advances the analysis epoch, which is an
    // explicit act rather than a catch-up; `--refresh` is the pass that *measures* the cache and has to be
    // this process's own work to mean anything; `--stats` asks about this process's read of the cache table;
    // `--watch` is asking to become the watcher itself. `--prune` is exempt on `--rebuild`'s reason: deleting
    // rows is an act on this store, and it has to happen after the ref snapshot this process is about to
    // write, not after somebody else's idea of when it ran.
    #[cfg(feature = "watch")]
    if !args.rebuild
        && !args.refresh
        && !args.stats
        && !args.watch
        && !args.prune
        && let Some(reply) = crate::daemon::ask_for_pass(root, "fathom index")
    {
        return index_by_daemon(args, &reply);
    }
    let store_path = index::default_store_path(root);
    // Containment first, before anything is created: a clone whose `.fathom` is a symlink must be refused
    // rather than written through (`e7.8`, threat model F-1).
    crate::sandbox::ensure_state_dir(root)?;
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
    // The memory ceilings, resolved once for the run and handed downhill as values (`reco-93w`): peak
    // resident memory is supposed to be a function of these numbers, so there is exactly one place they
    // are read and no crate below here gets to have an opinion about them.
    let ceilings = crate::memory::resolve(root);
    store.set_ceilings(&ceilings.ceilings);
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
        // The ceilings, in the one place the spine can use them: the queue depth between normalize and
        // write, and the cap on one decode buffer.
        ingest: fathom_ingest::ingest::IngestOptions {
            walk: fathom_ingest::graph::WalkOptions::head(),
            ceilings: ceilings.ceilings.clone(),
            ..fathom_ingest::ingest::IngestOptions::default()
        },
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
        // The module graph (`e3.11`, `reco-dlp`): on unless this run or this repository said not to, and
        // reading its knobs out of the repository's own `fathom.toml` — the CLI is the crate ADR-002 §4.5
        // lets open that file, and everything downstream gets the text as a value.
        graph: graph_pass(root, args.no_graph, &|line: String| {
            output::problem(format_args!("config: {line}"))
        }),
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
    for notice in &ceilings.notices {
        output::problem(format_args!("config: {notice}"));
    }
    // ADR-004 §8.6's cache budget, enforced after the run that grew the cache. The order is the point:
    // evicting before an index would throw away the rows the run is about to need, and evicting during
    // one would make a lookup's cost depend on what else is in the table. When the store is inside its
    // budget this is two reads and no writes.
    let eviction = store
        .cache_enforce(
            &fathom_store::ceiling::CacheCeiling {
                budget_bytes: ceilings.ceilings.durable_cache_bytes,
                protect_recent_ms: ceilings.ceilings.protect_recent_ms,
                dry_run: false,
            },
            now,
        )
        .map_err(failed)?;
    if args.rebuild {
        // The other half of §9.3's state machine: the epoch this run filled is the one readers may now see.
        store.settle_epoch().map_err(failed)?;
    }
    // A cache that had to be cut back is said, on every output path: a store that got smaller while you
    // were watching something else is exactly the kind of fact a report owes you.
    if eviction.over_budget() {
        output::problem(format_args!("cache: {}", eviction.sentence()));
    }
    let epoch = store.epoch().map_err(failed)?;

    // `--prune` runs *after* the index, and that order is the safety of it: the dead set is defined by the
    // refs as they are now, and the run above is what just wrote them (ADR-004 §8.1 — the pruner is the only
    // thing that deletes, and it deletes only what the store can prove is dead).
    let prune = if args.prune {
        Some(
            store
                .prune_dead_commits(args.dry_run, now)
                .map_err(failed)?,
        )
    } else {
        None
    };

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
                &stream,
                prune.as_ref(),
            )
        ));
    } else if args.stats {
        print_stats(&store_path, &report);
        if let Some(prune) = &prune {
            for line in prune_lines(prune) {
                output::line(format_args!("{line}"));
            }
        }
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
        if report.quiet {
            // The no-op said by itself (`reco-7tc`). A run that took 3 ms instead of 55 does not look like a
            // run that walked the history, and a line that read "0 commit(s) walked" in a repository with 500
            // of them would be a sentence about the report rather than about the repository.
            say(format_args!(
                "{}: nothing new — HEAD {} is indexed and no ref has moved, {} ms",
                store_path.display(),
                short_oid(
                    &repository
                        .head_oid()
                        .ok()
                        .flatten()
                        .map_or_else(String::new, |oid| oid.hex()),
                ),
                report.elapsed.as_millis()
            ));
        } else {
            say(format_args!(
                "{}: {}{} commit(s) walked, {} new, {} analyse unit(s) computed in {} ms",
                store_path.display(),
                if args.rebuild { "rebuild: " } else { "" },
                report.commits_walked,
                added,
                report.computed(),
                report.elapsed.as_millis()
            ));
        }
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
        // The module graph (`e3.11`): the line that says whether the architecture surfaces have anything
        // to read, and what it cost.
        graph_lines(&report.graph, &say);
        if let Some(prune) = &prune {
            for line in prune_lines(prune) {
                say(format_args!("{line}"));
            }
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

/// The module-graph pass an index run should do (`e3.11`, `reco-dlp`).
///
/// Three things can stop it, and each says which it was, because "there is no graph" is a state a reviewer
/// has to be able to tell the difference between: a flag on this run, a knob in the repository's own
/// `fathom.toml`, or nothing at all — the ordinary case, where it simply runs.
///
/// Gated with the lane it writes: without a store build there is no `index` module to hand a pass to.
#[cfg(feature = "sqlite-bundled")]
pub(crate) fn graph_pass(
    root: &std::path::Path,
    no_graph: bool,
    mut notice: impl FnMut(String),
) -> fathom_analyze::index::GraphPass {
    if no_graph {
        notice(
            "graph: not this run (--no-graph); the architecture surfaces will say the graph is missing"
                .to_string(),
        );
        return fathom_analyze::index::GraphPass::Off;
    }
    let text = std::fs::read_to_string(root.join("fathom.toml")).unwrap_or_default();
    let (policy, notices) = fathom_analyze::graph::GraphPolicy::from_repo_config_text(&text);
    for line in notices {
        notice(line);
    }
    if let Some(why) = policy.disabled_reason() {
        notice(format!("graph: off — {why}"));
        return fathom_analyze::index::GraphPass::Off;
    }
    fathom_analyze::index::GraphPass::on(text)
}

/// The module-graph lines of a run report: what it wrote, and the notices it brought back.
///
/// Printed only when there is something to print. A run that wrote no graph rows because the store already
/// had every revision is not a run that failed to write them, and a line saying "module graph: 0
/// revisions" in every ordinary warm index would teach people to stop reading it.
#[cfg(feature = "sqlite-bundled")]
fn graph_lines(
    graph: &fathom_analyze::graph::revise::Report,
    mut say: impl FnMut(std::fmt::Arguments),
) {
    // Printed whenever there is a sentence: a pass that wrote nothing *because it refused* has the most
    // important thing to say in the whole report.
    let sentence = graph.sentence();
    if !sentence.is_empty() {
        say(format_args!("  {sentence}"));
    }
    for notice in &graph.notices {
        say(format_args!("  graph: {notice}"));
    }
}

/// The seven-character prefix every verb here prints a commit id as, spelled once.
#[cfg(feature = "sqlite-bundled")]
fn short_oid(oid: &str) -> String {
    oid.chars().take(7).collect()
}

/// The `--prune` result, in the lines the plain report has room for.
///
/// Three cases, and the third is the one a report is tempted to print as the second: something went, nothing
/// needed going, and nothing *could* go because the store cannot tell a dead row from a live one.
#[cfg(feature = "sqlite-bundled")]
fn prune_lines(prune: &fathom_store::prune::PruneReport) -> Vec<String> {
    if let Some(why) = &prune.refusal {
        return vec![format!(
            "  prune     nothing removed: the store cannot tell dead rows from live ones ({why})"
        )];
    }
    if prune.commits_removed == 0 {
        return vec![
            "  prune     nothing to remove: every commit row in the index is reachable from a ref"
                .to_owned(),
        ];
    }
    let mut out = vec![format!(
        "  prune     {} {} dead commit row(s), {} file row(s){}",
        if prune.dry_run {
            "would remove"
        } else {
            "removed"
        },
        prune.commits_removed,
        prune.files_removed,
        if prune.episodes_removed > 0 {
            format!(
                ", {} episode(s) with no members left",
                prune.episodes_removed
            )
        } else {
            String::new()
        },
    )];
    // The guard, said out loud rather than as a smaller number: "it only took 1 of the 2" is a fact about
    // the reviewer's decisions, and silence about it reads as a bug in the prune (ADR-004 §8.2).
    if prune.withheld_reviewed > 0 {
        out.push(format!(
            "  prune     {} dead commit row(s) left in place: a review or a sign-off points at them",
            prune.withheld_reviewed
        ));
    }
    if prune.dry_run {
        out.push("  prune     nothing was removed: this was --dry-run".to_owned());
    }
    out
}

/// The `--json` shape, assembled here rather than derived from [`index::Report`]'s own serde so that the
/// key names are the CLI's promise and not a struct's internals: a field renamed in `fathom-analyze` must
/// not silently rename `fathom index --json`'s contract.
#[allow(clippy::too_many_arguments)]
// eight arguments, and every one is a different part of the store the
// report reads; a struct would only hide that
#[cfg(feature = "sqlite-bundled")]
fn json_report(
    root: &std::path::Path,
    store: &fathom_store::spine::Store,
    store_path: &std::path::Path,
    report: &fathom_analyze::index::Report,
    rebuild: bool,
    epoch: i64,
    stream: &crate::harness::Stream,
    prune: Option<&fathom_store::prune::PruneReport>,
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
        // The run compared the refs and skipped the walk, the analyses and the episode rebuild (`reco-7tc`).
        // Printed every run rather than only on a quiet one, so that `commits.walked: 0` can be read as "a
        // quiet run" or as "an empty repository" without having to guess which.
        "nothing_new": report.quiet,
        // Where the wall clock went, in whole milliseconds, in run order. Printed every run: a report
        // that says only "45 s" cannot be argued with, and one that says "spine 3 ms, analyses 44 s"
        // can be (`reco-66q`). The names are the phases of `fathom_analyze::index::run`.
        "phases_ms": serde_json::Value::Object(
            report
                .phases
                .iter()
                .map(|(phase, ms)| (phase.clone(), json!(ms)))
                .collect::<serde_json::Map<_, _>>(),
        ),
        // The module graph pass (`e3.11`, `reco-dlp`): what it wrote, what it refused, and why it stopped.
        // `null` when the pass was off (`--no-graph`, `graph.enabled = false`) — "not asked" is not the same
        // statement as "asked, and the store already held every revision", and a CI consumer has to be able
        // to tell them apart before it can gate on either.
        "graph": report.graph.json(),
        "commits": {
            "walked": report.commits_walked,
            "new": report.commits_new,
            "looked_up": report.commits_considered,
            "written": report.spine.commits_written,
        },
        "interrupted": report.interrupted,
        // What `--prune` took, or would have taken. Absent (`null`) when the flag was not passed, so a
        // consumer can tell "nothing was removed" apart from "nothing was asked".
        "prune": prune.map(|prune| {
            json!({
                "dry_run": prune.dry_run,
                "commits_removed": prune.commits_removed,
                "files_removed": prune.files_removed,
                "episodes_removed": prune.episodes_removed,
                "withheld_reviewed": prune.withheld_reviewed,
                "withheld_undecidable": prune.withheld_undecidable,
                "refusal": prune.refusal,
                "sample": prune.sample,
            })
        }),
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
        "{} — {} run, {}{} commit(s) walked, {} looked up, {} ms",
        store.display(),
        report.kind.as_str(),
        if report.quiet { "nothing new: " } else { "" },
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
    // The spine's back-pressure line (`reco-93w`): how much was ever in flight, and how often the
    // normalizer was told to wait. `queue_blocks == 0` on a long run means the ceiling never came into
    // play, which is a different statement from "the ceiling is not being enforced".
    output::line(format_args!(
        "  in flight   {} batch(es) at the peak ({} KiB), {} wait(s) at the hand-off",
        report.spine.queue_high_water_batches,
        report.spine.queue_high_water_bytes / 1024,
        report.spine.queue_blocks,
    ));
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

/// `fathom index`, deferred to a daemon (`e6.3`).
///
/// The report is a different document from `fathom.cli.index/1` on purpose: the fields a run report carries
/// — this process's own timings, the cache table, the segmentation — belong to a run this process did not
/// perform, and inventing them for a pass somebody else ran would be the kind of near-miss value this
/// repository treats as a bug. The daemon's own words are carried verbatim and the contract name says so.
#[cfg(feature = "watch")]
fn index_by_daemon(args: &IndexArgs, reply: &serde_json::Value) -> eyre::Result<ExitCode> {
    if args.json {
        output::line(format_args!(
            "{}",
            serde_json::json!({
                "contract": "fathom.cli.index.deferred/1",
                "delegated": true,
                "daemon": reply,
            })
        ));
    } else if !args.quiet {
        output::line(format_args!(
            "index: {}",
            crate::daemon::describe_pass(reply)
        ));
    }
    Ok(ExitCode::Ok)
}

/// `fathom daemon` and `fathom status`, when the watcher is compiled in (`e6.3`).
///
/// The gate is the `watch` feature rather than a runtime check for the same reason `fathom watch`'s is: the
/// daemon *is* a watcher with an endpoint, and a binary built without `watch` does not contain the change
/// source at all. `fathom status` is gated with it because the honest report of a build that cannot watch is
/// "this binary cannot answer that" (ADR-002 §6.1, §7.3).
#[cfg(feature = "watch")]
fn daemon(args: &DaemonArgs) -> eyre::Result<ExitCode> {
    match args.action.as_ref() {
        Some(DaemonAction::Status(status)) => crate::daemon::status(status),
        // Bare `fathom daemon` is a status report. The repository it reports on is the one underfoot,
        // because the subverb-less form has nowhere to carry a `--root`, and inventing one there and
        // only there would be a flag whose position changes its meaning.
        None => crate::daemon::status(&StatusArgs {
            root: ".".to_owned(),
            json: false,
            socket: None,
            index: false,
        }),
        Some(DaemonAction::Start(start)) => crate::daemon::start(start),
        Some(DaemonAction::Stop(stop)) => crate::daemon::stop(stop),
    }
}

/// `fathom daemon` without the watcher: the exit code that means "rebuild differently" (§6.1).
#[cfg(not(feature = "watch"))]
fn daemon(_args: &DaemonArgs) -> eyre::Result<ExitCode> {
    Err(feature_absent("fathom daemon", "watch"))
}

/// `fathom status` (`e6.3`).
#[cfg(feature = "watch")]
fn status(args: &StatusArgs) -> eyre::Result<ExitCode> {
    crate::daemon::status(args)
}

/// The same, in a binary that cannot watch.
#[cfg(not(feature = "watch"))]
fn status(_args: &StatusArgs) -> eyre::Result<ExitCode> {
    Err(feature_absent("fathom status", "watch"))
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

/// The same verb in a binary that was built without the terminal shell: the glance pane is the shell's own
/// view, and a `minimal` binary says which feature it is missing instead of drawing nothing (ADR-002 §6.1).
#[cfg(not(feature = "tui"))]
fn ambient(_args: &AmbientArgs, _globals: &Globals) -> eyre::Result<ExitCode> {
    Err(feature_absent("fathom ambient", "tui"))
}

#[cfg(feature = "tui")]
fn ambient(args: &AmbientArgs, globals: &Globals) -> eyre::Result<ExitCode> {
    crate::ambient::run(args, globals)
}

fn mcp(args: &McpArgs) -> eyre::Result<ExitCode> {
    if !cfg!(feature = "mcp") {
        return Err(feature_absent("fathom mcp serve", "mcp"));
    }
    let root = match &args.action {
        McpAction::Serve => std::path::Path::new("."),
        McpAction::Tools { root, .. } | McpAction::ApprovalToken { root, .. } => {
            std::path::Path::new(root)
        }
    };
    crate::mcp::run(args, root)
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

/// The last shell run's frame numbers, as the lines `fathom doctor` prints (`reco-08r`).
///
/// Delegated the way the watcher's heartbeat is, because the module that writes the record is gated with
/// the shell: a binary built without it has no frame to have measured, and says that rather than printing
/// the silence.
pub(crate) fn frame_lines(root: &std::path::Path) -> Vec<String> {
    #[cfg(feature = "tui")]
    {
        crate::framereport::doctor_lines(root)
    }
    #[cfg(not(feature = "tui"))]
    {
        let _ = root;
        vec![
            "frames: not compiled into this build; rebuild with `--features tui` to get `fathom tui`, \
             which is what writes the record"
                .to_owned(),
        ]
    }
}

/// The same, as data.
pub(crate) fn frame_json(root: &std::path::Path) -> serde_json::Value {
    #[cfg(feature = "tui")]
    {
        crate::framereport::doctor_json(root)
    }
    #[cfg(not(feature = "tui"))]
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
