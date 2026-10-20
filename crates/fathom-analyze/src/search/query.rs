//! `search::query` — the search box's grammar: free text plus filter tokens (`e5.2`, `reco-mkc`).
//!
//! One grammar, three consumers: `/` in the shell, `fathom search` in a pipe, and — when `e6.5` builds
//! it — ADR-005 §4.1's `search_history` tool. The tokens are the ones the ticket names, and they are the
//! ones ADR-006 §5.8's `query` pane documents in its strip:
//!
//! ```text
//! auth risk> since:2w store dep:serde author:stahl path:crates/fathom-store kind:episode
//! ```
//!
//! Three rules make this a parser rather than a `split_whitespace`:
//!
//! * **A filter narrows the corpus; free text ranks it.** `author:stahl risk>60 epoch` means "rank things
//!   matching `epoch` among stahl's high-risk episodes", never "rank things that mention the word
//!   `author:stahl`". Composing them is AND across dimensions — the same rule [`crate::search`] states for
//!   the rail's filter and the store's `EpisodeQuery`, so the same words mean the same thing in all three.
//! * **A token this build cannot honour is reported, never dropped.** An unknown `prefix:` is kept as free
//!   text *and* named in [`Query::unknown`] with the remedy, because a search that silently ignores
//!   `layer:core` tells the reviewer there is nothing there when the truth is that the filter does not
//!   exist yet. That difference is the whole reason this ticket is not `rg`.
//! * **Comparison tokens are spelled with their operator.** `risk>60` and `risk<20`, not `risk:60`,
//!   because a bare `risk:` has no direction and a filter whose meaning depends on a default is a filter
//!   nobody trusts. `risk:60` is accepted as `risk>=60` and said so in [`Query::notes`].
//!
//! Dates use the same vocabulary the filter editor uses — `2026-10-14`, `today`, `yesterday`, `7d`, `2w`,
//! `week`, `month` — so a reviewer does not learn two calendars in one tool. The arithmetic lives in
//! [`fathom_core::time`] for that reason.

use fathom_core::time::Timestamp;

/// One filter dimension's name as the reviewer types it.
pub const FILTER_PREFIXES: &[&str] = &["author:", "path:", "dep:", "kind:", "since:", "until:"];

/// The comparison filters, typed with their operator (`risk>60`, `deps<3`).
pub const COMPARISON_FILTERS: &[&str] = &["risk"];

/// What a filter binds to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Filter {
    /// An author fragment, case-insensitive, several allowed (OR inside the dimension).
    Author(String),
    /// A path fragment.
    Path(String),
    /// A dependency name fragment.
    Dep(String),
    /// A result kind: `episode`, `path`, `dependency`, `finding`, `symbol`.
    Kind(String),
    /// Inclusive lower bound on the entry's date.
    Since(Timestamp),
    /// Exclusive upper bound on the entry's date, stored as the first instant *after* the day named.
    Until(Timestamp),
    /// A risk floor, inclusive.
    RiskAtLeast(i64),
    /// A risk ceiling, inclusive.
    RiskAtMost(i64),
}

impl Filter {
    /// The dimension's name, for the strip and for [`crate::search::Report::applied`].
    #[must_use]
    pub fn dimension(&self) -> &'static str {
        match self {
            Filter::Author(_) => "author",
            Filter::Path(_) => "path",
            Filter::Dep(_) => "dep",
            Filter::Kind(_) => "kind",
            Filter::Since(_) => "since",
            Filter::Until(_) => "until",
            Filter::RiskAtLeast(_) => "risk>",
            Filter::RiskAtMost(_) => "risk<",
        }
    }

    /// The filter spelled back the way the reviewer typed it, which is what the help line and the artefact
    /// quote. Timestamps are re-rendered as the day they name, not as micros — and `until:` names the day
    /// *through* which the search goes, while it is *stored* as the first instant after it, so the two
    /// differ by exactly the day the reviewer did not mean to exclude.
    #[must_use]
    pub fn display(&self) -> String {
        match self {
            Filter::Author(value) => format!("author:{value}"),
            Filter::Path(value) => format!("path:{value}"),
            Filter::Dep(value) => format!("dep:{value}"),
            Filter::Kind(value) => format!("kind:{value}"),
            Filter::Since(at) => format!("since:{}", iso_day(*at)),
            Filter::Until(at) => format!("until:{}", iso_day(at.day_start().shift_days(-1))),
            Filter::RiskAtLeast(value) => format!("risk>={value}"),
            Filter::RiskAtMost(value) => format!("risk<={value}"),
        }
    }
}

/// A parsed query: the free-text tokens that rank, the filters that narrow, and what could not be honoured.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Query {
    /// The free-text tokens, lower-cased, in the order typed. Empty is a browse, not an error.
    pub tokens: Vec<String>,
    /// The filters that parsed.
    pub filters: Vec<Filter>,
    /// Tokens that looked like a filter and were not honoured, with the sentence that says why.
    ///
    /// They are *also* left in [`Self::tokens`] as ordinary text: dropping a token the reviewer typed would
    /// make the answer depend on a parser they cannot see.
    pub unknown: Vec<String>,
    /// Non-error remarks worth showing next to the results, such as the `risk:` → `risk>=` reading.
    pub notes: Vec<String>,
    /// The query as typed, so a report can be read without its command line.
    pub raw: String,
}

