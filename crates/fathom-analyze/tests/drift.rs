//! `e4.8` — plan versus execution: the six contradictions, each with both sides linked, plus the bounds
//! and the refusals.
//!
//! Every fixture here is hand-written on purpose. A drift test generated from a real repository would be
//! testing what the corpus happens to contain; these test what each rule *claims*, which is the thing a
//! threshold has to be argued about.
//!
//! A test may unwrap: an absent field here is the finding the rule was supposed to make, and saying so
//! at the call is shorter than saying it in a `main` no one runs.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_analyze::drift::{
    self, Deviation, Episode, Plan, PlanItem, Policy, Report, Rung, Session, SideKind, overlap,
    severity_of, tokens,
};
use fathom_analyze::scope::deviation;

/// The rung a harness hook writes: states the ask in prose, prices it at the prompt rung.
fn hook(items: Vec<PlanItem>, scope: &[&str]) -> Plan {
    let text = items
        .iter()
        .map(|item| item.text.as_str())
        .collect::<Vec<_>>()
        .join("; ");
    Plan {
        items,
        scope: scope.iter().map(|glob| (*glob).to_owned()).collect(),
        text: Some(text),
        rung: Rung {
            source: "hook-prompt".to_owned(),
            confidence: 0.95,
            stated: true,
        },
    }
}

/// The weaker rung: an `Intent:` trailer, self-reported by the agent.
fn trailer(text: &str, items: Vec<PlanItem>) -> Plan {
    Plan {
        items,
        scope: Vec::new(),
        text: Some(text.to_owned()),
        rung: Rung {
            source: "agent-intent-trailer".to_owned(),
            confidence: 0.55,
            stated: true,
        },
    }
}

fn episode(id: &str, summary: &str, touched: &[&str]) -> Episode {
    Episode {
        id: id.to_owned(),
        summary: summary.to_owned(),
        ordinal: 0,
        plan: None,
        touched: touched.iter().map(|path| (*path).to_owned()).collect(),
        tests_touched: Some(false),
        moved_paths: Some(false),
        manifests: Vec::new(),
        signoff: None,
    }
}

/// A session, with the ordinals filled in the way the CLI fills them.
fn session(episodes: Vec<Episode>) -> Session {
    let mut ordered = episodes;
    for (index, episode) in ordered.iter_mut().enumerate() {
        episode.ordinal = index;
    }
    Session {
        id: Some("s-test".to_owned()),
        episodes: ordered,
    }
}

fn kinds(report: &Report) -> Vec<String> {
    report
        .findings
        .iter()
        .map(|finding| finding.kind.clone())
        .collect()
}

// ---------------------------------------------------------------------------------------------
// The six kinds, one fixture each
// ---------------------------------------------------------------------------------------------

#[test]
fn an_open_plan_item_with_nothing_resembling_it_in_the_window_is_a_finding() {
    let mut first = episode("e-1", "tighten the retry budget", &["src/ingest/retry.rs"]);
    first.plan = Some(hook(
        vec![
            PlanItem::open("tighten the retry budget"),
            PlanItem::open("publish the migration guide for operators"),
        ],
        &["src/ingest/**"],
    ));
    let report = drift::analyse(
        &[session(vec![
            first,
            episode("e-2", "retry budget follow-up", &["src/ingest/retry.rs"]),
        ])],
        &Policy::default(),
    );

    let finding = report
        .findings
        .iter()
        .find(|f| f.kind == "plan-unimplemented")
        .expect("the migration guide was never touched");
    assert_eq!(finding.episode, "e-1");
    assert!(finding.message.contains("migration guide"));
    assert!(finding.message.contains("closest match"));
    // Both sides, and both are addresses a reviewer can open.
    assert_eq!(finding.claim.kind, SideKind::Plan);
    assert_eq!(
        finding.claim.quote,
        "publish the migration guide for operators"
    );
    assert!(finding.claim.address.contains("e-1"));
    assert_eq!(finding.evidence.kind, SideKind::Absence);
    assert!(finding.evidence.address.contains("fathom show"));
    assert_eq!(finding.owner, "e4.8");
}

