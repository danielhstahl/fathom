//! The spine-only ranking, measured (`reco-c05`).
//!
//! `reco-glv`'s acceptance step (b) asked a reviewer to "find and read the three riskiest episodes"
//! of the 500-commit fixture, and the shipped build could not answer it. The fixture plants ten
//! candidates; this file feeds the ranking the same ten shapes, expressed as the rows the spine
//! actually writes — paths, added and removed lines, file counts, author kind — and asserts what the
//! ordering gets right, what it deliberately gets wrong, and that every number it prints adds up.
//!
//! Two properties matter more here than the ordering itself:
//!
//! * **The arithmetic is the explanation.** `score == sum(components.points)`, and every component
//!   prints a detail naming the paths and counts behind it. A reviewer who cannot check a number has
//!   been handed a recommendation instead (ADR-001 §7.9).
//! * **The ceiling is part of the answer.** Two of the ten candidates are invisible to a spine
//!   ranking by construction — the environment variable that is declared and never read, and the new
//!   import cycle — and the test named for it pins that the ranking says so out loud rather than
//!   letting the list look complete.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_analyze::risk::{
    BLIND_SPOTS, COMPONENTS, FileChange, MODEL, Risk, Signals, Surface, classify, rank,
};

/// Rank one episode described the way the store's rows describe it: `(path, added, removed)`.
fn ranked(files: &[(&str, u64, u64)], author: &str) -> Risk {
    let changes: Vec<FileChange<'_>> = files
        .iter()
        .map(|(path, add, del)| FileChange {
            path,
            add: *add,
            del: *del,
            binary: path.ends_with(".png") || path.ends_with(".jpg"),
            path_class: "unknown",
            noise_class: "unknown",
        })
        .collect();
    let signals = Signals {
        files: &changes,
        n_add: files.iter().map(|(_, add, _)| *add).sum(),
        n_del: files.iter().map(|(_, _, del)| *del).sum(),
        n_files: u64::try_from(files.len()).unwrap_or(u64::MAX),
        n_binary: u64::try_from(
            files
                .iter()
                .filter(|(path, _, _)| path.ends_with(".png"))
                .count(),
        )
        .unwrap_or(0),
        n_commits: 1,
        author,
        partial_files: false,
    };
    rank(&signals)
}

/// The ten candidates `tools/agent_fixture.py` plants, as closely as the spine can see them, plus
/// the control the ticket names: the three-line documentation edit.
fn candidates() -> Vec<(&'static str, Risk)> {
    vec![
        (
            "destructive-migration",
            ranked(
                &[("db/migrations/0038_drop_email_backup.sql", 5, 0)],
                "agent",
            ),
        ),
        (
            "ci-secret-and-permission-escalation",
            ranked(
                &[
                    (".github/workflows/release.yml", 4, 1),
                    (".github/workflows/ci.yml", 2, 0),
                ],
                "agent",
            ),
        ),
        (
            "dependency-bump-storm",
            ranked(&[("uv.lock", 38, 38), ("pyproject.toml", 2, 2)], "agent"),
        ),
        (
            "test-deletion-behind-a-big-turn",
            ranked(
                &[
                    ("src/harbour/pricing.py", 520, 40),
                    ("tests/test_pricing.py", 8, 380),
                ],
                "agent",
            ),
        ),
        (
            "scope-creep",
            ranked(
                &[
                    ("src/harbour/api.py", 30, 10),
                    ("src/harbour/models.py", 30, 10),
                    ("src/harbour/store.py", 30, 10),
                    ("src/harbour/auth.py", 30, 10),
                    ("src/harbour/pricing.py", 30, 10),
                    ("src/harbour/worker.py", 30, 10),
                    ("tests/test_api.py", 20, 5),
                    ("web/src/checkout.ts", 40, 12),
                    ("web/src/booking.tsx", 40, 12),
                    ("docs/adr/0004-booking.md", 24, 4),
                    ("Dockerfile", 3, 0),
                    (".env.example", 2, 0),
                ],
                "agent",
            ),
        ),
        (
            "public-api-removal",
            ranked(
                &[
                    ("src/harbour/api.py", 4, 60),
                    ("web/src/booking.tsx", 18, 6),
                ],
                "agent",
            ),
        ),
        (
            "env-declared-never-read",
            ranked(&[(".env.example", 1, 0)], "agent"),
        ),
        (
            "new-import-cycle",
            ranked(
                &[
                    ("src/harbour/pricing.py", 6, 0),
                    ("src/harbour/reporting.py", 6, 0),
                ],
                "agent",
            ),
        ),
        (
            "generated-churn-and-binary",
            ranked(
                &[
                    ("src/generated/api_pb2.py", 1100, 0),
                    ("assets/map.png", 0, 0),
                ],
                "agent",
            ),
        ),
        (
            "one-enormous-agent-turn",
            ranked(
                &[
                    ("src/harbour/store.py", 900, 120),
                    ("src/harbour/api.py", 300, 40),
                    ("tests/test_store.py", 120, 180),
                ],
                "agent",
            ),
        ),
        (
            "docs-only-control",
            ranked(&[("docs/README.md", 3, 0)], "human"),
        ),
    ]
}

