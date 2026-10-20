//! `fathom_tui::session` — the reviewer's working state, as a value with keys (`e1.11`).
//!
//! A review session is the expensive thing a reviewer owns and fathom owns nothing else of: an hour of
//! narrowing, a cursor sitting on the episode they were about to sign off, three tabs holding the four
//! questions they still have. Losing it at `<C-c>` means starting the *question* over, which is the one
//! cost that makes a review tool feel like a toy. So the shell's whole state is put here, encoded, and
//! handed to `fathom-cli`, which is the only crate with a store handle — `fathom-tui` has none and does
//! not grow one to save four lines (`lib.rs` rule 1, lint `E01`).
//!
//! # What the keys are
//!
//! ```text
//! tui.state_version = 1                 what wrote this (a mismatch is reported, never fatal)
//! tui.view          = timeline          which view was live
//! tui.pane          = 1                 which pane in its focus ring had focus
//! tui.zoom          = 0|1               `<C-w>o`
//! tui.size          = 100x30            the frame the reviewer last read at
//! tui.pane_sizes    = 62,38             the widths that frame produced, pane by pane
//! tui.tabs          = timeline:e8f1:1:1  the tab list: `view:episode:pane:pinned`, `;`-joined
//! tui.mark          = e8f1a2…           the `m` mark `c` compares against
//! tui.search        = panic=            the last search submitted
//! timeline.filter / timeline.selection / timeline.members   the rail's half (`e1.6`'s [`timeline::Session`])
//! unread:<episode> = read               one row per episode this reviewer opened
//! ```
//!
//! `unread:<episode>` is deliberately **not** `review_state`. The ledger says what a reviewer *decided*;
//! this says what they *opened*, and a decision is class M while an opened-marker is class E — durable
//! enough to be worth resuming, disposable enough that losing it costs a keystroke and not a fact
//! (ADR-004 §10). Promoting one into the other is the sign-off surface's job (`x`, `e4.6`), not this
//! module's.
//!
//! # The three rules every key obeys
//!
//! * **Decoding is total.** An unknown key, an unknown view, a `tui.size` of `banana`, a value with a
//!   newline in it: each is one warning and a default, never a failed start. ADR-004 §9.4 is the rule
//!   this implements — a torn class-E write "is dropped, defaults are used, a warning is printed" — and
//!   a state file that can stop the app from starting is a state file that wins an argument it lost.
//! * **A newer version is read, not refused.** Unknown keys are left alone (they are not ours to delete)
//!   and known ones are decoded, so `fathom 0.2` does not make `fathom 0.1` unable to open the
//!   repository it was reviewing.
//! * **Nothing is recorded that has no reader.** Every key below is applied on the next start, printed
//!   by `fathom tui --show-session`, or both. `tui.pane_sizes` is the one derived value: it cannot be
//!   *demanded* of a terminal, so it is checked against the frame and reported when they disagree,
//!   which is what makes it worth a row instead of a decoration.

use std::collections::BTreeSet;

use crate::app::{App, Tab};
use crate::layout::pane_widths;
use crate::timeline;
use crate::views::view_for_name;

/// The schema this build writes. A stored copy from a *newer* fathom is decoded where it can be and
/// reported where it cannot; a copy from an older one is read and rewritten at this number.
pub const STATE_VERSION: u32 = 1;

/// What wrote the state. Field is an integer, not a date, so a clock skew cannot look like a migration.
pub const KEY_VERSION: &str = "tui.state_version";
/// Which view was live.
pub const KEY_VIEW: &str = "tui.view";
/// Which pane of that view's focus ring had focus.
pub const KEY_PANE: &str = "tui.pane";
/// Whether the focused pane had the whole frame.
pub const KEY_ZOOM: &str = "tui.zoom";
/// The frame size the reviewer last read at, `COLSxROWS`.
pub const KEY_SIZE: &str = "tui.size";
/// The pane widths that frame produced, comma-separated in focus-ring order.
pub const KEY_PANE_SIZES: &str = "tui.pane_sizes";
/// The tab list: `view:episode:pane:pinned` rows joined by `;`.
pub const KEY_TABS: &str = "tui.tabs";
/// The `m` mark `c` compares against.
pub const KEY_MARK: &str = "tui.mark";
/// The last search query submitted with `/` then `<Enter>`.
pub const KEY_SEARCH: &str = "tui.search";
/// Prefix of the read/unread set: `unread:<episode-id>` present means "this reviewer opened it".
pub const READ_PREFIX: &str = "unread:";
/// The value every `unread:` row carries. A row that says something else is damage, not a new state.
pub const READ_VALUE: &str = "read";

