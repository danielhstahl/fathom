//! Revision-expression resolution: the grammar `e1.1` promises, and a typed rejection of everything
//! outside it.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/mod.rs"]
mod common;

use common::*;
use fathom_core::error::{Diagnosable, ErrorCode};
use fathom_ingest::error::Error;
use fathom_ingest::oid::Oid;
use fathom_ingest::repo::Repository;
use std::path::Path;

fn open(path: &Path) -> Repository {
    Repository::discover(path).expect("discover")
}

/// A repository with the shapes a revspec has to cope with: branches, an annotated tag, a lightweight
/// tag, a remote-tracking ref, a merge, and a short-id collision on purpose.
struct RevFixture {
    // Kept alive so the temporary directory outlives the assertions.
    _fixture: Fixture,
    root: std::path::PathBuf,
    a: String,
    b: String,
    c: String,
    d: String,
}

fn rev_fixture() -> RevFixture {
    let fixture = Fixture::new("revspec");
    let root = fixture.init();
    let a = fixture.commit(&root, "A: root", &[("one.txt", "one\n")]);
    let b = fixture.commit(&root, "B: main", &[("two.txt", "two\n")]);
    git(&root, &["checkout", "-q", "-b", "topic", &a]);
    let c = fixture.commit(&root, "C: topic", &[("three.txt", "three\n")]);
    git(&root, &["checkout", "-q", "main"]);
    let d = fixture.merge(&root, &c, "D: merge topic");
    fixture.tag(&root, "v1.0");
    fixture.lightweight_tag(&root, "plain");
    fixture.branch(&root, "deep/branch/name");
    std::fs::create_dir_all(root.join(".git/refs/remotes/origin")).expect("mkdir");
    std::fs::write(root.join(".git/refs/remotes/origin/main"), format!("{b}\n")).expect("ref");
    RevFixture {
        _fixture: fixture,
        root,
        a,
        b,
        c,
        d,
    }
}

#[test]
fn head_and_its_alias_resolve_to_the_checked_out_commit() {
    let fixture = rev_fixture();
    let repo = open(&fixture.root);
    for spec in ["HEAD", "head", "@"] {
        assert_eq!(
            repo.resolve(spec).expect(spec).hex(),
            fixture.d,
            "`{spec}` is HEAD"
        );
    }
    assert_eq!(repo.resolve("@").expect("@").hex(), git_head(&fixture.root));
}

#[test]
fn full_ids_round_trip_and_short_ids_resolve_by_prefix() {
    let fixture = rev_fixture();
    let repo = open(&fixture.root);
    assert_eq!(repo.resolve(&fixture.a).expect("full").hex(), fixture.a);
    // git's own default abbreviation length must resolve to the same commit: if it did not, a SHA
    // pasted from `git log --oneline` would mean something different in fathom than in git.
    for length in 7..=12 {
        let prefix = &fixture.a[..length];
        match repo.resolve(prefix) {
            Ok(oid) => assert_eq!(oid.hex(), fixture.a, "prefix of length {length}"),
            Err(Error::RevSpecRejected { .. }) => {
                panic!("an unambiguous {length}-character prefix must resolve")
            }
            Err(other) => panic!("unexpected {other:?}"),
        }
        assert_eq!(git_rev_parse(&fixture.root, prefix), fixture.a);
    }
}

#[test]
fn a_prefix_of_the_wrong_length_or_an_ambiguous_one_is_rejected_not_guessed() {
    let fixture = rev_fixture();
    let repo = open(&fixture.root);
    // Three characters is shorter than the minimum git will abbreviate to, and fathom refuses to
    // invent an abbreviation policy of its own.
    let too_short = &fixture.a[..3];
    let error = repo
        .resolve(too_short)
        .expect_err("three characters is not an id");
    assert_eq!(error.code(), ErrorCode::RevSpecRejected);
    assert_eq!(error.code().exit(), fathom_core::exit::ExitCode::Usage);
    // An id that is well-formed but not in this repository.
    let absent = repo
        .resolve("0123456789abcdef0123456789abcdef01234567")
        .expect_err("no such object");
    assert!(format!("{absent}").contains("no such object"), "{absent}");
    // Not hex at all, and not a ref either: the message says which of the two it tried.
    let nonsense = repo.resolve("no-such-thing").expect_err("no such ref");
    assert!(
        format!("{nonsense}").contains("branch, tag, remote branch"),
        "{nonsense}"
    );
}

