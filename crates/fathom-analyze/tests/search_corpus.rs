//! The corpus's symbol section, and the identity that says where a corpus came from (`reco-6sw`).
//!
//! `search_scoring.rs` pins the scorer over rows it is handed; this file pins the *assembly* of those rows:
//! that a symbol source fills a section and retires its gap, that an absent one is stated rather than empty,
//! that the caps say which way they cut, and that a corpus can answer "are you still the tree you were read
//! from?" — the three acceptance criteria that are about honesty rather than ordering.
//!
//! It needs a SQLite backend for `corpus::from_store`, and takes an empty in-memory store for almost all of
//! it: the claims under test are about what the builder was handed, not about what the spine holds.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_analyze::search::corpus::{self, Source, Symbol};
use fathom_analyze::search::query::Query;
use fathom_analyze::search::{
    Corpus, Destination, Entry, Identity, Kind, SOURCE_INDEX, SOURCE_SYMBOLS, SYMBOL_MAX,
};
use fathom_core::time::Timestamp;
use fathom_store::spine::Store;

fn now() -> Timestamp {
    Timestamp::from_unix_micros(1_760_000_000_000_000)
}

/// A declaration in the shape the working-tree scan produces.
fn symbol(name: &str, path: &str, line: usize, exported: bool) -> Symbol {
    Symbol {
        name: name.to_owned(),
        kind: "fn".to_owned(),
        path: path.to_owned(),
        line,
        language: "rust".to_owned(),
        exported,
        updated: Some(Timestamp::from_unix_micros(
            1_760_000_000_000_000 - 86_400_000_000,
        )),
    }
}

fn store() -> Store {
    Store::open_in_memory().expect("an in-memory store")
}

/// The tree the corpus was read from, with the stamp a caller would compute for it.
fn source(digest: &str) -> Source {
    Source {
        from: "the working tree `pilot` · 3 files · 4242 bytes".to_owned(),
        digest: digest.to_owned(),
    }
}

fn corpus_with(symbols: Vec<Symbol>, from: Option<Source>, note: Option<String>) -> Corpus {
    let lend = corpus::Lend {
        symbols,
        symbols_from: from,
        symbol_note: note,
        // No dependencies and no findings: this file is about the symbol section, and the other two would
        // only add gaps that are true for the wrong reason.
        dependency_note: Some("nothing to lend".to_owned()),
        finding_note: Some("nothing to lend".to_owned()),
        ..corpus::Lend::default()
    };
    corpus::from_store(&store(), &lend).expect("an empty store is a readable store")
}

// ---------------------------------------------------------------------------
// the section, and the gap that retires when it is filled
// ---------------------------------------------------------------------------

#[test]
fn a_symbol_source_fills_the_section_and_the_gap_is_gone() {
    let corpus = corpus_with(
        vec![
            symbol("parse_lockfile", "src/parse.rs", 1, true),
            symbol("helper", "src/store.rs", 8, false),
        ],
        Some(source("aabbccddeeff")),
        None,
    );
    let symbols: Vec<&Entry> = corpus
        .entries
        .iter()
        .filter(|entry| entry.kind == Kind::Symbol)
        .collect();
    assert_eq!(symbols.len(), 2, "both declarations are rows: {symbols:?}");
    assert!(
        !corpus.gaps.iter().any(|gap| gap.kind == Kind::Symbol),
        "a filled section carries no gap — that is the whole point of the wiring: {:?}",
        corpus.gaps
    );
    let index = corpus.index();
    assert_eq!(index.count(Kind::Symbol), 2);
    assert_eq!(
        index.count(Kind::Episode),
        0,
        "and it counts what it holds per kind, so an empty section is visible as one"
    );
}

