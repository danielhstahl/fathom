//! The editor and difftool plans, as a matrix over the three OSes (`e5.8`, `reco-dmw`).
//!
//! The ticket's first acceptance criterion is "works with vim, neovim, helix and code on all three
//! OSes", with the parenthetical that a matrix test runs where CI allows and the gap is documented.
//! Here there is no gap: [`Os`] is a parameter rather than a `cfg`, so every cell below is executed on
//! the machine running the test — Linux, macOS and Windows in one `cargo test`, which is three CI
//! runners' coverage for one process. The cells that *cannot* be run here are named at the bottom of
//! this file rather than left unmentioned.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_core::handoff::{
    DIFFTOOLS, Difftool, EDITOR_KEYS, Editor, Env, Os, difftool_plan, editor_plan,
};

/// The four editors the ticket names, with the line argument each one documents.
const EDITORS: &[(&str, &str)] = &[
    // The vi family takes `+LINE` before the path.
    ("vim", "+42 src/store.rs"),
    ("nvim", "+42 src/store.rs"),
    ("hx", "src/store.rs:42"),
    ("code", "--goto src/store.rs:42"),
    // ...and the ones a reviewer's `$EDITOR` plausibly already is.
    ("zed", "src/store.rs:42"),
    ("idea", "--line 42 src/store.rs"),
    ("emacs", "+42 src/store.rs"),
];

const PLATFORMS: &[Os] = &[Os::Linux, Os::MacOs, Os::Windows, Os::Other];

#[test]
fn the_four_editors_the_ticket_names_work_on_every_platform_the_same_way() {
    for os in PLATFORMS {
        for (editor, want) in EDITORS {
            let env = Env::fixed(&[("EDITOR", editor)], &[editor]);
            let line = match editor_plan(*os, "src/store.rs", 42, None, &env) {
                Editor::Open(command) => {
                    assert_eq!(command.program, *editor, "{os:?} {editor}");
                    command.line()
                }
                Editor::Unavailable { why } => {
                    panic!("{os:?} should open {editor}, and said instead: {why}")
                }
            };
            assert_eq!(line, format!("{editor} {want}"), "{os:?} · {editor}");
        }
    }
}

#[test]
fn the_plan_says_where_the_editor_came_from_and_which_line_syntax_it_took() {
    // "Detect missing EDITOR gracefully and say what it decided to use" is two claims: the sentence
    // names the source, and it names the argument shape. A toast that said only "opening" could not be
    // distinguished from one that opened at the wrong line.
    let env = Env::fixed(&[("VISUAL", "nvim")], &["nvim"]);
    let Editor::Open(command) = editor_plan(Os::Linux, "a.rs", 7, None, &env) else {
        panic!("nvim on PATH must open");
    };
    assert!(command.why.contains("$VISUAL"), "{}", command.why);
    assert!(command.why.contains("+7"), "{}", command.why);

    let env = Env::fixed(&[], &["vi"]);
    let Editor::Open(command) = editor_plan(Os::Linux, "a.rs", 7, None, &env) else {
        panic!("the platform default must be planned, not refused");
    };
    assert!(
        command.why.contains("default") && command.why.contains("vi"),
        "a default editor has to say it is a default: {}",
        command.why
    );
}

#[test]
fn fathom_editor_overrides_the_shells_editor_without_overriding_the_shell() {
    let env = Env::fixed(
        &[
            ("FATHOM_EDITOR", "hx"),
            ("VISUAL", "code"),
            ("EDITOR", "vim"),
        ],
        &["hx", "code", "vim"],
    );
    let Editor::Open(command) = editor_plan(Os::Linux, "a.rs", 3, None, &env) else {
        panic!("hx is on PATH");
    };
    assert_eq!(command.program, "hx");
    assert_eq!(command.args, vec!["a.rs:3".to_owned()]);
    assert!(command.why.contains("$FATHOM_EDITOR"), "{}", command.why);
    // And the precedence table is the documented one, in order.
    assert_eq!(EDITOR_KEYS, ["FATHOM_EDITOR", "VISUAL", "EDITOR"]);
}

