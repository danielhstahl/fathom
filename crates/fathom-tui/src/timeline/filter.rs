//! `fathom_tui::timeline::filter` — the composable filter the rail runs, and its session form.
//!
//! Three things make this a type rather than a closure per view:
//!
//! * **Composition is AND across dimensions and OR inside one**, which is exactly [`EpisodeQuery`]'s
//!   rule in `fathom-store` (author AND path AND unread composes there too). One rule, two places that
//!   must never disagree about what "unread, by stahl, under `src/store`" selects — ADR-001 §7.10 again,
//!   with a test that proves the agreement on the same fixture.
//! * **It is a value, so it is persistable.** `e1.6`'s acceptance criterion says filters are *persisted
//!   in session state*, and [`Filter::encode`]/[`Filter::decode`] are the form that goes into the
//!   store's `session_state` table (the writing is `e1.6b`'s half in `fathom-cli`; the meaning is
//!   here). Decode is tolerant of unknown keys for the same reason the store's `ReviewState::parse`
//!   is: a newer fathom must not make this one fail to start.
//! * **It carries the grouping too.** "Group by" is not a filter, but ADR-006 §7 K1 says every action
//!   is bound or palette-listed, and the registry has no grouping action — inventing a key would break
//!   K3/K4 (the reserved set). `F` is the registry's own scope-editing surface, so grouping lives with
//!   the filter state and is persisted with it, and no key was invented to get it.
//!
//! [`EpisodeQuery`]: https://docs.rs/fathom-store (the store's `EpisodeQuery`; the TUI cannot name it,
//! so the rule is restated here and pinned by `tests/timeline_behaviour.rs::the_rail_and_the_store`).

use fathom_core::time::Timestamp;

use super::days;
use super::episode::{AuthorKind, Episode, ReviewState};

/// The most the facet list will offer per dimension. A facet list is a chooser, not a directory: past
/// a dozen entries a reviewer is scrolling instead of choosing, and the enumeration of a 10 000-row
/// index has to cost the same as the enumeration of a 3 000-row one.
pub const FACET_CAP: usize = 12;

/// How the rail groups its rows (ADR-006 §5.2: "grouped by day or by agent session").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroupBy {
    /// One header per UTC day. The default: the reviewer's first question is "when".
    #[default]
    Day,
    /// One header per harness session — the shape of "what did the agent do this turn".
    Session,
    /// No headers. `gg`/`G` and the jumps still mean the same thing.
    None,
}

impl GroupBy {
    /// Every grouping, in the order the editor lists them.
    pub const ALL: [GroupBy; 3] = [GroupBy::Day, GroupBy::Session, GroupBy::None];

    /// The stored spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            GroupBy::Day => "day",
            GroupBy::Session => "session",
            GroupBy::None => "none",
        }
    }

    /// Parse the stored spelling; anything unknown is the default.
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "session" => GroupBy::Session,
            "none" => GroupBy::None,
            _ => GroupBy::Day,
        }
    }
}

/// Which dimension the `F` editor is working on (ADR-006 §7.4 `filter.edit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dimension {
    /// Author name, matched case-insensitively, several at a time.
    Author,
    /// Agent vs human vs unknown attribution, several at a time.
    Kind,
    /// A path fragment (the strip's `area=`, and the store's `path`).
    Area,
    /// Reviewed state.
    State,
    /// Inclusive lower bound on the episode's last date.
    Since,
    /// Exclusive upper bound on the episode's last date.
    Until,
    /// Day / session / none.
    Group,
}

impl Dimension {
    /// Every dimension, in the order the editor lists them.
    pub const ALL: [Dimension; 7] = [
        Dimension::Author,
        Dimension::Kind,
        Dimension::Area,
        Dimension::State,
        Dimension::Since,
        Dimension::Until,
        Dimension::Group,
    ];

    /// The label the editor prints for it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Dimension::Author => "author",
            Dimension::Kind => "kind",
            Dimension::Area => "area",
            Dimension::State => "state",
            Dimension::Since => "since",
            Dimension::Until => "until",
            Dimension::Group => "group",
        }
    }

    /// Is this dimension typed as free text (author, area, dates) rather than chosen from a list?
    /// Dates are both: they have presets *and* accept a typed date.
    #[must_use]
    pub const fn is_text(self) -> bool {
        matches!(
            self,
            Dimension::Author | Dimension::Area | Dimension::Since | Dimension::Until
        )
    }
}

