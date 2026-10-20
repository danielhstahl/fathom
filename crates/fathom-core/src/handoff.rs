//! `fathom_core::handoff` — deciding what to run when Fathom is not the right tool, without running it.
//!
//! Fathom is a reader, not an editor (ADR-001 §8 non-goal 2), so the exits have to be good. Every exit
//! here is decided **before** anything is spawned, as a [`Command`] the caller can print, log and test —
//! and nothing in this crate ever spawns it. `fathom-core` reads no environment and starts no process
//! (ADR-002 §3, lints `W1`/`W2`), so the two facts a plan needs — what the variables say and what is on
//! `PATH` — arrive as an injected [`Env`], and the OS is a parameter rather than a `cfg` so the
//! three-platform matrix is one test run on one machine instead of three CI runners a contributor
//! cannot run locally (`e5.8`, `reco-dmw`).
//!
//! Three doors, all of them decided here:
//!
//! * **[`editor_plan`]** opens a file at a line in `$EDITOR`. Precedence is `$FATHOM_EDITOR`, then git's
//!   own `$VISUAL`/`$EDITOR`, then the platform default — and the line argument is the one *that* editor
//!   documents: `+LINE` for the vi
//!   family, `--goto path:line` for the VS Code family, `path:line` for zed and helix, `--line LINE` for
//!   the JetBrains CLIs, and no line argument at all for the editors that have none — said out loud
//!   rather than invented and blamed on the editor when it fails.
//! * **[`difftool_plan`]** hands one file's two sides to a heavyweight diff tool (difftastic, meld,
//!   kdiff3, `vim -d`, `code --diff`, …) when the reviewer wants more than Fathom's own pane, and falls
//!   back through the tools that are actually installed when the configured one is not.
//! * **what it decided is part of the plan.** Every [`Command`] carries a `why`, and every refusal carries
//!   the reason and the key that changes it. "No editor" and "no difftool" are ordinary outcomes with a
//!   sentence attached, not errors with a stack trace.
//!
//! The copy half of the handoff story (clipboard, OSC 52) lives with the shell that owns the terminal, in
//! `fathom_tui::diff::handoff`; this module is the part two binaries and a TUI all need to agree on.

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
    /// The OS this build is running on. Compile-time, because `fathom-core` reads no environment of
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

    /// The difftools this OS is expected to have, best first, as `(name, program)` pairs.
    ///
    /// A name rather than a bare program because the name is what a user writes in their config and the
    /// program is what this table has to find on `PATH`: `difftastic` is the tool's name and `difft` is
    /// the binary, and a plan that looked for `difftastic` on `PATH` would never find it.
    #[must_use]
    pub const fn difftool_candidates(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Os::Windows => &[
                ("difftastic", "difft"),
                ("kdiff3", "kdiff3"),
                ("winmerge", "WinMergeU"),
                ("vscode", "code"),
            ],
            Os::Linux | Os::MacOs | Os::Other => &[
                ("difftastic", "difft"),
                ("meld", "meld"),
                ("kdiff3", "kdiff3"),
                ("nvim", "nvim"),
                ("vscode", "code"),
            ],
        }
    }
}

/// Something to run, decided in advance and printable.
///
/// Nothing in this crate runs it: `fathom-core` starts no process (ADR-002 §3), so the caller either
/// prints it (`fathom open --print`, a toast) or runs it itself — the TUI hands it back through
/// [`crate::handoff::Handoff`] to the loop that owns the screen, and a CLI verb spawns it with
/// `std::process::Command`. A plan that cannot be printed is a plan that cannot be debugged.
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
pub type Lookup<'a> = std::sync::Arc<dyn Fn(&str) -> Option<String> + Send + Sync + 'a>;
/// How an [`Env`] decides whether a program can be run on this machine.
pub type Resolve<'a> = std::sync::Arc<dyn Fn(&str) -> bool + Send + Sync + 'a>;

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
    /// reads it and installs it with `App::install_handoff_env`. With nothing installed, a copy falls
    /// through to OSC 52 and the toast says so, which is a stated answer rather than a wrong one.
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

/// The variables an editor is read from, in precedence order.
///
/// `FATHOM_EDITOR` is first because it is the one knob that does not disturb the rest of the shell:
/// `VISUAL` then `EDITOR` are git's own order, and a reviewer who wants Fathom to use something other
/// than their shell's editor sets the first without rewriting the second.
pub const EDITOR_KEYS: &[&str] = &["FATHOM_EDITOR", "VISUAL", "EDITOR"];

