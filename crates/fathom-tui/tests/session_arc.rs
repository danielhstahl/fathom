//! The whole session, in keystrokes (`reco-xke`'s exit criterion).
//!
//! *"An entire review session, from notification to signed-off artefact, happens without leaving the
//! keyboard."* Every other test in this crate checks one surface; this one walks the arc, in order,
//! pressing chords through [`App::feed_key`] — the same entry point the terminal's bytes arrive at —
//! and asserting at each step that the session moved forward and that the keystroke that moved it
//! was a keystroke the shell knows.
//!
//! Three things make this more than a longer smoke test:
//!
//! * **every press must fire.** A chord that resolves to nothing is a dead key, and a dead key in the
//!   middle of this arc is a reviewer reaching for the mouse. The arc asserts the fired-action list,
//!   not just the state afterwards;
//! * **the seams are the real ones.** The ledger, the patch provider, the search corpus and the
//!   comparison provider are installed the way `fathom-cli` installs them (the shell may not open a
//!   store, `ADR-002 §5.2`), so "signed off" here means a ledger record was written, and "the diff"
//!   means patch bytes;
//! * **it ends where the ticket says.** The last step is the artefact view, which is the shell's half
//!   of `e5.7`: the record a reviewer posts is written by `fathom export`, and the pane that previews
//!   it has to say so rather than sit blank. `crates/fathom-cli/tests/session_arc.rs` finishes the
//!   sentence in the shell, with the same two verbs a human types.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};
use std::time::Instant;

use fathom_analyze::insight::{EpisodeDelta, QueuePolicy, ReasonVocabulary, TriageRecord};
use fathom_analyze::search::{Corpus, Entry, Index};
use fathom_core::time::Timestamp;
use fathom_tui::ambient::{Glance, Latest};
use fathom_tui::app::{App, Model, Overlay};
use fathom_tui::compare::{CompareProvider, Provider as CompareShell};
use fathom_tui::diff::{BytesSource, PatchProvider, PatchSource, Provider, Unavailable};
use fathom_tui::keymap::Keymap;
use fathom_tui::queue::{Ledger, LedgerHandle, Setup};
use fathom_tui::render::frame_lines;
use fathom_tui::timeline::{AuthorKind, Batch, Episode, ReviewState};
use fathom_tui::views::ViewId;

const NOW: Timestamp = Timestamp::from_unix_micros(1_760_443_200_000_000);
const MINUTE: i64 = 60_000_000;
const SIZE: (u16, u16) = (110, 40);
/// The episode the session reviews: the newest one, which is also the one the ambient pane leads with.
const NEWEST: &str = "e8f1a2c3d4e5f607";

// --- the seams the composition root would install -------------------------------------------------

/// The ledger, standing in for the store's (`ADR-002 §5.2`: the shell never opens one).
#[derive(Default)]
struct Recorder {
    written: Mutex<Vec<TriageRecord>>,
}

impl Ledger for Recorder {
    fn record(&self, record: &TriageRecord) -> Result<(), String> {
        self.written
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(record.clone());
        Ok(())
    }

    fn history(&self, _limit: i64) -> Result<Vec<TriageRecord>, String> {
        Ok(Vec::new())
    }
}

