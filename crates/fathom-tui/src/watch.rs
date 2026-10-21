//! `fathom_tui::watch` — how an open frame learns that the repository moved (`reco-bpc`).
//!
//! The premise this module exists for is physical (ADR-001, ADR-006 §1): fathom sits in a second terminal
//! in ambient mode while an agent works in the first. A frame that never changes is not a calm picture of
//! a moving repository, it is a false one — a frozen rail reads as a quiet repository, which is the worst
//! possible misreading of a repository that is not quiet. Detection was never the problem: `fathom watch`
//! notices a commit made in another terminal in ~160 ms. The frame simply had no producer.
//!
//! So this module supplies the missing seam and nothing else:
//!
//! * [`Watcher`] is the question the loop asks on a wake-up it already takes — *has anything moved?* — and
//!   [`Update`] is the answer, in the only shapes the shell can draw: a [`Batch`] of episodes and the
//!   index's numbers. Like [`crate::frame::FrameSink`], [`crate::diff::Provider`] and
//!   [`crate::compare::Provider`], it is a trait the composition root implements, because the shell may not
//!   open a store, read git or run the indexer (ADR-002 §5.2, lint `E01`): `fathom-cli` probes, indexes and
//!   re-reads, and hands over rows.
//! * [`Source`] is the frame's whole answer to "is this picture current", and it has two states, because
//!   the honest fallback is not silence. When something *will* move the frame it says `live`; when nothing
//!   will, it says `snapshot`, says why, and names what would make it move. ADR-001 §7.7: unknown is a
//!   value, and a promise nobody can keep is worse than an admitted limitation.
//!
//! Nothing here is a thread or a timer. The loop is tickless (ADR-006 §0.1) and stays that way: it asks the
//! watcher on the wake-ups it already takes for `<C-c>`, so a watcher that returns `None` — which is every
//! one of them until a commit lands — costs the price of a `stat` and a ref read, and buys no extra wake-up.
//! A watcher is therefore expected to be cheap to ask and to do its expensive work only when it has
//! noticed the thing it is asking about; [`Source::poll`] is the seam that keeps that contract invisible
//! to the loop.

use std::sync::Arc;

use crate::index::Facts;
use crate::timeline::Batch;

/// What moved, in the only shapes the shell can draw.
///
/// Every field is optional because a producer re-reads what its own probe says moved: a store the watcher
/// wrote is a new batch and new numbers, a `fathom index` run by hand in another terminal is only new
/// numbers, and a half-finished rebuild is a sentence and nothing else. An update that carries nothing is
/// [`Update::is_empty`] and the loop draws no frame for it.
#[derive(Debug, Clone, Default)]
pub struct Update {
    /// The rail's rows now. The whole page, not a diff: the rail's own re-attachment rule is what keeps
    /// the reviewer's cursor where it was when the rows moved.
    pub episodes: Option<Batch>,
    /// What the index holds now, as the composition root read it.
    pub index: Option<Facts>,
    /// How many commits this update brought, when the producer knows. Printed, never guessed: the shell
    /// does not know what the index held before it was asked (`ADR-001 §7.7`).
    pub new_commits: Option<u64>,
    /// The producer's own sentence, in preference to the one the shell composes — which is where a failed
    /// index run, a skipped slice and a store that would not open get said in the words that explain them.
    pub note: Option<String>,
}

impl Update {
    /// A batch of new rows, which is the update the acceptance criterion is about.
    #[must_use]
    pub fn episodes(batch: Batch) -> Self {
        Update {
            episodes: Some(batch),
            ..Update::default()
        }
    }

    /// Did this update say nothing at all? A `true` here means the loop draws nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.episodes.is_none()
            && self.index.is_none()
            && self.new_commits.is_none()
            && self.note.is_none()
    }
}

