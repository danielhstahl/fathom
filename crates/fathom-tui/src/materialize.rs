//! `fathom_tui::materialize` — the machinery for work that must not happen on the UI thread.
//!
//! This is `e1.8`'s shape, written once so the places that need it — hunk decoding, the file tree,
//! the timeline's pages — are one mechanism rather than three approximations of it. Four
//! guarantees, each of them a type here and each asserted in `tests/materialize.rs`:
//!
//! * **Bounded in flight.** A [`Scheduler`] holds at most [`Scheduler::max_in_flight`] jobs between
//!   queue and workers. Scroll a thousand rows and the queue is still that size: the excess is
//!   dropped from the bottom (weakest tier first) rather than queued to be read next week.
//! * **Priority to what is on screen.** Three [`Priority`] tiers: the visible window first, the rows
//!   one keystroke away second, everything merely guessed at last. A burst of prefetch never delays
//!   the row the reviewer is reading.
//! * **Cancellable.** Every admitted job carries a [`Cancel`] token; a request the reviewer scrolled
//!   past is revoked before it starts, and one already running is asked to stop at its next
//!   checkpoint. `Cancel` is a flag rather than a thread kill: safe Rust cannot stop a thread, and a
//!   worker that checks a flag is a worker that can be trusted to.
//! * **Bounded cache.** A [`Cache`] holds at most `max_bytes`, least-recently-used out of the way. A
//!   value larger than the whole cap is *refused with a reason* ([`Insert::TooBig`]) rather than
//!   squeezed in or silently dropped, because a pane that has to say "this is not here" should say it
//!   from a counter rather than from a guess.
//!
//! [`Generation`] is what makes the third guarantee scale: the UI owns a counter per lane and bumps
//! it whenever the set of things it wants changes, so one call can revoke a whole window's worth of
//! work — queued jobs removed, running ones told — and a result tagged with an older generation is
//! dropped on arrival whatever else is in flight. That is what makes "rapid `]`/`]`/`]` never shows
//! stale content" a property of the queue instead of a hope about timing.
//!
//! [`Engine`] owns a scheduler, a cache and the bus results arrive on, and runs jobs either on threads
//! ([`Engine::spawn`]) or on the calling thread ([`Engine::run_one`]). Both exist on purpose:
//! production wants threads, a test wants to know exactly which job ran when, and neither should
//! cost the other a second code path.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::hash::Hash;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread::JoinHandle;

use crate::event::{Bus, Message, WorkerMsg};

/// Who asked for a piece of work, best first.
///
/// The tiers are the reviewer's attention, not the machine's convenience: what is on screen, what is
/// one keystroke from the screen, and what the code merely expects to be wanted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    /// Inside the visible window — served before anything else.
    Visible,
    /// Just outside it: the next hunk, the next page, the neighbour the motion keys reach.
    Adjacent,
    /// A guess: the next episode, a row nobody has asked for yet.
    Prefetch,
}

impl Priority {
    /// Every tier, best first.
    pub const ALL: [Priority; 3] = [Priority::Visible, Priority::Adjacent, Priority::Prefetch];

