//! Every change kind, one test each (e1.2 / reco-76g).
//!
//! The ticket asks for a record that says "old/new path, change type, mode, binary flag, hunk headers,
//! added/removed counts, raw rename candidate". Each test below is one of those facts, against a real
//! `git` repository rather than a mock, because the only contract that matters is what git recorded.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{Change, Fixture, binary_bytes, date_at, git, git_head, lines_of, oversized_bytes};
use fathom_ingest::normalize::{MergePolicy, Normalizer, Options};
use fathom_ingest::record::{ChangeKind, Content, Limits, NotExamined, Taxonomy, Unclassified};
use fathom_ingest::repo::Repository;

fn repo_at(path: &std::path::Path) -> Repository {
    Repository::discover(path).expect("open the fixture")
}

fn normalizer(repo: &Repository) -> Normalizer<'_> {
    Normalizer::new(repo, &Unclassified, Options::default())
}

#[test]
fn an_added_file_carries_its_mode_counts_and_one_hunk() {
    let fixture = Fixture::new("normalize-add");
    let repo = fixture.init();
    let head = fixture.commit_changes(
        &repo,
        "add the module",
        &date_at(0),
        &[Change::Add {
            path: "src/lib.rs",
            lines: 4,
        }],
    );
    let record = normalizer(&repo_at(&repo))
        .record(head.parse().unwrap())
        .expect("normalize");

    assert_eq!(record.files.len(), 1);
    let file = &record.files[0];
    assert_eq!(file.change, ChangeKind::Add);
    assert_eq!(file.path.to_string_lossy(), "src/lib.rs");
    assert!(file.old_path.is_none(), "an addition has no previous name");
    assert_eq!(file.mode_new, Some(0o100644));
    assert_eq!(file.mode_old, None);
    assert_eq!(file.added, 4);
    assert_eq!(file.removed, 0);
    assert_eq!(file.content, Content::Text);
    assert!(!file.content.is_binary());
    assert_eq!(file.hunks.len(), 1);
    assert_eq!(file.hunks[0].header(), "@@ -0,0 +1,4 @@");
    assert_eq!(
        file.size_new,
        Some(u64::try_from(lines_of("src/lib.rs", 4).len()).unwrap()),
        "the blob size, from the object header"
    );
    assert_eq!(file.size_old, None);
    assert!(file.blob_new.is_some() && file.blob_old.is_none());
    assert!(file.rename.is_none());

    assert_eq!(record.totals.files, 1);
    assert_eq!(record.totals.added, 4);
    assert_eq!(record.totals.removed, 0);
    assert_eq!(record.totals.hunks, 1);
    assert!(record.totals.binary == 0 && record.totals.unexamined == 0);
}

#[test]
fn a_modification_counts_both_sides_and_names_every_hunk() {
    let fixture = Fixture::new("normalize-modify");
    let repo = fixture.init();
    fixture.commit_changes(
        &repo,
        "base",
        &date_at(0),
        &[Change::Add {
            path: "a.txt",
            lines: 10,
        }],
    );
    let head = fixture.commit_changes(
        &repo,
        "grow it at both ends",
        &date_at(1),
        &[Change::Append {
            path: "a.txt",
            lines: 3,
        }],
    );
    let record = normalizer(&repo_at(&repo))
        .record(head.parse().unwrap())
        .expect("normalize");
    let file = &record.files[0];
    assert_eq!(file.change, ChangeKind::Modify);
    assert_eq!(file.added, 3);
    assert_eq!(file.removed, 0);
    assert_eq!(file.hunks.len(), 1);
    assert_eq!(file.hunks[0].header(), "@@ -8,3 +8,6 @@");
    assert_eq!(file.hunk_total, 1);
    assert_eq!(file.mode_old, file.mode_new);
}

