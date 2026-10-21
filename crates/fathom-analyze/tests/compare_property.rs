//! The comparison's one hard claim, proved rather than sampled (`e5.3`, `reco-ifo`).
//!
//! The ticket's first acceptance criterion is a sentence about an algebra: *"comparison of two non-adjacent
//! episodes equals the composed per-episode deltas"*. That is not a fixture assertion, because a fixture
//! asserts it for one shape of data and the claim is about every shape. So this file states it twice:
//!
//! * **by enumeration**, on hand-built pairs whose shapes are the ones a reviewer would notice — a file
//!   added then deleted, a finding raised three times, a section that could not answer in the middle
//!   episode — because a property that fails on those is a property nobody should ship; and
//! * **by `proptest`**, over random spans of one to eight episodes cut at every split point, which is the
//!   only way to find the case where a path touched by four of seven episodes merges differently when the
//!   range is walked in twos.
//!
//! The property under test is [`fathom_analyze::compare::Delta::merged`]'s associativity, and what makes it
//! a property worth having is that the merge is *canonical*: files sorted by path, findings sorted by
//! `(kind, subject)`, gaps sorted, figure labels sorted, and every "the merge had to do something unusual"
//! sentence written from the finished composition instead of from the moment a sub-merge noticed it. A
//! merge that is merely correct for one walk order and merely *similar* for another is the bug this file
//! exists to keep out.
//!
//! The delta documents are real [`fathom_analyze::insight::EpisodeDelta`] documents, built by
//! [`fathom_analyze::insight::for_rows`] from rows this file writes — not from a hand-drawn model of what a
//! delta looks like. A comparison composed from invented documents would prove the algebra and nothing
//! about the product.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_analyze::compare::{
    self, Comparison, Contribution, Delta, Endpoint, Lineage, Spec, VALUE_CAP,
};
use fathom_analyze::insight::{self, Evidence, Finding, Gap, SectionId};
use fathom_store::episode::{EpisodeFile, EpisodeRow, ReviewState};
use proptest::prelude::*;

/// The paths the random spans draw from. Small on purpose: the interesting cases are overlaps, and a pool
/// of sixty paths spends its cases on episodes that cannot collide.
const POOL: &[&str] = &[
    "src/store.rs",
    "src/core.rs",
    "src/api/handlers.rs",
    "tests/store.rs",
    "docs/adr/004.md",
    "Cargo.lock",
];

const CHANGES: &[&str] = &["added", "modified", "deleted", "renamed", "mode-only"];

/// One episode, told as the rows the index would have stored.
#[derive(Debug, Clone)]
struct Row {
    /// Position in the lineage, which is what makes a span a span.
    seq: i64,
    /// The paths this episode touched, with the change kind and the line counts.
    files: Vec<(String, String, i64, i64)>,
    /// How many findings it raised, cycling through the kinds the detectors actually produce.
    findings: usize,
    /// Whether the episode could not answer one section — the state a real repository is always in
    /// somewhere.
    gap: bool,
}

fn row_strategy() -> impl Strategy<Value = Row> {
    (0usize..POOL.len(), 0usize..4, 0usize..3, any::<bool>()).prop_map(
        |(first, width, findings, gap)| {
            let files = (0..=width)
                .map(|offset| {
                    let path = POOL[(first + offset) % POOL.len()].to_owned();
                    let change = CHANGES[(first + offset) % CHANGES.len()].to_owned();
                    (
                        path,
                        change,
                        1 + ((first + offset) % 7) as i64,
                        (offset % 5) as i64,
                    )
                })
                .collect();
            Row {
                seq: 0,
                files,
                findings,
                gap,
            }
        },
    )
}

/// Give the span its ordinals, oldest first — the order [`Comparison::compose`] is promised.
fn number(rows: &mut [Row]) {
    for (index, row) in rows.iter_mut().enumerate() {
        row.seq = index as i64 + 1;
    }
}