/// The key prefixes this module owns, and the prefix `fathom-cli` reads.
///
/// Anything else in `session_state` belongs to another subsystem and is never written from here — which
/// is what lets two fathoms, a daemon and a future `fathom store prune --state` share one table.
pub const PREFIXES: [&str; 3] = ["tui.", "timeline.", "unread:"];

/// Tabs restored: the tab bar runs out of labels at nine (ADR-006 §5.0).
pub const MAX_TABS: usize = 9;
/// Rows in the read/unread set. A reviewer who opened 4 000 episodes is not going to be told which
/// one they were on by a row list, so the set is the most recent ones and says it is capped.
pub const MAX_READ: usize = 512;
/// Characters in one stored value. Anything longer is either a bug in this module or damage; both are
/// reported, and neither is loaded.
pub const MAX_VALUE: usize = 4096;
/// Characters of a search query worth remembering — longer than this and the reviewer is retyping it.
pub const MAX_SEARCH: usize = 120;
/// The smallest frame a stored size may claim (the narrowest view floor, `ambient` at 40 columns,
/// minus its borders), and the largest (`tui.size` from a reviewer on a 6K monitor is a fact, not a
/// licence to allocate).
const SIZE_MIN: u16 = 20;
const SIZE_MAX: u16 = 4096;

/// What came back, and what did not.
#[derive(Debug, Clone, Default)]
pub struct Restored {
    /// One line per thing applied, in the reviewer's words — the toast and the notices.
    pub applied: Vec<String>,
    /// One line per value this build could not use, naming the key and what it held.
    pub warnings: Vec<String>,
    /// The stored frame size, if one was stored and parsed. `fathom-cli` uses it as the size to draw
    /// at when the terminal cannot answer (`--plain`, a pipe, a `size()` that failed).
    pub size: Option<(u16, u16)>,
    /// How many `unread:` rows came back: the read set this reviewer is resuming into.
    pub opened: usize,
}

impl Restored {
    /// The toast line for a resume: what came back, or the honest "nothing to restore".
    #[must_use]
    pub fn summary(&self) -> String {
        if self.applied.is_empty() {
            return if self.warnings.is_empty() {
                "nothing to restore — this is a first session in this repository".to_owned()
            } else {
                "nothing usable was stored, so the defaults are in force".to_owned()
            };
        }
        format!("session restored — {}", self.applied.join(", "))
    }
}

/// Is `key` one this module (or `e1.6`'s timeline half) owns?
#[must_use]
pub fn owns(key: &str) -> bool {
    PREFIXES.iter().any(|prefix| key.starts_with(prefix))
}

/// The whole shell's state as the rows to store.
///
/// Includes [`timeline::Session::entries`], because the rail's filter, cursor and member SHAs *are*
/// session state and the rail is the only place that knows how to encode them.
#[must_use]
pub fn snapshot(app: &App) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = Vec::new();
    rows.push((KEY_VERSION.to_owned(), STATE_VERSION.to_string()));
    rows.push((KEY_VIEW.to_owned(), app.view.name().to_owned()));
    rows.push((KEY_PANE.to_owned(), app.pane.to_string()));
    rows.push((KEY_ZOOM.to_owned(), u8::from(app.zoomed).to_string()));
    rows.push((
        KEY_SIZE.to_owned(),
        format!("{}x{}", app.size.0, app.size.1),
    ));
    let widths: Vec<String> = pane_widths(app.view, app.size.0, &app.fit.applied)
        .into_iter()
        .map(|(_pane, width)| width.to_string())
        .collect();
    rows.push((KEY_PANE_SIZES.to_owned(), widths.join(",")));
    let tabs: Vec<String> = app
        .tabs
        .iter()
        .take(MAX_TABS)
        .map(|tab| {
            format!(
                "{}:{}:{}:{}",
                tab.view.name(),
                esc(tab.scope.episode.as_deref().unwrap_or("")),
                tab.pane,
                u8::from(tab.pinned)
            )
        })
        .collect();
    rows.push((KEY_TABS.to_owned(), tabs.join(";")));
    if let Some(mark) = &app.scope.mark {
        rows.push((KEY_MARK.to_owned(), esc(mark)));
    }
    if let Some(query) = app.last_search.as_deref().filter(|query| !query.is_empty()) {
        rows.push((KEY_SEARCH.to_owned(), esc(query)));
    }
    if !app.recent_searches.is_empty() {
        rows.push((
            fathom_analyze::search::history::HISTORY_KEY.to_owned(),
            fathom_analyze::search::history::history_value(&app.recent_searches),
        ));
    }
    for (key, value) in app.timeline.session().entries() {
        rows.push((key, value));
    }
    for id in app.opened.iter().rev().take(MAX_READ) {
        rows.push((format!("{READ_PREFIX}{}", esc(id)), READ_VALUE.to_owned()));
    }
    rows
}