#[test]
fn a_deleted_file_keeps_the_old_name_and_counts_the_removals() {
    let fixture = Fixture::new("normalize-delete");
    let repo = fixture.init();
    fixture.commit_changes(
        &repo,
        "make it",
        &date_at(0),
        &[Change::Add {
            path: "gone.txt",
            lines: 6,
        }],
    );
    let head = fixture.commit_changes(
        &repo,
        "lose it",
        &date_at(1),
        &[Change::Delete { path: "gone.txt" }],
    );
    let record = normalizer(&repo_at(&repo))
        .record(head.parse().unwrap())
        .expect("normalize");
    let file = &record.files[0];
    assert_eq!(file.change, ChangeKind::Delete);
    assert_eq!(file.path.to_string_lossy(), "gone.txt");
    assert_eq!(file.mode_old, Some(0o100644));
    assert_eq!(file.mode_new, None);
    assert_eq!(file.added, 0);
    assert_eq!(file.removed, 6);
    assert!(file.blob_old.is_some());
    assert!(file.blob_new.is_none());
    assert_eq!(file.hunks[0].header(), "@@ -1,6 +0,0 @@");
}

#[test]
fn a_rename_is_paired_and_carries_its_source_and_score() {
    let fixture = Fixture::new("normalize-rename");
    let repo = fixture.init();
    fixture.commit_changes(
        &repo,
        "the file, in its old home",
        &date_at(0),
        &[Change::Add {
            path: "old/name.rs",
            lines: 40,
        }],
    );
    let head = fixture.commit_changes(
        &repo,
        "moved, plus two lines",
        &date_at(1),
        &[
            Change::Rename {
                from: "old/name.rs",
                to: "new/name.rs",
            },
            Change::Append {
                path: "new/name.rs",
                lines: 2,
            },
        ],
    );
    let record = normalizer(&repo_at(&repo))
        .record(head.parse().unwrap())
        .expect("normalize");
    assert_eq!(
        record.files.len(),
        1,
        "a rename is one row, not a delete and an add"
    );
    let file = &record.files[0];
    assert_eq!(file.change, ChangeKind::Rename);
    assert_eq!(file.path.to_string_lossy(), "new/name.rs");
    assert_eq!(
        file.old_path.as_ref().unwrap().to_string_lossy(),
        "old/name.rs"
    );
    let rename = file.rename.as_ref().expect("a rename candidate");
    assert_eq!(rename.source.to_string_lossy(), "old/name.rs");
    assert_eq!(rename.source_blob, file.blob_old);
    assert!(!rename.copy);
    // 40 lines kept, 2 added: diffcore's formula over max(40, 42) says 96% the same.
    assert_eq!(rename.score, Some(96));
    assert_eq!(file.added, 2);
    assert_eq!(file.removed, 0);
    assert_eq!(record.totals.renames, 1);
}

#[test]
fn a_pure_rename_scores_a_perfect_hundred() {
    let fixture = Fixture::new("normalize-rename-pure");
    let repo = fixture.init();
    fixture.commit_changes(
        &repo,
        "content",
        &date_at(0),
        &[Change::Add {
            path: "a.md",
            lines: 12,
        }],
    );
    let head = fixture.commit_changes(
        &repo,
        "renamed, nothing else",
        &date_at(1),
        &[Change::Rename {
            from: "a.md",
            to: "b.md",
        }],
    );
    let file = &normalizer(&repo_at(&repo))
        .record(head.parse().unwrap())
        .expect("normalize")
        .files[0];
    assert_eq!(file.change, ChangeKind::Rename);
    assert_eq!(file.added, 0);
    assert_eq!(file.removed, 0);
    assert_eq!(file.rename.as_ref().unwrap().score, Some(100));
}

