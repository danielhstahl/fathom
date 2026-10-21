//! The one scorer, pinned: a golden order, a total order, and a latency budget (`e5.2`, `reco-mkc`).
//!
//! Two of the ticket's four acceptance criteria are about the ranking rather than the plumbing, and both are
//! claims that a single hand-written assertion cannot carry:
//!
//! * **"Scoring is deterministic and stable across runs (golden test)"** — the golden file
//!   (`tests/golden/search-ranking.txt`) holds the whole ordering for a fixed corpus and a fixed query
//!   list: every hit's score, kind and id. A weight changed in `score.rs` moves a line here and the diff
//!   says which query it moved on, which is the only way a re-weighting can be reviewed as a decision
//!   rather than noticed as a support ticket.
//! * **"p95 result latency under 50 ms while typing on a 10k-episode corpus (bench)"** — measured here,
//!   on a corpus of the size the budget names, rather than asserted in a comment. It is a budget about the
//!   *whole* keystroke (filter, score, sort, truncate), so the test calls `Index::search` and times that.
//!
//! The third thing pinned here is the property the other two depend on: the order is a **total** order.
//! Every test that shuffles the corpus and demands the same list is a guard against the failure mode where
//! `HashMap` iteration, a stable-sort tie, or a timestamp read during scoring makes the same repository
//! answer differently twice — ADR-001 §7.10's "one reality" applied to ordering.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::time::Instant;

use fathom_analyze::search::query::Query;
use fathom_analyze::search::score::{self, W_NAME_EXACT, W_NAME_PREFIX, W_NAME_SUBSEQ};
use fathom_analyze::search::{
    CONTRACT, Corpus, DEFAULT_LIMIT, Entry, Kind, MAX_LIMIT, SNIPPET_CHARS,
};
use fathom_core::time::Timestamp;

/// Whether to rewrite the golden file instead of comparing against it — the same convention
/// `deps_golden.rs` uses, and the same rule: it is for reviewing a deliberate change, not for accepting one.
fn bless() -> bool {
    std::env::var("FATHOM_GOLDEN").is_ok_and(|value| value == "write")
}

fn golden_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("search-ranking.txt")
}

/// The instant every fixture query is parsed against: the relative date forms need an anchor, and a test
/// that read the clock would answer a different question on Monday than on Tuesday.
fn now() -> Timestamp {
    Timestamp::from_unix_micros(1_760_000_000_000_000)
}

/// A day, in the units the timestamps are in.
const DAY: i64 = 86_400_000_000;

