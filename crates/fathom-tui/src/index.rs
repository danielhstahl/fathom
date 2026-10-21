//! What the caller knows about the index, and the empty states that follow from it (`reco-1nu`).
//!
//! The shell may not open a store (ADR-002 §5.2, lint `E01`), so before this module existed it could
//! not tell an indexed repository from an unindexed one. Both got the same sentence — *run `fathom
//! index`* — which is a remedy in one of them and a dead end in the other: on a repository whose index
//! held 500 commits and reported epoch 1 ready, the reviewer ran the command, reopened the shell, and
//! was told the same thing. ADR-006 §10.3's note on that row is "a shrug is not a remedy"; a false
//! remedy is worse than a shrug, because it gets followed.
//!
//! So `fathom-cli` — the composition root, which may read the store — says what it found, and the copy
//! branches on it. Two rules hold every sentence here:
//!
//! 1. **the index state printed is the index state the caller read**, not one the pane guessed;
//! 2. **no sentence names a command this binary does not have.** `fathom --demo` was one, and is gone.
//!
//! Every sentence is the join of [`Facts::summary`] (what the index holds) and [`Facts::remedy`] (what
//! to do, or who owns what is missing), so a pane that has room for one line can print the summary and
//! a pane that has room for three can print the whole thing — and the two cannot drift apart.
//!
//! The same batch answers the status strip, because the strip's four facts — the epoch, how many
//! episodes there are, how many are open, whether the analysis is caught up — are the same six numbers
//! seen from further off (`reco-zv9`). One read, one hand-over, two surfaces: a strip that printed
//! `unknown` beside a `fathom stats` that printed `epoch 1 ready` was two implementations of one
//! question, which is the thing this module was written to stop.

use crate::app::format_thousands;

/// Whether an index could be read at all, as far as the caller can say.
///
/// Five variants, because there are five different things to say and four different remedies. Collapsing
/// "could not open" into "not indexed" sends a reviewer to `fathom index` over a store that is corrupt,
/// and collapsing "this binary has no SQLite backend" into either sends them to `fathom index` with a
/// binary that cannot write a store at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Read {
    /// Nothing was looked up. The shell's own fallback: an embedded shell, or a test frame.
    #[default]
    Unknown,
    /// There is no store file: this repository has never been indexed.
    Absent,
    /// There is a store, it opened, and these are its numbers.
    Present,
    /// There is a store and it would not open (corrupt, locked, not a database).
    Unreadable,
    /// This binary was built without a SQLite backend, so no store can be read (`fathom --version`).
    NoBackend,
}

/// The index state the caller hands the shell before the first frame.
///
/// Deliberately plain data: `fathom-tui` has no store handle and no ingest edge (ADR-002 §5.2), so the
/// numbers arrive already read rather than being asked for. Every one is an `Option` — `unknown` is a
/// value and a blank is a lie (ADR-006 §0.1).
///
/// Six numbers, because the status strip asks six questions about the index and every one of them is
/// answerable from the store the CLI already opened (`reco-zv9`). The last two are the ones the strip
/// used to print `unknown` for: the queue's depth and the analysis verdict are not guesses a pane may
/// make, and they are not secrets the store keeps — `fathom stats` prints both.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    /// Whether the caller could read an index.
    pub read: Read,
    /// The published epoch (`fathom-store`'s `index_epoch`).
    pub epoch: Option<u64>,
    /// `epoch_status` verbatim: `ready`, `building`, or nothing at all when the store never said.
    pub status: Option<String>,
    /// Commits the index holds.
    pub commits: Option<u64>,
    /// Live episodes the index holds.
    pub episodes: Option<u64>,
    /// Live episodes still open — nobody has reviewed them (`reco-zv9`).
    ///
    /// The store's own count over every live episode, which is not the same set as the rows the shell
    /// happens to hold: a page of a longer index, or a filter, is fewer episodes than the queue is.
    pub queue_open: Option<u64>,
    /// Commits the index holds rows for and still owes an analysis (`reco-zv9`).
    ///
    /// `fathom stats` calls the same number "commit(s) owed an analysis". `Some(0)` is the warm case;
    /// `None` is a caller that could not ask, which is the only honest `idx unknown` there is.
    pub owed_analysis: Option<u64>,
}

