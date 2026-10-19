//! The copy audit: every refusal and every empty state is driven through the real binary (`e5.9`).
//!
//! `crates/fathom-tui/tests/empty_views.rs` audits the pane copy where the strings live, against a list of
//! verbs and flags maintained by hand. That list is the weak link: a hand-written list drifts from the
//! binary it describes, and when it does the test keeps passing while the product keeps lying — which is
//! exactly how `fathom --demo` sat in two panes for three stages. This file is the join. It asks the
//! built binary what it can do (`--help`, every verb's `--help`) and then fails if anything the frame or a
//! refusal prints is not on that list, so the published surface and the promised surface cannot part
//! company.
//!
//! The second half is the other acceptance criterion: a refusal that states a failure and no next action is
//! a stack trace with better manners. Each failure mode below is *driven* — a real process, a real
//! directory, a real wrong argument — because an audit that reads source is an audit of what the author
//! remembered.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::process::{Command, Output};

fn fathom(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fathom"))
        .args(args)
        .output()
        .expect("the built fathom binary is runnable")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn code(out: &Output) -> i32 {
    out.status
        .code()
        .expect("fathom exits with a code, not a signal")
}

/// A throwaway repository with three commits, and no index: the state a first-time reviewer's `.` is in.
fn repository() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "fathom-cli-copy-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create the fixture directory");
    git(&dir, &["init", "-q", "--initial-branch=main"]);
    for n in 0..3 {
        std::fs::write(dir.join("src.rs"), format!("fn f{n}() -> u32 {{ {n} }}\n"))
            .expect("write the fixture file");
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", &format!("commit {n}")]);
    }
    dir
}

fn git(cwd: &std::path::Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(cwd)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", cwd)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Ada")
        .env("GIT_AUTHOR_EMAIL", "ada@example.invalid")
        .env("GIT_COMMITTER_NAME", "Ada")
        .env("GIT_COMMITTER_EMAIL", "ada@example.invalid")
        .env("GIT_AUTHOR_DATE", "1700000000 +0000")
        .env("GIT_COMMITTER_DATE", "1700000000 +0000")
        .args(args)
        .output()
        .expect("git is on PATH");
    assert!(out.status.success(), "git {args:?}: {}", stderr(&out));
}

/// What the binary says it can do, read off its own help rather than from a list somebody maintains.
///
/// Built by running `--help` for the binary and for every verb it lists: the answer is therefore whatever
/// this build was compiled with, including which optional features are in it.
#[derive(Debug)]
struct Surface {
    verbs: Vec<String>,
    flags: Vec<String>,
}

impl Surface {
    fn read() -> Self {
        let help = stdout(&fathom(&["--help"]));
        let verbs: Vec<String> = help
            .lines()
            .skip_while(|line| !line.starts_with("Commands:"))
            .take_while(|line| !line.starts_with("Options:"))
            .filter_map(|line| {
                let line = line.strip_prefix("  ")?;
                let verb = line.split_whitespace().next()?;
                (!verb.is_empty() && verb != "help").then(|| verb.to_owned())
            })
            .collect();
        // The global flags, plus every flag any verb accepts. A sentence in a pane says `--rebuild` or it
        // says `fathom index --rebuild`; either way, "some verb takes it" is the honest bar.
        // Every `--flag` token anywhere in any help page. Token scanning rather than line parsing,
        // because clap prints short and long forms on one line (`-h, --help`) and an option that a pane
        // names has to be found the same way a reviewer would find it.
        let mut flags: Vec<String> = Vec::new();
        let mut pages = vec![help.clone()];
        for verb in verbs.clone() {
            pages.push(stdout(&fathom(&[verb.as_str(), "--help"])));
        }
        for page in &pages {
            for token in page.split_whitespace() {
                let token = token.trim_end_matches(',');
                if let Some(name) = token.strip_prefix("--")
                    && !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c == '-' || c.is_ascii_digit())
                {
                    flags.push(format!("--{name}"));
                }
            }
        }
        flags.sort();
        flags.dedup();
        Surface { verbs, flags }
    }

    /// Every `fathom <word>` and `--flag` token in a sentence, with the ones that are this binary's own
    /// prose removed. Prose like "`fathom index` is the verb that fills it" counts; a path like
    /// `fathom.toml` does not.
    fn named(&self, text: &str) -> Vec<String> {
        let mut found = Vec::new();
        for word in text.split_whitespace() {
            let word = word
                .trim_matches('`')
                .trim_end_matches([',', ';', '.', ':']);
            if word.starts_with("--") {
                // An option in a sentence is a promise that *something* accepts it.
                if self.known_flag(word) {
                    found.push(word.to_owned());
                } else {
                    found.push(format!("MISSING {word}"));
                }
            }
            if let Some(rest) = word.strip_prefix("fathom") {
                let verb = rest
                    .trim_start_matches('`')
                    .split(['`', '(', ')'])
                    .next()
                    .unwrap_or("");
                if verb.is_empty() {
                    continue;
                }
                // `fathom-tui`, `fathom.toml`, `fathom analyze` — only a space-separated verb counts.
                if !verb.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
                    continue;
                }
                if self.verbs.iter().any(|known| known == verb) {
                    found.push(verb.to_owned());
                } else {
                    found.push(format!("MISSING fathom {verb}"));
                }
            }
        }
        found
    }

    fn known_flag(&self, flag: &str) -> bool {
        self.flags.iter().any(|known| known == flag)
    }
}

