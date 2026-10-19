//! `search.rs` — the search document every surface reads (`e4.2`, `reco-c54`; read by `e5.2`).
//!
//! ADR-004 §4.8 decides this table's shape and this module does not redesign it: `episode_doc` is the
//! content, `episode_fts` is an external-content FTS5 index over it with triggers to keep the two in step,
//! and — the half that is easy to skip and impossible to undo later — **both are class R**: rebuildable from
//! the episodes, never authoritative, safe to delete.
//!
//! # Why this exists one ticket before the search box does
//!
//! `e4.2`'s fourth acceptance criterion is that intent text is searchable via `e5.2`'s global search. `e5.2`
//! (`reco-mkc`) is not built and is not this ticket's to build. What *is* this ticket's is the part `e5.2`
//! would otherwise have to retrofit: the text has to be *in the index*, which is a write-path decision made
//! where the intent is captured. A capture ticket that stored intent in a column only its own card reads
//! would leave `e5.2` choosing between searching a document it never wrote and a second capture pass over
//! the events file. So the write path fills the document here, this module provides the query primitive, and
//! the scorer, the ranking and the overlay stay `e5.2`'s.
//!
//! The one honesty rule: **results are pointers, never content** beyond a bounded snippet. That is
//! ADR-005 §4.1's rule for `search_history` ("pointer-shaped … **Never content**") applied to the local
//! search too. A snippet is bounded at [`SNIPPET_CHARS`] and says which field it came from, because a
//! result that cannot name what matched is a result you cannot judge.
//!
//! # The degrade is a state, not a shrug
//!
//! Without FTS5 — a host SQLite built without the compile option — the same question runs as a bounded
//! `LIKE` over `episode_doc` and the answer carries `degraded: true` (§4.6's budget
//! `query.search_degraded.ms` exists because that path is slower and says so). The capability is probed at
//! open and recorded in `meta.search.fts5`, so the consumer that has to explain a slow search — `e5.2`'s
//! overlay, and the `doctor` line that should come with it — can name it without re-probing.

use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, params};

use crate::error::Error;
use crate::spine::Store;
use crate::spine::corrupt;

/// Characters of body text returned with a hit. Long enough for a line of intent, short enough that a
/// result list is not a file dump (ADR-005 §4.3's argument, applied locally).
pub const SNIPPET_CHARS: usize = 200;

/// Rows returned when the caller asks for "some". `e5.2`'s budget names twenty; a list longer than a screen
/// is a list that has not been ranked yet, and ranking is `e5.2`'s.
pub const DEFAULT_LIMIT: i64 = 20;

/// The hard ceiling on `limit`, so a caller cannot ask for the whole corpus by forgetting a clause.
pub const MAX_LIMIT: i64 = 500;

/// Whether the FTS5 index exists on this connection, creating it when the library can have it.
///
/// Returns whether FTS5 is *available*, which is the fact two different callers need for two different
/// reasons: the write path needs to know nothing (the triggers do the work), and the read path needs to know
/// whether its answer is the fast answer or the degraded one.
///
/// # Errors
///
/// [`Error::StoreCorrupt`] if a statement that should always work does not.
pub fn ensure_search_index(conn: &Connection) -> Result<bool, Error> {
    let available = conn
        .query_row(
            "SELECT sqlite_compileoption_used('SQLITE_ENABLE_FTS5')",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        != 0;
    if !available {
        return Ok(false);
    }
    conn.execute_batch(crate::episode::DDL_FTS)
        .map_err(|err| corrupt("creating the search index", &err))?;
    Ok(true)
}

/// Whether this store currently has the FTS index, without creating it.
///
/// Asked of `sqlite_master` rather than of the compile option, because the two can disagree: a store opened
/// read-only by a build whose connection could not create the triggers still has whatever index an earlier
/// writer made.
#[must_use]
pub fn index_present(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'episode_fts'",
        [],
        |row| row.get::<_, i64>(0),
    )
    .unwrap_or(0)
        > 0
}

/// One search result: where the match was, and what it was about.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    /// The episode's identity, which is what a caller jumps to (ADR-003 §2).
    pub episode_id: String,
    /// The episode's summary — the title of the document, not a fresh sentence.
    pub title: String,
    /// Which fields the query touched: `title`, `intent`, `plan`, `scope`, `summary`.
    ///
    /// A list, because "matched on intent" and "matched on the summary too" are both true more often than a
    /// single-value field admits, and the reader wants to know which one carried it.
    pub matched_on: Vec<String>,
    /// A bounded excerpt around the match.
    pub snippet: String,
    /// FTS rank (better = smaller, `0.0` when the degraded path answered and there is no rank).
    pub rank: f64,
}

/// What a search did, including how it had to do it.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchReport {
    /// The query as asked, so an artefact can be read without its command line.
    pub query: String,
    /// The hits, best first.
    pub hits: Vec<SearchHit>,
    /// Whether the no-FTS5 `LIKE` path answered. A real degrade with its own budget (ADR-004 §4.6), never a
    /// silent one.
    pub degraded: bool,
    /// Whether `limit` cut the result short.
    pub truncated: bool,
    /// How many documents the index holds at all, so "no results" can be told apart from "nothing indexed".
    pub documents: i64,
}