/// Put stored rows onto a fresh `App`.
///
/// Order is the order the state depends in: the frame's own keys first (which view, which pane, which
/// tabs), then the rail's, because the rail's cursor only means something once the rows and the filter
/// that decide which rows exist are in place.
#[must_use]
pub fn restore(app: &mut App, entries: &[(String, String)]) -> Restored {
    let mut out = Restored::default();
    // Damage is judged by one function — the same one `fathom-cli` quarantines with — so "what did not
    // survive" is a single answer in two voices and not two rules that drift apart. A row that is merely
    // *unrecognised* (a key, a view or a value this build has never heard of) is not damage and is never
    // discarded: a newer fathom has to be able to read back what it wrote.
    let broken = damaged(entries);
    let skip: BTreeSet<String> = broken.iter().map(|(key, _)| key.clone()).collect();
    for (key, why) in &broken {
        let found = entries
            .iter()
            .find(|(have, _)| have == key)
            .map_or("", |(_, value)| value.as_str());
        out.warnings.push(format!(
            "stored `{key}` was `{}` — {why}; the default is in force for it",
            show(found)
        ));
    }
    let usable: Vec<(String, String)> = entries
        .iter()
        .filter(|(key, _)| !skip.contains(key))
        .cloned()
        .collect();
    let entries: &[(String, String)] = &usable;
    let get = |key: &str| {
        entries
            .iter()
            .find(|(have, _)| have == key)
            .map(|(_, value)| value.clone())
    };
    let reject = |out: &mut Restored, key: &str, value: &str, want: &str| {
        out.warnings.push(format!(
            "stored `{key}` was `{}` — {want}; the default is in force for it",
            show(value)
        ));
    };

    if let Some(version) = get(KEY_VERSION) {
        match version.trim().parse::<u32>().ok() {
            Some(found) if found > STATE_VERSION => out.warnings.push(format!(
                "session state was written by a newer fathom (state v{found}, this build knows \
                 v{STATE_VERSION}) — what it recognises was applied and the rest was left alone"
            )),
            Some(found) if found < STATE_VERSION => out.applied.push(format!(
                "session state v{found} was read and will be written back at v{STATE_VERSION}"
            )),
            Some(_) => {}
            None => reject(&mut out, KEY_VERSION, &version, "not a version number"),
        }
    }

    if let Some(size) = get(KEY_SIZE) {
        match parse_size(&size) {
            Some((cols, rows)) => {
                out.size = Some((cols, rows));
                app.resize(cols, rows);
            }
            None => reject(
                &mut out,
                KEY_SIZE,
                &size,
                "want `COLSxROWS` between 20 and 4096",
            ),
        }
    }

    let view = match get(KEY_VIEW) {
        Some(name) => match view_for_name(name.trim()) {
            Some(view) => Some(view),
            None => {
                reject(&mut out, KEY_VIEW, &name, "this build has no such view");
                None
            }
        },
        None => None,
    };
    let stored_pane = get(KEY_PANE).and_then(|value| value.trim().parse::<usize>().ok());
    if let Some(value) = get(KEY_PANE).filter(|_| stored_pane.is_none()) {
        reject(&mut out, KEY_PANE, &value, "want a pane index");
    }
    if let Some(value) = get(KEY_ZOOM) {
        match value.trim() {
            "1" => app.zoomed = true,
            "0" => app.zoomed = false,
            other => reject(&mut out, KEY_ZOOM, other, "want `0` or `1`"),
        }
    }
    let mark = get(KEY_MARK).map(|mark| unescape(&mark));
    if let Some(query) = get(KEY_SEARCH) {
        let query = unescape(&query);
        if query.is_empty() {
            app.last_search = None;
        } else {
            app.last_search = Some(chars(&query, MAX_SEARCH));
            out.applied
                .push(format!("last search `{}`", chars(&query, 24)));
        }
    }

    // The recent searches, in the search module's own encoding — one row, `|`-escaped, newest first. The
    // box reads them back into `↑`; the shell does not re-derive a history from the last query, because a
    // reviewer who searched for three things this morning is not looking for the third one.
    app.recent_searches = fathom_analyze::search::history::recent(entries);
    if !app.recent_searches.is_empty() {
        out.applied.push(format!(
            "{} recent searches",
            app.recent_searches.len()
        ));
    }

    let ids: Vec<String> = entries
        .iter()
        .filter(|(key, _)| key.starts_with(READ_PREFIX))
        .filter_map(|(key, value)| {
            if value.trim() != READ_VALUE {
                out.warnings.push(format!(
                    "stored `{key}` was `{}` — an unread row says `{READ_VALUE}`; it was ignored",
                    show(value)
                ));
                return None;
            }
            let id = unescape(key.trim_start_matches(READ_PREFIX));
            (!id.is_empty()).then_some(id)
        })
        .take(MAX_READ)
        .collect();
    out.opened = ids.len();
    if !ids.is_empty() {
        app.opened = ids.clone();
        app.timeline.set_seen(ids);
    }

    // Tabs before the rail: a restored tab list decides which view is live and which scope the rail
    // belongs to, and the rail's cursor is reported per tab.
    if let Some(tabs) = get(KEY_TABS) {
        let parsed = parse_tabs(&tabs, &mut out);
        if !parsed.is_empty() {
            app.tabs = parsed;
            app.active_tab = app.active_tab.min(app.tabs.len() - 1);
            let tab = app.tabs[app.active_tab].clone();
            app.view = tab.view;
            app.scope = tab.scope.clone();
            app.pane = tab.pane;
            app.refit();
            let pinned = app.tabs.iter().filter(|tab| tab.pinned).count();
            out.applied.push(format!(
                "{} {} ({} pinned)",
                app.tabs.len(),
                plural(app.tabs.len(), "tab"),
                pinned
            ));
        }
    }
    if let Some(view) = view {
        // `tui.view` wins over the live tab's, because a run that switched views after touching a tab
        // changed the view and nothing else, and the last thing the reviewer saw is what should come
        // back.
        app.view = view;
        app.refit();
    }
    if let Some(pane) = stored_pane {
        let len = app.ring().len().max(1);
        app.pane = pane.min(len.saturating_sub(1));
    }
    // The mark after the tabs, because adopting a tab overwrites `app.scope`, and the mark is the
    // frame's and not the tab's (`m` marks one episode for `c` to compare against).
    if let Some(mark) = mark.filter(|mark| !mark.is_empty()) {
        app.scope.mark = Some(mark);
        out.applied.push("the `m` mark".to_owned());
    }

    // The rail's half, last: the filter decides which rows exist, then the cursor lands on one.
    if entries.iter().any(|(key, _)| key.starts_with("timeline.")) {
        let said = app.timeline.apply_session(entries);
        if said != "nothing to restore" {
            out.applied.push(said);
        }
    }

    if let (Some((cols, rows)), Some(stored)) = (out.size, get(KEY_PANE_SIZES)) {
        // The derived value gets checked rather than trusted: a reviewer who left a 120×40 terminal and
        // came back to a 90×30 one must be told the layout is not the one they left, not shown a
        // different one and silently.
        let now: Vec<String> = pane_widths(app.view, cols, &app.fit.applied)
            .into_iter()
            .map(|(_pane, width)| width.to_string())
            .collect();
        let now = now.join(",");
        if stored.trim() != now {
            out.warnings.push(format!(
                "pane sizes are {now} here and {stored} where you left them — the layout is derived \
                 from the frame, so it follows the terminal it is in"
            ));
        } else {
            out.applied
                .push(format!("layout {cols}×{rows} with panes {now}"));
        }
    }
    out
}