/// The points one component contributed, or 0 when it did not fire.
fn points(risk: &Risk, wanted: &str) -> u64 {
    risk.components
        .iter()
        .find(|component| component.name == wanted)
        .map_or(0, |component| component.points)
}

#[test]
fn every_component_adds_up_to_the_score() {
    for (name, risk) in candidates() {
        let sum: u64 = risk
            .components
            .iter()
            .map(|component| component.points)
            .sum();
        assert_eq!(
            u64::from(risk.score),
            sum.min(100),
            "{name}: {} does not equal its components {:?}",
            risk.score,
            risk.components
        );
        for component in &risk.components {
            assert!(
                component.points <= component.cap,
                "{name}: {} contributes {} over its cap {}",
                component.name,
                component.points,
                component.cap
            );
            assert!(
                COMPONENTS
                    .iter()
                    .any(|(known, cap)| *known == component.name && *cap == component.cap),
                "{name}: component {} is not in COMPONENTS",
                component.name
            );
        }
        assert!(
            risk.score <= 100,
            "{name}: a score over 100 is not the scale"
        );
    }
}

#[test]
fn every_reason_names_something_the_reviewer_can_check() {
    for (name, risk) in candidates() {
        assert!(
            !risk.components.is_empty(),
            "{name}: a ranking with no components cannot be checked"
        );
        for component in &risk.components {
            assert!(
                !component.detail.is_empty(),
                "{name}: {} has no reason printed",
                component.name
            );
        }
        // The surfaces reason quotes the file that earned the surface, never the surface name again:
        // "surfaces: ci" explains nothing, "ci.yml — a workflow change moves with the artefact" is a
        // sentence the reviewer can check against the diff. The em dash is the seam between the two.
        let surfaces = risk
            .components
            .iter()
            .find(|component| component.name == "surfaces")
            .unwrap_or_else(|| panic!("{name}: no surfaces component"));
        assert!(
            surfaces.detail.contains(" — "),
            "{name}: the surfaces reason names no file: {}",
            surfaces.detail
        );
    }
}

#[test]
fn the_kind_of_file_beats_the_amount_of_file() {
    // The ticket's own complaint: "the three-line CI permission change is indistinguishable from a
    // three-line docs edit — and that one is the change that must not be missed".
    let ci = ranked(&[(".github/workflows/release.yml", 3, 0)], "agent");
    let docs = ranked(&[("docs/README.md", 3, 0)], "agent");
    assert!(
        ci.score > docs.score + 20,
        "a three-line CI change ({}) must not rank beside a three-line docs change ({})",
        ci.score,
        docs.score
    );
    assert_eq!(
        ci.badge(),
        "ci",
        "and the row has to say which surface it is"
    );
    assert_eq!(docs.badge(), "docs");

    let migration = ranked(
        &[("db/migrations/0038_drop_email_backup.sql", 3, 0)],
        "agent",
    );
    assert!(
        migration.score > docs.score + 20,
        "a three-line migration ({}) against a docs change ({})",
        migration.score,
        docs.score
    );
}

