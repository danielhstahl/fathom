//! `fathom` — the composition root (ADR-002 §3, §4.3, §7.1, §7.2, §7.3).
//!
//! Three things happen here and nothing else: install the hooks that own the terminal and the error
//! report, parse the command line, and turn whatever a verb returned into an exit code. No analysis, no
//! SQL, no rendering — if a verb needs logic, that logic is a downhill crate's function (§4.3), and this
//! file's smallness is what keeps that true instead of merely intended.
//!
//! This crate is also the only one with default features (R5), the only one allowed `eyre` (W5), and the
//! only one allowed to call the process exit — which is why `fathom_core::exit` has no `exit()` on it.

#![forbid(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

// The clock is read once, here, and only by the verbs that need an instant (ADR-002 §4.4). Every verb
// that *reports* the world needs the instant the report describes, not just the verbs that stamp rows,
// so the module is unconditional and `rg SystemTime` still lands here and nowhere else.
mod analyze;
// The glance pane for the second terminal (`reco-8bx`). It is gated with the shell because it draws the
// shell's own view, and its `<Enter>` hands the terminal to the shell; a `minimal` binary names the feature
// it is missing instead (ADR-002 §6.1).
#[cfg(feature = "tui")]
mod ambient;
// The comparison of two endpoints (`e5.3`, `reco-ifo`): the endpoint resolution, the span between them and
// the saved-comparison rows. The composition itself is `fathom_analyze::compare`; this is the half that
// needs a store and a git read (ADR-002 §4.3).
mod compare;
// Who wrote it, and how to tell Fathom it got it wrong (`e4.1`): the write path for a correction, which
// needs the store, so it lives above the classifier and below the verb table (ADR-002 §4.3).
mod attribution;
mod clock;
mod demo;
mod diagnose;
// Plan versus execution (`e4.8`): the store reads that turn an index into the sessions
// `fathom_analyze::drift` compares. The rules live there, so that every finding has a fixture; this module
// owns only what a terminal sees.
mod drift;
// The episodes the shell opens with (`reco-wzt`): read from the store here, because the shell may not
// open one (ADR-002 §5.2), and handed over as a batch the rail draws. Only the shell has a use for them,
// which is the only reason the module is gated with the shell.
#[cfg(feature = "tui")]
mod episodes;
mod features;
// The producer that feeds an open frame (`reco-bpc`): the cheap probe, the index slice and the re-read that
// make a shell notice a commit made in another terminal. It reads git and the store, which the shell may not
// (ADR-002 §5.2), and hands over rows — so it lives here, beside the read it shares with `fathom ambient`.
#[cfg(feature = "tui")]
mod feed;
// The review artefact (`e5.7`): the range, the rows, the file, the exit code. The shape and the renderings
// live in `fathom_analyze::export`; this is the half that needs a store, a git read and a ledger.
mod export;
// The guided first index: the progress row `fathom index` prints while it works (`e5.9`). It reads
// stderr to decide whether to print at all, which is a composition-root job (ADR-002 §7.5).
mod guide;
mod harness;
mod hooks;
// The episode narrative (`e4.7`): the store read and the formatting choice for the prose that
// `fathom_analyze::narrative` composes. It is a separate verb from `analyze` for the same reason `why` is:
// a document and the thing you read it for are different jobs.
mod narrative;
mod output;
// The diff pane's byte source (`reco-0kw`): the episode's patch, materialised from the store and
// the object store here, because the shell may not do either (ADR-002 §5.2). Only the shell has a
// use for it, so it is gated with the shell.
#[cfg(feature = "tui")]
mod patches;
mod query;
mod queue;
mod review;
// Which paths a run is allowed to write: the containment rule that a clone's symlink may not point a
// store, a cache or a ledger outside the repository (`e7.8`).
mod sandbox;
mod sanitize;
// The corpus the one search box reads (`reco-mkc`): the store and manifest reads the shell is not allowed
// to make for itself (ADR-002 §5.2), made here and handed over as an index — and the same corpus the
// headless `fathom search` scores, which is why the module is not gated with the shell.
mod search;
// The episode boundary knobs are configuration, and configuration is read here and nowhere else below the
// CLI (ADR-002 §4.5). The module is unconditional because `fathom index` needs it in every build that can
// index at all.
mod segment;
mod state;
// The terminal shell is the one verb whose crate is optional (ADR-002 §6.2): a `minimal` binary says
// "compiled out" rather than failing to start, so the module is gated with the feature it needs.
#[cfg(feature = "tui")]
mod tui;
// Who gets to name an editor, and how to run it (`e5.8`, `reco-dmw`). Not gated with the shell: an
// editor handoff needs a terminal to be *seen* in, not a terminal screen to be drawn on, and
// `fathom open` works in a `minimal` build for exactly that reason.
mod handoff;
// Paths and commits as OSC-8 links, where the terminal asked for them (`e5.8`).
mod links;
// Where the reviewer's own configuration lives (`e5.6`, `e5.8`): one list of directories, read by the
// theme resolver and the handoff settings alike.
mod userconfig;
// The two exits a reviewer leaves through: `$EDITOR` at a line, and a heavyweight difftool (`e5.8`).
mod open;
// The reviewer's session lives in the store, and only the shell has a use for it, so it is gated the
// same way the shell is: a `minimal` binary has no session to resume (`e1.11`).
#[cfg(feature = "tui")]
mod session;
// Which palette a run is painted with (`e5.6`). Gated with the shell because the only thing it resolves
// is the shell's theme — and because the printers read the same terminal facts the shell is handed.
#[cfg(feature = "tui")]
mod theme;
mod verbs;
// The watch daemon and `fathom status` (`e6.3`): the worker the daemon indexes with, the process
// management around `--foreground`, and the report. The loop itself is `fathom_ingest::daemon`; this
// is the half that needs the indexer, the clock and the store, which is why it is here (ADR-002 §4.3).
#[cfg(feature = "watch")]
mod daemon;
#[cfg(feature = "watch")]
mod watch;
mod why;

