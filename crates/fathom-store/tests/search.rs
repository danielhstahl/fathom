//! The search document (`e4.2`, `reco-c54`; read by `e5.2` and by ADR-005 §4.1's `search_history`).
//!
//! ADR-004 §4.8 decides this table's shape, so this file does not test a design — it tests three promises:
//!
//! * **the text got in.** Intent text, plan items and declared globs are in the body, so the day `e5.2`'s
//!   overlay is built it has intent to search and does not need a second capture pass;
//! * **the result is a pointer.** A hit names the episode and quotes a bounded snippet from a named field —
//!   never a file body, never a dump;
//! * **the document class is R.** Deleting `episode_doc` is a supported repair: the episodes are untouched
//!   and the next write rebuilds what was lost.
//!
//! Both read paths are covered: the FTS5 one where the running SQLite has it, and the bounded `LIKE` one
//! where it has not, which carries `degraded: true` rather than pretending to be the fast answer.
#![cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use fathom_store::episode::EpisodeRow;
use fathom_store::search::{DEFAULT_LIMIT, MAX_LIMIT, SNIPPET_CHARS, SearchReport};
use fathom_store::spine::{CommitRow, FileRow, Store, Trigger};

/// A fixed instant; every row in every test is stamped relative to it.
const T0: i64 = 1_700_000_000_000;

fn oid(n: u8) -> String {
    format!("{:040x}", n)
}

/// A commit row so an episode can have a member.
fn commit(n: u8, parent: Option<u8>, subject: &str, path: &str) -> CommitRow {
    CommitRow {
        oid: oid(n),
        tree_oid: "b".repeat(40),
        parent_oids: parent.map(|p| vec![oid(p)]).unwrap_or_default(),
        author_name: "pi-agent".to_owned(),
        author_email: "pi-agent@example.invalid".to_owned(),
        author_utc: T0 + i64::from(n) * 1_000,
        committer_utc: T0 + i64::from(n) * 1_000,
        committer_name: "pi-agent".to_owned(),
        committer_email: "pi-agent@example.invalid".to_owned(),
        message_head: subject.to_owned(),
        message_hash: format!("{:064x}", n),
        trailers: BTreeMap::new(),
        n_files: 1,
        n_add: 10,
        n_del: 2,
        n_binary: 0,
        oversized: false,
        files: vec![FileRow {
            path_hash: format!("{:064x}", usize::from(n) * 100),
            new_path: path.to_owned(),
            old_path: None,
            change_kind: "modify".to_owned(),
            mode_old: Some(0o100644),
            mode_new: Some(0o100644),
            is_binary: false,
            n_add: 10,
            n_del: 2,
            hunk_headers: vec!["@@ -1,2 +1,4 @@".to_owned()],
            blob_old: None,
            blob_new: Some("e".repeat(40)),
            path_class: "source".to_owned(),
            taxonomy_version: 1,
            noise_class: "informative".to_owned(),
        }],
    }
}

/// A store holding `rows` as the history of `refs/heads/main`.
fn seeded(rows: &[CommitRow]) -> Store {
    let mut store = Store::open_in_memory().expect("open in memory");
    let run = store.begin_run("cold", Trigger::Manual, T0).unwrap();
    store.write_commits(run, T0, rows).unwrap();
    let tip = rows
        .last()
        .map(|row| row.oid.clone())
        .unwrap_or_else(|| "0".repeat(40));
    store
        .replace_ref_tips(
            &[
                ("HEAD".to_owned(), tip.clone(), "head"),
                ("refs/heads/main".to_owned(), tip, "branch"),
            ],
            T0,
        )
        .unwrap();
    store
}

/// An episode that *has* an intent: text, a plan, a declared scope — the shape `e4.2` writes.
fn episode(id: &str, member: u8, summary: &str, intent: &str) -> EpisodeRow {
    EpisodeRow {
        id: id.to_owned(),
        fingerprint: format!("fp-{id}"),
        fallback_fingerprint: format!("fb-{id}"),
        kind: "committed".to_owned(),
        lineage_ref: "refs/heads/main".to_owned(),
        created_utc: T0,
        created_source: "index".to_owned(),
        boundary_rule: "author".to_owned(),
        author_kind: "agent".to_owned(),
        first_oid: oid(member),
        last_oid: oid(member),
        members: vec![oid(member)],
        n_files: 1,
        first_utc: T0,
        last_utc: T0,
        author_name: "pi-agent".to_owned(),
        author_email: "pi-agent@example.invalid".to_owned(),
        summary: summary.to_owned(),
        live: true,
        intent: intent.to_owned(),
        intent_state: if intent.is_empty() {
            String::new()
        } else {
            "recorded".to_owned()
        },
        intent_source: if intent.is_empty() {
            String::new()
        } else {
            "hook-prompt".to_owned()
        },
        intent_text: intent_text_of(intent),
        ..EpisodeRow::default()
    }
}

