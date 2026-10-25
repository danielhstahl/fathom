//! `search::symbols` — which files the corpus's symbol section comes from, and what they declare
//! (`reco-6sw`).
//!
//! [`crate::structure`] is deliberately a pure function of file text: *"no filesystem, no clock, no
//! environment"* is what makes it usable in a cache key and in a property test. That leaves a producer to
//! decide **which files to read**, and this module is that producer for the one search box: it asks the
//! door for the working tree, hands what comes back to [`crate::structure::extract`], and returns [`Symbol`]
//! rows the corpus can rank beside its episodes.
//!
//! # The walk is `fathom-ingest`'s; the decisions are here
//!
//! ADR-002 §3 makes this crate IO-free — an analysis that reaches for `std::fs` has a golden test that
//! depends on the machine and a cache key that is a checksum of somebody's filesystem, and `lint W1`
//! refuses it on every commit. The read did not disappear, it moved downhill to
//! [`fathom_ingest::worktree`], the same way [`crate::deps::scan`] reads dependency files through
//! [`fathom_ingest::repo`] instead of opening them (§3: *"analysis reads through `fathom-ingest` and
//! `fathom-store`"*). So the door owns the mechanical half — depth, skipped directories, `stat`, bytes —
//! and this module keeps every decision: [`Policy::wants`] says which files are worth opening, [`Policy`]'s
//! caps say what the read may cost and in what order it is spent, and [`Scan`]'s notes say
//! what the caps passed over. `reco-p41` is the ticket that moved it, after `reco-6sw` had landed the walk
//! here and left §5's R3 red on main.
//!
//! # Why a worktree walk and not the store
//!
//! A symbol is a function of a **file**. The store keeps rows per *episode*, and a declaration is not an
//! episode's: it is what the tree says right now, including the edits nobody has committed. Writing symbol
//! rows into the spine would make them a second old as the tree they describe, which is the state
//! [`super::Identity`] exists to name — so they are read live, from the same tree the reviewer is looking
//! at, and the corpus records which tree that was ([`Source`]) so a stale read can be called stale.
//!
//! # The caps, and why they are stated rather than silent
//!
//! A symbol source is one to two orders of magnitude bigger than the episode source: a 500-episode
//! repository can declare 40 000 names in 4 000 files, and [`super::Index::build`] is `O(corpus)` with no
//! streaming. So a scan bounds the files it reads ([`MAX_FILES`]) and the bytes it reads ([`MAX_BYTES`]),
//! and [`super::corpus::SYMBOL_MAX`] bounds the rows that survive into the corpus. Every one of those
//! bounds says which way it cut — a [`Scan`]'s `notes` and the corpus's `gaps` are the difference between
//! "the box is narrowing" and "the box is wrong", which is ADR-006 §5.8's line.
//!
//! # Cost, measured rather than assumed
//!
//! The scan is one directory walk plus one read per file plus one parse per file, and it runs once per
//! corpus read — when a shell starts, when the watcher notices the epoch move, once per `fathom search` —
//! and never on a keystroke. [`stamp`] is the same walk with the reads taken out — a `stat` per file and no
//! parse — so "has the tree moved since this index was built?" costs what a `stat` costs, and that is what
//! [`super::Index::is_current`] gets to be asked against.

use std::collections::BTreeMap;
use std::path::Path;

use fathom_ingest::worktree::{self, Entry, WalkPolicy};
use sha2::{Digest as _, Sha256};

use super::corpus::{Source, Symbol};
use crate::structure::{self, ExtractPolicy, Language, SourceFile, UnavailableReason};

/// How many source files one scan reads.
///
/// Four thousand is a large repository's first-party tree — the tree this repository's own walk finds is
/// 882 files, around a fifth of it — and it
/// is chosen against [`super::corpus::SYMBOL_MAX`]: past a few thousand files the symbol count hits the row
/// cap long before the file cap, so what the file cap is really stopping is a walk that turns into a `find`
/// over a tree nobody intended to search.
pub const MAX_FILES: usize = 4_000;