#[test]
fn a_test_surface_that_shrank_while_source_grew_is_a_reason_of_its_own() {
    // The fixture's `test-deletion-behind-a-big-turn`: 380 lines of tests deleted behind a 520-line
    // rewrite. The aggregate `n_del` the rail prints cannot tell this from a refactor.
    let risk = ranked(
        &[
            ("src/harbour/pricing.py", 520, 40),
            ("tests/test_pricing.py", 8, 380),
        ],
        "agent",
    );
    assert!(
        points(&risk, "deletions") >= 8,
        "the test shrink is worth its own points: {:?}",
        risk.components
    );
    let detail = risk
        .components
        .iter()
        .find(|component| component.name == "deletions")
        .expect("the deletions component")
        .detail
        .clone();
    assert!(
        detail.contains("test surface shrank"),
        "and says so: {detail}"
    );
}

#[test]
fn generated_and_lockfile_churn_is_discounted_rather_than_dropped() {
    // 1 100 lines of regenerated protobuf is a lot of lines and very little review.
    let generated = ranked(&[("src/generated/api_pb2.py", 1100, 0)], "agent");
    let hand = ranked(&[("src/harbour/store.py", 1100, 0)], "agent");
    assert!(
        points(&generated, "size") < points(&hand, "size"),
        "the same churn is worth less when it is generated: {} against {}",
        points(&generated, "size"),
        points(&hand, "size")
    );
    assert!(
        points(&generated, "size") > 0,
        "discounted, not deleted — noise is a view (e2.2), never a removal"
    );

    // A lockfile storm still ranks on the dependency surface: it is an operator action.
    let storm = ranked(&[("uv.lock", 38, 38)], "agent");
    assert!(
        storm.badge().contains("dependency"),
        "a lockfile change is a dependency surface: {}",
        storm.badge()
    );
}

#[test]
fn the_two_candidates_a_spine_cannot_see_are_stated_not_implied() {
    // `env-declared-never-read` and `new-import-cycle` are the fixture's own plantings and this
    // ranking cannot rank them: reading an env var and following an import are analysis, not spine.
    // What the ranking owes the reviewer instead is the ceiling, printed with every score.
    assert!(
        BLIND_SPOTS
            .iter()
            .any(|spot| spot.contains("environment variable")),
        "the declared-and-never-read case has to be named where a reviewer will read it"
    );
    assert!(
        BLIND_SPOTS.iter().any(|spot| spot.contains("import cycle")),
        "and so does the cycle"
    );
    assert!(
        BLIND_SPOTS.iter().any(|spot| spot.contains("rolled back")),
        "and the migration whose down-path is missing, which is the acceptance fixture's first plant"
    );
}

#[test]
fn the_same_input_always_ranks_the_same_way() {
    // ADR-001 §7.4: identical input, identical bytes — and a rank that drifted between two runs
    // would invalidate the reasons printed under it.
    let a: Vec<u8> = candidates().iter().map(|(_, risk)| risk.score).collect();
    let b: Vec<u8> = candidates().iter().map(|(_, risk)| risk.score).collect();
    assert_eq!(a, b, "two runs over the same rows disagree");
}

#[test]
fn the_vector_prints_the_arithmetic_and_the_badge_prints_the_surfaces() {
    let risk = ranked(
        &[
            ("db/migrations/0001_init.sql", 40, 10),
            ("Dockerfile", 3, 1),
        ],
        "agent",
    );
    let vector = risk.vector();
    assert!(
        vector.starts_with(MODEL),
        "the ranking names itself: {vector}"
    );
    assert!(
        vector.contains("surfaces") && vector.contains(&risk.score.to_string()),
        "and prints the sum: {vector}"
    );
    assert_eq!(
        risk.badge(),
        "migration+container",
        "surfaces come out most consequential first"
    );
    assert_eq!(
        risk.surfaces.first(),
        Some(&Surface::Migration),
        "and the top one is the one the row is about"
    );
    let surfaces = risk
        .components
        .iter()
        .find(|component| component.name == "surfaces")
        .expect("the surfaces component");
    assert!(
        surfaces
            .detail
            .starts_with("db/migrations/0001_init.sql — a schema migration"),
        "and the reason quotes the file that earned it, not the badge: {}",
        surfaces.detail
    );
    assert!(
        surfaces.detail.contains("also touched: container"),
        "and lists the rest: {}",
        surfaces.detail
    );
}

