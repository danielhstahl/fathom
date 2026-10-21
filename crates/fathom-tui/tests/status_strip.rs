//! The status strip, pinned both ways (`reco-zv9`).
//!
//! The strip read `epoch unknown · unknown eps · queue unknown open · idx unknown` on a repository
//! where `fathom stats` printed `epoch 1 ready`, 500 commits, integrity ok and 0 commits owed an
//! analysis. Every one of those four was knowable by the process that had just opened the store to
//! read the session — and ADR-006 §10.3's rule is that `unknown` is a value for what *cannot* be
//! known, so four values that could be known were four lies in the one line a reviewer reads to
//! decide whether to trust the frame.
//!
//! So this file pins the strip at both ends of the rule, and the middle too:
//!
//! * **known** — a caller that read the store gets the epoch, the count, the queue and the verdict,
//!   and the word `unknown` does not appear;
//! * **unknown** — a caller that read nothing (an embedder, a test frame, a repository with no index)
//!   gets `unknown` four times, which is the truth and stays on the page;
//! * **half known** — one fact nobody read is one `unknown`, not an excuse to shrug at the other
//!   three.
//!
//! ADR-006 §5.2's own mock line is the known case, character for character: the ADR has been printing
//! `⏺ epoch 41 · 3 412 eps · queue 7 open · idx warm` since it was written, and this is the first
//! build that can.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot fail loudly is not a test

use std::path::{Path, PathBuf};

use fathom_core::time::Timestamp;
use fathom_tui::app::{App, Model};
use fathom_tui::index::Facts;
use fathom_tui::keymap::Keymap;
use fathom_tui::render::status_line;
use fathom_tui::timeline::{AuthorKind, Batch, Episode, ReviewState};
use fathom_tui::views::ViewId;

/// Fixed clock, as every fixture in this crate uses: a fixture that reads the clock is a fixture with
/// a flake in it (ADR-002 §4.4).
const NOW: Timestamp = Timestamp::from_unix_micros(1_760_443_200_000_000);
const MINUTE: i64 = 60_000_000;
/// The smallest size the shell promises a layout at, so the pinned strip is the narrow case (ADR-002 §3).
const SIZE: (u16, u16) = (80, 24);

/// An index the caller actually read: ADR-006 §5.2's numbers, with the two facts that were missing
/// from the hand-over until now — seven episodes open, and nothing owed an analysis.
fn known() -> Facts {
    Facts::present(Some(41), Some("ready".to_owned()), Some(3_412), Some(3_412))
        .with_queue_open(Some(7))
        .with_backlog(Some(0))
}

/// `count` unread episodes, in the order a chronological store read returns them.
fn rows(count: usize) -> Vec<Episode> {
    (0..count)
        .map(|at| {
            let created = Timestamp::from_unix_micros(
                NOW.as_unix_micros() - i64::try_from(at + 1).unwrap_or(i64::MAX) * MINUTE,
            );
            let mut episode = Episode::new(format!("e-{at:04}"), created, format!("episode {at}"));
            episode.last = created;
            episode.author = AuthorKind::Agent;
            episode.author_name = "pi".to_owned();
            episode.n_add = 10;
            episode.n_del = 2;
            episode.n_files = 3;
            episode.paths = vec![format!("src/module{at}/main.rs")];
            episode.state = ReviewState::Unread;
            episode
        })
        .collect()
}

/// The frame one set of facts draws, at the pinned size.
fn frame(facts: Facts, count: usize) -> App {
    let mut app = App::new(
        Keymap::defaults(),
        Model::from_facade(),
        SIZE,
        "fathom/pilot · main",
    );
    app.timeline.set_now(NOW);
    // Rows first, then the facts: that is the order `frame::prime` uses, and the strip's queue count
    // is allowed to read both.
    if count > 0 {
        app.load_episodes(Batch::new(facts.origin(), rows(count)));
    }
    app.set_index_facts(&facts);
    app.set_view(ViewId::Queue);
    app
}

fn strip(app: &App) -> String {
    status_line(app)
}

/// The known case: all four facts named, and the placeholder nowhere in the line.
#[test]
fn the_strip_names_every_fact_the_caller_read() {
    let line = strip(&frame(known(), 4));
    assert_eq!(line, "⏺ epoch 41 · 3 412 eps · queue 7 open · idx warm");
    for fact in ["epoch 41", "3 412 eps", "queue 7 open", "idx warm"] {
        assert!(
            line.contains(fact),
            "the strip does not name {fact:?}: {line}"
        );
    }
    assert!(
        !line.contains("unknown"),
        "every fact on this line was read from the store: {line}"
    );
}

/// The unknown case, and the one that keeps the fix honest: a caller that read nothing must not be
/// answered with zeroes. This is the strip an unindexed repository gets, and it is correct.
#[test]
fn the_strip_prints_unknown_for_what_nobody_read() {
    let line = strip(&frame(Facts::unknown(), 0));
    assert_eq!(
        line,
        "⏺ epoch unknown · unknown eps · queue unknown open · idx unknown"
    );
    assert_eq!(
        line.matches("unknown").count(),
        4,
        "four facts the caller could not know, four unknowns: {line}"
    );
}

/// A repository with an index and nothing in it: the rail drew no rows, so the shell has no count of
/// its own — but the store does, and `0 open` and `unknown open` are different sentences.
#[test]
fn an_indexed_repository_with_an_empty_queue_says_empty_not_unknown() {
    let facts = Facts::present(Some(1), Some("ready".to_owned()), Some(0), Some(0))
        .with_queue_open(Some(0))
        .with_backlog(Some(0));
    let line = strip(&frame(facts, 0));
    assert_eq!(line, "⏺ epoch 1 · 0 eps · queue 0 open · idx warm");
}