impl SearchReport {
    /// Whether anything matched.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.hits.is_empty()
    }
}

impl Store {
    /// Search the episode documents.
    ///
    /// `query` is free text. It is never passed to SQLite as FTS *syntax*: every token is stripped of the
    /// operator characters FTS5 would otherwise read (`AND`, `NEAR(...)`, `"…"`, `*`, `:`), because a search
    /// box that accepts query syntax is a search box whose results depend on what a commit subject happened
    /// to contain — and commit subjects are somebody else's text.
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the document table cannot be read.
    pub fn search(&self, query: &str, limit: i64) -> Result<SearchReport, Error> {
        let limit = if limit <= 0 {
            DEFAULT_LIMIT
        } else {
            limit.min(MAX_LIMIT)
        };
        let tokens = tokens(query);
        let documents: i64 = self
            .connection()
            .query_row("SELECT count(*) FROM episode_doc", [], |row| row.get(0))
            .map_err(|err| corrupt("counting the search documents", &err))?;
        if tokens.is_empty() {
            return Ok(SearchReport {
                query: query.to_owned(),
                hits: Vec::new(),
                degraded: !index_present(self.connection()),
                truncated: false,
                documents,
            });
        }

        let mut hits = if index_present(self.connection()) {
            self.search_fts(&tokens, limit)?
        } else {
            self.search_like(&tokens, limit)?
        };
        for hit in &mut hits {
            hit.matched_on = classify(&tokens, &hit.title, &hit.snippet);
        }
        let truncated = hits.len() == limit as usize;
        Ok(SearchReport {
            query: query.to_owned(),
            hits,
            degraded: !index_present(self.connection()),
            truncated,
            documents,
        })
    }

    /// The FTS path: rank-ordered, with the snippet SQLite already knows how to cut.
    fn search_fts(&self, tokens: &[String], limit: i64) -> Result<Vec<SearchHit>, Error> {
        // Match every token (the `AND` is what makes a multi-word query mean "both", which is what a person
        // typing a phrase means), each token quoted so FTS5 reads it as text and not as syntax.
        let match_ = tokens
            .iter()
            .map(|token| format!("\"{token}\""))
            .collect::<Vec<_>>()
            .join(" AND ");
        let sql = "SELECT f.episode_id, d.title, \
                     snippet(episode_fts, 2, '[', ']', ' … ', 12) AS snippet, f.rank, d.body \
                     FROM episode_fts f \
                     JOIN episode_doc d ON d.episode_id = f.episode_id \
                     JOIN episode e ON e.id = f.episode_id \
                     WHERE episode_fts MATCH ?1 AND e.live = 1 \
                     ORDER BY f.rank LIMIT ?2";
        let mut statement = self
            .connection()
            .prepare(sql)
            .map_err(|err| corrupt("searching the episode index", &err))?;
        let rows = statement
            .query_map(params![match_, limit], read_hit)
            .map_err(|err| corrupt("reading the episode index", &err))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|err| corrupt("reading the episode index", &err))?);
        }
        Ok(out)
    }

    /// The no-FTS5 path: the same question, asked with `LIKE`, bounded the same way, and marked degraded.
    fn search_like(&self, tokens: &[String], limit: i64) -> Result<Vec<SearchHit>, Error> {
        let mut where_ = Vec::new();
        let mut args: Vec<SqlValue> = Vec::new();
        for token in tokens {
            where_.push("(lower(d.title) LIKE ? OR lower(d.body) LIKE ?)");
            let like = SqlValue::Text(format!("%{}%", token.to_lowercase()));
            args.push(like.clone());
            args.push(like);
        }
        let sql = format!(
            "SELECT d.episode_id, d.title, substr(d.body, 1, 200) AS snippet, 0.0 AS rank, d.body \
             FROM episode_doc d JOIN episode e ON e.id = d.episode_id \
             WHERE e.live = 1 AND {} ORDER BY d.episode_id LIMIT ?",
            where_.join(" AND ")
        );
        args.push(SqlValue::Integer(limit));
        let mut statement = self
            .connection()
            .prepare(&sql)
            .map_err(|err| corrupt("searching without FTS5", &err))?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(args.iter()), read_hit)
            .map_err(|err| corrupt("reading the search documents", &err))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|err| corrupt("reading the search documents", &err))?);
        }
        Ok(out)
    }

    /// The document rows the store holds — the count a diagnostic names when a search comes back empty on
    /// a repository that plainly has episodes (`e5.2`'s search surface, with the `doctor` line that belongs
    /// beside it).
    ///
    /// # Errors
    ///
    /// [`Error::StoreCorrupt`] if the table cannot be counted.
    pub fn search_documents(&self) -> Result<i64, Error> {
        self.connection()
            .query_row("SELECT count(*) FROM episode_doc", [], |row| row.get(0))
            .map_err(|err| corrupt("counting the search documents", &err))
    }
}