    /// The bucket this tier owns: lower is served first.
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Priority::Visible => 0,
            Priority::Adjacent => 1,
            Priority::Prefetch => 2,
        }
    }

    /// The word the status line prints.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Priority::Visible => "visible",
            Priority::Adjacent => "adjacent",
            Priority::Prefetch => "prefetch",
        }
    }

    /// Is this tier worth more than `other`?
    #[must_use]
    pub const fn outranks(self, other: Priority) -> bool {
        (self as u8) < (other as u8)
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Which request a job belongs to.
///
/// The UI owns the counter and bumps it whenever the set of wanted keys changes — a scroll, a new
/// file, a layout toggle, a re-index. A job carries the generation that asked for it; anything older
/// is superseded, because the current generation's needs are the only ones left.
pub type Generation = u64;

/// The polite way to stop a job that is already running.
///
/// A worker checks [`Cancel::is_cancelled`] at its own checkpoints — between chunks, between hunks —
/// and stops early. This is not thread cancellation, which safe Rust does not have; it is a shared
/// bit with a name, which is enough to make "do not deliver work nobody is waiting for" true.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    /// An un-cancelled token.
    #[must_use]
    pub fn new() -> Self {
        Cancel(Arc::new(AtomicBool::new(false)))
    }

    /// Has someone asked this job to stop?
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    /// Ask it to stop. Idempotent, and cheap enough to call from a keystroke handler.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// Did the work produce something?
///
/// [`Outcome::Nothing`] and [`Outcome::Failed`] are different sentences: "somebody cancelled it, so
/// nobody is waiting" is silence, and "the blob is not in the packfile" is a line in a pane.
#[derive(Debug, Clone)]
pub enum Outcome<V> {
    /// Here is the value; cache it and wake the frame.
    Ready(V),
    /// Nothing to cache — cancelled, or the key turned out to be empty.
    Nothing,
    /// It did not work, in the words the pane will print.
    Failed(String),
}

impl<V> Outcome<V> {
    /// The failure sentence, if this is one.
    #[must_use]
    pub fn failure(&self) -> Option<&str> {
        match self {
            Outcome::Failed(why) => Some(why),
            _ => None,
        }
    }
}

/// What a cached value says it costs, in the bytes the cache cap counts.
pub trait Cost {
    /// Bytes the value retains.
    fn cost(&self) -> usize;
}

impl Cost for () {
    fn cost(&self) -> usize {
        0
    }
}

/// One unit of work, as submitted.
#[derive(Debug, Clone)]
pub struct Job<K> {
    /// What is wanted.
    pub key: K,
    /// How badly.
    pub priority: Priority,
    /// The generation that asked for it.
    pub generation: Generation,
}

impl<K> Job<K> {
    /// One job.
    #[must_use]
    pub const fn new(key: K, priority: Priority, generation: Generation) -> Self {
        Job {
            key,
            priority,
            generation,
        }
    }
}

/// A job a worker is holding, and the token that can tell it to stop.
#[derive(Debug, Clone)]
pub struct Running<K> {
    /// What was asked for.
    pub key: K,
    /// Who asked.
    pub generation: Generation,
    /// The priority it was admitted at.
    pub priority: Priority,
    /// The token to check between chunks.
    pub cancel: Cancel,
}

/// What happened to a submitted job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admit {
    /// It is in the queue.
    Queued,
    /// The same key is already queued — one decode per key, whatever the burst.
    AlreadyQueued,
    /// A worker is already producing it.
    AlreadyRunning,
    /// The queue is at its cap and nothing weaker could be evicted for it. Counted, never silently
    /// discarded.
    DroppedFull,
}

/// What a batch of submissions cost.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SubmitReport {
    /// Jobs admitted.
    pub queued: usize,
    /// Jobs skipped because the key was already queued or running.
    pub duplicated: usize,
    /// Jobs refused by the in-flight cap.
    pub dropped: usize,
    /// Queued jobs evicted to make room for something better.
    pub displaced: usize,
}

/// The counters a test or a status line reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counters {
    /// Jobs handed in.
    pub submitted: u64,
    /// Jobs admitted to the queue.
    pub admitted: u64,
    /// Jobs the cap refused outright.
    pub dropped: u64,
    /// Jobs skipped as duplicates.
    pub duplicated: u64,
    /// Queued jobs cancelled or evicted before they started.
    pub superseded: u64,
    /// Running jobs told to stop.
    pub interrupted: u64,
    /// Jobs handed to a worker.
    pub started: u64,
    /// Jobs that finished, well or badly.
    pub completed: u64,
    /// Jobs that reported a failure.
    pub failed: u64,
    /// Jobs that stopped because they were cancelled.
    pub cancelled: u64,
    /// The most work ever in flight at once — the number the "no unbounded queue" criterion reads.
    pub peak_in_flight: usize,
}

/// The queue: three priority tiers, a hard cap, and cancellations.
///
/// The cap counts *queued plus running* jobs, which is the only definition that makes "rapid
/// scrolling cannot queue unbounded work" true: a queue that never grows but whose results are all
/// still arriving costs the same as one that grows.
pub struct Scheduler<K> {
    max_in_flight: usize,
    /// One FIFO per tier; tiers are drained best-first, and inside a tier the order is submission
    /// order, which is the reviewer's order.
    buckets: [VecDeque<Job<K>>; 3],
    /// Jobs a worker is holding.
    running: HashMap<K, Running<K>>,
    /// Keys sitting in a bucket, so a duplicate costs a lookup instead of a scan.
    queued: HashMap<K, Priority>,
    /// The counters.
    pub counts: Counters,
}

impl<K> Scheduler<K> {
    /// A queue with the given cap. Zero is allowed and means "nothing is ever admitted", which is how
    /// a test proves a cap is a cap rather than a coincidence.
    #[must_use]
    pub fn new(max_in_flight: usize) -> Self {
        Scheduler {
            max_in_flight,
            buckets: [VecDeque::new(), VecDeque::new(), VecDeque::new()],
            running: HashMap::new(),
            queued: HashMap::new(),
            counts: Counters::default(),
        }
    }

    /// The cap.
    #[must_use]
    pub const fn max_in_flight(&self) -> usize {
        self.max_in_flight
    }

    /// Queued plus running.
    #[must_use]
    pub fn in_flight(&self) -> usize {
        self.queued.len() + self.running.len()
    }

    /// Queued, not yet started.
    #[must_use]
    pub fn queued(&self) -> usize {
        self.queued.len()
    }

    /// Handed to a worker, not yet finished.
    #[must_use]
    pub fn running(&self) -> usize {
        self.running.len()
    }