#[test]
fn a_change_that_does_the_work_is_not_drift() {
    // The same plan, with the second episode doing the second item. The rule must find it, or the corpus is
    // measuring nothing.
    let mut first = episode("e-1", "tighten the retry budget", &["src/ingest/retry.rs"]);
    first.plan = Some(hook(
        vec![PlanItem::open("tighten the retry budget")],
        &["src/ingest/**"],
    ));
    let report = drift::analyse(&[session(vec![first])], &Policy::default());
    assert!(
        report.findings.is_empty(),
        "the episode's own change addresses its plan: {:?}",
        report.findings
    );
}

#[test]
fn a_promised_test_that_was_never_written_is_a_finding_and_a_written_one_is_not() {
    let mut planned = episode("e-1", "harden the parser", &["src/parse/mod.rs"]);
    planned.plan = Some(hook(
        vec![PlanItem::open(
            "add regression tests for the truncated frame case",
        )],
        &["src/parse/**"],
    ));
    let no_tests = drift::analyse(&[session(vec![planned.clone()])], &Policy::default());
    let finding = no_tests
        .findings
        .iter()
        .find(|f| f.kind == "test-promised-not-written")
        .expect("a test promise with no test surface touched");
    assert_eq!(finding.severity, "high");
    assert_eq!(finding.claim.kind, SideKind::Plan);
    assert_eq!(finding.evidence.kind, SideKind::Absence);

    let mut written = planned.clone();
    written.touched.push("tests/parse_frames.rs".to_owned());
    written.tests_touched = Some(true);
    let report = drift::analyse(&[session(vec![written])], &Policy::default());
    assert!(
        !kinds(&report).contains(&"test-promised-not-written".to_owned()),
        "a test was written: {:?}",
        report.findings
    );
}

#[test]
fn an_unanswered_test_surface_is_reported_as_a_gap_and_never_as_a_missing_test() {
    // The third acceptance criterion, in its sharpest form: "I cannot tell whether a test was touched"
    // must not become "no test was written".
    let mut planned = episode("e-1", "harden the parser", &["src/parse/mod.rs"]);
    planned.tests_touched = None;
    planned.plan = Some(hook(
        vec![PlanItem::open(
            "add regression tests for the truncated frame case",
        )],
        &["src/parse/**"],
    ));
    let report = drift::analyse(&[session(vec![planned])], &Policy::default());
    assert!(!kinds(&report).contains(&"test-promised-not-written".to_owned()));
    let skip = report
        .unchecked
        .iter()
        .find(|skip| skip.subject == "e-1" && skip.reason.contains("could not say"))
        .expect("the report says which test surface it could not read");
    assert_eq!(skip.owner.as_deref(), Some("e2.9"));
}

#[test]
fn an_intent_with_no_scope_cannot_be_judged_for_deviation_and_says_so() {
    // The other half of the third acceptance criterion: an intent that declared no scope, not an
    // unparseable one.
    let mut planned = episode("e-1", "docs only", &["docs/anything.md"]);
    planned.plan = Some(trailer("docs only", vec![PlanItem::open("docs only")]));
    let report = drift::analyse(&[session(vec![planned])], &Policy::default());
    assert!(
        !kinds(&report).contains(&"out-of-scope-edit".to_owned()),
        "no glob was declared, so nothing is outside it: {:?}",
        report.findings
    );
    assert!(
        report
            .unchecked
            .iter()
            .any(|skip| skip.reason.contains("declared no scope")),
        "and the report says that rather than staying quiet: {:?}",
        report.unchecked
    );
    assert_eq!(
        deviation::<&str, &str>(&[], &["any/path.rs"]),
        Deviation::Unknown,
        "no globs declared is not the same answer as inside"
    );
}