#[test]
fn an_absent_symbol_source_is_a_stated_gap_and_the_caller_s_note_is_printed() {
    let plain = corpus_with(Vec::new(), Some(source("aabbccddeeff")), None);
    let gap = plain
        .gaps
        .iter()
        .find(|gap| gap.kind == Kind::Symbol)
        .expect("an empty symbol section is a gap, not an absence");
    assert_eq!(gap.state, "not indexed");
    assert!(
        gap.because.contains("no symbol source"),
        "the default sentence names the source that was missing: {}",
        gap.because
    );

    // The caller's own sentence wins when it has one, because the caller is the party that looked.
    let told = corpus_with(
        Vec::new(),
        Some(source("aabbccddeeff")),
        Some("this binary links no grammar".to_owned()),
    );
    let gap = told
        .gaps
        .iter()
        .find(|gap| gap.kind == Kind::Symbol)
        .expect("still a gap");
    assert_eq!(
        gap.because, "this binary links no grammar",
        "the reason the scan gave is the reason the reader gets"
    );
}

#[test]
fn the_cap_keeps_exported_names_first_and_says_which_it_left_out() {
    let mut symbols: Vec<Symbol> = (0..SYMBOL_MAX)
        .map(|index| symbol(&format!("public_{index:06}"), "src/lib.rs", index + 1, true))
        .collect();
    symbols.extend((0..SYMBOL_MAX).map(|index| {
        symbol(
            &format!("private_{index:06}"),
            "src/impl.rs",
            index + 1,
            false,
        )
    }));
    let corpus = corpus_with(symbols, Some(source("aabbccddeeff")), None);
    let kept: Vec<&Entry> = corpus
        .entries
        .iter()
        .filter(|entry| entry.kind == Kind::Symbol)
        .collect();
    assert_eq!(kept.len(), SYMBOL_MAX, "the cap is the cap");
    assert!(
        kept.iter().all(|entry| entry.name.starts_with("public_")),
        "the names a caller can use from outside their file are the ones that survive a narrowing"
    );
    let gap = corpus
        .gaps
        .iter()
        .find(|gap| gap.kind == Kind::Symbol && gap.state == "partial")
        .expect("a narrowing is stated, not silent");
    assert!(
        gap.because.contains(&format!("{SYMBOL_MAX} symbol names"))
            && gap.because.contains("left 20000 out"),
        "and it says how many went: {}",
        gap.because
    );
}

#[test]
fn the_same_declaration_read_twice_is_one_row() {
    // Two paths onto one blob (a vendored copy, a workspace that links a crate in) produce the same
    // declaration twice; two rows would be two hits a reviewer cannot tell apart.
    let corpus = corpus_with(
        vec![
            symbol("parse", "a/parse.rs", 1, true),
            symbol("parse", "a/parse.rs", 1, true),
            symbol("parse", "b/parse.rs", 1, true),
        ],
        Some(source("aabbccddeeff")),
        None,
    );
    assert_eq!(
        corpus
            .entries
            .iter()
            .filter(|entry| entry.kind == Kind::Symbol)
            .count(),
        2
    );
}

// ---------------------------------------------------------------------------
// the identity: which store, which tree
// ---------------------------------------------------------------------------

#[test]
fn the_identity_names_the_store_and_the_tree() {
    let corpus = corpus_with(
        vec![symbol("parse_lockfile", "src/parse.rs", 1, true)],
        Some(source("aabbccddeeff")),
        None,
    );
    assert!(
        corpus.identity.get("index").is_some(),
        "the store's epoch is one part: {:?}",
        corpus.identity.parts().collect::<Vec<_>>()
    );
    assert_eq!(
        corpus.identity.get("symbols"),
        Some("aabbccddeeff · the working tree `pilot` · 3 files · 4242 bytes"),
        "the tree is the other part, and it carries the caller's own sentence about itself"
    );
    assert_eq!(
        corpus.epoch,
        Some(1),
        "a fresh store is at epoch one, and the row's epoch and the identity's are the same number"
    );
    assert_eq!(corpus.identity.get("index"), Some("epoch=1"));
}