/// The corpus. Every entry is the kind of row `search::corpus` builds from a real index — an episode with
/// its intent text, a path with its episode count, a dependency with its manifest, a finding with its
/// subject — and the names are the ones the queries below search for.
fn corpus() -> Corpus {
    let mut corpus = Corpus::new();
    corpus.documents = 14;
    let day = |ago: i64| {
        Some(Timestamp::from_unix_micros(
            1_760_000_000_000_000 - ago * DAY,
        ))
    };

    let mut epoch = Entry::episode(
        "e8f1a2c3d4",
        "epoch-safe store reads",
        "make the epoch check lock-free so the rail cannot read a half-written epoch while an index run \
         is committing",
        Some("Dana Stahl".to_owned()),
        day(1),
        Some(82),
    );
    epoch.paths = vec![
        "crates/fathom-store/src/epoch.rs".to_owned(),
        "crates/fathom-store/src/spine.rs".to_owned(),
    ];
    corpus.push(epoch);

    let mut delta = Entry::episode(
        "77aa0c99bb",
        "the delta model speaks for itself",
        "one versioned delta document for the TUI, the CLI and the export, with a gap that names its owner",
        Some("Priya Raman".to_owned()),
        day(3),
        Some(61),
    );
    delta.paths = vec!["crates/fathom-analyze/src/insight/delta.rs".to_owned()];
    corpus.push(delta);

    corpus.push(Entry::episode(
        "3c4d5e6f70",
        "ambient mode for the second terminal",
        "the glance pane: latest episode, queue depth, index state, one reason, at 80x12",
        Some("Dana Stahl".to_owned()),
        day(6),
        Some(35),
    ));
    corpus.push(Entry::episode(
        "99887766aa",
        "pin the store's epoch before the read",
        "the rail reads the epoch once; a second read inside the same frame is a torn epoch",
        Some("Ravi Patel".to_owned()),
        day(11),
        Some(44),
    ));
    corpus.push(Entry::episode(
        "5b5c5d5e5f",
        "fuzzy search across the history",
        "one box for episodes, paths, dependencies and findings, with a deterministic scorer",
        Some("Dana Stahl".to_owned()),
        day(2),
        Some(70),
    ));

    corpus.push(Entry::path(
        "crates/fathom-store/src/epoch.rs",
        Some("e8f1a2c3d4".to_owned()),
        "12 episodes · last yesterday",
        day(1),
    ));
    corpus.push(Entry::path(
        "crates/fathom-store/src/spine.rs",
        Some("99887766aa".to_owned()),
        "40 episodes · last 11d",
        day(11),
    ));
    corpus.push(Entry::path(
        "crates/fathom-analyze/src/search/mod.rs",
        Some("5b5c5d5e5f".to_owned()),
        "3 episodes · last 2d",
        day(2),
    ));
    corpus.push(Entry::path(
        "docs/adr/006-interface.md",
        Some("3c4d5e6f70".to_owned()),
        "2 episodes · last 6d",
        day(6),
    ));

    corpus.push(Entry::dependency(
        "serde",
        "cargo",
        Some("Cargo.toml".to_owned()),
        Some("77aa0c99bb".to_owned()),
    ));
    corpus.push(Entry::dependency(
        "ratatui",
        "cargo",
        Some("Cargo.toml".to_owned()),
        Some("3c4d5e6f70".to_owned()),
    ));

    corpus.push(Entry::finding(
        "new-cycle",
        "store→core",
        "a new import closes a cycle between the store and the core",
        "high",
        Some("e8f1a2c3d4".to_owned()),
    ));
    corpus.push(Entry::finding(
        "test-deleted-without-coverage",
        "tests/store.rs",
        "a test file was deleted and nothing new covers the behaviour",
        "high",
        Some("99887766aa".to_owned()),
    ));
    corpus.push(Entry::finding(
        "todo-debt",
        "src/store.rs",
        "a TODO was added with no issue attached",
        "note",
        Some("77aa0c99bb".to_owned()),
    ));
    corpus
}

/// The queries the golden file covers. Chosen to touch every rung of the scorer and every filter, so a
/// re-weighting shows up somewhere in here rather than in a user's result list.
const QUERIES: &[&str] = &[
    "epoch",
    "store",
    "store epoch",
    "fathstor",
    "serde",
    "kind:path epoch",
    "author:stahl risk>60",
    "risk>60",
    "since:7d epoch",
    "todo",
    "delta model",
    "layer:core",
    "",
];

/// The golden projection: one line per query, one indented line per hit.
fn projection() -> String {
    let index = corpus().index();
    let mut out = String::new();
    out.push_str(&format!("# {} · schema 1\n", CONTRACT));
    for text in QUERIES {
        let report = index.search(&Query::parse(text, now()), DEFAULT_LIMIT);
        out.push_str(&format!("query {text:?}\n"));
        out.push_str(&format!(
            "  {} of {} rows, {} candidates, {} scored\n",
            report.hits.len(),
            report.scanned,
            report.candidates,
            report.scored
        ));
        for note in report.notes.iter().chain(report.unknown.iter()) {
            out.push_str(&format!("  note {note}\n"));
        }
        for hit in &report.hits {
            out.push_str(&format!(
                "  {:>5} {:<10} {} :: {} :: {}\n",
                hit.score,
                hit.kind.as_str(),
                hit.id,
                hit.label,
                hit.why.join(" ")
            ));
        }
    }
    out
}

