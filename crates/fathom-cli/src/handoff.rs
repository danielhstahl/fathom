//! The environment the shell's handoffs are planned against (ADR-002 §3, ADR-006 §7, `e5.8`).
//!
//! `fathom-tui` is IO-free by rule: it reads no environment, opens no files and spawns nothing, so the
//! two facts its copy and `$EDITOR` plans need — what the variables say, and what is on `PATH` — are
//! read *here* and handed over as an [`Env`] of closures. This is the same arrangement as the keymap
//! overrides and the repository label: this crate decides, that one notices.
//!
//! Nothing here plans a command — [`fathom_core::handoff`] does that, so the shell's `E` key and
//! `fathom open` cannot disagree about which editor takes which argument. What lives here is the IO on
//! both sides of the plan: reading what the environment and the reviewer's own config say
//! ([`environment`], [`Settings`]), and running what was planned with the terminal handed over
//! ([`run`]).
//!
//! # Who gets to name a program this process runs
//!
//! An editor and a difftool are executables Fathom launches with the reviewer's own rights, so the
//! precedence is deliberately the shape `[theme]` uses and deliberately excludes one layer:
//!
//! ```text
//!   --editor / --tool                   what was asked for on this command line
//!   $FATHOM_EDITOR, $VISUAL, $EDITOR, $FATHOM_DIFFTOOL
//!   ~/.config/fathom/config.toml        `[ui] editor`, `[diff] difftool`
//!   the platform default                `vi`, or the first difftool on PATH
//! ```
//!
//! A repository's own `fathom.toml` is not in that list. A clone naming a program your machine runs is
//! the same class of thing as a clone choosing a theme your eyes have to read or consenting on your
//! behalf (ADR-001 §4.6: config a clone provides is untrusted config) and, unlike a theme, an editor
//! *executes*. A repository that tries anyway is told why it was ignored rather than silently
//! overruled — `fathom open` prints the notice, and so does the shell's own notice list.

use std::path::Path;

use fathom_core::handoff::{Command, Env, Lookup};

/// The calling process's environment, as a handoff plan wants it.
///
/// `tui`-gated: the shell is what hands a file off to an editor. `fathom open` builds its own environment
/// from the resolved settings, and a headless build does not need this one.
#[cfg(feature = "tui")]
#[must_use]
pub fn environment() -> Env<'static> {
    environment_with(&Settings::default())
}

/// The environment *plus* whatever the reviewer's own config contributed, as one lookup.
///
/// The config value is folded in under the key the planner already reads (`FATHOM_EDITOR`), so the
/// precedence lives in one place instead of being applied twice — once here and once by every caller.
#[must_use]
pub fn environment_with(settings: &Settings) -> Env<'static> {
    Env {
        lookup: overlay(
            std::sync::Arc::new(|key| std::env::var(key).ok()),
            settings.editor.clone(),
        ),
        resolve: std::sync::Arc::new(on_path),
    }
}