impl Recorder {
    fn written(&self) -> Vec<TriageRecord> {
        self.written
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

/// A patch source, so `d` lands on a diff with hunks in it and not on a refusal.
struct Fixture;

impl PatchProvider for Fixture {
    fn patch_for(&self, scope: &str) -> Result<Box<dyn PatchSource>, Unavailable> {
        Ok(Box::new(BytesSource::new(
            format!("{scope}.diff"),
            format!(
                "--- a/src/store.rs\n+++ b/src/store.rs\n@@ -1,2 +1,3 @@\n fn a() {{}}\n+fn epoch() {{}} // {scope}\n fn b() {{}}\n"
            ),
        )))
    }
}

/// A comparison that answers with a few lines, so the panel has something to scroll.
struct Counted;

impl CompareProvider for Counted {
    fn compare(&self, spec: &str) -> Result<Vec<String>, Unavailable> {
        Ok(vec![
            format!("comparison · {spec}"),
            "structure · 1 file added, 1 modified".to_owned(),
            "findings · none".to_owned(),
        ])
    }
}

fn corpus() -> Corpus {
    let mut corpus = Corpus::new();
    corpus.documents = 4;
    corpus.push(Entry::episode(
        NEWEST,
        "epoch-safe store reads",
        "pin the epoch before the read",
        Some("Dana Stahl".to_owned()),
        Some(Timestamp::from_unix_micros(
            NOW.as_unix_micros() - 12 * MINUTE,
        )),
        Some(72),
    ));
    corpus.push(Entry::episode(
        "9c4d10aabbccddee",
        "cache the epoch probe",
        "one probe per index run",
        Some("Dana Stahl".to_owned()),
        Some(Timestamp::from_unix_micros(
            NOW.as_unix_micros() - 41 * MINUTE,
        )),
        Some(31),
    ));
    corpus
}

fn rows() -> Vec<Episode> {
    vec![
        Episode::new(
            NEWEST,
            Timestamp::from_unix_micros(NOW.as_unix_micros() - 12 * MINUTE),
            "epoch-safe store reads",
        )
        .with_author(),
        Episode::new(
            "9c4d10aabbccddee",
            Timestamp::from_unix_micros(NOW.as_unix_micros() - 41 * MINUTE),
            "cache the epoch probe",
        )
        .with_author(),
    ]
}

/// `Episode::new` is a constructor, not a builder; this is the sugar the fixture wants.
trait WithAuthor {
    fn with_author(self) -> Episode;
}

impl WithAuthor for Episode {
    fn with_author(self) -> Episode {
        let mut episode = self;
        episode.author = AuthorKind::Agent;
        episode.author_name = "pi".to_owned();
        episode.n_files = 2;
        episode.n_add = 60;
        episode.n_del = 4;
        episode.state = ReviewState::Unread;
        episode.delta = Some(EpisodeDelta::sparse(&episode.id, &episode.summary, None));
        episode
    }
}

/// The notification, as the composition root would read it off the store: the newest episode, unread,
/// with the episode to open named. The interactive shell's own ambient view is a facade-fed pane
/// (`ADR-006 §5`), so this is the same hand-off `fathom ambient` does.
fn glance() -> Glance {
    Glance {
        now: NOW,
        root: "fathom/pilot".to_owned(),
        branch: Some("main".to_owned()),
        latest: Some(Latest {
            id: NEWEST.to_owned(),
            summary: "epoch-safe store reads".to_owned(),
            author: "agent".to_owned(),
            created: Some(Timestamp::from_unix_micros(
                NOW.as_unix_micros() - 12 * MINUTE,
            )),
            state: "unread".to_owned(),
            provisional: false,
            risk: None,
        }),
        drill: Some(NEWEST.to_owned()),
        ..Glance::default()
    }
}

/// The shell as the composition root hands it over: clock set, episodes loaded, ledger installed, and
/// every provider a real session has behind it.
fn session() -> (App, Arc<Recorder>) {
    let recorder = Arc::new(Recorder::default());
    let mut app = App::new(
        Keymap::defaults(),
        Model::from_facade(),
        SIZE,
        "fathom/pilot · main",
    );
    app.timeline.set_now(NOW);
    app.load_episodes(Batch::new("fixture index", rows()));
    app.queue = Setup {
        vocabulary: ReasonVocabulary::shipped(),
        policy: QueuePolicy::default(),
        actor: "ada <ada@example.com>".to_owned(),
        ledger: Some(LedgerHandle(Arc::clone(&recorder) as Arc<dyn Ledger>)),
        history: Vec::new(),
        notices: Vec::new(),
        total: 2,
    }
    .state();
    app.queue.sessions = fathom_tui::queue::State::sessions_of(&rows());
    app.patches = Some(Provider(Arc::new(Fixture) as Arc<dyn PatchProvider>));
    app.search = Some(Arc::new(Index::build(&corpus())));
    app.compare = Some(CompareShell::new(Counted));
    // The session state a reviewer who has been here before would have left behind: the mark is set,
    // so `c` is a comparison rather than a refusal.
    app.scope.mark = Some("9c4d10aabbccddee".to_owned());
    // The glance pane's content is the facade's, and the composition root feeds it — exactly what
    // `fathom ambient` does with the same builder (`e5.1`).
    app.model.rows.insert(
        (ViewId::Ambient, "glance".to_owned()),
        fathom_tui::ambient::text_lines(&glance(), fathom_tui::ambient::Mode::default(), 60),
    );
    (app, recorder)
}

/// Press a chord the way the terminal would, and say loudly when nothing answered.
fn press(app: &mut App, chord: &str) {
    let fired = app.feed_key(chord, Instant::now());
    assert!(
        !fired.is_empty(),
        "`{chord}` fired nothing — a dead key in the middle of the session arc"
    );
}

/// Type into a box. Letters that go into a prompt are not chords and fire no action, which is the one
/// place a press is allowed to be silent.
fn type_text(app: &mut App, text: &str) {
    for ch in text.chars() {
        app.feed_key(&ch.to_string(), Instant::now());
    }
}

fn frame(app: &App) -> String {
    frame_lines(app).join("\n")
}

// --- the arc ----------------------------------------------------------------------------------

#[test]
fn notification_to_sign_off_happens_without_leaving_the_keyboard() {
    let (mut app, recorder) = session();

    // 1 · the notification. The ambient pane is what lives in the second terminal, and it leads with
    //     the newest episode — the reviewer reads it before touching anything.
    press(&mut app, "a");
    assert_eq!(app.view, ViewId::Ambient);
    assert!(
        frame(&app).contains(&NEWEST[..6]),
        "the glance pane names the newest episode:\n{}",
        frame(&app)
    );

    // 2 · the queue, and the row the reviewer picks up.
    press(&mut app, "q");
    assert_eq!(app.view, ViewId::Queue);
    press(&mut app, "j");
    press(&mut app, "k");
    let selected = app
        .queue_selected()
        .map(|episode| episode.id.clone())
        .expect("the worklist has a row under the cursor");

    // 3 · drill in, then the diff: patch bytes, not a shrug.
    press(&mut app, "<Enter>");
    press(&mut app, "d");
    assert_eq!(app.view, ViewId::Diff);
    assert!(
        app.diff.file.is_some() || app.diff.tree_len() > 0,
        "`d` landed on a diff with something in it"
    );
    press(&mut app, "j");
    press(&mut app, "<Tab>");

    // 4 · `w`: why this episode is where it is.
    press(&mut app, "t");
    press(&mut app, "w");
    assert!(
        app.explain.is_some(),
        "`w` answered about the episode in scope"
    );

    // 5 · `/`: one box, and the hit deep-links to whichever view owns it.
    press(&mut app, "/");
    assert_eq!(app.overlay, Some(Overlay::Search));
    type_text(&mut app, "epoch");
    assert!(
        app.search_report
            .as_ref()
            .is_some_and(|report| !report.hits.is_empty()),
        "the box scored the query as it was typed"
    );
    press(&mut app, "<Enter>");
    assert_eq!(
        app.view,
        ViewId::Timeline,
        "an episode hit goes to the rail, selected"
    );

    // 6 · `c`: the question a reviewer away for a week asks — this episode against the one I marked.
    press(&mut app, "c");
    assert_eq!(
        app.overlay,
        Some(Overlay::Compare),
        "`c` floated the comparison, its own endpoints and all"
    );
    assert!(
        frame(&app).contains("comparison ·"),
        "the panel drew the provider's answer"
    );
    press(&mut app, "<Esc>");
    assert_eq!(app.overlay, None);
    // `m` leaves a mark behind, so the next `c` has an endpoint without a dialog.
    press(&mut app, "m");
    assert!(app.scope.mark.is_some(), "`m` left a mark behind");

    // 7 · `x`: the sign-off. This is the step with a record behind it — the ledger, not a toast.
    let in_scope = app
        .act_target()
        .map(str::to_owned)
        .unwrap_or_else(|| selected.clone());
    press(&mut app, "x");
    let written = recorder.written();
    assert_eq!(written.len(), 1, "one act, written once");
    assert_eq!(
        written[0].episode, in_scope,
        "the act is about the episode that was in scope"
    );

    // 8 · the artefact: `o` is the shell's half of the record, and it says which verb writes it.
    press(&mut app, "o");
    assert_eq!(app.view, ViewId::Artefact);
    let last = frame(&app);
    assert!(
        last.contains("fathom export"),
        "the artefact pane names the verb that writes the record:\n{last}"
    );

    // 9 · `?` says what the shell can do, and `,` is the door to all of it.
    press(&mut app, "?");
    assert_eq!(app.overlay, Some(Overlay::Help));
    press(&mut app, "<Esc>");
    assert_eq!(app.overlay, None);

    // And the session never once needed the pointer: the whole arc ran on the default keymap, with no
    // mouse installed and no action dispatched by hand.
    assert!(!app.mouse, "the fixture never took the mouse");
}

#[test]
fn the_palette_reaches_every_step_of_the_arc() {
    // The same arc, entered from the one place a reviewer goes when they have forgotten the key: the
    // `,` palette, filtered with the same fuzziness as `/`. Each of these is a step above, reached by
    // letters instead of a memorised chord.
    let (mut app, _recorder) = session();
    for (query, wanted) in [
        ("ambent", "view.ambient"),
        ("queue", "view.queue"),
        ("dif", "view.diff"),
        ("artefact", "view.artefact"),
    ] {
        app.overlay = None;
        app.prompt.clear();
        press(&mut app, ",");
        type_text(&mut app, query);
        let rows = app.palette_rows();
        let ids: Vec<&'static str> = rows
            .iter()
            .filter_map(fathom_tui::app::PaletteRow::action_id)
            .collect();
        assert!(
            ids.contains(&wanted),
            "`{query}` did not surface {wanted}: {ids:?}"
        );
        let at = ids.iter().position(|id| *id == wanted).unwrap();
        app.palette_row = at;
        press(&mut app, "<Enter>");
        assert_eq!(
            app.overlay, None,
            "the palette closed on its own after choosing {wanted}"
        );
    }
}

#[test]
fn nothing_in_the_arc_is_left_to_the_mouse() {
    // The pointer (`e5.5`) is additive: every gesture it adds is also a key. This checks the
    // consequence the other way round — that the mouse being off costs the session nothing, because
    // each gesture's action id is bound in the mode the arc is standing in.
    let km = Keymap::defaults();
    for (mode, chord, action) in [
        (fathom_tui::keymap::Mode::List, "<Enter>", "drill.in"),
        (fathom_tui::keymap::Mode::List, "<Esc>", "drill.out"),
        (fathom_tui::keymap::Mode::Global, ",", "palette.open"),
        (fathom_tui::keymap::Mode::Global, "/", "search.open"),
        (fathom_tui::keymap::Mode::Global, "?", "help.open"),
        (fathom_tui::keymap::Mode::Global, "x", "review.accept"),
    ] {
        let chain = fathom_tui::keymap::mode_chain(mode, fathom_tui::keymap::Mode::List);
        assert_eq!(
            km.resolve(&chain, chord),
            Some(action),
            "`{chord}` no longer means {action} in mode '{}', so the mouse is no longer optional",
            mode.name()
        );
    }
}