#[test]
fn the_ranking_is_the_golden_one() {
    let made = projection();
    let path = golden_path();
    if bless() {
        std::fs::write(&path, &made).expect("the golden file is writable");
        eprintln!("wrote {}", path.display());
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "no golden file at {}; write it with FATHOM_GOLDEN=write",
            path.display()
        )
    });
    assert_eq!(
        expected, made,
        "the ranking moved; review it and re-bless with FATHOM_GOLDEN=write"
    );
}

#[test]
fn the_same_corpus_and_query_answer_the_same_way_every_time() {
    let index = corpus().index();
    let first: Vec<(i64, String)> = index
        .search(&Query::parse("store", now()), 50)
        .hits
        .iter()
        .map(|hit| (hit.score, hit.id.clone()))
        .collect();
    for _ in 0..8 {
        let again: Vec<(i64, String)> = index
            .search(&Query::parse("store", now()), 50)
            .hits
            .iter()
            .map(|hit| (hit.score, hit.id.clone()))
            .collect();
        assert_eq!(first, again);
    }
}

#[test]
fn the_order_survives_a_shuffled_corpus() {
    // The tie-break has to be a total order over the row's own fields, or the row that was inserted first
    // wins a tie and the corpus's insertion order leaks into the answer.
    let index = corpus().index();
    let forward: Vec<(i64, Kind, String)> = index
        .search(&Query::parse("e", now()), 100)
        .hits
        .iter()
        .map(|hit| (hit.score, hit.kind, hit.id.clone()))
        .collect();
    let mut reversed = corpus();
    reversed.entries.reverse();
    let backward: Vec<(i64, Kind, String)> = reversed
        .index()
        .search(&Query::parse("e", now()), 100)
        .hits
        .iter()
        .map(|hit| (hit.score, hit.kind, hit.id.clone()))
        .collect();
    assert_eq!(forward, backward);
    let mut rotated = corpus();
    rotated.entries.rotate_left(5);
    let third: Vec<(i64, Kind, String)> = rotated
        .index()
        .search(&Query::parse("e", now()), 100)
        .hits
        .iter()
        .map(|hit| (hit.score, hit.kind, hit.id.clone()))
        .collect();
    assert_eq!(forward, third);
}

#[test]
fn the_weights_are_the_ones_the_module_documents() {
    let index = corpus().index();
    let scored = |token: &str| {
        index
            .search(&Query::parse(token, now()), 50)
            .hits
            .first()
            .map(|hit| hit.score)
    };
    assert_eq!(
        scored("serde"),
        Some(W_NAME_EXACT),
        "a dependency named exactly the query is the exact-name rung"
    );
    assert_eq!(
        scored("epoch-safe"),
        Some(W_NAME_PREFIX),
        "the query is the start of an episode's subject"
    );
    assert!(
        scored("fathstor").is_some_and(|score| score >= W_NAME_SUBSEQ),
        "the subsequence rung is what makes a fuzzy box a fuzzy box"
    );
    // A subsequence never outranks a substring anywhere in the corpus — that is the ordering the weights
    // are chosen for, and the reason a typo does not steal the top of the list from a real match.
    let fuzzy = index.search(&Query::parse("fathstor", now()), 50).hits;
    let exact = index.search(&Query::parse("serde", now()), 50).hits;
    assert!(
        exact[0].score > fuzzy[0].score,
        "{} vs {}",
        exact[0].score,
        fuzzy[0].score
    );
}

#[test]
fn every_rung_pays_what_it_promised() {
    // The documented table, rung by rung, on one corpus.
    let mut corpus = Corpus::new();
    corpus.push(Entry::episode(
        "one",
        "epoch is live",
        "the epoch check runs before every read of the store",
        None,
        None,
        None,
    ));
    let index = corpus.index();
    let score_of = |query: &str| {
        index
            .search(&Query::parse(query, now()), 10)
            .hits
            .first()
            .map(|hit| hit.score)
    };
    // Quoted, because an unquoted three-word query is three tokens and the sum of their best fields is
    // not the exact-label rung — which is the difference between a phrase search and a word search.
    assert_eq!(score_of("\"epoch is live\""), Some(W_NAME_EXACT));
    assert_eq!(score_of("epoch"), Some(W_NAME_PREFIX));
    assert_eq!(
        score_of("store"),
        Some(score::W_BODY_WORD),
        "`store` starts a word of the body"
    );
    assert_eq!(
        score_of("tore"),
        Some(score::W_BODY_SUB),
        "`tore` is inside a body word"
    );
    assert!(
        score_of("epch").is_some(),
        "the subsequence rung still answers"
    );
}

