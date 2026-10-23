//! `memory.rs` — the ceilings, and the two containers that obey them (`reco-93w`).
//!
//! The claim this module exists to make true is the one ADR-004 §11.8 makes about RSS budgets and that
//! `reco-93w` states as its own: **peak resident memory is a function of configuration, not of
//! repository size.** A budget printed in a JSON file is not that claim; it is a measurement of whatever
//! the code happened to do. The claim needs a container that refuses to grow past a number somebody
//! chose, and a number somebody chose has to arrive as an argument — so the ceilings are values, read
//! by `fathom-cli` like every other config value (ADR-002 §4.5), and every structure that holds a
//! repository's bytes in this process is given one.
//!
//! Three pieces, and they are deliberately small:
//!
//! * [`Ceilings`] — the knobs, in bytes, with the range this build will honour. A value outside the range
//!   is clamped and named, never honoured silently: a cloned `fathom.toml` is hostile input (§5 of
//!   `docs/concepts/config.md`), and a repository that can ask for 4 TiB of page cache can take a CI
//!   runner down.
//! * [`BoundedCache`] — a byte-weighted LRU. Every entry carries a weight and a class; eviction is by
//!   least-recent-use, and **what got evicted is counted per class**, because "the cache is bounded" is
//!   only useful once you can say what the bound cost. Eviction here is not data loss: everything these
//!   caches hold is class R (ADR-004 §10), re-derivable from the durable store, which is what makes a
//!   spill cheap rather than dramatic.
//! * [`Queue`] — a bounded, blocking hand-off. This is back-pressure: a producer whose consumer has
//!   fallen behind waits instead of allocating, so a slow writer shrinks the pipeline's footprint
//!   rather than growing it. A queue with no ceiling is just a heap with extra steps, and the heap is
//!   the part that gets OOM-killed.
//!
//! # What is deliberately not here
//!
//! **No allocator hooks, no `mmap`, no `unsafe`.** This module bounds what the program asks for
//! *by asking for less*, which is the only mechanism available in a crate that forbids `unsafe`
//! (ADR-002 §4) and the only one that survives a change of allocator.
//!
//! **No clock.** LRU ordering is a monotonic tick ([`BoundedCache::insert`]'s calls, in order), not a
//! timestamp, so a cache's behaviour is a function of the access sequence alone — which is what lets
//! these containers be golden-tested like everything else in this crate (ADR-001 §7.4). A *durable*
//! LRU is a different thing and does use a clock: ADR-004 §8.6's eviction runs on `last_used_utc`, in
//! `fathom-store`, where the wall clock arrives as an argument.
//!
//! **No counters written to a database.** Eviction and back-pressure counts are returned as values
//! ([`CacheStats`], [`QueueStats`]); the caller decides whether they are a printed line, a `counter`
//! row, or nothing.

use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// The largest ceiling this build will honour for any single bounded structure, in bytes.
///
/// Four gibibytes, because that is the point at which "the operator asked for it" stops being an
/// explanation on the machines this product is aimed at, and because a cloned repository's
/// `fathom.toml` gets a vote in this number (§5 of `docs/concepts/config.md`).
pub const MAX_CEILING_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// The largest queue depth this build will honour, in items.
///
/// Sixty-four, which is thirty-two times the default: past that the queue is not a buffer any more, it
/// is a cache with none of a cache's accounting.
pub const MAX_QUEUE_DEPTH: usize = 64;

/// One mebibyte, spelled once so the ceilings below read as the numbers the config file uses.
pub const MIB: u64 = 1024 * 1024;

