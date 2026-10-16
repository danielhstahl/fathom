//! `fathom_tui::diff::materialize` — the hunk decoder that does not run on the UI thread.
//!
//! This is `e1.8` proper. The pane still draws in one pass; what changed is *where a hunk's rows come
//! from*. [`super::doc::decode`] is the expensive half — read a hunk's bytes, run them through the
//! planner — and it needs nothing a frame owns, so it moves to a worker:
//!
//! * the visible window's hunks are asked for at [`Priority::Visible`], the next few hunks at
//!   [`Priority::Adjacent`], and a neighbour episode at [`Priority::Prefetch`];
//! * decoded hunks land in a cache bounded by [`super::Budget::decode_bytes`], so a 50 000-line file
//!   read end to end costs the cap and not the file;
//! * jobs for the window the reviewer scrolled past are cancelled — queued ones dropped, running
//!   ones told to stop at their next line ([`crate::materialize::Cancel`]);
//! * and a window the cache cannot answer is drawn as what it is: the rows that exist, plus a
//!   `⋯ hunk 4 is being decoded` row at the anchor row the real rows will land on. Never the previous
//!   window's content, which is the difference between a fast pane and a lying one.
//!
//! # Who owns the bytes
//!
//! Sources are installed here, keyed by origin, because a worker needs to open one without touching
//! [`super::DiffView`]. An origin is an identity: two loads that call themselves the same name are
//! assumed to be the same bytes, and the caller that knows better says so with
//! [`Materializer::forget_origin`]. That is the whole staleness contract, and it is a stated one
//! rather than a hash of the file's contents.
//!
//! # Inline versus threaded
//!
//! [`Materializer::spawn`] is what switches a pane from "decode it here, now, on the UI thread"
//! ([`super::doc::Live`]) to "draw what the cache has and queue the rest". A `--plain` run, a golden
//! test and any host that never started a worker stay inline: same function, same rows, and no
//! placeholder that would never have been filled.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::event::Bus;
use crate::materialize::{Engine, Generation, Job, Outcome, Priority, SubmitReport, Work};

use super::budget::Budget;
use super::doc::{Decoded, Hunks, Plan, decode};
use super::model::{FileEntry, Shape};
use super::scan::Scan;
use super::source::{PatchReader, PatchSource, Unavailable};

/// Which decoded hunk a cache entry is for.
///
/// `origin` is in here so a warmed neighbour episode and the patch on screen cannot collide, and
/// `plan` is in here so a job is self-contained: a worker reads a key and has the byte range, the
/// header and the line numbers it needs, with no shared index to lock and no borrow to outlive.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key {
    /// Which patch the bytes came from, as its source named itself.
    pub origin: String,
    /// Which file, as the index spelled it.
    pub path: String,
    /// Which shape of the pane these rows are for — a decode belongs to exactly one.
    pub shape: Shape,
    /// What to read.
    pub plan: Plan,
}

impl Key {
    /// The key for one hunk of one file of one origin.
    #[must_use]
    pub fn new(origin: &str, path: &str, shape: Shape, plan: Plan) -> Self {
        Key {
            origin: origin.to_owned(),
            path: path.to_owned(),
            shape,
            plan,
        }
    }
}

/// The pane's view of the cache: what a worker has filed, and what it still owes.
///
/// This is the `Hunks` source that makes a frame non-blocking. A hunk that is here is drawn; a hunk
/// that is queued is announced; a hunk that failed prints why. The three are the whole difference
/// between a pane that is fast and a pane that is vague, and none of them is a wait.
pub struct FromCache<'a> {
    materializer: &'a Materializer,
    entry: &'a FileEntry,
    origin: String,
    shape: Shape,
}

impl<'a> FromCache<'a> {
    /// A source over one file of one origin.
    #[must_use]
    pub fn new(
        materializer: &'a Materializer,
        entry: &'a FileEntry,
        origin: &str,
        shape: Shape,
    ) -> Self {
        FromCache {
            materializer,
            entry,
            origin: origin.to_owned(),
            shape,
        }
    }

    /// The key for one of this file's hunks.
    #[must_use]
    pub fn key(&self, hunk: usize) -> Option<Key> {
        let record = self.entry.hunks.get(hunk)?;
        Some(Key::new(
            &self.origin,
            &self.entry.path,
            self.shape,
            Plan::of(hunk, record),
        ))
    }
}

