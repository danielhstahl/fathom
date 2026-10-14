//! The backend seam: which failures may reach a second reader, and which may not (e1.1).
//!
//! The ticket's constraint is that the default path never runs `git`, while still leaving room for a
//! `git`-binary fallback in a later ticket. The way to test a seam before the second backend exists is to
//! put a fake in each seat and pin the routing rule down exactly: one test per eligible failure, one per
//! ineligible one, and a counter on the fallback so "we did not ask" is an assertion rather than a hope.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/mod.rs"]
mod common;

use common::*;
use fathom_core::error::{Diagnosable, ErrorCode};
use fathom_ingest::backend::{GitReader, NativeReader, Reader, fallback_eligible};
use fathom_ingest::error::{Error, TruncationCause, Unavailable, WorkingTreeUnavailable};
use fathom_ingest::graph::{CommitHeader, Detail, LogEntry, WalkOptions, WalkOutcome};
use fathom_ingest::oid::Oid;
use fathom_ingest::repo::{
    Head, ObjectHash, Ref, RefFilter, RepoHealth, RepoIdentity, Repository, RepositoryKind,
};
use fathom_ingest::state::{ScanOptions, WorkingState};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The commit id a fake answers with, so a test can tell whose answer it got.
const FAKE_OID: &str = "1111111111111111111111111111111111111111";

/// Calls made to one fake reader. Shared so the test can read it after the reader owns the fake.
#[derive(Debug, Default)]
struct Counter {
    head: AtomicUsize,
    resolve: AtomicUsize,
    refs: AtomicUsize,
    commit: AtomicUsize,
    walk: AtomicUsize,
}

impl Counter {
    fn total(&self) -> usize {
        self.head.load(Ordering::Relaxed)
            + self.resolve.load(Ordering::Relaxed)
            + self.refs.load(Ordering::Relaxed)
            + self.commit.load(Ordering::Relaxed)
            + self.walk.load(Ordering::Relaxed)
    }
}

/// The failure a fake should produce, as data: `Error` is not `Clone`, and a fake that is asked twice has
/// to fail twice.
#[derive(Debug, Clone, Copy)]
enum Failure {
    None,
    NotARepository,
    Untrusted,
    RevSpec,
    MissingObject,
    Shallow,
    Bare,
    Unsupported,
    FallbackRequired,
}

impl Failure {
    fn build(self) -> Error {
        match self {
            Failure::None => unreachable!("asked to fail a fake that was told not to"),
            Failure::NotARepository => Error::NotARepository {
                root: "/nowhere".into(),
            },
            Failure::Untrusted => Error::RepositoryUntrusted {
                root: "/nowhere".into(),
                candidate: "/nowhere/.git".into(),
            },
            Failure::RevSpec => Error::RevSpecRejected {
                spec: "no-such-thing".into(),
                why: "no such ref".into(),
            },
            Failure::MissingObject => Error::ObjectUnavailable {
                oid: Oid::from_str(MISSING_OID).expect("oid"),
                why: Unavailable::Missing,
            },
            Failure::Shallow => Error::HistoryTruncated {
                cause: TruncationCause::Shallow { boundary: vec![] },
                boundary: vec![],
            },
            Failure::Bare => Error::WorkingTreeUnavailable {
                root: "/nowhere".into(),
                why: WorkingTreeUnavailable::Bare,
            },
            Failure::Unsupported => Error::RepositoryUnsupported {
                detail: "an object format this build cannot parse".into(),
            },
            Failure::FallbackRequired => Error::FallbackRequired {
                what: "reading a SHA-256 repository",
                remedy: "build fathom with SHA-256 support",
            },
        }
    }
}

const MISSING_OID: &str = "0123456789abcdef0123456789abcdef01234567";

/// A reader that answers with fixed values — or fails on purpose — and counts what it was asked.
struct Fake {
    identity: RepoIdentity,
    failure: Failure,
    counter: Arc<Counter>,
}

impl Fake {
    fn new(label: &str) -> Self {
        Self::with_failure(label, Failure::None)
    }

    fn with_failure(label: &str, failure: Failure) -> Self {
        Fake {
            identity: RepoIdentity {
                display_name: label.to_owned(),
                git_dir: PathBuf::from("/nowhere/.git"),
                common_dir: PathBuf::from("/nowhere/.git"),
                work_dir: Some(PathBuf::from("/nowhere")),
                kind: RepositoryKind::WorkTree,
                object_hash: ObjectHash::Sha1,
                superproject: None,
                trusted: true,
            },
            failure,
            counter: Arc::new(Counter::default()),
        }
    }

