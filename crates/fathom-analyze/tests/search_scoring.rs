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
/// The vocabulary the synthetic corpora are built from. Module-level because a query that asks about the
/// generated episodes has to spell their words the same way the generator does.
const WORDS: [&str; 12] = [
    "epoch", "store", "delta", "queue", "graph", "deps", "risk", "test", "lock", "render", "cache",
    "patch",
];

fn big_corpus(episodes: usize) -> Corpus {
    let mut corpus = Corpus::new();
    corpus.documents = episodes as i64;
    let words = WORDS;
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

/// The budget, measured: 50 ms p95 over the keystrokes in [`TYPED`] on a ten-thousand-episode corpus.
///
/// What sets the p95 is the four-word query `graph render cache patch`, and it is worth knowing why before
/// deciding that a failure is the runner's fault. None of those four words is a substring of anything in
/// that corpus, but every one of them is a *subsequence* of every episode body — "g…r…a…p…h" is in the
/// sentence the fixture repeats — so all 14 339 rows are hits, none of them can be refused by the letter
/// masks, and the keystroke is a substring walk plus a letter walk over every body in the corpus, four
/// times over: tens of megabytes of text read to answer one keystroke. That is the worst keystroke the box
/// can be asked for, which is the point of asserting a p95 rather than an average.
///
/// Two things about the number. It is a **debug** build, because that is what `cargo test` builds, and the
/// same two cores on the same corpus measure a 0.9 ms median and a 3.8 ms p95 with `--release` — so this
/// assertion is about eight times stricter than the shipped binary's real latency, which is what ADR-006
/// §5.8 signed up for by naming this test as the budget's measurement of record instead of leaving it to
/// the release-only perf gate and its runner calibration.
///
/// And it has history. Those same two cores measured a 19 ms median and a 57 ms p95 — over the budget,
/// which is the ticket this one answers — before [`score::Token`] stopped re-deriving a token's letter set
/// six times per row, before the field walk stopped reading the 230-byte body before the hundred bytes of
/// cheap fields, and before `core`'s `memchr` replaced the byte-at-a-time scans here. See the notes on
/// `find` and [`score::subsequence`] for what each of those was worth on its own.
///
/// A second runner-shape bug was found by the sibling budget below (`reco-p6zw`, on the mac leg): the two
/// timed tests in this file were timed *concurrently with each other*, which is worth a factor of two on a
/// four-core runner all by itself. Both are now timed one at a time ([`LATENCY_BUDGET`]), and both print
/// how many cores the runner actually offered. With that, this corpus measures a 4.0 ms median and a
/// 13.8 ms p95 on four cores, a 25.8 ms p95 on two, and a 49 ms p95 on one — the one-core number is within
/// 1 ms of the budget, and it is a genuinely one-core box being asked for four typo'd words that are each a
/// subsequence of every body in the corpus, which is the worst keystroke the scorer has. If this goes red
/// on a runner that prints `1 cores offered to this process`, the honest question is whether the runner is a
/// one-core box, because the budget was never signed up for that; on two cores and up there is room to
/// spare.
#[test]
fn typing_stays_inside_the_budget_on_a_ten_thousand_episode_corpus() {
    // Timed alone: see [`LATENCY_BUDGET`]. The two budget tests in this file are the two longest things in
    // it, and each of them wants every core the runner has.
    let _budget = latency_budget();
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

    let mut samples: Vec<(std::time::Duration, &'static str)> = Vec::new();
    for round in 0..5 {
        for text in TYPED {
            // Alternating the limit is part of the measurement: the palette's preview asks for five and
            // the pane asks for twenty, and a budget met only at the cheap one is not met.
            let limit = if round % 2 == 0 { 5 } else { DEFAULT_LIMIT };
            let started = Instant::now();
            let report = index.search(&Query::parse(text, now()), limit);
            let took = started.elapsed();
            samples.push((took, text));
            assert!(
                report.hits.len() <= limit,
                "`{text}` returned {} rows for a limit of {limit}",
                report.hits.len()
            );
        }
    }
    // Sorted by what each sample took, with the keystroke still attached to it. A budget that fails has to
    // say which keystroke it tripped on: the four-word query in `TYPED` costs several times what a
    // one-letter one does, so "p95 is over budget" with no name sends the next reader back here with a
    // stopwatch. (`reco-axp` is the precedent: a gate that cannot name what it blew is a ticket that opens
    // with "this could be a slower runner".)
    samples.sort_by_key(|(took, _)| *took);
    let (median, median_keystroke) = samples[samples.len() / 2];
    let (p95, p95_keystroke) = samples[(samples.len() * 95) / 100];
    let (worst, worst_keystroke) = samples[samples.len() - 1];
    let cores = runner_cores();
    eprintln!(
        "search over {rows} rows ({EPISODES} episodes): index built in {build:?}, median {median:?} on `{median_keystroke}`, \
         p95 {p95:?} on `{p95_keystroke}`, worst {worst:?} on `{worst_keystroke}` ({} cores offered to this process)",
        cores
    );
    assert!(
        p95 < BUDGET,
        "p95 {p95:?} is over the {BUDGET:?} budget ADR-006 §5.8 sets for `query.search.ms`; the keystroke at \
         the 95th percentile was `{p95_keystroke}`, the worst was {worst:?} on `{worst_keystroke}`, and the \
         runner offered {cores} cores to this process"
    );
    assert!(
        median < BUDGET,
        "the median keystroke must also be inside the budget, and was {median:?} on `{median_keystroke}`"
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

// ---------------------------------------------------------------------------
// the cost of the symbol section: `Index::build` and the resident set (`reco-6sw`)
// ---------------------------------------------------------------------------

/// A symbol source in the shape [`search::corpus`](fathom_analyze::search::corpus) produces, at the size the
/// ticket says to expect: one to two orders of magnitude above the episode count.
///
/// The names are deliberately mixed — some that collide with the episode vocabulary and some that do not —
/// because a corpus of 20 000 rows that none of the typed queries can reach would measure nothing about the
/// scan.
fn symbol_rows(count: usize) -> Vec<Entry> {
    let words = WORDS;
    (0..count)
        .map(|index| {
            let word = words[index % words.len()];
            let other = words[(index / words.len()) % words.len()];
            Entry::symbol(
                format!("{word}_{other}{index}"),
                "fn",
                format!("crates/fathom-{word}/src/{other}.rs"),
                index + 1,
                Some(Timestamp::from_unix_micros(
                    1_760_000_000_000_000 - (index as i64) * DAY,
                )),
            )
        })
        .collect()
}

/// The resident set, in bytes, or `None` where the platform has no `/proc/self/statm`.
///
/// Second field of `statm` is resident pages, and 4 KiB is the page size of every runner this budget is
/// asserted on; the number is a signal about orders of magnitude, and an unreadable one is reported as
/// unreadable rather than guessed at.
fn resident_bytes() -> Option<usize> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages: usize = statm.split_whitespace().nth(1)?.parse().ok()?;
    pages.checked_mul(4096)
}

/// The keystrokes a reviewer types once symbols are in the box: identifier fragments, a path fragment, and
/// the filter that narrows to the section.
const TYPED_WITH_SYMBOLS: &[&str] = &[
    "lockfil",
    "epoch_store20",
    "parss",
    "epcho",
    "src/store",
    "kind:symbol epoch",
    "kind:sym epoch",
    "delta cache patch",
];

/// The subject of the episode `big_corpus` built at `n`, spelled the way the generator spells it.
fn episode_subject(n: usize) -> String {
    format!(
        "{} {} when the rail reads half a frame",
        WORDS[n % WORDS.len()],
        WORDS[(n / 3) % WORDS.len()]
    )
}

/// The latency budgets in this file are timed one at a time, against a lock neither of them owns.
///
/// libtest runs the tests of a binary concurrently, one per core, and both of this file's budget tests are
/// the kind of work that wants every core it can get — the ten-thousand-episode corpus is a parallel scan
/// and the symbol corpus is a bigger one. Measured on a four-core runner (`-- --nocapture`, `reco-p6zw`):
/// `a_two_thousand_episode_corpus_with_its_symbols_builds_inside_the_budget` prints a 9.8 ms p95 when it
/// runs alone and a 20-22 ms p95 when the rest of this file runs alongside it. That is more than a factor
/// of two of inflation, bought from the harness and not from the code under test — and on a four-core CI
/// runner it is the difference between a passing leg and a red one.
///
/// A reviewer types one keystroke at a time on a box that is not also running three other searches, and
/// the release gate (`tools/perf_gate.py`) measures the same budget in a quiet process, so a number
/// inflated by this file's own thread churn is not the thing ADR-006 §5.8 signed up to guard. The lock
/// changes the measurement, not the budget: the number compared against 50 ms is still the debug build's
/// own, over the same corpus, with the same keystrokes, and a regression that makes a keystroke genuinely
/// slower still trips it.
static LATENCY_BUDGET: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Take the latency budget: hold this guard for the whole of a timed test.
///
/// Poison-tolerant on purpose: a budget test that panicked while holding the lock has already reported
/// itself, and making the *next* budget test fail with "mutex is poisoned" would bury the first failure
/// under an artifact of the harness.
fn latency_budget() -> std::sync::MutexGuard<'static, ()> {
    LATENCY_BUDGET
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// How many cores the runner offered this process, which the budget lines print.
///
/// A wall-clock budget whose failure message does not say what it was measured on is a ticket that opens
/// with "this could be a slower runner" — which is `reco-axp`'s rule, and the exact sentence this file has
/// had to write more than once. `available_parallelism` is the honest number: it is what the parallel scan
/// will actually get, cgroup limits included, rather than what the machine claims to have.
fn runner_cores() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
}

/// The symbol section at the size the product expects — 2 000 episodes and 20 000 declarations — and what
/// it costs to build and to search (`reco-6sw`, and then `reco-p6zw`).
///
/// The mac leg went red here with a p95 over 50 ms, and both of the ticket's hypotheses turned out to be
/// true, which is worth spelling out because the two fixes are different:
///
/// * **The scorer was paying for work it did not need.** Over this corpus the keystroke is dominated by the
///   twenty thousand symbol rows, whose names a typo'd token walks as a *subsequence* in nearly every row.
///   Measured one-instrument-at-a-time: the name rung's letter walk was about three quarters of a keystroke,
///   the `== needle` test in the exact-match rung (which reads four `String` heaps per row to answer "no")
///   a sixth of it, and the per-row re-derivations of the token's own shape a slice of the rest. The fixes
///   are in [`score`]: walk short fields in one pass with no calls ([`score::subsequence`]), carry the
///   token's facts on the token, compare word lengths before comparing words, keep a row's mask data in the
///   first cache line of the row, and stop allocating half a megabyte of per-keystroke scratch to hold the
///   rows that did not match. The budget went from a 20-26 ms p95 to a 6-7 ms one on four cores.
/// * **The measurement was inflated by the harness.** libtest runs this file's tests on one thread per
///   core, so the two budget tests were timing each other; see [`LATENCY_BUDGET`]. That alone was worth
///   better than a factor of two on the number below, and it is the whole reason a runner that is not
///   underpowered can still print a p95 over the budget.
///
/// What is left is the honest cost of the corpus: a 6-7 ms p95 on four cores, 11.6 ms on two, and 21.8 ms
/// on one. The worst keystroke is `src/store` — a path fragment no row contains, which every row's paths
/// and body are read to rule out — and the runner's own shape moves it by less than a factor of three
/// between one core and four, which is what makes the one-core number the useful floor to quote.
#[test]
fn a_two_thousand_episode_corpus_with_its_symbols_builds_inside_the_budget() {
    // Timed alone: see [`LATENCY_BUDGET`].
    let _budget = latency_budget();
    const EPISODES: usize = 2_000;
    // `fathom_analyze::search::SYMBOL_MAX`, spelled as a number here so that the test measures the size the
    // builder is allowed to reach rather than tracking a constant that could be lowered to make it pass.
    const SYMBOLS: usize = 20_000;
    // The corpus is read once per process, so this is a startup budget and not a keystroke one. The number
    // is the measured debug-profile cost (`tests/search_scoring.rs -- --nocapture`) with room for a loaded
    // CI runner; the release cost is several times smaller, and the keystroke budget below — the one with a
    // name attached to it — is asserted unchanged.
    const BUILD_BUDGET: std::time::Duration = std::time::Duration::from_millis(600);

    // A budget per row rather than a flat megabyte figure, because the profile this runs in changes the
    // allocator's habits and a flat number would then be measuring the profile and not the code. Measured
    // here, debug profile, 22 868 rows: the corpus and its index together cost about 3 KiB a row, of which
    // the index alone is about 1.8 KiB. Eight KiB a row is the tripwire for an order-of-magnitude mistake;
    // the number that decides whether the box can afford the section is printed for the release run.
    const ROW_BUDGET: usize = 8 * 1024;

    let before_corpus = resident_bytes();
    let mut corpus = big_corpus(EPISODES);
    let entries = corpus.entries.len();
    corpus.entries.extend(symbol_rows(SYMBOLS));
    corpus.documents = EPISODES as i64;
    let after_corpus = resident_bytes();

    let started = Instant::now();
    let index = corpus.index();
    let build = started.elapsed();
    let after_index = resident_bytes();

    let rows = index.len();
    assert_eq!(rows, entries + SYMBOLS);
    let grew = match (before_corpus, after_index) {
        (Some(start), Some(end)) if end > start => Some(end - start),
        _ => None,
    };
    eprintln!(
        "index::build over {rows} rows ({EPISODES} episodes + {SYMBOLS} symbols): {build:?}; \
         resident start {} · corpus {} · index {} · growth {}",
        before_corpus.map_or_else(
            || "?".to_owned(),
            |n| format!("{:.1} MiB", n as f64 / 1_048_576.0)
        ),
        after_corpus.map_or_else(
            || "?".to_owned(),
            |n| format!("{:.1} MiB", n as f64 / 1_048_576.0)
        ),
        after_index.map_or_else(
            || "?".to_owned(),
            |n| format!("{:.1} MiB", n as f64 / 1_048_576.0)
        ),
        grew.map_or_else(
            || "?".to_owned(),
            |n| format!("{:.1} MiB", n as f64 / 1_048_576.0)
        ),
    );
    assert!(
        build < BUILD_BUDGET,
        "`Index::build` took {build:?} over {rows} rows, over the {BUILD_BUDGET:?} the ticket budgets it \
         at — the answer is a per-kind cap in the builder and a visible 'narrowing' line, never a silent \
         truncation"
    );
    if let (Some(start), Some(end)) = (before_corpus, after_index) {
        let rows_for_budget = usize::max(rows, 1);
        let per_row = end.saturating_sub(start) / rows_for_budget;
        assert!(
            per_row < ROW_BUDGET,
            "the corpus and its index cost {per_row} bytes a row over {rows} rows, over the {ROW_BUDGET} \
             the ticket's cost question budgets them at; the section's size is bounded by \n \
             `search::SYMBOL_MAX`, and a bigger number here means the cap, not the test, is what needs to \
             change",
        );
    }

    // The question `reco-6sw` risked — that the second source would dominate the ranking — asked of a corpus
    // at the size the ticket says to expect. A symbol is one line of one file; an episode is the unit of work
    // the product is about, and here the rows are 100:1 in the declarations' favour and their names are
    // deliberately built from the episodes' own vocabulary. If one of these subjects now opens on a
    // declaration, the fix is a per-kind share in the builder and a stated narrowing, not a quieter test.
    for n in [1usize, 7, 400, 1_337] {
        let subject = episode_subject(n);
        let hits = index.search(&Query::parse(&subject, now()), 5).hits;
        assert!(
            hits.iter()
                .take(3)
                .any(|hit| { hit.kind == Kind::Episode && hit.label == subject }),
            "`{subject}` did not open on the episodes that own that subject in a corpus of {EPISODES} \
             episodes and {SYMBOLS} symbols; the first rows were {:?}",
            hits.iter()
                .map(|hit| (hit.kind, hit.label.clone()))
                .collect::<Vec<_>>()
        );
        assert!(
            hits.iter().take(3).all(|hit| hit.kind == Kind::Episode),
            "`{subject}` let a declaration into the first rows in a corpus of {SYMBOLS} symbols: {:?}",
            hits.iter()
                .map(|hit| (hit.kind, hit.label.clone()))
                .collect::<Vec<_>>()
        );
    }
    // And the section is reachable on its own terms, which is the other half of "not dominated": a reader who
    // wants declarations gets declarations, not a list of episodes that happen to share the words.
    let symbols = index
        .search(&Query::parse("kind:symbol epoch", now()), 5)
        .hits;
    assert_eq!(
        symbols
            .iter()
            .filter(|hit| hit.kind == Kind::Symbol)
            .count(),
        5,
        "`kind:symbol epoch` should be five declarations"
    );

    // The budget that actually has a number attached to it is the keystroke, and it is unchanged by the
    // section's size only because the scan is parallel and the letter-set test rejects most rows early.
    // What it costs, and which keystroke pays it, is the same question `reco-p6zw` had to ask of the mac
    // leg, so the samples keep the text they came from: `src/store` — a path fragment that no row contains
    // and every row's paths and body are therefore walked to prove it — is several times what `lockfil`
    // costs, and "p95 over budget" with no name attached is a ticket that starts with a stopwatch.
    for text in TYPED_WITH_SYMBOLS {
        let _ = index.search(&Query::parse(text, now()), DEFAULT_LIMIT);
    }
    let mut samples: Vec<(std::time::Duration, &'static str)> = Vec::new();
    for round in 0..3 {
        for text in TYPED_WITH_SYMBOLS {
            let limit = if round % 2 == 0 { 5 } else { DEFAULT_LIMIT };
            let started = Instant::now();
            let report = index.search(&Query::parse(text, now()), limit);
            let took = started.elapsed();
            samples.push((took, text));
            assert!(
                report.hits.len() <= limit,
                "`{text}` returned {} rows for a limit of {limit}",
                report.hits.len()
            );
        }
    }
    samples.sort_by_key(|(took, _)| *took);
    let (median, median_keystroke) = samples[samples.len() / 2];
    let (p95, p95_keystroke) = samples[(samples.len() * 95) / 100];
    let (worst, worst_keystroke) = samples[samples.len() - 1];
    let cores = runner_cores();
    eprintln!(
        "search over {rows} rows: median {median:?} on `{median_keystroke}`, p95 {p95:?} on \
         `{p95_keystroke}`, worst {worst:?} on `{worst_keystroke}` ({} cores offered to this process)",
        cores
    );
    assert!(
        p95 < std::time::Duration::from_millis(50),
        "with the symbol section in the corpus, p95 {p95:?} on `{p95_keystroke}` is over the 50 ms \
         `query.search.ms` budget; the worst keystroke was {worst:?} on `{worst_keystroke}`, the median \
         {median:?} on `{median_keystroke}`, and the runner offered {cores} cores to this process"
    );
}