impl Hunks for FromCache<'_> {
    fn decoded(&mut self, n: usize) -> Option<Arc<Decoded>> {
        let key = self.key(n)?;
        self.materializer.get(&key)
    }

    fn coming(&mut self, n: usize) -> bool {
        self.key(n)
            .is_some_and(|key| self.materializer.coming(&key))
    }

    fn why_missing(&mut self, n: usize) -> Option<String> {
        let key = self.key(n)?;
        self.materializer.failure(&key)
    }
}

/// One origin's pre-warmed index: the scan a worker did so the UI thread does not have to.
#[derive(Debug, Clone)]
pub struct Warm {
    /// The origin, as its source named itself.
    pub origin: String,
    /// The index, once the worker has finished it. `None` means still warming.
    pub scan: Option<Scan>,
    /// Why it will not finish, if it will not.
    pub why: Option<String>,
    /// How many hunks were decoded ahead of the switch.
    pub decoded: usize,
}

impl Warm {
    /// One line for a toast or a strip: `warming episode 01J… · 24 hunk(s) ahead`.
    #[must_use]
    pub fn words(&self) -> String {
        if let Some(why) = &self.why {
            return format!("{}: {why}", self.origin);
        }
        match &self.scan {
            Some(scan) => format!(
                "{} warmed · {} file(s) · {} hunk(s) already decoded",
                self.origin,
                crate::app::format_thousands(scan.files.len() as u64),
                self.decoded
            ),
            None => format!("{} is still warming", self.origin),
        }
    }
}

/// What a decode worker needs and nothing else: where to read, how much to read at a time, and the
/// bookkeeping three threads agree on. Shared by `Arc`, so every clone of the materialiser — the UI
/// thread's and every worker's — sees one copy of it.
struct State {
    sources: Mutex<BTreeMap<String, Box<dyn PatchSource>>>,
    budget: Mutex<Budget>,
    /// Origins a caller offered as "the next episode" or "the previous one", by slot name.
    prewarmed: Mutex<BTreeMap<String, String>>,
    /// Warmed indexes, by origin.
    warm: Mutex<BTreeMap<String, Warm>>,
    /// The generation counter per lane, so a lane's own scrolls supersede it and nothing else does.
    lanes: Mutex<BTreeMap<(String, String, usize), Generation>>,
}

/// The diff viewer's decode engine.
///
/// Held by [`super::DiffView`] and cheap to hand around: the state behind it is `Arc`-shared with
/// every worker.
#[derive(Clone)]
pub struct Materializer {
    engine: Engine<Key, Decoded>,
    state: Arc<State>,
    /// Has anyone started a worker? Until then a miss is decoded inline, because nothing else will.
    threaded: Arc<AtomicBool>,
}

impl Default for Materializer {
    fn default() -> Self {
        Materializer::new(Budget::DEFAULT)
    }
}

impl std::fmt::Debug for Materializer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Materializer")
            .field("threaded", &self.is_threaded())
            .field("stats", &self.engine.stats())
            .finish_non_exhaustive()
    }
}