#[test]
fn a_path_outside_the_declared_scope_is_a_finding_with_the_glob_on_one_side() {
    let mut planned = episode(
        "e-1",
        "retry budget",
        &["src/ingest/retry.rs", "src/cli/main.rs"],
    );
    planned.plan = Some(hook(
        vec![PlanItem::open("tighten the retry budget")],
        &["src/ingest/**"],
    ));
    let report = drift::analyse(&[session(vec![planned])], &Policy::default());
    let finding = report
        .findings
        .iter()
        .find(|f| f.kind == "out-of-scope-edit")
        .expect("src/cli/main.rs is outside src/ingest/**");
    assert_eq!(finding.claim.kind, SideKind::Scope);
    assert_eq!(finding.claim.quote, "src/ingest/**");
    assert_eq!(finding.evidence.kind, SideKind::Diff);
    assert_eq!(finding.evidence.quote, "src/cli/main.rs");
}

#[test]
fn a_path_the_plan_named_is_not_out_of_scope_even_when_the_glob_misses_it() {
    let mut planned = episode(
        "e-1",
        "retry budget",
        &["src/ingest/retry.rs", "docs/cli/main.md"],
    );
    planned.plan = Some(hook(
        vec![
            PlanItem::open("tighten the retry budget"),
            PlanItem::open("document the retry budget in docs/cli/main.md"),
        ],
        &["src/ingest/**"],
    ));
    let report = drift::analyse(&[session(vec![planned])], &Policy::default());
    assert!(
        !kinds(&report).contains(&"out-of-scope-edit".to_owned()),
        "the plan accounted for the path: {:?}",
        report.findings
    );
}

#[test]
fn a_dependency_manifest_the_plan_never_mentioned_is_a_finding() {
    let mut planned = episode("e-1", "retry budget", &["src/ingest/retry.rs"]);
    planned
        .manifests
        .push("crates/fathom-analyze/Cargo.toml".to_owned());
    planned
        .touched
        .push("crates/fathom-analyze/Cargo.toml".to_owned());
    planned.plan = Some(hook(
        vec![PlanItem::open("tighten the retry budget")],
        &["src/ingest/**"],
    ));
    let report = drift::analyse(&[session(vec![planned.clone()])], &Policy::default());
    let finding = report
        .findings
        .iter()
        .find(|f| f.kind == "unplanned-dependency")
        .expect("a manifest appeared with nothing about it in the plan");
    assert_eq!(finding.evidence.kind, SideKind::Dependency);
    assert!(finding.message.contains("Cargo.toml"));
    assert!(finding.message.contains("resolved graph"));

    // And the same touch is not a finding when the plan was about the dependency.
    let mut planned_bump = planned;
    planned_bump.plan = Some(hook(
        vec![PlanItem::open("bump the dependency that parses frames")],
        &["src/ingest/**"],
    ));
    let report = drift::analyse(&[session(vec![planned_bump])], &Policy::default());
    assert!(!kinds(&report).contains(&"unplanned-dependency".to_owned()));
}

#[test]
fn a_claimed_refactor_that_touched_its_target_is_not_a_finding_and_one_that_did_not_is() {
    let mut planned = episode("e-1", "refactor the ingest pool", &["src/ingest/mod.rs"]);
    planned.plan = Some(hook(
        vec![PlanItem::open("refactor the retry budget module")],
        &["src/ingest/**"],
    ));
    let report = drift::analyse(&[session(vec![planned.clone()])], &Policy::default());
    let finding = report
        .findings
        .iter()
        .find(|f| f.kind == "refactor-claimed-not-performed")
        .expect("`retry` was never touched and nothing moved");
    assert!(finding.message.contains("`retry`"));

    let mut touched = planned.clone();
    touched.touched.push("src/ingest/retry.rs".to_owned());
    let report = drift::analyse(&[session(vec![touched])], &Policy::default());
    assert!(!kinds(&report).contains(&"refactor-claimed-not-performed".to_owned()));

    // A recorded path move is the other way a structural claim is honoured.
    let mut moved = planned;
    moved.moved_paths = Some(true);
    let report = drift::analyse(&[session(vec![moved])], &Policy::default());
    assert!(!kinds(&report).contains(&"refactor-claimed-not-performed".to_owned()));
}

