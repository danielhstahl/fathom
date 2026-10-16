//! The hooks `main` installs before anything else runs (ADR-002 §7.1, §7.2).
//!
//! A panic in a process that owns the terminal loses the user's screen and the reason at the same time,
//! so the contract is: put the screen back, print the panic and the code the process will leave with,
//! and let unwinding run so every `Drop` guard gets its turn (which is why `panic = "abort"` is refused
//! in the profiles — a few hundred kilobytes is not a price worth paying for a corrupted terminal).

use eyre::Result;
use fathom_core::exit::ExitCode;

/// Install the report format and the panic hook.
///
/// The exit code a panic produces is [`ExitCode::Error`], and it is reached by unwinding: `main` catches
/// at its own boundary so a `Drop` guard that restores the screen still runs.
pub fn install() -> Result<()> {
    color_eyre::install()?;

    // The shell installs its own restoring hook when it takes the screen (`fathom_tui::terminal::
    // install_panic_hook`), chained ahead of this one, so a panic inside the UI restores first and
    // reports second. What stays here is the ordering: restore, then report — for every verb,
    // including the ones that never touch the terminal.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        previous(info);
    }));
    Ok(())
}

/// Put the terminal back the way it was found.
///
/// Called from the panic hook and from `main`'s exit path. Idempotent, allocation-free and error-free by
/// construction, because a cleanup path that can fail is a second failure on the way out.
pub fn restore_terminal() {
    // Nothing to do here, and that is a decision rather than a gap: the verb that takes the screen owns
    // putting it back, because a restore handle held two places is a restore that can be missed. The
    // shell (`e1.5`, `reco-v6t`) restores through a `Drop` guard on its own stack frame plus the chained
    // hook above, so a panic, a `<C-c>`, a `SIGTERM` and an ordinary exit all pass through the same
    // idempotent code — and this function is the seam that would call it if a verb ever owned a screen
    // without a stack frame to hang the guard on.
}

/// The code a panic leaves the process with, spelled once so the hook and `main` cannot disagree.
pub const PANIC_EXIT: ExitCode = ExitCode::Error;