    fn fake_oid() -> Oid {
        Oid::from_str(FAKE_OID).expect("oid")
    }

    /// Fail if told to, otherwise take the counted success path.
    fn answer<T>(&self, field: &AtomicUsize, value: impl FnOnce() -> T) -> Result<T, Error> {
        field.fetch_add(1, Ordering::Relaxed);
        match self.failure {
            Failure::None => Ok(value()),
            other => Err(other.build()),
        }
    }
}

impl GitReader for Fake {
    fn identity(&self) -> &RepoIdentity {
        &self.identity
    }

    fn head(&self) -> Result<Head, Error> {
        self.answer(&self.counter.head, || Head::Detached {
            oid: Self::fake_oid(),
        })
    }

    fn resolve(&self, _spec: &str) -> Result<Oid, Error> {
        self.answer(&self.counter.resolve, Self::fake_oid)
    }

    fn list_refs(&self, _filter: RefFilter) -> Result<Vec<Ref>, Error> {
        self.answer(&self.counter.refs, Vec::new)
    }

    fn read_commit(&self, _oid: Oid, _detail: Detail) -> Result<Option<CommitHeader>, Error> {
        self.answer(&self.counter.commit, || {
            Some(CommitHeader {
                oid: Self::fake_oid(),
                tree: Self::fake_oid(),
                parents: Vec::new(),
                author: fathom_ingest::graph::Signature {
                    name: "Fake".into(),
                    email: "fake@fathom.invalid".into(),
                    when: fathom_core::time::Timestamp::UNIX_EPOCH,
                    offset_minutes: 0,
                },
                committer: fathom_ingest::graph::Signature {
                    name: "Fake".into(),
                    email: "fake@fathom.invalid".into(),
                    when: fathom_core::time::Timestamp::UNIX_EPOCH,
                    offset_minutes: 0,
                },
                subject: "fake".into(),
                message: b"fake\n".to_vec(),
                extra_header_names: Vec::new(),
            })
        })
    }

    fn health(&self) -> Result<RepoHealth, Error> {
        // Deliberately not faked: `RepoHealth` is a report *about a repository*, and a fabricated one
        // would test nothing. `Reader::native` covers it against a real repository below.
        Err(Error::NotImplemented {
            what: "health through a fake backend",
            ticket: "e1.1",
        })
    }

    fn working_state(&self, _options: &ScanOptions) -> Result<WorkingState, Error> {
        Err(Error::NotImplemented {
            what: "working state through a fake backend",
            ticket: "e1.1",
        })
    }

    fn walk(&self, _options: &WalkOptions) -> Result<WalkOutcome, Error> {
        self.answer(&self.counter.walk, || WalkOutcome {
            entries: vec![LogEntry {
                oid: Self::fake_oid(),
                parents: Vec::new(),
                depth: 0,
                detail: None,
            }],
            notices: Vec::new(),
            truncation: None,
        })
    }
}

fn open(path: &Path) -> Repository {
    Repository::discover(path).expect("discover")
}

/// One reader pair, with the primary failing as told, plus both counters.
fn pair(primary: Failure, fallback: Failure) -> (Reader, Arc<Counter>, Arc<Counter>) {
    let primary_reader = Fake::with_failure("primary", primary);
    let fallback_reader = Fake::with_failure("fallback", fallback);
    let primary_counter = Arc::clone(&primary_reader.counter);
    let fallback_counter = Arc::clone(&fallback_reader.counter);
    (
        Reader::delegating(Box::new(primary_reader), Box::new(fallback_reader)),
        primary_counter,
        fallback_counter,
    )
}

