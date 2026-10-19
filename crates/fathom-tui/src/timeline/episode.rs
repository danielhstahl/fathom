//! `fathom_tui::timeline::episode` — what a rail row is made of, and how it arrives.
//!
//! The rail draws an episode, not a commit and not a database row. Everything the six columns and the
//! card need is in [`Episode`], and everything else — the delta model, the risk score, the findings —
//! arrives with the tickets that own it (`e2.11`, `e4.4`) and is printed as `?` until then, because a
//! rail column that cannot be cited is a defect (ADR-001 §7.10) rather than a gap to invent a number
//! for.
//!
//! Two rules decide the shape:
//!
//! * **Nothing here is read; it is handed over.** `fathom-tui` is IO-free (ADR-002 §3, lint `W1`), so
//!   the facts arrive through [`EpisodeSource`] — implemented by whoever holds them (`fathom-cli` over
//!   the index, a fixture over a table) and called off the UI thread. The rail draws what it was given.
//! * **An episode is addressed by its identity, never by its row.** [`Episode::id`] is the digest
//!   ADR-003 §2 derives from the three facts a rewrite preserves, and the digests beside it are the
//!   relink tiers the selection re-attaches along after a rebase (§6). The row index is a rendering
//!   detail; the identity is the thing a reviewer's cursor belongs to (ADR-006 §5.2).
//!
//! Timestamps are [`Timestamp`] — microseconds, UTC — because that is `fathom-core`'s unit. The store
//! keeps millis, so whoever maps rows into this type converts; the rail never sees a unit it has to
//! guess at.

use fathom_analyze::insight::{EpisodeDelta, PageWindow, RiskView};
use fathom_core::time::Timestamp;

use crate::diff::Unavailable;

use super::days;
use super::filter::{Filter, GroupBy};

/// Who wrote the episode (ADR-003 §2.6). `unknown` is a value, not a failure: the rail prints `?`
/// rather than guessing that an unfamiliar identity is a human.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AuthorKind {
    /// Written by a coding agent.
    Agent,
    /// Written by a person.
    Human,
    /// No usable identity (ADR-001 §7.7).
    Unknown,
}

impl AuthorKind {
    /// Every kind, in the order the filter editor lists them.
    pub const ALL: [AuthorKind; 3] = [AuthorKind::Agent, AuthorKind::Human, AuthorKind::Unknown];

    /// The store's spelling, so a facet read back from the index compares equal.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            AuthorKind::Agent => "agent",
            AuthorKind::Human => "human",
            AuthorKind::Unknown => "unknown",
        }
    }

    /// Parse the store's spelling; anything unrecognised is `unknown` rather than an error, because a
    /// row written by a newer fathom must not make this one refuse to draw (ADR-004 §9.5).
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "agent" => AuthorKind::Agent,
            "human" => AuthorKind::Human,
            _ => AuthorKind::Unknown,
        }
    }

    /// The rail's badge. `unknown` is `?` because a badge that guesses is worse than a badge that says
    /// it does not know.
    #[must_use]
    pub const fn badge(self) -> &'static str {
        match self {
            AuthorKind::Agent => "agent",
            AuthorKind::Human => "human",
            AuthorKind::Unknown => "?",
        }
    }
}

impl core::fmt::Display for AuthorKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What the reviewer has done with this episode, as the rail's last column.
///
/// The spellings are `fathom_store::episode::ReviewState`'s on purpose — one vocabulary for the state
/// a decision leaves behind, so a filter set in the TUI means the same query in the store (ADR-001
/// §7.10). The glyph is in the text channel, never in a colour, because the marker has to survive
/// `--plain`, a screen reader and `NO_COLOR` (ADR-006 §10.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReviewState {
    /// Nobody has opened it.
    Unread,
    /// Opened, no verdict yet.
    Read,
    /// Signed off (`x`).
    Accepted,
    /// Flagged for attention with a note (`f`).
    Flagged,
    /// Refused with a reason (`-`).
    Dismissed,
    /// Deferred (`z`).
    Snoozed,
}

impl ReviewState {
    /// Every state, in the order the filter editor lists them.
    pub const ALL: [ReviewState; 6] = [
        ReviewState::Unread,
        ReviewState::Read,
        ReviewState::Accepted,
        ReviewState::Flagged,
        ReviewState::Dismissed,
        ReviewState::Snoozed,
    ];