impl Query {
    /// Parse a search box's contents.
    ///
    /// `now` anchors the relative date forms; the caller holds the clock (ADR-002 §4.4).
    #[must_use]
    pub fn parse(input: &str, now: Timestamp) -> Query {
        let mut query = Query {
            raw: input.to_owned(),
            ..Default::default()
        };
        for word in split_terms(input) {
            query.push_word(&word, now);
        }
        query
    }

    /// Is there anything to search for at all?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty() && self.filters.is_empty()
    }

    /// The dimensions in play, for `Report::applied`.
    #[must_use]
    pub fn applied(&self) -> Vec<String> {
        let mut out: Vec<String> = self.filters.iter().map(Filter::display).collect();
        out.extend(self.unknown.iter().cloned());
        out
    }

    fn push_word(&mut self, word: &str, now: Timestamp) {
        let Some((prefix, value)) = word.split_once([':', '>', '<']) else {
            self.push_token(word);
            return;
        };
        let prefix = prefix.to_ascii_lowercase();
        let value = value.trim();
        // The dimension is named *with* its operator in [`FILTER_PREFIXES`] (`author:`) and *without* it in
        // [`COMPARISON_FILTERS`] (`risk`, because `risk` is spelled with `>` or `<`), so the two lists are
        // consulted in their own spellings rather than one of them being written twice.
        let typed = format!("{prefix}:");
        let comparison = COMPARISON_FILTERS.contains(&prefix.as_str());
        let known = FILTER_PREFIXES.contains(&typed.as_str()) || comparison;
        if !known || value.is_empty() {
            // Not a filter this build knows. Keep it as text and say so once, with the vocabulary.
            if known && value.is_empty() {
                self.unknown
                    .push(format!("`{word}` names a filter with no value — dropped as text"));
            } else if word.contains(':') || word.contains('>') || word.contains('<') {
                self.unknown.push(format!(
                    "`{word}` is not a filter this build knows — searched as text. Filters: {} \
                     and risk>/risk<.",
                    FILTER_PREFIXES.join(" ")
                ));
                self.push_token(word);
            } else {
                self.push_token(word);
            }
            return;
        }
        match prefix.as_str() {
            "author" => self.filters.push(Filter::Author(value.to_lowercase())),
            "path" => self.filters.push(Filter::Path(value.to_lowercase())),
            "dep" => self.filters.push(Filter::Dep(value.to_lowercase())),
            "kind" => self.filters.push(Filter::Kind(value.to_lowercase())),
            "since" => match parse_when(value, now) {
                Ok(Some(at)) => self.filters.push(Filter::Since(at)),
                Ok(None) => self
                    .notes
                    .push(format!("`since:{value}` means any date, so it filtered nothing")),
                Err(why) => self.unknown.push(why),
            },
            "until" => match parse_when(value, now) {
                Ok(Some(at)) => {
                    // A reviewer who types a day means *through* that day, so the stored bound is the next
                    // one. The display spells the day they named.
                    self.filters
                        .push(Filter::Until(at.day_start().shift_days(1)));
                }
                Ok(None) => self
                    .notes
                    .push(format!("`until:{value}` means any date, so it filtered nothing")),
                Err(why) => self.unknown.push(why),
            },
            "risk" => {
                let Ok(value) = value.trim_end_matches('=').parse::<i64>() else {
                    self.unknown.push(format!(
                        "`{word}` is not a risk threshold — `risk>` takes a whole number 0..=100"
                    ));
                    return;
                };
                // `risk:` has no direction; reading it as a floor is the only reading that is not a
                // surprise, and the note says the substitution happened.
                if word.contains(':') {
                    self.notes
                        .push(format!("`{word}` was read as `risk>={value}`"));
                }
                if word.contains('<') {
                    self.filters.push(Filter::RiskAtMost(value));
                } else {
                    self.filters.push(Filter::RiskAtLeast(value));
                }
            }
            _ => {}
        }
    }

    fn push_token(&mut self, word: &str) {
        let token = word.trim().to_lowercase();
        if !token.is_empty() {
            self.tokens.push(token);
        }
    }
}