#[test]
fn a_moved_tree_makes_the_index_not_current() {
    // The acceptance criterion, stated as a question the box can be asked: the corpus identity changes when
    // the tree the symbols came from changes, so `is_current` says the symbols are stale rather than the box
    // answering from them.
    let index = corpus_with(
        vec![symbol("parse_lockfile", "src/parse.rs", 1, true)],
        Some(source("aabbccddeeff")),
        None,
    )
    .index();

    let mut same = Identity::new();
    same.set(SOURCE_SYMBOLS, source("aabbccddeeff").stamp());
    assert!(index.is_current(&same), "the same tree is the same corpus");

    let mut moved = Identity::new();
    moved.set(SOURCE_SYMBOLS, source("ffeeeeeedddd").stamp());
    assert!(!index.is_current(&moved));
    let said = index
        .staleness(&moved)
        .expect("a moved tree has a sentence");
    assert!(
        said.contains("the working tree")
            && said.contains("aabbccddeeff")
            && said.contains("ffeeeeeedddd"),
        "the sentence names which source moved and what it moved from and to: {said}",
    );
}

#[test]
fn a_rebuilt_index_makes_the_index_not_current() {
    let mut store = store();
    let before = corpus::from_store(
        &store,
        &corpus::Lend {
            symbols: vec![symbol("parse_lockfile", "src/parse.rs", 1, true)],
            symbols_from: Some(source("aabbccddeeff")),
            ..corpus::Lend::default()
        },
    )
    .expect("readable")
    .index();

    let epoch = store.epoch().expect("the store knows its epoch");
    let mut now = Identity::new();
    now.set(SOURCE_INDEX, format!("epoch={epoch}"));
    assert!(
        before.is_current(&now),
        "the epoch the corpus was read from is still the epoch published"
    );

    store.advance_epoch().expect("the epoch moves");
    let mut after = Identity::new();
    after.set(
        SOURCE_INDEX,
        format!("epoch={}", store.epoch().expect("readable")),
    );
    assert!(
        !before.is_current(&after),
        "an index rebuilt while a box was open is a corpus of episodes that no longer exist"
    );
    assert!(
        before
            .staleness(&after)
            .is_some_and(|why| why.contains("the index epoch came from")),
        "and it says which part moved"
    );
}

#[test]
fn a_part_the_caller_cannot_name_is_not_claimed_to_have_moved() {
    // A surface that only knows the store's epoch asks about the store's epoch, and says nothing about the
    // tree it cannot see. `Identity` is a set of parts for exactly this reason.
    let index = corpus_with(
        vec![symbol("parse_lockfile", "src/parse.rs", 1, true)],
        Some(source("aabbccddeeff")),
        None,
    )
    .index();
    let mut only_index = Identity::new();
    only_index.set(SOURCE_INDEX, "epoch=1");
    assert!(index.is_current(&only_index), "nothing was contradicted");
    assert!(index.staleness(&only_index).is_none());
}

// ---------------------------------------------------------------------------
// what the rows can do: deep links, filters, and one honest ordering
// ---------------------------------------------------------------------------

fn mixed() -> Corpus {
    let mut corpus = corpus_with(
        vec![
            symbol("parse_lockfile", "src/parse.rs", 1, true),
            symbol("Store", "src/store.rs", 2, true),
        ],
        Some(source("aabbccddeeff")),
        None,
    );
    corpus.documents = 1;
    let mut episode = Entry::episode(
        "e8f1a2c3d4",
        "epoch-safe store reads",
        "pin the epoch before the read",
        Some("Dana Stahl".to_owned()),
        Some(now()),
        Some(82),
    );
    episode.paths = vec!["src/store.rs".to_owned()];
    corpus.push(episode);
    corpus.push(Entry::path(
        "src/parse.rs",
        Some("e8f1a2c3d4".to_owned()),
        "1 episode",
        Some(now()),
    ));
    corpus
}

#[test]
fn a_symbol_hit_deep_links_to_the_file_that_declares_it() {
    let index = mixed().index();
    let report = index.search(&Query::parse("parse_lockfile", now()), 20);
    let hit = report
        .hits
        .first()
        .unwrap_or_else(|| panic!("the declaration is found: {report:?}"));
    assert_eq!(hit.kind, Kind::Symbol);
    assert_eq!(hit.file.as_deref(), Some("src/parse.rs"));
    assert_eq!(
        hit.destination(),
        Destination::Diff {
            episode: None,
            path: "src/parse.rs".to_owned(),
        },
        "a symbol hit means \"go to the file and select it\", with no episode invented to scope it"
    );
    assert!(
        hit.detail.contains("src/parse.rs:1"),
        "the row prints where in the file it is: {}",
        hit.detail
    );
}