    /// The store's spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ReviewState::Unread => "unread",
            ReviewState::Read => "read",
            ReviewState::Accepted => "accepted",
            ReviewState::Flagged => "flagged",
            ReviewState::Dismissed => "dismissed",
            ReviewState::Snoozed => "snoozed",
        }
    }

    /// Parse the store's spelling; unknown values come back as `Unread`, which is the state that
    /// cannot hide a decision somebody already made.
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "read" => ReviewState::Read,
            "accepted" => ReviewState::Accepted,
            "flagged" => ReviewState::Flagged,
            "dismissed" => ReviewState::Dismissed,
            "snoozed" => ReviewState::Snoozed,
            _ => ReviewState::Unread,
        }
    }

    /// The rail's marker: `⚑` unread, `✓` opened, `✔` accepted, `⚐` flagged, `×` refused, `z` snoozed.
    #[must_use]
    pub const fn glyph(self) -> &'static str {
        match self {
            ReviewState::Unread => "⚑",
            ReviewState::Read => "✓",
            ReviewState::Accepted => "✔",
            ReviewState::Flagged => "⚐",
            ReviewState::Dismissed => "×",
            ReviewState::Snoozed => "z",
        }
    }

    /// Is this the state that counts as unread, including "we do not know" (ADR-006 §10.3: an unknown
    /// is never shown as finished work)?
    #[must_use]
    pub const fn is_unread(self) -> bool {
        matches!(self, ReviewState::Unread)
    }

    /// Has a human decided something? Mirrors the store's `is_verdict`.
    #[must_use]
    pub const fn is_decided(self) -> bool {
        !matches!(self, ReviewState::Unread | ReviewState::Read)
    }

    /// Does this episode satisfy an `Open` filter — the "anything someone has opened" reading of the
    /// store's own rule that `Some(Read)` means *opened*, not specifically `read`?
    #[must_use]
    pub const fn is_opened(self) -> bool {
        !self.is_unread()
    }
}

impl core::fmt::Display for ReviewState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One episode, as much of it as the timeline can be told.
///
/// Every field is one the rail or the card prints, and the rail prints `?` where a field is `None` or
/// empty rather than drawing a plausible zero (ADR-001 §7.7). The four identity fields are the ADR-003
/// §2 digests plus the member SHAs, which is exactly what a re-attach needs after a rewrite (§6).
#[derive(Debug, Clone)]
pub struct Episode {
    /// The derived identity (ADR-003 §2.1): what a review is keyed to.
    pub id: String,
    /// The strong digest (author, subject, path set) — moves when the id moves.
    pub fingerprint: String,
    /// Relink tier 1 (author, subject).
    pub fallback_fingerprint: String,
    /// Relink tier 2 (author, path set).
    pub shape_fingerprint: String,
    /// The member commit SHAs — tier 3, and the source of the card's "4 commits".
    pub members: Vec<String>,
    /// When the episode began.
    pub created: Timestamp,
    /// The last member's date, which is what the store's date-range filter bounds on.
    pub last: Timestamp,
    /// How the author was classified.
    pub author: AuthorKind,
    /// The author as committed — the filter's `author` dimension matches on this.
    pub author_name: String,
    /// How sure the attribution is; `None` prints `?` and never 0.
    pub author_confidence: Option<f64>,
    /// The one-line summary: the rail's subject and the card's title.
    pub summary: String,
    /// The files, as the store recorded them for this episode.
    pub paths: Vec<String>,
    /// Files touched.
    pub n_files: u64,
    /// Lines added.
    pub n_add: u64,
    /// Lines removed.
    pub n_del: u64,
    /// Binary files in the change, which is why the sparkline says `bin` instead of dots.
    pub n_binary: u64,
    /// What the reviewer has done with it.
    pub state: ReviewState,
    /// The harness session, when a trailer or an event named one (ADR-005).
    pub session: Option<String>,
    /// The harness turn, ditto.
    pub turn: Option<String>,
    /// Which boundary rule opened this episode, and how far to trust it (ADR-003 §4.7). The card
    /// prints both, because a boundary a human drew and a boundary a heuristic guessed are different
    /// claims.
    pub boundary_rule: Option<String>,
    /// See [`Episode::boundary_rule`].
    pub boundary_confidence: Option<f64>,
    /// The versioned delta model for this episode, or `None` when the caller did not build one.
    ///
    /// The card renders this and nothing else (`e2.11`): a pane that recomputed the figures it was handed
    /// would be the second reality the ticket exists to remove. `None` is not an error — it means the row
    /// came from a caller that hands rows and not models (a fixture, a shell with no index), and the card
    /// says so rather than guessing.
    pub delta: Option<EpisodeDelta>,
    /// Whether any current commit still belongs to it (ADR-003 §6.3): a dead episode is shown, marked
    /// `gone`, because "your sign-off is on history that is no longer here" must not be silent.
    pub live: bool,
}