/// One fact nobody read is one `unknown`, not a reason to stop saying the rest.
#[test]
fn one_unknown_fact_does_not_take_the_other_three_down() {
    let facts = Facts::present(Some(2), Some("ready".to_owned()), Some(500), Some(412))
        .with_queue_open(Some(3));
    let line = strip(&frame(facts, 4));
    assert_eq!(line, "⏺ epoch 2 · 412 eps · queue 3 open · idx unknown");
}

/// A backlog is cold, and a building epoch is cold whatever its backlog says: the run that would warm
/// it has not finished.
#[test]
fn an_index_that_owes_an_analysis_is_cold() {
    let cold = known().with_backlog(Some(3));
    let line = strip(&frame(cold, 4));
    assert!(line.ends_with("· idx cold"), "{line}");
    let building = Facts::present(
        Some(42),
        Some("building".to_owned()),
        Some(3_412),
        Some(3_412),
    )
    .with_queue_open(Some(7))
    .with_backlog(Some(0));
    let line = strip(&frame(building, 4));
    assert!(line.ends_with("· idx cold"), "{line}");
}

/// The queue is the index's, not the page's: a rail holding four of 3 412 episodes — or a filter that
/// hid rows — may print the page's unread count only when the caller could not say the real one.
#[test]
fn a_page_of_a_longer_index_counts_the_queue_and_not_the_page() {
    // Four rows on screen, all of them read: the page would say `0 open`.
    let mut read_rows = rows(4);
    for episode in &mut read_rows {
        episode.state = ReviewState::Read;
    }
    let mut app = App::new(
        Keymap::defaults(),
        Model::from_facade(),
        SIZE,
        "fathom/pilot · main",
    );
    app.timeline.set_now(NOW);
    app.load_episodes(Batch::new("index epoch 41 (ready)", read_rows));
    app.set_index_facts(&known());
    app.set_view(ViewId::Queue);
    let line = strip(&app);
    assert!(line.contains("queue 7 open"), "{line}");

    // And when nobody read the store, the shell still counts its own rows rather than shrugging —
    // which is the `reco-c05` behaviour this must not regress.
    let mut blind = App::new(
        Keymap::defaults(),
        Model::from_facade(),
        SIZE,
        "fathom/pilot · main",
    );
    blind.timeline.set_now(NOW);
    blind.load_episodes(Batch::new("fixture index", rows(4)));
    blind.set_view(ViewId::Queue);
    let line = strip(&blind);
    assert!(line.contains("queue 4 open"), "{line}");
    assert!(
        line.contains("idx unknown"),
        "nothing said the verdict: {line}"
    );
}

/// As the reviewer reads, the number moves — provided the rows *are* the queue. With the caller's
/// count in hand for a longer index, the store's number is the one that stays honest.
#[test]
fn reading_an_episode_moves_the_number_the_strip_is_printing() {
    // The caller said nothing about the index's size, so four rows are four episodes are the queue.
    let mut read = rows(4);
    read[0].state = ReviewState::Read;
    let mut app = App::new(
        Keymap::defaults(),
        Model::from_facade(),
        SIZE,
        "fathom/pilot · main",
    );
    app.timeline.set_now(NOW);
    app.load_episodes(Batch::new("fixture index", read));
    app.set_index_facts(&Facts::present(
        Some(41),
        Some("ready".to_owned()),
        Some(4),
        Some(4),
    ));
    app.set_view(ViewId::Queue);
    assert!(
        strip(&app).contains("queue 3 open"),
        "one already-read episode is not open: {}",
        strip(&app)
    );
}

// --- the golden -------------------------------------------------------------------------------------

/// Where the pinned strips live, beside the other whole-frame goldens.
fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("status")
}

fn blessing() -> bool {
    std::env::var("FATHOM_GOLDEN").is_ok_and(|value| value == "write")
}

/// The one golden: the same strip in the states that used to be confused for each other. It is one
/// file rather than three because the point is the comparison — a golden of the known strip alone
/// would pass a build that printed `unknown` in the other two for the wrong reason.
fn golden_text() -> String {
    let mut out = String::new();
    for (name, note, facts, count) in [
        (
            "known",
            "ADR-006 §5.2's own line: epoch, count, queue and verdict, all read from the store by \
             `fathom-cli`, none guessed by the shell",
            known(),
            4,
        ),
        (
            "known · cold",
            "the same repository with three commits owing an analysis: the verdict is a fact the \
             store answers, and `fathom stats` prints the backlog beside it",
            known().with_backlog(Some(3)),
            4,
        ),
        (
            "known · empty queue",
            "an indexed repository with nothing in it: `0 open` is the store's answer, not the \
             shell's guess from an empty rail",
            Facts::present(Some(1), Some("ready".to_owned()), Some(0), Some(0))
                .with_queue_open(Some(0))
                .with_backlog(Some(0)),
            0,
        ),
        (
            "unknown",
            "nothing handed over — no index, an embedder, a test frame. This is the strip where \
             `unknown` is the truth, and the only one",
            Facts::unknown(),
            0,
        ),
    ] {
        out.push_str(&format!("# {name}\n# {note}\n\n"));
        out.push_str(&strip(&frame(facts, count)));
        out.push_str("\n\n");
    }
    out
}

#[test]
fn both_strips_are_pinned() {
    let want = golden_text();
    let path = golden_dir().join("strip.txt");
    if blessing() {
        std::fs::create_dir_all(golden_dir()).expect("the golden directory is writable");
        std::fs::write(&path, &want).expect("the strip golden is writable");
        return;
    }
    let have = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
    assert_eq!(
        have, want,
        "the strip moved; bless it with FATHOM_GOLDEN=write"
    );
}