/// The store row for one fixture episode: enough identity for the document to be about something.
fn episode_row(item: &Row) -> EpisodeRow {
    let id = format!("{:0>64x}", item.seq);
    EpisodeRow {
        id: id.clone(),
        fingerprint: format!("fp-{}", item.seq),
        fallback_fingerprint: format!("ff-{}", item.seq),
        shape_fingerprint: format!("sf-{}", item.seq),
        kind: "committed".to_owned(),
        lineage_ref: "refs/heads/main".to_owned(),
        lineage_seq: item.seq,
        created_utc: 1_760_000_000_000 + item.seq * 3_600_000,
        created_source: "commit".to_owned(),
        boundary_rule: "single-author".to_owned(),
        author_kind: "agent".to_owned(),
        author_name: "Dana Stahl".to_owned(),
        author_email: "dana@example.com".to_owned(),
        first_oid: format!("{}a", &id[..40]),
        last_oid: format!("{}b", &id[..40]),
        members: vec![format!("{}m", &id[..40])],
        first_utc: 1_760_000_000_000 + item.seq * 3_600_000,
        last_utc: 1_760_000_000_000 + item.seq * 3_600_000 + 60_000,
        summary: format!("episode {}", item.seq),
        ..EpisodeRow::default()
    }
}

/// The file rows, in the store's summed-rollup shape.
fn episode_files(item: &Row) -> Vec<EpisodeFile> {
    let mut files: Vec<EpisodeFile> = item
        .files
        .iter()
        .map(|(path, _change, add, del)| EpisodeFile {
            path: path.clone(),
            add: *add,
            del: *del,
            commits: 1,
            binary: false,
            path_class: "source".to_owned(),
            noise_class: "source".to_owned(),
        })
        .collect();
    files.sort_by(|left, right| left.path.cmp(&right.path));
    files
}

/// The findings, in the delta model's own shape.
fn findings_for(item: &Row) -> Vec<Finding> {
    (0..item.findings)
        .map(|index| Finding {
            kind: match index % 3 {
                0 => "new-cycle",
                1 => "test-deleted-without-coverage",
                _ => "todo-debt",
            }
            .to_owned(),
            subject: format!("store→core{}", index % 2),
            message: "a new import closes a cycle between the store and the core".to_owned(),
            severity: if index % 2 == 0 { "high" } else { "note" }.to_owned(),
            evidence: Evidence::owner("e4.3"),
            owner: "e4.3".to_owned(),
        })
        .collect()
}

/// One fixture episode as a comparison contribution: the document `fathom analyze` would print, plus the
/// findings its owner would lend.
fn contribution(item: &Row) -> Contribution {
    let mut delta = insight::for_rows(
        &episode_row(item),
        &episode_files(item),
        ReviewState::Unread,
        false,
    );
    if item.gap {
        // Force a gap into the tests section, which is the section `e3.x` has not filled in every
        // repository: the point is that a hole in one episode is a hole in the range.
        let gap = Gap {
            section: SectionId::Tests.as_str().to_owned(),
            short: "test surface not measured".to_owned(),
            reason: "the test-surface analyzer has not run for this episode".to_owned(),
            owner: "e3.4".to_owned(),
            remedy: Some("fathom analyze <episode>".to_owned()),
        };
        let mut carried = false;
        for section in &mut delta.sections {
            if section.id == SectionId::Tests.as_str() {
                section.gap = Some(gap.clone());
                carried = true;
            }
        }
        if !carried {
            delta.sections.push(insight::Section {
                id: SectionId::Tests.as_str().to_owned(),
                title: "Tests".to_owned(),
                question: "What is tested after this change?".to_owned(),
                owner: "e3.4".to_owned(),
                state: insight::STATE_PARTIAL.to_owned(),
                figures: Vec::new(),
                remainder: None,
                gap: Some(gap.clone()),
            });
        }
        delta.gaps.push(gap);
    }
    Contribution {
        delta,
        findings: findings_for(item),
    }
}