#[test]
fn filters_narrow_and_free_text_ranks() {
    let index = corpus().index();
    let report = index.search(&Query::parse("author:stahl risk>60", now()), 50);
    let authors: Vec<&str> = report.hits.iter().map(|hit| hit.id.as_str()).collect();
    assert!(authors.contains(&"e8f1a2c3d4"));
    assert!(
        !authors.contains(&"99887766aa"),
        "ravi is not stahl, and the risk filter alone would have let the row through"
    );
    assert!(!authors.contains(&"3c4d5e6f70"), "35 is not above 60");
    for hit in &report.hits {
        assert!(hit.risk.is_none_or(|risk| risk >= 60));
    }
}

#[test]
fn a_date_filter_counts_days_and_not_milliseconds() {
    let index = corpus().index();
    // The fixture's rows are 1, 2, 3, 6 and 11 days old, so `7d` is a cut with something on both sides of
    // it and the assertion is about the arithmetic rather than about a boundary nobody can see.
    let week = index.search(&Query::parse("since:7d", now()), 50);
    assert!(
        !week.hits.iter().any(|hit| hit.id == "99887766aa"),
        "eleven days ago is outside a week"
    );
    assert!(
        week.hits.iter().any(|hit| hit.id == "5b5c5d5e5f"),
        "two days ago is inside it"
    );
    let until = index.search(&Query::parse("until:7d", now()), 50);
    assert!(
        until.hits.iter().any(|hit| hit.id == "99887766aa"),
        "and `until:7d` is the other half, not a second copy of the first one"
    );
    assert!(
        !until.hits.iter().any(|hit| hit.id == "e8f1a2c3d4"),
        "yesterday is after the seventh day back"
    );
}

#[test]
fn a_filter_this_build_cannot_honour_is_reported_and_still_searched() {
    let index = corpus().index();
    let report = index.search(&Query::parse("layer:core", now()), 10);
    assert_eq!(report.unknown.len(), 1);
    assert!(report.unknown[0].contains("not a filter this build knows"));
    // The token was searched as text as well, which finds nothing here and says both facts.
    assert!(report.hits.is_empty() || report.hits.iter().any(|hit| hit.label.contains("core")));
}

#[test]
fn a_limit_that_would_dump_the_corpus_is_clamped_and_said() {
    let index = corpus().index();
    let clamped = index.search(&Query::parse("", now()), MAX_LIMIT * 10);
    assert!(
        clamped.hits.len() <= MAX_LIMIT,
        "the ceiling is the ceiling whatever the caller asks for"
    );
    let zero = index.search(&Query::parse("", now()), 0);
    assert!(
        zero.hits.len() <= DEFAULT_LIMIT,
        "`0` means the default, not an empty answer"
    );
}

#[test]
fn a_snippet_never_becomes_a_file_dump() {
    let mut corpus = Corpus::new();
    let long = "one ".repeat(500);
    corpus.push(Entry::episode(
        "long",
        "a very long intent",
        &long,
        None,
        None,
        None,
    ));
    let index = corpus.index();
    let report = index.search(&Query::parse("one", now()), 10);
    let hit = report.hits.first().expect("the row matched");
    assert!(
        hit.snippet.chars().count() <= SNIPPET_CHARS + 1,
        "{} characters of snippet is not a result line",
        hit.snippet.chars().count()
    );
    assert!(
        !hit.snippet.ends_with(' '),
        "a clipped snippet ends with the ellipsis, not with a space: {:?}",
        hit.snippet
    );
}