impl Materializer {
    /// A materialiser with the given caps.
    #[must_use]
    pub fn new(budget: Budget) -> Self {
        Materializer {
            engine: Engine::new("diff decode", budget.decode_bytes, budget.max_in_flight),
            state: Arc::new(State {
                sources: Mutex::new(BTreeMap::new()),
                budget: Mutex::new(budget),
                prewarmed: Mutex::new(BTreeMap::new()),
                warm: Mutex::new(BTreeMap::new()),
                lanes: Mutex::new(BTreeMap::new()),
            }),
            threaded: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The engine, for a caller that wants to assert on counters.
    #[must_use]
    pub fn engine(&self) -> &Engine<Key, Decoded> {
        &self.engine
    }

    /// Re-read the caps after a caller changed the budget.
    pub fn set_budget(&self, budget: Budget) {
        if let Ok(mut slot) = self.state.budget.lock() {
            *slot = budget;
        }
    }

    /// The caps in force.
    #[must_use]
    pub fn budget(&self) -> Budget {
        self.state
            .budget
            .lock()
            .map(|budget| *budget)
            .unwrap_or(Budget::DEFAULT)
    }

    /// Install the bytes for one origin, opening them once to prove they are there.
    ///
    /// Replacing an origin drops its decoded hunks: a source that changed is a different set of
    /// offsets, and offsets that point at the wrong bytes are the one failure mode a viewer must not
    /// have.
    pub fn install(&self, source: Box<dyn PatchSource>) -> Result<String, Unavailable> {
        let origin = source.origin();
        source.open()?;
        self.forget_origin(&origin);
        if let Ok(mut sources) = self.state.sources.lock() {
            sources.insert(origin.clone(), source);
        }
        Ok(origin)
    }

    /// Install a source *without* dropping what is already decoded for that origin — the adoption
    /// path, where the bytes were pre-warmed a moment ago and throwing the work away would make the
    /// switch feel slow for no reason.
    pub fn adopt(&self, source: Box<dyn PatchSource>) -> Result<String, Unavailable> {
        let origin = source.origin();
        source.open()?;
        if let Ok(mut sources) = self.state.sources.lock() {
            sources.insert(origin.clone(), source);
        }
        Ok(origin)
    }

    /// Drop one origin's source and everything decoded from it.
    pub fn forget_origin(&self, origin: &str) {
        if let Ok(mut sources) = self.state.sources.lock() {
            sources.remove(origin);
        }
        self.invalidate(origin);
        if let Ok(mut warm) = self.state.warm.lock() {
            warm.remove(origin);
        }
    }

    /// Drop everything decoded from one origin, keeping its source installed. A reload does this:
    /// the bytes are the same bytes, but the caller says they moved, and an offset into old bytes is
    /// the one thing a viewer must not draw.
    pub fn invalidate(&self, origin: &str) {
        let prefix = origin.to_owned();
        self.engine.remove_where(|key, _value| key.origin == prefix);
        if let Ok(mut lanes) = self.state.lanes.lock() {
            lanes.retain(|lane, _| lane.0 != prefix);
        }
    }

    /// Open a reader over one origin's bytes.
    pub fn open(&self, origin: &str) -> Result<Box<dyn PatchReader>, Unavailable> {
        let sources = self
            .state
            .sources
            .lock()
            .map_err(|_| Unavailable::new("the decode worker holds the source lock"))?;
        let source = sources.get(origin).ok_or_else(|| {
            Unavailable::new(format!("no patch source is installed for `{origin}`"))
        })?;
        source.open()
    }

    /// Where one origin's bytes came from, if anything is installed under that name.
    #[must_use]
    pub fn known(&self, origin: &str) -> bool {
        self.state
            .sources
            .lock()
            .is_ok_and(|sources| sources.contains_key(origin))
    }

    /// Start `workers` decoder threads and switch the pane to the threaded policy.
    ///
    /// The handles are kept: dropping the materialiser releases the wakers and the threads leave with
    /// it, and a caller that wants to wait for that can take them.
    pub fn spawn(&self, bus: Arc<Bus>, workers: usize) {
        self.engine.attach_bus(Arc::clone(&bus));
        self.threaded.store(true, Ordering::SeqCst);
        let work = self.work();
        let handles = self.engine.spawn(Some(bus), workers.max(1), work);
        // Kept alive by the engine's wakers; the handles are dropped because nothing in the shell
        // needs to join a decoder, and a decoder that outlives a frame is normal.
        drop(handles);
    }

    /// Is a worker going to answer a miss? `false` means the pane must decode it itself.
    #[must_use]
    pub fn is_threaded(&self) -> bool {
        self.threaded.load(Ordering::SeqCst)
    }

    /// Switch to the cache-served policy *without* starting threads: the caller drives the queue with
    /// [`Materializer::run_one`] and [`Materializer::drain`], so a test can look at the frames between
    /// a request and its answers instead of racing threads that answer instantly.
    ///
    /// This is not a test-only convenience. The frames it makes reachable — a hole, a hole filling in,
    /// a hole that filled with the wrong window's rows — are the ones that only exist when a decode is
    /// in flight, and a test that races threads cannot see them at all.
    pub fn serve_from_cache(&self) {
        self.threaded.store(true, Ordering::SeqCst);
    }

    /// The decode function every worker runs.
    fn work(&self) -> Work<Key, Decoded> {
        let state = Arc::clone(&self.state);
        Arc::new(move |job: &Job<Key>, cancel: &crate::materialize::Cancel| {
            let budget = state
                .budget
                .lock()
                .map(|budget| *budget)
                .unwrap_or(Budget::DEFAULT);
            let opened = state
                .sources
                .lock()
                .ok()
                .and_then(|sources| sources.get(&job.key.origin).map(|source| source.open()));
            let mut reader = match opened {
                None => {
                    return Outcome::Failed(format!(
                        "no patch source is installed for `{}`",
                        job.key.origin
                    ));
                }
                Some(Err(why)) => return Outcome::Failed(format!("`{}`: {why}", job.key.origin)),
                Some(Ok(reader)) => reader,
            };
            // No row limit: a cached hunk has to serve every window that later intersects it, and
            // the read is bounded by `Budget::window_lines` whatever the request was.
            let decoded = decode(
                &job.key.plan,
                reader.as_mut(),
                job.key.shape,
                &budget,
                Some(cancel),
                None,
            );
            if decoded.cancelled {
                // Somebody scrolled past this hunk. Dropping the result is the point of cancelling:
                // no cache write, no stale frame, no wasted second decode.
                return Outcome::Nothing;
            }
            Outcome::Ready(decoded)
        })
    }

    /// Ask for the hunks a window needs, plus the ones just past it, and cancel the ones this lane
    /// no longer wants.
    ///
    /// "This lane" is one `(origin, path, shape)` triple: a new window supersedes the old window's
    /// queued jobs in the same lane, and leaves every other lane — a different file, a warm-up of
    /// the next episode — alone. That is what makes rapid `]`/`]`/`]` cheap instead of merely busy.
    pub fn request(
        &self,
        origin: &str,
        entry: &super::model::FileEntry,
        shape: Shape,
        window: (usize, usize),
        extra_after: usize,
    ) -> SubmitReport {
        let generation: Generation = self.generation_for(origin, &entry.path, shape);
        let current = generation;
        let (first, last) = window;
        let mut jobs: Vec<Job<Key>> = Vec::new();
        for number in first..last.min(entry.hunks.len()) {
            let Some(record) = entry.hunks.get(number) else {
                continue;
            };
            jobs.push(Job::new(
                Key::new(origin, &entry.path, shape, Plan::of(number, record)),
                Priority::Visible,
                generation,
            ));
        }
        for number in last..entry.hunks.len().min(last.saturating_add(extra_after)) {
            let Some(record) = entry.hunks.get(number) else {
                continue;
            };
            jobs.push(Job::new(
                Key::new(origin, &entry.path, shape, Plan::of(number, record)),
                Priority::Adjacent,
                generation,
            ));
        }
        let wanted_first = first;
        let wanted_last = last.saturating_add(extra_after);
        let lane_origin = origin.to_owned();
        let lane_path = entry.path.clone();
        // Two reasons to cancel in the same lane: the job belongs to a window the reviewer has moved
        // past, or it belongs to an *older request* of this lane, which is the same fact stated as a
        // number. Anything in another lane — another file, a warm-up of the next episode — is left
        // running, because nothing about this scroll says it is unwanted.
        self.engine.cancel_where(move |job| {
            let key = &job.key;
            if job.priority == Priority::Prefetch {
                // Prefetch is a different promise from this lane's window: "get it ready whenever",
                // and nothing about scrolling past a hunk makes a warm-up's copy of it unwanted. This
                // is what keeps a pre-warmed episode's queued decodes alive through the switch.
                return false;
            }
            if key.origin != lane_origin || key.path != lane_path || key.shape != shape {
                return false;
            }
            job.generation < current || key.plan.hunk < wanted_first || key.plan.hunk >= wanted_last
        });
        self.engine.submit(jobs)
    }

    /// The cached rows for a key, if a worker has filed them.
    pub fn get(&self, key: &Key) -> Option<Arc<Decoded>> {
        self.engine.cached(key)
    }

    /// Is a result for this key still on the way?
    #[must_use]
    pub fn coming(&self, key: &Key) -> bool {
        self.engine.pending(key)
    }

    /// Why a key failed, if it did — the sentence a footer prints.
    #[must_use]
    pub fn failure(&self, key: &Key) -> Option<String> {
        self.engine.failure(key)
    }

    /// The counters.
    #[must_use]
    pub fn stats(&self) -> crate::materialize::EngineStats {
        self.engine.stats()
    }

    /// Run the next queued job on the calling thread. The deterministic path a test drives instead of
    /// starting threads.
    pub fn run_one(&self) -> bool {
        self.engine.run_one(&self.work())
    }

    /// Run every queued job on the calling thread.
    pub fn drain(&self) -> usize {
        self.engine.drain(&self.work())
    }

    /// How many hunks the cache holds for one origin.
    #[must_use]
    pub fn cached_for(&self, origin: &str) -> usize {
        let stats = self.engine.stats();
        let _ = stats;
        // The cache does not index by origin; a caller that wants this number is a test, and a test
        // can afford the walk.
        self.engine
            .keys()
            .iter()
            .filter(|key| key.origin == origin)
            .count()
    }

    // --- pre-warming the next and previous episode ------------------------------------------------
    //
    // The switch to an episode costs two things: scanning its patch, and decoding the hunks the first
    // frame shows. Both happen here, on a worker, before the reviewer presses `n`.

    /// Offer a neighbour patch to warm. `slot` names it (`next`, `previous`) so a later `load` of the
    /// same origin adopts what was done instead of starting again.
    pub fn prewarm(
        &self,
        slot: &str,
        source: Box<dyn PatchSource>,
        bus: Option<Arc<Bus>>,
    ) -> Result<String, Unavailable> {
        let origin = source.origin();
        source.open()?;
        if let Ok(mut sources) = self.state.sources.lock() {
            sources.insert(origin.clone(), source);
        }
        if let Ok(mut prewarmed) = self.state.prewarmed.lock() {
            prewarmed.insert(slot.to_owned(), origin.clone());
        }
        if let Ok(mut warm) = self.state.warm.lock() {
            warm.insert(
                origin.clone(),
                Warm {
                    origin: origin.clone(),
                    scan: None,
                    why: None,
                    decoded: 0,
                },
            );
        }
        let this = self.clone();
        let slot = slot.to_owned();
        let origin_for_thread = origin.clone();
        if let Some(bus) = bus.clone() {
            self.engine.attach_bus(bus);
        }
        std::thread::spawn(move || {
            this.warm_one(&slot, &origin_for_thread);
        });
        Ok(origin)
    }

    /// The warm-up's body: scan, then decode the first hunks of the first files at `Prefetch`.
    fn warm_one(&self, slot: &str, origin: &str) {
        let budget = self.budget();
        let scan = match self.open(origin) {
            Ok(mut reader) => super::scan::scan(origin, reader.as_mut(), &budget),
            Err(why) => {
                if let Ok(mut warm) = self.state.warm.lock()
                    && let Some(entry) = warm.get_mut(origin)
                {
                    entry.why = Some(why.to_string());
                }
                return;
            }
        };
        let mut decoded = 0_usize;
        let mut jobs = Vec::new();
        let shape = Shape::default();
        for entry in scan.files.iter() {
            for (number, record) in entry.hunks.iter().enumerate() {
                if decoded >= budget.prewarm_hunks {
                    break;
                }
                jobs.push(Job::new(
                    Key::new(origin, &entry.path, shape, Plan::of(number, record)),
                    Priority::Prefetch,
                    // A warm-up is not superseded by the on-screen lane's generation churn: it is a
                    // different lane by construction (different path), and it is never cancelled.
                    0,
                ));
                decoded += 1;
            }
            if decoded >= budget.prewarm_hunks {
                break;
            }
        }
        if let Ok(mut warm) = self.state.warm.lock()
            && let Some(entry) = warm.get_mut(origin)
        {
            entry.scan = Some(scan.clone());
            entry.decoded = decoded;
        }
        self.engine.submit(jobs);
        let _ = slot;
    }

    /// Take a warmed index for an origin, if a warm-up finished one. `None` means scan it normally.
    pub fn take_warm(&self, origin: &str) -> Option<Scan> {
        let mut warm = self.state.warm.lock().ok()?;
        warm.get_mut(origin).and_then(|entry| entry.scan.take())
    }

    /// What a warm-up has to say, for a toast.
    #[must_use]
    pub fn warm_words(&self, origin: &str) -> Option<String> {
        self.state.warm.lock().ok()?.get(origin).map(Warm::words)
    }

    /// Which origins are currently offered as a neighbour, and what they are.
    #[must_use]
    pub fn prewarmed(&self) -> Vec<(String, String)> {
        self.state
            .prewarmed
            .lock()
            .map(|slots| slots.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default()
    }

    /// Adopt path: has this origin been pre-warmed? If so, forget which slot it was in — the pane is
    /// about to *be* that episode, so the slot is free for the next neighbour.
    pub fn claim_prewarm(&self, origin: &str) -> bool {
        let mut slots = match self.state.prewarmed.lock() {
            Ok(slots) => slots,
            Err(_) => return false,
        };
        let found = slots
            .iter()
            .find(|(_slot, value)| value.as_str() == origin)
            .map(|(slot, _)| slot.clone());
        if let Some(slot) = found {
            slots.remove(&slot);
            true
        } else {
            false
        }
    }

    /// Bump and return the lane's generation. Every submit in a lane carries a higher number than the
    /// one before it, so "cancel what the reviewer scrolled past" is a comparison and not a book of
    /// who asked for what.
    fn generation_for(&self, origin: &str, path: &str, shape: Shape) -> Generation {
        let lane = (origin.to_owned(), path.to_owned(), shape.slot());
        let Ok(mut lanes) = self.state.lanes.lock() else {
            return 1;
        };
        let next = lanes.get(&lane).copied().unwrap_or(0).saturating_add(1);
        lanes.insert(lane, next);
        next
    }
}