fn contributions(items: &[Row]) -> Vec<Contribution> {
    items.iter().map(contribution).collect()
}

fn endpoint(spec: &str, seq: i64, lineage: &str) -> Endpoint {
    Endpoint {
        spec: spec.to_owned(),
        episode: format!("{spec:0<64}"),
        summary: format!("episode {seq}"),
        lineage: lineage.to_owned(),
        lineage_seq: seq,
        utc: 1_760_000_000 + seq,
    }
}

// ---------------------------------------------------------------------------
// the property
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Cutting the span anywhere and merging the halves gives the whole range's composition, byte for byte.
    #[test]
    fn cutting_the_range_anywhere_composes_to_the_same_answer(mut rows in prop::collection::vec(row_strategy(), 1..8)) {
        number(&mut rows);
        let items = contributions(&rows);
        let whole = Delta::compose(&items);
        for split in 0..=items.len() {
            let halves = Delta::compose(&items[..split]).merged(&Delta::compose(&items[split..]));
            prop_assert_eq!(
                serde_json::to_string(&whole).unwrap(),
                serde_json::to_string(&halves).unwrap(),
                "split {} of {}",
                split,
                items.len()
            );
        }
    }

    /// Three ways of grouping the same walk: whole, in halves, and one episode at a time.
    #[test]
    fn grouping_the_walk_differently_changes_nothing(mut rows in prop::collection::vec(row_strategy(), 2..7)) {
        number(&mut rows);
        let items = contributions(&rows);
        let whole = Delta::compose(&items);
        let mut one_at_a_time = Delta::default();
        for item in &items {
            one_at_a_time = one_at_a_time.merged(&Delta::of(item));
        }
        prop_assert_eq!(
            serde_json::to_string(&whole).unwrap(),
            serde_json::to_string(&one_at_a_time).unwrap()
        );
    }
}

#[test]
fn the_property_holds_on_the_shape_a_reviewer_actually_returns_to() {
    // The ticket's own story: "the point I last looked" is three episodes back, and the middle episode
    // touched two of the same files. Composing the range must equal composing each episode in turn.
    let mut rows = vec![
        Row {
            seq: 0,
            files: vec![
                ("src/store.rs".to_owned(), "modified".to_owned(), 40, 4),
                ("src/core.rs".to_owned(), "modified".to_owned(), 10, 0),
            ],
            findings: 2,
            gap: false,
        },
        Row {
            seq: 0,
            files: vec![("src/store.rs".to_owned(), "modified".to_owned(), 5, 20)],
            findings: 2,
            gap: true,
        },
        Row {
            seq: 0,
            files: vec![("src/api/handlers.rs".to_owned(), "added".to_owned(), 80, 0)],
            findings: 0,
            gap: false,
        },
    ];
    number(&mut rows);
    let items = contributions(&rows);
    let whole = Delta::compose(&items);
    let prefix = Delta::compose(&items[..1]);
    let rest = Delta::compose(&items[1..]);
    assert_eq!(whole, prefix.merged(&rest));
    assert_eq!(whole.episodes, 3);
    // `src/store.rs` was touched by two of the three, and the composition says so with the summed churn.
    let store = whole
        .files
        .iter()
        .find(|file| file.path == "src/store.rs")
        .expect("the shared path is in the composition");
    assert_eq!(store.add, 45);
    assert_eq!(store.del, 24);
    assert!(store.commits > 1, "two episodes touched it");
}