use clap::{CommandFactory, FromArgMatches};
use fathom_core::exit::ExitCode;

fn main() -> eyre::Result<()> {
    hooks::install()?;
    let code = run();
    hooks::restore_terminal();
    std::process::exit(code.as_i32());
}

/// Parse, dispatch, and produce the number to leave with.
fn run() -> ExitCode {
    // `long_version` is clap's "--version says more than -V" hook, which is exactly ADR-002 §6.4's
    // split: a parseable short version, and a long one that states the resolved feature set. The
    // `leak()` is one small string, once, at startup — clap stores it as a `&'static str`.
    let long_version: &'static str = features::version_line().leak();
    let command = verbs::Cli::command().long_version(long_version);
    // clap handles `--help` and `--version` itself and exits 0; a rejected command line exits 2, which is
    // `USAGE` in ADR-002 §7.3 — the contract is satisfied by not fighting it.
    let matches = command.get_matches();
    let cli = verbs::Cli::from_arg_matches(&matches).unwrap_or_else(|err| err.exit());
    let globals = verbs::Globals {
        progress: cli.progress,
        no_progress: cli.no_progress,
        demo: cli.demo,
        plain: cli.plain,
        no_alt_screen: cli.no_alt_screen,
        fresh: cli.fresh,
        mouse: cli.mouse,
        lines: cli.lines,
        theme: cli.theme,
        no_watch: cli.no_watch,
    };

    // The panic hook has restored the screen and printed the report by the time this returns; catching
    // here rather than exiting inside the hook is what lets every `Drop` guard run (ADR-002 §7.2, and the
    // reason `panic = "abort"` is refused).
    let dispatched = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        verbs::dispatch(cli.verb.as_ref(), &globals)
    }));
    match dispatched {
        Ok(Ok(code)) => code,
        Ok(Err(report)) => explain(&report),
        Err(_) => hooks::PANIC_EXIT,
    }
}

/// The one place an error becomes an exit code (ADR-002 §7.1).
fn explain(report: &eyre::Report) -> ExitCode {
    match report.downcast_ref::<verbs::Pending>() {
        Some(pending) => {
            output::problem(format_args!("error: {}", pending.message()));
            if let Some(hint) = pending.hint() {
                output::problem(format_args!("hint: {hint}"));
            }
            output::problem(format_args!("code: {}", pending.code()));
            pending.code()
        }
        None => {
            output::problem(format_args!("error: {report:#}"));
            ExitCode::Error
        }
    }
}