/// How many bytes of source text one scan reads, and the cap that actually binds on a real repository.
///
/// # the measurement, which is why this number is six and not sixteen
///
/// `reco-6sw` asked for the cost of the second source to be measured before it went in, and this repository
/// is the corpus: 882 first-party source files, 17.5 MiB, and `measure_the_symbol_scan` in
/// `crates/fathom-analyze/tests/search_corpus.rs` is the ignored test that prints these numbers rather than
/// asserting them, run `--release` on this tree:
///
/// | ceiling | files read | wall | declarations | `Index::build` |
/// |---|---|---|---|---|
/// | none | 882 of 882 | 5.27 s | 21 697 | 41 ms |
/// | 16 MiB | 813 of 882 | 4.88 s | 19 753 | 39 ms |
/// | this constant, six | 333 of 882 | 1.89 s | 8 191 (4 452 exported) | 15 ms |
///
/// The parse is about 3.5 MiB/s and the directory walk is nothing next to it, so the byte ceiling *is* the
/// budget for opening the box and the row cap [`super::SYMBOL_MAX`] is not what stops it — the uncapped row
/// is the only one of the three that reaches the cap at all, and it walks straight past it (21 697
/// declarations for 20 000 rows). Sixteen megabytes buys eleven thousand more names than six for three more
/// seconds of waiting before a search box will answer, and those eleven thousand are whichever internals the
/// byte budget reached first. What is left is the stamp: a `stat` of every file in the tree, with no read and
/// no parse, costs 135–150 ms here, which is why it is taken once per corpus read and not per keystroke.
///
/// Six megabytes is under two seconds on a tree this size — slow enough that the scan must never run on a
/// keystroke (it runs once per corpus read, and the tree's [`stamp`] says when it has to run again), and
/// small enough that a repository the size of this one is reported as narrowed rather than silently indexed
/// whole. A caller that has measured passes a bigger number here.
///
/// [`stamp`]: stamp()
pub const MAX_BYTES: usize = 6 * 1024 * 1024;

/// How deep the walk goes.
///
/// The door's own default, named here because the scan's gap note quotes it — see
/// [`worktree::DEFAULT_MAX_DEPTH`]. Six is deep enough for a monorepo's `packages/x/src/lib/y.rs` and
/// shallow enough that a tree cannot turn the walk into a crawl. Deeper repositories lose names and say so
/// in the scan's notes.
pub const MAX_DEPTH: usize = worktree::DEFAULT_MAX_DEPTH;

/// What a scan is allowed to cost.
#[derive(Debug, Clone)]
pub struct Policy {
    /// Extra directory names to skip, in addition to [`worktree::DEFAULT_SKIPPED_DIRS`].
    ///
    /// The caller feeds this from `graph.ignored_dirs`, so that the graph's opinion of what is not this
    /// repository is the search box's opinion too — two ignore lists drift, and one is a fact.
    pub ignored_dirs: Vec<String>,
    /// [`MAX_FILES`], overridable by a caller that has measured.
    pub max_files: usize,
    /// [`MAX_BYTES`].
    pub max_bytes: usize,
    /// The structural extractor's own policy, whose `max_file_bytes` refuses one oversized file.
    pub extract: ExtractPolicy,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            ignored_dirs: Vec::new(),
            max_files: MAX_FILES,
            max_bytes: MAX_BYTES,
            extract: ExtractPolicy::default(),
        }
    }
}

impl Policy {
    /// Whether a repository-relative path is inside a directory the scan will not enter.
    ///
    /// The names are the door's ([`worktree::DEFAULT_SKIPPED_DIRS`]) plus this repository's
    /// `graph.ignored_dirs`, so the graph's opinion of what is not this repository is the search box's
    /// opinion too — two ignore lists drift, and one is a fact. [`Self::walk`] hands the same list to the
    /// walk, which never enters one of those directories; this is the belt on the path that comes back.
    #[must_use]
    pub fn ignores(&self, path: &str) -> bool {
        path.split('/').any(|segment| {
            !segment.is_empty()
                && (segment.starts_with('.')
                    || worktree::DEFAULT_SKIPPED_DIRS.contains(&segment)
                    || self
                        .ignored_dirs
                        .iter()
                        .any(|ignored| ignored.eq_ignore_ascii_case(segment)))
        })
    }