#[test]
fn every_hit_names_where_enter_goes() {
    let index = corpus().index();
    let report = index.search(&Query::parse("epoch", now()), 20);
    assert!(!report.hits.is_empty());
    for hit in &report.hits {
        use fathom_analyze::search::Destination;
        match hit.destination() {
            Destination::Timeline { episode } => assert!(
                !episode.is_empty(),
                "a hit that cannot say what to select has made the reviewer search twice: {}",
                hit.id
            ),
            Destination::Diff { episode, path } => {
                assert!(
                    episode.is_some(),
                    "a diff deep link with no episode has no file tree to open: {}",
                    hit.id
                );
                assert!(!path.is_empty());
            }
            Destination::Deps { name, .. } => assert!(!name.is_empty()),
            Destination::None { because } => panic!("a deep link gave up: {because}"),
        }
    }
}

#[test]
fn the_report_sums_itself_up_for_the_strip() {
    let index = corpus().index();
    let report = index.search(&Query::parse("epoch", now()), 20);
    let summary = report.summary();
    assert!(summary.contains("hit"), "{summary}");
    assert!(summary.contains("deterministic"), "{summary}");
    assert!(report.scanned > 0);
}

// ---------------------------------------------------------------------------
// the budget: 50 ms p95 on a 10 000-episode corpus, while typing
// ---------------------------------------------------------------------------

/// Build a corpus of `episodes` episodes plus their paths, dependencies and findings — the shape
/// `search::corpus` produces from a real index, at the size the ticket's budget names.
fn big_corpus(episodes: usize) -> Corpus {
    let mut corpus = Corpus::new();
    corpus.documents = episodes as i64;
    let words = [
        "epoch", "store", "delta", "queue", "graph", "deps", "risk", "test", "lock", "render",
        "cache", "patch",
    ];
    for index in 0..episodes {
        let word = words[index % words.len()];
        let other = words[(index / 3) % words.len()];
        let mut entry = Entry::episode(
            format!("{index:016x}"),
            format!("{word} {other} when the rail reads half a frame"),
            format!(
                "the {word} path was read while the {other} run was committing, so the frame disagreed \
                 with itself; make the read take the epoch once and pass it down instead of reaching for \
                 it again in the middle of drawing"
            ),
            Some(format!("Author {index}")),
            Some(Timestamp::from_unix_micros(
                1_760_000_000_000_000 - (index as i64) * DAY,
            )),
            Some((index % 100) as i64),
        );
        entry.paths = vec![
            format!("crates/fathom-{word}/src/{other}.rs"),
            format!("tests/{word}_{other}.rs"),
        ];
        entry.deps = vec![format!("{word}-crate")];
        corpus.push(entry);
        if index % 5 == 0 {
            corpus.push(Entry::path(
                format!("crates/fathom-{word}/src/{other}.rs"),
                Some(format!("{index:016x}")),
                format!("{} episodes", 1 + index % 40),
                Some(Timestamp::from_unix_micros(
                    1_760_000_000_000_000 - (index as i64) * DAY,
                )),
            ));
        }
        if index % 7 == 0 {
            corpus.push(Entry::dependency(
                format!("{word}-crate"),
                "cargo",
                Some("Cargo.toml".to_owned()),
                Some(format!("{index:016x}")),
            ));
        }
        if index % 11 == 0 {
            corpus.push(Entry::finding(
                "new-cycle",
                format!("{word}→{other}"),
                "a new import closes a cycle",
                "high",
                Some(format!("{index:016x}")),
            ));
        }
    }
    corpus
}

/// The keystrokes a reviewer types: the prefix of a word, then the word, then a filter after it.
const TYPED: &[&str] = &[
    "e",
    "ep",
    "epo",
    "epoc",
    "epoch",
    "epoch s",
    "epoch st",
    "epoch sto",
    "epoch store",
    "epoch store risk>60",
    "epoch store risk>60 author:a",
    "epoch store risk>60 author:a kind:episode",
    "rsk",
    "stte",
    "dep:serde ratatui",
    "kind:path crate",
    "graph render cache patch",
    "lock to render",
];

