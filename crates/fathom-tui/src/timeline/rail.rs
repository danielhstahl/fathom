//! `fathom_tui::timeline::rail` — the episode list itself: its rows, its window, and its cursor.
//!
//! The rail is a *virtualized* list, and that is the property the acceptance criteria hang on: the
//! shell asks for `visible` rows and gets `visible` [`StyledLine`]s, never the whole list and never a
//! string per episode built in advance. Everything that scales with the index — the filtered order,
//! the group headers, the facet counts, the sparkline scale — is rebuilt when the *data or the
//! filter* changes, and nothing that happens on a scroll keystroke is O(episodes). That is what makes
//! "scrolling must never block on the store" a shape rather than a hope: the store is not on this path
//! at all ([`super::EpisodeSource`] runs on a worker), and a frame costs O(rows on screen).
//!
//! The other property is the one a reviewer feels: **the cursor belongs to the episode.** A re-index —
//! or a rebase that moves an episode's id — is a `load`, and a `load` re-attaches the cursor along
//! ADR-003 §6's tiers instead of leaving it on whatever row number it happened to hold.

use std::cell::Cell;
use std::collections::BTreeSet;

use fathom_core::time::Timestamp;

use crate::app::format_thousands;
use crate::style::StyledLine;

use super::days;
use super::episode::{Batch, Episode, GroupKey, ReviewState};
use super::filter::{Facets, Filter};
use super::render::{self, Scale};
use super::session::SESSION_MAX;

/// One row of the rail: a group header, an episode, or the marker standing in for a collapsed group.
///
/// `Hidden` exists because a collapsed group that simply stops printing is ADR-001 §7.2's silent
/// suppression: the rows it hides are *counted on screen*, and the key that opens them is named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slot {
    /// A group header.
    Group {
        /// What it groups on.
        key: GroupKey,
        /// `Tue 14 Oct ── 6 episodes`, built from the rows under it.
        label: String,
        /// Episodes under this header, hidden or not.
        count: usize,
        /// Is it collapsed?
        collapsed: bool,
    },
    /// An episode row, addressed by index into the loaded batch.
    Episode {
        /// Index into [`Rail::episodes`].
        index: usize,
    },
    /// `··· 17 more` — a collapsed group's rows, counted and named.
    Hidden {
        /// How many rows are behind it.
        count: usize,
    },
}

impl Slot {
    /// The episode this slot points at, if it points at one.
    #[must_use]
    pub const fn index(&self) -> Option<usize> {
        match self {
            Slot::Episode { index } => Some(*index),
            Slot::Group { .. } | Slot::Hidden { .. } => None,
        }
    }

    /// Is this a header?
    #[must_use]
    pub const fn is_group(&self) -> bool {
        matches!(self, Slot::Group { .. })
    }
}

/// What the cursor is *about* — the id and the digests ADR-003 §6 relinks along, so a selection can be
/// put back after a rewrite moved the identity itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Anchor {
    /// The derived id (tier 0: the row is the same episode).
    pub id: String,
    /// Relink tier 1: author + subject.
    pub fallback: String,
    /// Relink tier 2: author + path set.
    pub shape: String,
    /// Relink tier 3: member commits that survived.
    pub members: Vec<String>,
}

impl Anchor {
    /// Take the anchor from an episode.
    #[must_use]
    pub fn of(episode: &Episode) -> Anchor {
        Anchor {
            id: episode.id.clone(),
            fallback: episode.fallback_fingerprint.clone(),
            shape: episode.shape_fingerprint.clone(),
            members: episode.members.clone(),
        }
    }
}

/// One or two, in English: a group header that prints "1 episodes" is the kind of wording that makes
/// a UI feel unfinished, and the rail's groups are very often one episode long.
#[must_use]
pub fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        noun.to_owned()
    } else {
        format!("{noun}s")
    }
}

/// Name a re-attach tier the way the toast and the tests print it.
#[must_use]
pub const fn tier_words(tier: u8) -> &'static str {
    match tier {
        0 => "the same id",
        1 => "tier 1 · author+subject",
        2 => "tier 2 · author+paths",
        3 => "tier 3 · a member commit survived",
        _ => "no tier matched — the episode left the index",
    }
}

/// The counters the frame-budget and constant-memory criteria are asserted against.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RailStats {
    /// Episodes loaded.
    pub episodes: usize,
    /// Rows that survived the filter.
    pub kept: usize,
    /// Slots after grouping — the list's length as the motion keys see it.
    pub slots: usize,
    /// Rows the last frame drew. Bounded by the pane, never by the index.
    pub rows_built: usize,
    /// Characters the last frame built for those rows.
    pub chars_built: usize,
    /// How many times the order was rebuilt. A scroll must not raise this.
    pub rebuilds: u64,
    /// Bytes the loaded rows retain — linear in the input and nothing else.
    pub retained_bytes: usize,
}

