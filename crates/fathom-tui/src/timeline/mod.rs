//! `fathom_tui::timeline` — the timeline view (`e1.6`): the episode rail, its filters, its jumps and
//! its preview.
//!
//! This is the home the reviewer reaches with `t`: *"what happened, in what order, and which of it
//! have I not read?"* The shell owns the frame, the focus ring and every keystroke (ADR-006 §1, §7);
//! this module owns what the two panes mean, and it is built to four properties, one per acceptance
//! criterion:
//!
//! * **A 10 000-episode rail scrolls in a frame budget and in constant memory.** The rail holds rows,
//!   not strings: the filtered order, the group headers, the facet counts and the sparkline scale are
//!   rebuilt when the data or the filter changes, and a scroll keystroke builds exactly the `visible`
//!   rows it draws. [`RailStats`] is what the bench asserts against, so a change that makes the frame
//!   cost grow with the index goes red instead of shipping a 40 ms frame (`tests/timeline_bench.rs`).
//! * **Filters compose and persist.** [`Filter`] is AND across dimensions and OR inside one — the
//!   store's rule, restated because the TUI may not name the store — and [`Filter::encode`] is the
//!   form [`session`] hands to `session_state`.
//! * **Selection follows the episode.** The cursor's identity is an [`Anchor`], and a reload re-attaches
//!   it along ADR-003 §6's tiers, saying which one fired. A rebase moves an id; it does not move a
//!   reviewer's cursor somewhere else without a word about it.
//! * **Everything is reachable without a mouse, on the published keymap.** `j`/`k`/`H`/`M`/`L`/`gg`/`G`/
//!   `<C-d>`/`<C-u>`/`<Space>`/`<Enter>`/`<Tab>` from `list`, plus the four registry entries this ticket
//!   owns: `F` and the palette's `filter.clear`, `goto.first_unread`, `goto.today`. No key was invented:
//!   grouping rides on the filter state, because K1 is satisfied by the palette and K4 forbids taking
//!   anything from the reserved set.
//!
//! And one property the ticket does not state but ADR-001 §7.2 does: the rail never hides silently. A
//! filter that dropped rows says how many, a collapsed group prints `··· 17 more`, a rail holding half
//! the index says `index holds 3 412`, an unscored column prints `?`, and an episode whose commits are
//! gone is marked `gone`.

mod card;
mod days;
mod editor;
mod episode;
mod filter;
mod rail;
mod render;
mod session;

pub use card::lines as card_lines;
pub use days::{date_only, date_or_any, day_label, day_start, relative};
pub use editor::{Editor, Next as EditorNext, Row as EditorRow, Stage as EditorStage, Target};
pub use episode::{
    AuthorKind, Batch, Episode, EpisodeSource, GroupKey, MemorySource, NoIndex, ReviewState,
};
pub use filter::{Dimension, FACET_CAP, Facets, Filter, GroupBy};
pub use rail::{Anchor, Rail, RailStats, Slot};
pub use render::{Columns, SPARK_CELLS, Scale};
pub use session::{KEY_FILTER, KEY_MEMBERS, KEY_SELECTION, Restore, SESSION_MAX, Session};

use fathom_core::time::Timestamp;

use crate::style::StyledLine;

/// The timeline view's own state: the rail, the filter, and the `F` overlay's position in it.
///
/// Held by [`crate::app::App`] next to the diff viewer's own state, and for the same reason: the shell
/// cannot hold half an episode list and stay honest about which pane's selection it is drawing.
#[derive(Debug, Default)]
pub struct Timeline {
    /// The rail.
    rail: Rail,
    /// The `filter.edit` chooser.
    editor: Editor,
    /// The last thing the view decided, for the toast line.
    pub notice: Option<String>,
}

impl Timeline {
    /// A rail with nothing loaded, drawn for `now`.
    #[must_use]
    pub fn new(now: Timestamp) -> Self {
        Timeline {
            rail: Rail::new(now),
            editor: Editor::closed(),
            notice: None,
        }
    }

    // --- the data ---------------------------------------------------------------------------------

    /// The instant the rail ages itself against.
    #[must_use]
    pub fn now(&self) -> Timestamp {
        self.rail.now()
    }

    /// Reset it (the frame's instant, on every draw, because a long-lived shell must not age against a
    /// timestamp it started with an hour ago).
    pub fn set_now(&mut self, now: Timestamp) {
        self.rail.set_now(now);
    }

