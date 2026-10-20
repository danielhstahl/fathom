//! OSC-8 hyperlinks: the emission rule, the probe, and the two URL builders (`e5.8`, `reco-dmw`).
//!
//! The acceptance criterion this file exists for is one line of the ticket: **"OSC-8 links are emitted
//! only when the terminal advertises support."** The other half of the ticket's promise — that the
//! absence of a feature is a normal, well-explained path — is checked in the same place, because the
//! reason a link was *not* emitted is the thing a reviewer has to be told.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_core::links::{
    Hyperlinks, Route, advertise, commit_url, file_url, middleman, osc8, strip,
};

/// A terminal that answers none of the questions, which is what a bare `xterm` and a CI runner look like.
const NOTHING: &[(&str, &str)] = &[("TERM", "xterm-256color")];

fn probe(vars: &[(&str, &str)]) -> Hyperlinks {
    let map: std::collections::BTreeMap<String, String> = vars
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect();
    Hyperlinks::from_env(|key| map.get(key).cloned())
}

fn look(vars: &[(&str, &str)]) -> impl FnMut(&str) -> Option<String> {
    let map: std::collections::BTreeMap<String, String> = vars
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect();
    move |key: &str| map.get(key).cloned()
}

// --- the emission rule ------------------------------------------------------------------------

#[test]
fn an_unadvertised_terminal_gets_the_plain_text_and_a_sentence_about_why() {
    let links = probe(NOTHING);
    assert!(!links.enabled(), "{}", links.why());
    assert_eq!(links.wrap("file:///x", "src/x.rs"), "src/x.rs");
    assert!(
        links.why().contains("nothing advertised"),
        "{}",
        links.why()
    );
    assert!(
        links.why().contains("FATHOM_HYPERLINKS"),
        "the refusal has to name the knob: {}",
        links.why()
    );
}

#[test]
fn every_terminal_that_names_itself_is_believed() {
    for (key, value, want) in [
        ("TERM_PROGRAM", "WezTerm", "WezTerm"),
        ("TERM_PROGRAM", "ghostty", "ghostty"),
        ("TERM_PROGRAM", "iTerm.app", "iTerm2"),
        ("TERM_PROGRAM", "vscode", "VS Code"),
        ("TERM_PROGRAM", "contour", "contour"),
        ("KITTY_WINDOW_ID", "1", "kitty"),
        ("ALACRITTY_WINDOW_ID", "1", "Alacritty"),
        ("WT_SESSION", "abc", "Windows Terminal"),
        ("VTE_VERSION", "7602", "VTE"),
    ] {
        let links = probe(&[(key, value)]);
        assert!(
            links.enabled(),
            "{key}={value} should be an advertisement: {}",
            links.why()
        );
        assert!(links.why().contains(want), "{}", links.why());
        assert!(links.wrap("file:///a", "a").starts_with('\u{1b}'));
        assert_eq!(
            strip(&links.wrap("file:///a", "a")),
            "a",
            "the text survives"
        );
    }
}

#[test]
fn a_version_that_is_too_old_is_not_an_advertisement() {
    // iTerm2 2.9 and VS Code 1.40 do not render OSC-8; saying "advertised" there would be the claim
    // this ticket is written to avoid making.
    for (program, version) in [
        ("iTerm.app", "2.9.5"),
        ("vscode", "1.40.2"),
        ("org.kde.konsole", "21.08"),
    ] {
        let links = probe(&[("TERM_PROGRAM", program), ("TERM_PROGRAM_VERSION", version)]);
        assert!(
            !links.enabled(),
            "{program} {version} predates OSC-8 and got: {}",
            links.why()
        );
    }
    for (program, version) in [
        ("iTerm.app", "3.5.3"),
        ("vscode", "1.95.2"),
        ("org.kde.konsole", "24.02"),
    ] {
        let links = probe(&[("TERM_PROGRAM", program), ("TERM_PROGRAM_VERSION", version)]);
        assert!(
            links.enabled(),
            "{program} {version} should link: {}",
            links.why()
        );
    }
}

#[test]
fn a_named_terminal_that_gives_no_version_is_believed_saying_it_was_assumed() {
    let links = probe(&[("TERM_PROGRAM", "iTerm.app")]);
    assert!(links.enabled(), "{}", links.why());
    assert!(links.why().contains("assumed"), "{}", links.why());
    assert!(advertise(&mut look(&[("TERM_PROGRAM", "iTerm.app")])).is_some());
}

#[test]
fn vte_is_compared_as_a_number_not_a_name() {
    // 3600 is 0.36, the first VTE with OSC-8. `36` and `7602` are both plausible real values.
    assert!(probe(&[("VTE_VERSION", "3600")]).enabled());
    assert!(probe(&[("VTE_VERSION", "7602")]).enabled());
    assert!(!probe(&[("VTE_VERSION", "3409")]).enabled());
    assert!(!probe(&[("VTE_VERSION", "not-a-number")]).enabled());
    assert!(middleman(&mut look(&[])).is_none());
}