/// The rail's state.
pub struct Rail {
    /// What the index said, newest first.
    episodes: Vec<Episode>,
    /// The rows: headers, episodes and collapsed markers, in draw order.
    slots: Vec<Slot>,
    /// The cursor, as a slot index.
    cursor: usize,
    /// The first slot drawn.
    offset: usize,
    /// Rows the pane has. A `Cell` because the shell measures it while drawing, and drawing takes
    /// `&App` (a frame may not be able to mutate what it is painting).
    visible: Cell<usize>,
    /// Rows the last frame built, for the bench.
    rows_built: Cell<usize>,
    /// Characters the last frame built, for the bench.
    chars_built: Cell<usize>,
    /// Collapsed groups.
    collapsed: BTreeSet<GroupKey>,
    /// Episodes this reviewer opened — in this session, or in one it resumed (`e1.11`).
    ///
    /// The session's read set, kept apart from `review_state` on purpose: the rail may use it to decide
    /// what still counts as unread, but it is not a decision and never becomes a sign-off.
    seen: BTreeSet<String>,
    /// What the cursor is about, across a reload.
    anchor: Option<Anchor>,
    /// The filter in force.
    filter: Filter,
    /// The chooser's candidates, counted at load time.
    facets: Facets,
    /// The sparkline's scale, from the loaded rows.
    scale: Scale,
    /// Where the rows came from.
    origin: String,
    /// How many the index holds, if the source said.
    total: Option<u64>,
    /// Which epoch the rows came from.
    epoch: Option<u64>,
    /// Why there is nothing, when there is nothing.
    why_empty: Option<String>,
    /// The frame's instant, for the age column and the jumps.
    now: Timestamp,
    /// How many rows the filter kept, cached so a strip costs nothing.
    kept: usize,
    /// The order is stale and will be rebuilt before anything reads it.
    dirty: bool,
    /// The rows on screen in queue order — episode indices, riskiest first (`reco-c05`). Built here
    /// for the same reason the display order is: `rebuild()` is the only place the rail is
    /// O(episodes), and a worklist that re-ranked on every frame would cost what the index costs
    /// (which is what `timeline_bench` measures).
    ranked: Vec<usize>,
    /// How many on-screen rows carry a ranking, and how many are unread. Both are printed every
    /// frame — the strip's `N ranked of M held` and the status line's `queue N open` — so neither
    /// may be a pass over the index at draw time.
    risked: usize,
    unread: usize,
    /// The last re-attach: the tier that fired, and its words.
    pub reattached: Option<(u8, String)>,
    /// The counters.
    pub stats: RailStats,
}

impl Default for Rail {
    fn default() -> Self {
        Rail {
            episodes: Vec::new(),
            slots: Vec::new(),
            cursor: 0,
            offset: 0,
            visible: Cell::new(1),
            rows_built: Cell::new(0),
            chars_built: Cell::new(0),
            collapsed: BTreeSet::new(),
            seen: BTreeSet::new(),
            anchor: None,
            filter: Filter::default(),
            facets: Facets::default(),
            scale: Scale::default(),
            origin: "no index".to_owned(),
            total: None,
            epoch: None,
            // Nothing yet, and no caller has said why. `empty_words` prints the shell's own honest
            // fallback for that, and `set_index_facts` replaces it the moment the caller speaks
            // (`reco-1nu`): a rail may not assert that the repository is unindexed, because it has no
            // way to know.
            why_empty: None,
            now: Timestamp::UNIX_EPOCH,
            kept: 0,
            ranked: Vec::new(),
            risked: 0,
            unread: 0,
            dirty: true,
            reattached: None,
            stats: RailStats::default(),
        }
    }
}

impl core::fmt::Debug for Rail {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Deliberately not every episode: a `Debug` of the app shows up in test failures, and a
        // 10 000-row dump there is a wall of noise hiding the one line that matters.
        f.debug_struct("Rail")
            .field("origin", &self.origin)
            .field("episodes", &self.episodes.len())
            .field("slots", &self.slots.len())
            .field("cursor", &self.cursor)
            .field("offset", &self.offset)
            .field("visible", &self.visible.get())
            .field("filter", &self.filter.summary())
            .field(
                "anchor",
                &self.anchor.as_ref().map(|anchor| anchor.id.clone()),
            )
            .finish()
    }
}

impl Rail {
    /// An empty rail drawn for a given instant.
    #[must_use]
    pub fn new(now: Timestamp) -> Self {
        Rail {
            now,
            ..Rail::default()
        }
    }

    // --- the data ---------------------------------------------------------------------------------