#[test]
fn an_editor_named_but_not_installed_refuses_with_the_program_it_looked_for() {
    let env = Env::fixed(&[("EDITOR", "helix-not-here")], &[]);
    match editor_plan(Os::MacOs, "a.rs", 1, None, &env) {
        Editor::Unavailable { why } => {
            assert!(why.contains("helix-not-here"), "{why}");
            assert!(why.contains("not on PATH"), "{why}");
        }
        Editor::Open(command) => panic!("nothing is on PATH here, and it planned {command}"),
    }
}

#[test]
fn an_editor_with_no_line_argument_still_opens_and_says_the_line_is_elsewhere() {
    let env = Env::fixed(&[("EDITOR", "smarcedit")], &["smarcedit"]);
    let Editor::Open(command) = editor_plan(Os::Linux, "a.rs", 99, None, &env) else {
        panic!("smarcedit is on PATH");
    };
    assert_eq!(command.args, vec!["a.rs".to_owned()]);
    assert!(
        command.why.contains("no documented line argument"),
        "{}",
        command.why
    );
    assert!(command.why.contains("99"), "{}", command.why);
}

#[test]
fn the_arguments_survive_a_path_with_a_space_in_it() {
    // `Command::line()` is a display form, not a shell command, and a path that would be two words if
    // typed has to look like one word when it is printed.
    let env = Env::fixed(&[("EDITOR", "code")], &["code"]);
    let Editor::Open(command) = editor_plan(Os::Windows, r"C:\my repo\a.rs", 12, None, &env) else {
        panic!("code is on PATH");
    };
    assert_eq!(command.args[1], r"C:\my repo\a.rs:12");
    assert!(
        command.line().contains("\"C:\\my repo\\a.rs:12\""),
        "{}",
        command.line()
    );
}

// --- the difftool handoff ---------------------------------------------------------------------

/// Write the two sides somewhere; the planner only names paths.
const LEFT: &str = "/tmp/fathom-a/store.rs";
const RIGHT: &str = "/tmp/fathom-b/store.rs";

#[test]
fn a_configured_difftool_is_asked_in_its_own_argument_shape() {
    let cases: &[(&str, &str, &[&str])] = &[
        ("difftastic", "difft", &[]),
        ("meld", "meld", &[]),
        ("kdiff3", "kdiff3", &[]),
        ("nvim", "nvim", &["-d"]),
        ("vim", "vim", &["-d"]),
        ("vscode", "code", &["--diff"]),
        ("winmerge", "WinMergeU", &["/e"]),
    ];
    for (name, program, prefix) in cases {
        let env = Env::fixed(&[], &[program]);
        let Difftool::Open(command) = difftool_plan(Os::Linux, Some(name), LEFT, RIGHT, &env)
        else {
            panic!("{name} is on PATH and should have been planned");
        };
        let mut want: Vec<&str> = prefix.to_vec();
        want.push(LEFT);
        want.push(RIGHT);
        assert_eq!(command.program, *program, "{name}");
        assert_eq!(command.args, want, "{name}");
        assert!(command.why.contains(name), "{}", command.why);
    }
}

#[test]
fn a_configured_tool_that_is_absent_falls_back_and_says_that_it_did() {
    // The name in the config and the binary on PATH are two different strings for difftastic, and a
    // fallback that blames "difftastic" for a missing `difft` is a sentence with the wrong noun in it.
    // More important: a substitution that is not announced is a reviewer reading a diff they did not
    // ask for, so the plan says which tool was wanted and which one is being used.
    let env = Env::fixed(&[], &["meld"]);
    let Difftool::Open(command) = difftool_plan(Os::Linux, Some("difftastic"), LEFT, RIGHT, &env)
    else {
        panic!("meld is the fallback and it is on PATH");
    };
    assert_eq!(command.program, "meld");
    assert!(command.why.contains("difftastic"), "{}", command.why);
    assert!(command.why.contains("instead"), "{}", command.why);

    // And when the requested tool *is* there, nothing is substituted.
    let env = Env::fixed(&[], &["difft", "meld"]);
    let Difftool::Open(command) = difftool_plan(Os::Linux, Some("difftastic"), LEFT, RIGHT, &env)
    else {
        panic!("difft is on PATH");
    };
    assert_eq!(command.program, "difft");
    assert!(!command.why.contains("instead"), "{}", command.why);
}