/// The rail's filter: every constraint that must hold, and the grouping that orders the result.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Filter {
    /// Exact author names (case-insensitive). Empty means no constraint.
    pub authors: Vec<String>,
    /// Attribution kinds. Empty means no constraint.
    pub kinds: Vec<AuthorKind>,
    /// Reviewed state. `Some(ReviewState::Read)` means *opened* — read or any verdict — which is the
    /// store's rule, restated: `Some(Read)` is not "specifically read".
    pub state: Option<ReviewState>,
    /// A path fragment; an episode matches when any path it touched contains it.
    pub area: Option<String>,
    /// Inclusive lower bound.
    pub since: Option<Timestamp>,
    /// Exclusive upper bound.
    pub until: Option<Timestamp>,
    /// How the rows group (see [`Filter`]'s note on why this rides here).
    pub group: GroupBy,
}

impl Filter {
    /// No constraints, default grouping — what a fresh rail and a `filter.clear` both produce.
    #[must_use]
    pub fn none() -> Self {
        Filter::default()
    }

    /// Does this episode survive every constraint? And across dimensions, or inside one.
    #[must_use]
    pub fn matches(&self, episode: &Episode) -> bool {
        if !self.authors.is_empty() {
            let name = episode.author_name.to_lowercase();
            if name.is_empty() || !self.authors.iter().any(|want| name == want.to_lowercase()) {
                return false;
            }
        }
        if !self.kinds.is_empty() && !self.kinds.contains(&episode.author) {
            return false;
        }
        if let Some(state) = self.state {
            let keeps = match state {
                ReviewState::Unread => episode.state.is_unread(),
                ReviewState::Read => episode.state.is_opened(),
                other => episode.state == other,
            };
            if !keeps {
                return false;
            }
        }
        if let Some(area) = &self.area {
            let needle = area.to_lowercase();
            // A path fragment matches a prefix *or* any component, so `store` finds `src/store/x.rs`
            // and `crates/fathom-store/` without the reviewer guessing separators.
            let hit = episode.paths.iter().any(|path| {
                let path = path.to_lowercase();
                path.contains(&needle)
            });
            if !hit {
                return false;
            }
        }
        if !days::in_range(episode.last, self.since, self.until) {
            return false;
        }
        true
    }

    /// How many constraints are live — the number the strip prints when there is no room for the
    /// detail (`filter: 3 (F edits)`).
    #[must_use]
    pub fn active(&self) -> usize {
        usize::from(!self.authors.is_empty())
            + usize::from(!self.kinds.is_empty())
            + usize::from(self.state.is_some())
            + usize::from(self.area.is_some())
            + usize::from(self.since.is_some())
            + usize::from(self.until.is_some())
    }

    /// Is nothing constrained?
    #[must_use]
    pub fn is_clear(&self) -> bool {
        self.active() == 0
    }

    /// Reset every constraint *and* the grouping: `filter.clear` is "put the rail back", and a filter
    /// that cleared everything except the shape of the list would leave the reviewer with a rail
    /// grouped by a session they stopped caring about three rebases ago.
    pub fn clear(&mut self) {
        *self = Filter::default();
    }

    /// The strip's text: ADR-006 §5.2's `filter: area=any state=unread (F edits)`, extended with the
    /// dimensions that are set and, when any is, how many rows survived.
    #[must_use]
    pub fn summary(&self) -> String {
        let area = self.area.as_deref().unwrap_or("any");
        let state = self
            .state
            .map_or("any".to_owned(), |state| state.to_string());
        let mut out = format!("filter: area={area} state={state}");
        if !self.authors.is_empty() {
            out.push_str(&format!(" author={}", self.authors.join("+")));
        }
        if !self.kinds.is_empty() {
            let kinds: Vec<&str> = self.kinds.iter().map(|kind| kind.as_str()).collect();
            out.push_str(&format!(" kind={}", kinds.join("+")));
        }
        if self.since.is_some() || self.until.is_some() {
            out.push_str(&format!(
                " {}…{}",
                self.since.map_or("start".to_owned(), days::iso),
                self.until
                    .map_or("now".to_owned(), days::last_included_display)
            ));
        }
        out.push_str(&format!(" group={} (F edits)", self.group.as_str()));
        out
    }