    /// Is there nothing to do?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.in_flight() == 0
    }

    /// How many jobs of one tier are queued.
    #[must_use]
    pub fn queued_at(&self, priority: Priority) -> usize {
        self.buckets[priority.index()].len()
    }

    /// Take the best job, mark it running, and hand back its cancel token.
    ///
    /// Not `Iterator::next`, which this is not: it hands out a job only if the cap allows and marks
    /// it in flight, so two workers never get the same key.
    #[must_use]
    pub fn take_next(&mut self) -> Option<Running<K>>
    where
        K: Clone + Eq + Hash,
    {
        for tier in Priority::ALL {
            while let Some(job) = self.buckets[tier.index()].pop_front() {
                self.queued.remove(&job.key);
                if self.running.contains_key(&job.key) {
                    // Already being produced; a second copy would be the same bytes twice.
                    continue;
                }
                let running = Running {
                    key: job.key,
                    generation: job.generation,
                    priority: job.priority,
                    cancel: Cancel::new(),
                };
                self.running.insert(running.key.clone(), running.clone());
                self.counts.started += 1;
                self.note_peak();
                return Some(running);
            }
        }
        None
    }

    /// A job is finished, well or badly.
    pub fn complete(&mut self, key: &K) -> Option<Running<K>>
    where
        K: Eq + Hash,
    {
        let taken = self.running.remove(key);
        if let Some(running) = &taken {
            self.counts.completed += 1;
            if running.cancel.is_cancelled() {
                self.counts.cancelled += 1;
            }
        }
        taken
    }

    /// Cancel every queued job the predicate names, and tell every running one to stop.
    ///
    /// Returns how many were affected. Running jobs are *not* removed: they finish, and their result
    /// is dropped by the [`Engine`] because nobody asked for it any more. Pretending they are gone
    /// while a thread is still reading would be a lie about the machine's state.
    pub fn cancel_where(&mut self, mut pred: impl FnMut(&Job<K>) -> bool) -> usize
    where
        K: Clone + Eq + Hash,
    {
        let mut affected = 0_usize;
        for tier in Priority::ALL {
            let bucket = std::mem::take(&mut self.buckets[tier.index()]);
            for job in bucket {
                if pred(&job) {
                    self.queued.remove(&job.key);
                    self.counts.superseded += 1;
                    affected += 1;
                } else {
                    self.buckets[tier.index()].push_back(job);
                }
            }
        }
        let stale: Vec<K> = self
            .running
            .values()
            .filter(|running| {
                pred(&Job {
                    key: running.key.clone(),
                    priority: running.priority,
                    generation: running.generation,
                })
            })
            .filter(|running| !running.cancel.is_cancelled())
            .map(|running| running.key.clone())
            .collect();
        for key in stale {
            if let Some(running) = self.running.get(&key) {
                running.cancel.cancel();
                self.counts.interrupted += 1;
                affected += 1;
            }
        }
        affected
    }

    /// Cancel everything an older generation asked for.
    pub fn supersede_before(&mut self, generation: Generation) -> usize
    where
        K: Clone + Eq + Hash,
    {
        self.cancel_where(|job| job.generation < generation)
    }

    /// Forget everything: the state a re-index or a new patch leaves the queue in.
    pub fn clear(&mut self)
    where
        K: Eq + Hash,
    {
        for bucket in &mut self.buckets {
            bucket.clear();
        }
        self.queued.clear();
        for running in self.running.values() {
            running.cancel.cancel();
        }
        self.running.clear();
    }

    /// Is this key queued or running?
    #[must_use]
    pub fn holds(&self, key: &K) -> bool
    where
        K: Eq + Hash,
    {
        self.queued.contains_key(key) || self.running.contains_key(key)
    }

    /// Evict one queued job from the weakest tier that is strictly weaker than `better`.
    fn displace(&mut self, better: Priority) -> Option<Job<K>>
    where
        K: Eq + Hash,
    {
        for tier in [Priority::Prefetch, Priority::Adjacent] {
            if !better.outranks(tier) {
                // The newcomer is not worth more than this tier, so this tier keeps its job.
                continue;
            }
            let bucket = &mut self.buckets[tier.index()];
            if let Some(job) = bucket.pop_front() {
                self.queued.remove(&job.key);
                self.counts.superseded += 1;
                return Some(job);
            }
        }
        None
    }

    fn note_peak(&mut self) {
        let now = self.in_flight();
        if now > self.counts.peak_in_flight {
            self.counts.peak_in_flight = now;
        }
    }
}

