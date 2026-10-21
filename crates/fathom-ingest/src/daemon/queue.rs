//! The bounded queue of work the daemon has been asked to do — and what "back-pressure" means when
//! there is nothing to push back on (`e6.3` / `reco-kvk`).
//!
//! # The two inputs, which are not alike
//!
//! The daemon is asked for work from two directions and they fail differently:
//!
//! * **requests** come from clients (`fathom index`, a TUI that wants the pane to be current, an
//!   `fathom status --index`). There are few of them, they are *idempotent* — an index request asked for
//!   twice is the same index — and a client can be told "already queued". That is the queue below:
//!   bounded, coalescing by key, and shedding with a count when it overflows, which is the
//!   shed-with-count ADR-005 §8.1 deferred to this ticket because there was no queue to apply it to.
//! * **the hook stream** is an append-only file. You cannot shed it (ADR-005 §2: a silently-dropped
//!   event is invisible and a counted one is a number somebody acts on), so it has no queue at all: the
//!   file *is* the queue, and back-pressure is simply *not reading so fast* — a byte cap per cycle, with
//!   the resulting lag reported as `hooks.lag_bytes`. That is the honest form of the requirement: the
//!   producer is unaffected, the consumer stays inside its budget, and the gap between them is printed
//!   instead of hidden.
//!
//! # Why coalescing is the interesting part
//!
//! Acceptance criterion one is "three TUIs plus a CLI cause exactly one indexing pass". Four clients
//! asking within one pass is four `Index` requests with the same key, and a queue that kept them as work
//! items would run four passes and be *correct* — and forty percent the size of the problem. Coalescing
//! is why the counter says one. The counter is also the evidence: `coalesced` is what makes "one pass"
//! an observation rather than an anecdote.
//!
//! # Why shedding has to be counted and answered
//!
//! Overflow is the failure this queue actually fears: a client in a loop, or a fleet of panes. When the
//! bound is reached the newest request is refused with a stated reason (and `fathom index` prints it and
//! exits 0 — the daemon is working, the request was redundant, and a nonzero exit for "somebody is
//! already doing it" is how scripts learn to ignore exit codes). `shed` is how many times that happened,
//! which is the number that turns "the daemon feels sluggish" into "it was refused 300 requests in a
//! minute".

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// One queued request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    /// What the request is for. Two requests with the same key are the same request.
    pub key: String,
    /// Who asked, for the counters. The first asker's origin is kept, because the first one is the
    /// request that is actually being served.
    pub origin: String,
    /// When the request first arrived. The age of the oldest queued request is what "is the daemon
    /// keeping up" means, so this is a monotonic clock and not a wall clock.
    pub since: Instant,
    /// How many clients are waiting to be told this one finished.
    pub waiters: usize,
}

impl Pending {
    /// How long the oldest request in the queue has been waiting.
    #[must_use]
    pub fn age(&self) -> Duration {
        self.since.elapsed()
    }
}

/// What happened to a request that was pushed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// It became a new queue entry.
    Queued,
    /// An identical request was already waiting; nothing new will run because of it, and that is the
    /// point. Counted in [`Queue::coalesced`].
    Coalesced,
    /// The queue is at its bound and this request was refused. Counted in [`Queue::shed`], and the
    /// caller is told, because a refusal nobody reports is a dropped request.
    Shed,
}

/// A bounded, coalescing queue of index requests.
#[derive(Debug)]
pub struct Queue {
    items: VecDeque<Pending>,
    depth: usize,
    pushed: u64,
    coalesced: u64,
    shed: u64,
    served: u64,
    high_water: usize,
}

impl Queue {
    /// A queue that holds at most `depth` distinct requests.
    #[must_use]
    pub fn new(depth: usize) -> Queue {
        Queue {
            items: VecDeque::new(),
            depth: depth.max(1),
            pushed: 0,
            coalesced: 0,
            shed: 0,
            served: 0,
            high_water: 0,
        }
    }

    /// The bound.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// How many distinct requests are waiting right now. This is the `queue.depth` in the status
    /// document, and it is *distinct* requests on purpose: twelve clients asking for the same thing is
    /// one piece of work, and a status page that said twelve would be lying about the work.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Is nothing waiting?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The busiest the queue has been, which is the number that says whether `depth` is the right number.
    #[must_use]
    pub fn high_water(&self) -> usize {
        self.high_water
    }

    /// Requests accepted since the daemon started.
    #[must_use]
    pub fn pushed(&self) -> u64 {
        self.pushed
    }

    /// Requests that folded into an identical one already waiting.
    #[must_use]
    pub fn coalesced(&self) -> u64 {
        self.coalesced
    }

    /// Requests refused because the queue was full.
    #[must_use]
    pub fn shed(&self) -> u64 {
        self.shed
    }

