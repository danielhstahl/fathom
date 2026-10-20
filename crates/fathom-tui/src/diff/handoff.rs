//! `fathom_tui::diff::handoff` — leaving the terminal on purpose: copy, and the plans that say what to run.
//!
//! Fathom is a reader, not an editor (ADR-001 §8 non-goal 2), so the exits have to be good. The *plans*
//! — which editor, which difftool, what the argument shape is, and what was decided — live one crate
//! below this in [`fathom_core::handoff`], because `fathom open`, `fathom difftool` and the shell all
//! have to reach the same answer (`e5.8`, `reco-dmw`); they are re-exported here so the diff pane's
//! `y`, `Y` and `E` keys keep the path they were written against.
//!
//! What is left in this module is the half that only a thing attached to a terminal can decide:
//!
//! * **copy path** and **copy hunk** pick a clipboard program for the OS in use, from a list whose
//!   order is a policy: an explicit `FATHOM_CLIPBOARD` wins, then the platform's own tool, then the
//!   ones that exist only when they are actually on `PATH`. With none of them, the text is put on the
//!   terminal with an OSC-52 escape, which is the one clipboard that works over `ssh` — and the toast
//!   says which of these happened, because "copied" that did not copy is worse than a refusal.
//! * [`Handoff`] is the door the loop runs the decision through, because only the loop owns the screen:
//!   raw mode and the alternate screen are taken, and a child process started from inside them inherits
//!   a terminal it cannot use.

// The plans every handoff is written from, in one place so the shell and the verbs cannot disagree
// about which editor takes which argument (`e5.8`).
pub use fathom_core::handoff::{
    Command, DIFFTOOLS, Difftool, EDITOR_KEYS, Editor, Env, Os, difftool_plan, editor_plan,
};

/// What a copy decided, and how to say so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Copy {
    /// Pipe the text to a clipboard program.
    Program(Command),
    /// No program was available: write an OSC-52 escape to the terminal instead.
    Osc52 {
        /// The full escape sequence, ready to write.
        sequence: String,
    },
    /// Neither, with the reason.
    Unavailable {
        /// What was tried, in the tool's own words.
        why: String,
    },
}

impl Copy {
    /// Turn the plan into the one thing the loop has to do outside the frame, plus the sentence to
    /// print while it happens. Never a bare "copied": the sentence names the mechanism, because that
    /// is the part a reviewer can act on when it did not work.
    #[must_use]
    pub fn deliver(&self, text: &str) -> (Handoff, String) {
        let count = text.lines().count();
        match self {
            Copy::Program(command) => (
                Handoff::Run(command.clone()),
                format!(
                    "copying {} line(s) — {} B · {}",
                    count,
                    text.len(),
                    command.why
                ),
            ),
            Copy::Osc52 { sequence } => (
                Handoff::Write(sequence.clone()),
                format!(
                    "sending {} line(s) to the terminal clipboard (OSC 52) — if nothing lands, set \
                     FATHOM_CLIPBOARD",
                    count
                ),
            ),
            Copy::Unavailable { why } => {
                (Handoff::Refused, format!("nothing to copy with — {why}"))
            }
        }
    }
}

/// Work the loop does outside a frame: run one program with the terminal handed back, or write bytes
/// straight to the terminal. Both belong to the loop because only the loop owns the screen — raw mode
/// and the alternate screen are taken, and a child process started from inside them inherits a terminal
/// it cannot use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Handoff {
    /// Hand the terminal back and run this.
    Run(Command),
    /// Write these bytes to the terminal as it stands (OSC 52: the one clipboard over `ssh`).
    Write(String),
    /// The plan said no, and the toast is the whole answer.
    Refused,
}

/// Decide how to copy `text`. Nothing is run here.
#[must_use]
pub fn copy_plan(os: Os, text: &str, env: &Env<'_>) -> Copy {
    if let Some(spec) = env.var("FATHOM_CLIPBOARD") {
        let mut words = spec.split_whitespace();
        if let Some(program) = words.next() {
            if env.runnable(program) {
                return Copy::Program(Command {
                    program: program.to_owned(),
                    args: words.map(str::to_owned).collect(),
                    stdin: Some(text.to_owned()),
                    why: format!("`FATHOM_CLIPBOARD={spec}`"),
                });
            }
            return Copy::Unavailable {
                why: format!("`FATHOM_CLIPBOARD={spec}` is not runnable"),
            };
        }
    }
    let wayland = env.var("WAYLAND_DISPLAY").is_some();
    let x11 = env.var("DISPLAY").is_some();
    for candidate in os.clipboard_candidates() {
        if !matches!(os, Os::MacOs | Os::Windows) {
            // On the X/Wayland split, a tool is only worth trying if its session type is the one the
            // environment says we are in; trying `xclip` without a display just prints a usage error.
            if *candidate == "wl-copy" && !wayland {
                continue;
            }
            if matches!(*candidate, "xclip" | "xsel") && !x11 {
                continue;
            }
        }
        if !env.runnable(candidate) {
            continue;
        }
        let (args, why) = match *candidate {
            "xclip" => (
                vec!["-selection".to_owned(), "clipboard".to_owned()],
                "xclip -selection clipboard".to_owned(),
            ),
            "xsel" => (
                vec!["--clipboard".to_owned(), "--input".to_owned()],
                "xsel --clipboard --input".to_owned(),
            ),
            "powershell" => (
                vec![
                    "-NoProfile".to_owned(),
                    "-NonInteractive".to_owned(),
                    "-Command".to_owned(),
                    "Set-Clipboard -NoNewline ([Console]::In.ReadToEnd())".to_owned(),
                ],
                "powershell Set-Clipboard".to_owned(),
            ),
            other => (Vec::new(), other.to_owned()),
        };
        return Copy::Program(Command {
            program: (*candidate).to_owned(),
            args,
            stdin: Some(text.to_owned()),
            why: format!("via {why} on {}", os.name()),
        });
    }
    Copy::Osc52 {
        sequence: osc52(text),
    }
}

/// The OSC-52 escape: `ESC ] 52 ; c ; BASE64 BEL`.
#[must_use]
pub fn osc52(text: &str) -> String {
    format!("\u{1b}]52;c;{}\u{7}", base64(text.as_bytes()))
}

/// Minimal standard-alphabet base64, because OSC-52 needs it and a dependency for eight lines of
/// table would be a worse trade than the table.
#[must_use]
pub fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(TABLE[usize::try_from(n >> 18).unwrap_or(0) & 63] as char);
        out.push(TABLE[usize::try_from(n >> 12).unwrap_or(0) & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[usize::try_from(n >> 6).unwrap_or(0) & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[usize::try_from(n & 63).unwrap_or(0)] as char
        } else {
            '='
        });
    }
    out
}