impl<K: Clone + Eq + Hash> Scheduler<K> {
    /// Hand in one job.
    ///
    /// At the cap, a job that outranks a queued tier displaces one job from it: the visible window
    /// wins over a prefetch that is now behind the reviewer, which is the whole point of tiers.
    pub fn submit(&mut self, job: Job<K>) -> Admit {
        self.counts.submitted += 1;
        if self.running.contains_key(&job.key) {
            self.counts.duplicated += 1;
            return Admit::AlreadyRunning;
        }
        if self.queued.contains_key(&job.key) {
            self.counts.duplicated += 1;
            return Admit::AlreadyQueued;
        }
        while self.in_flight() >= self.max_in_flight {
            if self.displace(job.priority).is_none() {
                self.counts.dropped += 1;
                return Admit::DroppedFull;
            }
        }
        self.counts.admitted += 1;
        self.queued.insert(job.key.clone(), job.priority);
        let index = job.priority.index();
        self.buckets[index].push_back(job);
        self.note_peak();
        Admit::Queued
    }

    /// Hand in a batch and report what it cost.
    pub fn submit_all(&mut self, jobs: impl IntoIterator<Item = Job<K>>) -> SubmitReport {
        let mut report = SubmitReport::default();
        for job in jobs {
            let before = self.counts.superseded;
            match self.submit(job) {
                Admit::Queued => report.queued += 1,
                Admit::AlreadyQueued | Admit::AlreadyRunning => report.duplicated += 1,
                Admit::DroppedFull => report.dropped += 1,
            }
            report.displaced += (self.counts.superseded - before) as usize;
        }
        report
    }

    /// The keys sitting in a bucket, worst tier first.
    #[must_use]
    pub fn pending_keys(&self) -> Vec<K> {
        Priority::ALL
            .iter()
            .rev()
            .flat_map(|tier| self.buckets[tier.index()].iter().map(|job| job.key.clone()))
            .collect()
    }
}

/// What a [`Cache`] did with an insertion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Insert {
    /// Stored; the field is how many entries eviction freed.
    Cached {
        /// Entries evicted to make room.
        evicted: usize,
    },
    /// Bigger than the whole cap, so it was not stored at all.
    TooBig {
        /// What it would have cost.
        cost: usize,
        /// The cap it lost to.
        cap: usize,
    },
}

/// The counters a bounded cache keeps.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    /// Bytes retained.
    pub bytes: usize,
    /// The cap.
    pub max_bytes: usize,
    /// Entries held.
    pub entries: usize,
    /// Lookups that found something.
    pub hits: u64,
    /// Lookups that did not.
    pub misses: u64,
    /// Entries evicted to make room.
    pub evictions: u64,
    /// Insertions refused because the value exceeded the cap.
    pub refused: u64,
}

/// A byte-bounded, least-recently-used cache.
///
/// The bound is on *bytes the values cost*, as the caller reports them, which is the only bound that
/// means anything for a decode cache: an entry count lets a million one-byte entries and a
/// gigabyte one through alike. An insertion that cannot fit after eviction reports
/// [`Insert::TooBig`] instead of evicting everything — one hunk larger than the cap is a fact about
/// the hunk, and the pane prints it as one.
pub struct Cache<K, V> {
    max_bytes: usize,
    entries: HashMap<K, Entry<V>>,
    /// `(tick, key)`, oldest first, with stale records skipped on the way out. Every touch appends,
    /// so the list is O(touches) until [`Cache::compact`] folds it back.
    order: VecDeque<(u64, K)>,
    tick: u64,
    bytes: usize,
    stats: CacheStats,
}

struct Entry<V> {
    value: Arc<V>,
    cost: usize,
    tick: u64,
}