/// The knobs, in bytes, as one process resolved them.
///
/// Every field is a *ceiling*, not a target: a structure under its ceiling does nothing differently, and
/// the behaviour that matters is what happens at the ceiling — evict ([`BoundedCache`]) or wait
/// ([`Queue`]) — never grow.
///
/// The values arrive from `fathom-cli`, which reads `fathom.toml` and the environment and hands this
/// struct downhill as data (ADR-002 §4.5). Nothing in this module opens a file or reads `FATHOM_*`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ceilings {
    /// The in-process analysis payload cache — the decoded results a warm run reads back from the store
    /// on every lookup. Eviction is lossless: the durable `cache`/`blob` rows are the spill.
    pub analysis_cache_bytes: u64,
    /// The in-process working set of materialised graph snapshots (one revision's graph, in memory).
    /// Eviction is lossless for the same reason: a snapshot is a fold of durable `graph_delta` rows.
    pub graph_snapshot_bytes: u64,
    /// The decode buffer ceiling: the largest object a worker will inflate at one time. This is
    /// `ingest.size_cap_mib` in the config file and `Limits::blob_memory_cap` (`fathom-ingest`) as a
    /// value — one number, one meaning: a bigger object is recorded as oversized, not read.
    pub decode_bytes: u64,
    /// How many batches may be normalised and waiting for the writer before the pipeline waits. This is
    /// the back-pressure knob: peak spine memory is roughly `queue_batches × commits_per_batch`, and
    /// both are configuration.
    pub queue_batches: usize,
    /// The durable ceiling on the `cache` table, in bytes — ADR-004 §8.6's `cache.budget.mib`. Unlike
    /// the three above this is disk, not RSS, and it is what makes `store.size.mib` a bound rather than
    /// a hope: the cache measured 51 % of store bytes in the fixture (§13.4).
    pub durable_cache_bytes: u64,
    /// How recently a durable cache row must have been used to be protected from §8.6's eviction, in
    /// milliseconds. Zero disables the protection set, which is what a CI job that wants a hard ceiling
    /// asked for.
    pub protect_recent_ms: i64,
}

/// The keys [`Ceilings::set`] answers to, in documentation order.
///
/// These are the *slot* names, not the `fathom.toml` paths: `[graph] snapshot_cache_mib` appears here as
/// `snapshot_cache_mib`, and `cache.budget.mib` as `cache_budget_mib` because `mib` alone would name two
/// different ceilings. `fathom-cli` owns the mapping from config key to slot, because it owns reading
/// the file (ADR-002 §4.5).
pub const KEYS: [&str; 6] = [
    "analyzer_working_set_mib",
    "snapshot_cache_mib",
    "size_cap_mib",
    "queue_batches",
    "cache_budget_mib",
    "protect_recent_minutes",
];

impl Default for Ceilings {
    /// The built-in defaults — each one the `defaults` layer's value for its key, kept in step with
    /// `benchmarks/budgets.json` by `tests/memory_ceilings.rs` rather than by hope.
    fn default() -> Self {
        Ceilings {
            // `rss.analyzer_working_set.mib` at the `medium` size class.
            analysis_cache_bytes: 128 * MIB,
            // A revision's graph measured 22 335 B/revision in spike `s3`; 32 MiB holds a session's
            // worth of navigation without asking for more, and a monorepo's does not push the peak.
            graph_snapshot_bytes: 32 * MIB,
            // `ingest.size_cap_mib`, the shipped file-size cap.
            decode_bytes: 8 * MIB,
            // Two batches in flight: one being written, one being normalised. One serialises the two
            // stages; three buys nothing over two that a cache would not buy better.
            queue_batches: 2,
            // `cache.budget.mib`.
            durable_cache_bytes: 512 * MIB,
            // A day. An episode that has been looked at today is a session, not a cold row.
            protect_recent_ms: 24 * 60 * 60 * 1000,
        }
    }
}

impl Ceilings {
    /// Every ceiling at once, clamped to what this build will honour.
    ///
    /// Clamping rather than refusing is the right failure mode here: these are budgets, and a repository
    /// that sets one to a nonsense value gets the nearest thing that works and is told about it
    /// ([`Ceilings::set`], and `fathom config explain`'s notices).
    #[must_use]
    pub fn clamped(mut self) -> Self {
        for field in [
            &mut self.analysis_cache_bytes,
            &mut self.graph_snapshot_bytes,
            &mut self.decode_bytes,
            &mut self.durable_cache_bytes,
        ] {
            *field = (*field).clamp(MIB, MAX_CEILING_BYTES);
        }
        self.queue_batches = self.queue_batches.clamp(1, MAX_QUEUE_DEPTH);
        self.protect_recent_ms = self.protect_recent_ms.max(0);
        self
    }