#[test]
fn the_rename_threshold_is_git_s_m_percent_and_100_means_identical_only() {
    let fixture = Fixture::new("normalize-threshold");
    let repo = fixture.init();
    fixture.commit_changes(
        &repo,
        "the file, in its old home",
        &date_at(0),
        &[Change::Add {
            path: "old/name.rs",
            lines: 40,
        }],
    );
    let head = fixture.commit_changes(
        &repo,
        "moved, plus two lines",
        &date_at(1),
        &[
            Change::Rename {
                from: "old/name.rs",
                to: "new/name.rs",
            },
            Change::Append {
                path: "new/name.rs",
                lines: 2,
            },
        ],
    );
    let repo = repo_at(&repo);
    let head = head.parse().unwrap();

    // git pairs this with the default `-M50%`, and so do we: see the test above.
    //
    // At `-M100%` git stops calling it a rename and prints a deletion plus an addition, because the two
    // extra lines are not identical content — which is the same fact the record has to report when the
    // operator says "only trust exact moves".
    let exact = Normalizer::new(
        &repo,
        &Unclassified,
        Options {
            limits: Limits {
                rename_threshold: 100,
                ..Limits::default()
            },
            ..Options::default()
        },
    )
    .record(head)
    .expect("normalize at -M100%");
    let kinds: Vec<(ChangeKind, String)> = exact
        .files
        .iter()
        .map(|file| (file.change, file.path.to_string_lossy().into_owned()))
        .collect();
    assert!(
        kinds.iter().any(|(kind, _)| *kind == ChangeKind::Delete),
        "at 100% the pairing must fail rather than flatter us: {kinds:?}"
    );
    assert_eq!(exact.totals.renames, 0);
}

#[test]
fn a_binary_file_is_measured_as_binary_and_not_diffed() {
    let fixture = Fixture::new("normalize-binary");
    let repo = fixture.init();
    fixture.commit_changes(
        &repo,
        "a picture",
        &date_at(0),
        &[Change::AddBytes {
            path: "img.png",
            bytes: binary_bytes(4096),
        }],
    );
    let head = git_head(&repo).parse().unwrap();
    let record = normalizer(&repo_at(&repo)).record(head).expect("normalize");
    let file = &record.files[0];
    assert_eq!(file.content, Content::Binary);
    assert!(file.content.is_binary());
    assert_eq!(file.added, 0, "a binary file has no line counts to claim");
    assert_eq!(file.removed, 0);
    assert!(file.hunks.is_empty());
    assert_eq!(file.hunk_total, 0);
    assert_eq!(record.totals.binary, 1);
    assert_eq!(
        record.totals.unexamined, 0,
        "binary is not the same as unexamined"
    );
}

#[test]
fn a_file_over_the_cap_is_recorded_unexamined_with_its_size() {
    let fixture = Fixture::new("normalize-oversized");
    let repo = fixture.init();
    let oversized = oversized_bytes();
    assert!(oversized.len() > 8 * 1024 * 1024);
    let head = fixture.commit_changes(
        &repo,
        "a log nobody should read into memory",
        &date_at(0),
        &[Change::AddBytes {
            path: "huge.log",
            bytes: oversized.clone(),
        }],
    );
    let record = normalizer(&repo_at(&repo))
        .record(head.parse().unwrap())
        .expect("normalize");
    let file = &record.files[0];
    assert!(
        oversized.len() as u64 > Limits::default().blob_memory_cap,
        "the fixture must actually be over the documented cap"
    );
    assert_eq!(
        file.content,
        Content::NotExamined {
            reason: NotExamined::Oversized {
                size: oversized.len() as u64,
            }
        },
        "the size is recorded from the object header, without inflating the blob"
    );
    assert!(!file.content.is_binary(), "not examined is not binary");
    assert!(!file.content.measured());
    assert_eq!(file.added, 0);
    assert_eq!(file.hunks.len(), 0);
    assert_eq!(file.size_new, Some(oversized.len() as u64));
    assert_eq!(record.totals.unexamined, 1);
    assert!(record.oversized());
}