impl<K, V> Cache<K, V>
where
    K: Clone + Eq + Hash,
{
    /// An empty cache with the given byte cap.
    #[must_use]
    pub fn new(max_bytes: usize) -> Self {
        Cache {
            max_bytes,
            entries: HashMap::new(),
            order: VecDeque::new(),
            tick: 0,
            bytes: 0,
            stats: CacheStats {
                max_bytes,
                ..CacheStats::default()
            },
        }
    }

    /// The cap — a budget value a test reads back.
    #[must_use]
    pub const fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    /// Bytes retained right now.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Entries held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Is it empty?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The counters.
    #[must_use]
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            bytes: self.bytes,
            max_bytes: self.max_bytes,
            entries: self.entries.len(),
            ..self.stats
        }
    }

    /// Look a key up, marking it the freshest thing in the cache.
    pub fn get(&mut self, key: &K) -> Option<Arc<V>> {
        let Some(entry) = self.entries.get_mut(key) else {
            self.stats.misses += 1;
            return None;
        };
        self.stats.hits += 1;
        self.tick += 1;
        entry.tick = self.tick;
        let tick = entry.tick;
        let value = Arc::clone(&entry.value);
        self.push_order(tick, key);
        Some(value)
    }

    /// Is it in there? Does not count as a use.
    #[must_use]
    pub fn contains(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }

    /// The value without touching it, for a read-only pass.
    #[must_use]
    pub fn peek(&self, key: &K) -> Option<Arc<V>> {
        self.entries.get(key).map(|entry| Arc::clone(&entry.value))
    }

    /// Every key currently held, in unspecified order.
    #[must_use]
    pub fn keys(&self) -> Vec<K> {
        self.entries.keys().cloned().collect()
    }

    /// Store a value that costs `cost` bytes, evicting least-recently-used entries until it fits.
    pub fn insert(&mut self, key: K, value: Arc<V>, cost: usize) -> Insert {
        if cost > self.max_bytes {
            self.stats.refused += 1;
            return Insert::TooBig {
                cost,
                cap: self.max_bytes,
            };
        }
        // Replace-in-place first: re-decoding the same key must not evict anything.
        if let Some(old) = self.entries.get(&key) {
            self.bytes = self.bytes.saturating_sub(old.cost);
            self.entries.remove(&key);
        }
        let mut evicted = 0_usize;
        while self.bytes + cost > self.max_bytes {
            match self.evict_one() {
                Some(freed) => {
                    evicted += 1;
                    self.stats.evictions += 1;
                    self.bytes = self.bytes.saturating_sub(freed);
                }
                None => break,
            }
        }
        self.tick += 1;
        let tick = self.tick;
        self.bytes += cost;
        self.entries
            .insert(key.clone(), Entry { value, cost, tick });
        self.push_order(tick, &key);
        Insert::Cached { evicted }
    }

    /// Drop every entry the predicate names. Returns how many went.
    pub fn remove_where(&mut self, mut pred: impl FnMut(&K, &V) -> bool) -> usize {
        let doomed: Vec<K> = self
            .entries
            .iter()
            .filter(|(key, entry)| pred(key, &entry.value))
            .map(|(key, _)| key.clone())
            .collect();
        let count = doomed.len();
        for key in doomed {
            if let Some(entry) = self.entries.remove(&key) {
                self.bytes = self.bytes.saturating_sub(entry.cost);
            }
        }
        self.compact();
        count
    }

    /// Drop one entry.
    pub fn remove(&mut self, key: &K) -> bool {
        let Some(entry) = self.entries.remove(key) else {
            return false;
        };
        self.bytes = self.bytes.saturating_sub(entry.cost);
        self.compact();
        true
    }

    /// Empty it.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
        self.bytes = 0;
    }

    fn evict_one(&mut self) -> Option<usize> {
        while let Some((tick, key)) = self.order.pop_front() {
            let matches = self
                .entries
                .get(&key)
                .is_some_and(|entry| entry.tick == tick);
            if !matches {
                // Touched since this record was written: the fresher record is further back.
                continue;
            }
            let entry = self.entries.remove(&key)?;
            return Some(entry.cost);
        }
        None
    }

    fn push_order(&mut self, tick: u64, key: &K) {
        self.order.push_back((tick, key.clone()));
        // The order list carries one record per touch, so a long-lived cache would grow it without
        // bound. Fold it back to one record per entry once it has drifted to twice the entries:
        // amortised O(1) per touch, and the eviction order survives because ticks are monotone.
        if self.order.len() > self.entries.len() * 2 + 64 {
            self.compact();
        }
    }

    fn compact(&mut self) {
        self.order.retain(|(tick, key)| {
            self.entries
                .get(key)
                .is_some_and(|entry| entry.tick == *tick)
        });
    }
}

/// A [`Cache`], a [`Scheduler`] and a mailbox, with the lock discipline the frame loop needs.
///
/// One [`Engine`] is cheap to clone and is handed to the UI thread and to every worker. Locks are
/// taken one at a time and never across a decode: a worker pops a job, releases the queue, reads its
/// bytes, then takes the cache to file the result. A UI thread asking for a cache hit therefore never
/// waits behind a blob decode, which is the entire point of moving the decode off the UI thread.
pub struct Engine<K, V: Cost>
where
    K: Clone + Eq + Hash + Ord + Send + 'static,
    V: Send + Sync + 'static,
{
    inner: Arc<Shared<K, V>>,
}

impl<K, V: Cost> Clone for Engine<K, V>
where
    K: Clone + Eq + Hash + Ord + Send + 'static,
    V: Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        Engine {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<K, V: Cost> fmt::Debug for Engine<K, V>
where
    K: Clone + Eq + Hash + Ord + Send + 'static,
    V: Send + Sync + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Engine")
            .field("task", &self.inner.task)
            .field("stats", &self.stats())
            .finish_non_exhaustive()
    }
}

/// The decode function: turn a key into a value, checking the token as it goes.
pub type Work<K, V> = Arc<dyn Fn(&Job<K>, &Cancel) -> Outcome<V> + Send + Sync>;

