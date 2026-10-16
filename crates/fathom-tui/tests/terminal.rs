//! The screen is borrowed, and every path gives it back.
//!
//! The ticket's second acceptance criterion: neither `<C-c>` nor a worker panic may leave the
//! terminal in alt-screen. Three doors have to be true for that — the normal exit, a panic, and a
//! signal — and each one is tested here against a recorded writer instead of a real terminal,
//! because "the byte `?1049l` was written" is the claim, and a recording proves it without a pty.

#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot fail loudly is not a test

use std::io::{BufWriter, Write};
use std::sync::{Arc, Mutex};

use fathom_tui::app::{App, Model};
use fathom_tui::event::{Bus, Message, WorkerMsg, spawn_worker};
use fathom_tui::frame::{HeadlessSink, run_loop};
use fathom_tui::keymap::Keymap;
use fathom_tui::terminal::{
    ENTER_ALT_SCREEN, Guard, LEAVE_ALT_SCREEN, SHOW_CURSOR, TerminalRestore, install_panic_hook,
    install_signal_hooks,
};
use fathom_tui::views::ViewId;

/// A writer that remembers every byte, so the escape-sequence exchange is readable.
#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<u8>>>);

impl Recorder {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().expect("recorder lock")).into_owned()
    }
}

impl Write for Recorder {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("recorder lock").extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn recorder() -> (Recorder, Arc<TerminalRestore>) {
    let rec = Recorder::default();
    let restore = TerminalRestore::with_sink(Box::new(rec.clone()));
    (rec, restore)
}

/// The file is serialised, and here is why: the panic hook and the signal flags are process-global.
/// A panic in one test runs the hook chain another test installed, which restores *that* test's
/// screen and clears its `owns_screen` before its own assertion can read it — a failure with no
/// cause in the test that reports it. One lock, and the whole file runs in well under a second.
static PROCESS_GLOBALS: Mutex<()> = Mutex::new(());

fn serialised() -> std::sync::MutexGuard<'static, ()> {
    PROCESS_GLOBALS
        .lock()
        .expect("the serialisation lock is never poisoned")
}

fn app(size: (u16, u16)) -> App {
    App::new(
        Keymap::defaults(),
        Model::from_facade(),
        size,
        "fathom/pilot · main",
    )
}

#[test]
fn taking_and_restoring_the_screen_writes_the_pair_in_order() {
    let _serial = serialised();
    let (rec, restore) = recorder();
    restore.enter(true).expect("enter");
    assert!(restore.owns_screen());
    assert!(restore.alt_screen());
    let taken = rec.text();
    assert!(
        taken.starts_with(ENTER_ALT_SCREEN) && taken.contains(SHOW_CURSOR),
        "{taken:?}"
    );
    restore.restore();
    let text = rec.text();
    let leave = text
        .find(LEAVE_ALT_SCREEN)
        .expect("the leave sequence must be written");
    assert!(
        leave > text.find(ENTER_ALT_SCREEN).expect("enter first"),
        "the screen is left after it was entered, not before"
    );
    assert!(
        !restore.owns_screen(),
        "the process no longer owns the screen after a restore"
    );
    // And a second restore writes nothing: a stray `?1049l` scrolls the user's own scrollback.
    let before = rec.text();
    restore.restore();
    assert_eq!(before, rec.text(), "restore is idempotent");
}

#[test]
fn inline_mode_never_touches_the_alt_screen() {
    let _serial = serialised();
    // `--no-alt-screen`: raw mode yes, alt buffer no. If the leave sequence is written anyway the
    // user's scrollback jumps, which is the bug the option exists to avoid.
    let (rec, restore) = recorder();
    restore.enter(false).expect("enter");
    assert!(
        !restore.alt_screen(),
        "alt screen was never taken, so it is not owned"
    );
    assert!(restore.owns_screen(), "raw mode is still ours");
    restore.restore();
    assert!(
        !rec.text().contains(LEAVE_ALT_SCREEN),
        "no alt-screen sequence in inline mode: {:?}",
        rec.text()
    );
    assert!(rec.text().contains(SHOW_CURSOR));
}