/// Put a config-provided editor underneath a real variable lookup.
///
/// Split out from [`environment_with`] because the precedence is the part worth testing, and a test that
/// sets a process variable to prove it would be a test that cannot run in parallel with anything else.
#[must_use]
fn overlay(variables: Lookup<'static>, editor: Option<String>) -> Lookup<'static> {
    std::sync::Arc::new(move |key: &str| {
        variables(key).or_else(|| {
            if key == "FATHOM_EDITOR" {
                editor.clone()
            } else {
                None
            }
        })
    })
}

/// What the reviewer configured about the exits, with the provenance of each answer.
#[derive(Debug, Clone, Default)]
pub struct Settings {
    /// The editor spec to plan with, when the environment did not answer.
    pub editor: Option<String>,
    /// Where that came from, for the message that says what was used.
    pub editor_source: Option<String>,
    /// The difftool name to plan with, when the environment did not answer.
    pub difftool: Option<String>,
    /// Where that came from.
    pub difftool_source: Option<String>,
    /// What the repository's config tried to set and was not allowed to, in words.
    pub notices: Vec<String>,
}

impl Settings {
    /// Read the reviewer's own config, and notice what the repository's config tried to do.
    ///
    /// A missing file is the normal case and says nothing. A file that will not parse is reported once
    /// and then ignored, exactly as `[theme]` does: a typo in an unrelated section must not stop a
    /// reviewer from opening a file.
    #[must_use]
    pub fn from_config(root: &Path) -> Self {
        let mut settings = Settings::default();
        let mut editor_done = false;
        let mut difftool_done = false;
        for path in crate::userconfig::candidates() {
            let Some(text) = read(&path) else { continue };
            let Some(document) = parse(&text, &path, &mut settings.notices) else {
                continue;
            };
            if !editor_done && let Some(editor) = string_at(&document, "ui", "editor") {
                settings.editor = Some(editor);
                settings.editor_source = Some(format!("`[ui] editor` in {}", display(&path)));
                editor_done = true;
            }
            if !difftool_done && let Some(tool) = string_at(&document, "diff", "difftool") {
                settings.difftool = Some(tool);
                settings.difftool_source = Some(format!("`[diff] difftool` in {}", display(&path)));
                difftool_done = true;
            }
            // First file that exists wins, section by section — the same rule `crate::theme` applies.
            if editor_done && difftool_done {
                break;
            }
        }
        // The repository's own file, read only to say what was ignored: same parser, opposite policy.
        let repo = root.join("fathom.toml");
        if let Some(text) = read(&repo) {
            let mut notices = Vec::new();
            if let Some(document) = parse(&text, &repo, &mut notices) {
                for (table, key) in [("ui", "editor"), ("diff", "difftool")] {
                    if let Some(value) = string_at(&document, table, key) {
                        notices.push(format!(
                            "fathom.toml's `[{table}] {key} = {value:?}` was ignored: a clone does \
                             not get to name a program this process runs. Put it in \
                             ~/.config/fathom/config.toml, or in the environment, instead."
                        ));
                    }
                }
            }
            settings.notices.extend(notices);
        }
        settings
    }

    /// The difftool to ask the planner for, and where that answer came from, in precedence order: the
    /// flag, then `$FATHOM_DIFFTOOL`, then the user config. `None` means "choose for me", which the
    /// planner answers with the platform's own candidate order — and says so.
    #[must_use]
    pub fn difftool_request(&self, flag: Option<&str>) -> (Option<String>, String) {
        if let Some(tool) = flag.filter(|value| !value.trim().is_empty()) {
            return (Some(tool.to_owned()), "`--tool`".to_owned());
        }
        if let Some(tool) = std::env::var("FATHOM_DIFFTOOL")
            .ok()
            .filter(|value| !value.trim().is_empty())
        {
            return (Some(tool), "$FATHOM_DIFFTOOL".to_owned());
        }
        match (&self.difftool, &self.difftool_source) {
            (Some(tool), Some(source)) => (Some(tool.clone()), source.clone()),
            _ => (None, "nothing was configured".to_owned()),
        }
    }
}

/// Run a planned command with this process's own stdin, stdout and stderr — which is the whole point:
/// an editor that cannot draw is worse than no editor, so the terminal is handed over untouched.
///
/// The exit status comes back rather than being interpreted: a reviewer who quit `vim` with `:cq`
/// cancelled their edit, and the caller decides whether that is worth an exit code.
///
/// # Errors
///
/// Why the program could not be started at all, which is a different thing from it running and failing
/// and must not share a sentence with it.
pub fn run(command: &Command) -> Result<std::process::ExitStatus, String> {
    let mut child = std::process::Command::new(&command.program)
        .args(&command.args)
        .stdin(match command.stdin {
            Some(_) => std::process::Stdio::piped(),
            None => std::process::Stdio::inherit(),
        })
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .map_err(|err| format!("`{}` could not be started: {err}", command.program))?;
    if let Some(text) = &command.stdin
        && let Some(mut pipe) = child.stdin.take()
    {
        use std::io::Write as _;
        let _ = pipe.write_all(text.as_bytes());
    }
    child
        .wait()
        .map_err(|err| format!("`{}` could not be waited for: {err}", command.program))
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

/// One file's text, or `None` when it is not there — which is the normal case and never a notice.
fn read(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// Parse one config file, or notice that it will not parse.
fn parse(text: &str, path: &Path, notices: &mut Vec<String>) -> Option<toml_edit::DocumentMut> {
    match text.parse::<toml_edit::DocumentMut>() {
        Ok(document) => Some(document),
        Err(err) => {
            notices.push(format!(
                "{} does not parse, so the editor and difftool defaults are in force: {err}",
                display(path)
            ));
            None
        }
    }
}

/// `key` from `[table]`, if that is what is there.
fn string_at(document: &toml_edit::DocumentMut, table: &str, key: &str) -> Option<String> {
    document
        .get(table)?
        .as_table()?
        .get(key)?
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
}

/// A path for a message: home-relative where that is possible, as written otherwise.
fn display(path: &Path) -> String {
    match std::env::var_os("HOME") {
        Some(home) => Path::new("~")
            .join(path.strip_prefix(home).unwrap_or(path))
            .display()
            .to_string(),
        None => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

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

    #[test]
    fn a_real_variable_beats_the_config_and_the_config_beats_the_default() {
        // The precedence, in the one place it is implemented. Neither half can be tested by setting a
        // process variable here — that would serialise every test in the binary — so the lookup under
        // test is the one the environment is built out of, with its variable source substituted.
        let nothing: Lookup<'static> = std::sync::Arc::new(|_key: &str| None);
        let env = Env {
            lookup: overlay(nothing.clone(), Some("hx --noplugins".to_owned())),
            resolve: std::sync::Arc::new(|_program| true),
        };
        assert_eq!(env.var("FATHOM_EDITOR").as_deref(), Some("hx --noplugins"));
        assert_eq!(
            env.var("EDITOR").as_deref(),
            None,
            "config is not the shell's `$EDITOR`"
        );
        let variable: Lookup<'static> =
            std::sync::Arc::new(|key: &str| (key == "FATHOM_EDITOR").then(|| "vim".to_owned()));
        let env = Env {
            lookup: overlay(variable, Some("hx".to_owned())),
            resolve: std::sync::Arc::new(|_program| true),
        };
        assert_eq!(
            env.var("FATHOM_EDITOR").as_deref(),
            Some("vim"),
            "$FATHOM_EDITOR wins"
        );
    }

    #[test]
    fn a_difftool_request_names_its_source_so_the_plan_can_say_it() {
        let settings = Settings {
            difftool: Some("meld".to_owned()),
            difftool_source: Some("`[diff] difftool` in ~/.config/fathom/config.toml".to_owned()),
            ..Settings::default()
        };
        let (tool, source) = settings.difftool_request(Some("kdiff3"));
        assert_eq!(tool.as_deref(), Some("kdiff3"));
        assert_eq!(source, "`--tool`");
        let (tool, source) = settings.difftool_request(None);
        assert_eq!(tool.as_deref(), Some("meld"));
        assert!(source.contains("difftool"), "{source}");
        let bare = Settings::default();
        let (_tool, source) = bare.difftool_request(None);
        assert!(
            source == "nothing was configured" || source == "$FATHOM_DIFFTOOL",
            "an unconfigured request has to say which of the two it is: {source}"
        );
    }

    #[test]
    fn an_unparseable_config_is_a_notice_and_not_a_refusal() {
        let dir = std::env::temp_dir().join(format!("fathom-handoff-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("fathom.toml");
        std::fs::write(&path, "this is not = = toml").unwrap();
        let mut notices = Vec::new();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(parse(&text, &path, &mut notices).is_none());
        assert!(notices[0].contains("does not parse"), "{notices:?}");
        assert!(notices[0].contains("defaults are in force"), "{notices:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_repository_cannot_name_an_editor_for_the_reviewer() {
        // The untrusted-clone rule, checked against a real file rather than asserted in prose.
        let dir = std::env::temp_dir().join(format!("fathom-handoff-repo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("fathom.toml"),
            "[ui]\neditor = \"curl\"\n[diff]\ndifftool = \"wget\"\n",
        )
        .unwrap();
        let settings = Settings::from_config(&dir);
        assert!(
            settings.editor.is_none(),
            "the repository's editor must not be honoured"
        );
        assert!(settings.difftool.is_none(), "nor its difftool");
        assert_eq!(settings.notices.len(), 2, "{:?}", settings.notices);
        assert!(
            settings
                .notices
                .iter()
                .all(|notice| notice.contains("ignored")),
            "{:?}",
            settings.notices
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn running_a_program_that_is_not_there_is_a_different_sentence_from_failing() {
        let command = Command {
            program: "fathom-definitely-not-a-real-program".to_owned(),
            args: vec![],
            stdin: None,
            why: "test".to_owned(),
        };
        let err = run(&command)
            .err()
            .unwrap_or_else(|| panic!("spawn must fail"));
        assert!(err.contains("could not be started"), "{err}");
    }

    #[test]
    fn a_program_that_runs_reports_its_own_status() {
        // The other half of the same door: `sh -c 'exit 3'` runs, and the status says 3. `sh` is on
        // every POSIX platform the matrix names; on Windows this test is the same shape with `cmd`.
        let (program, args): (&str, Vec<String>) = if cfg!(windows) {
            ("cmd", vec!["/C".to_owned(), "exit 3".to_owned()])
        } else {
            ("sh", vec!["-c".to_owned(), "exit 3".to_owned()])
        };
        let command = Command {
            program: program.to_owned(),
            args,
            stdin: None,
            why: "test".to_owned(),
        };
        let status = run(&command).unwrap_or_else(|err| panic!("{err}"));
        assert_eq!(status.code(), Some(3));
    }
}