#[test]
fn typing_stays_inside_the_budget_on_a_ten_thousand_episode_corpus() {
    const EPISODES: usize = 10_000;
    const BUDGET: std::time::Duration = std::time::Duration::from_millis(50);

    let built = Instant::now();
    let corpus = big_corpus(EPISODES);
    let rows = corpus.entries.len();
    let index = corpus.index();
    let build = built.elapsed();

    // Warm once: the first query pays for page faults and cold caches, which is not what the budget is a
    // statement about — the budget is about the keystroke a reviewer is typing now.
    for text in TYPED {
        let _ = index.search(&Query::parse(text, now()), DEFAULT_LIMIT);
    }

    let mut samples: Vec<std::time::Duration> = Vec::new();
    for round in 0..5 {
        for text in TYPED {
            // Alternating the limit is part of the measurement: the palette's preview asks for five and
            // the pane asks for twenty, and a budget met only at the cheap one is not met.
            let limit = if round % 2 == 0 { 5 } else { DEFAULT_LIMIT };
            let started = Instant::now();
            let report = index.search(&Query::parse(text, now()), limit);
            let took = started.elapsed();
            samples.push(took);
            assert!(
                report.hits.len() <= limit,
                "`{text}` returned {} rows for a limit of {limit}",
                report.hits.len()
            );
        }
        let _ = round;
    }
    samples.sort();
    let p95 = samples[(samples.len() * 95) / 100];
    let median = samples[samples.len() / 2];
    let worst = samples[samples.len() - 1];
    eprintln!(
        "search over {rows} rows ({EPISODES} episodes): index built in {:?}, median {median:?}, \
         p95 {p95:?}, worst {worst:?}",
        build
    );
    assert!(
        p95 < BUDGET,
        "p95 {p95:?} is over the {BUDGET:?} budget ADR-006 §5.8 sets for `query.search.ms`"
    );
    assert!(
        median < BUDGET,
        "the median keystroke must also be inside the budget, and was {median:?}"
    );
}

#[test]
fn the_order_the_index_returns_is_the_order_the_scorer_states() {
    // `score::compare_hits` is the written law of the tie-break; the index is the thing that runs. This is
    // the seam between them: the index compiles the same order into an integer per row at build time, and a
    // rank that drifted from the stated order would be a ranking change hiding inside a performance change.
    let index = corpus().index();
    for text in QUERIES {
        let report = index.search(&Query::parse(text, now()), DEFAULT_LIMIT);
        let keys: Vec<(i64, fathom_analyze::search::Kind, &str, &str)> = report
            .hits
            .iter()
            .map(|hit| (hit.score, hit.kind, hit.label.as_str(), hit.id.as_str()))
            .collect();
        let mut stated = keys.clone();
        stated.sort_unstable_by(|left, right| score::compare_hits(*left, *right));
        assert_eq!(
            keys, stated,
            "the hits for `{text}` are not in the order the scorer states"
        );
    }
}

#[test]
fn a_numbered_label_sorts_like_a_list_and_not_like_a_dictionary() {
    // Three screenshots, one query, one score each: the whole ranking is the tie-break, which is where
    // `capture 12.png` either lands after `capture 2.png` (a list) or in front of it (a dictionary).
    let mut corpus = Corpus::new();
    for name in [
        "shots/capture 100.png",
        "shots/capture 2.png",
        "shots/capture 12.png",
    ] {
        corpus.push(Entry::path(name, None, "3 episodes", Some(now())));
    }
    corpus.documents = 3;
    let index = corpus.index();
    let report = index.search(&Query::parse("capture", now()), DEFAULT_LIMIT);
    let labels: Vec<&str> = report.hits.iter().map(|hit| hit.label.as_str()).collect();
    assert_eq!(
        labels,
        vec![
            "shots/capture 2.png",
            "shots/capture 12.png",
            "shots/capture 100.png"
        ],
        "a numbered label must sort by the number in it"
    );
}