// --- the middleman ----------------------------------------------------------------------------

#[test]
fn tmux_gets_a_say_before_anything_is_emitted() {
    // tmux 3.3 (2021) is the first that forwards hyperlinks, and an old one turns a link into litter.
    let old = probe(&[
        ("TERM_PROGRAM", "WezTerm"),
        ("TMUX", "/tmp/tmux-1000/default,1,0"),
        ("TMUX_VERSION", "3.2a"),
    ]);
    assert!(!old.enabled(), "{}", old.why());
    assert!(old.why().contains("3.2a"), "{}", old.why());

    let new = probe(&[
        ("TERM_PROGRAM", "WezTerm"),
        ("TMUX", "/tmp/tmux-1000/default,1,0"),
        ("TMUX_VERSION", "3.4"),
    ]);
    assert!(new.enabled(), "{}", new.why());
    assert!(new.why().contains("forwards"), "{}", new.why());

    // No version answered is not a guess: unknown is a value (ADR-006 §0.1), and the remedy is stated.
    let unknown = probe(&[
        ("TERM_PROGRAM", "WezTerm"),
        ("TMUX", "/tmp/tmux-1000/default,1,0"),
    ]);
    assert!(!unknown.enabled(), "{}", unknown.why());
    assert!(
        unknown.why().contains("did not say its version"),
        "{}",
        unknown.why()
    );

    assert_eq!(
        middleman(&mut look(&[("STY", "1234.p0")])),
        Some(Route::Drops(
            "GNU screen (STY) does not pass OSC-8 through".to_owned()
        ))
    );
    assert!(!probe(&[("KITTY_WINDOW_ID", "1"), ("STY", "1.p0")]).enabled());
}

// --- the override ------------------------------------------------------------------------------

#[test]
fn the_knob_overrides_the_probe_in_both_directions() {
    let forced_on = probe(&[("FATHOM_HYPERLINKS", "always"), ("TERM", "dumb")]);
    assert!(forced_on.enabled(), "{}", forced_on.why());
    assert!(
        forced_on.why().contains("FATHOM_HYPERLINKS=always"),
        "{}",
        forced_on.why()
    );

    let forced_off = probe(&[("FATHOM_HYPERLINKS", "never"), ("TERM_PROGRAM", "ghostty")]);
    assert!(!forced_off.enabled(), "{}", forced_off.why());
    assert_eq!(forced_off.wrap("file:///a", "a"), "a");

    // `auto` is the same as saying nothing, and an unrecognised value is reported as unrecognised
    // rather than quietly read as `auto`.
    assert_eq!(
        probe(&[("FATHOM_HYPERLINKS", "auto"), ("TERM_PROGRAM", "ghostty")]).enabled(),
        probe(&[("TERM_PROGRAM", "ghostty")]).enabled()
    );
    let confused = probe(&[
        ("FATHOM_HYPERLINKS", "yes-please"),
        ("TERM_PROGRAM", "ghostty"),
    ]);
    assert!(
        confused.why().contains("not `always`, `never` or `auto`"),
        "{}",
        confused.why()
    );
    assert!(
        confused.enabled(),
        "auto is the fallthrough, and ghostty still advertised"
    );
}

#[test]
fn an_empty_variable_is_not_an_answer() {
    // Terminals set these to "" rather than unsetting them sometimes, and "" is not an advertisement.
    assert!(!probe(&[("TERM_PROGRAM", ""), ("VTE_VERSION", ""), ("TMUX", "")]).enabled());
}

// --- the escape and the URLs -------------------------------------------------------------------

#[test]
fn the_escape_carries_the_uri_and_leaves_the_text_alone() {
    let linked = osc8("file:///repo/src/store.rs", "src/store.rs");
    assert_eq!(
        linked,
        "\u{1b}]8;;file:///repo/src/store.rs\u{1b}\\src/store.rs\u{1b}]8;;\u{1b}\\"
    );
    assert_eq!(strip(&linked), "src/store.rs");
    assert_eq!(strip("plain"), "plain");
    // An unterminated sequence must not panic or swallow the line silently.
    assert_eq!(strip("a\u{1b}]8;;x"), "a");
}

#[test]
fn file_urls_are_absolute_only_and_take_a_line() {
    assert_eq!(
        file_url("/repo/src/store.rs", Some(42)).as_deref(),
        Some("file:///repo/src/store.rs#L42")
    );
    assert_eq!(
        file_url("/repo/src/store.rs", None).as_deref(),
        Some("file:///repo/src/store.rs")
    );
    assert_eq!(
        file_url("/repo/x", Some(0)).as_deref(),
        Some("file:///repo/x")
    );
    // A relative path is refused, because its target depends on a cwd the terminal may change.
    assert_eq!(file_url("src/store.rs", Some(1)), None);
    assert_eq!(file_url("", None), None);
    // The characters a real repository path contains.
    assert_eq!(
        file_url("/repo/my dir/ô.rs", None).as_deref(),
        Some("file:///repo/my%20dir/%C3%B4.rs")
    );
    // Windows: the third slash is part of the form, not a typo.
    assert_eq!(
        file_url(r"C:/repo/src/x.rs", Some(7)).as_deref(),
        Some("file:///C:/repo/src/x.rs#L7")
    );
}