#[test]
fn a_symbol_only_query_returns_symbols_and_nothing_else() {
    let index = mixed().index();
    for filter in ["kind:symbol", "kind:sym", "kind:symbols"] {
        let report = index.search(&Query::parse(&format!("lockfile {filter}"), now()), 20);
        assert!(!report.hits.is_empty(), "`{filter}` finds the declaration");
        assert!(
            report
                .hits
                .iter()
                .all(|hit| hit.kind == Kind::Symbol && hit.file.is_some()),
            "`{filter}` narrows the list to symbols, each of which names a file: {:?}",
            report.hits
        );
    }
}

#[test]
fn an_exact_path_still_beats_a_fuzzy_symbol_fragment() {
    // The acceptance criterion that is really about the scorer: adding a section must not teach it to prefer
    // a fuzzy symbol over the file the reviewer typed exactly.
    let index = mixed().index();
    let report = index.search(&Query::parse("src/parse.rs", now()), 20);
    assert_eq!(
        report.hits.first().map(|hit| hit.kind),
        Some(Kind::Path),
        "the path typed exactly wins: {:?}",
        report.hits
    );
    assert!(
        report.hits.iter().any(|hit| hit.kind == Kind::Symbol),
        "and the declaration is in the same list, ranked by the same scorer: {:?}",
        report.hits
    );
    let scores: Vec<i64> = report.hits.iter().map(|hit| hit.score).collect();
    let mut non_increasing = scores.clone();
    non_increasing.sort_unstable_by(|left, right| right.cmp(left));
    assert_eq!(
        scores, non_increasing,
        "the list is in score order across kinds, not grouped by kind: {:?}",
        report.hits
    );
}

#[test]
fn a_symbol_row_carries_the_files_date_and_not_an_episodes_one() {
    let index = mixed().index();
    let report = index.search(&Query::parse("parse_lockfile", now()), 20);
    let hit = &report.hits[0];
    assert!(hit.episode.is_none(), "a declaration belongs to no episode");
    assert!(
        hit.utc.is_some_and(|utc| utc < now()),
        "and its date is the file's last touch: {:?}",
        hit.utc
    );
}

// ---------------------------------------------------------------------------
// the measurement the ticket asks for, run by hand
// ---------------------------------------------------------------------------