    /// Whether a file the walk is offering is worth opening: a language one of the linked grammars speaks,
    /// in a directory the scan will enter.
    ///
    /// This is the whole of what the door is told about *why* a file matters — it is the predicate
    /// [`worktree::find`] calls, and it is deliberately the only analysis the walk is given.
    #[must_use]
    pub fn wants(&self, path: &str) -> bool {
        Language::detect(path).is_known() && !self.ignores(path)
    }

    /// The walk this policy asks the door for: same depth, same ignore list, no opinions of its own.
    #[must_use]
    pub fn walk_policy(&self) -> WalkPolicy {
        WalkPolicy {
            max_depth: MAX_DEPTH,
            skip_dirs: self.ignored_dirs.clone(),
        }
    }
}

/// What one scan produced.
#[derive(Debug, Clone, Default)]
pub struct Scan {
    /// The declarations, in path order and then declaration order — the order the extractor guarantees.
    pub symbols: Vec<Symbol>,
    /// Which tree these came from, for the corpus identity ([`Source`], [`stamp`]).
    pub source: Source,
    /// Files the walk found in a language some grammar speaks.
    pub found: usize,
    /// Files actually read and handed to the extractor.
    pub read: usize,
    /// Bytes read.
    pub bytes: usize,
    /// The distinct languages the walk found, sorted — printed, so "no symbols" can name what it looked for.
    pub languages: Vec<String>,
    /// Files that came back with no structure at all.
    pub unavailable: usize,
    /// Files whose grammar is not linked into this binary ([`UnavailableReason::GrammarNotCompiled`]).
    pub no_grammar: usize,
    /// Files a linked grammar refused: too large, not UTF-8, more error than code.
    pub refused: usize,
    /// The bounded, deterministically ordered sentences about what the scan could not do.
    pub notes: Vec<String>,
}

impl Scan {
    /// Why a caller has no symbol names, as the sentence the corpus prints for its gap.
    ///
    /// There is no `Option` here on purpose: a scan that produced no declarations always has a reason, and
    /// the reasons are three different remedies. "No file here is in a language we can read" is a fact about
    /// the repository, "this binary links no grammar" is a fact about the build, and "every file was
    /// refused" is a fact about the files — ADR-001 §7.2 makes the tool say which one it means.
    #[must_use]
    pub fn gap_note(&self) -> String {
        let languages = if self.languages.is_empty() {
            "none".to_owned()
        } else {
            self.languages.join(", ")
        };
        if self.found == 0 {
            return format!(
                "no file in this working tree is in a language the structural extractor speaks \
                 (looked for {languages}, to depth {MAX_DEPTH}, skipping the build and vendor \
                 directories), so there is no symbol name to search"
            );
        }
        if self.no_grammar > 0 {
            return format!(
                "this binary was built without a tree-sitter grammar (`lang-*`, ADR-002 §6.5): it found \
                 {} files in {languages} and cannot read declarations out of them — the shipped binary \
                 links all six",
                self.found
            );
        }
        if self.read == 0 {
            return format!(
                "{} files in {languages} were found and none were read, because the scan's caps were \
                 spent before it reached them ({} files, {} bytes)",
                self.found, self.found, self.bytes
            );
        }
        format!(
            "{} files in {languages} were read and none declared anything a grammar could name{}{}; \
             `fathom structure` says what the extractor made of them",
            self.read,
            if self.refused == 0 {
                String::new()
            } else {
                format!(
                    ", of which {} were refused and carry a reason instead of declarations",
                    self.refused
                )
            },
            if self.notes.is_empty() {
                String::new()
            } else {
                format!(" — {}", self.notes[0])
            }
        )
    }
}