#[test]
fn short_ref_names_resolve_in_gits_order() {
    let fixture = rev_fixture();
    let repo = open(&fixture.root);
    let cases: [(&str, &str); 7] = [
        ("main", &fixture.d),
        ("refs/heads/main", &fixture.d),
        ("topic", &fixture.c),
        ("refs/heads/topic", &fixture.c),
        ("origin/main", &fixture.b),
        ("plain", &fixture.d),
        ("deep/branch/name", &fixture.d),
    ];
    for (spec, expected) in cases {
        assert_eq!(
            repo.resolve(spec)
                .unwrap_or_else(|err| panic!("{spec}: {err}"))
                .hex(),
            expected,
            "`{spec}`"
        );
        assert_eq!(
            git_rev_parse(&fixture.root, spec),
            expected,
            "git resolves `{spec}` to the same commit"
        );
    }
    // A tag name is a revspec too. `fathom`'s `resolve` returns a *commit* — that is its contract — so
    // the git comparison is against git's peeled form: `git rev-parse v1.0` on its own prints the tag
    // object, while `git log v1.0` and `git checkout v1.0` both mean the commit, which is what fathom
    // answers. `v1.0^{tag}` below is how the tag object itself is asked for.
    assert_eq!(repo.resolve("v1.0").expect("tag").hex(), fixture.d);
    assert_eq!(git_rev_parse(&fixture.root, "v1.0^{commit}"), fixture.d);
}

#[test]
fn ancestry_suffixes_walk_the_way_git_walks_them() {
    let fixture = rev_fixture();
    let repo = open(&fixture.root);
    let cases: [(&str, &str); 11] = [
        ("HEAD~1", fixture.b.as_str()),
        ("HEAD~2", fixture.a.as_str()),
        ("HEAD~", fixture.b.as_str()),
        ("HEAD^", fixture.b.as_str()),
        ("HEAD^1", fixture.b.as_str()),
        ("HEAD^2", fixture.c.as_str()),
        ("HEAD~1~1", fixture.a.as_str()),
        ("HEAD^1^1", fixture.a.as_str()),
        ("main~1", fixture.b.as_str()),
        ("v1.0~1", fixture.b.as_str()),
        ("HEAD~0", fixture.d.as_str()),
    ];
    for (spec, expected) in cases {
        assert_eq!(
            repo.resolve(spec)
                .unwrap_or_else(|err| panic!("`{spec}`: {err}"))
                .hex(),
            expected,
            "`{spec}`"
        );
        assert_eq!(
            git_rev_parse(&fixture.root, spec),
            expected,
            "git agrees on `{spec}`"
        );
    }
}

#[test]
fn a_suffix_past_the_history_is_a_typed_error_naming_the_commit_that_ran_out() {
    let fixture = rev_fixture();
    let repo = open(&fixture.root);
    let error = repo
        .resolve("HEAD~9")
        .expect_err("there is no ninth ancestor");
    match &error {
        Error::RevSpecRejected { spec, why } => {
            assert_eq!(spec, "HEAD~9");
            assert!(
                why.contains("root commit") || why.contains("no first parent"),
                "the message says which commit ran out: {why}"
            );
            assert!(why.contains(&fixture.a), "and names it: {why}");
        }
        other => panic!("expected a rejection, got {other:?}"),
    }
    let no_second_parent = repo
        .resolve("HEAD~1^2")
        .expect_err("B has no second parent");
    assert_eq!(no_second_parent.code(), ErrorCode::RevSpecRejected);
    assert!(
        format!("{no_second_parent}").contains("1 parent"),
        "the message counts what exists: {no_second_parent}"
    );
}

#[test]
fn peeling_is_explicit_and_bounded() {
    let fixture = rev_fixture();
    let repo = open(&fixture.root);
    let tag_object: Oid = {
        let all = repo
            .refs(fathom_ingest::repo::RefFilter::Tags)
            .expect("refs");
        let annotated = all
            .iter()
            .find(|reference| reference.name == "refs/tags/v1.0")
            .expect("v1.0");
        match annotated.target {
            fathom_ingest::repo::RefTarget::Direct { oid } => oid,
            fathom_ingest::repo::RefTarget::Symbolic { .. } => panic!("an annotated tag is direct"),
        }
    };
    // `^{}` and `^{commit}` both end at the commit; `^{tag}` stops at the tag object; `^{tree}` goes to
    // the commit's tree.
    assert_eq!(repo.resolve("v1.0^{}").expect("peel").hex(), fixture.d);
    assert_eq!(
        repo.resolve("v1.0^{commit}").expect("peel").hex(),
        fixture.d
    );
    assert_eq!(
        repo.resolve("v1.0^{tag}").expect("tag").hex(),
        tag_object.hex()
    );
    assert_eq!(repo.resolve("v1.0^{}^{tree}").expect("tree").hex(), {
        let head = repo.head().expect("head").oid().expect("born");
        repo.commit(head)
            .expect("commit")
            .expect("commit")
            .tree
            .hex()
    });
    // git's own peel syntax agrees, which is the half that says the grammar is the real one.
    assert_eq!(git_rev_parse(&fixture.root, "v1.0^{}"), fixture.d);
    assert_eq!(git_rev_parse(&fixture.root, "v1.0^{commit}"), fixture.d);
    assert_eq!(git_rev_parse(&fixture.root, "v1.0^{tag}"), tag_object.hex());

    // Asking a commit for a tag is nonsense, and says so.
    let error = repo.resolve("HEAD^{tag}").expect_err("HEAD is not a tag");
    assert_eq!(error.code(), ErrorCode::RevSpecRejected);
}