#[test]
fn a_plan_item_that_reopens_signed_off_work_is_a_finding_citing_the_ledger() {
    let mut planned = episode("e-1", "finish the retry budget", &["src/ingest/retry.rs"]);
    planned.signoff = Some("ok — retry budget is done, ship it".to_owned());
    planned.plan = Some(hook(
        vec![
            PlanItem::done("tighten the retry budget"),
            PlanItem::open("revisit the retry budget after the load test"),
        ],
        &["src/ingest/**"],
    ));
    let report = drift::analyse(&[session(vec![planned])], &Policy::default());
    let finding = report
        .findings
        .iter()
        .find(|f| f.kind == "contradicts-signoff")
        .expect("the plan re-opens what the sign-off closed");
    assert_eq!(finding.evidence.kind, SideKind::Ledger);
    assert!(finding.evidence.address.contains("fathom review"));
    assert_eq!(finding.severity, "high");
    let corroborating = finding.corroborating.as_ref().expect("the done item too");
    assert_eq!(corroborating.kind, SideKind::Plan);
}

// ---------------------------------------------------------------------------------------------
// Both sides, always: the acceptance criterion every finding is held to
// ---------------------------------------------------------------------------------------------

#[test]
fn every_finding_links_both_sides_of_the_contradiction() {
    let mut planned = episode("e-1", "everything at once", &["src/ingest/retry.rs"]);
    planned
        .manifests
        .push("crates/fathom-analyze/Cargo.toml".to_owned());
    planned.touched.push("docs/runbook.md".to_owned());
    planned.signoff = Some("ok — the retry budget is done".to_owned());
    planned.plan = Some(hook(
        vec![
            PlanItem::done("tighten the retry budget"),
            PlanItem::open("revisit the retry budget later"),
            PlanItem::open("write tests for the retry budget"),
            PlanItem::open("extract the connection pool module"),
        ],
        &["src/ingest/**"],
    ));
    let report = drift::analyse(&[session(vec![planned])], &Policy::default());
    assert!(
        report.findings.len() >= 4,
        "one fixture, several kinds: {:?}",
        kinds(&report)
    );
    for finding in &report.findings {
        assert!(!finding.subject.is_empty(), "{finding:?} has no subject");
        assert!(
            !finding.claim.address.is_empty() && !finding.evidence.address.is_empty(),
            "{finding:?} is missing a side"
        );
        assert!(
            finding.claim.address.contains("fathom") || finding.claim.address.starts_with('#'),
            "the claim side must be an address a reviewer opens: {:?}",
            finding.claim
        );
        assert!(
            finding.evidence.quote.chars().count() <= 512,
            "side B is a quotation, not a dump: {:?}",
            finding.evidence
        );
        assert!(
            finding.confidence > 0.0 && finding.confidence <= 0.95,
            "no finding claims certainty: {:?}",
            finding.confidence
        );
        assert!(finding.suppression_key.starts_with("drift:"));
    }
}

#[test]
fn the_findings_arrive_as_the_delta_models_own_findings() {
    // The door every other finding uses: a drift finding is an `insight::Finding` on the way into the
    // delta document, so the narrative and the card cite it through the existing evidence type.
    let mut planned = episode("e-1", "retry budget", &["src/ingest/retry.rs"]);
    planned.plan = Some(hook(
        vec![PlanItem::open("publish the migration guide for operators")],
        &["src/ingest/**"],
    ));
    let report = drift::analyse(&[session(vec![planned])], &Policy::default());
    let insights = report.to_insight();
    assert_eq!(insights.len(), report.findings.len());
    let message = &insights[0].message;
    assert!(message.contains("said:"), "{message}");
    assert!(message.contains("history:"), "{message}");
    assert_eq!(insights[0].owner, "e4.8");
}

// ---------------------------------------------------------------------------------------------
// Bounds, confidence and refusals
// ---------------------------------------------------------------------------------------------