/// The size of a frame a stored value claims, clamped into the range a terminal can hold.
fn parse_size(text: &str) -> Option<(u16, u16)> {
    let (cols, rows) = text.trim().split_once(['x', 'X'])?;
    let cols = cols.trim().parse::<u32>().ok()?;
    let rows = rows.trim().parse::<u32>().ok()?;
    let span = |value: u32| {
        u16::try_from(value)
            .ok()
            .filter(|v| (SIZE_MIN..=SIZE_MAX).contains(v))
    };
    Some((span(cols)?, span(rows)?))
}

/// Decode `tui.tabs`. A row whose view this build does not know is dropped with a warning; the rest of
/// the list still comes back, because losing three tabs over one typo is a worse outcome than three
/// tabs.
fn parse_tabs(text: &str, out: &mut Restored) -> Vec<Tab> {
    let mut tabs: Vec<Tab> = Vec::new();
    for row in text.split(';').take(MAX_TABS) {
        if row.is_empty() {
            continue;
        }
        let mut fields = row.split(':');
        let (Some(name), Some(episode), Some(pane), Some(pinned)) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            out.warnings.push(format!(
                "stored tab row `{}` is not `view:episode:pane:pinned`",
                show(row)
            ));
            continue;
        };
        let Some(view) = view_for_name(name) else {
            out.warnings
                .push(format!("stored tab view `{name}` is unknown to this build"));
            continue;
        };
        let episode = unescape(episode);
        tabs.push(Tab {
            view,
            scope: crate::app::Scope {
                episode: (!episode.is_empty()).then_some(episode),
                mark: None,
                filter: None,
            },
            pane: pane.trim().parse::<usize>().unwrap_or(0),
            pinned: pinned.trim() == "1",
        });
    }
    tabs
}