/// Walk a working tree and read its declarations.
///
/// The walk is [`worktree::find`], asked with [`Policy::walk_policy`] and answered with [`Policy::wants`]; the
/// reading is [`Entry::read`]; everything after that — the caps, the order they are spent in, what the
/// extractor made of the bytes and what the scan says about all of it — is this module's.
///
/// Never fails: a tree that cannot be walked is a tree with no symbols in it, and the scan's `notes` and the
/// corpus's gap say which of the failures it was. The one thing a caller must not have to infer is the
/// difference between "this repository declares nothing" and "I could not look".
#[must_use]
pub fn walk(root: &Path, policy: &Policy) -> Scan {
    let found = found_files(root, policy);
    let source = stamp_of(root, &found);
    let mut languages: Vec<String> = found
        .iter()
        .filter_map(|file| {
            let language = Language::detect(&file.path);
            language.is_known().then(|| language.as_str().to_owned())
        })
        .collect();
    languages.sort();
    languages.dedup();
    let mut scan = Scan {
        source,
        found: found.len(),
        languages,
        ..Scan::default()
    };
    if found.is_empty() {
        scan.notes
            .push("no file in a language the extractor speaks was found".to_owned());
        return scan;
    }

    // One file's last touch is its declarations' date (`reco-6sw`: a symbol is a function of a file, and
    // the date a symbol row carries is the file's, never an episode's).
    let mut touched: BTreeMap<String, i64> = BTreeMap::new();
    for file in &found {
        let _ = touched.insert(file.path.clone(), file.modified);
    }

    let mut bodies: Vec<(String, Vec<u8>)> = Vec::new();
    let mut left_out = 0usize;
    for file in fair_order(&found) {
        if bodies.len() >= policy.max_files {
            left_out += 1;
        } else if usize::try_from(file.size).unwrap_or(usize::MAX) > policy.extract.max_file_bytes {
            // The extractor would refuse it anyway; refusing it here is what keeps the byte cap honest and
            // lets the scan say how much of the tree it never looked at.
            scan.unavailable += 1;
            scan.refused += 1;
            continue;
        } else if scan.bytes + usize::try_from(file.size).unwrap_or(usize::MAX) > policy.max_bytes {
            left_out += 1;
        } else {
            match file.read() {
                Ok(text) => {
                    scan.bytes += text.len();
                    bodies.push((file.path.clone(), text));
                }
                Err(_) => scan.unavailable += 1,
            }
            continue;
        }
        // A file a cap stood in front of is counted, not silently absent.
        scan.unavailable += 1;
    }
    scan.read = bodies.len();
    if left_out > 0 {
        scan.notes.push(format!(
            "the scan read {} of {} files inside its caps of {} files and {} bytes, shared across the {} \
             directories at the top of the tree; `kind:symbol` cannot name what the {left_out} file{} it \
             passed over contain — raise the caps, or search the tree itself, when that matters",
            scan.read,
            found.len(),
            policy.max_files,
            policy.max_bytes,
            top_levels(&found).len(),
            if left_out == 1 { "" } else { "s" }
        ));
    }
    if bodies.is_empty() {
        return scan;
    }

    let borrowed: Vec<SourceFile<'_>> = bodies
        .iter()
        .map(|(path, bytes)| SourceFile::new(path, bytes.as_slice()))
        .collect();
    let extracted = structure::extract(&borrowed, &policy.extract);

    let mut grammar_missing = 0usize;
    let mut other = 0usize;
    for record in &extracted.files {
        let seconds = touched.get(&record.path).copied().unwrap_or(0);
        let updated = Some(fathom_core::time::Timestamp::from_unix_micros(
            seconds.saturating_mul(1_000_000),
        ));
        let Some(shape) = &record.structure else {
            scan.unavailable += 1;
            if matches!(
                record.reason,
                Some(UnavailableReason::GrammarNotCompiled { .. })
            ) {
                grammar_missing += 1;
            } else {
                other += 1;
            }
            continue;
        };
        let exported: std::collections::BTreeSet<&str> =
            shape.exports.iter().map(String::as_str).collect();
        for declaration in &shape.symbols {
            scan.symbols.push(Symbol {
                name: declaration.name.clone(),
                kind: declaration.kind.as_str().to_owned(),
                path: record.path.clone(),
                line: declaration.span.start_line,
                language: record.language.as_str().to_owned(),
                exported: exported.contains(declaration.name.as_str()),
                updated,
            });
        }
    }
    scan.no_grammar = grammar_missing;
    scan.refused += other;
    if grammar_missing > 0 {
        scan.notes.push(format!(
            "{grammar_missing} files named a grammar this build does not link (the first wanted `{}`)",
            first_feature(&extracted)
        ));
    }
    if other > 0 {
        scan.notes.push(format!(
            "{other} files were refused or unparseable and carry a reason instead of declarations"
        ));
    }
    scan
}