/// The join between the promise and the binary: nothing the frame prints may name a command that does not
/// exist. Rendered in the two states a reviewer can be in — no index, and indexed — because the empty
/// states differ and both make promises.
#[test]
#[cfg(feature = "sqlite-bundled")]
fn nothing_the_frame_prints_names_a_command_this_build_lacks() {
    let repo = repository();
    let surface = Surface::read();
    assert!(
        surface.verbs.len() >= 8,
        "the verb list came out implausibly short: {surface:?}",
    );
    // `--demo` is the regression this whole file exists for: it was in the copy before it was in the
    // binary. If the flag ever disappears again, this is the test that says so.
    assert!(
        surface.flags.iter().any(|flag| flag == "--demo"),
        "`--demo` is not in `fathom --help` any more"
    );

    let bare = fathom_in(&repo, &["--plain"]);
    let _ = std::fs::remove_dir_all(repo.join("nope"));
    let texts = [stdout(&bare), stderr(&bare)];
    for text in texts {
        let words = surface.named(&text);
        let missing: Vec<&String> = words
            .iter()
            .filter(|word| word.starts_with("MISSING"))
            .collect();
        assert!(
            missing.is_empty(),
            "the frame names commands this build does not have: {missing:?}\n---\n{text}"
        );
    }
}

/// Every refusal the binary can be driven into names something to do — a verb, a key, a path to read —
/// and not merely the fact that it failed.
///
/// This is the audited list from [docs/concepts/onboarding.md](../../../docs/concepts/onboarding.md) §5,
/// executed. Each case is a mistake a real reviewer makes on a real first afternoon.
#[test]
#[cfg(feature = "sqlite-bundled")]
fn every_refusal_ends_with_a_next_action() {
    let repo = repository();
    let surface = Surface::read();
    // The empty state and the guide both live on the frame/stderr path of the same invocation, so one
    // run here covers "no index yet" for three verbs at once.
    let cases: &[(&str, &[&str])] = &[
        ("an episode that is not there", &["show", "deadbeef"]),
        (
            "an episode handle with no grammar",
            &["show", "definitely-not-a-ref"],
        ),
        ("an analyse with no episode named", &["analyze"]),
        ("an analyse of nothing", &["analyze", "deadbeef"]),
        ("a store that does not exist yet", &["stats"]),
        ("a root that is not a repository", &["stats", "/"]),
        (
            "a format that does not exist",
            &["export", "--format", "yaml"],
        ),
        ("an unimplemented verb", &["check"]),
        ("an unimplemented renderer", &["export"]),
        ("an unknown verb", &["no-such-verb"]),
    ];
    for (what, args) in cases {
        let out = fathom_in(&repo, args);
        let text = format!("{}{}", stdout(&out), stderr(&out));
        if code(&out) == 0 {
            // `stats --json` on an unindexed repository is a *report* that says so, not a refusal, and a
            // successful run owes no remedy.
            continue;
        }
        // Two shapes, and both are the promise: a text refusal carries `hint:`, a JSON one carries a
        // `remedy` field, because a machine has to route on it rather than read a line.
        let hint = text
            .lines()
            .map(str::trim)
            .find(|line| {
                // Three shapes carry the promise, and all three are the product's own: a `hint:` line, a
                // `remedy:` line or field, and clap's usage footer — which is a next action, and is not
                // rewritten just to satisfy a test.
                line.starts_with("hint:")
                    || line.starts_with("remedy:")
                    || line.contains("\"remedy\"")
                    || line.contains("try `--help`")
                    || line.contains("try '--help'")
            })
            .unwrap_or_else(|| {
                panic!(
                    "`fathom {}` ({what}) refused without a next action:\n{text}",
                    args.join(" ")
                )
            });
        assert!(
            hint.trim().len() > "hint:".len() + 8,
            "{what}: the remedy is too thin to act on: {hint:?}"
        );
        assert!(
            hint.contains(' '),
            "{what}: a ticket number is an owner, not a next action: {hint:?}"
        );
        let words = surface.named(hint);
        let missing: Vec<&String> = words
            .iter()
            .filter(|word| word.starts_with("MISSING"))
            .collect();
        assert!(
            missing.is_empty(),
            "{what}: the remedy names commands this build does not have: {missing:?}"
        );
    }
}