/// The producer the composition root installs (`reco-bpc`).
///
/// One method, because the loop asks one question. `&self` with interior mutability rather than `&mut self`
/// — which is what [`crate::ambient::Feed`] gets away with `&mut self` for — because the shell hands the
/// watcher to no one else but does hold it behind an [`Arc`] beside every other provider it was given.
///
/// Implementations own the cost: probe cheaply, and read only what the probe says moved. The loop asks
/// roughly ten times a second at the outside ([`crate::terminal::SIGNAL_POLL`]) and most of the time the
/// answer must be `None` for the price of a few `stat`s.
pub trait Watcher: Send + Sync {
    /// Has anything moved? `None` is the common answer and draws nothing.
    fn poll(&self) -> Option<Update>;

    /// Asked to look *now*, ignoring whatever the poll interval said (`<C-r>`).
    ///
    /// The default is the same question a poll asks; an implementation that caches — or that throttles
    /// itself to keep an idle frame cheap — overrides this to drop the cache and re-read.
    fn refresh(&self) -> Option<Update> {
        self.poll()
    }
}

/// How this frame is kept current, and what it says about that.
///
/// Exactly two states, because there are exactly two promises a frame can make about the future, and a
/// frame that makes neither is the one this ticket was filed against.
#[derive(Clone)]
pub enum Source {
    /// A producer is running: new commits arrive by themselves.
    Live(Arc<dyn Watcher>),
    /// Nothing will ever move this frame, and the frame says so in two lengths: `why` is the reason the
    /// status line has room for, `refresh` is the sentence that names what would change it.
    Snapshot {
        /// The short reason, for the status line: `no index`, `--no-watch`, `this build cannot index`.
        why: String,
        /// What would make the frame move, in the frame's own words, remedy included.
        refresh: String,
    },
}

impl Source {
    /// Watch this repository through `watcher`.
    #[must_use]
    pub fn live(watcher: impl Watcher + 'static) -> Self {
        Source::Live(Arc::new(watcher))
    }

    /// Say plainly that this frame is a snapshot, and what it would take to move it.
    #[must_use]
    pub fn snapshot(why: impl Into<String>, refresh: impl Into<String>) -> Self {
        Source::Snapshot {
            why: why.into(),
            refresh: refresh.into(),
        }
    }

    /// Is something keeping this current?
    #[must_use]
    pub fn is_live(&self) -> bool {
        matches!(self, Source::Live(_))
    }

    /// The status line's bit: `live`, or `snapshot · why` (`reco-bpc`).
    ///
    /// Deliberately one short phrase, because the status line shares its row with the epoch, the counts
    /// and the notice count, and a frame that spends its row explaining its own freshness has not read
    /// ADR-006 §6.4.
    #[must_use]
    pub fn bit(&self) -> String {
        match self {
            Source::Live(_) => "live".to_owned(),
            Source::Snapshot { why, .. } => format!("snapshot · {why}"),
        }
    }

    /// The whole sentence, for wherever the frame has a row to spare — the notice list, the `--plain`
    /// footer, a bug report.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self {
            Source::Live(_) => {
                "this frame watches the repository: a commit made in another terminal \
                                arrives by itself"
                    .to_owned()
            }
            Source::Snapshot { why, refresh } => {
                format!("this frame is a snapshot — {why}; {refresh}")
            }
        }
    }

    /// Ask the producer. A snapshot never moves, so it never answers.
    pub fn poll(&self) -> Option<Update> {
        match self {
            Source::Live(watcher) => watcher.poll(),
            Source::Snapshot { .. } => None,
        }
    }

    /// Ask the producer now (`<C-r>`). A snapshot still has nothing to offer, and says which it is.
    pub fn refresh(&self) -> Option<Update> {
        match self {
            Source::Live(watcher) => watcher.refresh(),
            Source::Snapshot { .. } => None,
        }
    }
}

impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::Live(_) => f.debug_tuple("Live").field(&"<watcher>").finish(),
            Source::Snapshot { why, refresh } => f
                .debug_struct("Snapshot")
                .field("why", why)
                .field("refresh", refresh)
                .finish(),
        }
    }
}