struct Shared<K, V: Cost>
where
    K: Clone + Eq + Hash + Ord + Send + 'static,
    V: Send + Sync + 'static,
{
    task: &'static str,
    queue: Mutex<Scheduler<K>>,
    cache: Mutex<Cache<K, V>>,
    /// The last failure per key, so a pane can print why a hunk is not there. Bounded and
    /// oldest-out: these are notes for a footer, not a log.
    failures: Mutex<HashMap<K, String>>,
    failure_order: Mutex<VecDeque<K>>,
    /// One slot per worker, with the id that worker removes on the way out; `submit` nudges each and
    /// a worker that finds nothing re-parks.
    wakers: Mutex<Vec<(u64, SyncSender<()>)>>,
    next_worker: AtomicU64,
    bus: Mutex<Option<Arc<Bus>>>,
    /// How much work the current request needs, for a status line that can print a percentage.
    total: AtomicUsize,
    done: AtomicUsize,
}

/// Everything a caller can ask an engine about at once.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EngineStats {
    /// Jobs queued.
    pub queued: usize,
    /// Jobs running.
    pub running: usize,
    /// The in-flight cap.
    pub max_in_flight: usize,
    /// The cache's own counters.
    pub cache: CacheStats,
    /// The queue's counters.
    pub counts: Counters,
}