    /// Set one ceiling from a whole-number config value, by the key's last segment.
    ///
    /// `Ok(Some(line))` is the notice a caller should print when the value needed clamping;
    /// `Ok(None)` is "taken as written"; `Err` names an unknown key so `fathom config explain` can
    /// distinguish a typo from a value it did not read at all.
    pub fn set(&mut self, key: &str, value: i64) -> Result<Option<String>, String> {
        /// Turn a `*_mib` config value into bytes, clamped to what this build honours, and say whether
        /// the clamp bit — `(bytes, notice)`, with the notice present only when the number moved.
        fn mib(key: &str, field: &str, value: i64) -> (u64, Option<String>) {
            if value <= 0 {
                return (
                    0,
                    Some(format!(
                        "{key} = {value} is not a budget; {field} keeps its default"
                    )),
                );
            }
            let said = value as u64;
            let wide = said.saturating_mul(MIB);
            let bytes = wide.clamp(MIB, MAX_CEILING_BYTES);
            let notice = (wide != bytes).then(|| {
                format!(
                    "{key} = {value} MiB is past what this build honours; {field} is capped at {} MiB",
                    bytes / MIB
                )
            });
            (bytes, notice)
        }
        match key {
            "analyzer_working_set_mib" => {
                let (bytes, notice) = mib(key, "the analysis cache", value);
                self.analysis_cache_bytes = bytes;
                Ok(notice)
            }
            "snapshot_cache_mib" => {
                let (bytes, notice) = mib(key, "the graph snapshot cache", value);
                self.graph_snapshot_bytes = bytes;
                Ok(notice)
            }
            "size_cap_mib" => {
                let (bytes, notice) = mib(key, "the decode buffer", value);
                self.decode_bytes = bytes;
                Ok(notice)
            }
            "cache_budget_mib" => {
                let (bytes, notice) = mib(key, "the durable cache", value);
                self.durable_cache_bytes = bytes;
                Ok(notice)
            }
            "queue_batches" => {
                if value < 1 {
                    return Ok(Some(format!(
                        "queue_batches = {value} would deadlock the writer; one batch in flight is the floor"
                    )));
                }
                let depth = usize::try_from(value)
                    .unwrap_or(usize::MAX)
                    .min(MAX_QUEUE_DEPTH);
                self.queue_batches = depth;
                Ok((usize::try_from(value).ok() != Some(depth))
                    .then(|| format!("queue_batches = {value} is capped at {MAX_QUEUE_DEPTH}")))
            }
            "protect_recent_minutes" => {
                if value < 0 {
                    return Ok(Some(format!(
                        "protect_recent_minutes = {value} is negative; the protection window is {} ms",
                        self.protect_recent_ms
                    )));
                }
                self.protect_recent_ms = value.saturating_mul(60 * 1000);
                Ok(None)
            }
            other => Err(format!("unknown memory ceiling key `{other}`")),
        }
    }

    /// The ceilings as bytes, keyed by the config key, for `--json` and for a printed table.
    #[must_use]
    pub fn observe(&self) -> BTreeMap<&'static str, u64> {
        BTreeMap::from([
            ("analyzer_working_set_mib", self.analysis_cache_bytes / MIB),
            ("snapshot_cache_mib", self.graph_snapshot_bytes / MIB),
            ("size_cap_mib", self.decode_bytes / MIB),
            ("queue_batches", self.queue_batches as u64),
            ("cache_budget_mib", self.durable_cache_bytes / MIB),
            (
                "protect_recent_minutes",
                (self.protect_recent_ms / 60_000) as u64,
            ),
        ])
    }
}

/// What one [`BoundedCache`] did, and what its ceiling cost.
///
/// Every field is a count of something a reviewer can act on: hits and misses say whether the ceiling
/// is big enough, evictions and their bytes say what was thrown away, and [`CacheStats::refusals`]
/// says the uncomfortable one — an entry that would never have fitted, so it was never held.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CacheStats {
    /// The byte ceiling in force.
    pub limit_bytes: u64,
    /// Bytes currently held.
    pub bytes: u64,
    /// Entries currently held.
    pub entries: usize,
    /// The most bytes ever held at one time — the number the ceiling is supposed to be the answer to.
    pub high_water_bytes: u64,
    /// Lookups that found something.
    pub hits: u64,
    /// Lookups that did not.
    pub misses: u64,
    /// Entries inserted, including ones that inserted and were then themselves evicted.
    pub insertions: u64,
    /// Entries evicted to stay under the ceiling.
    pub evictions: u64,
    /// Bytes freed by eviction.
    pub evicted_bytes: u64,
    /// Entries refused because one entry is larger than the whole ceiling.
    pub refusals: u64,
    /// Bytes refused, for the same reason.
    pub refused_bytes: u64,
    /// Evictions, counted by the class the entry was filed under.
    pub evicted_by_class: BTreeMap<String, u64>,
}

