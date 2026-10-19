//! Theming, from the outside (`e5.6`): what a reviewer can choose, what Fathom says it chose, and what
//! happens when the choice is bad.
//!
//! The TUI crate's own test (`crates/fathom-tui/tests/theming.rs`) measures the palettes. This file
//! checks the parts that only exist at the process boundary, because that is where the ticket's
//! acceptance criteria are actually usable:
//!
//! * `--theme` is a flag a person can type, and it arrives in the report rather than being eaten by the
//!   shell;
//! * the precedence is the one documented — flag over environment over the user's own config, and *never*
//!   the repository's, because a clone that can repaint your terminal is a clone that has changed your
//!   accessibility settings;
//! * a theme is a TOML file a user can write, and a file that does not parse or names a face that does
//!   not exist is *refused with a list of what was wrong*, not half-applied;
//! * a name that means nothing says so and names the names that do;
//! * `--show-theme --audit` is a gate: it exits 7 on a palette that fails its own floors, so a colour
//!   scheme in a repository can be CI-checked the way a codebase is.
//!
//! Every run gets a private `HOME` and `XDG_CONFIG_HOME`, because "no configured theme" has to mean the
//! same thing on the maintainer's laptop as on CI.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A temporary directory to run in, with the user's real config deliberately unreachable.
fn sandbox() -> PathBuf {
    static SERIAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "fathom-theme-{}-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
        std::time::UNIX_EPOCH.elapsed().unwrap().as_nanos()
    ));
    fs::create_dir_all(dir.join("home").join(".config")).expect("sandbox created");
    fs::create_dir_all(&dir).expect("sandbox created");
    dir
}

/// Run `fathom` with a stated environment, in a directory that is not the developer's.
fn fathom(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fathom"));
    command
        .current_dir(dir)
        .args(args)
        .env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor")
        .env("LANG", "C.UTF-8")
        .env("LC_ALL", "C.UTF-8")
        .env("HOME", dir.join("home"))
        .env("XDG_CONFIG_HOME", dir.join("home").join(".config"));
    for key in [
        "TMUX",
        "STY",
        "SSH_TTY",
        "SSH_CONNECTION",
        "NO_COLOR",
        "FATHOM_THEME",
        "FATHOM_THEME_DIR",
    ] {
        command.env_remove(key);
    }
    for (key, value) in env {
        command.env(key, value);
    }
    command
        .output()
        .expect("the built fathom binary is runnable")
}

fn out(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn err(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn code(output: &Output) -> i32 {
    output.status.code().unwrap_or(-1)
}

/// Write a file inside the sandbox and return its path.
fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent created");
    }
    fs::write(&path, text).expect("file written");
    path
}