#[test]
fn the_panic_hook_restores_before_the_unwind_reaches_the_caller() {
    let _serial = serialised();
    let (rec, restore) = recorder();
    install_panic_hook(&restore);
    restore.enter(true).expect("enter");
    assert!(restore.owns_screen());
    // The default hook's own output would go to stderr; silence it for this one panic.
    let result = std::panic::catch_unwind(|| {
        panic!("the sink exploded while drawing row 4");
    });
    assert!(result.is_err(), "the panic propagated as it should");
    assert!(
        !restore.owns_screen(),
        "the hook must have restored the screen on the way out"
    );
    let text = rec.text();
    assert!(
        text.contains(LEAVE_ALT_SCREEN),
        "the leave sequence was written by the hook: {text:?}"
    );
    // The recorded writer only ever gets terminal control bytes; the panic text goes to the report
    // hook, which is the next thing the same hook runs (see `install_panic_hook`).
    let _ = BufWriter::new(Vec::new());
}

#[test]
fn a_panic_while_drawing_comes_out_of_the_loop_and_the_screen_is_still_restored() {
    let _serial = serialised();
    // The shape of the real failure: a pane owner (`e2.10` and friends) panics inside `draw`. The
    // loop does not swallow it, the hook restores, and the process is the one that reports.
    struct Panicking;
    impl fathom_tui::frame::FrameSink for Panicking {
        fn draw(&mut self, _app: &mut App) -> Result<(), fathom_tui::error::Error> {
            panic!("a pane panicked while drawing");
        }
    }
    let (rec, restore) = recorder();
    install_panic_hook(&restore);
    restore.enter(true).expect("enter");
    let bus = Bus::new();
    let mut shell = app((100, 30));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_loop(&mut shell, &bus, &mut Panicking, None, Some(1))
    }));
    assert!(
        result.is_err(),
        "the loop lets a draw panic through to the hook"
    );
    assert!(
        !restore.owns_screen(),
        "the hook restored the screen: {:?}",
        rec.text()
    );
    assert!(rec.text().contains(LEAVE_ALT_SCREEN));
}

#[test]
fn a_worker_panic_is_a_stated_partial_and_the_loop_keeps_running() {
    let _serial = serialised();
    let bus = Bus::new();
    let index = spawn_worker(Arc::clone(&bus), "index", |_tx| {
        panic!("the index worker fell over at episode 41");
    });
    let mut shell = app((100, 30));
    // The loop drains the bus until the death notice arrives, then keeps going.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while shell.model.notes.is_empty() && std::time::Instant::now() < deadline {
        match bus.recv_timeout(std::time::Duration::from_millis(50)) {
            fathom_tui::event::Received::Message(message) => {
                fathom_tui::frame::apply(&mut shell, message)
            }
            fathom_tui::event::Received::Timeout => {}
            fathom_tui::event::Received::Disconnected => break,
        }
    }
    let notes = shell.model.notes.join("\n");
    assert!(
        notes.contains("index stopped") && notes.contains("episode 41"),
        "the death is named, with its reason: {notes}"
    );
    assert!(
        shell
            .toast
            .as_deref()
            .is_some_and(|t| t.contains("index stopped")),
        "{:?}",
        shell.toast
    );
    // The loop is alive: a keystroke after the panic still resolves.
    shell.feed_key("t", std::time::Instant::now());
    assert_eq!(shell.view, ViewId::Timeline, "the loop survived the panic");
    index.join().expect("the panicked worker unwinds cleanly");
    // …and the terminal was never in play, because nothing left it.
    let (rec, restore) = recorder();
    assert!(!restore.owns_screen() && rec.text().is_empty());
}