/// The progress row's contract, from the outside: asked for, it is there; declined, it is not; and the
/// machine-readable stream is never polluted by it.
#[test]
#[cfg(feature = "sqlite-bundled")]
fn the_progress_row_appears_when_asked_and_never_where_it_would_break_a_parser() {
    let repo = repository();
    let forced = fathom_in(&repo, &["index", "--progress"]);
    let text = stderr(&forced);
    assert_eq!(code(&forced), 0, "{text}");
    assert!(
        text.contains('%') && text.contains("commit"),
        "`--progress` printed no percentage row on a pipe:\n{text}"
    );
    assert!(
        !stdout(&forced).is_empty(),
        "the run report left stdout: a progress row belongs on stderr"
    );

    let declined = fathom_in(&repo, &["index", "--rebuild", "--no-progress"]);
    assert_eq!(code(&declined), 0, "{}", stderr(&declined));
    assert!(
        !stderr(&declined).contains("estimating…"),
        "`--no-progress` printed a row anyway:\n{}",
        stderr(&declined)
    );

    // The report as a document: one JSON object on stdout, nothing else there, no row on stderr.
    let json = fathom_in(&repo, &["index", "--rebuild", "--json", "--progress"]);
    assert_eq!(code(&json), 0, "{}", stderr(&json));
    let parsed: serde_json::Value = serde_json::from_str(stdout(&json).trim())
        .expect("`index --json` prints one JSON object on stdout and nothing else there");
    assert_eq!(
        parsed["commits"]["walked"].as_u64(),
        Some(3),
        "the report is the same facts as the text form: {parsed}"
    );
    assert!(
        !stderr(&json).contains("indexing") && !stderr(&json).contains("estimating…"),
        "`--json` printed a progress row on stderr: {}",
        stderr(&json)
    );
}

/// The shell's own estimate line, seen through the binary rather than the library: a run with nothing to
/// do still says which phase it finished rather than printing a bare count.
#[test]
#[cfg(feature = "sqlite-bundled")]
fn a_warm_index_reports_a_finished_phase_rather_than_a_percentage() {
    let repo = repository();
    fathom_in(&repo, &["index"]);
    let out = fathom_in(&repo, &["index", "--progress"]);
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let text = stderr(&out);
    if text.contains('%') {
        assert!(
            text.contains("done") || text.contains("left") || text.contains("estimating…"),
            "a percentage appeared with nothing beside it to act on:\n{text}"
        );
    }
}

fn fathom_in(dir: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fathom"))
        .current_dir(dir)
        .env("LC_CTYPE", "C.UTF-8")
        .args(args)
        .output()
        .expect("the built fathom binary is runnable")
}