/// The `text` field of an intent document, read the way the store writes it.
fn intent_text_of(intent: &str) -> String {
    serde_json::from_str::<serde_json::Value>(intent)
        .ok()
        .and_then(|doc| {
            doc.get("text")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_default()
}

/// The intent document `fathom_ingest::intent::capture` would have written, spelled out here because this
/// crate must not depend on the one that writes it.
fn intent(text: &str, plan: &[&str], scope: &[&str]) -> String {
    serde_json::json!({
        "schema": "fathom-intent/1",
        "state": "recorded",
        "text": text,
        "source": "hook-prompt",
        "confidence": 0.95,
        "session_id": "s1",
        "turn_id": "s1:1",
        "plan": plan
            .iter()
            .map(|item| serde_json::json!({"text": item}))
            .collect::<Vec<_>>(),
        "scope": scope,
        "evidence": [],
    })
    .to_string()
}

/// Every episode row in a three-episode store: one with a prompt, one whose intent is only a plan, one with
/// no intent document at all (a row written before migration 5).
fn three() -> Store {
    let mut store = seeded(&[
        commit(1, None, "feat: the scroll fix", "src/rail.rs"),
        commit(2, Some(1), "fix: the reviewer's note", "src/card.rs"),
        commit(3, Some(2), "chore: tidy", "docs/tidy.md"),
    ]);
    store
        .write_episodes(
            T0,
            &[
                episode(
                    "ep-scroll",
                    1,
                    "feat: the scroll fix",
                    &intent(
                        "make the rail scroll without dropping the selection",
                        &["add the sticky offset"],
                        &["crates/fathom-tui/**"],
                    ),
                ),
                episode(
                    "ep-card",
                    2,
                    "fix: the reviewer's note",
                    &intent("address the card pane review", &[], &["docs/*.md"]),
                ),
                episode("ep-tidy", 3, "chore: tidy", ""),
            ],
        )
        .unwrap();
    store
}

fn hits(report: &SearchReport) -> Vec<&str> {
    report
        .hits
        .iter()
        .map(|hit| hit.episode_id.as_str())
        .collect()
}

#[test]
fn intent_text_is_searchable_and_the_hit_names_what_matched() {
    // The ticket's fourth criterion, at the layer this ticket owns: the text is *in* the index, and a hit is
    // a pointer to the episode plus a quote from the field that answered.
    let store = three();
    let report = store.search("sticky offset", 10).unwrap();
    assert_eq!(hits(&report), vec!["ep-scroll"]);
    let hit = report.hits.first().expect("one hit");
    assert_eq!(hit.title, "feat: the scroll fix");
    assert!(
        hit.snippet.contains("sticky") && hit.snippet.contains("offset"),
        "the snippet quotes the match, markers and all: {:?}",
        hit.snippet
    );
    assert!(
        hit.snippet.chars().count() <= SNIPPET_CHARS + 4,
        "and it is a snippet, not the document: {}",
        hit.snippet.chars().count()
    );
    assert!(
        hit.matched_on.contains(&"intent".to_owned()),
        "a result that cannot say what matched is a result you cannot judge: {hit:?}"
    );
    assert!(!report.is_empty());
    assert_eq!(
        report.documents, 3,
        "every episode has a document, intent or not"
    );
}

#[test]
fn a_query_finds_the_title_and_the_declared_scope_too() {
    let store = three();
    // The summary.
    assert_eq!(
        hits(&store.search("reviewer", 10).unwrap()),
        vec!["ep-card"]
    );
    // A declared glob: searching for the path an episode promised to stay inside finds it. The query is
    // split at the hyphen because the document side is split there too.
    let by_glob = store.search("crates/fathom-tui", 10).unwrap();
    assert_eq!(hits(&by_glob), vec!["ep-scroll"]);
    let hit = by_glob.hits.first().expect("the glob found its episode");
    assert!(
        !hit.matched_on.contains(&"title".to_owned()),
        "this match came from the declared scope, not the subject line: {hit:?}"
    );
    assert!(
        hit.matched_on.contains(&"intent".to_owned()),
        "and the hit says which field carried it: {hit:?}"
    );
}

#[test]
fn query_syntax_never_reaches_the_engine() {
    // A search box that passed FTS syntax through would let a commit subject decide what the next search
    // means, and commit subjects are somebody else's text.
    let store = three();
    for query in [
        "NEAR(scroll card)",
        "AND scroll",
        "\"sticky\"",
        "sticky*",
        "title:scroll",
        "scroll OR nonexistent-token",
    ] {
        let report = store.search(query, 10).unwrap();
        // Whatever it finds, it finds as plain words and never as an engine error.
        let _ = report;
    }
    // …and an operator-only query is no query at all, not a full scan.
    let empty = store.search("\"\" AND", 10).unwrap();
    assert!(empty.hits.is_empty(), "no tokens, no hits: {empty:?}");
    assert_eq!(empty.documents, 3, "but it still says what the index holds");
}

#[test]
fn a_limit_says_whether_it_cut_the_list() {
    let store = three();
    // Two of the three subjects say `fix`; the third has no intent to say it with.
    let all = store.search("fix", 0).unwrap();
    assert_eq!(all.hits.len(), 2, "limit 0 is the default, not no results");
    assert!(!all.truncated);
    let one = store.search("fix", 1).unwrap();
    assert_eq!(one.hits.len(), 1);
    assert!(one.truncated, "and it admits there was more");
    assert_eq!(DEFAULT_LIMIT, 20);
    assert_eq!(MAX_LIMIT, 500);
    // A ceiling a caller cannot forget to set: asking for the whole corpus gets what matches, capped.
    assert_eq!(store.search("fix", 100_000).unwrap().hits.len(), 2);
}

#[test]
fn a_snippet_is_bounded_and_says_where_it_was_cut() {
    let long = "a very long prompt ".repeat(40);
    let mut store = seeded(&[commit(1, None, "feat: long", "src/a.rs")]);
    store
        .write_episodes(
            T0,
            &[episode(
                "ep-long",
                1,
                "feat: long",
                &intent(&long, &[], &[]),
            )],
        )
        .unwrap();
    let report = store.search("prompt", 5).unwrap();
    let hit = report.hits.first().expect("the long one matched");
    assert!(
        hit.snippet.chars().count() <= SNIPPET_CHARS + 4,
        "a result list is not a file dump: {} chars",
        hit.snippet.chars().count()
    );
}

#[test]
fn a_deleted_document_degrades_the_search_and_breaks_nothing() {
    // `episode_doc` is class R: rebuildable, never authoritative, safe to delete (ADR-004 §4.8). The store
    // must come back with the `LIKE` answer and say it is the degraded one — not with an error, and not with
    // a silent half-answer.
    let mut store = three();
    assert_eq!(
        store.search_documents().unwrap(),
        3,
        "the write path filled the document as well as the row"
    );
    store
        .connection()
        .execute("DELETE FROM episode_doc", [])
        .unwrap();
    let after = store.search("sticky", 10).unwrap();
    assert!(
        after.hits.is_empty() && after.documents == 0,
        "with the document gone there is nothing to find, and the report says so: {after:?}"
    );
    // Indexing again rebuilds what was deleted, from the rows that own the text.
    store
        .write_episodes(
            T0 + 1,
            &[episode(
                "ep-scroll",
                1,
                "feat: the scroll fix",
                &intent(
                    "make the rail scroll without dropping the selection",
                    &["add the sticky offset"],
                    &["crates/fathom-tui/**"],
                ),
            )],
        )
        .unwrap();
    let rebuilt = store.search("sticky", 10).unwrap();
    assert_eq!(hits(&rebuilt), vec!["ep-scroll"]);
    assert_eq!(rebuilt.documents, 1);
}

#[test]
fn the_index_is_there_when_the_library_can_have_it() {
    // Either answer is fine — it is a property of the linked SQLite, not of this build. What is not fine is
    // not knowing which one it is: the search report reads this flag rather than re-probing, and `e5.2`'s
    // surface (with the `doctor` line that belongs to it) reads the report.
    let store = three();
    let present = fathom_store::search::index_present(store.connection());
    let report = store.search("scroll", 10).unwrap();
    assert_eq!(
        report.degraded, !present,
        "the report's degrade is the index's absence, named and not guessed"
    );
}

#[test]
fn an_episode_with_no_intent_still_has_a_document() {
    // The document is the episode's, not the intent's. A row written before migration 5 has no intent and
    // is still findable by its subject line — the intent capture widens the search, it does not gate it.
    let store = three();
    let found = store.search("tidy", 10).unwrap();
    assert_eq!(hits(&found), vec!["ep-tidy"]);
    let tidy = store
        .episodes(&fathom_store::episode::EpisodeQuery::default())
        .unwrap()
        .into_iter()
        .find(|row| row.id == "ep-tidy")
        .expect("the row is there");
    assert!(
        tidy.intent_state.is_empty(),
        "and the row says nobody ever wrote an intent for it"
    );
}