#[test]
fn classification_follows_the_stored_taxonomy_when_the_index_has_one() {
    // Every index this build's indexer writes says `unknown` in `path_class`, which is why the path
    // table exists. When the e0.9 taxonomy is wired in, the stored answer wins — except for
    // `config`, which is too coarse to rank on.
    assert_eq!(
        classify("deploy/handlers.go", "source", "informative"),
        Surface::Source
    );
    assert_eq!(
        classify("db/migrations/0002.sql", "migration", "informative"),
        Surface::Migration
    );
    assert_eq!(
        classify("web/app.tsx", "unknown", "unknown"),
        Surface::Source,
        "the path table answers for an unclassified index"
    );
    assert_eq!(
        classify(".github/workflows/ci.yml", "config", "informative"),
        Surface::Ci,
        "`config` in the taxonomy is CI, container or runtime configuration, and the ranking says which"
    );
    assert_eq!(
        classify("notes.txt", "unknown", "unknown"),
        Surface::Other,
        "an unknown is an unknown, not a guess (docs/concepts/paths.md §1)"
    );
}

#[test]
fn breadth_and_author_are_separate_claimable_points() {
    // Scope creep: twelve files across many surfaces is its own risk, independent of the size of
    // any one of them.
    let creep = ranked(
        &[
            ("src/a.py", 5, 0),
            ("src/b.py", 5, 0),
            ("src/c.py", 5, 0),
            ("src/d.py", 5, 0),
            ("src/e.py", 5, 0),
            ("src/f.py", 5, 0),
            ("tests/test_a.py", 5, 0),
            ("docs/x.md", 5, 0),
            ("web/src/app.tsx", 5, 0),
            ("Dockerfile", 5, 0),
            (".env.example", 5, 0),
            ("uv.lock", 5, 0),
        ],
        "agent",
    );
    assert!(points(&creep, "breadth") > 0, "breadth fired: {creep:?}");
    assert!(creep.badge().split('+').count() >= 3);

    // And the same change written by a person is a different claim than one written by an agent.
    let agent = ranked(&[("src/a.py", 40, 0)], "agent");
    let human = ranked(&[("src/a.py", 40, 0)], "human");
    assert!(
        agent.score > human.score,
        "ADR-001 §7.3 is the whole reason the tool exists: {} vs {}",
        agent.score,
        human.score
    );
}

#[test]
fn an_episode_with_no_files_is_answered_rather_than_crashed() {
    let empty = Signals {
        files: &[],
        n_add: 0,
        n_del: 0,
        n_files: 0,
        n_binary: 0,
        n_commits: 0,
        author: "unknown",
        partial_files: false,
    };
    let risk = rank(&empty);
    assert_eq!(
        risk.score, 6,
        "only the attribution signal fires, and it is 6"
    );
    assert_eq!(risk.vector(), "spine-1 6 = author 6".to_owned());
}

#[test]
fn the_ten_candidates_rank_in_an_order_the_reviewer_can_defend() {
    // Not a claim that the fixture's own list is right — a claim about *this* ordering: the
    // operator-action candidates and the big rewrites are in front, the documentation control is
    // last-but-one, and the two invisible ones sit where a spine can see them, not where the fixture
    // hoped they would be.
    let mut ranked: Vec<(&str, u8)> = candidates()
        .into_iter()
        .map(|(name, risk)| (name, risk.score))
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    let order: Vec<&str> = ranked.iter().map(|(name, _)| *name).collect();
    let position = |wanted: &str| order.iter().position(|name| *name == wanted);

    let docs = position("docs-only-control");
    assert!(position("ci-secret-and-permission-escalation") < docs);
    assert!(position("destructive-migration") < docs);
    assert!(position("test-deletion-behind-a-big-turn") < docs);
    assert!(position("scope-creep") < docs);
    assert!(position("one-enormous-agent-turn") < docs);
    // The env var is a *surface*, so it outranks an unremarkable docs change even though the
    // declaration-never-read half of it is invisible. The cycle looks like two small source edits and
    // ranks where two small source edits rank — above a documentation fix, below everything with a
    // surface on it. Both are stated in `blind_spots()`, which the TUI prints under every explanation.
    assert!(position("env-declared-never-read") < docs);
    assert!(position("new-import-cycle") < docs);
    assert!(position("new-import-cycle") > position("public-api-removal"));
}