    /// Hand the rail a batch of episodes. Returns the sentence for the toast line.
    ///
    /// A re-index comes through here too, and that is the case the wording is written for: the count
    /// changed, the rows may have moved, and the cursor's re-attachment is reported rather than
    /// assumed.
    pub fn load(&mut self, batch: Batch) -> String {
        let before = self.rail.selection_id().map(str::to_owned);
        let count = batch.len();
        let total = batch.total;
        self.rail.load(batch);
        let moved = match (before, self.rail.selection_id()) {
            (Some(had), Some(now)) if had != now => self
                .rail
                .reattached
                .as_ref()
                .map_or(String::new(), |(_, words)| {
                    format!(" · cursor re-attached, {words}")
                }),
            (Some(_), Some(_)) => String::new(),
            (Some(_), None) => " · the selected episode left the index".to_owned(),
            (None, Some(_)) => String::new(),
            (None, None) => String::new(),
        };
        // Only said when it is a fact: a rail holding every loaded episode must not hint that a page
        // is missing (ADR-001 §7.2 cuts both ways).
        let held = match total {
            Some(total) if total > u64::try_from(count).unwrap_or(u64::MAX) => {
                format!(" · index holds {}", crate::app::format_thousands(total))
            }
            _ => String::new(),
        };
        format!(
            "{} {} from {}{held}{moved}",
            crate::app::format_thousands(u64::try_from(count).unwrap_or(u64::MAX)),
            crate::timeline::rail::plural(count, "episode"),
            self.rail.origin()
        )
    }

    /// The rail, read-only.
    #[must_use]
    pub const fn rail(&self) -> &Rail {
        &self.rail
    }

    /// The rail, for the motion keys the shell routes here.
    pub fn rail_mut(&mut self) -> &mut Rail {
        &mut self.rail
    }

    /// The episode under the cursor.
    #[must_use]
    pub fn selected(&self) -> Option<&Episode> {
        self.rail.selected()
    }

    // --- the filter -------------------------------------------------------------------------------

    /// The filter in force.
    #[must_use]
    pub fn filter(&self) -> &Filter {
        self.rail.filter()
    }

    /// Replace the filter; the cursor follows the episode it was on.
    pub fn set_filter(&mut self, filter: Filter) {
        self.rail.set_filter(filter);
    }

    /// `filter.clear`.
    pub fn clear_filters(&mut self) -> String {
        let was = self.rail.filter().active();
        let group = self.rail.filter().group;
        let cleared = Filter {
            group,
            ..Filter::default()
        };
        if was == 0 && cleared.group == self.rail.filter().group {
            return "no filters were set".to_owned();
        }
        self.rail.set_filter(cleared);
        format!(
            "{was} {} cleared — every loaded episode is back",
            crate::timeline::rail::plural(was, "filter"),
        )
    }

    // --- motion -----------------------------------------------------------------------------------

    /// Move the cursor to a slot.
    pub fn move_to(&mut self, slot: usize) {
        self.rail.move_to(slot);
    }

    /// Scroll the window.
    pub fn scroll_by(&mut self, delta: i32) {
        self.rail.scroll_by(delta);
    }

    /// `<Space>`: collapse or expand the group under the cursor.
    #[must_use]
    pub fn toggle_collapse(&mut self) -> String {
        self.rail.toggle_collapse()
    }

    /// `goto.first_unread`.
    #[must_use]
    pub fn goto_first_unread(&mut self) -> String {
        self.rail.goto_first_unread()
    }

    /// Replace the read set the rail treats as opened (`e1.11`'s restore path).
    pub fn set_seen(&mut self, ids: impl IntoIterator<Item = String>) {
        self.rail.set_seen(ids);
    }

    /// Mark one episode opened in the read set.
    pub fn mark_seen(&mut self, id: &str) -> bool {
        self.rail.mark_seen(id)
    }

    /// How many episodes this session knows were opened.
    #[must_use]
    pub fn seen_len(&self) -> usize {
        self.rail.seen_len()
    }

    /// `goto.today`.
    #[must_use]
    pub fn goto_today(&mut self) -> String {
        self.rail.goto_today()
    }