#[test]
fn an_uninstalled_tool_says_so_and_names_the_key_that_changes_it() {
    let env = Env::fixed(&[], &[]);
    match difftool_plan(Os::Linux, Some("meld"), LEFT, RIGHT, &env) {
        Difftool::Unavailable { why, tried } => {
            assert!(why.contains("meld"), "{why}");
            assert!(why.contains("not on PATH"), "{why}");
            assert!(why.contains("FATHOM_DIFFTOOL"), "{why}");
            assert!(tried.starts_with(&["meld".to_owned()]), "{tried:?}");
        }
        Difftool::Open(command) => panic!("nothing is installed, and it planned {command}"),
    }
}

#[test]
fn with_nothing_configured_the_first_candidate_on_path_wins() {
    let env = Env::fixed(&[], &["meld"]);
    let Difftool::Open(command) = difftool_plan(Os::Linux, None, LEFT, RIGHT, &env) else {
        panic!("meld is a candidate and it is on PATH");
    };
    assert_eq!(command.program, "meld");
    assert!(
        command.why.contains("nothing was configured"),
        "{}",
        command.why
    );
    // The order is the table's, and difftastic is first because it is the only one of them built for a
    // terminal — which is what a reviewer who has installed two of them wants by default.
    assert_eq!(Os::Linux.difftool_candidates()[0].0, "difftastic");
    assert_eq!(Os::Linux.difftool_candidates()[0].1, "difft");
}

#[test]
fn no_difftool_at_all_is_a_normal_path_with_a_remedy_in_it() {
    let env = Env::fixed(&[], &[]);
    let Difftool::Unavailable { why, tried } = difftool_plan(Os::MacOs, None, LEFT, RIGHT, &env)
    else {
        panic!("a machine with no difftool must not plan a command");
    };
    assert!(!why.is_empty());
    assert!(!tried.is_empty(), "the refusal has to list what it tried");
    for wanted in Os::MacOs.difftool_candidates() {
        assert!(why.contains(wanted.0), "{why} should mention {}", wanted.0);
    }
    assert!(
        why.contains("fathom analyze"),
        "{why} should name the path that needs nothing installed"
    );
}

#[test]
fn an_unknown_tool_name_runs_as_typed_with_the_two_files_last() {
    // The table is closed; the world of diff tools is not. A reviewer with `svdiff` installed is one
    // `$FATHOM_DIFFTOOL=svdiff` away from using it, without this crate having heard of it.
    let env = Env::fixed(&[], &["svdiff"]);
    let Difftool::Open(command) = difftool_plan(Os::Linux, Some("svdiff"), LEFT, RIGHT, &env)
    else {
        panic!("svdiff is on PATH");
    };
    assert_eq!(command.program, "svdiff");
    assert_eq!(command.args, vec![LEFT.to_owned(), RIGHT.to_owned()]);
    assert!(command.why.contains("not a known name"), "{}", command.why);
}

#[test]
fn a_tool_may_be_named_by_its_program_or_its_config_name() {
    for (name, program, _prefix) in DIFFTOOLS {
        let env = Env::fixed(&[], &[program]);
        let planned = difftool_plan(Os::Linux, Some(name), LEFT, RIGHT, &env);
        assert!(
            matches!(planned, Difftool::Open(_)),
            "{name} should plan with {program} on PATH"
        );
        // The `.exe` a Windows shim carries is not part of the name.
        let env = Env::fixed(&[], &["code"]);
        assert!(matches!(
            difftool_plan(Os::Windows, Some("code.exe"), LEFT, RIGHT, &env),
            Difftool::Open(_)
        ));
    }
}