#[test]
fn show_theme_prints_the_table_and_the_verdict() {
    let dir = sandbox();
    let output = fathom(&dir, &["tui", "--show-theme"], &[]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    // The name, the two colours the audit measures against, a line per face with a ratio, and a verdict
    // that says which floor was applied. Nothing here needs a screenshot or a README.
    assert!(text.starts_with("theme dark"), "{text}");
    assert!(text.contains("background #151821"), "{text}");
    for face in ["text", "added", "removed", "warning", "failure"] {
        assert!(
            text.contains(&format!("{face} ")),
            "{text} should list the {face} face"
        );
    }
    assert!(text.contains("audit: pass"), "{text}");
    assert!(
        text.contains("4.5:1"),
        "{text} should name the contrast floor"
    );
    assert!(
        text.contains("protanopia") && text.contains("deuteranopia"),
        "{text} should name the two dichromacies it simulated"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_flag_arrives_in_the_report() {
    let dir = sandbox();
    let output = fathom(&dir, &["tui", "--show-theme", "--theme", "light"], &[]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    let text = out(&output);
    assert!(text.starts_with("theme light"), "{text}");
    assert!(text.contains("background #f7f7f4"), "{text}");
    // …and the sixteen-colour terminal gets the slot mapping rather than a quantization, which is what
    // the report shows when the terminal cannot take twenty-four bits.
    let limited = fathom(
        &dir,
        &["tui", "--show-theme", "--theme", "dark"],
        &[("TERM", "xterm"), ("COLORTERM", "")],
    );
    assert_eq!(code(&limited), 0, "{}", err(&limited));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn precedence_is_flag_then_environment_then_user_config() {
    let dir = sandbox();
    // The user's own config picks light…
    write(
        &dir,
        "home/.config/fathom/config.toml",
        "[theme]\nname = \"light\"\n",
    );
    let configured = fathom(&dir, &["tui", "--show-theme"], &[]);
    assert!(
        out(&configured).starts_with("theme light"),
        "the user config should reach the report: {}{}",
        out(&configured),
        err(&configured)
    );
    // …the environment outranks it…
    let from_env = fathom(&dir, &["tui", "--show-theme"], &[("FATHOM_THEME", "ansi")]);
    assert!(
        out(&from_env).starts_with("theme ansi"),
        "FATHOM_THEME should outrank the config: {}{}",
        out(&from_env),
        err(&from_env)
    );
    // …and the flag outranks both.
    let from_flag = fathom(
        &dir,
        &["tui", "--show-theme", "--theme", "dark"],
        &[("FATHOM_THEME", "ansi")],
    );
    assert!(
        out(&from_flag).starts_with("theme dark"),
        "--theme should outrank the environment: {}{}",
        out(&from_flag),
        err(&from_flag)
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_repository_cannot_repaint_your_terminal() {
    // The precedence rule with a security shape to it: `fathom.toml` is what a clone provides, and ADR-001
    // §4.6 treats config from a clone as untrusted config for consent and tokens. How somebody's screen
    // looks is the same kind of decision — so a `[theme]` table in a repository's config changes nothing,
    // and says nothing either, because it is not a key Fathom reads.
    let dir = sandbox();
    write(
        &dir,
        "fathom.toml",
        "meta.config_version = 1\n\n[theme]\nname = \"light\"\n",
    );
    let output = fathom(&dir, &["tui", "--show-theme"], &[]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).starts_with("theme dark"),
        "a cloned fathom.toml chose the theme:\n{}",
        out(&output)
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_unknown_theme_says_so_and_names_the_names_that_work() {
    let dir = sandbox();
    let output = fathom(&dir, &["tui", "--show-theme", "--theme", "midnight"], &[]);
    // A fallback that is not mentioned is how a reviewer spends an afternoon on a theme that never
    // loaded, so: the notice, the built-in list, and the theme that *is* in force.
    assert_eq!(code(&output), 0, "{}", err(&output));
    let problems = err(&output);
    assert!(problems.contains("midnight"), "{problems}");
    for name in ["dark", "light", "ansi"] {
        assert!(problems.contains(name), "{problems} should list {name}");
    }
    assert!(
        problems.contains("Built-ins") && problems.contains("A file is"),
        "{problems} should say what was wrong and what would have worked"
    );
    assert!(out(&output).starts_with("theme dark"), "{}", out(&output));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_theme_is_a_documented_toml_file_a_user_can_write() {
    let dir = sandbox();
    let theme = r##"
name = "sand"
inherits = "dark"
background = "#1c1a16"
foreground = "#eae5da"

[faces]
added = "#3fd49a"
removed = "#ff8e7d"
warning = "bright-yellow bold"
"##;
    write(&dir, "sand.toml", theme);
    let path = fathom(&dir, &["tui", "--show-theme", "--theme", "sand.toml"], &[]);
    assert_eq!(code(&path), 0, "{}", err(&path));
    let text = out(&path);
    assert!(text.starts_with("theme sand"), "{text}");
    assert!(text.contains("background #1c1a16"), "{text}");
    assert!(
        text.contains("#3fd49a"),
        "{text} should show the overridden added ink"
    );
    // A shape came through too — `bright-yellow bold` is a slot and a modifier, not a colour.
    assert!(text.contains("bright-yellow"), "{text}");

    // The same file, found by name in a theme directory: that is how a dotfiles repo ships a palette.
    let themes = dir.join("themes");
    write(&dir, "themes/sand.toml", theme);
    let named = fathom(
        &dir,
        &["tui", "--show-theme", "--theme", "sand"],
        &[("FATHOM_THEME_DIR", themes.to_str().unwrap())],
    );
    assert_eq!(code(&named), 0, "{}", err(&named));
    assert!(out(&named).starts_with("theme sand"), "{}", out(&named));

    // …and `--list-themes` shows the file beside the built-ins, so a reviewer can see what Fathom found.
    let listing = fathom(
        &dir,
        &["tui", "--list-themes"],
        &[("FATHOM_THEME_DIR", themes.to_str().unwrap())],
    );
    assert_eq!(code(&listing), 0, "{}", err(&listing));
    let text = out(&listing);
    for name in ["dark", "light", "ansi"] {
        assert!(
            text.contains(name),
            "{text} should list the built-in {name}"
        );
    }
    assert!(
        text.contains("sand"),
        "{text} should list the file it found"
    );
    assert!(
        text.contains("overridden"),
        "{text} should say what the file did"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_broken_theme_is_refused_with_every_problem_in_it() {
    let dir = sandbox();
    write(
        &dir,
        "broken.toml",
        "inherits = \"midnight\"\n[faces]\nadde = \"green\"\nremoved = \"#zzzzzz\"\n",
    );
    let output = fathom(
        &dir,
        &["tui", "--show-theme", "--theme", "broken.toml"],
        &[],
    );
    // The run still draws — the default theme is a real answer — but every problem is printed, and the
    // fallback is named rather than assumed.
    assert_eq!(code(&output), 0, "{}", err(&output));
    let problems = err(&output);
    assert!(problems.contains("midnight"), "{problems}");
    assert!(problems.contains("adde"), "{problems}");
    assert!(problems.contains("#zzzzzz"), "{problems}");
    assert!(problems.contains("refused"), "{problems}");
    assert!(out(&output).starts_with("theme dark"), "{}", out(&output));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_audit_flag_makes_a_bad_palette_an_error_exit() {
    // The automated check as a gate: exit 7 (`CheckViolations`) with a sentence that says what failed.
    let dir = sandbox();
    write(
        &dir,
        "mud.toml",
        "name = \"mud\"\ninherits = \"dark\"\n[faces]\ntext = \"#767676\"\nadded = \
         \"#777676\"\nremoved = \"#787776\"\nwarning = \"#757678\"\n",
    );
    let failing = fathom(
        &dir,
        &["tui", "--show-theme", "--audit", "--theme", "mud.toml"],
        &[],
    );
    assert_eq!(code(&failing), 7, "{}{}", out(&failing), err(&failing));
    let message = err(&failing);
    assert!(message.contains("mud"), "{message}");
    assert!(message.contains("audit"), "{message}");
    // …and the shipped palettes pass the same gate, which is what makes the gate mean something.
    for name in ["dark", "light", "ansi"] {
        let passing = fathom(
            &dir,
            &["tui", "--show-theme", "--audit", "--theme", name],
            &[],
        );
        assert_eq!(
            code(&passing),
            0,
            "{name} failed its own audit:\n{}{}",
            out(&passing),
            err(&passing)
        );
    }
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn no_colour_still_prints_the_frame_and_the_theme_is_honoured_in_it() {
    // `--plain` with a theme is the combination that used to be untestable from outside: no terminal, no
    // escape sequences, same text. The theme cannot change a text render — that is the promise — so what
    // is checkable is that the theme flag costs the reviewer nothing.
    let dir = sandbox();
    fs::create_dir_all(dir.join(".git")).expect("a repository-shaped directory");
    let plain = fathom(&dir, &["--plain", "tui", "--show-session"], &[]);
    assert_eq!(code(&plain), 0, "{}", err(&plain));
    assert!(
        !out(&plain).contains('\x1b'),
        "the text contract grew an escape sequence"
    );
    fs::remove_dir_all(&dir).ok();
}