impl Default for Episode {
    fn default() -> Self {
        Episode {
            id: String::new(),
            fingerprint: String::new(),
            fallback_fingerprint: String::new(),
            shape_fingerprint: String::new(),
            members: Vec::new(),
            created: Timestamp::UNIX_EPOCH,
            last: Timestamp::UNIX_EPOCH,
            author: AuthorKind::Unknown,
            author_name: String::new(),
            author_confidence: None,
            summary: String::new(),
            paths: Vec::new(),
            n_files: 0,
            n_add: 0,
            n_del: 0,
            n_binary: 0,
            state: ReviewState::Unread,
            session: None,
            turn: None,
            boundary_rule: None,
            boundary_confidence: None,
            delta: None,
            live: true,
        }
    }
}

/// The ranking a row carries, in the only spelling the product has for it.
///
/// This used to be a struct declared here. `e2.11` replaced it with the analysis crate's own
/// [`RiskView`] — same fields the rail prints, plus the ones the card and the artefact print — because a
/// second copy of a model type in a view crate is precisely the "their own version of reality" that
/// ADR-001 §7.10 forbids, and lint `M03` now fails the build if one is re-declared.
pub type Risk = fathom_analyze::insight::RiskView;

impl Episode {
    /// Replace every character in this row that may not be drawn as itself.
    ///
    /// A commit subject, an author name and a path are all text somebody else wrote, and the frame
    /// draws them into columns it measured. Two things go wrong if they are pasted through: a
    /// bidi override can make a path read as something it is not, and an invisible one-cell
    /// character makes the pane that measured it one cell wrong, which is a wrapped line and a
    /// shifted frame two rows down ([`crate::text`], ADR-006 §6).
    ///
    /// Called once, on load, by the rail — so every pane, the export and `--plain` all read the
    /// same already-visible version rather than each remembering to sanitize.
    pub fn sanitize(&mut self) {
        let scrub = |text: &mut String| {
            let clean = crate::text::sanitize(text);
            if clean != *text {
                *text = clean;
            }
        };
        scrub(&mut self.author_name);
        scrub(&mut self.summary);
        for path in &mut self.paths {
            scrub(path);
        }
        for text in [&mut self.session, &mut self.turn, &mut self.boundary_rule]
            .into_iter()
            .flatten()
        {
            scrub(text);
        }
    }

    /// Attach a bare ranking to a row that has one and nothing else.
    ///
    /// The risk the queue ranked this row by, or `None` when nothing has ranked it — printed as `?`,
    /// because "unranked" and "ranked zero" are different claims (ADR-001 §7.7).
    ///
    /// Since `e2.11` the ranking travels *inside the delta model* rather than as a field of its own, so
    /// the rail's risk column and the card's `attention` section cannot disagree about a number: they are
    /// reading one value out of one document. `reco-c05` computes it; `e4.4`'s scorer replaces the inputs,
    /// not the path.
    #[must_use]
    pub fn risk(&self) -> Option<&RiskView> {
        self.delta.as_ref().and_then(|delta| delta.risk.as_ref())
    }

    /// The minimum an episode has to have to be a row: an identity, a date and a subject.
    #[must_use]
    pub fn new(id: impl Into<String>, created: Timestamp, summary: impl Into<String>) -> Self {
        Episode {
            id: id.into(),
            created,
            last: created,
            summary: summary.into(),
            ..Episode::default()
        }
    }

    /// The id as the rail prints it: six characters, because the rail has 42 columns and the id is a
    /// label, not a handle you type.
    #[must_use]
    pub fn short_id(&self) -> String {
        self.id.chars().take(6).collect()
    }

    /// Total churn — the number behind the sparkline.
    #[must_use]
    pub fn size(&self) -> u64 {
        self.n_add.saturating_add(self.n_del)
    }

    /// Member count, stated the way the card states it.
    #[must_use]
    pub fn n_commits(&self) -> usize {
        self.members.len()
    }