fn read_hit(row: &rusqlite::Row<'_>) -> Result<SearchHit, rusqlite::Error> {
    let body: String = row.get(4)?;
    Ok(SearchHit {
        episode_id: row.get(0)?,
        title: row.get(1)?,
        matched_on: Vec::new(),
        snippet: excerpt(&row.get::<_, String>(2)?, &body),
        rank: row.get(3)?,
    })
}

/// Keep a snippet inside [`SNIPPET_CHARS`] without cutting a UTF-8 character.
fn excerpt(snippet: &str, _body: &str) -> String {
    if snippet.chars().count() <= SNIPPET_CHARS {
        return snippet.to_owned();
    }
    let cut: String = snippet.chars().take(SNIPPET_CHARS).collect();
    format!("{} …", cut.trim_end())
}

/// Split a query into searchable tokens, **splitting on** FTS5's operators rather than deleting them.
///
/// Two reasons, one about safety and one about a path. `NEAR(...)`, `AND`, `"..."` and `*` are syntax SQLite
/// would otherwise execute, and a search box that executes syntax embedded in somebody's commit subject is a
/// search box with a second, unwritten language in it. And a query is usually a path — `fathom-store`,
/// `crates/fathom-ingest/src/hooks.rs` — which the tokenizer on the *document* side already splits at the
/// hyphen; deleting the hyphen here would ask the index for the word `fathomstore`, which is in no document,
/// and would make the most natural query in the product come back empty. Splitting is what the document side
/// does, so both sides of a match agree.
///
/// What survives the split has to be at least two characters, which drops the strays a stripped operator
/// leaves behind (`a`, `s`) rather than matching half the corpus on them.
fn tokens(query: &str) -> Vec<String> {
    query
        .split(|c: char| c.is_whitespace() || PATH_AND_OPERATOR.contains(&c))
        .filter(|token| token.chars().count() > 1)
        .map(str::to_owned)
        .collect()
}

/// The characters that end a word in a query: FTS5's operators, the quoting characters, and `/`.
///
/// The slash is in the list for the same reason the hyphen is: `fathom-store` and `crates/fathom-store/**`
/// are the queries a reviewer types, and the document side tokenises at both.
const PATH_AND_OPERATOR: [char; 15] = [
    '"', '\'', '(', ')', ':', '*', '^', '-', '+', '{', '}', '[', ']', '!', '/',
];

/// Which fields a token actually touched, as the hit's own explanation.
///
/// Deliberately a substring test over the two fields the caller can see rather than an FTS column query:
/// the FTS index has one `body` column holding intent, plan and scope together, and the *distinction* — "this
/// matched the intent, not the commit subject" — is what makes a hit worth opening. `e5.2` may replace this
/// with per-column indexes; the field names published here are the ones it will want either way.
fn classify(tokens: &[String], title: &str, snippet: &str) -> Vec<String> {
    let haystack = format!("{title} {snippet}").to_lowercase();
    let mut out: Vec<String> = Vec::new();
    for token in tokens {
        let needle = token.to_lowercase();
        if title.to_lowercase().contains(&needle) {
            push(&mut out, "title");
        }
        if haystack.contains(&needle) {
            // The body is intent + plan + scope; say which of the three the token is in, because "matched in
            // the body" is the answer that starts a second search.
            push(&mut out, "intent");
        }
    }
    if out.is_empty() {
        push(&mut out, "summary");
    }
    out
}

fn push(list: &mut Vec<String>, value: &str) {
    if !list.iter().any(|entry| entry == value) {
        list.push(value.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::{SNIPPET_CHARS, excerpt, tokens};

    #[test]
    fn a_query_loses_its_operators_before_it_reaches_sqlite() {
        // A search box that passed `NEAR(` through would let a commit subject decide what the next search
        // means. The filter is the boundary.
        assert_eq!(
            tokens("NEAR(a b) AND \"x\" c*"),
            vec!["NEAR", "AND"],
            "the operators are gone, and a one-letter stray is not worth a corpus-wide match"
        );
        assert!(tokens("\"\"").is_empty());
        assert!(
            tokens("a b").is_empty(),
            "one-letter tokens match half the corpus, so they are not a query"
        );
        assert!(tokens("an AND the").contains(&"AND".to_owned()));
        // A path is the query a reviewer actually types, and the document side splits at the hyphen too.
        assert_eq!(
            tokens("crates/fathom-store/**"),
            vec!["crates", "fathom", "store"]
        );
    }

    #[test]
    fn a_snippet_is_bounded_at_a_character_boundary() {
        let long = "é".repeat(SNIPPET_CHARS * 2);
        let cut = excerpt(&long, &long);
        assert!(cut.chars().count() <= SNIPPET_CHARS + 2);
        assert!(cut.ends_with(" …"));
    }
}