/// Measure the working-tree scan and the index build it feeds, on a real repository.
///
/// ```text
/// cargo test -p fathom-analyze --features sqlite-bundled,langs-all --release \
///        --test search_corpus -- --ignored --nocapture measure_the_symbol_scan
/// FATHOM_SCAN_ROOT=/path/to/repo cargo test … -- --ignored --nocapture measure_the_symbol_scan
/// ```
///
/// Nothing here asserts a wall-clock number, because a test cannot know the machine it is on: the numbers
/// are printed for the reviewer who *does* know it, and `reco-6sw`'s budget assertions live in
/// `tests/search_scoring.rs`, where the corpus is synthetic and the size is fixed. What this test does
/// assert is that the scan bounded itself and said so — that it returned, that it named a tree, and that
/// every file it did not read is counted in a note rather than missing in silence.
#[test]
#[ignore = "a measurement over the machine's filesystem, not a gate"]
fn measure_the_symbol_scan() {
    use fathom_analyze::search::symbols;
    use std::time::Instant;

    let root = std::env::var("FATHOM_SCAN_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            // The workspace root, two levels above this crate: the repository the measurement is usually
            // wanted for is the one the test is being run from.
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(|crates| crates.parent())
                .map(|root| root.to_path_buf())
                .unwrap_or_else(|| std::path::PathBuf::from("."))
        });
    let root = root.canonicalize().unwrap_or(root);
    // The caps are environment-sized, because the point of running this by hand is to see what a cap costs on
    // a tree the reviewer cares about; the defaults are what the box uses.
    let cap = |key: &str, default: usize| {
        std::env::var(key)
            .ok()
            .and_then(|text| text.parse().ok())
            .unwrap_or(default)
    };
    let policy = symbols::Policy {
        max_files: cap("FATHOM_SCAN_MAX_FILES", symbols::MAX_FILES),
        max_bytes: cap("FATHOM_SCAN_MAX_BYTES", symbols::MAX_BYTES),
        ..symbols::Policy::default()
    };

    let started = Instant::now();
    let stamp = symbols::stamp(&root, &policy);
    let stamped = started.elapsed();

    let started = Instant::now();
    let scan = symbols::walk(&root, &policy);
    let walked = started.elapsed();
    assert_eq!(
        stamp, scan.source,
        "the cheap stamp is the tree the walk described, which is what makes it worth asking about"
    );

    let started = Instant::now();
    let mut corpus = Corpus::new();
    let missing = scan.symbols.is_empty().then(|| scan.gap_note());
    corpus::add_symbols(
        &mut corpus,
        &scan.symbols,
        Some(&scan.source),
        missing.as_deref(),
    );
    let rows = corpus.entries.len();
    let index = corpus.index();
    let built = started.elapsed();

    let resident = std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|statm| {
            statm
                .split_whitespace()
                .nth(1)
                .and_then(|pages| pages.parse::<usize>().ok())
        })
        .map_or_else(
            || "?".to_owned(),
            |pages| format!("{:.1} MiB", pages as f64 * 4096.0 / 1_048_576.0),
        );

    eprintln!(
        "scan of {}:\n  stamp {} in {stamped:?}\n  walk {walked:?}: {} files read of {} found ({} bytes), {} \
         declarations ({} of them exported), {} unavailable, {} with no grammar\n  Index::build over {rows} \
         symbol rows: {built:?}; resident {resident}\n  notes: {:?}",
        root.display(),
        scan.source.stamp(),
        scan.read,
        scan.found,
        scan.bytes,
        scan.symbols.len(),
        scan.symbols.iter().filter(|symbol| symbol.exported).count(),
        scan.unavailable,
        scan.no_grammar,
        scan.notes
    );
    assert!(
        !scan.source.digest.is_empty(),
        "a scan always names the tree it walked"
    );
    assert_eq!(
        index.count(Kind::Symbol),
        std::cmp::min(scan.symbols.len(), SYMBOL_MAX),
        "the rows the index holds are the declarations the scan returned, capped where the builder says"
    );
    if scan.symbols.len() > SYMBOL_MAX {
        assert!(
            corpus.gaps.iter().any(|gap| gap.state == "partial"),
            "a narrowing that big must be stated"
        );
    }
}

// ---------------------------------------------------------------------------
// the walk itself, over a tree that exists
// ---------------------------------------------------------------------------

/// A working tree with two declarations in it, written to a directory no other test is using.
fn tree(files: &[(&str, &str)]) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "fathom-search-symbols-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    for (path, text) in files {
        let full = root.join(path);
        std::fs::create_dir_all(full.parent().expect("a directory to write into"))
            .expect("create the fixture directory");
        std::fs::write(full, text).expect("write the fixture file");
    }
    root
}

