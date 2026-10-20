//! `search::history` — the reviewer's recent and saved searches, in the rows the session carries
//! (`e5.2`, `reco-mkc`).
//!
//! This half of the search corpus is *not* the store: it is a handful of `(key, value)` rows that the shell
//! already reads and writes back for every other piece of state it keeps (`ADR-004 §4.7`,
//! `fathom_tui::session`). Keeping it out of the store-backed module is what lets a binary with no backend
//! still remember what the reviewer was looking for, and it is the reason this module is compiled when
//! neither SQLite feature is on.
//!
//! Two rules:
//!
//! * **The keys live under the shell's own prefix.** `tui.` is the namespace the shell is allowed to write
//!   (see `fathom_tui::session::owns`); a search key outside it is a key nobody writes back, and a recent
//!   list that is never written back is a feature that silently does not exist.
//! * **A query round-trips whole.** `|` separates the list and `\` escapes it, so a query that contains a
//!   pipe comes back as one query; the clip to [`QUERY_MAX`] happens before the escaping so a bound on the
//!   typed text cannot cut an escape in half.

/// The session key whose value is the reviewer's recent searches, newest first, `|`-joined.
///
/// One row rather than one row per search because the session table is a small key/value table that the
/// shell reads whole (`ADR-004 §4.7`), and because a prefix scan over per-search rows would order them by
/// name rather than by recency — which is the whole content of the feature.
pub const HISTORY_KEY: &str = "tui.search.history";

/// The prefix a saved search is stored under, so the palette's listing is a prefix scan.
///
/// Under `tui.` because the shell owns the session rows it writes (ADR-004 §4.7, `fathom_tui::session`'s
/// `owns`), and a search key outside the namespace the shell is allowed to write would be a key nobody
/// writes back.
pub const SAVED_PREFIX: &str = "tui.search.saved.";

/// How many recent searches are kept. ADR-006 §5.8's `↑` cycles "the last few", and a history longer than
/// a pane is a log, not a history.
pub const HISTORY_MAX: usize = 10;

/// How many characters of a query a saved search keeps (the same bound `e1.11` puts on a remembered query).
pub const QUERY_MAX: usize = 120;


/// The recent searches, newest first, out of the session rows.
#[must_use]
pub fn recent(entries: &[(String, String)]) -> Vec<String> {
    let value = entries
        .iter()
        .rev()
        .find(|(key, _)| key == HISTORY_KEY)
        .map(|(_, value)| value.as_str())
        .unwrap_or_default();
    split_escaped(value)
        .into_iter()
        .map(|query| query.trim().to_owned())
        .filter(|query| !query.is_empty())
        .take(HISTORY_MAX)
        .collect()
}

/// Encode a recent-search list the way [`recent`] reads it back.
///
/// The symmetric half, which is the part a caller would otherwise get wrong: the history is one `|`-joined
/// value, so a query containing a `|` has to be escaped on the way in or it arrives as two searches on the
/// way out.
#[must_use]
pub fn history_value(queries: &[String]) -> String {
    let mut out: Vec<String> = Vec::new();
    for query in queries.iter().take(HISTORY_MAX) {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            continue;
        }
        let spelled = escape(trimmed);
        if !out.contains(&spelled) {
            out.push(spelled);
        }
    }
    out.join("|")
}

/// The session row that records `query` as the most recent search.
///
/// The caller supplies the session rows it already holds so the recency list is rebuilt, not appended to
/// blindly: the session writer takes a whole row set, and a row that grows one entry per keystroke is a
/// row that never shrinks.
#[must_use]
pub fn recent_entry(entries: &[(String, String)], query: &str) -> (String, String) {
    let query = query.trim();
    let mut list: Vec<String> = Vec::new();
    if !query.is_empty() {
        list.push(escape(query));
    }
    for seen in recent(entries) {
        if list.len() >= HISTORY_MAX {
            break;
        }
        let spelled = escape(&seen);
        if !list.contains(&spelled) {
            list.push(spelled);
        }
    }
    (HISTORY_KEY.to_owned(), list.join("|"))
}

/// The name a saved search carries, out of a session key.
#[must_use]
pub fn saved_name(key: &str) -> Option<&str> {
    key.strip_prefix(SAVED_PREFIX).filter(|name| !name.is_empty())
}