#[test]
fn a_file_added_then_deleted_comes_out_deleted() {
    // The worst wrong answer a comparison can give is a file that is not there, so the last recorded
    // change kind wins over the first one — and an `unrecorded` row (which is what a rollup read says for
    // every file) may not overwrite a kind that *was* recorded.
    let mut added = contribution(&Row {
        seq: 1,
        files: vec![("src/gone.rs".to_owned(), "added".to_owned(), 12, 0)],
        findings: 0,
        gap: false,
    });
    let mut deleted = contribution(&Row {
        seq: 2,
        files: vec![("src/gone.rs".to_owned(), "deleted".to_owned(), 0, 12)],
        findings: 0,
        gap: false,
    });
    for (item, change) in [(&mut added, "added"), (&mut deleted, "deleted")] {
        for file in &mut item.delta.files {
            file.change = change.to_owned();
            file.basis = "full".to_owned();
        }
    }
    let range = Delta::of(&added).merged(&Delta::of(&deleted));
    let file = &range.files[0];
    assert_eq!(file.path, "src/gone.rs");
    assert_eq!(
        file.change, "deleted",
        "the tree's state at the right endpoint"
    );
    assert_eq!(file.add, 12);
    assert_eq!(file.del, 12);
}

#[test]
fn one_findings_kind_seen_thrice_is_one_finding_seen_three_times() {
    let item = Row {
        seq: 0,
        files: vec![("src/store.rs".to_owned(), "modified".to_owned(), 3, 0)],
        findings: 3,
        gap: false,
    };
    let items = contributions(&[
        Row {
            seq: 0,
            ..item.clone()
        },
        Row {
            seq: 0,
            ..item.clone()
        },
        Row { seq: 0, ..item },
    ]);
    let range = Delta::compose(&items);
    assert_eq!(
        range.findings.len(),
        3,
        "three kinds, each raised by every episode"
    );
    let counts: Vec<usize> = range.finding_seen.values().copied().collect();
    assert_eq!(
        counts,
        vec![3, 3, 3],
        "and the count is the fact that carries"
    );
}

#[test]
fn a_hole_in_one_episode_is_a_hole_in_the_range() {
    let items = contributions(&[
        Row {
            seq: 0,
            files: vec![("src/a.rs".to_owned(), "modified".to_owned(), 1, 0)],
            findings: 0,
            gap: false,
        },
        Row {
            seq: 0,
            files: vec![("src/b.rs".to_owned(), "modified".to_owned(), 1, 0)],
            findings: 0,
            gap: true,
        },
    ]);
    let range = Delta::compose(&items);
    assert!(
        range
            .gaps
            .iter()
            .any(|gap| gap.section == SectionId::Tests.as_str()),
        "the range could not answer what its middle episode could not"
    );
    let tests = range
        .sections
        .iter()
        .find(|section| section.id == SectionId::Tests.as_str())
        .expect("the section is in the composition");
    assert!(tests.gap.is_some());
}

// ---------------------------------------------------------------------------
// endpoints, lineage and the shareable spec
// ---------------------------------------------------------------------------

fn comparison(items: &[Row], from: Endpoint, to: Endpoint) -> Comparison {
    Comparison::compose(from, to, &contributions(items))
}

#[test]
fn two_endpoints_on_one_branch_are_named_as_one_walk() {
    let items = vec![Row {
        seq: 0,
        files: vec![("src/a.rs".to_owned(), "modified".to_owned(), 2, 0)],
        findings: 0,
        gap: false,
    }];
    let made = comparison(
        &items,
        endpoint("a1b2c3", 4, "main"),
        endpoint("d4e5f6", 9, "main"),
    );
    assert_eq!(
        made.lineage,
        Lineage::Same {
            lineage: "main".into()
        }
    );
    assert_eq!(made.from.label(), "main@a1b2c300");
    assert!(
        made.notes
            .iter()
            .any(|note| note.contains("both endpoints are on `main`")),
        "{:?}",
        made.notes
    );
    assert!(made.header().contains("main@a1b2c300..main@d4e5f600"));
}

#[test]
fn a_cross_branch_pair_says_it_is_not_one_history() {
    let items = vec![Row {
        seq: 0,
        files: vec![("src/a.rs".to_owned(), "modified".to_owned(), 2, 0)],
        findings: 0,
        gap: false,
    }];
    let made = comparison(
        &items,
        endpoint("1a2b3c", 3, "main"),
        endpoint("9f8e7d", 7, "feat/x"),
    );
    assert_eq!(
        made.lineage,
        Lineage::Across {
            from: "main".into(),
            to: "feat/x".into()
        }
    );
    assert!(
        made.notes
            .iter()
            .any(|note| note.contains("different lineages") && note.contains("temporal")),
        "{:?}",
        made.notes
    );
    // The header carries both labels, which is the whole point of labelling: a pasted line that says
    // `9f8e7d` alone reads like one branch's history to whoever pastes it next.
    assert!(made.header().contains("main@1a2b3c00..feat/x@9f8e7d00"));
}

