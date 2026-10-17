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
mod clock;
mod diagnose;
// The episodes the shell opens with (`reco-wzt`): read from the store here, because the shell may not
// open one (ADR-002 §5.2), and handed over as a batch the rail draws. Only the shell has a use for them,
// which is the only reason the module is gated with the shell.
#[cfg(feature = "tui")]
mod episodes;
mod features;
mod hooks;
mod output;
// The diff pane's byte source (`reco-0kw`): the episode's patch, materialised from the store and
// the object store here, because the shell may not do either (ADR-002 §5.2). Only the shell has a
// use for it, so it is gated with the shell.
#[cfg(feature = "tui")]
mod patches;
mod query;
mod sanitize;
// The episode boundary knobs are configuration, and configuration is read here and nowhere else below the
// CLI (ADR-002 §4.5). The module is unconditional because `fathom index` needs it in every build that can
// index at all.
mod segment;
mod state;
// The terminal shell is the one verb whose crate is optional (ADR-002 §6.2): a `minimal` binary says
// "compiled out" rather than failing to start, so the module is gated with the feature it needs.
// The handoff environment is read for the shell and for nothing else, so it is gated exactly as the
// shell is: a `minimal` binary has no terminal to hand back and no plan to read variables for.
#[cfg(feature = "tui")]
mod handoff;
// The reviewer's session lives in the store, and only the shell has a use for it, so it is gated the
// same way the shell is: a `minimal` binary has no session to resume (`e1.11`).
#[cfg(feature = "tui")]
mod session;
#[cfg(feature = "tui")]
mod tui;
mod verbs;
#[cfg(feature = "watch")]
mod watch;

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
        plain: cli.plain,
        no_alt_screen: cli.no_alt_screen,
        fresh: cli.fresh,
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