/// What a finished run leaves behind, in the form `fathom-cli` can write (`e1.11`).
#[derive(Debug, Clone, Default)]
pub struct Delta {
    /// Did the reviewer change anything worth a row?
    ///
    /// `false` is the `--fresh` acceptance criterion: a run that was looked at and left alone writes
    /// nothing, touches no `mtime`, and cannot overwrite what the last real session stored. It is
    /// computed by comparing the snapshot taken when the frame was first drawn with the one taken when
    /// it came down, so a resize, a keystroke or a scroll counts and an idle look-around does not.
    pub changed: bool,
    /// The whole snapshot as the run ended — every key this module owns, not just the moved ones.
    ///
    /// `fathom-cli` diffs this against what it read at startup and so knows which rows are *changed*,
    /// which are *new*, and which the reviewer deleted.
    pub entries: Vec<(String, String)>,
}

/// Compare two snapshots (`before` is what was on screen when the first frame was drawn).
#[must_use]
pub fn delta(before: &[(String, String)], after: &[(String, String)]) -> Delta {
    let same = before.len() == after.len()
        && after
            .iter()
            .all(|row| before.iter().any(|had| had.0 == row.0 && had.1 == row.1));
    Delta {
        changed: !same,
        entries: after.to_vec(),
    }
}

/// The percent-escape half of this module's storage form.
///
/// Only the characters the grammar uses (`;` `:` `,` `%` `=`), the whitespace that would make a log line
/// lie about how many rows it printed, and anything non-printable are escaped; a review of a repository
/// called `src/store` stays readable in `--show-session`.
fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '%' => out.push_str("%25"),
            ';' => out.push_str("%3B"),
            ':' => out.push_str("%3A"),
            ',' => out.push_str("%2C"),
            '=' => out.push_str("%3D"),
            c if c.is_control() || c == '\n' || c == '\t' => {
                for byte in c.encode_utf8(&mut [0u8; 4]).as_bytes() {
                    out.push_str(&format!("%{byte:02X}"));
                }
            }
            c => out.push(c),
        }
    }
    out
}

/// Undo [`esc`]. An escape this build did not write is copied through unchanged rather than dropped: a
/// value from a newer fathom should arrive at that fathom intact.
fn unescape(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(text.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&text[index + 1..index + 3], 16)
        {
            out.push(byte);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A value's first 40 characters, one line, for a warning that has to name what it found.
fn show(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if out.chars().count() >= 40 {
            out.push('…');
            break;
        }
        if ch.is_control() {
            out.push_str(&format!("\\u{{{:X}}}", u32::from(ch)));
        } else {
            out.push(ch);
        }
    }
    out
}

/// `text` cut to at most `limit` characters (never mid-grapheme-cluster: `chars()` is the unit).
fn chars(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

/// One or two, in English — the same rule the rail uses for its group headers.
fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        noun.to_owned()
    } else {
        format!("{noun}s")
    }
}

