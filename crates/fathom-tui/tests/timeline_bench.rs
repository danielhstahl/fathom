//! The timeline's frame budget and constant memory (`e1.6`, acceptance criterion 1).
//!
//! Two measurements, both against the renderer the terminal actually gets:
//!
//! * **p99 frame time under 16 ms with a 10 000-episode fixture.** The event is a scroll: 240
//!   keystrokes drawn one frame each, timed individually — the same thing `frame::run_loop` reports
//!   through [`fathom_tui::frame::Report`], measured over the operation the criterion names.
//! * **Constant memory.** A counting global allocator wraps `System` and counts bytes, so "constant"
//!   is a number rather than an adjective: the bytes allocated per frame at 10 000 episodes must not
//!   exceed the bytes allocated per frame at 500 by more than the window's own slack. A rail that
//!   quietly started building 10 000 row strings per frame would be caught here, and it would be caught
//!   by a few megabytes rather than by a laptop getting warm.
//!
//! The tests in this file are serialised with one mutex: the allocator counters are process-wide, and a
//! second test allocating during a measurement inflates the number being asserted. (Separate
//! integration-test files are separate processes, so nothing outside this file can interfere.)
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use fathom_tui::app::{App, Model};
use fathom_tui::keymap::Keymap;
use fathom_tui::render::frame_lines;
use fathom_tui::timeline::Batch;
use fathom_tui::views::ViewId;

#[path = "timeline_corpus/mod.rs"]
mod corpus;

use corpus::{NOW, generated_from};

/// The counting allocator. Every allocation in this process goes through it, including the ones the
/// test harness itself makes — which is why the measurement is a *delta* across a loop whose own
/// allocation profile is the thing under test.
struct Counting;

static ALLOC_BYTES: AtomicUsize = AtomicUsize::new(0);
static ALLOC_CALLS: AtomicUsize = AtomicUsize::new(0);
/// Held while a measurement is running, so two tests cannot measure over each other.
static MEASURING: Mutex<()> = Mutex::new(());

#[global_allocator]
static ALLOCATOR: Counting = Counting;

// SAFETY: every method forwards to `System` with the caller's own `Layout` and no transformation,
// so the allocator's contract is exactly `System`'s. The counters are `Relaxed` atomics, which is
// sound because nothing in the measurement reads them as a synchronisation point: they exist to count
// bytes, and a miscounted concurrent allocation would inflate a number, never corrupt a heap.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        ALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller's contract is `System`'s, and `layout` is the caller's valid layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: as above; `dealloc` cannot count the freed bytes without lying about errors, so the
        // metric is bytes *requested*, which is what a growth-in-the-frame regression moves anyway.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if new_size > layout.size() {
            ALLOC_BYTES.fetch_add(new_size - layout.size(), Ordering::Relaxed);
        }
        ALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: as above.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

/// Take the measurement lock. Poisoned is fine: a failed measurement left nothing behind.
fn measuring() -> MutexGuard<'static, ()> {
    MEASURING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Build an app at the documented size over `count` episodes.
fn app_with(count: usize) -> App {
    let mut app = App::new(
        Keymap::defaults(),
        Model::empty("the delta model is not in this build"),
        (80, 24),
        "fathom/pilot · main",
    );
    app.timeline.set_now(NOW);
    app.set_view(ViewId::Timeline);
    app.load_episodes(Batch::new("10k fixture", generated_from(count)));
    app
}

/// A scroll, measured one frame at a time.
#[derive(Debug)]
struct Frames {
    times: Vec<Duration>,
    bytes: usize,
    allocations: usize,
    drawn: usize,
}

impl Frames {
    fn p99(&self) -> Duration {
        let mut times = self.times.clone();
        times.sort();
        let index = ((times.len() as f64 * 0.99).ceil() as usize)
            .saturating_sub(1)
            .min(times.len().saturating_sub(1));
        times.get(index).copied().unwrap_or(Duration::ZERO)
    }
    fn worst(&self) -> Duration {
        self.times.iter().copied().max().unwrap_or(Duration::ZERO)
    }
    fn allocations_per_frame(&self) -> f64 {
        if self.drawn == 0 {
            return 0.0;
        }
        f64::from(self.allocations as u32) / f64::from(self.drawn as u32)
    }
    fn bytes_per_frame(&self) -> f64 {
        if self.drawn == 0 {
            return 0.0;
        }
        f64::from(self.bytes as u32) / f64::from(self.drawn as u32)
    }
}

/// Two chord sets, because the two criteria measure different things. `SCROLL_KEYS` is motion only —
/// what "scrolls at p99 and constant memory" names. `EDIT_KEYS` adds `<Space>`, which legitimately
/// rebuilds the order, and belongs in the frame-budget figure: a keystroke that takes 40 ms is over
/// budget whether or not anybody called it a scroll.
const SCROLL_KEYS: [&str; 8] = ["j", "j", "k", "<C-d>", "<C-d>", "<C-u>", "G", "gg"];
const EDIT_KEYS: [&str; 9] = [
    "j", "j", "<C-d>", "j", "<C-d>", "<C-u>", "G", "gg", "<Space>",
];