/// Decide how to open `path` at `line` (1-based; 0 means "wherever it opens"). Nothing is run here.
///
/// `request` is a spec the *reviewer named on the command line* (`fathom open --editor "nvim --listen"`),
/// which is a different fact from an environment variable and has to be described differently: a plan
/// that says "from $EDITOR" about a value nobody exported is the start of a confusing bug report.
/// `None` means the usual walk — `$FATHOM_EDITOR`, `$VISUAL`, `$EDITOR`, the platform default.
///
/// The sentence in [`Command::why`] names both halves of the decision — where the editor came from and
/// which line syntax that editor takes — because "it opened at the top" and "nothing happened" are the
/// two failures a reviewer cannot diagnose from the outside, and this ticket's acceptance criterion is
/// that the tool says what it decided.
#[must_use]
pub fn editor_plan(os: Os, path: &str, line: u32, request: Option<&str>, env: &Env<'_>) -> Editor {
    let requested = request
        .map(str::trim)
        .filter(|value| !value.trim().is_empty());
    let spec = requested.map(str::to_owned).or_else(|| {
        EDITOR_KEYS
            .iter()
            .find_map(|key| env.var(key).filter(|value| !value.trim().is_empty()))
    });
    let (spec, source) = match spec {
        Some(spec) => (
            spec,
            requested.map_or_else(
                || {
                    format!(
                        "from ${}",
                        EDITOR_KEYS
                            .iter()
                            .find(|key| env.var(key).is_some_and(|value| !value.trim().is_empty()))
                            .copied()
                            .unwrap_or("EDITOR")
                    )
                },
                |asked| format!("`{asked}` was asked for"),
            ),
        ),
        None => (
            os.default_editor().to_owned(),
            format!(
                "neither $FATHOM_EDITOR, $VISUAL nor $EDITOR is set, so this is {}'s default",
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

/// What a difftool handoff decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Difftool {
    /// Run this on the two prepared files.
    Open(Command),
    /// Nothing could be run, with the reason and the names that were looked for.
    Unavailable {
        /// Why nothing was chosen, in the tool's own words.
        why: String,
        /// Every tool name that was considered, so the remedy can name one of them.
        tried: Vec<String>,
    },
}

/// The difftools Fathom knows how to drive, as `(config name, program, args before the two files)`.
///
/// The list is closed on purpose: a difftool is handed two real files and the reviewer's eyes, and an
/// argument shape guessed at is a tool that opens the wrong side twice. Anything else is reachable with
/// `FATHOM_DIFFTOOL=<program>` — an unknown name is passed through with the two paths last, which is
/// what most of the rest of them want.
pub const DIFFTOOLS: &[(&str, &str, &[&str])] = &[
    ("difftastic", "difft", &[]),
    ("meld", "meld", &[]),
    ("kdiff3", "kdiff3", &[]),
    ("winmerge", "WinMergeU", &["/e"]),
    ("beyondcompare", "bcompare", &[]),
    ("xxdiff", "xxdiff", &[]),
    ("nvim", "nvim", &["-d"]),
    ("vim", "vim", &["-d"]),
    ("vscode", "code", &["--diff"]),
    ("codium", "codium", &["--diff"]),
    ("cursor", "cursor", &["--diff"]),
];

/// Decide how to show `left` against `right` in a heavyweight diff tool. Nothing is run here.
///
/// `request` is what the user asked for — `--tool`, `$FATHOM_DIFFTOOL`, or the user config's
/// `[diff] difftool` — and it is the caller's job to have resolved that precedence, because reading a
/// config file is IO and this function is not (`e5.8`'s rule is the same as ADR-002 §4.5's: the
/// composition root decides what the configuration *is*). What this function owns is the argument shape
/// and the fallback:
///
/// * a requested tool that is not on `PATH` is a stated fallback, not a silent substitution — the plan
///   says which tool was asked for, which one is being used instead, and why;
/// * an unrecognised requested name is used as the program itself with the two paths last, so a
///   reviewer with a difftool this table has never heard of is one variable away from using it;
/// * with nothing requested, the platform's candidate list is walked and the first one that exists wins;
/// * with none of them, the answer is a sentence, and it names the keys that change it. That is the
///   "no difftool is a normal path" acceptance criterion, and it is why this returns a value rather
///   than failing.
#[must_use]
pub fn difftool_plan(
    os: Os,
    request: Option<&str>,
    left: &str,
    right: &str,
    env: &Env<'_>,
) -> Difftool {
    let requested = request.map(str::trim).filter(|value| !value.is_empty());
    let source = match requested {
        Some(value) => format!("`{value}` was asked for"),
        None => format!(
            "nothing was configured, so this is {}'s own order",
            os.name()
        ),
    };
    let mut tried: Vec<String> = Vec::new();
    if let Some(wanted) = requested {
        tried.push(wanted.to_owned());
        // A name from the table, or a program the reviewer named themselves — possibly with the
        // arguments it needs, which is the only way to drive a tool this table has never heard of
        // (`--tool "mytool --click --wait"`). Known names keep their own argument shape whatever
        // follows them, because `vscode -d` would ask VS Code to open a folder.
        let (program, inline) = match wanted.split_once(char::is_whitespace) {
            Some((head, rest)) => (head, rest.split_whitespace().map(str::to_owned).collect()),
            None => (wanted, Vec::new()),
        };
        if let Some((name, known, default_prefix)) = lookup_difftool(program) {
            if env.runnable(known) {
                return open(
                    name,
                    known,
                    default_prefix,
                    left,
                    right,
                    format!("{source} · `{known}` is on PATH"),
                );
            }
            tried.push(known.to_owned());
            // Asked for and not there. The tool's config name and its program are two strings a user
            // can plausibly get wrong (`difftastic` / `difft`), so the refusal names the one it looked
            // for — and then falls through to the candidate walk, because a reviewer who asked for a
            // heavyweight diff and has *a* heavyweight diff installed wants to see a diff.
        } else if env.runnable(program) {
            // Not in the table: the user knows a tool this list does not. Use it as named and put the
            // two files last, which is the argument shape the rest of them want.
            let how = if inline.is_empty() {
                "not a known name, so it runs as typed with the two files last"
            } else {
                "not a known name, so it runs as typed — its own arguments first, then the two files"
            };
            let inline: Vec<&str> = inline.iter().map(String::as_str).collect();
            return open(
                program,
                program,
                &inline,
                left,
                right,
                format!("{source} · {how}"),
            );
        }
        // ...and an unknown name that is not on PATH either falls through below.
    }
    for (name, program) in os.difftool_candidates() {
        tried.push((*name).to_owned());
        let Some((_name, _program, prefix)) = lookup_difftool(name) else {
            continue;
        };
        if env.runnable(program) {
            let note = match requested {
                Some(wanted) => format!(
                    "{source} but `{wanted}` is not on PATH, so this one is being used instead — \
                     install it or name another tool with --tool / $FATHOM_DIFFTOOL to stop the \
                     substitution",
                ),
                None => format!("{source} · `{program}` is on PATH"),
            };
            return open(name, program, prefix, left, right, note);
        }
    }
    let missing = match requested {
        Some(wanted) => format!(
            "`{wanted}` was asked for and is not on PATH, and neither is anything else this tool \
             knows how to drive"
        ),
        None => format!("none of {} is on PATH", tried.join(", ")),
    };
    Difftool::Unavailable {
        why: format!(
            "{missing} — set $FATHOM_DIFFTOOL or `[diff] difftool` in your user config to a tool \
             that is, or read the diff with `fathom analyze` / the `d` pane, which needs nothing \
             installed"
        ),
        tried,
    }
}

/// The table lookup, by config name or by program name (either spells either).
#[must_use]
fn lookup_difftool(name: &str) -> Option<(&'static str, &'static str, &'static [&'static str])> {
    let key = name.trim().to_ascii_lowercase();
    let key = key.rsplit(['/', '\\']).next().unwrap_or(&key);
    let key = key.trim_end_matches(".exe");
    DIFFTOOLS
        .iter()
        .find(|(config, program, _)| *config == key || *program == key)
        .map(|(config, program, prefix)| (*config, *program, *prefix))
}

/// One assembled difftool command. The two files always come last, because every tool in the table
/// takes them in that order.
#[must_use]
fn open(
    name: &str,
    program: &str,
    prefix: &[&str],
    left: &str,
    right: &str,
    why: String,
) -> Difftool {
    let mut args: Vec<String> = prefix.iter().map(|arg| (*arg).to_owned()).collect();
    args.push(left.to_owned());
    args.push(right.to_owned());
    Difftool::Open(Command {
        program: program.to_owned(),
        args,
        stdin: None,
        why: format!("difftool `{name}` — {why}"),
    })
}