#[test]
fn confidence_follows_the_intent_rung_and_the_rung_is_named() {
    let items = || vec![PlanItem::open("publish the migration guide for operators")];
    let stated = {
        let mut e = episode("e-1", "unrelated subject", &["src/ingest/retry.rs"]);
        e.plan = Some(hook(items(), &["src/ingest/**"]));
        e
    };
    let trailerised = {
        let mut e = episode("e-1", "unrelated subject", &["src/ingest/retry.rs"]);
        e.plan = Some(trailer("unrelated subject", items()));
        e
    };
    let strong = drift::analyse(&[session(vec![stated])], &Policy::default());
    let weak = drift::analyse(&[session(vec![trailerised])], &Policy::default());
    let strong = &strong.findings[0];
    let weak = &weak.findings[0];
    assert_eq!(strong.intent_source, "hook-prompt");
    assert_eq!(weak.intent_source, "agent-intent-trailer");
    assert!(
        weak.confidence < strong.confidence,
        "a self-reported trailer is a weaker claim than a recorded prompt: {} vs {}",
        weak.confidence,
        strong.confidence
    );
    assert!(
        weak.confidence >= 0.30,
        "and still reportable: {}",
        weak.confidence
    );
}

#[test]
fn a_dismissed_contradiction_is_not_reported_and_is_counted() {
    let build = || {
        let mut e = episode("e-1", "retry budget", &["src/ingest/retry.rs"]);
        e.plan = Some(hook(
            vec![PlanItem::open("publish the migration guide for operators")],
            &["src/ingest/**"],
        ));
        session(vec![e])
    };
    let plain = build();
    let first = drift::analyse(std::slice::from_ref(&plain), &Policy::default());
    let key = first.findings[0].suppression_key.clone();

    // A policy is a bag of defaults with one knob turned: the struct-update form says which knob, and
    // says it once (`clippy::field_reassign_with_default`).
    let policy = Policy {
        dismissed: [key.clone()].into_iter().collect(),
        ..Default::default()
    };
    let second = drift::analyse(&[build()], &policy);
    assert!(
        second.findings.is_empty(),
        "a dismissed key must not come back"
    );
    assert_eq!(second.suppressed, vec![key.clone()]);
    assert!(
        second
            .tallies
            .iter()
            .any(|tally| tally.kind == "plan-unimplemented" && tally.dismissed == 1),
        "counted, not silently dropped: {:?}",
        second.tallies
    );
    assert!(second.verdict().contains("dismissed"));

    // The key survives a rewrite of the episode it was found in: it is built from the rule and the item,
    // not from the id an amend changes.
    let mut renamed = build();
    renamed.episodes[0].id = "e-999".to_owned();
    let third = drift::analyse(&[renamed], &policy);
    assert!(
        third.findings.is_empty(),
        "dismissing survives the episode id changing: {:?}",
        third.findings
    );
}