impl Read {
    /// The stable spelling, for `--json` and for a bug report that has to say which state it saw.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Read::Unknown => "unknown",
            Read::Absent => "absent",
            Read::Present => "present",
            Read::Unreadable => "unreadable",
            Read::NoBackend => "no_backend",
        }
    }
}

impl Facts {
    /// Nothing was looked up: the shell's own fallback, and what every headless test frame draws.
    #[must_use]
    pub const fn unknown() -> Self {
        Facts {
            read: Read::Unknown,
            epoch: None,
            status: None,
            commits: None,
            episodes: None,
            queue_open: None,
            owed_analysis: None,
        }
    }

    /// No store file. The one state where `fathom index` is the honest remedy.
    #[must_use]
    pub const fn absent() -> Self {
        Facts {
            read: Read::Absent,
            epoch: None,
            status: None,
            commits: None,
            episodes: None,
            queue_open: None,
            owed_analysis: None,
        }
    }

    /// A store that opened, with whatever it would answer.
    #[must_use]
    pub fn present(
        epoch: Option<u64>,
        status: Option<String>,
        commits: Option<u64>,
        episodes: Option<u64>,
    ) -> Self {
        Facts {
            read: Read::Present,
            epoch,
            status,
            commits,
            episodes,
            queue_open: None,
            owed_analysis: None,
        }
    }

    /// Hand over the queue's depth (`reco-zv9`): how many live episodes nobody has reviewed.
    ///
    /// Chainable rather than a fifth argument on [`Self::present`], because a bare `Option<u64>` in a
    /// list of four bare `Option<u64>`s is a call site nobody can read and every caller of `present`
    /// would have to be rewritten to say which one it was passing.
    #[must_use]
    pub fn with_queue_open(mut self, open: Option<u64>) -> Self {
        self.queue_open = open;
        self
    }

    /// Hand over the analysis backlog: how many commits owe one (`reco-zv9`).
    #[must_use]
    pub fn with_backlog(mut self, owed: Option<u64>) -> Self {
        self.owed_analysis = owed;
        self
    }

    /// A store that would not open. The sentence names `fathom doctor`, which is the verb that
    /// adjudicates a store; this one does not guess at the reason.
    #[must_use]
    pub const fn unreadable() -> Self {
        Facts {
            read: Read::Unreadable,
            epoch: None,
            status: None,
            commits: None,
            episodes: None,
            queue_open: None,
            owed_analysis: None,
        }
    }

    /// This binary has no SQLite backend.
    #[must_use]
    pub const fn no_backend() -> Self {
        Facts {
            read: Read::NoBackend,
            epoch: None,
            status: None,
            commits: None,
            episodes: None,
            queue_open: None,
            owed_analysis: None,
        }
    }

    /// Is there a usable index behind the empty pane?
    #[must_use]
    pub const fn is_present(&self) -> bool {
        matches!(self.read, Read::Present)
    }

    /// Is the epoch still being written — a rebuild started and not finished?
    #[must_use]
    pub fn is_building(&self) -> bool {
        self.is_present() && self.status.as_deref() == Some("building")
    }

    /// Is the analysis cache warm — does the index owe nothing? (`reco-zv9`)
    ///
    /// This is the strip's `idx warm` / `idx cold`, and `None` is the `unknown` beside them, which is
    /// only allowed when the caller genuinely could not ask: no store, a store that would not open, a
    /// store that would not answer. A building epoch is cold by definition, whatever its backlog says —
    /// the run that would warm it has not finished. `fathom stats` prints the backlog itself.
    #[must_use]
    pub fn warm(&self) -> Option<bool> {
        if !self.is_present() {
            return None;
        }
        if self.is_building() {
            return Some(false);
        }
        self.owed_analysis.map(|owed| owed == 0)
    }

    /// `index epoch 1` — with ` (` `)` around the status in the strip's form and a bare space in the
    /// sentence's, because `index epoch 1 (ready) · …` is a label and `index epoch 1 ready` is prose.
    fn epoch_bit(&self, wrap: bool) -> String {
        let epoch = self
            .epoch
            .map_or_else(|| "unknown".to_owned(), |epoch| epoch.to_string());
        match self.status.as_deref() {
            Some(status) if wrap => format!("index epoch {epoch} ({status})"),
            Some(status) => format!("index epoch {epoch} {status}"),
            None => format!("index epoch {epoch}"),
        }
    }

