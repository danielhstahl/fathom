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

    // `e1.5` is the first verb that takes the screen, and the guard it installs will do the real work.
    // Until then this is a no-op with a name, so that the ordering below is already correct: restore,
    // then report.
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
    // No screen is owned by any verb yet; `e1.5` (reco-v6t) fills this in with the alternate-buffer and
    // raw-mode restoration its own `Drop` guard shares.
}

/// The code a panic leaves the process with, spelled once so the hook and `main` cannot disagree.
pub const PANIC_EXIT: ExitCode = ExitCode::Error;