/// Which tree a scan of `root` would read, without reading a single file body.
///
/// A `stat` per file and no parse: this is what makes "has the tree moved since this index was built?"
/// cheap enough to ask ([`super::Index::is_current`]). The [`Source`] it returns is identical to the one
/// [`walk`] returns for an unmoved tree, because both are [`stamp_of`] over the same walk.
#[must_use]
pub fn stamp(root: &Path, policy: &Policy) -> Source {
    stamp_of(root, &found_files(root, policy))
}

/// Which files a walk of `root` would read, as the door reported them: repository-relative path, size and
/// last touch, no body read.
///
/// One expression behind two callers — [`walk`] reads these, [`stamp`] folds them — which is the whole reason
/// the cheap question "has the tree moved?" is answerable: it is the same fold over the same list the
/// symbols came from, with the reads taken out.
fn found_files(root: &Path, policy: &Policy) -> Vec<Entry> {
    worktree::find(root, &policy.walk_policy(), &|path| policy.wants(path))
}

/// The files to read, in the order the caps should spend themselves.
///
/// Path order lets whatever sorts first take the whole byte budget: on a workspace like this one `crates/`
/// swallows everything and `tests/`, `tools/` and `docs/` are left out of the symbol section entirely — a
/// narrowing that makes `kind:symbol` answer "not in the corpus" about the one directory the reviewer was
/// standing in. So the budget is shared instead: files are grouped by the directory at the top of their
/// path and taken one from each group in turn, so every part of the tree is represented before any part
/// takes a second share.
fn fair_order(found: &[Entry]) -> Vec<&Entry> {
    let mut groups: BTreeMap<String, std::collections::VecDeque<&Entry>> = BTreeMap::new();
    for file in found {
        groups
            .entry(top_level(&file.path))
            .or_default()
            .push_back(file);
    }
    let mut out = Vec::with_capacity(found.len());
    loop {
        let mut took = false;
        for group in groups.values_mut() {
            if let Some(file) = group.pop_front() {
                out.push(file);
                took = true;
            }
        }
        if !took {
            break;
        }
    }
    out
}

/// The directory a path sits under at the top of the tree, which is the group [`fair_order`] shares the
/// budget across. A file at the root of the tree is its own group.
fn top_level(path: &str) -> String {
    let file = Path::new(path);
    let under_a_directory = file
        .parent()
        .is_some_and(|parent| !parent.as_os_str().is_empty());
    if !under_a_directory {
        return ".".to_owned();
    }
    file.iter()
        .next()
        .map(|first| first.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".".to_owned())
}

/// How many groups there are to share — the number in the scan's note about what it passed over.
fn top_levels(found: &[Entry]) -> std::collections::BTreeSet<String> {
    found.iter().map(|file| top_level(&file.path)).collect()
}

/// The identity of one walked tree: a digest over every file's path, size and last touch.
///
/// Paths and sizes alone miss a file edited to the same length; mtimes alone miss a checkout that preserved
/// them. Together they are the cheap answer to "is this the same tree", and they cost the `stat` the walk
/// already did.
fn stamp_of(root: &Path, found: &[Entry]) -> Source {
    let mut hasher = Sha256::new();
    let mut bytes: u64 = 0;
    for file in found {
        hasher.update(file.path.as_bytes());
        hasher.update([0u8]);
        hasher.update(file.size.to_le_bytes());
        hasher.update(file.modified.to_le_bytes());
        bytes += file.size;
    }
    let digest: String = hasher
        .finalize()
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let name = root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.to_string_lossy().into_owned());
    Source {
        from: format!(
            "the working tree `{name}` · {} files · {bytes} bytes",
            found.len()
        ),
        digest,
    }
}

/// The first grammar a scan asked for and did not have, for the note that names the feature to rebuild with.
fn first_feature(extracted: &structure::CorpusStructure) -> String {
    for record in &extracted.files {
        if let Some(UnavailableReason::GrammarNotCompiled { feature }) = &record.reason {
            return feature.clone();
        }
    }
    "lang-*".to_owned()
}