/// Split a query into words, honouring `"a phrase"` as one word.
///
/// Quotes are removed, not escaped: the scorer matches letters, and a quote left in the token would ask it
/// to find a character no document contains.
fn split_terms(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for ch in input.chars() {
        match ch {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Parse a date the reviewer can type: `2026-10-14`, `today`, `yesterday`, `week`, `2w`, `7d`, `month`.
///
/// The same vocabulary as the `F` filter editor, deliberately: two date grammars in one tool is a
/// bug report waiting to be written. `Ok(None)` is `any`/`all`/empty — not a failure.
///
/// # Errors
///
/// A one-line sentence naming what was not a date, for the strip to print back.
pub fn parse_when(spec: &str, now: Timestamp) -> Result<Option<Timestamp>, String> {
    let text = spec.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let lowered = text.to_ascii_lowercase();
    let day = now.day_start();
    let shifted = |days: i64| Some(day.shift_days(days));
    match lowered.as_str() {
        "any" | "all" | "none" | "*" => return Ok(None),
        "today" => return Ok(Some(day)),
        "yesterday" => return Ok(shifted(-1)),
        "week" | "1w" => return Ok(shifted(-7)),
        "2w" => return Ok(shifted(-14)),
        "month" | "1mo" => return Ok(shifted(-30)),
        "year" | "1y" => return Ok(shifted(-365)),
        _ => {}
    }
    if let Some(count) = lowered.strip_suffix('d')
        && let Ok(count) = count.parse::<i64>()
    {
        return Ok(shifted(-count.max(0)));
    }
    if let Some(count) = lowered.strip_suffix('w')
        && let Ok(count) = count.parse::<i64>()
    {
        return Ok(shifted(-count.max(0) * 7));
    }
    let parts: Vec<&str> = lowered.split('-').collect();
    if parts.len() == 3
        && let (Ok(year), Ok(month), Ok(domen)) = (
            parts[0].parse::<i64>(),
            parts[1].parse::<u32>(),
            parts[2].parse::<u32>(),
        )
        && (1..=12).contains(&month)
        && (1..=31).contains(&domen)
    {
        let days = fathom_core::time::days_from_civil(year, month, domen);
        return Ok(Some(Timestamp::from_unix_micros(days * MICROS_PER_DAY)));
    }
    Err(format!(
        "`{text}` is not a date — try 2026-10-14, today, yesterday or 7d"
    ))
}

/// Microseconds in a day, for the test fixture's own instant.
const MICROS_PER_DAY: i64 = fathom_core::time::MICROS_PER_DAY;

/// `2026-10-14` from a timestamp, for echoing a date filter back.
fn iso_day(at: Timestamp) -> String {
    let (year, month, day) = at.civil();
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn now() -> Timestamp {
        // 2026-10-14T12:00:00Z, a Wednesday, so the relative forms land on days that are easy to name.
        Timestamp::from_unix_micros(fathom_core::time::days_from_civil(2026, 10, 14)
            * MICROS_PER_DAY
            + 12 * 3_600_000_000)
    }

    #[test]
    fn a_filter_narrows_and_free_text_ranks() {
        let query = Query::parse("epoch risk>60 author:stahl", now());
        assert_eq!(query.tokens, vec!["epoch"]);
        assert_eq!(query.filters.len(), 2);
        assert_eq!(query.filters[0], Filter::RiskAtLeast(60));
        assert_eq!(query.filters[1], Filter::Author("stahl".to_owned()));
        assert!(query.unknown.is_empty());
    }

    #[test]
    fn a_phrase_stays_one_token() {
        let query = Query::parse("\"epoch safe\" store", now());
        assert_eq!(query.tokens, vec!["epoch safe", "store"]);
    }

    #[test]
    fn an_unknown_filter_is_said_and_still_searched() {
        // The failure this prevents: `layer:core` comes back empty and the reviewer believes the
        // repository, when the truth is that the filter does not exist.
        let query = Query::parse("layer:core", now());
        assert_eq!(query.tokens, vec!["layer:core"], "the token is not dropped");
        assert_eq!(query.unknown.len(), 1);
        assert!(query.unknown[0].contains("not a filter this build knows"));
    }

    #[test]
    fn a_bare_risk_is_read_as_a_floor_and_says_so() {
        let query = Query::parse("risk:60", now());
        assert_eq!(query.filters, vec![Filter::RiskAtLeast(60)]);
        assert_eq!(query.notes.len(), 1, "the substitution is reported");
        let ceiling = Query::parse("risk<20", now());
        assert_eq!(ceiling.filters, vec![Filter::RiskAtMost(20)]);
    }

    #[test]
    fn dates_share_the_filter_editors_vocabulary() {
        let day = |spec: &str| match parse_when(spec, now()).unwrap() {
            Some(at) => iso_day(at),
            None => "any".to_owned(),
        };
        assert_eq!(day("2026-10-01"), "2026-10-01");
        assert_eq!(day("today"), "2026-10-14");
        assert_eq!(day("yesterday"), "2026-10-13");
        assert_eq!(day("7d"), "2026-10-07");
        assert_eq!(day("week"), "2026-10-07");
        assert_eq!(day("any"), "any");
        assert!(parse_when("last tuesday", now()).is_err());
    }

    #[test]
    fn until_means_through_the_day_named() {
        let query = Query::parse("until:2026-10-14", now());
        let Filter::Until(bound) = &query.filters[0] else {
            panic!("until parsed as something else");
        };
        // The bound is the next midnight, and it is *shown* as the day the reviewer typed.
        assert_eq!(iso_day(*bound), "2026-10-15");
        assert_eq!(query.filters[0].display(), "until:2026-10-14");
    }
}