impl CacheStats {
    /// The hit rate as a percentage, or `None` when nothing was asked — `fathom stats` prints
    /// `n/a (nothing asked)` rather than `0.0 %` for a cache nobody used.
    #[must_use]
    pub fn hit_rate(&self) -> Option<f64> {
        let asked = self.hits + self.misses;
        (asked > 0).then(|| self.hits as f64 * 100.0 / asked as f64)
    }

    /// What share of the ceiling is in use, as whole percent (may exceed 100 only if the ceiling moved).
    #[must_use]
    pub fn fill_percent(&self) -> u64 {
        if self.limit_bytes == 0 {
            return 0;
        }
        self.bytes.saturating_mul(100) / self.limit_bytes
    }
}

/// A byte-weighted LRU with a ceiling and an eviction ledger.
///
/// Keys are whatever the caller can hash and order; values are carried behind their weight, which the
/// caller supplies because only the caller knows what a decoded payload costs the process (the map's
/// own overhead is a rounding error next to a payload, and pretending to account for it precisely is
/// how a cache ends up claiming 4 KiB for a 4 MiB value).
///
/// Interior-mutable on purpose: a cache that forces `&mut self` cannot sit inside a reader that is
/// itself shared, and the alternative — a cache that is only reachable mutably — is how every caller
/// ends up with its own.
///
/// ```
/// use fathom_core::memory::BoundedCache;
///
/// // Two 60-byte entries fit in 100 bytes; inserting the second evicts the first.
/// let cache = BoundedCache::<&str, String>::new(100);
/// cache.insert("a", "one".to_owned(), 60, "structure");
/// cache.insert("b", "two".to_owned(), 60, "structure");
/// assert_eq!(cache.get_with(&"a", |_| true), None);
/// assert_eq!(cache.get_with(&"b", |v| v.clone()), Some("two".to_owned()));
/// let stats = cache.observe();
/// assert_eq!(stats.evictions, 1);
/// assert_eq!(stats.evicted_by_class.get("structure"), Some(&1));
/// ```
#[derive(Debug)]
pub struct BoundedCache<K, V> {
    limit: AtomicU64,
    inner: Mutex<CacheInner<K, V>>,
}

#[derive(Debug)]
struct CacheInner<K, V> {
    entries: HashMap<K, Entry<V>>,
    /// LRU order: `(tick, key)`, oldest first. The tick is a counter of insertions and hits, not a
    /// clock — see this module's note about determinism.
    order: BTreeMap<(u64, K), ()>,
    tick: u64,
    bytes: u64,
    hits: u64,
    misses: u64,
    insertions: u64,
    evictions: u64,
    evicted_bytes: u64,
    high_water_bytes: u64,
    refusals: u64,
    refused_bytes: u64,
    evicted_by_class: BTreeMap<String, u64>,
}

#[derive(Debug)]
struct Entry<V> {
    value: V,
    weight: u64,
    class: String,
    /// The tick this entry was last used at — the same value its `order` key carries. Kept on the entry
    /// so replacing or removing a key can find its own LRU slot instead of scanning for it.
    tick: u64,
}

