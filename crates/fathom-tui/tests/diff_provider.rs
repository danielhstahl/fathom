//! The seam `d` rides on: a provider installed by the shell turns the selected episode into a
//! patch, and the diff pane paints it (`reco-0kw`).
//!
//! `fathom-tui` cannot open a store, and must not — so the *product* path for `d` is: resolve the
//! selected episode, ask the installed [`PatchProvider`] for bytes, load them here. These tests pin
//! that machinery with a fake provider; the byte source itself — the store — is proven by
//! `crates/fathom-cli/tests/diff_store.rs`, spawning the shipped binary, and end to end by
//! `tests/test_diff_open_contract.py`. The pane was reachable for months and *empty*: no provider
//! existed to install. That is what this file makes impossible to re-introduce silently.
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "corpus/mod.rs"]
mod corpus;

use std::sync::{Arc, Mutex};

use fathom_tui::app::{App, Model};
use fathom_tui::diff::{BytesSource, PatchProvider, PatchSource, Provider, Unavailable};
use fathom_tui::keymap::Keymap;
use fathom_tui::timeline::Batch;
use fathom_tui::views::ViewId;

#[path = "timeline_corpus/mod.rs"]
mod tcorpus;

use corpus::frame_text;
use tcorpus::{NOW, mock};

/// A provider that records every scope it was asked about, and answers from one fixture — or
/// refuses, for the scopes that are named as gone. What the fixture contains does not matter here
/// (the decode path has its own tests); that a *loaded* pane arrives does.
#[derive(Default)]
struct Recorder {
    calls: Mutex<Vec<String>>,
}

impl Recorder {
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl PatchProvider for Recorder {
    fn patch_for(&self, scope: &str) -> Result<Box<dyn PatchSource>, Unavailable> {
        self.calls.lock().unwrap().push(scope.to_owned());
        if scope.contains("gone") {
            return Err(Unavailable::new(format!(
                "no episode {scope} in the index — `fathom episodes` lists what is"
            )));
        }
        Ok(Box::new(BytesSource::new(
            format!("{scope}.diff"),
            String::from_utf8_lossy(include_bytes!("fixtures/mixed.diff")).into_owned(),
        )))
    }
}

/// A timeline app with mock rows and the fake provider installed — the state a reviewer is in one
/// `d` away from a diff.
fn app_with_provider(size: (u16, u16)) -> (App, Arc<Recorder>) {
    let recorder = Arc::new(Recorder::default());
    let mut app = App::new(
        Keymap::defaults(),
        Model::from_facade(),
        size,
        "fathom/provider · fixture",
    );
    app.timeline.set_now(NOW);
    app.load_episodes(Batch::new("fixture index", mock()));
    app.set_view(ViewId::Timeline);
    app.patches = Some(Provider(Arc::clone(&recorder) as Arc<dyn PatchProvider>));
    (app, recorder)
}

/// Press a key the way the terminal would.
fn press(app: &mut App, chord: &str) {
    let fired = app.feed_key(chord, std::time::Instant::now());
    assert!(
        !fired.is_empty(),
        "{chord:?} dispatched nothing — is it still bound?"
    );
}

#[test]
fn d_asks_the_provider_for_the_selected_episode_and_loads_it() {
    let (mut app, recorder) = app_with_provider((110, 28));
    let selected = mock()[0].id.clone();

    press(&mut app, "d");

    assert_eq!(
        recorder.calls(),
        vec![selected.clone()],
        "`d` asks for exactly the episode under the cursor, once"
    );
    assert_eq!(app.view, ViewId::Diff, "and the view opens");
    assert!(
        app.diff.is_loaded(),
        "the pane has an index: {:?}",
        app.diff
    );
    assert_eq!(
        app.diff.index().map(|s| s.files.len()),
        mock_file_count(),
        "the fixture's files are the tree the pane now paints"
    );
    assert!(
        frame_text(&app).contains("@@ -10,9 +10,10 @@"),
        "and at least one hunk, from its `@@` header down, is on screen:\n{}",
        frame_text(&app)
    );

    // `d` again on the same episode: the pane already answers this question. A reload would drop
    // the reviewer's scroll position and re-read the store for nothing.
    press(&mut app, "d");
    assert_eq!(
        recorder.calls().len(),
        1,
        "the same episode does not re-ask the provider"
    );
}

#[test]
fn moving_the_cursor_then_d_loads_that_episode_instead() {
    let (mut app, recorder) = app_with_provider((110, 28));
    let rows = mock();

    // The scope is what the card and the mark key on; moving the reviewer's scope is the product's
    // own way of saying "now read that one".
    press(&mut app, "d");
    app.scope.episode = Some(rows[1].id.clone());
    press(&mut app, "d");

    assert_eq!(
        recorder.calls(),
        vec![rows[0].id.clone(), rows[1].id.clone()],
        "each episode is asked once, by its own id"
    );
    assert!(app.diff.is_loaded(), "the second patch replaced the first");
}

#[test]
fn a_provider_refusal_is_printed_not_swallowed() {
    let (mut app, recorder) = app_with_provider((110, 28));
    // The scope wins over the rail cursor: this is the id the mark and the card already key on.
    app.scope.episode = Some("ep-gone".into());

    press(&mut app, "d");

    assert_eq!(recorder.calls(), vec!["ep-gone".to_owned()]);
    assert_eq!(
        app.view,
        ViewId::Diff,
        "the refusal opens the pane anyway — an empty rectangle explains nothing"
    );
    assert!(!app.diff.is_loaded(), "and nothing is loaded");
    let said = frame_text(&app);
    assert!(
        said.contains("no episode ep-gone in the index"),
        "the refusal's own words are the pane's content:\n{said}"
    );
}

/// The fixture's own file count, computed from the same bytes the provider hands over.
#[must_use]
fn mock_file_count() -> Option<usize> {
    let patch = corpus::patch("mixed");
    Some(patch.matches("\ndiff --git ").count() + usize::from(patch.starts_with("diff --git ")))
}

#[test]
fn d_without_a_provider_or_an_episode_still_opens_the_pane_saying_what_is_missing() {
    let mut app = App::new(
        Keymap::defaults(),
        Model::from_facade(),
        (110, 28),
        "fathom/provider · empty",
    );
    app.set_view(ViewId::Timeline);

    press(&mut app, "d");

    assert_eq!(app.view, ViewId::Diff);
    assert!(!app.diff.is_loaded());
    let said = frame_text(&app);
    assert!(
        said.contains("no diff") && said.contains("`d`"),
        "the empty pane has to name the keystroke and the missing thing:\n{said}"
    );
}
