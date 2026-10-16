//! The environment the shell's handoffs are planned against (ADR-002 §3, ADR-006 §7).
//!
//! `fathom-tui` is IO-free by rule: it reads no environment, opens no files and spawns nothing, so the
//! two facts its copy and `$EDITOR` plans need — what the variables say, and what is on `PATH` — are
//! read *here* and handed over as a [`Env`] of closures. This is the same arrangement as the keymap
//! overrides and the repository label: the shell decides, this crate notices.
//!
//! Nothing runs from this module. `y`, `Y` and `E` produce a plan; running it belongs to the frame sink
//! that owns the screen, and that door is `e5.8`'s (`reco-dmw`).

use std::path::Path;

use fathom_tui::diff::Env;

/// The calling process's environment, as the diff view's plans want it.
#[must_use]
pub fn environment() -> Env<'static> {
    Env {
        lookup: std::sync::Arc::new(|key| std::env::var(key).ok()),
        resolve: std::sync::Arc::new(on_path),
    }
}

/// The `which` of it: is `program` an executable this process could run?
///
/// Windows needs the extension dance because `code` there is `code.cmd`, and a plan that reports
/// "`code` is not runnable" while `code.cmd` sits two directories away is a plan that blames the user's
/// editor for the shell's blindness.
#[must_use]
fn on_path(program: &str) -> bool {
    if program.contains('/') || program.contains('\\') {
        return Path::new(program).is_file();
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    let extensions: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ".bat", ""]
    } else {
        &[""]
    };
    std::env::split_paths(&path).any(|dir| {
        extensions
            .iter()
            .any(|extension| dir.join(format!("{program}{extension}")).is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_environment_answers_for_the_machine_it_was_read_on() {
        let env = environment();
        // `PATH` exists on every platform fathom runs on, and a program nobody installed is not
        // runnable. Both halves in one assertion: a lookup that always said "yes" would pass the first
        // and fail the second.
        assert!(
            env.var("PATH").is_some(),
            "PATH is what the clipboard and editor plans are searched against"
        );
        assert!(
            !env.runnable("fathom-definitely-not-a-real-program"),
            "a name that is not on PATH must not plan as if it were"
        );
    }
}