    /// Replace the loaded rows (a first load, a re-index, or a worker's answer).
    ///
    /// The cursor follows the *episode*: if the anchor still matches a row it lands there and the tier
    /// that matched is recorded; if nothing matches, the cursor goes to the top and says so, because
    /// "your selection was on an episode this rebase dropped" is a fact the reviewer needs
    /// (ADR-003 I2: a decision never disappears silently).
    pub fn load(&mut self, batch: Batch) {
        self.origin = batch.origin;
        self.total = batch.total;
        self.epoch = batch.epoch;
        self.why_empty = batch.why_empty;
        self.episodes = batch.episodes;
        // Every string in a row came out of somebody else's commit: a subject, an author name, a
        // path. [`Episode::sanitize`] makes the ones that cannot be drawn as themselves visible
        // before any pane measures them (`e5.5`), which is the only place it can be done once for
        // every pane — the rail is the one choke point every row passes through.
        for episode in &mut self.episodes {
            episode.sanitize();
        }
        // Newest first, with the id as the tie-break so two episodes with the same date have one
        // order rather than whichever order the store happened to return.
        self.episodes
            .sort_by(|a, b| b.created.cmp(&a.created).then_with(|| a.id.cmp(&b.id)));
        self.facets = Facets::of(&self.episodes);
        self.scale = Scale::of(&self.episodes);
        self.dirty = true;
        self.rebuild();
        self.follow_anchor(true);
    }

    /// Replace the filter. The order is rebuilt; the cursor follows the episode it was on, because
    /// filtering a list must not be what moves a reviewer's selection.
    pub fn set_filter(&mut self, filter: Filter) {
        if filter == self.filter {
            return;
        }
        self.filter = filter;
        self.dirty = true;
        self.rebuild();
        self.follow_anchor(false);
    }

    /// The filter in force.
    #[must_use]
    pub const fn filter(&self) -> &Filter {
        &self.filter
    }

    /// The chooser's candidates.
    #[must_use]
    pub fn facets(&self) -> &Facets {
        &self.facets
    }

    /// Tell the rail what instant to age itself against.
    pub fn set_now(&mut self, now: Timestamp) {
        self.now = now;
    }

    /// The instant the rail is drawn for.
    #[must_use]
    pub const fn now(&self) -> Timestamp {
        self.now
    }

    /// Where the rows came from.
    #[must_use]
    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// Which epoch the rows came from, when the source named one.
    #[must_use]
    pub const fn epoch(&self) -> Option<u64> {
        self.epoch
    }

    /// How many the index holds, when the source said.
    #[must_use]
    pub const fn total(&self) -> Option<u64> {
        self.total
    }

    /// Why the rail is empty, when it is.
    #[must_use]
    pub fn why_empty(&self) -> Option<&str> {
        self.why_empty.as_deref()
    }

    /// The loaded rows, filtered by what is on screen.
    #[must_use]
    pub fn episodes(&self) -> &[Episode] {
        &self.episodes
    }

    /// Stamp the review marker on one row (`e4.5`).
    ///
    /// The shell writes the ledger first and calls this only when the write landed, which is the whole
    /// difference between a badge that means "a reviewer decided this" and one that means "a key was
    /// pressed". Returns whether the row was found, so the caller can say which it did.
    pub fn set_state(&mut self, id: &str, state: ReviewState) -> bool {
        if let Some(row) = self.episodes.iter_mut().find(|episode| episode.id == id) {
            row.state = state;
            return true;
        }
        false
    }

    /// The rows on screen as episode indices, collapsed groups excluded — what a test counts when it
    /// wants "which episodes does this filter select", and what `m` marks.
    #[must_use]
    pub fn order(&self) -> Vec<usize> {
        self.slots.iter().filter_map(Slot::index).collect()
    }

    /// The on-screen rows in queue order: episode indices, riskiest first, unranked ones last
    /// (`reco-c05`). A slice, because the worklist draws a window of it and must not rebuild one.
    #[must_use]
    pub fn ranked(&self) -> &[usize] {
        &self.ranked
    }

    /// How many on-screen rows carry a ranking, and how many are still unread: the strip's and the
    /// status line's numbers, counted when the order was built rather than at draw time.
    #[must_use]
    pub fn counts(&self) -> (usize, usize) {
        (self.risked, self.unread)
    }

    /// The first row sharing one of these member SHAs — ADR-003 §6 tier 3, used when a session is
    /// restored onto an index whose ids have moved.
    #[must_use]
    pub fn find_by_members(&self, members: &[String]) -> Option<usize> {
        if members.is_empty() {
            return None;
        }
        self.slots.iter().position(|slot| {
            self.episode_of(slot)
                .is_some_and(|episode| episode.members.iter().any(|m| members.contains(m)))
        })
    }