/// Scroll `frames` times, drawing each one, and report both metrics.
fn scroll(app: &mut App, frames: usize) -> Frames {
    scroll_with(app, frames, &SCROLL_KEYS)
}

fn scroll_with(app: &mut App, frames: usize, chords: &[&str]) -> Frames {
    let mut times = Vec::with_capacity(frames);
    let mut drawn = 0_usize;
    let bytes_before = ALLOC_BYTES.load(Ordering::Relaxed);
    let calls_before = ALLOC_CALLS.load(Ordering::Relaxed);
    for index in 0..frames {
        let chord = chords[index % chords.len()];
        app.feed_key(chord, Instant::now());
        let started = Instant::now();
        let lines = frame_lines(app);
        times.push(started.elapsed());
        // The frame's rows are what a sink would write; reading one keeps the result from being
        // optimised away and costs a fraction of a microsecond.
        if lines.is_empty() {
            unreachable!("a frame is always `rows` lines");
        }
        drawn += 1;
    }
    Frames {
        times,
        bytes: ALLOC_BYTES.load(Ordering::Relaxed) - bytes_before,
        allocations: ALLOC_CALLS.load(Ordering::Relaxed) - calls_before,
        drawn,
    }
}

#[test]
fn a_10_000_episode_rail_scrolls_inside_the_frame_budget() {
    let _guard = measuring();
    let mut app = app_with(10_000);
    // Warm the caches the first frame fills (facets, the order, the group headers), so the measurement
    // is the steady state a reviewer scrolls through and not the cold one.
    scroll_with(&mut app, 40, &EDIT_KEYS);
    let frames = scroll_with(&mut app, 240, &EDIT_KEYS);
    let p99 = frames.p99();
    println!(
        "10 000 episodes: p99 {:.2} ms, worst {:.2} ms over {} scroll frames (debug profile)",
        f64::from(p99.as_micros() as u32) / 1000.0,
        f64::from(frames.worst().as_micros() as u32) / 1000.0,
        frames.drawn
    );
    assert_eq!(frames.drawn, 240);
    assert!(
        p99 < Duration::from_millis(16),
        "p99 {:.2} ms over 240 frames (worst {:.2} ms) at 10 000 episodes: the rail must draw a \
         window, not the index",
        f64::from(p99.as_micros() as u32) / 1000.0,
        f64::from(frames.worst().as_micros() as u32) / 1000.0,
    );
}

#[test]
fn a_frame_allocates_a_window_s_worth_of_memory_not_an_index_s() {
    let _guard = measuring();
    let mut small = app_with(500);
    scroll(&mut small, 30);
    let small = scroll(&mut small, 120);

    let mut big = app_with(10_000);
    scroll(&mut big, 30);
    let big = scroll(&mut big, 120);

    let small_per_frame = small.bytes_per_frame();
    let big_per_frame = big.bytes_per_frame();
    println!(
        "per frame: {:.0} B at 10 000 episodes vs {:.0} B at 500 ({:.0} allocations/frame)",
        big_per_frame,
        small_per_frame,
        big.allocations_per_frame()
    );
    // The window is the same at both sizes, so the per-frame figure has to be the same figure. The
    // tolerance is generous on purpose: this catches an O(episodes) frame, which is megabytes, not a
    // few hundred bytes of formatter churn.
    // Motion only, so the comparison means what it says: the window is the same number of rows in both
    // corpora, so the bytes are the same bytes. The slack covers the formatter's fixed costs, not a
    // factor of two — a rail that built an index-proportional amount per frame would be over by
    // megabytes, and this catches it at the first such commit.
    assert!(
        big_per_frame <= small_per_frame * 1.25 + 16_384.0,
        "per-frame allocation was {big_per_frame:.0} B at 10 000 episodes and {small_per_frame:.0} B \
         at 500 — a frame must not cost what the index costs"
    );
    assert!(
        big_per_frame < 2_000_000.0,
        "an absolute ceiling as well: {big_per_frame:.0} B per frame is not a 16 ms frame"
    );
    assert!(
        big.allocations_per_frame() > 0.0,
        "the counters are live: a frame that allocated nothing measures nothing"
    );
}

#[test]
fn the_rail_holds_its_rows_linearly_and_a_scroll_rebuilds_nothing() {
    let _guard = measuring();
    let mut app = app_with(1_000);
    let base = app.timeline.stats();
    let per_row = base.retained_bytes / base.episodes.max(1);
    let big = app_with(10_000).timeline.stats();
    assert!(
        big.retained_bytes / big.episodes < per_row * 3,
        "retained bytes per episode are the same at both sizes ({per_row} vs {}): the rail keeps \
         rows, not copies of them",
        big.retained_bytes / big.episodes.max(1)
    );

    app.timeline.rail_mut().ensure_current();
    let before = app.timeline.last_stats().rebuilds;
    for _ in 0..50 {
        app.feed_key("j", Instant::now());
        app.feed_key("<C-d>", Instant::now());
    }
    assert_eq!(
        app.timeline.last_stats().rebuilds,
        before,
        "nothing that scales with the index runs while scrolling"
    );
}
