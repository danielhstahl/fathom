//! The two empty states, pinned to the character (`reco-1nu`).
//!
//! The bug was one sentence standing in for two facts: a repository nobody had indexed, and a
//! repository whose index held 500 commits and reported epoch 1 ready, were both told *run `fathom
//! index`*. The first is a remedy. The second is a loop — the reviewer ran it, reopened the shell, and
//! was told the same thing — and the same frame offered `fathom --demo`, a flag no binary has ever had.
//!
//! So the strings are pinned here (both states, at the library boundary), in the goldens
//! (`tests/golden/timeline/nothing_loaded.txt` and `index_ready.txt`) for what a pane draws, and from
//! the outside in `crates/fathom-cli/tests/cli.rs`, which runs the binary against a repository it
//! indexed itself. A document can say *never name a command the binary does not have*; only a test can
//! keep that true.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_tui::index::{Facts, Read};

/// State one: no index at all. The remedy is `fathom index`, and it works.
#[test]
fn no_index_prints_the_verb_that_works() {
    assert_eq!(
        Facts::absent().empty_words(),
        "no index here yet — run `fathom index` here, then reopen fathom"
    );
}

/// State two: the index is finished, and nothing is loaded because the episode read is not in this
/// build. Every clause is doing work: the numbers are the caller's, the owner is named, and the one
/// verb that does print an episode today is offered in place of the command that would do nothing.
#[test]
fn a_finished_index_prints_what_it_holds_instead_of_an_index_command() {
    let facts = Facts::present(Some(1), Some("ready".to_owned()), Some(500), Some(412));
    assert_eq!(
        facts.empty_words(),
        "index epoch 1 ready · 500 commits · 412 eps — the episode read is e2.11's, not in this \
         build; nothing in this build can list them, and `fathom show <episode>` prints one \
         headlessly today"
    );
    // The loop this ticket exists to break: the index is done, so do not tell anyone to run it.
    assert!(
        !facts.empty_words().contains("run `fathom index`"),
        "a finished index was sent back to `fathom index`: {}",
        facts.empty_words()
    );
    assert!(
        !facts.summary().contains("no episodes"),
        "the index holds 412 of them: {}",
        facts.summary()
    );
}

/// The two states are different sentences, not one sentence with a different tail.
#[test]
fn the_two_states_do_not_collapse_into_one_sentence() {
    let bare = Facts::absent();
    let indexed = Facts::present(Some(1), Some("ready".to_owned()), Some(500), Some(412));
    assert_ne!(bare.summary(), indexed.summary());
    assert_ne!(bare.remedy(), indexed.remedy());
    assert_ne!(bare.empty_words(), indexed.empty_words());
}

/// Every state's sentence names only verbs this binary has. `--demo` is the regression this asserts
/// against: it sat in ADR-006 §10.3's own table and in two panes while no flag of that name existed.
#[test]
fn no_state_names_a_flag_that_does_not_exist() {
    let all = [
        Facts::unknown(),
        Facts::absent(),
        Facts::unreadable(),
        Facts::no_backend(),
        Facts::present(Some(2), Some("building".to_owned()), Some(500), Some(0)),
        Facts::present(Some(1), Some("ready".to_owned()), Some(500), Some(412)),
        Facts::present(None, None, None, None),
    ];
    for facts in all {
        for line in [
            facts.empty_words(),
            facts.summary(),
            facts.remedy(),
            facts.origin(),
        ] {
            assert!(
                !line.contains("--demo"),
                "{} names `--demo`: {line}",
                facts.summary()
            );
            assert!(
                !line.trim().is_empty(),
                "{}: an empty sentence is a shrug",
                facts.summary()
            );
        }
    }
}

/// A half-read store prints `unknown`, never a zero: `unknown` is a value and a blank is a lie
/// (ADR-006 §0.1), and `0 commits` over a store that would not open is the same lie with a digit on it.
#[test]
fn a_number_that_was_not_read_is_printed_as_unknown_not_as_zero() {
    let thin = Facts::present(None, None, None, None);
    assert_eq!(thin.summary(), "index epoch unknown");
    let open = Facts::present(Some(2), Some("building".to_owned()), Some(0), Some(0));
    assert_eq!(
        open.empty_words(),
        "index epoch 2 building · 0 commits · 0 eps — the epoch is still being written — wait for \
         it; if a rebuild died halfway, `fathom index --rebuild` starts a clean one"
    );
    assert_eq!(Read::NoBackend, Facts::no_backend().read);
}