impl<K, V> BoundedCache<K, V>
where
    K: Clone + Eq + Hash + Ord,
{
    /// An empty cache with `limit` bytes of room. A limit of zero is allowed and means "hold nothing":
    /// every insert evicts itself, which is the honest spelling of a ceiling someone set to zero.
    #[must_use]
    pub fn new(limit: u64) -> Self {
        BoundedCache {
            limit: AtomicU64::new(limit),
            inner: Mutex::new(CacheInner {
                entries: HashMap::new(),
                order: BTreeMap::new(),
                tick: 0,
                bytes: 0,
                hits: 0,
                misses: 0,
                insertions: 0,
                evictions: 0,
                evicted_bytes: 0,
                high_water_bytes: 0,
                refusals: 0,
                refused_bytes: 0,
                evicted_by_class: BTreeMap::new(),
            }),
        }
    }

    /// The ceiling in force.
    #[must_use]
    pub fn limit(&self) -> u64 {
        self.limit.load(Ordering::Relaxed)
    }

    /// Move the ceiling, evicting as necessary to obey the new one, and report what that cost.
    ///
    /// Lowering a ceiling is a real operation, not a wish: the bytes have to go now, or the ceiling is
    /// a suggestion.
    pub fn set_limit(&self, limit: u64) -> CacheStats {
        self.limit.store(limit, Ordering::Relaxed);
        let mut inner = self.lock();
        evict_to_fit(&mut inner, limit);
        self.stats_with(&inner)
    }

    /// Look one entry up, mapping it through `f` while it is still borrowed.
    ///
    /// The closure form exists because the obvious `-> Option<&V>` needs the lock held past the return,
    /// and `-> Option<V>` forces a clone of exactly the bytes this cache exists to avoid re-deriving.
    /// A hit counts a hit; a miss counts a miss; both move the entry (or not) to the front of the LRU.
    pub fn get_with<R>(&self, key: &K, f: impl FnOnce(&V) -> R) -> Option<R> {
        let mut guard = self.lock();
        // One deref of the guard, so the field borrows below are disjoint rather than six mutable borrows
        // of the same `MutexGuard`.
        let inner = &mut *guard;
        let used = match inner.entries.get(key) {
            Some(entry) => entry.tick,
            None => {
                inner.misses += 1;
                return None;
            }
        };
        inner.hits += 1;
        inner.tick += 1;
        let tick = inner.tick;
        inner.order.remove(&(used, key.clone()));
        inner.order.insert((tick, key.clone()), ());
        // Re-find the entry after the bookkeeping: the borrow above ended with the `match`, and the two
        // maps are separate structures, so the value lookup is the cheap half.
        inner.entries.get_mut(key).map(|entry| {
            entry.tick = tick;
            f(&entry.value)
        })
    }

    /// Whether a key is present, without counting it as a use or returning the value.
    #[must_use]
    pub fn contains(&self, key: &K) -> bool {
        self.lock().entries.contains_key(key)
    }

    /// Insert `value`, weighing it `weight` bytes and filing it under `class` for the eviction ledger.
    ///
    /// Returns the bytes evicted by this insert — which includes the case where the entry just inserted
    /// was the one evicted, because one entry does not fit under the ceiling on its own. That case is
    /// counted in [`CacheStats::refusals`] as well as being an eviction-shaped event, because "it was
    /// inserted and is already gone" is exactly what a caller needs to know and would otherwise never
    /// see.
    pub fn insert(&self, key: K, value: V, weight: u64, class: &str) -> u64 {
        let limit = self.limit();
        let mut inner = self.lock();
        inner.insertions += 1;
        if weight > limit {
            // The unit of work is bigger than the whole ceiling. No eviction policy fixes that: the
            // choice is between holding it and refusing it, so it is refused and counted, and the
            // operator sees a refusal count where the alternative was a cache that flushed itself.
            inner.refusals += 1;
            inner.refused_bytes = inner.refused_bytes.saturating_add(weight);
            return 0;
        }
        inner.tick += 1;
        let tick = inner.tick;
        if let Some(previous) = inner.entries.remove(&key) {
            inner.order.remove(&(previous.tick, key.clone()));
            inner.bytes = inner.bytes.saturating_sub(previous.weight);
        }
        let evicted_before = inner.evicted_bytes;
        // Room is made *before* the bytes are added. A cache that inserts and then trims has a peak of
        // `limit + weight` — a ceiling the program never asked for, and exactly the kind of one the
        // `high_water_bytes` counter exists to catch. Making room first means the resident total is
        // never above the limit, so the high-water mark is evidence rather than an approximation.
        evict_to_fit(&mut inner, limit.saturating_sub(weight));
        inner.bytes = inner.bytes.saturating_add(weight);
        inner.entries.insert(
            key.clone(),
            Entry {
                value,
                weight,
                class: class.to_owned(),
                tick,
            },
        );
        inner.order.insert((tick, key), ());
        if inner.bytes > inner.high_water_bytes {
            inner.high_water_bytes = inner.bytes;
        }
        inner.evicted_bytes.saturating_sub(evicted_before)
    }

    /// Remove one entry, returning its weight (or `None` if it was not there).
    pub fn remove(&self, key: &K) -> Option<u64> {
        let mut inner = self.lock();
        let entry = inner.entries.remove(key)?;
        inner.bytes = inner.bytes.saturating_sub(entry.weight);
        inner.order.remove(&(entry.tick, key.clone()));
        Some(entry.weight)
    }

    /// Drop everything, keeping the counters: a ceiling change that empties the cache should still be
    /// able to say what it emptied.
    pub fn clear(&self) {
        let mut inner = self.lock();
        inner.entries.clear();
        inner.order.clear();
        inner.bytes = 0;
    }

    /// Discard everything held, keeping the counters.
    ///
    /// The difference between this and eviction is *why*: eviction is a bound being enforced and is counted
    /// as such, while this is a phase ending and the bytes going back on purpose. The counters survive
    /// because they describe the run — a cleared cache that also cleared its history would make a released
    /// cache indistinguishable from an unused one in the report.
    #[must_use]
    pub fn drain(&self) -> u64 {
        let mut inner = self.lock();
        let freed = inner.bytes;
        inner.entries.clear();
        inner.order.clear();
        inner.bytes = 0;
        freed
    }

    /// The counters, including the eviction ledger.
    #[must_use]
    pub fn observe(&self) -> CacheStats {
        self.stats_with(&self.lock())
    }

    fn stats_with(&self, inner: &CacheInner<K, V>) -> CacheStats {
        CacheStats {
            limit_bytes: self.limit(),
            bytes: inner.bytes,
            entries: inner.entries.len(),
            high_water_bytes: inner.high_water_bytes,
            hits: inner.hits,
            misses: inner.misses,
            insertions: inner.insertions,
            evictions: inner.evictions,
            evicted_bytes: inner.evicted_bytes,
            refusals: inner.refusals,
            refused_bytes: inner.refused_bytes,
            evicted_by_class: inner.evicted_by_class.clone(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, CacheInner<K, V>> {
        // A poisoned lock means a panic happened while holding it; the cache is not the thing worth
        // saving in that moment, so take the guard and let the panic travel on its own path.
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Evict least-recently-used entries until `inner` is at or under `limit`.
fn evict_to_fit<K, V>(inner: &mut CacheInner<K, V>, limit: u64)
where
    K: Clone + Eq + Hash + Ord,
{
    while inner.bytes > limit {
        let Some((tick, key)) = inner.order.keys().next().cloned() else {
            break;
        };
        inner.order.remove(&(tick, key.clone()));
        if let Some(entry) = inner.entries.remove(&key) {
            inner.bytes = inner.bytes.saturating_sub(entry.weight);
            inner.evictions += 1;
            inner.evicted_bytes = inner.evicted_bytes.saturating_add(entry.weight);
            *inner
                .evicted_by_class
                .entry(entry.class.clone())
                .or_insert(0) += 1;
        }
    }
}

/// What one [`Queue`] did while it was full.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueueStats {
    /// The item ceiling in force.
    pub limit_items: usize,
    /// The byte ceiling in force, or `0` for "unbounded in bytes".
    pub limit_bytes: u64,
    /// The most items ever waiting at once.
    pub high_water_items: usize,
    /// The most bytes ever waiting at once.
    pub high_water_bytes: u64,
    /// Items pushed.
    pub pushed_items: u64,
    /// Bytes pushed.
    pub pushed_bytes: u64,
    /// How many times a producer arrived to a full queue and had to wait. **This is the back-pressure
    /// counter**: non-zero means the consumer is the bottleneck and the pipeline slowed instead of
    /// growing.
    pub blocks: u64,
    /// How many times a producer arrived after the consumer had gone away.
    pub aborts: u64,
}

/// The reason [`Queue::push`] refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Aborted;

/// A bounded, blocking hand-off between two stages of one pipeline.
///
/// `push` waits while the queue is at its ceiling; `pop` waits until there is something to take or the
/// queue is closed. That is the whole mechanism, and it is the one that makes "the analyzer slowed down
/// instead of ballooning" true rather than aspirational: the only two things a producer can do when its
/// consumer is slow are wait or allocate, and an unbounded queue chooses allocate.
///
/// The byte ceiling is advisory per item — callers estimate an item's size (`commit` records know
/// theirs) and the queue bounds the sum. Items larger than the whole byte ceiling always go through,
/// because refusing to hand over a single unit of work would stall the pipeline for a reason nobody can
/// fix by tuning.
#[derive(Debug)]
pub struct Queue<T> {
    limit_items: usize,
    limit_bytes: u64,
    inner: Mutex<QueueInner<T>>,
    not_full: std::sync::Condvar,
    not_empty: std::sync::Condvar,
}

#[derive(Debug)]
struct QueueInner<T> {
    items: std::collections::VecDeque<(T, u64)>,
    bytes: u64,
    closed: bool,
    aborted: bool,
    stats: QueueStats,
}

impl<T> Queue<T> {
    /// A queue holding at most `limit_items` items (at least one) and, when `limit_bytes` is non-zero,
    /// at most that many bytes.
    #[must_use]
    pub fn new(limit_items: usize, limit_bytes: u64) -> Self {
        Queue {
            limit_items: limit_items.max(1),
            limit_bytes,
            inner: Mutex::new(QueueInner {
                items: std::collections::VecDeque::new(),
                bytes: 0,
                closed: false,
                aborted: false,
                stats: QueueStats {
                    limit_items: limit_items.max(1),
                    limit_bytes,
                    ..QueueStats::default()
                },
            }),
            not_full: std::sync::Condvar::new(),
            not_empty: std::sync::Condvar::new(),
        }
    }

    /// Hand an item of the given size to the queue, waiting if it is full.
    ///
    /// `Err(Aborted)` means the consumer has gone away ([`Queue::abort`]); a producer's job then is to
    /// stop producing, not to keep a batch it can never hand over.
    pub fn push(&self, item: T, bytes: u64) -> Result<(), Aborted> {
        let mut inner = self.lock();
        loop {
            if inner.aborted {
                inner.stats.aborts += 1;
                return Err(Aborted);
            }
            let room = inner.items.len() < self.limit_items
                && (self.limit_bytes == 0
                    || inner.items.is_empty()
                    || inner.bytes.saturating_add(bytes) <= self.limit_bytes);
            if room {
                inner.items.push_back((item, bytes));
                inner.bytes = inner.bytes.saturating_add(bytes);
                inner.stats.pushed_items += 1;
                inner.stats.pushed_bytes = inner.stats.pushed_bytes.saturating_add(bytes);
                if inner.items.len() > inner.stats.high_water_items {
                    inner.stats.high_water_items = inner.items.len();
                }
                if inner.bytes > inner.stats.high_water_bytes {
                    inner.stats.high_water_bytes = inner.bytes;
                }
                drop(inner);
                self.not_empty.notify_one();
                return Ok(());
            }
            inner.stats.blocks += 1;
            inner = self
                .not_full
                .wait(inner)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    /// Take the next item, waiting until there is one. `None` when the queue is closed and drained, or
    /// when a producer aborted and there is nothing left.
    pub fn pop(&self) -> Option<T> {
        let mut inner = self.lock();
        loop {
            if let Some((item, bytes)) = inner.items.pop_front() {
                inner.bytes = inner.bytes.saturating_sub(bytes);
                drop(inner);
                self.not_full.notify_one();
                return Some(item);
            }
            if inner.closed || inner.aborted {
                return None;
            }
            inner = self
                .not_empty
                .wait(inner)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    /// No more items are coming; wake any waiting consumer once the queue drains.
    pub fn close(&self) {
        self.lock().closed = true;
        self.not_empty.notify_all();
    }

    /// The consumer has stopped early; wake any waiting producer so it can abandon its batch.
    pub fn abort(&self) {
        {
            let mut inner = self.lock();
            inner.aborted = true;
        }
        self.not_full.notify_all();
        self.not_empty.notify_all();
    }

    /// Items waiting right now.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lock().items.len()
    }

    /// Whether nothing is waiting right now.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lock().items.is_empty()
    }

    /// The counters, including the back-pressure high-water marks.
    #[must_use]
    pub fn observe(&self) -> QueueStats {
        self.lock().stats.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, QueueInner<T>> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