#[test]
fn commit_urls_follow_whichever_forge_the_remote_names() {
    let oid = "0123456789abcdef0123456789abcdef01234567";
    let cases = [
        (
            "git@github.com:org/repo.git",
            format!("https://github.com/org/repo/commit/{oid}"),
        ),
        (
            "https://github.com/org/repo.git",
            format!("https://github.com/org/repo/commit/{oid}"),
        ),
        (
            "ssh://git@gitlab.example.org/team/repo.git",
            format!("https://gitlab.example.org/team/repo/-/commit/{oid}"),
        ),
        (
            "https://bitbucket.org/org/repo.git",
            format!("https://bitbucket.org/org/repo/commits/{oid}"),
        ),
        (
            "git://gitea.com/org/repo",
            format!("https://gitea.com/org/repo/commit/{oid}"),
        ),
        (
            "https://user:token@github.com/org/repo.git",
            "https://github.com/org/repo/commit/0123456789abcdef0123456789abcdef01234567"
                .to_string(),
        ),
    ];
    for (remote, want) in cases {
        assert_eq!(
            commit_url(remote, oid).as_deref(),
            Some(want.as_str()),
            "{remote}"
        );
    }
    // A credential in a remote URL is not copied into a URL a terminal may log.
    let leaked = commit_url("https://user:token@github.com/org/repo.git", oid).unwrap();
    assert!(!leaked.contains("token"), "{leaked}");
    // A local or pathless remote has no web UI to point at, and says so by declining.
    for remote in ["file:///srv/git/repo", "/srv/git/repo", "origin", ""] {
        assert_eq!(commit_url(remote, oid), None, "{remote}");
    }
}

#[test]
fn callers_can_build_a_link_or_a_plain_string_with_one_call() {
    let on = Hyperlinks::always("test");
    let off = Hyperlinks::never("test");
    assert_eq!(on.wrap("file:///a", "a"), osc8("file:///a", "a"));
    assert_eq!(off.wrap("file:///a", "a"), "a");
    // An empty URI is never a link, not even on a terminal that asked for links: an OSC-8 with no
    // target is how a terminal draws an underline under nothing.
    assert_eq!(on.wrap("", "a"), "a");
}

// --- the stream, which is a different question from the terminal -------------------------------

#[test]
fn a_pipe_inside_an_advertising_terminal_still_gets_plain_text() {
    // The environment describes the terminal a session has; the caller says whether its own bytes are
    // going to it. `fathom show | fzf`, `fathom show > notes.md` and a CI runner's log each sit inside
    // a terminal that advertises, and in each of them an introducer becomes somebody else's payload
    // (ADR-006 §6.7).
    let links = Hyperlinks::from_stream(false, look(&[("TERM_PROGRAM", "ghostty")]));
    assert!(!links.enabled(), "{}", links.why());
    assert_eq!(links.wrap("file:///x", "src/x.rs"), "src/x.rs");
    // Both facts in the sentence, because the reviewer's next question is which of them failed.
    assert!(links.why().contains("not a terminal"), "{}", links.why());
    assert!(links.why().contains("advertises OSC-8"), "{}", links.why());
    assert!(
        links.advertised(),
        "the terminal's own answer is still reported, so the report is not misleading"
    );

    // The same environment, on a stream that is a terminal, emits.
    let tty = Hyperlinks::from_stream(true, look(&[("TERM_PROGRAM", "ghostty")]));
    assert!(tty.enabled(), "{}", tty.why());
    // ...and that is the whole of what the no-argument constructor means.
    assert_eq!(
        tty.why(),
        Hyperlinks::from_env(look(&[("TERM_PROGRAM", "ghostty")])).why()
    );
}

#[test]
fn an_explicit_override_outranks_the_stream_as_well_as_the_terminal() {
    // `FATHOM_HYPERLINKS=always` is a reviewer saying they know where the bytes are going — which
    // includes a pipe they are reading, and includes `fathom show | fzf` where the reader understands
    // the sequences. `never` closes the door from the other side.
    let forced = Hyperlinks::from_stream(
        false,
        look(&[("TERM_PROGRAM", "ghostty"), ("FATHOM_HYPERLINKS", "always")]),
    );
    assert!(forced.enabled(), "{}", forced.why());
    assert!(forced.forced());
    assert_eq!(
        forced.wrap("file:///x", "src/x.rs"),
        osc8("file:///x", "src/x.rs")
    );

    let refused = Hyperlinks::from_stream(
        true,
        look(&[("TERM_PROGRAM", "ghostty"), ("FATHOM_HYPERLINKS", "never")]),
    );
    assert!(!refused.enabled(), "{}", refused.why());
    assert!(refused.forced());
    assert!(refused.wrap("file:///x", "src/x.rs") == "src/x.rs");
}