    /// The current value of one dimension, as the editor prints it beside its label.
    #[must_use]
    pub fn value_of(&self, dimension: Dimension) -> String {
        match dimension {
            Dimension::Author => join_or_any(&self.authors),
            Dimension::Kind => {
                if self.kinds.is_empty() {
                    "any".to_owned()
                } else {
                    self.kinds
                        .iter()
                        .map(|kind| kind.badge())
                        .collect::<Vec<_>>()
                        .join("+")
                }
            }
            Dimension::Area => self.area.clone().unwrap_or_else(|| "any".to_owned()),
            Dimension::State => self
                .state
                .map_or("any".to_owned(), |state| state.to_string()),
            Dimension::Since => days::date_or_any(self.since),
            Dimension::Until => self
                .until
                .map_or_else(|| "any".to_owned(), days::last_included_display),
            Dimension::Group => self.group.as_str().to_owned(),
        }
    }

    /// Apply a typed or chosen value to one dimension. `text` is the raw field; `any`/`none` clears
    /// it. `now` is the frame's instant, which the relative date forms (`today`, `7d`) are measured
    /// against — the shell passes it in because this crate does not read a clock (ADR-002 §4.4).
    ///
    /// Returns a message for the toast line, always in the reviewer's terms. Errors are the
    /// reviewer's typing, not a fault: the editor prints them back and keeps the filter as it was,
    /// which is the difference between a refusal and a surprise.
    pub fn set(
        &mut self,
        dimension: Dimension,
        text: &str,
        now: Timestamp,
    ) -> Result<String, String> {
        let text = text.trim();
        let clear = text.is_empty()
            || text.eq_ignore_ascii_case("any")
            || text.eq_ignore_ascii_case("all")
            || text.eq_ignore_ascii_case("none")
            || text == "*";
        match dimension {
            Dimension::Author => {
                if clear {
                    self.authors.clear();
                    return Ok("author filter cleared".to_owned());
                }
                // `+` stacks: the reviewer who wants two authors asked for two, not the second.
                let names: Vec<String> = text
                    .split(['+', ','])
                    .map(|name| name.trim().to_owned())
                    .filter(|name| !name.is_empty())
                    .collect();
                if names.is_empty() {
                    return Err("that author name is empty".to_owned());
                }
                self.authors = names;
                Ok(format!("author = {}", self.authors.join("+")))
            }
            Dimension::Kind => {
                if clear {
                    self.kinds.clear();
                    return Ok("kind filter cleared".to_owned());
                }
                let kinds: Vec<AuthorKind> = text
                    .split(['+', ','])
                    .filter_map(|kind| match kind.trim().to_ascii_lowercase().as_str() {
                        "agent" | "a" => Some(AuthorKind::Agent),
                        "human" | "h" => Some(AuthorKind::Human),
                        "unknown" | "?" => Some(AuthorKind::Unknown),
                        _ => None,
                    })
                    .collect();
                if kinds.is_empty() {
                    return Err(format!(
                        "`{text}` is not a kind — try agent, human or unknown"
                    ));
                }
                self.kinds = kinds;
                Ok(format!(
                    "kind = {}",
                    self.kinds
                        .iter()
                        .map(|kind| kind.badge())
                        .collect::<Vec<_>>()
                        .join("+")
                ))
            }
            Dimension::Area => {
                self.area = if clear { None } else { Some(text.to_owned()) };
                Ok(format!("area = {}", self.value_of(Dimension::Area)))
            }
            Dimension::State => {
                self.state = if clear {
                    None
                } else {
                    Some(ReviewState::parse(text))
                };
                Ok(format!("state = {}", self.value_of(Dimension::State)))
            }
            Dimension::Since => {
                self.since = days::parse_date(text, now)?;
                Ok(format!("since = {}", self.value_of(Dimension::Since)))
            }
            Dimension::Until => {
                // Stored exclusive (the store's rule), entered inclusive (the reviewer's): `until =
                // today` means through today, and prints back as today.
                self.until = days::parse_date(text, now)?.map(days::day_end);
                Ok(format!("until = {}", self.value_of(Dimension::Until)))
            }
            Dimension::Group => {
                self.group = if clear {
                    GroupBy::Day
                } else {
                    GroupBy::parse(text)
                };
                Ok(format!("group = {}", self.group.as_str()))
            }
        }
    }