#[test]
fn expressions_fathom_does_not_support_are_refused_with_the_grammar() {
    let fixture = rev_fixture();
    let repo = open(&fixture.root);
    // Each of these is real git syntax that needs reflog parsing, a history search, or a pathspec
    // engine; fathom refuses them by name rather than answering something different.
    for spec in [
        "HEAD@{2}",
        "@{-1}",
        ":/root commit",
        "main@{yesterday}",
        "HEAD@{0}",
    ] {
        let error = repo.resolve(spec).expect_err("unsupported");
        assert_eq!(error.code(), ErrorCode::RevSpecRejected, "`{spec}`");
        assert!(
            error
                .hint()
                .is_some_and(|hint| hint.contains("short SHAs") || hint.contains("HEAD")),
            "the hint must say what fathom does accept: {:?}",
            error.hint()
        );
    }
    // An empty spec is not a shorthand for HEAD.
    assert_eq!(
        repo.resolve("").expect_err("empty").code(),
        ErrorCode::RevSpecRejected
    );
    assert_eq!(
        repo.resolve("   ").expect_err("blank").code(),
        ErrorCode::RevSpecRejected
    );
}

#[test]
fn an_unborn_head_has_no_commit_and_says_which_branch_is_waiting() {
    let fixture = Fixture::new("unborn-rev");
    let root = fixture.init();
    let repo = open(&root);
    let error = repo.resolve("HEAD").expect_err("nothing to resolve to");
    assert_eq!(error.code(), ErrorCode::RevSpecRejected);
    assert!(
        format!("{error}").contains("no commits yet"),
        "the message explains the state: {error}"
    );
    // A named branch that does not exist is a different failure, and the message differs.
    assert!(
        repo.resolve("main").is_err(),
        "the unborn branch is not resolvable either"
    );
}

#[test]
fn a_revspec_that_names_a_blob_is_refused_as_not_a_commit() {
    let fixture = Fixture::new("blob-rev");
    let root = fixture.init();
    fixture.commit(&root, "first", &[("one.txt", "contents\n")]);
    let repo = open(&root);
    let head = repo.head().expect("head").oid().expect("born");
    let header = repo.commit(head).expect("commit").expect("commit");
    let blob = repo.tree_entries(header.tree).expect("tree").remove(0).oid;
    let error = repo
        .resolve(&blob.hex())
        .expect_err("a blob is not a revision");
    assert!(format!("{error}").contains("blob"), "{error}");
    // A tree by itself is legal through `^{tree}`, but not as a start point.
    assert!(format!("{}", repo.resolve(&header.tree.hex()).expect_err("tree")).contains("tree"));
    assert_eq!(
        repo.resolve(&format!("{}^{{tree}}", header.tree.hex()))
            .expect("a tree peels to itself")
            .hex(),
        header.tree.hex()
    );
}

#[test]
fn detached_head_and_symbolic_refs_still_resolve() {
    let fixture = rev_fixture();
    detach(&fixture);
    let repo = open(&fixture.root);
    assert_eq!(
        repo.resolve("HEAD").expect("detached head").hex(),
        fixture.b
    );
    assert_eq!(repo.resolve("HEAD^").expect("its parent").hex(), fixture.a);
    // The branch still exists even though HEAD is not on it.
    assert_eq!(repo.resolve("main").expect("branch").hex(), fixture.d);
}

fn detach(fixture: &RevFixture) {
    git(&fixture.root, &["checkout", "-q", "--detach", &fixture.b]);
}

#[test]
fn a_name_that_is_also_a_valid_hex_prefix_prefers_the_id() {
    // git checks the object id first for a plausible hex string, and so does fathom: the ambiguity is
    // rare (a branch called `abc123`), but the tie-break must be *stated*, not accidental.
    let fixture = Fixture::new("hex-name");
    let root = fixture.init();
    let a = fixture.commit(&root, "A: root", &[("one.txt", "one\n")]);
    fixture.commit(&root, "B: main", &[("two.txt", "two\n")]);
    // A branch whose name happens to be the first eight hex characters of its own parent commit.
    git(&root, &["branch", &a[..8], &a]);
    let repo = open(&root);
    assert_eq!(
        repo.resolve(&a[..8]).expect("the id wins").hex(),
        a,
        "the object id is what the string names"
    );
    assert_eq!(git_rev_parse(&root, &a[..8]), a, "and git agrees");
}