    /// Requests handed to a pass.
    #[must_use]
    pub fn served(&self) -> u64 {
        self.served
    }

    /// The age of the oldest waiting request, for the status page's "queue 4 s old" line.
    #[must_use]
    pub fn oldest(&self) -> Option<Duration> {
        self.items.front().map(Pending::age)
    }

    /// Are we holding `key` right now? A caller uses this to answer "already queued" without queueing.
    #[must_use]
    pub fn holds(&self, key: &str) -> bool {
        self.items.iter().any(|item| item.key == key)
    }

    /// How many clients are waiting on `key`.
    #[must_use]
    pub fn waiters(&self, key: &str) -> usize {
        self.items
            .iter()
            .find(|item| item.key == key)
            .map_or(0, |item| item.waiters)
    }

    /// Offer a request.
    ///
    /// `admit_waiter` marks a client that wants to be told when this request is served: an identical
    /// request that is already waiting joins it rather than duplicating it, which is what lets four
    /// clients be woken by one pass.
    #[must_use]
    pub fn push(&mut self, key: &str, origin: &str, admit_waiter: bool) -> Admission {
        self.pushed += 1;
        if let Some(item) = self.items.iter_mut().find(|item| item.key == key) {
            if admit_waiter {
                item.waiters += 1;
            }
            self.coalesced += 1;
            return Admission::Coalesced;
        }
        if self.items.len() >= self.depth {
            self.shed += 1;
            return Admission::Shed;
        }
        self.items.push_back(Pending {
            key: key.to_owned(),
            origin: origin.to_owned(),
            since: Instant::now(),
            waiters: usize::from(admit_waiter),
        });
        self.high_water = self.high_water.max(self.items.len());
        Admission::Queued
    }

    /// Take everything waiting, as one batch — the pass is about to run, and a batch is what tells the
    /// status page what this pass was for.
    pub fn drain(&mut self) -> Vec<Pending> {
        let out: Vec<Pending> = self.items.drain(..).collect();
        self.served += u64::try_from(out.len()).unwrap_or(u64::MAX);
        out
    }

    /// Put a batch back, for the case where the pass that took it did not get to run (the CPU budget ran
    /// out mid-cycle, or the run failed and will be retried). The `since` stamps are kept, so a queue
    /// that has been waiting two minutes still *says* it has been waiting two minutes rather than
    /// restarting its own age every time it is retried.
    pub fn requeue(&mut self, batch: Vec<Pending>) {
        for item in batch {
            if self.items.len() >= self.depth {
                self.shed += 1;
                continue;
            }
            if self.holds(&item.key) {
                self.coalesced += 1;
                continue;
            }
            self.items.push_back(item);
        }
        self.high_water = self.high_water.max(self.items.len());
    }

    /// A snapshot for the status document.
    #[must_use]
    pub fn stats(&self) -> serde_json::Value {
        serde_json::json!({
            "depth": self.items.len(),
            "bound": self.depth,
            "high_water": self.high_water,
            "pushed": self.pushed,
            "coalesced": self.coalesced,
            "shed": self.shed,
            "served": self.served,
            "oldest_ms": self.oldest().map(|age| age.as_millis()),
            "keys": self.items.iter().map(|item| item.key.clone()).collect::<Vec<_>>(),
        })
    }
}

/// The other input: the hook stream's lag, which is back-pressure's only honest unit when the producer
/// is a file.
#[derive(Debug, Default)]
pub struct Lag {
    /// Bytes the last pass read.
    pub read_bytes: u64,
    /// The per-cycle byte cap in force.
    pub cap_bytes: u64,
    /// Bytes written and not yet consumed, as of the last look.
    pub behind_bytes: u64,
    /// Lines consumed in total.
    pub lines: u64,
    /// Cycles in which the cap was reached, so `behind_bytes` is not a fluke but a state.
    pub saturated: u64,
    /// True when the last pass could not take everything waiting.
    pub backpressured: bool,
}

impl Lag {
    /// Fold one pass's numbers in.
    pub fn observe(&mut self, read_bytes: u64, behind_bytes: u64, lines: u64) {
        self.read_bytes = read_bytes;
        self.behind_bytes = behind_bytes;
        self.lines += lines;
        self.backpressured = behind_bytes > 0;
        if read_bytes >= self.cap_bytes && self.cap_bytes > 0 {
            self.saturated += 1;
        }
    }

    /// A snapshot for the status document.
    #[must_use]
    pub fn stats(&self) -> serde_json::Value {
        serde_json::json!({
            "lag_bytes": self.behind_bytes,
            "cap_bytes": self.cap_bytes,
            "read_bytes": self.read_bytes,
            "lines": self.lines,
            "saturated_cycles": self.saturated,
            "backpressure": self.backpressured,
        })
    }
}