#[test]
fn a_tight_cap_makes_a_small_file_unexamined_too() {
    // The cap is the knob; this says it is wired to the read and not to a constant.
    let fixture = Fixture::new("normalize-cap");
    let repo = fixture.init();
    let head = fixture.commit_changes(
        &repo,
        "small file, smaller cap",
        &date_at(0),
        &[Change::Add {
            path: "a.txt",
            lines: 10,
        }],
    );
    let tight = Normalizer::new(
        &repo_at(&repo),
        &Unclassified,
        Options {
            limits: Limits {
                blob_memory_cap: 16,
                ..Limits::default()
            },
            ..Options::default()
        },
    )
    .record(head.parse().unwrap())
    .expect("normalize");
    let file = &tight.files[0];
    assert!(matches!(
        file.content,
        Content::NotExamined {
            reason: NotExamined::Oversized { .. }
        }
    ));
    assert!(
        file.size_new.is_some(),
        "the size still comes from the header"
    );
}

#[test]
fn a_mode_only_change_is_a_modification_with_no_line_counts() {
    let fixture = Fixture::new("normalize-mode");
    let repo = fixture.init();
    fixture.commit_changes(
        &repo,
        "a script",
        &date_at(0),
        &[Change::Add {
            path: "run.sh",
            lines: 3,
        }],
    );
    let head = fixture.commit_changes(
        &repo,
        "make it executable",
        &date_at(1),
        &[Change::Exec { path: "run.sh" }],
    );
    let record = normalizer(&repo_at(&repo))
        .record(head.parse().unwrap())
        .expect("normalize");
    let file = &record.files[0];
    assert_eq!(file.change, ChangeKind::Modify);
    assert_eq!(file.mode_old, Some(0o100644));
    assert_eq!(file.mode_new, Some(0o100755));
    assert_eq!(file.added, 0);
    assert_eq!(file.removed, 0);
    assert!(file.hunks.is_empty(), "no content changed, so no hunk lies");
}

#[test]
fn a_merge_contributes_no_file_rows_by_default_and_says_so() {
    let fixture = Fixture::new("normalize-merge");
    let repo = fixture.init();
    fixture.commit_changes(
        &repo,
        "base",
        &date_at(0),
        &[Change::Add {
            path: "a.txt",
            lines: 2,
        }],
    );
    git(&repo, &["checkout", "-q", "-b", "side"]);
    fixture.commit_changes(
        &repo,
        "side: the brought-in change",
        &date_at(1),
        &[Change::Add {
            path: "side.txt",
            lines: 2,
        }],
    );
    git(&repo, &["checkout", "-q", "main"]);
    fixture.commit_changes(
        &repo,
        "main: the change the merge stands for",
        &date_at(2),
        &[Change::Add {
            path: "main-side.txt",
            lines: 1,
        }],
    );
    let head = fixture
        .merge_at(&repo, "side", "Merge branch 'side'", &date_at(3))
        .parse()
        .unwrap();
    let record = normalizer(&repo_at(&repo)).record(head).expect("normalize");
    assert_eq!(record.parents.len(), 2);
    assert!(
        record.files.is_empty(),
        "git log --raw prints nothing for a merge, and neither do we"
    );
    assert!(record.totals.is_empty());
    assert_eq!(
        record.diff_base,
        fathom_ingest::record::DiffBase::MergeSkipped { parents: 2 }
    );

    // The other policy is available, and the record says which one ran.
    let first_parent = Normalizer::new(
        &repo_at(&repo),
        &Unclassified,
        Options {
            merge: MergePolicy::FirstParent,
            ..Options::default()
        },
    )
    .record(head)
    .expect("normalize with first-parent");
    assert_eq!(first_parent.files.len(), 1);
    assert!(matches!(
        first_parent.diff_base,
        fathom_ingest::record::DiffBase::MergeFirstParent { .. }
    ));
}