    /// How many slots the motion keys move over.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rail.len()
    }

    /// Is the rail empty?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rail.is_empty()
    }

    /// The cursor's slot index.
    #[must_use]
    pub fn cursor(&self) -> usize {
        self.rail.cursor()
    }

    /// The counters, forcing a rebuild if the rail is stale.
    #[must_use]
    pub fn stats(&mut self) -> RailStats {
        self.rail.stats()
    }

    /// The counters as they stand.
    #[must_use]
    pub fn last_stats(&self) -> RailStats {
        self.rail.last_stats()
    }

    // --- drawing ---------------------------------------------------------------------------------

    /// The rail's visible rows, styled, exactly `visible` of them.
    #[must_use]
    pub fn rail_lines(&self, width: usize, visible: usize) -> Vec<StyledLine> {
        self.rail.lines(width, visible)
    }

    /// The rail's visible rows as text.
    #[must_use]
    pub fn rail_text(&self, width: usize, visible: usize) -> Vec<String> {
        self.rail.text(width, visible)
    }

    /// The preview of the selected episode, styled.
    #[must_use]
    pub fn preview_lines(&self, width: usize, visible: usize) -> Vec<StyledLine> {
        card::lines(
            self.rail.selected(),
            self.rail.now(),
            self.rail.origin(),
            width,
            visible,
        )
    }

    /// The rail's footer strip.
    #[must_use]
    pub fn strip(&self) -> String {
        self.rail.strip()
    }

    // --- the `F` surface --------------------------------------------------------------------------

    /// Open the filter chooser (`filter.edit`).
    pub fn open_editor(&mut self) {
        self.editor.open();
    }

    /// Is the chooser open?
    #[must_use]
    pub fn editor_open(&self) -> bool {
        self.editor.open
    }

    /// Close it.
    pub fn close_editor(&mut self) {
        self.editor.close();
    }

    /// Is the chooser on its value stage?
    #[must_use]
    pub fn editor_in_value(&self) -> bool {
        self.editor.in_value()
    }

    /// The chooser's rows for the current stage and buffer.
    #[must_use]
    pub fn editor_rows(&self, buffer: &str) -> Vec<EditorRow> {
        self.editor
            .rows(self.rail.filter(), self.rail.facets(), buffer)
    }

    /// Move the chooser's cursor.
    pub fn editor_move(&mut self, row: usize, len: usize) {
        self.editor.move_to(row, len);
    }

    /// The chooser's cursor index.
    #[must_use]
    pub fn editor_cursor(&self) -> usize {
        self.editor.row
    }

    /// The chooser panel's lines.
    #[must_use]
    pub fn editor_panel(&self, buffer: &str, rows: &[EditorRow], width: usize) -> Vec<String> {
        self.editor
            .panel(self.rail.filter(), rows, self.editor.row, buffer, width)
    }

    /// Choose the row under the cursor, apply what it implies, and say what happened.
    pub fn editor_apply(&mut self, index: usize, buffer: &str) -> (String, EditorNext) {
        let rows = self.editor_rows(buffer);
        let Some(row) = rows.get(index) else {
            return (
                "nothing is under the chooser's cursor".to_owned(),
                EditorNext::Stay,
            );
        };
        let (filter, said, next) =
            self.editor
                .apply(row, self.rail.filter(), buffer, self.rail.now());
        if let Some(filter) = filter {
            self.rail.set_filter(filter);
        }
        (said, next)
    }

    /// What the editor says about itself, for the toast when nothing was chosen.
    #[must_use]
    pub fn editor_message(&self) -> Option<&str> {
        self.editor.message.as_deref()
    }

    // --- session state ----------------------------------------------------------------------------

    /// What the rail would like remembered.
    #[must_use]
    pub fn session(&self) -> Session {
        Session {
            filter: self.rail.filter().clone(),
            selection: self.rail.selection_id().map(str::to_owned),
            members: self.rail.anchor_members(),
        }
    }

    /// Restore a session: the filter first (it decides which rows exist), then the cursor, then the
    /// report of what actually came back.
    ///
    /// A selection that is not in the loaded rows is *reported*, not guessed at: "the episode moved"
    /// is a fact the reviewer needs before signing off on the wrong row.
    pub fn apply_session(&mut self, entries: &[(String, String)]) -> String {
        let session = Session::from_entries(entries);
        let had_filter = !session.filter.is_clear() || session.filter.group != GroupBy::Day;
        if had_filter {
            self.rail.set_filter(session.filter.clone());
        }
        let stored = session.selection.as_deref();
        let outcome = if stored.is_some_and(|id| self.rail.select_id(id)) {
            Restore::Id
        } else if !session.members.is_empty() {
            // Tier 3, the way ADR-003 §6 does it: the id moved while the reviewer was away, and a
            // member commit may have survived into the replacement episode. The toast says so, because
            // landing on a *different* episode silently is the failure mode this whole view is built
            // to avoid.
            let members = session.members.clone();
            match self.rail.find_by_members(&members) {
                Some(index) => {
                    self.rail.move_to(index);
                    Restore::Members {
                        landed: self
                            .rail
                            .selected()
                            .map_or_else(String::new, |episode| episode.id.clone()),
                    }
                }
                None if stored.is_some() => Restore::Gone,
                None => Restore::Nothing,
            }
        } else if stored.is_some() {
            Restore::Gone
        } else {
            Restore::Nothing
        };
        session.report(had_filter, outcome)
    }
}