    /// Toggle one value inside a multi-valued dimension (the editor's `<Enter>` on a facet row):
    /// `agent` twice means "not agent any more", and nothing else is surprised by it.
    pub fn toggle(&mut self, dimension: Dimension, value: &str) {
        match dimension {
            Dimension::Author => {
                let name = value.trim();
                if self.authors.iter().any(|have| have == name) {
                    self.authors.retain(|have| have != name);
                } else {
                    self.authors.push(name.to_owned());
                }
            }
            Dimension::Kind => {
                let kind = AuthorKind::parse(value);
                if self.kinds.contains(&kind) {
                    self.kinds.retain(|have| *have != kind);
                } else {
                    self.kinds.push(kind);
                }
            }
            _ => {}
        }
    }

    /// The session-state form: `key=value` pairs joined by `;`, percent-escaped.
    ///
    /// Stable and boring on purpose — it goes into SQLite's `session_state` and has to survive being
    /// read by a later version that knows dimensions this one has never heard of.
    #[must_use]
    pub fn encode(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !self.authors.is_empty() {
            parts.push(format!("author={}", escape(&self.authors.join("+"))));
        }
        if !self.kinds.is_empty() {
            let kinds: Vec<&str> = self.kinds.iter().map(|kind| kind.as_str()).collect();
            parts.push(format!("kind={}", escape(&kinds.join("+"))));
        }
        if let Some(state) = self.state {
            parts.push(format!("state={}", state.as_str()));
        }
        if let Some(area) = &self.area {
            parts.push(format!("area={}", escape(area)));
        }
        if let Some(since) = self.since {
            parts.push(format!("since={}", days::iso(since)));
        }
        if let Some(until) = self.until {
            parts.push(format!("until={}", days::iso(until)));
        }
        parts.push(format!("group={}", self.group.as_str()));
        parts.join(";")
    }

    /// Read a [`Filter::encode`] string back. Unknown keys and unknown values are ignored, not
    /// rejected: a filter written by a newer fathom degrades to the parts this one understands, and
    /// the strip prints what was applied — never a silent partial.
    #[must_use]
    pub fn decode(text: &str) -> Filter {
        let mut filter = Filter {
            group: GroupBy::Day,
            ..Filter::default()
        };
        for pair in text.split(';') {
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            let value = unescape(value);
            match key.trim().to_ascii_lowercase().as_str() {
                "author" => filter.authors = split_values(&value),
                "kind" => {
                    filter.kinds = split_values(&value)
                        .into_iter()
                        .map(|kind| AuthorKind::parse(&kind))
                        .collect();
                }
                "state" => filter.state = Some(ReviewState::parse(&value)),
                "area" => filter.area = Some(value),
                "since" => filter.since = parse_iso(&value),
                "until" => filter.until = parse_iso(&value),
                "group" => filter.group = GroupBy::parse(&value),
                _ => {}
            }
        }
        filter
    }
}

/// The values the reviewer can choose for one dimension, counted out of the rows on hand.
///
/// Facets come from the loaded batch, not from a query: a chooser that offers a value with no rows
/// behind it is a filter that looks like it worked and then prints an empty rail.
#[derive(Debug, Clone, Default)]
pub struct Facets {
    /// Author names, most episodes first.
    pub authors: Vec<(String, usize)>,
    /// Attribution kinds present, with counts.
    pub kinds: Vec<(AuthorKind, usize)>,
    /// Path prefixes down to three deep, most touched first — `src`, `src/store`, `tests`.
    pub areas: Vec<(String, usize)>,
    /// States present, with counts.
    pub states: Vec<(ReviewState, usize)>,
}