#[test]
// The walk hands files to the structural extractor, and a grammar is a feature in this build exactly as it
// is one for the extractor itself (`ADR-002 §6.2`): a grammar-less build of this crate proves the *walk*, and
// the next test proves what such a build says out loud.
#[cfg(feature = "lang-rust")]
fn the_stamps_that_name_a_tree_agree_with_the_walk_that_read_it() {
    // The whole staleness story rests on one property: [`symbols::stamp`] is the same walk with the reads
    // taken out, so the cheap question "is the tree I am holding still the tree?" is answered with the same
    // words the corpus was stamped with. If those two ever disagree, `is_current` says "stale" at the wrong
    // moments — which is worse than not asking, because the reviewer learns to ignore it.
    use fathom_analyze::search::symbols;

    let root = tree(&[
        // Root-level files, because Rust's export rule is a fact about the crate: a name is public API only
        // if every module between it and the crate root is `pub`. Two loose files under `src/` with no crate
        // root in the corpus are not public API by that rule, and asserting otherwise would test the module
        // path derivation and not the walk.
        (
            "parse.rs",
            "pub fn parse_lockfile(text: &str) -> usize { text.len() }\n",
        ),
        ("store.rs", "fn read() {}\n"),
    ]);
    let policy = symbols::Policy::default();

    let stamp = symbols::stamp(&root, &policy);
    let scan = symbols::walk(&root, &policy);
    assert_eq!(
        stamp, scan.source,
        "the cheap stamp and the walk describe one tree in one sentence"
    );
    assert_eq!(
        symbols::stamp(&root, &policy).digest,
        stamp.digest,
        "an unchanged tree stamps the same twice"
    );
    let names: Vec<&str> = scan
        .symbols
        .iter()
        .map(|symbol| symbol.name.as_str())
        .collect();
    assert!(
        names.contains(&"parse_lockfile") && names.contains(&"read"),
        "the declarations in the tree are the declarations the walk returns: {names:?}"
    );
    let lockfile = scan
        .symbols
        .iter()
        .find(|symbol| symbol.name == "parse_lockfile")
        .expect("the declaration is there");
    assert_eq!(lockfile.path, "parse.rs");
    assert_eq!(
        lockfile.line, 1,
        "the line is one-based, as the extractor counts it"
    );
    // What counts as exported is the extractor's rule, per language, and `tests/structure_core.rs` is where
    // that rule is pinned; the walk only carries the answer, and the corpus test above pins what the corpus
    // does with it when the cap bites.
    assert!(
        scan.symbols.iter().all(|symbol| symbol.line >= 1),
        "every declaration carries a one-based line"
    );
    let paths: Vec<&str> = scan.symbols.iter().map(|s| s.path.as_str()).collect();
    assert_eq!(
        paths,
        vec!["parse.rs", "store.rs"],
        "declarations come back in path order, which is the order the caps are spent in: {paths:?}"
    );
    assert!(
        scan.symbols.iter().any(|symbol| symbol.name == "read"),
        "a private name is in the section too — it is only ranked after the exported ones when the cap \
         bites, and the cap is not in play in a two-file tree"
    );
    assert!(
        scan.symbols.iter().all(|symbol| symbol.language == "rust"),
        "the row says which grammar read it, because two same-named declarations in two languages are \
         two rows"
    );
    assert!(
        scan.notes.is_empty(),
        "a two-file tree fits every cap and should not narrate it: {:?}",
        scan.notes
    );

    // An edit moves the stamp, with nothing committed and no store involved: this is the working tree, and
    // the tree is what the symbol rows are a function of.
    std::fs::write(root.join("store.rs"), "fn read() {}\nfn write() {}\n")
        .expect("edit the fixture file");
    assert_ne!(
        symbols::stamp(&root, &policy).digest,
        stamp.digest,
        "a changed file moves the digest, or the identity is decorative"
    );
}

#[test]
// The same tree, in a build that linked no grammar at all. This is the distinction ADR-001 §7.2 keeps
// insisting on: "this repository declares no names" and "this binary cannot read any" are different remedies,
// and a build of fathom without a grammar must never print the first.
#[cfg(not(feature = "lang-rust"))]
fn a_build_with_no_grammar_says_so_rather_than_saying_the_tree_is_empty() {
    use fathom_analyze::search::symbols;

    let root = tree(&[(
        "src/parse.rs",
        "pub fn parse_lockfile(text: &str) -> usize { text.len() }\n",
    )]);
    let scan = symbols::walk(&root, &symbols::Policy::default());
    assert_eq!(scan.found, 1, "the file is a language the walk knows");
    assert!(scan.symbols.is_empty(), "and nothing was read out of it");
    assert_eq!(
        scan.no_grammar, scan.found,
        "every file was refused because no grammar is linked, not because it is noise"
    );
    let said = scan.gap_note();
    assert!(
        said.contains("grammar") && said.contains("rust"),
        "the sentence names the missing grammars and the languages it looked for: {said}"
    );

    let mut corpus = Corpus::new();
    corpus::add_symbols(&mut corpus, &scan.symbols, Some(&scan.source), Some(&said));
    let gap = corpus
        .gaps
        .iter()
        .find(|gap| gap.kind == Kind::Symbol)
        .expect("the section states itself");
    assert_eq!(gap.state, "not indexed");
    assert_eq!(
        gap.because, said,
        "the caller's sentence is the gap's sentence"
    );
}