#[test]
fn the_root_commit_diffes_against_the_empty_tree() {
    let fixture = Fixture::new("normalize-root");
    let repo = fixture.init();
    let head = fixture.commit_changes(
        &repo,
        "the first one",
        &date_at(0),
        &[Change::Add {
            path: "README.md",
            lines: 1,
        }],
    );
    let record = normalizer(&repo_at(&repo))
        .record(head.parse().unwrap())
        .expect("normalize");
    assert!(record.parents.is_empty());
    assert_eq!(record.diff_base, fathom_ingest::record::DiffBase::EmptyTree);
    assert_eq!(record.files[0].change, ChangeKind::Add);
}

#[test]
fn message_facts_trailers_and_utc_offsets_reach_the_record() {
    let fixture = Fixture::new("normalize-message");
    let repo = fixture.init();
    let head = fixture.commit_changes(
        &repo,
        "Add the exporter\n\nA body that mentions Co-authored-by: nobody in prose.\n\n\
         Co-authored-by: Ada Lovelace <ada@example.invalid>\nagent-session: s-9\n",
        &date_at(0),
        &[Change::Add {
            path: "a.txt",
            lines: 1,
        }],
    );
    let record = normalizer(&repo_at(&repo))
        .record(head.parse().unwrap())
        .expect("normalize");
    assert_eq!(record.message.head, "Add the exporter");
    let message = [
        "Add the exporter",
        "",
        "A body that mentions Co-authored-by: nobody in prose.",
        "",
        "Co-authored-by: Ada Lovelace <ada@example.invalid>",
        "agent-session: s-9",
        "",
    ]
    .join("\n");
    assert_eq!(record.message.bytes, message.len());
    assert_eq!(record.message.hash.len(), 64, "sha256, hex");
    let trailers: Vec<(&str, &str)> = record.trailers.iter().collect();
    assert_eq!(
        trailers,
        vec![
            ("agent-session", "s-9"),
            ("co-authored-by", "Ada Lovelace <ada@example.invalid>"),
        ]
    );
    // Offsets, not just instants: the fixture pins +00:00, and the record keeps it.
    assert_eq!(record.author.offset_minutes, 0);
    assert_eq!(record.committer.offset_minutes, 0);
    assert_eq!(record.author.name, "Fathom Fixture");
    assert_eq!(record.author.email, "fixture@fathom.invalid");
}

#[test]
fn an_empty_commit_records_nothing_without_losing_anything() {
    let fixture = Fixture::new("normalize-empty");
    let repo = fixture.init();
    fixture.commit_changes(
        &repo,
        "first",
        &date_at(0),
        &[Change::Add {
            path: "a.txt",
            lines: 1,
        }],
    );
    let head = fixture.empty_commit(&repo, "nothing changed", &date_at(1));
    let record = normalizer(&repo_at(&repo))
        .record(head.parse().unwrap())
        .expect("normalize");
    assert!(record.files.is_empty());
    assert!(!record.files_truncated);
    assert!(record.totals.is_empty());
    assert_eq!(record.totals.files, 0);
}

#[test]
fn every_path_is_stamped_unclassified_until_e2_1_arrives() {
    let fixture = Fixture::new("normalize-taxonomy");
    let repo = fixture.init();
    let head = fixture.commit_changes(
        &repo,
        "vendor and source alike",
        &date_at(0),
        &[
            Change::Add {
                path: "vendor/lib.js",
                lines: 1,
            },
            Change::Add {
                path: "src/main.rs",
                lines: 1,
            },
        ],
    );
    let record = normalizer(&repo_at(&repo))
        .record(head.parse().unwrap())
        .expect("normalize");
    assert_eq!(Unclassified.version(), 0);
    for file in &record.files {
        assert_eq!(file.taxonomy.path_class, "unknown");
        assert_eq!(file.taxonomy.taxonomy_version, 0);
        assert!(file.taxonomy.noise.is_unknown());
    }
}