    /// Is this row still unread? (Used as a predicate reference.)
    #[must_use]
    pub fn is_unread(&self) -> bool {
        self.state.is_unread()
    }

    /// The value episodes are grouped on (ADR-006 §5.2: "the rail groups by day or by agent
    /// session"). `None` is the single-ungrouped case, and an episode with no session is its own group
    /// keyed on the author rather than a group called "none" that would collect every unrelated row.
    #[must_use]
    pub fn group_key(&self, group: GroupBy) -> Option<GroupKey> {
        match group {
            GroupBy::None => None,
            GroupBy::Day => Some(GroupKey::Day(days::day_start(self.created))),
            GroupBy::Session => Some(GroupKey::Session(match &self.session {
                Some(session) => format!("session {session}"),
                None => format!(
                    "author {}",
                    if self.author_name.is_empty() {
                        "?"
                    } else {
                        &self.author_name
                    }
                ),
            })),
        }
    }

    /// Bytes this row keeps alive, including its heap. Measured, not estimated, because the memory
    /// criterion in `e1.6`'s acceptance list is about the rail holding a big index and the only
    /// honest number is `len` summed over the strings that are actually there.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        let strings = [
            &self.id,
            &self.fingerprint,
            &self.fallback_fingerprint,
            &self.shape_fingerprint,
            &self.author_name,
            &self.summary,
        ];
        let mut total = strings.iter().map(|s| s.len()).sum::<usize>();
        // The delta is the row's biggest tenant since `e2.11`, and it is counted because the criterion is
        // about what the rail actually holds. The model measures itself ([`EpisodeDelta::retained_bytes`])
        // so that the number cannot disagree with the structure it describes.
        if let Some(delta) = &self.delta {
            total += delta.retained_bytes();
        }
        total += self.members.iter().map(String::len).sum::<usize>();
        total += self.paths.iter().map(String::len).sum::<usize>();
        total += self
            .session
            .as_ref()
            .map_or(0, |s| s.len() + s.capacity() - s.len());
        total += self.turn.as_ref().map_or(0, |s| s.len());
        total += self.boundary_rule.as_ref().map_or(0, String::len);
        // The vectors' own capacity, which is what a `Vec<String>` actually costs.
        total += self.members.capacity() * std::mem::size_of::<String>();
        total += self.paths.capacity() * std::mem::size_of::<String>();
        total
    }
}

/// A group header's key, and the label it prints.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum GroupKey {
    /// One UTC calendar day, as that day's first instant.
    Day(Timestamp),
    /// One agent session (or one author, when no session was recorded).
    Session(String),
}

/// A load of episodes: what the rail holds, where it came from, and how much the index has that the
/// rail does not.
///
/// The `total` half matters. The façade pages (ADR-002 §5.3: the façade owns the memory budget), so a
/// rail over a 3 412-episode index may legitimately hold 500 of them — and must say so in its strip
/// instead of letting the reviewer conclude the repository is quiet today.
#[derive(Debug, Clone)]
pub struct Batch {
    /// Where these rows came from, printed in the strip: `index epoch 41`, `fixture 10k`, a path.
    pub origin: String,
    /// The rows, newest first (the rail sorts defensively, but a source that lies about order is
    /// then at least not the reason the display is wrong).
    pub episodes: Vec<Episode>,
    /// How many the index holds, when the source knows. `None` prints `unknown`.
    pub total: Option<u64>,
    /// The epoch these rows were read from, so a re-index can be *named* rather than inferred.
    pub epoch: Option<u64>,
    /// Why the batch is empty, when it is — the pane prints this rather than nothing (ADR-001 §7.6).
    pub why_empty: Option<String>,
}

impl Batch {
    /// A batch of rows with a name.
    #[must_use]
    pub fn new(origin: impl Into<String>, episodes: Vec<Episode>) -> Self {
        let count = episodes.len();
        Batch {
            origin: origin.into(),
            episodes,
            total: Some(u64::try_from(count).unwrap_or(u64::MAX)),
            epoch: None,
            why_empty: None,
        }
    }

    /// No rows, and the reason.
    #[must_use]
    pub fn empty(origin: impl Into<String>, why: impl Into<String>) -> Self {
        Batch {
            origin: origin.into(),
            episodes: Vec::new(),
            total: None,
            epoch: None,
            why_empty: Some(why.into()),
        }
    }