#[test]
fn the_window_and_the_caps_are_real_bounds_and_they_say_so() {
    // An item whose only matching change sits past the window is drift at window 1 and not drift at
    // window 4 — that is what the window means, and the report must show the knob rather than the luck.
    let mut first = episode("e-1", "start", &["src/ingest/a.rs"]);
    first.plan = Some(hook(
        vec![PlanItem::open("retire the legacy frame parser")],
        &["src/ingest/**"],
    ));
    let middle = (0..4).map(|i| {
        episode(
            &format!("m-{i}"),
            "nothing to do here",
            &["src/ingest/b.rs"],
        )
    });
    let last = episode(
        "e-9",
        "retire the legacy frame parser",
        &["src/parse/legacy.rs"],
    );
    let mut episodes = vec![first];
    episodes.extend(middle);
    episodes.push(last);

    let tight = {
        let policy = Policy {
            window: 1,
            ..Default::default()
        };
        drift::analyse(&[session(episodes.clone())], &policy)
    };
    let wide = {
        let policy = Policy {
            window: 8,
            ..Default::default()
        };
        drift::analyse(&[session(episodes.clone())], &policy)
    };
    assert!(
        kinds(&tight).contains(&"plan-unimplemented".to_owned()),
        "the change is out of the window: {:?}",
        kinds(&tight)
    );
    assert!(
        !kinds(&wide).iter().any(|kind| kind == "plan-unimplemented"),
        "the same item is done inside a wider window: {:?}",
        kinds(&wide)
    );

    // And a cap that bites is printed as a cap.
    let mut flood = Vec::new();
    for i in 0..9 {
        let mut e = episode(&format!("e-{i}"), "unrelated", &["src/ingest/retry.rs"]);
        e.plan = Some(hook(
            vec![PlanItem::open(format!(
                "publish the operator guide number {i}"
            ))],
            &["src/ingest/**"],
        ));
        flood.push(e);
    }
    let policy = Policy {
        max_per_kind: 2,
        ..Default::default()
    };
    let capped = drift::analyse(&[session(flood.clone())], &policy);
    assert_eq!(
        capped
            .findings
            .iter()
            .filter(|f| f.kind == "plan-unimplemented")
            .count(),
        2
    );
    assert!(capped.truncated);
    assert!(capped.verdict().contains("the caps cut some"));
    // The other cap, on a policy of its own: a `Policy` is built from its defaults in one expression,
    // not poked at afterwards (`clippy::field_reassign_with_default`). `max_per_kind` is lifted to 20
    // so that the *total* is what gets cut here.
    let squeezed = drift::analyse(
        &[session(flood)],
        &Policy {
            max_per_kind: 20,
            max_findings: 3,
            ..Default::default()
        },
    );
    assert_eq!(squeezed.findings.len(), 3);
    assert!(squeezed.truncated);
}

#[test]
fn an_episode_that_stated_nothing_is_counted_rather_than_guessed_at() {
    let report = drift::analyse(
        &[session(vec![episode("e-1", "silence", &["src/a.rs"])])],
        &Policy::default(),
    );
    assert!(report.findings.is_empty());
    assert_eq!(report.planned, 0);
    assert!(report.verdict().starts_with("nothing to compare"));
    assert!(
        report
            .unchecked
            .iter()
            .any(|skip| skip.reason.contains("stated a plan")),
        "and it says which session it looked at: {:?}",
        report.unchecked
    );
}

#[test]
fn a_plan_is_compared_inside_its_own_session_unless_told_otherwise() {
    let mut first = episode("e-1", "start", &["src/ingest/a.rs"]);
    first.plan = Some(hook(
        vec![PlanItem::open("retire the legacy frame parser")],
        &["src/ingest/**"],
    ));
    let earlier = session(vec![first]);
    let later = session(vec![episode(
        "e-2",
        "retire the legacy frame parser",
        &["src/parse/legacy.rs"],
    )]);

    let separate = drift::analyse(&[earlier.clone(), later.clone()], &Policy::default());
    assert!(
        kinds(&separate).contains(&"plan-unimplemented".to_owned()),
        "two sessions are two conversations: {:?}",
        kinds(&separate)
    );

    let policy = Policy {
        cross_session: true,
        ..Default::default()
    };
    let joined = drift::analyse(&[earlier, later], &policy);
    assert!(
        !kinds(&joined).contains(&"plan-unimplemented".to_owned()),
        "with cross-session comparison the next session's change counts: {:?}",
        kinds(&joined)
    );
}

#[test]
fn the_same_input_prints_the_same_bytes() {
    let mut e = episode("e-1", "retry budget", &["src/ingest/retry.rs"]);
    e.plan = Some(hook(
        vec![
            PlanItem::open("publish the migration guide for operators"),
            PlanItem::open("write tests for the retry budget"),
        ],
        &["src/ingest/**"],
    ));
    e.signoff = Some("ok — retry budget shipped".to_owned());
    let sessions = [session(vec![e])];
    let policy = Policy::default();
    let first = drift::analyse(&sessions, &policy);
    for _ in 0..4 {
        let again = drift::analyse(&sessions, &policy);
        assert_eq!(again.to_json(), first.to_json());
        assert_eq!(again.render(), first.render());
    }
}