impl Facets {
    /// Count the facets out of a batch, capped at [`FACET_CAP`] per dimension.
    ///
    /// One pass over the rows and a bounded output, so the cost of opening the editor does not grow
    /// with the index — the same reason the rail renders a window instead of the list.
    #[must_use]
    pub fn of(episodes: &[Episode]) -> Facets {
        let mut authors: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        let mut kinds: std::collections::BTreeMap<&'static str, usize> =
            std::collections::BTreeMap::new();
        let mut areas: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        let mut states: std::collections::BTreeMap<&'static str, usize> =
            std::collections::BTreeMap::new();
        for episode in episodes {
            if !episode.author_name.is_empty() {
                *authors.entry(episode.author_name.clone()).or_insert(0) += 1;
            }
            *kinds.entry(episode.author.as_str()).or_insert(0) += 1;
            *states.entry(episode.state.as_str()).or_insert(0) += 1;
            for path in &episode.paths {
                // Depth is capped, and a path contributes its ancestors: `src/store/x.rs` is offered
                // as `src` and `src/store`, which is the two things a reviewer types.
                let mut prefix = String::new();
                for part in path.split('/').take(3) {
                    if prefix.is_empty() {
                        prefix = part.to_owned();
                    } else {
                        prefix.push('/');
                        prefix.push_str(part);
                    }
                    if prefix.is_empty() {
                        continue;
                    }
                    *areas.entry(prefix.clone()).or_insert(0) += 1;
                }
            }
        }
        Facets {
            authors: ranked(authors),
            kinds: ranked(kinds)
                .into_iter()
                .map(|(kind, count)| (AuthorKind::parse(kind), count))
                .collect(),
            areas: ranked(areas),
            states: ranked(states)
                .into_iter()
                .map(|(state, count)| (ReviewState::parse(state), count))
                .collect(),
        }
    }

    /// The rows a dimension can offer right now.
    #[must_use]
    pub fn values(&self, dimension: Dimension) -> Vec<(String, usize)> {
        match dimension {
            Dimension::Author => self.authors.clone(),
            Dimension::Kind => self
                .kinds
                .iter()
                .map(|(kind, count)| (kind.as_str().to_owned(), *count))
                .collect(),
            Dimension::Area => self.areas.clone(),
            Dimension::State => self
                .states
                .iter()
                .map(|(state, count)| (state.as_str().to_owned(), *count))
                .collect(),
            Dimension::Since | Dimension::Until => vec![
                ("today".to_owned(), 0),
                ("yesterday".to_owned(), 0),
                ("7d".to_owned(), 0),
                ("30d".to_owned(), 0),
                ("90d".to_owned(), 0),
            ],
            Dimension::Group => GroupBy::ALL
                .iter()
                .map(|group| (group.as_str().to_owned(), 0))
                .collect(),
        }
    }
}

/// Sort a facet map by count desc then key asc, and cut it at the cap — so the order is a function of
/// the data and not of `HashMap` luck.
fn ranked<T: Ord + Clone>(counts: std::collections::BTreeMap<T, usize>) -> Vec<(T, usize)> {
    let mut rows: Vec<(T, usize)> = counts.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    rows.truncate(FACET_CAP);
    rows
}

/// `a+b`, or `any`.
fn join_or_any(values: &[String]) -> String {
    if values.is_empty() {
        "any".to_owned()
    } else {
        values.join("+")
    }
}

/// Split a `+`-joined list of values.
fn split_values(text: &str) -> Vec<String> {
    text.split(['+', ','])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Percent-escape the four bytes the grammar itself uses.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            ';' => out.push_str("%3B"),
            '=' => out.push_str("%3D"),
            '%' => out.push_str("%25"),
            '+' => out.push_str("%2B"),
            _ => out.push(ch),
        }
    }
    out
}

/// Undo [`escape`].
fn unescape(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = &text[index + 1..index + 3];
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte as char);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index] as char);
        index += 1;
    }
    out
}

/// Parse an ISO date back out of a session string; a bad one is `None`, i.e. the constraint is off.
/// Relative forms are not encoded (they are a way of *computing* a bound, not the bound), so the
/// marker only has to be a stable instant.
fn parse_iso(text: &str) -> Option<Timestamp> {
    days::parse_date(text, Timestamp::UNIX_EPOCH).ok().flatten()
}