#[test]
fn a_backwards_range_says_so() {
    let items = vec![Row {
        seq: 0,
        files: vec![("src/a.rs".to_owned(), "modified".to_owned(), 2, 0)],
        findings: 0,
        gap: false,
    }];
    let made = comparison(
        &items,
        endpoint("newer", 12, "main"),
        endpoint("older", 3, "main"),
    );
    assert!(
        made.notes
            .iter()
            .any(|note| note.contains("reads backwards")),
        "{:?}",
        made.notes
    );
}

#[test]
fn a_range_with_no_findings_says_nothing_was_evaluated() {
    // "No findings" and "nobody looked" are different claims and the empty list cannot tell them apart, so
    // the note has to.
    let items = vec![Row {
        seq: 0,
        files: vec![("src/a.rs".to_owned(), "modified".to_owned(), 2, 0)],
        findings: 0,
        gap: false,
    }];
    let made = comparison(&items, endpoint("a", 1, "main"), endpoint("b", 2, "main"));
    assert!(
        made.notes
            .iter()
            .any(|note| note.contains("the detectors were not run")),
        "{:?}",
        made.notes
    );
}

#[test]
fn a_unioned_figure_prints_every_value_it_took_and_the_comparison_says_why() {
    // Two episodes whose `what` section says different things: the merged row lists both, and the note
    // says they were unioned rather than merged into one number that means nothing.
    let mut first = contribution(&Row {
        seq: 1,
        files: vec![("src/a.rs".to_owned(), "modified".to_owned(), 2, 0)],
        findings: 0,
        gap: false,
    });
    let mut second = contribution(&Row {
        seq: 2,
        files: vec![("src/a.rs".to_owned(), "modified".to_owned(), 900, 0)],
        findings: 0,
        gap: false,
    });
    for (item, value) in [
        (&mut first, "two lines"),
        (&mut second, "nine hundred lines"),
    ] {
        let section = &mut item
            .delta
            .sections
            .iter_mut()
            .find(|section| section.id == SectionId::What.as_str())
            .expect("every document answers `what`");
        for figure in &mut section.figures {
            figure.value = value.to_owned();
        }
    }
    let range = Delta::of(&first).merged(&Delta::of(&second));
    let section = range
        .sections
        .iter()
        .find(|section| section.id == SectionId::What.as_str())
        .expect("the section survived the merge");
    let text = serde_json::to_string(&section.figures).unwrap();
    assert!(
        text.contains("two lines") && text.contains("nine hundred lines"),
        "{text}"
    );
    assert!(!range.union_parts.is_empty());

    let made = Comparison::compose(
        endpoint("a", 1, "main"),
        endpoint("b", 2, "main"),
        &[first, second],
    );
    assert!(
        made.notes
            .iter()
            .any(|note| note.contains("carried different values")),
        "{:?}",
        made.notes
    );
}

#[test]
fn a_union_longer_than_the_cap_says_how_many_it_left_out() {
    // Checked at compile time on purpose: a fixture that builds `VALUE_CAP + 3` items is only a test of
    // truncation if the cap is a cap, and a runtime assert would pass a build where it is 1.
    const _: () = assert!(VALUE_CAP >= 2, "the cap exists to bound a line, not a page");
    let mut items = Vec::new();
    for index in 0..(VALUE_CAP + 3) {
        let mut item = contribution(&Row {
            seq: index as i64,
            files: vec![("src/a.rs".to_owned(), "modified".to_owned(), 1, 0)],
            findings: 0,
            gap: false,
        });
        let section = item
            .delta
            .sections
            .iter_mut()
            .find(|section| section.id == SectionId::What.as_str())
            .expect("every document answers `what`");
        for figure in &mut section.figures {
            figure.value = format!("v{index}");
        }
        items.push(item);
    }
    let made = Comparison::compose(endpoint("a", 1, "main"), endpoint("b", 9, "main"), &items);
    let printed = made.render().join("\n");
    assert!(
        printed.contains(&format!("(+{} more)", VALUE_CAP + 3 - VALUE_CAP)),
        "{printed}"
    );
}