    /// Where the rail's rows came from, as the strip prints it: `index epoch 1 (ready)`.
    #[must_use]
    pub fn origin(&self) -> String {
        match self.read {
            Read::Unknown | Read::Absent => "no index".to_owned(),
            Read::Unreadable => "index unreadable".to_owned(),
            Read::NoBackend => "no sqlite backend".to_owned(),
            Read::Present => self.epoch_bit(true),
        }
    }

    /// The index state alone, as one short line: `index epoch 1 ready · 500 commits · 500 eps`.
    #[must_use]
    pub fn summary(&self) -> String {
        match self.read {
            Read::Unknown => "nothing loaded yet".to_owned(),
            Read::Absent => "no index here yet".to_owned(),
            Read::Unreadable => "the index would not open".to_owned(),
            Read::NoBackend => "this binary has no SQLite backend".to_owned(),
            Read::Present => {
                let mut bits = vec![self.epoch_bit(false)];
                if let Some(commits) = self.commits {
                    bits.push(format!("{} commits", format_thousands(commits)));
                }
                if let Some(episodes) = self.episodes {
                    bits.push(format!("{} eps", format_thousands(episodes)));
                }
                bits.join(" · ")
            }
        }
    }

    /// What to do about it, as one line short enough to read whole in the narrowest pane — or who owns
    /// what is missing, when nothing in this build can be done. A remedy that is not true is worse than
    /// none, which is the whole reason this module exists (`reco-1nu`).
    ///
    /// `reco-wzt` moved one sentence here. This used to say *the episode read is `e2.11`'s, not in this
    /// build*, which was the truth right up until the composition root started reading live episodes out
    /// of the spine: the read exists now, and what is still missing is the delta model behind the ranked
    /// queue and the card's `delta` row. A remedy has to be something the reviewer can do.
    #[must_use]
    pub fn remedy(&self) -> String {
        match self.read {
            Read::Unknown => "the shell was handed no index state".to_owned(),
            Read::Absent => "run `fathom index` here, then reopen fathom".to_owned(),
            Read::Unreadable => "`fathom doctor` says what is wrong".to_owned(),
            Read::NoBackend => "`fathom --version` lists what is built in".to_owned(),
            Read::Present if self.is_building() => {
                "the epoch is still being written — wait for it".to_owned()
            }
            Read::Present
                if self
                    .episodes
                    .is_some_and(|episodes| episodes == 0) =>
            {
                "the index has nothing to list; `fathom doctor` says whether that is the repository or \
                 the run"
                    .to_owned()
            }
            Read::Present => {
                "`t` opens the timeline, which lists them as the index knows them".to_owned()
            }
        }
    }

    /// The whole empty state: index state, remedy, and — for a finished index that has episodes in it —
    /// the headless verb that prints one, plus who owns the read this build has not got.
    ///
    /// The rail body wraps this and the rail strip prints it whole, so it may be long; a narrow pane
    /// prints [`Facts::summary`] and [`Facts::remedy`] as two rows instead.
    #[must_use]
    pub fn empty_words(&self) -> String {
        let base = format!("{} — {}", self.summary(), self.remedy());
        if self.is_building() {
            return format!(
                "{base}; if a rebuild died halfway, `fathom index --rebuild` starts a clean one"
            );
        }
        // The one place `--demo` belongs in a pane (`e5.9`): a reviewer staring at an unindexed repository
        // is exactly the person who has not decided whether this is worth setting up, and the answer is a
        // flag rather than a commit. It used to be named here when no such flag existed, which is the
        // mistake `reco-1nu` was filed against — the difference now is that it can be typed.
        if self.read == Read::Absent {
            return format!(
                "{base}; `fathom --demo` reviews a miniature repository, if you would rather see this \
                 one working before indexing your own"
            );
        }
        if self.is_present() && self.episodes.is_some_and(|episodes| episodes > 0) {
            return format!("{base}; `fathom show <episode>` prints one in full");
        }
        base
    }
}