    /// The counters, after any pending rebuild.
    #[must_use]
    pub fn stats(&mut self) -> RailStats {
        self.rebuild();
        self.counters()
    }

    /// The counters as they stand, without forcing a rebuild — what a bench reads between frames.
    #[must_use]
    pub fn last_stats(&self) -> RailStats {
        self.counters()
    }

    fn counters(&self) -> RailStats {
        RailStats {
            rows_built: self.rows_built.get(),
            chars_built: self.chars_built.get(),
            ..self.stats
        }
    }

    // --- the order -------------------------------------------------------------------------------

    /// Rebuild the order if anything invalidated it: one pass over the loaded rows, and the only
    /// place the rail is O(episodes).
    fn rebuild(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        self.stats.rebuilds += 1;
        let mut slots: Vec<Slot> = Vec::new();
        let mut kept: usize = 0;
        // A run is one group's worth of matching rows, in draw order.
        let mut run_key: Option<GroupKey> = None;
        let mut run_rows: Vec<usize> = Vec::new();
        for (index, episode) in self.episodes.iter().enumerate() {
            if !self.filter.matches(episode) {
                continue;
            }
            kept += 1;
            let key = episode.group_key(self.filter.group);
            if run_key.is_some() && run_key != key {
                self.flush(&mut slots, run_key.take(), &run_rows);
                run_rows.clear();
            }
            run_key = key;
            run_rows.push(index);
        }
        self.flush(&mut slots, run_key.take(), &run_rows);
        self.kept = kept;
        self.stats.episodes = self.episodes.len();
        self.stats.kept = kept;
        self.stats.slots = slots.len();
        self.stats.retained_bytes = self.episodes.iter().map(Episode::retained_bytes).sum();
        self.slots = slots;
        // The queue's order over the same rows the rail is showing, so a filter is one question
        // answered once: the worklist ranks what the filter kept (`reco-c05`).
        let on_screen: Vec<usize> = self.slots.iter().filter_map(Slot::index).collect();
        self.risked = on_screen
            .iter()
            .filter(|index| self.episodes[**index].risk().is_some())
            .count();
        self.unread = on_screen
            .iter()
            .filter(|index| self.episodes[**index].is_unread())
            .count();
        let mut ranked = on_screen;
        ranked.sort_by(|left, right| {
            crate::worklist::precede(&self.episodes[*left], &self.episodes[*right])
        });
        self.ranked = ranked;
        self.cursor = self.cursor.min(self.slots.len().saturating_sub(1));
        self.clamp_offset();
    }