// ---------------------------------------------------------------------------
// the spec and the saved comparisons
// ---------------------------------------------------------------------------

#[test]
fn the_spec_is_two_dots_and_the_shape_a_reviewer_pastes() {
    let spec = Spec::parse("main@1a2b3c..feat/x@9f8e7d").unwrap();
    assert_eq!(spec.from, "main@1a2b3c");
    assert_eq!(spec.to, "feat/x@9f8e7d");
    assert_eq!(spec.display(), "main@1a2b3c..feat/x@9f8e7d");
    // Whitespace is a paste artefact, not part of the spec.
    assert_eq!(Spec::parse(" a .. b ").unwrap().display(), "a..b");
}

#[test]
fn a_spec_that_is_not_a_range_is_refused_in_the_shape_of_a_range() {
    for bad in ["a1b2c3", "a...b", "..b", "a..", "a..b..c"] {
        let why = Spec::parse(bad).expect_err("`{bad}` is not a range");
        assert!(
            why.contains("is not a comparison"),
            "`{bad}` refused with: {why}"
        );
    }
}

#[test]
fn a_saved_comparison_is_a_session_row_and_comes_back_by_name() {
    let saved = compare::Saved {
        name: "since-the-refactor".to_owned(),
        spec: "77aa0c..e8f1a2".to_owned(),
        note: "the week the store moved".to_owned(),
        created_utc: None,
    };
    let mut entries = vec![("tui.view".to_owned(), "timeline".to_owned())];
    entries.push(compare::saved_entry(&saved));
    let read = compare::saved_from_session(&entries);
    assert_eq!(read, vec![saved], "the note survives the round trip");
    // And the palette's listing is a prefix scan over the same rows, which is why the value is a spec and
    // not a blob: a reviewer can hand-edit it in the store.
    assert_eq!(
        compare::saved_name("tui.compare.saved.since-the-refactor"),
        Some("since-the-refactor")
    );
    assert_eq!(compare::saved_name("tui.compare.saved."), None);
    assert_eq!(compare::saved_name("tui.view"), None);
    // The prefix is not a style choice (`e5.3`, `e5.4`): `fathom_tui::session::owns` decides which rows a
    // run is *given*, and a row outside that namespace is a row the shell never sees — a saved comparison
    // the palette cannot list is a saved comparison that does not exist.
    assert!(
        compare::SAVED_PREFIX.starts_with("tui."),
        "saved comparisons have to live in the namespace the session loader hands the shell: {}",
        compare::SAVED_PREFIX
    );
}

#[test]
fn an_empty_range_is_an_answer_and_says_it_is_empty() {
    let empty = Delta::compose(&[]);
    assert!(empty.is_empty());
    assert_eq!(empty.episodes, 0);
}

#[test]
fn the_contract_id_is_in_the_serialised_form() {
    let items = vec![Row {
        seq: 0,
        files: vec![("src/a.rs".to_owned(), "modified".to_owned(), 2, 0)],
        findings: 0,
        gap: false,
    }];
    let made = comparison(&items, endpoint("a", 1, "main"), endpoint("b", 2, "main"));
    let json = serde_json::to_value(&made).unwrap();
    assert_eq!(json["contract"], compare::CONTRACT);
    assert_eq!(json["from"]["spec"], "a");
    assert!(json["delta"]["episodes"].as_u64().unwrap() >= 1);
}