/// One verb, reduced to something a counter can check: a name, and a read whose success value does not
/// matter (only whether it succeeded).
type Verb = (&'static str, fn(&Reader) -> Result<usize, Error>);

/// The verbs a delegating reader answers, so each routing test can run over all of them instead of one.
fn verbs() -> Vec<Verb> {
    vec![
        ("head", |reader: &Reader| {
            reader
                .head()
                .map(|head| head.oid().map(|oid| oid.hex().len()).unwrap_or(0))
        }),
        ("resolve", |reader: &Reader| {
            reader.resolve("HEAD").map(|oid| oid.hex().len())
        }),
        ("list_refs", |reader: &Reader| {
            reader.list_refs(RefFilter::All).map(|refs| refs.len())
        }),
        ("read_commit", |reader: &Reader| {
            reader
                .read_commit(Oid::from_str(MISSING_OID).expect("oid"), Detail::Ids)
                .map(|header| usize::from(header.is_some()))
        }),
        ("walk", |reader: &Reader| {
            reader
                .walk(&WalkOptions::head())
                .map(|outcome| outcome.entries.len())
        }),
    ]
}

#[test]
fn an_eligible_failure_is_answered_by_the_fallback() {
    for failure in [Failure::Unsupported, Failure::FallbackRequired] {
        let error = failure.build();
        assert!(fallback_eligible(&error), "the rule must name it: {error}");
        for (verb, read) in verbs() {
            let (reader, primary, fallback) = pair(failure, Failure::None);
            read(&reader)
                .unwrap_or_else(|err| panic!("{verb}: the fallback should have answered: {err}"));
            assert_eq!(primary.total(), 1, "{verb}: the primary was asked once");
            assert_eq!(
                fallback.total(),
                1,
                "{verb}: the fallback was asked once, and only for this verb"
            );
        }
    }
}

#[test]
fn an_ineligible_failure_stays_with_the_primary() {
    // Each of these is a fact about the caller's input or about the repository's contents, not about what
    // this build can read. Asking `git` the same question would replace one clear error with a second,
    // different, and more confusing one — or, for a missing object, imply that fathom might go and get it,
    // which it never does (ADR-001 §8.2).
    for failure in [
        Failure::NotARepository,
        Failure::Untrusted,
        Failure::RevSpec,
        Failure::MissingObject,
        Failure::Shallow,
        Failure::Bare,
    ] {
        let error = failure.build();
        assert!(!fallback_eligible(&error), "{error} must not delegate");
        for (verb, read) in verbs() {
            // A fallback that *could* answer, so the only reason the caller sees an error is that asking
            // was refused.
            let (reader, primary, fallback) = pair(failure, Failure::None);
            let seen = read(&reader).expect_err("the primary's failure is the answer");
            assert_eq!(
                seen.code(),
                error.code(),
                "{verb}: the original error must survive unchanged, got {seen}"
            );
            assert_eq!(primary.total(), 1, "{verb}: the primary was asked once");
            assert_eq!(
                fallback.total(),
                0,
                "{verb}: the fallback was not asked about {error}"
            );
        }
    }
}

#[test]
fn when_the_fallback_also_fails_its_error_is_what_the_caller_gets() {
    // Two failures, one answer. The fallback's reason is the more specific one — "gix could not do X, and
    // neither could git" is the truth the user needs, and swallowing it would hide the second backend's
    // existence.
    let (reader, primary, fallback) = pair(Failure::FallbackRequired, Failure::Unsupported);
    let error = reader.head().expect_err("both failed");
    assert_eq!(error.code(), ErrorCode::RepositoryUnsupported, "{error}");
    assert_eq!(primary.total(), 1);
    assert_eq!(fallback.total(), 1);
}

#[test]
fn a_success_never_reaches_the_fallback() {
    for (verb, read) in verbs() {
        let (reader, primary, fallback) = pair(Failure::None, Failure::FallbackRequired);
        read(&reader).expect("the primary answers");
        assert_eq!(primary.total(), 1, "{verb}");
        assert_eq!(
            fallback.total(),
            0,
            "{verb}: a fallback that would have failed was never asked"
        );
    }
}

#[test]
fn a_native_reader_answers_exactly_like_the_repository_it_wraps() {
    let fixture = Fixture::new("seam");
    let root = fixture.init();
    let first = fixture.commit(&root, "first", &[("one.txt", "one\n")]);
    fixture.commit(&root, "second", &[("two.txt", "two\n")]);
    let repo = open(&root);
    let reader = Reader::native(open(&root));
    assert_eq!(
        reader.head().expect("head"),
        repo.head().expect("head"),
        "`HEAD` is identical through the seam"
    );
    assert_eq!(
        reader.list_refs(RefFilter::All).expect("refs"),
        repo.refs(RefFilter::All).expect("refs"),
        "the ref list is identical"
    );
    assert_eq!(
        reader.resolve("HEAD").expect("resolve"),
        repo.resolve("HEAD").expect("resolve")
    );
    let tip = repo.head().expect("head").oid().expect("born");
    assert_eq!(
        reader
            .read_commit(tip, Detail::Headers)
            .expect("commit")
            .expect("present")
            .subject,
        "second",
        "and so is a commit header"
    );
    assert_eq!(
        reader
            .log_entries(&WalkOptions::head())
            .expect("walk")
            .len(),
        2,
        "and the walk"
    );
    assert!(
        reader
            .working_state(&ScanOptions::default())
            .expect("state")
            .is_clean(),
        "and the working tree"
    );
    assert_eq!(
        reader.identity(),
        repo.identity(),
        "the identity comes from the same place"
    );
    assert!(
        reader.repository().is_some(),
        "a native reader still exposes the repository, because the streaming reads need it"
    );
    assert_ne!(first, git_head(&root), "the fixture really has two commits");
}

#[test]
fn a_fallback_sits_untouched_behind_a_working_repository() {
    // The default-path guarantee stated at the API level, and checked again from the outside by
    // `tests/no_git_subprocess.rs`: with a real repository and a real `gix` reader, nothing is ever asked
    // of the second backend.
    let fixture = Fixture::new("seam-quiet");
    let root = fixture.init();
    fixture.commit(&root, "first", &[("one.txt", "one\n")]);
    let watching = Fake::new("watcher");
    let counter = Arc::clone(&watching.counter);
    let reader = Reader::with_fallback(open(&root), Box::new(watching));
    reader.head().expect("head");
    reader.resolve("HEAD").expect("resolve");
    reader.list_refs(RefFilter::All).expect("refs");
    reader.walk(&WalkOptions::head()).expect("walk");
    reader
        .working_state(&ScanOptions::default())
        .expect("state");
    assert_eq!(
        counter.total(),
        0,
        "the native reader answered everything, so the fallback did no work at all"
    );
}

#[test]
fn a_missing_object_sends_a_request_nowhere() {
    // The important negative, end to end: a repository that is *missing* an object must not cause any
    // second attempt, because the only way to get a missing object is to fetch, and fathom never fetches.
    let fixture = Fixture::new("seam-missing");
    let root = fixture.init();
    fixture.commit(&root, "first", &[("one.txt", "one\n")]);
    fixture.commit(&root, "second", &[("two.txt", "two\n")]);
    let head = git_head(&root);
    fixture.remove_object(&root, &head);
    let watching = Fake::new("watcher");
    let counter = Arc::clone(&watching.counter);
    let reader = Reader::with_fallback(open(&root), Box::new(watching));
    let error = reader.resolve("HEAD").expect_err("the tip object is gone");
    assert_eq!(error.code(), ErrorCode::ObjectUnavailable, "{error}");
    assert_eq!(counter.total(), 0, "nothing was retried behind it: {error}");
}

#[test]
fn a_delegating_reader_does_not_expose_a_repository() {
    // The streaming reads need the native reader's `gix` context; once a fallback may answer, a caller
    // must not be able to assume one is there.
    let fixture = Fixture::new("seam-none");
    let root = fixture.init();
    fixture.commit(&root, "first", &[("one.txt", "one\n")]);
    let reader = Reader::with_fallback(open(&root), Box::new(Fake::new("fallback")));
    assert!(reader.repository().is_none());
    assert_eq!(
        reader.identity().display_name,
        open(&root).identity().display_name
    );
}

#[test]
fn the_trait_is_object_safe_and_send() {
    fn assert_send<T: Send + ?Sized>() {}
    assert_send::<dyn GitReader>();
    assert_send::<NativeReader>();
    assert_send::<Reader>();
    let reader: Box<dyn GitReader> = Box::new(Fake::new("object-safe"));
    assert_eq!(reader.identity().display_name, "object-safe");
    assert_eq!(reader.kind(), RepositoryKind::WorkTree);
}

#[test]
fn a_walk_through_the_seam_carries_the_same_outcome() {
    let fixture = Fixture::new("seam-walk");
    let root = fixture.init();
    fixture.commit(&root, "first", &[("one.txt", "one\n")]);
    fixture.commit(&root, "second", &[("two.txt", "two\n")]);
    fixture.commit(&root, "third", &[("three.txt", "three\n")]);
    let repo = open(&root);
    let reader = Reader::native(open(&root));
    let direct = repo.log(WalkOptions::head()).finish().expect("walk");
    let through = reader.walk(&WalkOptions::head()).expect("walk");
    let oids = |entries: &[LogEntry]| {
        entries
            .iter()
            .map(|entry| entry.oid.hex())
            .collect::<Vec<_>>()
    };
    assert_eq!(oids(&direct.entries), oids(&through.entries));
    assert_eq!(direct.notices, through.notices);
    assert_eq!(direct.truncation, through.truncation);
}