/// The value an oversized row would have to be truncated to: `None` means "do not load this", and the
/// caller quarantines it. `fathom-cli`'s job, because quarantine is a filesystem act and this crate has
/// no filesystem to act on.
#[must_use]
pub fn too_long(value: &str) -> bool {
    value.len() > MAX_VALUE
}

/// Which stored rows are **damage**, as `(key, reason)`.
///
/// The line is drawn at *"could any fathom have written this?"*, not at *"does this build like it?"*:
///
/// * a value too long for the format, a raw control character where the format escapes one, a
///   `tui.size` that is not `COLSxROWS`, a `tui.pane` that is not a number — none of those is a value
///   any version writes, so they are a torn write or a hand edit, and `fathom-cli` moves them to
///   `.fathom/quarantine/` (ADR-004 §9.1 step 4, §9.4);
/// * an unknown key, an unknown view name, a state version above this build's, a `unread:` row with a
///   verdict in it — all legitimate writes from a fathom newer than this one, so they are reported and
///   **left in place** for that fathom to read back.
///
/// `restore` runs this first and refuses to apply what it names, which is what makes "corrupt state
/// self-heals rather than refuses to start" one rule instead of two.
#[must_use]
pub fn damaged(entries: &[(String, String)]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (key, value) in entries {
        // A key this module does not own is not this module to judge.
        if !owns(key) {
            continue;
        }
        if too_long(value) {
            out.push((
                key.clone(),
                format!("longer than the {MAX_VALUE} bytes the format allows"),
            ));
            continue;
        }
        if key.chars().any(char::is_control) || value.chars().any(char::is_control) {
            out.push((
                key.clone(),
                "holds a control character, which this format escapes".to_owned(),
            ));
            continue;
        }
        if key == READ_PREFIX {
            out.push((
                key.clone(),
                "an `unread:` row with no episode id".to_owned(),
            ));
            continue;
        }
        let reason = match key.as_str() {
            KEY_VERSION => {
                if value.trim().parse::<u32>().ok().is_some() {
                    None
                } else {
                    Some("not a state version number")
                }
            }
            KEY_PANE => {
                if value.trim().parse::<usize>().ok().is_some() {
                    None
                } else {
                    Some("not a pane index")
                }
            }
            KEY_ZOOM if !matches!(value.trim(), "0" | "1") => Some("not `0` or `1`"),
            KEY_SIZE => {
                if size_shape(value) {
                    None
                } else {
                    Some("not `COLSxROWS`")
                }
            }
            KEY_PANE_SIZES => {
                let shaped = value.split(',').all(|width| {
                    width.trim().is_empty() || width.trim().parse::<u16>().ok().is_some()
                });
                if shaped {
                    None
                } else {
                    Some("not a list of widths")
                }
            }
            KEY_TABS => {
                // `view:episode:pane:pinned` rows, `;`-joined. An odd field count is a torn row.
                let shaped = value
                    .split(';')
                    .filter(|row| !row.is_empty())
                    .all(|row| row.split(':').count() == 4);
                if shaped {
                    None
                } else {
                    Some("not `view:episode:pane:pinned` rows")
                }
            }
            _ => None,
        };
        if let Some(reason) = reason {
            out.push((key.clone(), reason.to_owned()));
        }
    }
    out
}

/// Does `text` have the shape `COLSxROWS` (either case, optional spaces)? Range is a separate question:
/// a size out of range is a disagreement to report, not damage to quarantine.
fn size_shape(text: &str) -> bool {
    let Some((cols, rows)) = text.trim().split_once(['x', 'X']) else {
        return false;
    };
    cols.trim().parse::<u32>().ok().is_some() && rows.trim().parse::<u32>().ok().is_some()
}

/// Re-export so the CLI can name the timeline's keys without reaching into a submodule.
pub use timeline::{KEY_FILTER, KEY_MEMBERS, KEY_SELECTION};