    /// Emit one group: its header, then its rows, or its header and the counted marker if collapsed.
    fn flush(&self, slots: &mut Vec<Slot>, key: Option<GroupKey>, rows: &[usize]) {
        let Some(key) = key else {
            // Ungrouped: rows with no header at all.
            slots.extend(rows.iter().map(|index| Slot::Episode { index: *index }));
            return;
        };
        let first = rows.first().and_then(|index| self.episodes.get(*index));
        let count = rows.len();
        let label = match &key {
            // A day header names the session too when the whole day was one session, which is the
            // common case for an agent's turn and is what ADR-006 §5.2's mock prints.
            GroupKey::Day(start) => {
                let shared = first.and_then(|episode| {
                    let session = episode.session.as_deref()?;
                    rows.iter()
                        .skip(1)
                        .all(|index| {
                            self.episodes
                                .get(*index)
                                .and_then(|ep| ep.session.as_deref())
                                == Some(session)
                        })
                        .then(|| session.to_owned())
                });
                let who = shared.as_deref().and_then(|_| {
                    let name = first.map_or("", |episode| episode.author_name.as_str());
                    (!name.is_empty()).then(|| format!(" ({name})"))
                });
                match shared {
                    Some(session) => format!(
                        "{} · session {session}{} ── {count} {}",
                        days::day_label(*start),
                        who.unwrap_or_default(),
                        plural(count, "episode")
                    ),
                    None => format!(
                        "{} ── {count} {}",
                        days::day_label(*start),
                        plural(count, "episode")
                    ),
                }
            }
            GroupKey::Session(session) => {
                let who = first.map_or_else(String::new, |episode| {
                    if episode.author_name.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", episode.author_name)
                    }
                });
                let when = first.map_or_else(String::new, |episode| {
                    format!(" · {}", days::date_only(episode.created))
                });
                format!(
                    "{session}{who} ── {count} {}{when}",
                    plural(count, "episode")
                )
            }
        };
        let collapsed = self.collapsed.contains(&key);
        slots.push(Slot::Group {
            key,
            label,
            count,
            collapsed,
        });
        if collapsed {
            slots.push(Slot::Hidden { count });
        } else {
            slots.extend(rows.iter().map(|index| Slot::Episode { index: *index }));
        }
    }

    /// Force the order up to date; the shell calls it before drawing.
    pub fn ensure_current(&mut self) {
        self.rebuild();
    }

    /// How many slots the motion keys move over.
    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Is the rail showing nothing at all?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// The slot at an index.
    #[must_use]
    pub fn slot(&self, index: usize) -> Option<&Slot> {
        self.slots.get(index)
    }

    /// Every slot — for a golden render of a short rail, and for tests that count rows.
    #[must_use]
    pub fn slots(&self) -> &[Slot] {
        &self.slots
    }

    /// The episode at a slot index.
    #[must_use]
    pub fn episode_at(&self, slot: usize) -> Option<&Episode> {
        self.slots
            .get(slot)
            .and_then(Slot::index)
            .and_then(|index| self.episodes.get(index))
    }

    /// The row under the cursor.
    #[must_use]
    pub fn selected(&self) -> Option<&Episode> {
        self.episode_at(self.cursor)
    }

    /// The cursor's slot index.
    #[must_use]
    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    /// The first slot drawn.
    #[must_use]
    pub const fn offset(&self) -> usize {
        self.offset
    }

    /// Rows that survived the filter.
    #[must_use]
    pub const fn kept(&self) -> usize {
        self.kept
    }

    /// The episode the cursor is *about*, by identity — what `m` marks, what a tab remembers
    /// (ADR-006 §1 *scope*), and what session state restores.
    #[must_use]
    pub fn selection_id(&self) -> Option<&str> {
        self.anchor.as_ref().map(|anchor| anchor.id.as_str())
    }

    /// Point the cursor at a specific episode id, as restored from session state. `false` means the
    /// id is not in what is loaded, which the caller reports rather than swallowing.
    pub fn select_id(&mut self, id: &str) -> bool {
        self.rebuild();
        let Some(index) = self.find_id(id) else {
            return false;
        };
        self.cursor = index;
        self.anchor = self.episode_at(index).map(Anchor::of);
        self.clamp_offset();
        true
    }

    /// Which member SHAs the anchor still has, for the session record. Capped at [`SESSION_MAX`] so a
    /// re-attach hint cannot grow the session row without bound.
    #[must_use]
    pub fn anchor_members(&self) -> Vec<String> {
        let mut members = self
            .anchor
            .as_ref()
            .map_or_else(Vec::new, |anchor| anchor.members.clone());
        members.truncate(SESSION_MAX);
        members
    }

    // --- motion ----------------------------------------------------------------------------------

    /// How many rows the pane has, as last reported by the shell.
    #[must_use]
    pub fn visible(&self) -> usize {
        self.visible.get()
    }

    /// Tell the rail how tall its pane is.
    pub fn set_visible(&mut self, rows: usize) {
        self.visible.set(rows.max(1));
        self.clamp_offset();
    }

    /// Move the cursor to a slot, clamped, and take the window with it.
    pub fn move_to(&mut self, slot: usize) {
        self.rebuild();
        self.cursor = if self.slots.is_empty() {
            0
        } else {
            slot.min(self.slots.len() - 1)
        };
        if let Some(episode) = self.selected() {
            self.anchor = Some(Anchor::of(episode));
        }
        self.clamp_offset();
    }

    /// Scroll the window without moving the cursor.
    pub fn scroll_by(&mut self, delta: i32) {
        self.rebuild();
        let max = self.slots.len().saturating_sub(self.visible.get());
        let next = (i64::try_from(self.offset).unwrap_or(0) + i64::from(delta))
            .clamp(0, i64::try_from(max).unwrap_or(0));
        self.offset = usize::try_from(next).unwrap_or(0);
    }

    /// Collapse or expand the group under the cursor (`<Space>`, the registry's `list.expand`).
    ///
    /// Returns what happened in words, including on the rows where nothing can be collapsed: a key
    /// that silently does nothing reads as a broken key.
    pub fn toggle_collapse(&mut self) -> String {
        self.rebuild();
        let Some(slot) = self.slot(self.cursor).cloned() else {
            return "nothing is under the cursor to collapse".to_owned();
        };
        match slot {
            Slot::Group {
                key,
                label,
                count,
                collapsed,
            } => {
                if collapsed {
                    self.collapsed.remove(&key);
                } else {
                    self.collapsed.insert(key);
                }
                self.dirty = true;
                self.rebuild();
                self.cursor = self.cursor.min(self.slots.len().saturating_sub(1));
                if collapsed {
                    format!("expanded — {count} rows are back ({label})")
                } else {
                    format!(
                        "collapsed {count} rows — `··· {count} more` says so, `<Space>` opens it"
                    )
                }
            }
            Slot::Hidden { count } => {
                format!("`··· {count} more` is what `<Space>` on the header above opens")
            }
            Slot::Episode { index } => {
                let episode = &self.episodes[index];
                format!(
                    "`{}` is one episode — `<Space>` collapses a group header, `<Enter>` opens the card",
                    episode.short_id()
                )
            }
        }
    }

    /// Jump to the first unread row in the *filtered* order (`goto.first_unread`).
    ///
    /// "Unread" means *not opened*, and the session's read set is part of that answer (`e1.11`): a row
    /// the reviewer read yesterday and did not mark is not the row to send them to today, and a jump that
    /// walks them back to it makes `goto.first_unread` a rewind button.
    pub fn goto_first_unread(&mut self) -> String {
        self.rebuild();
        if let Some(index) = self.slots.iter().position(|slot| {
            self.episode_of(slot).is_some_and(|episode| {
                // Unread in the ledger, and not opened in any session this one remembers.
                episode.is_unread() && !self.seen.contains(&episode.id)
            })
        }) {
            self.move_to(index);
            let episode = self.selected().map_or_else(String::new, Episode::short_id);
            return format!("first unread — `{episode}`");
        }
        if self.slots.is_empty() {
            return self.empty_words();
        }
        let unread = self
            .episodes
            .iter()
            .filter(|episode| episode.is_unread())
            .count();
        let unseen = self
            .episodes
            .iter()
            .filter(|episode| episode.is_unread() && !self.seen.contains(&episode.id))
            .count();
        if unseen > 0 {
            format!(
                "{unseen} unread in what is loaded, none of them in this filter — `F` edits it, \
                 `filter clear` is in the palette"
            )
        } else if unread > 0 {
            format!(
                "every unread row here was opened in an earlier session ({unread} of them) — `x` \
                 marks one accepted, `F` edits what this filter admits"
            )
        } else {
            "nothing unread in what is loaded".to_owned()
        }
    }

    /// Replace the read set (`e1.11`'s restore path).
    pub fn set_seen(&mut self, ids: impl IntoIterator<Item = String>) {
        self.seen = ids.into_iter().collect();
    }

    /// Mark one episode opened; `false` when it already was.
    pub fn mark_seen(&mut self, id: &str) -> bool {
        self.seen.insert(id.to_owned())
    }

    /// How many episodes the read set holds.
    #[must_use]
    pub fn seen_len(&self) -> usize {
        self.seen.len()
    }

    /// Jump to today's first row (`goto.today`), or to the newest row before today when the day is
    /// empty — a jump that does nothing is a dead end, and ADR-006 §6 forbids those.
    pub fn goto_today(&mut self) -> String {
        self.rebuild();
        if self.slots.is_empty() {
            return self.empty_words();
        }
        let today = days::day_start(self.now);
        if let Some(index) = self
            .slots
            .iter()
            .position(|slot| self.day_of(slot) == Some(today))
        {
            self.move_to(index);
            return format!("today — {}", days::day_label(self.now));
        }
        if let Some(index) = self
            .slots
            .iter()
            .position(|slot| self.day_of(slot).is_some_and(|day| day < today))
        {
            self.move_to(index);
            let age = self.selected().map_or_else(String::new, |episode| {
                days::relative(self.now, episode.created)
            });
            return format!(
                "nothing today — nearest before {} is `{}` ({age} old)",
                days::day_label(self.now),
                self.selected().map_or_else(String::new, Episode::short_id)
            );
        }
        format!("nothing on or before {}", days::day_label(self.now))
    }

    /// The strip under the rail: the filter, what survived it, and how much of the index is loaded.
    ///
    /// The "index holds N" half matters: a rail that was given a page and shows only it must not let
    /// the reviewer conclude the repository has been quiet (ADR-001 §7.2).
    #[must_use]
    pub fn strip(&self) -> String {
        let mut out = self.filter.summary();
        if self.episodes.is_empty() {
            out.push_str(&format!(" · {}", self.empty_words()));
            return out;
        }
        out.push_str(&format!(
            " · {} of {}",
            format_thousands(u64::try_from(self.kept).unwrap_or(u64::MAX)),
            format_thousands(u64::try_from(self.episodes.len()).unwrap_or(u64::MAX))
        ));
        let loaded = u64::try_from(self.episodes.len()).unwrap_or(u64::MAX);
        if let Some(total) = self.total.filter(|total| *total > loaded) {
            out.push_str(&format!(
                " · index holds {} ({})",
                format_thousands(total),
                self.origin
            ));
        }
        // Only said when it is true (ADR-001 §7.2 cuts both ways): the rows that read `unread` in the
        // ledger and were opened anyway are the ones a reviewer would otherwise re-read, and the wording
        // keeps the distinction the ledger cares about — opened is not marked (`e1.11`).
        let opened = self
            .episodes
            .iter()
            .filter(|episode| episode.is_unread() && self.seen.contains(&episode.id))
            .count();
        if opened > 0 {
            out.push_str(&format!(
                " · {} opened, not marked",
                format_thousands(u64::try_from(opened).unwrap_or(u64::MAX))
            ));
        }
        out
    }

    /// The windowed, drawn rows: exactly `visible` lines, each `width` cells.
    ///
    /// `&self`, because drawing must not need to mutate: the window is *computed* from the stored
    /// offset and the cursor, so a frame paints the rail without invalidating it.
    #[must_use]
    pub fn lines(&self, width: usize, visible: usize) -> Vec<StyledLine> {
        let visible = visible.max(1);
        self.visible.set(visible);
        let offset = self.window_start(visible);
        let columns = render::Columns::for_width(width);
        if self.slots.is_empty() {
            // An empty rail is a *stated* rail. ADR-001 §7.2: the pane says why it is empty and what
            // to do — `fathom index`, or `F` to widen the filter — and it fills the pane with that
            // rather than with rows nobody can read.
            let out = render::empty_lines(&self.empty_words(), width, visible);
            self.rows_built.set(out.len());
            self.chars_built
                .set(out.iter().map(StyledLine::cells).sum());
            return out;
        }
        let mut out: Vec<StyledLine> = Vec::with_capacity(visible);
        let mut chars = 0_usize;
        for row in 0..visible {
            let index = offset + row;
            let Some(slot) = self.slots.get(index) else {
                out.push(StyledLine::plain("").padded(width));
                continue;
            };
            let cursor = index == self.cursor;
            let line = match slot {
                Slot::Group { label, .. } => render::group_line(label, cursor),
                Slot::Hidden { count } => render::hidden_line(*count, cursor),
                Slot::Episode { index } => render::episode_line(
                    &self.episodes[*index],
                    self.now,
                    self.scale,
                    columns,
                    cursor,
                    width,
                ),
            };
            chars += line.cells();
            out.push(line.padded(width));
        }
        self.rows_built.set(out.len());
        self.chars_built.set(chars);
        out
    }

    /// The first slot to draw so the cursor is on screen, computed rather than stored: the pane may
    /// have been resized by the ladder since anything mutable last ran.
    fn window_start(&self, visible: usize) -> usize {
        if self.slots.is_empty() {
            return 0;
        }
        let mut start = self.offset;
        if self.cursor < start {
            start = self.cursor;
        } else if self.cursor >= start + visible {
            start = self.cursor.saturating_sub(visible - 1);
        }
        start.min(self.slots.len().saturating_sub(visible))
    }

    /// The slot on drawn line `line`, or `None` if that line is blank because the window ran out.
    ///
    /// The inverse of [`Rail::lines`], and it has to use the same `window_start` for the same
    /// `visible`: the pointer asks "which row is under me", and an answer computed a second way
    /// would be a second opinion about where the cursor is.
    #[must_use]
    pub fn slot_at(&self, visible: usize, line: usize) -> Option<usize> {
        let index = self.window_start(visible.max(1)) + line;
        self.slots.get(index).map(|_| index)
    }

    /// Say what the index holds, now that the caller has said (`reco-1nu`).
    ///
    /// Only ever touches an empty rail: rows that exist outrank a description of their absence, and a
    /// batch that arrived with its own `why_empty` said something more specific than the shell can and
    /// keeps it.
    pub fn set_index_facts(&mut self, facts: &crate::index::Facts) {
        if !self.episodes.is_empty() {
            return;
        }
        self.origin = facts.origin();
        if self.why_empty.is_none() {
            self.why_empty = Some(facts.empty_words());
        }
    }

    /// The same rows as text — `--plain`, a pipe, and the golden files read this.
    #[must_use]
    pub fn text(&self, width: usize, visible: usize) -> Vec<String> {
        self.lines(width, visible)
            .into_iter()
            .map(|line| line.text().to_owned())
            .collect()
    }

    /// The empty statement the rail prints instead of rows.
    ///
    /// Three sources, in order of who knows best: a batch's own reason, what the caller said about the
    /// index (`reco-1nu`), and the shell's fallback for a frame nobody primed.
    #[must_use]
    pub fn empty_words(&self) -> String {
        if self.episodes.is_empty() {
            return self
                .why_empty
                .clone()
                .unwrap_or_else(|| crate::index::Facts::unknown().empty_words());
        }
        if self.kept == 0 {
            return "the filter selects nothing — `F` edits it, `filter clear` is in the palette"
                .to_owned();
        }
        "rows on screen".to_owned()
    }

    // --- the internals that keep the cursor honest ------------------------------------------------

    fn episode_of(&self, slot: &Slot) -> Option<&Episode> {
        slot.index().and_then(|index| self.episodes.get(index))
    }

    fn day_of(&self, slot: &Slot) -> Option<Timestamp> {
        self.episode_of(slot)
            .map(|episode| days::day_start(episode.created))
    }

    fn find_id(&self, id: &str) -> Option<usize> {
        self.slots.iter().position(|slot| {
            self.episode_of(slot)
                .is_some_and(|episode| episode.id == id)
        })
    }

    /// Put the cursor on a row that has an episode on it.
    ///
    /// The cursor parks on the *first episode*, not the first slot: a rail that opens with its cursor
    /// on a group header shows an empty preview pane, and "what happened today" is the question the
    /// reviewer arrived with. `<Space>` can still reach a header — `k` one step gets there.
    fn land_on_episode(&mut self) {
        if self.slots.get(self.cursor).and_then(Slot::index).is_some() {
            return;
        }
        if let Some(index) = self.slots.iter().position(|slot| slot.index().is_some()) {
            self.cursor = index;
            if let Some(episode) = self.selected() {
                self.anchor = Some(Anchor::of(episode));
            }
        }
        self.clamp_offset();
    }

    /// Put the cursor back on the episode it was about, by identity, after a rebuild. `after_load`
    /// means the rows themselves changed, so the relink tiers are in play.
    fn follow_anchor(&mut self, after_load: bool) {
        let Some(anchor) = self.anchor.clone() else {
            self.cursor = 0;
            self.land_on_episode();
            self.clamp_offset();
            return;
        };
        if let Some(index) = self.find_id(&anchor.id) {
            self.cursor = index;
            if after_load {
                self.reattached = None;
            }
        } else if after_load {
            self.reattach(anchor);
        } else {
            // The filter hid the selected episode: the cursor moves to the nearest row the filter
            // kept, and the anchor keeps the *episode*, so it comes back when the filter changes.
            self.cursor = self.cursor.min(self.slots.len().saturating_sub(1));
            self.land_on_episode();
            // `land_on_episode` re-anchors to wherever the cursor fell back to. The reviewer's episode
            // is still the hidden one: taking the anchor back is what makes taking the filter off put
            // the cursor back where it was, which is the half of "selection follows the episode" that
            // a filter is able to break.
            self.anchor = Some(anchor);
            self.clamp_offset();
            return;
        }
        self.land_on_episode();
        self.clamp_offset();
    }

    /// ADR-003 §6.2's tiers, tried in order and never revised by a weaker one (RL1).
    fn reattach(&mut self, anchor: Anchor) {
        let found = self
            .find_slot(|episode| {
                !anchor.fallback.is_empty() && episode.fallback_fingerprint == anchor.fallback
            })
            .map(|index| (index, 1u8))
            .or_else(|| {
                self.find_slot(|episode| {
                    !anchor.shape.is_empty() && episode.shape_fingerprint == anchor.shape
                })
                .map(|index| (index, 2u8))
            })
            .or_else(|| {
                self.find_slot(|episode| {
                    episode
                        .members
                        .iter()
                        .any(|member| anchor.members.contains(member))
                })
                .map(|index| (index, 3u8))
            });
        match found {
            Some((index, tier)) => {
                self.cursor = index;
                if let Some(episode) = self.selected() {
                    self.anchor = Some(Anchor::of(episode));
                }
                self.reattached = Some((tier, tier_words(tier).to_owned()));
            }
            None => {
                // I2: the identity is gone and no tier reached it. The cursor goes to the top and the
                // rail says the episode left — the alternative is a highlight on a row nobody chose.
                self.cursor = 0;
                self.anchor = self.selected().map(Anchor::of);
                self.reattached = Some((4, tier_words(4).to_owned()));
            }
        }
    }

    fn find_slot(&self, predicate: impl Fn(&Episode) -> bool) -> Option<usize> {
        self.slots
            .iter()
            .position(|slot| self.episode_of(slot).is_some_and(&predicate))
    }

    /// Keep the cursor on screen: the window follows the cursor rather than the other way round,
    /// because a cursor that has scrolled off is indistinguishable from a frozen screen.
    fn clamp_offset(&mut self) {
        if self.slots.is_empty() {
            self.cursor = 0;
            self.offset = 0;
            return;
        }
        self.cursor = self.cursor.min(self.slots.len() - 1);
        if self.cursor < self.offset {
            self.offset = self.cursor;
        } else if self.cursor >= self.offset + self.visible.get() {
            self.offset = self.cursor.saturating_sub(self.visible.get() - 1);
        }
        let max = self.slots.len().saturating_sub(self.visible.get());
        self.offset = self.offset.min(max);
    }
}