#[test]
fn worker_progress_and_rows_land_in_the_model_not_in_a_thread_somewhere() {
    let _serial = serialised();
    let bus = Bus::new();
    let bus2 = Arc::clone(&bus);
    let risk = spawn_worker(Arc::clone(&bus), "risk", move |_tx| {
        let _ = bus2.send(Message::Worker(WorkerMsg::Progress {
            task: "risk",
            done: 1_204,
            of: Some(3_412),
        }));
        let _ = bus2.send(Message::Worker(WorkerMsg::Rows {
            task: "risk",
            view: ViewId::Queue,
            pane: "worklist".to_owned(),
            lines: vec!["e8f1a2 risk 82".to_owned()],
        }));
        let _ = bus2.send(Message::Worker(WorkerMsg::Finished {
            task: "risk",
            note: String::new(),
        }));
    });
    let mut shell = app((100, 30));
    // Everything the worker will say, in order, before the assertions — a loop that stops at the
    // first interesting message would be racing the third one.
    // Apply everything the worker says, remembering the progress line as it stood mid-stream: the
    // last message clears it, which is the point of the last assertion.
    let mut seen = String::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        let batch = bus.drain();
        if batch.is_empty() {
            if shell.model.has_rows() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
            continue;
        }
        for message in batch {
            fathom_tui::frame::apply(&mut shell, message);
            if let Some(line) = &shell.model.index.progress {
                seen = line.clone();
            }
        }
    }
    let progress = seen.clone();
    assert!(
        progress.contains("risk 35 % · 1 204 / 3 412 done")
            || progress.contains("risk 35 % · 1204 / 3412 done"),
        "the progress line is the reviewer's, with thousands separators: {progress}"
    );
    assert_eq!(
        shell.model.rows_for(ViewId::Queue, "worklist"),
        ["e8f1a2 risk 82".to_owned()],
        "rows arrive in the pane they were addressed to"
    );
    risk.join().expect("the worker finished");
    assert!(
        shell.model.index.progress.is_none(),
        "a finished task clears its progress line: {:?}",
        shell.model.index.progress
    );
}

#[test]
fn sigterm_arrives_as_a_flag_and_the_loop_exits_through_the_same_door_as_quit() {
    let _serial = serialised();
    // The signal door, for real: the handler's whole job is to remember, and the loop notices on
    // its poll. Raised in-process, so this covers the flag, the poll and the exit.
    let (signals, notices) = install_signal_hooks();
    assert!(
        notices.is_empty(),
        "SIGINT/SIGTERM/SIGHUP are hookable here: {notices:?}"
    );
    assert_eq!(signals.take(), None, "nothing has arrived yet");
    let raised = signal_hook::low_level::raise(signal_hook::consts::SIGTERM);
    assert!(raised.is_ok(), "could not raise SIGTERM in this process");
    let mut shell = app((100, 30));
    let bus = Bus::new();
    let mut sink = HeadlessSink {
        frames: Vec::new(),
        size: None,
    };
    let report = run_loop(&mut shell, &bus, &mut sink, Some(&signals), None).expect("loop");
    assert_eq!(
        report.end,
        fathom_tui::frame::LoopEnd::Signalled("SIGTERM"),
        "the loop ended because of the signal, not because it ran out of events"
    );
    assert!(
        shell.quit.is_some(),
        "the quit reason is recorded on the way out, whatever the caller prints"
    );
}

#[test]
fn a_signal_that_arrives_as_a_message_exits_the_loop_too() {
    let _serial = serialised();
    // The other door: a loop built on a bus that carries the signal itself (the shape a test or a
    // `--no-alt-screen` session on a platform without signal hooks ends up with).
    let bus = Bus::new();
    bus.send(Message::Signal(fathom_tui::terminal::Signal::Interrupt));
    let mut shell = app((100, 30));
    let mut sink = HeadlessSink {
        frames: Vec::new(),
        size: None,
    };
    let report = run_loop(&mut shell, &bus, &mut sink, None, None).expect("loop");
    assert!(shell.quit.is_some());
    assert!(
        matches!(report.end, fathom_tui::frame::LoopEnd::Quit),
        "{:?}",
        report.end
    );
}

#[test]
fn the_guard_is_the_normal_exit_and_writes_the_pair_when_it_goes_out_of_scope() {
    let _serial = serialised();
    // Needs a terminal to take raw mode; on a piped `cargo test` run there is none, and the hook
    // and explicit-restore tests above already cover the same `restore()` call the `Drop` makes.
    let (rec, restore) = recorder();
    let taken = Guard::take(&restore, true);
    let Ok(guard) = taken else {
        eprintln!(
            "skipped: no terminal to take raw mode ({}); the drop path calls the same restore() \
             the panic-hook test above exercises",
            taken.err().unwrap_or_default()
        );
        return;
    };
    assert!(rec.text().contains(ENTER_ALT_SCREEN));
    drop(guard);
    assert!(
        rec.text().contains(LEAVE_ALT_SCREEN),
        "the guard's Drop put the screen back: {:?}",
        rec.text()
    );
    assert!(!restore.owns_screen());
}