// --- what a matrix on one machine cannot cover ------------------------------------------------
//
// Everything above runs on Linux, macOS and Windows from one `cargo test`, because nothing here reads
// a `cfg` — but three things are outside a unit test's reach and are stated rather than skipped:
//
// 1. **Does the program exist on this machine.** `Env::runnable` is the answer to that question, and a
//    unit test supplies it. The real `PATH` walk lives in `crates/fathom-cli/src/handoff.rs` and is
//    covered there, including the `.cmd`/`.bat` shim dance `code` needs on Windows.
// 2. **Does the tool do what its documentation says.** This table encodes each tool's published
//    argument shape; the matrix proves Fathom passes those arguments and cannot prove the tool honours
//    them. `crates/fathom-cli/tests/handoff_verb.rs` runs a real program end to end instead.
// 3. **Windows-only GUI tools.** `WinMergeU` is in the table and in the matrix, and the matrix cannot
//    launch it on a Linux runner. It is a plan, a printable line, and one `--print` away from being
//    checked on the platform that has it.

// --- what the reviewer asked for, as opposed to what the environment said ----------------------

#[test]
fn a_spec_asked_for_is_described_as_asked_for_rather_than_as_a_variable() {
    // `--editor` is not `$EDITOR`, and a plan that says otherwise starts a bug report that cannot be
    // finished. The requested spec wins over the environment, because the command line is the newest
    // thing the reviewer typed.
    let env = Env::fixed(
        &[("EDITOR", "nano"), ("FATHOM_EDITOR", "nano")],
        &["nano", "nvim"],
    );
    let Editor::Open(command) = editor_plan(Os::Linux, "src/store.rs", 12, Some("nvim"), &env)
    else {
        panic!("`nvim` was asked for and is runnable");
    };
    assert_eq!(command.program, "nvim");
    assert_eq!(command.line(), "nvim +12 src/store.rs");
    assert!(
        command.why.contains("`nvim` was asked for"),
        "{}",
        command.why
    );
    assert!(
        !command.why.contains("$EDITOR"),
        "the environment did not decide this one:\n{}",
        command.why
    );

    // ...and with nothing asked for, the variable is named as the source again.
    let Editor::Open(command) = editor_plan(Os::Linux, "src/store.rs", 12, None, &env) else {
        panic!("`nano` is on PATH in this environment");
    };
    assert!(
        command.why.contains("from $FATHOM_EDITOR"),
        "{}",
        command.why
    );
}

#[test]
fn a_difftool_named_with_its_own_arguments_keeps_them_and_takes_the_files_last() {
    // `--tool "mytool --wait"` is the only way to drive a tool this table has never heard of. The
    // program is the first word, its own arguments follow, and the two files go last because that is
    // the shape every remaining diff tool wants.
    let env = Env::fixed(&[], &["mytool"]);
    let Difftool::Open(command) = difftool_plan(
        Os::Linux,
        Some("mytool --wait"),
        "/tmp/a/before-x.rs",
        "/tmp/a/after-x.rs",
        &env,
    ) else {
        panic!("`mytool` is on PATH");
    };
    assert_eq!(command.program, "mytool");
    assert_eq!(
        command.args,
        vec![
            "--wait".to_owned(),
            "/tmp/a/before-x.rs".to_owned(),
            "/tmp/a/after-x.rs".to_owned()
        ],
        "its own argument first, then the two files"
    );
    assert!(
        command.why.contains("its own arguments first"),
        "{}",
        command.why
    );
}

#[test]
fn a_known_tool_name_keeps_its_table_shape_whatever_is_typed_after_it() {
    // `--tool "vscode -d"` would ask VS Code to open a folder rather than diff two files. A name this
    // table knows is driven the way its documentation says, and the stray arguments are not used — so
    // the plan says which tool it resolved the name to, and the reviewer can see the substitution.
    let env = Env::fixed(&[], &["code"]);
    let Difftool::Open(command) = difftool_plan(
        Os::Linux,
        Some("vscode -d"),
        "/tmp/before.rs",
        "/tmp/after.rs",
        &env,
    ) else {
        panic!("`code` is on PATH");
    };
    assert_eq!(command.program, "code");
    assert_eq!(
        command.args,
        vec![
            "--diff".to_owned(),
            "/tmp/before.rs".to_owned(),
            "/tmp/after.rs".to_owned(),
        ],
        "the table's argument shape, with the files where the table says they go"
    );
    assert!(
        command.why.contains("`code` is on PATH"),
        "the plan names the program it resolved the request to:\n{}",
        command.why
    );
}