/// One saved search: the name the reviewer gave it and the query behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedSearch {
    /// The name, which is what the palette prints.
    pub name: String,
    /// The query, exactly as it was saved.
    pub query: String,
}

/// The saved searches, sorted by name so the palette order does not depend on SQLite's row order.
#[must_use]
pub fn saved(entries: &[(String, String)]) -> Vec<SavedSearch> {
    let mut out: Vec<SavedSearch> = entries
        .iter()
        .filter_map(|(key, value)| {
            let name = saved_name(key)?.to_owned();
            let query = value.trim();
            (!query.is_empty()).then(|| SavedSearch {
                name,
                query: query.to_owned(),
            })
        })
        .collect();
    out.sort_by(|left, right| left.name.cmp(&right.name));
    out
}

/// The session row that saves a search under a name.
#[must_use]
pub fn saved_entry(name: &str, query: &str) -> (String, String) {
    let name = name.trim().replace(' ', "-");
    (
        format!("{SAVED_PREFIX}{name}"),
        query.trim().chars().take(QUERY_MAX).collect(),
    )
}

/// `|` is the list separator and `\` the escape, so a query that contains either survives a round trip.
///
/// The clip happens before the escaping, so the bound is a bound on the query a reviewer typed and cannot
/// cut an escape sequence in half.
fn escape(query: &str) -> String {
    let clipped: String = query.chars().take(QUERY_MAX).collect();
    clipped.replace('\\', "\\\\").replace('|', "\\|")
}

/// Split an escaped list on its unescaped separators, then unescape each part.
///
/// Splitting first and unescaping after is the only order that works: `split('|')` over the escaped text
/// would cut `store\|core` in two and hand back two queries neither of which was ever typed.
#[must_use]
fn split_escaped(value: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                // An escape at the end of the row is a half-written entry: keep the character rather
                // than drop the row, because a lost query is worse than a stray backslash.
                current.push(chars.next().unwrap_or('\\'));
            }
            '|' => out.push(std::mem::take(&mut current)),
            other => current.push(other),
        }
    }
    out.push(current);
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn a_history_value_round_trips_through_the_reader_it_written_for() {
        // The shell writes with `history_value` and reads with `recent`. Those are one contract, and the
        // failure mode of breaking it is a search history that quietly shrinks to nothing.
        let queries = ["epoch store", "risk>60", "epoch"].map(String::from).to_vec();
        let value = history_value(&queries);
        assert_eq!(
            recent(&[(HISTORY_KEY.to_owned(), value)]),
            vec!["epoch store", "risk>60", "epoch"]
        );
    }

    #[test]
    fn a_recent_list_is_newest_first_and_holds_its_bound() {
        let mut entries: Vec<(String, String)> = Vec::new();
        for query in ["epoch", "store", "serde", "epoch"] {
            entries.push(recent_entry(&entries, query));
        }
        let list = recent(&entries);
        assert_eq!(
            list.first().map(String::as_str),
            Some("epoch"),
            "a repeat moves to the front rather than duplicating"
        );
        assert_eq!(list.iter().filter(|seen| *seen == "epoch").count(), 1);
        assert_eq!(list.len(), 3);
    }

    #[test]
    fn a_query_with_the_separator_in_it_comes_back_unchanged() {
        let entries = vec![recent_entry(&[], "risk>60 store|core")];
        assert_eq!(
            recent(&entries).first().map(String::as_str),
            Some("risk>60 store|core"),
            "the delimiter must not be able to split a query in two"
        );
    }

    #[test]
    fn a_saved_search_is_found_by_prefix_and_sorted_by_name() {
        let entries = vec![
            (format!("{SAVED_PREFIX}wide"), "risk>60".to_owned()),
            (format!("{SAVED_PREFIX}audit"), "kind:finding".to_owned()),
            ("tui.view".to_owned(), "timeline".to_owned()),
        ];
        let saved = saved(&entries);
        assert_eq!(
            saved
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            vec!["audit", "wide"]
        );
    }

    #[test]
    fn a_name_a_reviewer_typed_with_spaces_in_it_still_makes_a_key() {
        let (key, _) = saved_entry("since the refactor", "risk>60");
        assert_eq!(key, format!("{SAVED_PREFIX}since-the-refactor"));
        assert!(saved_name(&key).is_some());
    }
}