    /// How many rows are in here.
    #[must_use]
    pub fn len(&self) -> usize {
        self.episodes.len()
    }

    /// Is the batch empty?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.episodes.is_empty()
    }
}

/// Where episodes come from.
///
/// The rail never queries: a worker calls [`EpisodeSource::load`] off the UI thread and hands the
/// resulting [`Batch`] to [`super::Timeline::load`], which is what keeps "scrolling never blocks on
/// the store" true by construction rather than by care (ADR-006 §0.1, ADR-001 §7.6).
///
/// There is deliberately no store-backed source in this crate — `fathom-tui` has no store handle and
/// no ingest edge (ADR-002 §5.2, lints `M01`/`M02`/`E01`). Today the only rows anyone fetches are the
/// page `fathom-cli` reads over the index and hands to [`super::Timeline::load`] before the first frame
/// (`frame::Options::episodes`), which is a `Batch` rather than a source because nothing fetches a
/// second page yet; a worker that pages on scroll, or a fixture over a table, is what implements this.
pub trait EpisodeSource: Send {
    /// Where the rows come from, as the strip prints it.
    fn origin(&self) -> String;

    /// Load the rows a filter selects, newest first, within the façade's page window.
    ///
    /// The filter is a *hint*: the source applies it so it does not read rows nobody wanted, and the
    /// rail applies it again to whatever arrives, so a source that ignores it cannot make the rail
    /// show the wrong episodes.
    ///
    /// # Errors
    ///
    /// [`Unavailable`] when the index is not there or not readable — which the pane prints as a
    /// sentence with a remedy, not as an empty list.
    fn load(&self, filter: &Filter, page: PageWindow) -> Result<Batch, Unavailable>;
}

/// Rows already in memory: a fixture, a test, a caller that read them itself.
#[derive(Debug, Clone)]
pub struct MemorySource {
    /// The label `origin` prints.
    pub origin: String,
    /// The rows.
    pub episodes: Vec<Episode>,
    /// Why a load would refuse, if this source is the "no index" kind.
    pub unavailable: Option<String>,
}

impl MemorySource {
    /// A named batch.
    #[must_use]
    pub fn new(origin: impl Into<String>, episodes: Vec<Episode>) -> Self {
        MemorySource {
            origin: origin.into(),
            episodes,
            unavailable: None,
        }
    }

    /// A source that refuses, with the sentence the pane prints.
    #[must_use]
    pub fn unavailable(origin: impl Into<String>, why: impl Into<String>) -> Self {
        MemorySource {
            origin: origin.into(),
            episodes: Vec::new(),
            unavailable: Some(why.into()),
        }
    }
}

impl EpisodeSource for MemorySource {
    fn origin(&self) -> String {
        self.origin.clone()
    }

    fn load(&self, filter: &Filter, page: PageWindow) -> Result<Batch, Unavailable> {
        if let Some(why) = &self.unavailable {
            return Err(Unavailable::with_hint(
                why.clone(),
                // The same remedy the empty rail prints, from the same place, so the two cannot drift.
                // (`fathom tui --no-index` used to be named here; no build has ever had that flag.)
                crate::index::Facts::absent().remedy(),
            ));
        }
        let limit = usize::try_from(page.limit()).unwrap_or(usize::MAX);
        let rows: Vec<Episode> = self
            .episodes
            .iter()
            .filter(|episode| filter.matches(episode))
            .skip(usize::try_from(page.offset()).unwrap_or(0))
            .take(limit)
            .cloned()
            .collect();
        let total = self
            .episodes
            .iter()
            .filter(|episode| filter.matches(episode))
            .count();
        Ok(Batch {
            origin: self.origin.clone(),
            total: Some(u64::try_from(total).unwrap_or(u64::MAX)),
            epoch: None,
            why_empty: None,
            episodes: rows,
        })
    }
}

/// The source for "there is no index yet" — a refusal that names the verb to run, so the pane never
/// draws an empty list that looks like an empty repository.
#[derive(Debug, Clone)]
pub struct NoIndex(pub String);

impl EpisodeSource for NoIndex {
    fn origin(&self) -> String {
        self.0.clone()
    }

    fn load(&self, _filter: &Filter, _page: PageWindow) -> Result<Batch, Unavailable> {
        Err(Unavailable::with_hint(
            self.0.clone(),
            "run `fathom index` first; `fathom stats` says what the index holds",
        ))
    }
}