#[test]
fn the_rendering_prints_both_sides_and_the_policy_that_ran() {
    let mut e = episode("e-1", "retry budget", &["src/ingest/retry.rs"]);
    e.plan = Some(hook(
        vec![PlanItem::open("publish the migration guide for operators")],
        &["src/ingest/**"],
    ));
    let rendered = drift::analyse(&[session(vec![e])], &Policy::default()).render();
    let text = rendered.join("\n");
    assert!(text.contains("fathom-drift/1"));
    assert!(text.contains("said"));
    assert!(text.contains("history"));
    assert!(text.contains("min_overlap"));
    assert!(text.contains("dismiss with `drift:"));
}

#[test]
fn the_kinds_are_a_closed_vocabulary_and_every_one_is_reachable() {
    assert_eq!(drift::KINDS.len(), 6);
    let ids: Vec<&str> = drift::KINDS.iter().map(|(id, _, _, _)| *id).collect();
    let _ = severity_of("high");
    for id in [
        "plan-unimplemented",
        "out-of-scope-edit",
        "unplanned-dependency",
        "test-promised-not-written",
        "refactor-claimed-not-performed",
        "contradicts-signoff",
    ] {
        assert!(ids.contains(&id), "{id} left the vocabulary");
        assert!(!drift::question_of(id).is_empty());
        assert!(
            ["high", "notable", "note"].contains(&severity_of(id)),
            "{id} uses a severity the delta model does not know"
        );
    }
    assert_eq!(severity_of("from-a-newer-build"), "note");
}

#[test]
fn the_switches_switch_off_and_say_which_way_the_report_went() {
    let build = || {
        let mut e = episode("e-1", "retry budget", &["src/ingest/retry.rs"]);
        e.plan = Some(hook(
            vec![
                PlanItem::open("publish the migration guide for operators"),
                PlanItem::open("write tests for the retry budget"),
            ],
            &["src/ingest/**"],
        ));
        session(vec![e])
    };
    let policy = Policy {
        off: ["plan-unimplemented".to_owned()].into_iter().collect(),
        ..Default::default()
    };
    let report = drift::analyse(&[build()], &policy);
    assert!(!kinds(&report).contains(&"plan-unimplemented".to_owned()));
    assert!(
        report
            .tallies
            .iter()
            .any(|tally| tally.kind == "plan-unimplemented" && !tally.enabled),
        "the report says the kind was off: {:?}",
        report.tallies
    );
    assert!(kinds(&report).contains(&"test-promised-not-written".to_owned()));
}

// ---------------------------------------------------------------------------------------------
// The module's own hygiene
// ---------------------------------------------------------------------------------------------

#[test]
fn drift_reads_nothing_but_what_it_is_handed() {
    // ADR-002 §4.5: the analysis half is a pure function of what it is given. A `use rusqlite`, a
    // `std::fs`, or a clock read anywhere in this module is the seam being crossed.
    for file in ["src/drift.rs", "src/drift/detect.rs"] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file);
        let source = std::fs::read_to_string(&path).expect("the module is where it says it is");
        for banned in [
            "rusqlite",
            "std::fs",
            "std::time",
            "chrono",
            "fathom_store",
            "reqwest",
        ] {
            assert!(
                !source.contains(banned),
                "{file} reaches for {banned}; drift is a pure function of its input"
            );
        }
    }
}

#[test]
fn overlap_is_found_inside_not_equal_to() {
    // The measurement every rule hangs off, pinned so a threshold change is a deliberate one.
    let item = tokens("bump the parser");
    let corpus = tokens("parser frame handling in src/parse/parser.rs bump");
    assert!(overlap_of(&item, &corpus) >= 0.66);
    assert_eq!(overlap_of(&item, &tokens("nothing at all here")), 0.0);
    assert_eq!(
        overlap_of(&tokens(""), &corpus),
        0.0,
        "an empty item is nothing to compare"
    );
}

fn overlap_of(
    a: &std::collections::BTreeSet<String>,
    b: &std::collections::BTreeSet<String>,
) -> f64 {
    overlap(a, b)
}
