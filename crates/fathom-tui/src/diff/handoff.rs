//! `fathom_tui::diff::handoff` — leaving the terminal on purpose: copy, and open at a line in $EDITOR.
//!
//! Fathom is a reader, not an editor (ADR-001 §8 non-goal 2), so the exits have to be good. There are
//! three, and all of them are decided **before** anything is run, as a [`Command`] the caller can print,
//! test and log:
//!
//! * **copy path** and **copy hunk** pick a clipboard program for the OS in use, from a list whose
//!   order is a policy: an explicit `FATHOM_CLIPBOARD` wins, then the platform's own tool, then the
//!   ones that exist only when they are actually on `PATH`. With none of them, the text is put on the
//!   terminal with an OSC-52 escape, which is the one clipboard that works over `ssh` — and the toast
//!   says which of these happened, because "copied" that did not copy is worse than a refusal.
//! * **open in $EDITOR** follows git's own precedence (`VISUAL`, then `EDITOR`), splits it into a
//!   program plus its existing arguments, and then adds the line argument *that editor actually
//!   takes*: `+LINE` for the vi family, `-g file:LINE` for the VS Code family, `file:LINE` for zed,
//!   helix and sublime, `--line LINE` for the JetBrains CLIs, nothing for `notepad` — and it says which
//!   it chose and why, rather than inventing a flag and blaming the editor when it fails.
//!
//! The OS is a parameter, not a `cfg`, so the matrix is testable on the machine that runs the tests:
//! the same assertions cover macOS, Linux and Windows in one run instead of one each on three CI
//! runners that a contributor cannot run locally.

use std::fmt;

/// The operating systems a handoff has to work on. `Other` is a stated answer, not a crash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    /// Linux and the BSDs, which share the clipboard story.
    Linux,
    /// macOS: `pbcopy`, and `code` usually on `PATH`.
    MacOs,
    /// Windows: `clip`, and the editors that ship a `.cmd` shim.
    Windows,
    /// Anything else, which gets the same care and a notice.
    Other,
}

impl Os {
    /// The OS this build is running on. Compile-time, because `fathom-tui` reads no environment of
    /// its own (ADR-002 §3): the caller can always pass a different [`Os`] to the planners, which is
    /// what makes the three-OS matrix one test suite instead of three CI runners.
    #[must_use]
    pub const fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::MacOs
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else if cfg!(any(
            target_os = "linux",
            target_os = "android",
            target_os = "freebsd",
            target_os = "openbsd",
            target_os = "netbsd"
        )) {
            Os::Linux
        } else {
            Os::Other
        }
    }

    /// The name a toast prints.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Os::Linux => "linux",
            Os::MacOs => "macos",
            Os::Windows => "windows",
            Os::Other => "unknown",
        }
    }

    /// The clipboard programs this OS is expected to have, best first.
    #[must_use]
    pub const fn clipboard_candidates(self) -> &'static [&'static str] {
        match self {
            Os::MacOs => &["pbcopy"],
            Os::Windows => &["clip", "powershell"],
            Os::Linux | Os::Other => &["wl-copy", "xclip", "xsel"],
        }
    }

    /// The editor to fall back to when the environment names none.
    #[must_use]
    pub const fn default_editor(self) -> &'static str {
        match self {
            Os::Windows => "notepad",
            Os::Linux | Os::MacOs | Os::Other => "vi",
        }
    }
}

/// Something to run, decided in advance and printable.
///
/// Nothing in this crate runs it: `fathom-tui` is IO-free (ADR-002 §3), so a command that leaves the
/// terminal goes back to the loop as a [`crate::frame::FrameSink::run_external`] request, and the sink
/// that took the screen hands it back for the length of the child process. A sink that cannot says so,
/// and the reviewer reads that instead of watching a screen that pretended to copy something.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// The program to run.
    pub program: String,
    /// Its arguments, already in order.
    pub args: Vec<String>,
    /// Bytes to feed its stdin, when it reads one.
    pub stdin: Option<String>,
    /// Why this command and not another, in words — the toast and the test both read it.
    pub why: String,
}

impl Command {
    /// The one-line form a status line or a log prints.
    #[must_use]
    pub fn line(&self) -> String {
        let mut out = self.program.clone();
        for arg in &self.args {
            out.push(' ');
            if arg.contains(' ') || arg.is_empty() {
                out.push('"');
                out.push_str(arg);
                out.push('"');
            } else {
                out.push_str(arg);
            }
        }
        out
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.line())
    }
}

/// How an [`Env`] reads one environment variable. An `Arc` rather than a `Box` so an `Env` is `Clone`:
/// the host builds one at startup and hands a copy to every plan that needs it.
pub type Lookup<'a> = std::sync::Arc<dyn Fn(&str) -> Option<String> + 'a>;
/// How an [`Env`] decides whether a program can be run on this machine.
pub type Resolve<'a> = std::sync::Arc<dyn Fn(&str) -> bool + 'a>;