impl<K, V> Engine<K, V>
where
    K: Clone + Eq + Hash + Ord + Send + 'static,
    V: Cost + Send + Sync + 'static,
{
    /// An engine for one kind of work.
    #[must_use]
    pub fn new(task: &'static str, cache_bytes: usize, max_in_flight: usize) -> Self {
        Engine {
            inner: Arc::new(Shared {
                task,
                queue: Mutex::new(Scheduler::new(max_in_flight)),
                cache: Mutex::new(Cache::new(cache_bytes)),
                failures: Mutex::new(HashMap::new()),
                failure_order: Mutex::new(VecDeque::new()),
                wakers: Mutex::new(Vec::new()),
                next_worker: AtomicU64::new(1),
                bus: Mutex::new(None),
                total: AtomicUsize::new(0),
                done: AtomicUsize::new(0),
            }),
        }
    }

    /// The task name the status line prints.
    #[must_use]
    pub fn task(&self) -> &'static str {
        self.inner.task
    }

    /// Attach the queue the frame loop reads, so a finished job can wake a draw.
    pub fn attach_bus(&self, bus: Arc<Bus>) {
        if let Ok(mut slot) = self.inner.bus.lock() {
            *slot = Some(bus);
        }
    }

    /// Say how much work the current request needs, so the status line can print a percentage
    /// instead of `unknown %`.
    pub fn set_total(&self, total: usize) {
        self.inner.total.store(total, Ordering::Relaxed);
        self.inner.done.store(0, Ordering::Relaxed);
    }

    /// Hand in jobs, and wake a worker if one is listening.
    pub fn submit(&self, jobs: impl IntoIterator<Item = Job<K>>) -> SubmitReport {
        let report = match self.inner.queue.lock() {
            Ok(mut queue) => queue.submit_all(jobs),
            Err(_) => SubmitReport::default(),
        };
        if report.queued > 0 {
            self.wake();
        }
        report
    }

    /// Cancel queued work the predicate names and interrupt running work it matches.
    pub fn cancel_where(&self, pred: impl FnMut(&Job<K>) -> bool) -> usize {
        match self.inner.queue.lock() {
            Ok(mut queue) => queue.cancel_where(pred),
            Err(_) => 0,
        }
    }

    /// Forget every queued job, cancel every running one and drop every cached value.
    pub fn reset(&self) {
        if let Ok(mut queue) = self.inner.queue.lock() {
            queue.clear();
        }
        if let Ok(mut cache) = self.inner.cache.lock() {
            cache.clear();
        }
        if let (Ok(mut failures), Ok(mut order)) =
            (self.inner.failures.lock(), self.inner.failure_order.lock())
        {
            failures.clear();
            order.clear();
        }
        self.inner.done.store(0, Ordering::Relaxed);
    }

    /// The cached value for a key, if there is one.
    pub fn cached(&self, key: &K) -> Option<Arc<V>> {
        match self.inner.cache.lock() {
            Ok(mut cache) => cache.get(key),
            Err(_) => None,
        }
    }

    /// Every key the cache holds, in unspecified order. A caller that wants "how much of origin X is
    /// warm" uses this; the cache does not index by origin because nothing on a frame path asks.
    #[must_use]
    pub fn keys(&self) -> Vec<K> {
        match self.inner.cache.lock() {
            Ok(cache) => cache.keys(),
            Err(_) => Vec::new(),
        }
    }

    /// Does the cache hold it? Does not count as a hit.
    #[must_use]
    pub fn holds(&self, key: &K) -> bool {
        match self.inner.cache.lock() {
            Ok(cache) => cache.contains(key),
            Err(_) => false,
        }
    }

    /// Is this key queued or running — i.e. is a result still on the way? A pane prints `decoding`
    /// when this is true and a reason when it is not, which is the difference between a wait and a
    /// dead end.
    #[must_use]
    pub fn pending(&self, key: &K) -> bool {
        match self.inner.queue.lock() {
            Ok(queue) => queue.holds(key),
            Err(_) => false,
        }
    }

    /// Store a value directly, bypassing the queue. A warm-up that produces several values from one
    /// job uses this for the ones after the first.
    pub fn cache_direct(&self, key: &K, value: Arc<V>) -> Insert {
        let cost = value.cost();
        match self.inner.cache.lock() {
            Ok(mut cache) => cache.insert(key.clone(), value, cost),
            Err(_) => Insert::TooBig { cost, cap: 0 },
        }
    }

    /// Drop every cached value the predicate names — the shape of "this origin's bytes are gone".
    pub fn remove_where(&self, pred: impl FnMut(&K, &V) -> bool) -> usize {
        match self.inner.cache.lock() {
            Ok(mut cache) => cache.remove_where(pred),
            Err(_) => 0,
        }
    }

    /// Why a key failed, if it did.
    #[must_use]
    pub fn failure(&self, key: &K) -> Option<String> {
        let failures = self.inner.failures.lock().ok()?;
        failures.get(key).cloned()
    }

    /// The counters, from both halves at once — the pair only means something together.
    #[must_use]
    pub fn stats(&self) -> EngineStats {
        let (queued, running, max_in_flight, counts) = match self.inner.queue.lock() {
            Ok(queue) => (
                queue.queued(),
                queue.running(),
                queue.max_in_flight(),
                queue.counts,
            ),
            Err(_) => (0, 0, 0, Counters::default()),
        };
        EngineStats {
            queued,
            running,
            max_in_flight,
            cache: match self.inner.cache.lock() {
                Ok(cache) => cache.stats(),
                Err(_) => CacheStats::default(),
            },
            counts,
        }
    }

    /// Run the next job on the calling thread; `true` means a job ran.
    ///
    /// This is the deterministic path the tests drive: same queue, same cache, same bookkeeping, no
    /// thread and no timing.
    pub fn run_one(&self, work: &Work<K, V>) -> bool {
        let Some(job) = self.take() else {
            return false;
        };
        self.execute(&job, work);
        true
    }

    /// Run jobs until the queue is empty — a test's stand-in for a worker pool.
    pub fn drain(&self, work: &Work<K, V>) -> usize {
        let mut ran = 0_usize;
        while self.run_one(work) {
            ran += 1;
            if ran > 1_000_000 {
                break; // a job that re-submits itself would otherwise hang a test
            }
        }
        ran
    }

    /// Take the best job off the queue. Public because a caller that wants its own thread shape can
    /// compose [`Engine::execute`] with it.
    #[must_use]
    pub fn take(&self) -> Option<Running<K>> {
        self.inner.queue.lock().ok()?.take_next()
    }

    /// Run one job that was taken, file the result, and wake the frame.
    ///
    /// A job whose token was set while it ran files as nothing, whatever it produced. Cooperation is
    /// best effort — a worker checks the token at its own checkpoints, and a decode that never reaches
    /// one would otherwise deliver itself into the cache and wake a frame for a window nobody is
    /// looking at any more. Dropping the result is the whole point of cancelling.
    pub fn execute(&self, job: &Running<K>, work: &Work<K, V>) {
        let request = Job {
            key: job.key.clone(),
            priority: job.priority,
            generation: job.generation,
        };
        let outcome = work(&request, &job.cancel);
        if job.cancel.is_cancelled() {
            self.file(job, Outcome::Nothing);
            return;
        }
        self.file(job, outcome);
    }

    /// File an outcome: cache it or note the failure, retire the job, and tell the bus.
    pub fn file(&self, job: &Running<K>, outcome: Outcome<V>) {
        match outcome {
            Outcome::Ready(value) => {
                let value = Arc::new(value);
                let cost = value.cost();
                let mut note = format!("{} · {}", self.inner.task, job.priority.name());
                if let Ok(mut cache) = self.inner.cache.lock()
                    && let Insert::TooBig { cost, cap } =
                        cache.insert(job.key.clone(), Arc::clone(&value), cost)
                {
                    self.note_failure(
                        &job.key,
                        format!("one piece costs {cost} B, over the {cap} B decode cache"),
                    );
                    note = format!("{} · too large for the decode cache", self.inner.task);
                }
                if let Ok(mut queue) = self.inner.queue.lock() {
                    queue.complete(&job.key);
                }
                self.done(1);
                self.notify(note);
            }
            Outcome::Nothing => {
                if let Ok(mut queue) = self.inner.queue.lock() {
                    queue.complete(&job.key);
                }
                self.done(1);
            }
            Outcome::Failed(why) => {
                self.note_failure(&job.key, why);
                if let Ok(mut queue) = self.inner.queue.lock() {
                    queue.complete(&job.key);
                    queue.counts.failed += 1;
                }
                self.done(1);
                self.notify(format!("{} · a piece failed", self.inner.task));
            }
        }
    }

    /// Park `workers` threads on this engine, each turning a panic into a [`WorkerMsg::Died`] — the
    /// same rule [`crate::event::spawn_worker`] applies, because a dead decoder is a stated partial
    /// and not a dead terminal (ADR-006 §8.1).
    ///
    /// The handles are returned, not dropped: a caller that wants its threads gone before the process
    /// exits can drop the engine and join. A worker exits when the engine is dropped, because the
    /// waker's sender lives in the engine.
    pub fn spawn(
        &self,
        bus: Option<Arc<Bus>>,
        workers: usize,
        work: Work<K, V>,
    ) -> Vec<JoinHandle<()>> {
        if let Some(bus) = bus {
            self.attach_bus(bus);
        }
        let mut handles = Vec::with_capacity(workers.max(1));
        for _ in 0..workers.max(1) {
            let shared = Arc::clone(&self.inner);
            let work = Arc::clone(&work);
            let id = shared.next_worker.fetch_add(1, Ordering::Relaxed);
            let (tx, rx) = sync_channel::<()>(1);
            if let Ok(mut wakers) = shared.wakers.lock() {
                wakers.push((id, tx));
            }
            handles.push(std::thread::spawn(move || {
                let engine = Engine {
                    inner: Arc::clone(&shared),
                };
                worker_loop(&engine, &shared, &rx, &work);
                if let Ok(mut wakers) = shared.wakers.lock() {
                    wakers.retain(|(waker, _)| *waker != id);
                }
            }));
        }
        handles
    }

    fn wake(&self) {
        if let Ok(wakers) = self.inner.wakers.lock() {
            for (_, waker) in wakers.iter() {
                // A one-slot channel: a burst of submits is one wake, which is what a coalesced
                // frame deserves.
                let _ = waker.try_send(());
            }
        }
    }

    fn done(&self, n: usize) {
        self.inner.done.fetch_add(n, Ordering::Relaxed);
    }

    fn note_failure(&self, key: &K, why: String) {
        let (Ok(mut failures), Ok(mut order)) =
            (self.inner.failures.lock(), self.inner.failure_order.lock())
        else {
            return;
        };
        if !failures.contains_key(key) {
            order.push_back(key.clone());
            while order.len() > 64 {
                if let Some(oldest) = order.pop_front() {
                    failures.remove(&oldest);
                }
            }
        }
        failures.insert(key.clone(), why);
    }

    /// Wake the loop with a progress line. One message per finished job, which the loop's coalescer
    /// collapses into one frame however fast the worker is. When the queue runs dry the same worker
    /// says so, so the status line goes back to saying nothing instead of freezing at the last count.
    fn notify(&self, _note: String) {
        let Some(bus) = self.inner.bus.lock().ok().and_then(|slot| slot.clone()) else {
            return;
        };
        let done = self.inner.done.load(Ordering::Relaxed) as u64;
        let total = self.inner.total.load(Ordering::Relaxed);
        let idle = self
            .inner
            .queue
            .lock()
            .map(|queue| queue.is_empty())
            .unwrap_or(true);
        let message = if idle {
            WorkerMsg::Quiet {
                task: self.inner.task,
            }
        } else {
            WorkerMsg::Progress {
                task: self.inner.task,
                done,
                of: match total {
                    0 => None,
                    total => Some(total as u64),
                },
            }
        };
        let _ = bus.send(Message::Worker(message));
    }

    fn report_death(&self, why: &str) {
        let Some(bus) = self.inner.bus.lock().ok().and_then(|slot| slot.clone()) else {
            return;
        };
        let _ = bus.send(Message::Worker(WorkerMsg::Died {
            task: self.inner.task,
            why: why.to_owned(),
        }));
    }
}

/// One worker's loop: drain the queue, park, repeat, and never let a panic escape the thread.
fn worker_loop<K, V>(
    engine: &Engine<K, V>,
    shared: &Arc<Shared<K, V>>,
    rx: &Receiver<()>,
    work: &Work<K, V>,
) where
    K: Clone + Eq + Hash + Ord + Send + 'static,
    V: Cost + Send + Sync + 'static,
{
    loop {
        // A spurious wake is fine and expected: `try_recv` drains a burst, and the loop re-reads the
        // queue instead of counting messages.
        while rx.try_recv().is_ok() {}
        if let Some(job) = engine.take() {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                engine.execute(&job, work);
            }));
            if let Err(payload) = result {
                let why = crate::event::panic_message(&payload);
                if let Ok(mut queue) = shared.queue.lock() {
                    queue.complete(&job.key);
                }
                engine.report_death(&why);
            }
            continue;
        }
        // `recv` ends with `Err` when every sender is gone, which is the engine saying it is done
        // with this thread.
        if rx.recv().is_err() {
            break;
        }
    }
}