/// An environment: the caller's variable lookup and the caller's `PATH` lookup, both injectable so
/// the three-OS matrix is a unit test and not a request for three machines.
#[derive(Clone)]
pub struct Env<'a> {
    /// Look up one variable.
    pub lookup: Lookup<'a>,
    /// Is this program runnable here?
    pub resolve: Resolve<'a>,
}

impl std::fmt::Debug for Env<'_> {
    /// Closures do not print, so this says what the thing *is*, which is the only fact a log line or a
    /// failing assertion can use anyway.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Env")
            .field("env", &"variables and PATH answers supplied by the host")
            .finish()
    }
}

impl Env<'_> {
    /// An environment that knows nothing: no variables, nothing runnable. This is what a `DiffView`
    /// starts with, because reading the environment is not this crate's job (ADR-002 §3) — `fathom-cli`
    /// reads it and installs it with [`App::install_handoff_env`](crate::app::App::install_handoff_env).
    /// With nothing installed, a copy falls through to OSC 52 and the toast says so, which is a stated
    /// answer rather than a wrong one.
    #[must_use]
    pub fn empty() -> Env<'static> {
        Env {
            lookup: std::sync::Arc::new(|_key| None),
            resolve: std::sync::Arc::new(|_program| false),
        }
    }

    /// A fixed environment: `vars` as the variables, `on_path` as the resolvable programs.
    #[must_use]
    pub fn fixed(vars: &[(&str, &str)], on_path: &[&'static str]) -> Env<'static> {
        let owned: std::collections::BTreeMap<String, String> = vars
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        let paths: Vec<&'static str> = on_path.to_vec();
        Env {
            lookup: std::sync::Arc::new(move |key| owned.get(key).cloned()),
            resolve: std::sync::Arc::new(move |program| paths.contains(&program)),
        }
    }

    /// Read one variable.
    #[must_use]
    pub fn var(&self, key: &str) -> Option<String> {
        (self.lookup)(key)
    }

    /// Can this program be run here?
    #[must_use]
    pub fn runnable(&self, program: &str) -> bool {
        (self.resolve)(program)
    }
}

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

/// What an editor handoff decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Editor {
    /// Run this, at this line.
    Open(Command),
    /// There is no way to name one, with the reason.
    Unavailable {
        /// What was missing.
        why: String,
    },
}

/// Decide how to open `path` at `line` (1-based; 0 means "wherever it opens"). Nothing is run here.
#[must_use]
pub fn editor_plan(os: Os, path: &str, line: u32, env: &Env<'_>) -> Editor {
    let spec = ["VISUAL", "EDITOR"]
        .iter()
        .find_map(|key| env.var(key).filter(|value| !value.trim().is_empty()));
    let (spec, source) = match spec {
        Some(spec) => (spec, "from the environment".to_owned()),
        None => (
            os.default_editor().to_owned(),
            format!(
                "neither $VISUAL nor $EDITOR is set, so this is {}'s default",
                os.name()
            ),
        ),
    };
    let mut words = spec.split_whitespace();
    let program = words.next().unwrap_or("").to_owned();
    if program.is_empty() {
        return Editor::Unavailable {
            why: "the editor named nothing".to_owned(),
        };
    }
    if !env.runnable(&program) {
        return Editor::Unavailable {
            why: format!("`{program}` ({source}) is not on PATH"),
        };
    }
    let mut args: Vec<String> = words.map(str::to_owned).collect();
    let name = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&program)
        .to_ascii_lowercase();
    let trimmed = name.trim_end_matches(".exe");
    let note = if line == 0 {
        args.push(path.to_owned());
        "opened without a line".to_owned()
    } else if matches!(
        trimmed,
        "vim"
            | "vi"
            | "nvim"
            | "mvim"
            | "lvim"
            | "nvimpager"
            | "vim-tiny"
            | "kak"
            | "nano"
            | "micro"
    ) {
        args.push(format!("+{line}"));
        args.push(path.to_owned());
        format!("`{trimmed}` takes +{line}")
    } else if matches!(
        trimmed,
        "code" | "code-insiders" | "codium" | "code-oss" | "cursor" | "windsurf"
    ) {
        args.push("--goto".to_owned());
        args.push(format!("{path}:{line}"));
        format!("`{trimmed}` takes --goto path:line")
    } else if matches!(
        trimmed,
        "idea" | "goland" | "pycharm" | "clion" | "webstorm" | "rubymine" | "rider" | "rustrover"
    ) {
        args.push("--line".to_owned());
        args.push(line.to_string());
        args.push(path.to_owned());
        format!("`{trimmed}` takes --line LINE path")
    } else if matches!(
        trimmed,
        "hx" | "zed" | "subl" | "sublime_text" | "lite" | "gap"
    ) {
        args.push(format!("{path}:{line}"));
        format!("`{trimmed}` takes path:line")
    } else if trimmed.starts_with("emacs") {
        args.push(format!("+{line}"));
        args.push(path.to_owned());
        format!("`{trimmed}` takes +{line}")
    } else {
        args.push(path.to_owned());
        format!(
            "`{trimmed}` has no documented line argument, so it opens at the top — the line is {line}"
        )
    };
    Editor::Open(Command {
        program,
        args,
        stdin: None,
        why: format!("{source} · {note}"),
    })
}
