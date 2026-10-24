//! The CI gate's decision layer (`e6.6`, `reco-ngk`).
//!
//! The gate is the only part of Fathom that decides anything, so these tests are the ticket's acceptance
//! criteria restated as questions about a value:
//!
//! * **existing debt does not block, new debt does** — the ratchet has already removed the forgiven
//!   violations before the gate is built, so the gate's own test is that an empty delta passes and an
//!   introduced finding does not;
//! * **a check that could not run is never a pass** — `blocked`, `off` and `ran{examined: 0}` are three
//!   states that must not collapse into "clean";
//! * **nothing examined while something was blocked is a failure**;
//! * **the one sentence** names the count, the threshold, the range and the worst finding with its
//!   location, and is one sentence;
//! * **policy refuses instead of guessing** — a version this build cannot read, a value that is not a
//!   severity, a key nobody typed, a pattern that would silence nothing;
//! * **the SARIF document is uploadable** — every `ruleId` declared, locations only where the scan can see
//!   the file, and a fingerprint that survives the finding sliding down a file.
//!
//! Everything here is a literal: no repository, no store, no clock. That is the point of keeping the
//! decision layer in `fathom_analyze::check` rather than in the verb — the exit code is a pure function of
//! a value a test can build, and the verb's own suite
//! (`crates/fathom-cli/tests/check_verb.rs`) only has to prove the gathering.
#![allow(clippy::unwrap_used, clippy::expect_used)] // a test that cannot fail loudly is not a test

use fathom_analyze::check::render;
use fathom_analyze::check::sarif::{self, Context};
use fathom_analyze::check::{
    CONTRACT, Check, CheckRun, Evidence, Finding, Gate, Outcome, POLICY_VERSION, Policy, Range,
    Severity, Status,
};
use fathom_analyze::ratchet::Mode;

/// A policy from a config fragment, refusing anything that would be a silent default.
fn policy(text: &str) -> Policy {
    Policy::from_config_text(text)
        .unwrap_or_else(|err| panic!("policy should parse: {err}\n{text}"))
}

/// A deny-mode policy failing at `warning` — the posture a blocking CI job runs under.
fn deny() -> Policy {
    policy("[check]\nmode = \"deny\"\nfail_on = \"warning\"\n")
}

/// A run that judged `examined` things and found nothing.
fn clean(check: Check, examined: usize) -> CheckRun {
    CheckRun::ran(check, None, examined, Vec::new())
}

/// A run that judged `examined` things and found `findings`.
fn found(check: Check, examined: usize, findings: Vec<Finding>) -> CheckRun {
    let volume = findings.iter().map(|f| f.severity).max();
    CheckRun::ran(
        check,
        volume.or(Some(Severity::Warning)),
        examined,
        findings,
    )
}

fn finding(check: Check, rule: &str, severity: Severity, path: &str, line: u32) -> Finding {
    Finding::new(
        check,
        rule,
        severity,
        format!("{rule} objected about {path}"),
    )
    .with_evidence(Evidence::file(path, Some(line), "graph:layering"))
}

fn range() -> Range {
    Range {
        base: "aaaa1111".to_owned(),
        head: "bbbb2222".to_owned(),
        base_from: "`--base`".to_owned(),
        head_from: "the working head".to_owned(),
        episodes: 3,
        episodes_partial: None,
    }
}

fn gate(runs: Vec<CheckRun>) -> Gate {
    gate_under(deny(), runs)
}

fn gate_under(policy: Policy, runs: Vec<CheckRun>) -> Gate {
    Gate::build(policy, range(), runs)
}

// ---------------------------------------------------------------------------
// the policy: what is enforced, and what refuses to be guessed
// ---------------------------------------------------------------------------

#[test]
fn an_absent_table_is_the_documented_default_and_says_which_default_it_is() {
    // No `[check]` at all is a repository that has not chosen, which is not the same failure as a chosen
    // policy that cannot be read. The defaults are the *quiet* pair — report at warning, do not block —
    // because adopting a gate should cost nothing on day one, and the notice prints what was assumed.
    let read = policy("[layers]\nl0 = [\"src\"]");
    assert_eq!(read.mode, Mode::Warn);
    assert_eq!(read.fail_on, Some(Severity::Warning));
    assert_eq!(read.version, POLICY_VERSION);
    assert!(read.decides());
    assert!(!read.enforces(), "warn objects without blocking");
    assert!(
        read.notices
            .iter()
            .any(|notice| notice.contains("no [check]") && notice.contains("mode `warn`")),
        "{:?}",
        read.notices
    );

    // An empty file is the same statement, differently spelled.
    assert!(policy("").notices.iter().any(|n| n.contains("defaults")));
}

#[test]
fn every_check_ships_at_a_volume_that_matches_what_its_engine_thinks_is_blocking() {
    // The defaults are not uniform, and the reason is each engine's own `severity` default: layering,
    // cycles and the API contract are structural facts a reviewer can act on, so they fail the run;
    // dependencies, tests and sign-off are judgement calls, so they alarm without blocking. A uniform
    // default would be one of the two wrong way round for three of the six.
    let read = policy("");
    for (check, want) in [
        (Check::Layering, Some(Severity::Error)),
        (Check::Cycles, Some(Severity::Error)),
        (Check::Api, Some(Severity::Error)),
        (Check::Dependencies, Some(Severity::Warning)),
        (Check::Tests, Some(Severity::Warning)),
        (Check::SignOff, Some(Severity::Warning)),
    ] {
        assert_eq!(
            read.severity(check),
            want,
            "{} default volume",
            check.as_str()
        );
    }
    assert_eq!(read.require, Vec::new(), "nothing is required until asked");
    assert_eq!(read.max_findings, 40);
    assert_eq!(read.tests.min_new_lines, 200);
    assert_eq!(read.risk.threshold, 70);
}

#[test]
fn a_policy_version_this_build_cannot_read_refuses_instead_of_partly_obeying() {
    // A newer `fathom` wrote the file; this one is in CI. Honouring the version means refusing to read a
    // layout you do not have — a partial read would enforce a policy that exists only in the reader's head.
    for (text, needle) in [
        ("[check]\nversion = 99\n", "is 99"),
        ("[check]\nversion = 0\n", "never a policy layout"),
        ("[check]\nversion = \"one\"\n", "must be an integer"),
    ] {
        let error = Policy::from_config_text(text).expect_err("must refuse");
        assert!(error.to_string().contains(needle), "{text} -> {error}");
    }
    // The version this build *does* read is read.
    assert_eq!(policy("[check]\nversion = 1\n").version, 1);
}

#[test]
fn a_key_nobody_typed_is_refused_at_the_key_that_names_the_keys() {
    // The mirror image of a missing key, and the more dangerous one: a key you wrote and the gate ignored
    // is a default you did not choose. `layering` here is plausible-looking, which is the point.
    let error = Policy::from_config_text("[check]\nlayering = \"off\"\n").expect_err("must refuse");
    let message = error.to_string();
    assert!(message.contains("check.layering is not a key"), "{message}");
    assert!(
        message.contains("max_findings"),
        "{message} lists the real keys"
    );

    // Same argument one table down.
    let error =
        Policy::from_config_text("[check.tests]\nmin_lines = 40\n").expect_err("must refuse");
    assert!(
        error
            .to_string()
            .contains("check.tests.min_lines is not a key"),
        "{error}"
    );
}

#[test]
fn a_value_that_is_not_one_of_the_listed_ones_refuses_naming_what_it_found() {
    for (text, needle) in [
        (
            "[check]\nmode = \"sometimes\"\n",
            "check.mode is `sometimes`",
        ),
        ("[check]\nfail_on = \"maybe\"\n", "note|warning|error|never"),
        ("[check]\nrequire = [\"layerings\"]\n", "check.require"),
        ("[check]\nmax_findings = 0\n", "bound of zero"),
        ("[check]\nmax_findings = \"lots\"\n", "must be an integer"),
        (
            "[check.severity]\ntests = \"loud\"\n",
            "off|note|warning|error",
        ),
        ("[check.tests]\nmin_tested_ratio = 4.0\n", "not a share"),
        ("[check.tests]\nmin_new_lines = -1\n", "no size is"),
        ("[check.risk]\nthreshold = 101\n", "threshold"),
        ("[check]\nseverity = 5\n", "[check.severity] is not a table"),
        ("[check.severity]\nlayerings = \"off\"\n", "names no check"),
    ] {
        let error = Policy::from_config_text(text).expect_err("{text} must refuse");
        assert!(error.to_string().contains(needle), "{text} -> {error}");
    }
}

#[test]
fn an_ignore_pattern_that_could_match_nothing_is_refused_at_load() {
    // `ignore = ["fixtures"]` looks like it silences something and silences nothing — the discovery comes
    // at review time, when a green run is being explained. The family half must name a check.
    let error =
        Policy::from_config_text("[check]\nignore = [\"fixtures\"]\n").expect_err("must refuse");
    assert!(error.to_string().contains("check.ignore"), "{error}");

    let read = policy("[check]\nignore = [\"tests\", \"layering:no_upward\", \"sign-off:ep-1\"]\n");
    assert_eq!(read.ignore.len(), 3);
}

#[test]
fn the_silencing_matcher_is_a_family_prefix_and_a_rule_substring_and_nothing_cleverer() {
    let read = policy("[check]\nignore = [\"tests\", \"cycles:billing\", \"api:removed\"]\n");
    assert!(read.silenced_by("tests/logic-without-tests").is_some());
    assert!(
        read.silenced_by("cycles/new-cycle").is_none(),
        "the rule half must match"
    );
    assert!(
        read.silenced_by("cycles/new-cycle: billing→vault")
            .is_some(),
        "substring"
    );
    assert!(
        read.silenced_by("layering/no_upward::a->b").is_none(),
        "a different family"
    );
    assert_eq!(
        read.silenced_by("api/removed"),
        Some("api:removed"),
        "the pattern is reported back, so the run can name what silenced it"
    );
}

#[test]
fn kebab_and_snake_names_read_the_same_because_both_will_be_typed() {
    // The check's canonical spelling is `sign-off`, and `[check.severity]` is a table of check names;
    // forcing one spelling would make the other a silently unknown key, which the previous test refuses.
    for text in [
        "[check.severity]\nsign-off = \"error\"\n",
        "[check.severity]\nsign_off = \"error\"\n",
    ] {
        assert_eq!(
            policy(text).severity(Check::SignOff),
            Some(Severity::Error),
            "{text}"
        );
    }
    for text in [
        "[check]\nrequire = [\"sign-off\"]\n",
        "[check]\nrequire = [\"sign_off\"]\n",
    ] {
        assert_eq!(policy(text).require, vec![Check::SignOff], "{text}");
    }
}

#[test]
fn off_is_a_volume_like_any_other_and_a_switched_off_check_cannot_speak() {
    let read = policy("[check.severity]\ntests = \"off\"\n");
    assert_eq!(read.severity(Check::Tests), None);
    // A row built from the policy's volume for an off check is an `off` row, and the gate will not count
    // what it finds there — the choice is stated in the table, not discovered in the findings.
    let gate = Gate::build(
        read,
        range(),
        vec![CheckRun::off(Check::Tests), clean(Check::Layering, 10)],
    );
    assert_eq!(
        gate.runs[0].status,
        Status::Ran { examined: 10 },
        "rows follow `Check::ALL`"
    );
    let tests = gate
        .runs
        .iter()
        .find(|r| r.check == Check::Tests)
        .expect("a row per check");
    assert_eq!(tests.status, Status::Off);
    assert_eq!(tests.volume, None);

    assert_eq!(gate.outcome(), Outcome::Pass);
}

// ---------------------------------------------------------------------------
// the decision: what makes a run red
// ---------------------------------------------------------------------------

#[test]
fn existing_debt_does_not_block_and_the_same_violation_arriving_new_does() {
    // The ratchet does its work before the gate is built: `layering::episode` consults
    // `.fathom/ratchet.toml` and hands over only what this range introduced, with the forgiven count in a
    // note. So the gate's half of "fails only on new violations" is this pair of runs.
    let baselined = clean(Check::Layering, 412).with_notes(vec![
        "3 violation(s) were baselined in .fathom/ratchet.toml, so they did not count".to_owned(),
    ]);
    let pass = gate(vec![baselined, clean(Check::Cycles, 0)]);
    assert_eq!(pass.outcome(), Outcome::Pass);
    assert_eq!(pass.exit_code().as_i32(), 0);
    assert_eq!(pass.found(), 0, "a baselined violation is not a finding");

    let introduced = found(
        Check::Layering,
        412,
        vec![finding(
            Check::Layering,
            "no_upward::src/l0/base.rs->src/l1/up.rs",
            Severity::Error,
            "src/l0/base.rs",
            3,
        )],
    );
    let fail = gate(vec![introduced, clean(Check::Cycles, 0)]);
    assert_eq!(fail.outcome(), Outcome::Fail);
    assert_eq!(fail.exit_code().as_i32(), 7);
    assert_eq!(fail.failing().len(), 1);
}

#[test]
fn the_threshold_is_one_line_and_counts_everything_at_or_above_it() {
    let run = found(
        Check::Cycles,
        9,
        vec![
            finding(Check::Cycles, "a→b→a", Severity::Note, "a.rs", 1),
            finding(Check::Cycles, "c→d→c", Severity::Warning, "c.rs", 2),
            finding(Check::Cycles, "e→f→e", Severity::Error, "e.rs", 3),
        ],
    );
    let counts = gate(vec![run.clone()]);
    assert_eq!(
        counts.counts(),
        [
            (Severity::Note, 1),
            (Severity::Warning, 1),
            (Severity::Error, 1)
        ]
    );
    assert_eq!(counts.failing().len(), 2, "warning and error, not the note");

    let stricter = Gate::build(
        policy("[check]\nmode = \"deny\"\nfail_on = \"error\"\n"),
        range(),
        vec![run.clone()],
    );
    assert_eq!(stricter.failing().len(), 1);
    assert_eq!(stricter.outcome(), Outcome::Fail);

    // `fail_on = never`: report everything, decide nothing — the nightly that tells you what you would have
    // blocked on, which is how a team picks a threshold.
    let never = Gate::build(
        policy("[check]\nmode = \"deny\"\nfail_on = \"never\"\n"),
        range(),
        vec![run],
    );
    assert_eq!(never.failing().len(), 0);
    assert_eq!(never.outcome(), Outcome::Pass);
    assert_eq!(never.found(), 3, "still counted, still printed");
}

#[test]
fn warn_objects_and_uploads_but_exits_zero_where_deny_blocks() {
    // The three-way switch `[ratchet] mode` already established, and the reason the shipped default can be
    // the permissive one: `warn` is what makes it safe to turn the gate on at all.
    let runs = || {
        vec![found(
            Check::Layering,
            10,
            vec![finding(
                Check::Layering,
                "r",
                Severity::Error,
                "src/a.rs",
                1,
            )],
        )]
    };
    let warn = Gate::build(
        policy("[check]\nmode = \"warn\"\nfail_on = \"warning\"\n"),
        range(),
        runs(),
    );
    assert_eq!(warn.outcome(), Outcome::Fail, "the verdict is the verdict");
    assert_eq!(
        warn.exit_code().as_i32(),
        0,
        "and the mode declined to enforce it"
    );
    assert_eq!(
        warn.failing().len(),
        1,
        "the finding is still there to print"
    );
    assert!(
        warn.sentence()
            .contains("`check.mode = warn`, so this is reported and not enforced"),
        "the sentence explains the exit code: {}",
        warn.sentence()
    );

    let off = Gate::build(policy("[check]\nmode = \"off\"\n"), range(), runs());
    assert_eq!(off.outcome(), Outcome::Off);
    assert_eq!(off.exit_code().as_i32(), 0);
    assert!(
        off.sentence().contains("decided nothing"),
        "{}",
        off.sentence()
    );
    assert!(off.failing().is_empty(), "off applies no threshold at all");

    assert_eq!(Gate::build(deny(), range(), runs()).exit_code().as_i32(), 7);
}

#[test]
fn a_required_check_that_could_not_run_fails_the_run_and_says_why() {
    // Without this rule, a broken pipeline is a green pipeline: `findings: []` is what a blocked check
    // prints if it has nowhere to put its inability.
    let mut policy = deny();
    policy.require = vec![Check::Layering];
    let blocked = CheckRun::blocked(
        Check::Layering,
        Some(Severity::Error),
        "this index holds no module graph",
    )
    .with_remedy("`fathom analyze` first");

    // Required and blocked: red, with the reason as the sentence.
    let gate = Gate::build(
        policy.clone(),
        range(),
        vec![blocked.clone(), clean(Check::Api, 40)],
    );
    assert_eq!(gate.outcome(), Outcome::Fail);
    assert_eq!(gate.exit_code().as_i32(), 7);
    assert_eq!(
        gate.sentence(),
        "fathom check failed: `layering` is required and could not run — this index holds no module graph"
    );
    assert_eq!(gate.blocked_required().len(), 1);

    // Not required: the run passes on what *did* run, and the gap is a notice the log prints.
    let other = gate_under(deny(), vec![blocked, clean(Check::Api, 40)]);
    assert_eq!(other.outcome(), Outcome::Pass);
    assert!(other.blocked_required().is_empty());
}

#[test]
fn nothing_examined_while_something_was_blocked_is_a_failure_not_an_empty_bill() {
    // The silent pass, named. A run where every check was blocked or saw no episode has established
    // nothing, and `findings: 0` in a log reads as "clean" to everyone who reads it.
    let blocked = CheckRun::blocked(
        Check::Api,
        Some(Severity::Error),
        "the base is outside this clone",
    );
    let empty = clean(Check::Tests, 0);
    let run = gate(vec![blocked, empty]);
    assert!(run.nothing_examined());
    assert_eq!(run.outcome(), Outcome::Fail);
    assert_eq!(
        run.sentence(),
        "fathom check failed: nothing was examined in aaaa1111..bbbb2222 — the base is outside this clone"
    );

    // One check that looked at one thing is enough to have earned a pass.
    assert!(
        !gate(vec![
            CheckRun::blocked(Check::Api, Some(Severity::Error), "no"),
            clean(Check::Tests, 1),
        ])
        .nothing_examined()
    );
    assert_eq!(
        gate(vec![clean(Check::Api, 0), clean(Check::Tests, 1)]).outcome(),
        Outcome::Pass,
        "nothing blocked and something examined: that is a real pass"
    );

    // `off` is exempt, because it is a choice rather than an absence: the operator said what they want
    // switched off, and a run that refuses to decide must not be argued with by an exit code.
    let all_off = Gate::build(
        policy("[check]\nmode = \"off\"\n"),
        range(),
        vec![CheckRun::off(Check::Tests), CheckRun::off(Check::Layering)],
    );
    assert_eq!(all_off.outcome(), Outcome::Off);
    assert_eq!(all_off.exit_code().as_i32(), 0);
}

#[test]
fn a_pass_with_nothing_examined_says_so_instead_of_reading_as_clean() {
    // The pass that is not a clean bill of health gets its own sentence, because "passed" is the word a
    // reviewer stops reading at.
    let gate = Gate::build(
        deny(),
        range(),
        vec![
            CheckRun::blocked(Check::Api, Some(Severity::Error), "the base is missing"),
            CheckRun::off(Check::Tests),
        ],
    );
    // Blocked but nothing required and nothing examined → the failure above, not this one. Force a pass.
    let _ = &gate;
    let soft = Gate::build(
        policy("[check]\nmode = \"deny\"\nfail_on = \"never\"\n"),
        range(),
        vec![CheckRun::off(Check::Tests), CheckRun::off(Check::Api)],
    );
    assert_eq!(soft.outcome(), Outcome::Pass);
    assert!(
        soft.sentence().contains("with nothing examined"),
        "{}",
        soft.sentence()
    );
}

// ---------------------------------------------------------------------------
// the one sentence
// ---------------------------------------------------------------------------

#[test]
fn the_failure_sentence_names_the_count_the_threshold_the_range_and_the_worst_finding() {
    // The acceptance criterion, verbatim: *"the 'why did CI fail' message is one sentence long and points
    // at the evidence"*. Worst-first ordering means `first()` is the worst, so the pointer is the one a
    // reader would have gone looking for anyway.
    let gate = gate(vec![
        found(
            Check::Layering,
            412,
            vec![finding(
                Check::Layering,
                "no-reach-past-the-door",
                Severity::Error,
                "crates/fathom-cli/src/state.rs",
                88,
            )],
        ),
        clean(Check::Api, 1180),
    ]);
    assert_eq!(
        gate.sentence(),
        "fathom check failed: 1 finding(s) at or above `warning` in aaaa1111..bbbb2222 — worst is error \
         [layering/no-reach-past-the-door] at crates/fathom-cli/src/state.rs:88 (graph:layering): \
         no-reach-past-the-door objected about crates/fathom-cli/src/state.rs"
    );
}

#[test]
fn the_sentence_is_one_sentence_in_every_state() {
    // Not style: the same string is a log line, a SARIF `automationDetails.message.text`, a markdown
    // heading and a step-annotation body. A paragraph breaks the first and reads badly in the other three.
    let failing = gate(vec![found(
        Check::Layering,
        1,
        vec![finding(Check::Layering, "r", Severity::Error, "a.rs", 1)],
    )]);
    let passing = gate(vec![clean(Check::Layering, 412)]);
    let off = Gate::build(
        policy("[check]\nmode = \"off\"\n"),
        range(),
        vec![clean(Check::Layering, 1)],
    );
    for gate in [failing, passing, off] {
        let sentence = gate.sentence();
        assert!(!sentence.contains('\n'), "{sentence}");
        assert_eq!(sentence.matches(". ").count(), 0, "{sentence}");
        assert!(sentence.starts_with("fathom check "), "{sentence}");
    }
}

#[test]
fn a_pass_states_the_threshold_it_cleared_and_what_ran() {
    let gate = gate(vec![clean(Check::Layering, 412), clean(Check::Api, 1180)]);
    assert_eq!(
        gate.sentence(),
        "fathom check passed aaaa1111..bbbb2222: 0 finding(s) below the `warning` threshold, 2 check(s) ran"
    );
}

// ---------------------------------------------------------------------------
// silencing, capping and the counts behind them
// ---------------------------------------------------------------------------

#[test]
fn a_silenced_finding_is_counted_and_named_not_vanished() {
    // `check.ignore` is a repository choosing not to hear something, which is legitimate, and a gate that
    // forgets to mention it is a gate with a hidden suppression list — the thing the ratchet page argues
    // against for its own ledger.
    let policy =
        policy("[check]\nmode = \"deny\"\nfail_on = \"warning\"\nignore = [\"tests:generated\"]\n");
    let run = found(
        Check::Tests,
        4,
        vec![Finding::new(
            Check::Tests,
            "logic-without-tests:generated",
            Severity::Error,
            "src/generated/blob.rs grew 400 lines with no test beside it",
        )],
    );
    let gate = Gate::build(policy, range(), vec![run]);
    assert_eq!(gate.outcome(), Outcome::Pass, "nothing left to fail on");
    assert_eq!(gate.silenced.len(), 1);
    assert_eq!(gate.silenced[0].key, "tests/logic-without-tests:generated");
    assert_eq!(gate.silenced[0].pattern, "tests:generated");
    assert_eq!(gate.silenced[0].count, 1);
    assert!(gate.silenced[0].example.contains("400 lines"));
    assert_eq!(gate.found(), 1, "the count still includes it");
    assert!(gate.sentence().contains("1 silenced by `check.ignore`"));
}

#[test]
fn the_list_is_bounded_and_the_count_is_not() {
    // A range that introduced two hundred violations must not bury the two hundred and one under a wall of
    // text — and must not *say* two either. `findings` is a prefix, `total` is the answer, `truncated` is
    // the difference, and all five formats read the same three numbers.
    let policy = Policy {
        max_findings: 2,
        ..deny()
    };
    let many: Vec<Finding> = (0..5)
        .map(|i| {
            finding(
                Check::Layering,
                &format!("r{i}"),
                Severity::Warning,
                "a.rs",
                i + 1,
            )
        })
        .collect();
    let gate = Gate::build(policy, range(), vec![found(Check::Layering, 10, many)]);
    assert_eq!(gate.runs[0].findings.len(), 2);
    assert_eq!(gate.runs[0].total, 5);
    assert_eq!(gate.found(), 5);
    assert_eq!(gate.listed(), 2);
    assert!(
        gate.sentence().contains("(2 of 5 listed)"),
        "{}",
        gate.sentence()
    );

    // A collapsed row is one finding with a count, so fifty hunks of one violation read as one line.
    let collapsed_row = {
        let mut one = Finding::new(
            Check::Dependencies,
            "new-direct",
            Severity::Warning,
            "a package",
        );
        one.count = 50;
        found(Check::Dependencies, 3, vec![one])
    };
    let collapsed = Gate::build(deny(), range(), vec![collapsed_row]);
    assert_eq!(collapsed.found(), 1);
    assert_eq!(
        collapsed.counts()[1].1,
        50,
        "the severity count is occurrences"
    );
    assert_eq!(
        collapsed.failing().len(),
        1,
        "and the failure count is findings"
    );
}

#[test]
fn findings_are_ordered_worst_first_then_by_family_and_rule() {
    // Two runs over the same inputs must print the same bytes, or the SARIF fingerprints and a golden test
    // are meaningless. A `HashSet` somewhere would break that silently.
    let run = found(
        Check::Layering,
        10,
        vec![
            finding(Check::Layering, "z", Severity::Note, "z.rs", 1),
            finding(Check::Layering, "b", Severity::Error, "b.rs", 1),
            finding(Check::Layering, "a", Severity::Error, "a.rs", 1),
        ],
    );
    let gate = gate(vec![run]);
    let keys: Vec<String> = gate.runs[0]
        .findings
        .iter()
        .map(|finding| format!("{:?} {}", finding.severity, finding.rule))
        .collect();
    assert_eq!(keys, ["Error a", "Error b", "Note z"]);
}

// ---------------------------------------------------------------------------
// the document every format is a projection of
// ---------------------------------------------------------------------------

#[test]
fn the_json_document_carries_the_verdict_and_its_reasons() {
    let policy = policy(
        "[check]\nmode = \"deny\"\nfail_on = \"warning\"\nbase = \"origin/main\"\n[check.severity]\ntests = \"off\"\n",
    );
    let mut gate = Gate::build(
        policy,
        range(),
        vec![
            found(
                Check::Api,
                1180,
                vec![finding(
                    Check::Api,
                    "removed",
                    Severity::Error,
                    "src/lib.rs",
                    4,
                )],
            ),
            CheckRun::blocked(
                Check::Cycles,
                Some(Severity::Error),
                "this index holds no module graph",
            )
            .with_remedy("`fathom analyze`"),
            CheckRun::off(Check::Tests),
        ],
    );
    gate.producer = "fathom 0.1.0-test".to_owned();
    gate.notices = vec!["the base was resolved from the pipeline".to_owned()];
    let value = gate.to_json();

    assert_eq!(value["contract"], CONTRACT);
    assert_eq!(value["producer"], "fathom 0.1.0-test");
    assert_eq!(value["outcome"], "fail");
    assert_eq!(value["exit"], 7);
    assert_eq!(value["exit_name"], "CHECK_VIOLATIONS");
    assert_eq!(value["why"], gate.sentence());
    assert_eq!(value["range"]["text"], "aaaa1111..bbbb2222");
    assert_eq!(value["range"]["base_from"], "`--base`");
    assert_eq!(value["policy"]["base"], "origin/main");
    assert_eq!(value["policy"]["fail_on"], "warning");
    assert_eq!(value["policy"]["severity"]["api"], "error");
    assert_eq!(value["policy"]["severity"]["tests"], "off");
    assert_eq!(value["summary"]["failing"], 1);
    // One blocked row out of three. The run failed on the API finding, not on the blocked `cycles` row
    // (`cycles` is not required here), so this count is the only place that difference is visible — which is
    // exactly why it is in the document and not only in the rows.
    assert_eq!(value["summary"]["blocked"], 1);
    assert_eq!(
        value["summary"]["blocked_required"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(value["summary"]["by_severity"][0]["severity"], "note");
    assert_eq!(value["summary"]["by_severity"][2]["severity"], "error");
    // Rows are in `Check::ALL` order — layering, cycles, api, dependencies, tests, sign-off — which is the
    // order of the ticket's sentence, not alphabetical, so each row is looked up by name here and the order
    // is asserted once, on its own.
    let rows: Vec<&Value> = value["checks"]
        .as_array()
        .expect("a row per check")
        .iter()
        .collect();
    assert_eq!(
        rows.iter()
            .map(|row| row["check"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["cycles", "api", "tests"]
    );
    let api = rows[1];
    assert_eq!(api["examined"], 1180);
    assert_eq!(api["findings"][0]["evidence"][0]["path"], "src/lib.rs");
    assert_eq!(api["findings"][0]["evidence"][0]["line"], 4);
    let cycles = rows[0];
    assert_eq!(cycles["status"], "blocked");
    assert_eq!(cycles["reason"], "this index holds no module graph");
    assert_eq!(cycles["remedy"], "`fathom analyze`");
    assert_eq!(
        cycles["examined"],
        Value::Null,
        "a blocked row examined nothing"
    );
    assert_eq!(rows[2]["status"], "off");
    assert_eq!(
        value["notices"][0],
        "the base was resolved from the pipeline"
    );
}

use serde_json::Value;

#[test]
fn a_blocked_row_never_reports_an_examined_count() {
    // `"examined": 0` on a *ran* row is honest and `"examined": 0` on a blocked row is a lie about scope —
    // so the field is absent unless the check actually looked.
    let value = gate(vec![
        clean(Check::Tests, 0),
        CheckRun::blocked(Check::Api, Some(Severity::Error), "no"),
    ])
    .to_json();
    assert_eq!(
        value["checks"][0]["check"], "api",
        "rows follow `Check::ALL` order"
    );
    assert_eq!(value["checks"][0]["status"], "blocked");
    assert_eq!(value["checks"][0]["examined"], Value::Null);
    assert_eq!(value["checks"][1]["check"], "tests");
    assert_eq!(value["checks"][1]["status"], "ran");
    assert_eq!(value["checks"][1]["examined"], 0);
}

// ---------------------------------------------------------------------------
// SARIF
// ---------------------------------------------------------------------------

#[test]
fn every_rule_is_declared_and_no_location_points_where_the_scan_cannot_look() {
    // The two things that make a code-scanning upload warn rather than land: a `ruleId` no rule declares,
    // and a location whose file is not in the revision under scan. The second is why `log` takes an
    // `exists` callback — only the composition root has the tree.
    let gate = gate(vec![
        found(
            Check::Layering,
            412,
            vec![
                finding(
                    Check::Layering,
                    "no-reach-past-the-door",
                    Severity::Error,
                    "src/a.rs",
                    12,
                ),
                finding(
                    Check::Layering,
                    "no_upward::gone.rs->src/b.rs",
                    Severity::Warning,
                    "src/gone.rs",
                    7,
                ),
            ],
        ),
        clean(Check::Api, 10),
    ]);
    let context = Context {
        fathom_version: "0.1.0-test",
        revision: Some("aaaa1111bbbb2222"),
        repository_uri: Some("https://example.invalid/repo"),
    };
    let log = sarif::log(&gate, &context, &|path| path == "src/a.rs");

    assert_eq!(log["version"], "2.1.0");
    assert_eq!(log["runs"][0]["tool"]["driver"]["name"], "fathom");
    assert_eq!(log["runs"][0]["columnKind"], "utf16CodeUnits");
    assert_eq!(
        log["runs"][0]["automationDetails"]["description"]["text"],
        gate.sentence()
    );
    assert_eq!(
        log["runs"][0]["versionControlDetails"][0]["revisionId"],
        "aaaa1111bbbb2222"
    );
    assert_eq!(
        log["runs"][0]["versionControlDetails"][0]["repositoryUri"],
        "https://example.invalid/repo"
    );

    let rules = log["runs"][0]["tool"]["driver"]["rules"]
        .as_array()
        .expect("rules");
    let ids: Vec<&str> = rules
        .iter()
        .map(|rule| rule["id"].as_str().expect("a rule id"))
        .collect();
    // The *rule*, not the instance. An engine that keys a finding by the place (`no_upward::a->b`) would
    // otherwise declare one SARIF rule per violation, and a code-scanning view that groups by rule would
    // turn sixty violations of one rule into sixty groups of one.
    assert_eq!(
        ids,
        [
            "fathom/layering/no-reach-past-the-door",
            "fathom/layering/no-upward"
        ],
        "one descriptor per rule that spoke, in the order the findings first mention them"
    );
    for rule in rules {
        let id = rule["id"].as_str().unwrap();
        assert!(
            !rule["shortDescription"]["text"]
                .as_str()
                .unwrap()
                .is_empty(),
            "{id}"
        );
        assert!(
            rule["help"]["text"].as_str().unwrap().contains("[check]"),
            "{id} help tells a reader where the volume is set"
        );
        assert!(
            rule["helpUri"]
                .as_str()
                .unwrap()
                .starts_with("https://fathom.dev/"),
            "the rule's help points at a page, not at nothing"
        );
    }

    let results = log["runs"][0]["results"].as_array().expect("results");
    assert_eq!(
        results.len(),
        2,
        "the finding in a missing file is still a result…"
    );
    assert_eq!(
        results[0]["ruleId"],
        "fathom/layering/no-reach-past-the-door"
    );
    assert_eq!(results[0]["ruleIndex"], 0);
    assert_eq!(results[1]["ruleId"], "fathom/layering/no-upward");
    assert_eq!(
        results[1]["ruleIndex"], 1,
        "ruleIndex must point at the rule the ruleId names — an orphan or a mismatched pair is the classic \
         code-scanning upload rejection"
    );
    assert_eq!(
        results[1]["level"], "warning",
        "a warning finding is a SARIF `warning`, not a silent demotion"
    );
    // Two instances of the *same* rule, at the same line, in different files, must not share a fingerprint:
    // that is the collision that turns "this violation moved" into "two unrelated violations appeared".
    let first_print = results[0]["partialFingerprints"]["primaryLocationLineHash"]
        .as_str()
        .unwrap();
    let second_print = results[1]["partialFingerprints"]["primaryLocationLineHash"]
        .as_str()
        .unwrap();
    assert_ne!(
        first_print, second_print,
        "both findings are at line 12/7 of different files under different rules"
    );
    assert_eq!(
        results[1]["properties"]["fathomRule"].as_str().unwrap(),
        "no_upward::gone.rs->src/b.rs",
        "the instance the rule id drops is still on the result, where a reader can see which edge this is"
    );
    assert_eq!(
        results[0]["level"], "error",
        "severity maps to SARIF's three levels"
    );
    assert!(
        results[0]["message"]["text"]
            .as_str()
            .unwrap()
            .contains("objected about")
    );
    assert_eq!(
        results[0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
        "src/a.rs"
    );
    assert_eq!(
        results[0]["locations"][0]["physicalLocation"]["region"]["startLine"],
        12
    );
    assert!(
        results[1].get("locations").is_none(),
        "…with its location dropped, because a dead location is the upload warning"
    );
    for result in results {
        let id = result["ruleId"].as_str().unwrap();
        assert!(ids.contains(&id), "{id} is used without being declared");
        assert_eq!(
            result["partialFingerprints"]["primaryLocationLineHash"]
                .as_str()
                .map(str::len),
            Some(16)
        );
    }
}

#[test]
fn a_fingerprint_survives_the_finding_sliding_down_a_file() {
    // Deliberately not the line number. A scan is posted per push, and a finding that moved because someone
    // added a function above it is the same finding; re-opening it as a new alert every rebase trains people
    // to close alerts, which is the failure the whole fingerprint field exists to prevent.
    let at = |line: u32, path: &str| {
        Finding::new(
            Check::Layering,
            "no-reach-past-the-door",
            Severity::Error,
            "the door was walked past",
        )
        .with_evidence(Evidence::file(path, Some(line), "graph:layering"))
        .fingerprint()
    };
    assert_eq!(
        at(12, "src/a.rs"),
        at(90, "src/a.rs"),
        "line does not matter"
    );
    assert_ne!(at(12, "src/a.rs"), at(12, "src/b.rs"), "file does");
    // And an absent location still fingerprints, so a locationless finding is not deduplicated into nothing.
    assert_eq!(
        Finding::new(
            Check::Api,
            "removed",
            Severity::Error,
            "a public item went away"
        )
        .fingerprint()
        .len(),
        16
    );
}

// ---------------------------------------------------------------------------
// the renderings
// ---------------------------------------------------------------------------

#[test]
fn the_log_prints_the_table_with_the_state_each_row_is_in() {
    let policy = policy(
        "[check]\nmode = \"deny\"\nfail_on = \"warning\"\n[check.severity]\ntests = \"off\"\n",
    );
    let mut gate = Gate::build(
        policy,
        range(),
        vec![
            found(
                Check::Layering,
                412,
                vec![finding(
                    Check::Layering,
                    "no_upward::a->b",
                    Severity::Error,
                    "src/a.rs",
                    3,
                )],
            )
            .with_notes(vec!["3 violation(s) were baselined".to_owned()]),
            CheckRun::off(Check::Tests),
            CheckRun::blocked(
                Check::Api,
                Some(Severity::Error),
                "the base is outside this clone",
            )
            .with_remedy("`fetch-depth: 0`"),
        ],
    );
    gate.notices = vec!["this is a shallow clone".to_owned()];
    let log = render::human(&gate, 40).join("\n");

    assert!(log.contains("fathom check · aaaa1111..bbbb2222 · base from `--base` · policy v1 · mode deny · fail at warning"), "{log}");
    assert!(
        log.contains("layering      error    ran       412 dependency edges judged  1 (1 listed)"),
        "{log}"
    );
    assert!(
        log.contains("tests         off      off       switched off by policy"),
        "{log}"
    );
    assert!(
        log.contains("api           error    blocked   the base is outside this cl… —"),
        "{log}"
    );
    assert!(log.contains("remedy: `fetch-depth: 0`"), "{log}");
    assert!(log.contains("note: 3 violation(s) were baselined"), "{log}");
    assert!(log.contains("notices"), "{log}");
    assert!(log.contains("this is a shallow clone"), "{log}");
    assert!(log.contains("fathom check failed:"), "{log}");
    assert!(
        log.contains("`fathom check --format json` for every finding"),
        "{log}"
    );
}

#[test]
fn step_annotations_cover_only_the_findings_that_made_the_run_red() {
    // Annotating every finding means the pull request is decorated on a day the gate is only reporting —
    // `warn` mode's whole point is that it must not shout.
    let run = || {
        found(
            Check::Layering,
            10,
            vec![
                finding(Check::Layering, "loud", Severity::Error, "src/a.rs", 12),
                finding(Check::Layering, "quiet", Severity::Note, "src/b.rs", 4),
            ],
        )
    };
    let denying = gate(vec![run()]);
    let lines = render::annotations(&denying);
    assert_eq!(lines.len(), 1, "the note is not annotated: {lines:?}");
    assert!(lines[0].starts_with("::error "), "{}", lines[0]);
    assert!(lines[0].contains("file=src/a.rs,line=12"), "{}", lines[0]);
    assert!(
        lines[0].contains("title=fathom · layering/loud"),
        "{}",
        lines[0]
    );
    assert!(
        lines[0].ends_with(
            "error [layering/loud] loud objected about src/a.rs — src/a.rs:12 (graph:layering)"
        ),
        "{}",
        lines[0]
    );

    let warnings = render::annotations(&Gate::build(
        policy("[check]\nmode = \"warn\"\nfail_on = \"warning\"\n"),
        range(),
        vec![run()],
    ));
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0].starts_with("::warning "),
        "warn never wears the red badge: {}",
        warnings[0]
    );
    assert!(warnings[0].contains("would block: "), "{}", warnings[0]);
    assert!(render::annotations(&gate(vec![clean(Check::Layering, 10)])).is_empty());
}

#[test]
fn step_annotations_escape_the_workflow_grammar() {
    // A rule id is user data — a layering rule's key contains `->` and `::` — and GitHub's workflow-command
    // parser ends a property list at `,` and `:`. Unescaped, a rule key could inject properties.
    let annotations = render::annotations(&gate(vec![found(
        Check::Layering,
        1,
        vec![finding(
            Check::Layering,
            "no_upward::a->b, and:more",
            Severity::Error,
            "src/a.rs",
            3,
        )],
    )]));
    assert_eq!(annotations.len(), 1);
    let line = &annotations[0];
    assert!(
        line.contains("title=fathom · layering/no_upward%3A%3Aa->b%2C and%3Amore"),
        "{line}"
    );
    let body = line.rsplit("::").next().unwrap();
    assert!(!body.contains('\n'), "{body}");
}

#[test]
fn the_job_summary_is_the_table_the_sentence_and_every_finding() {
    let mut gate = gate(vec![
        found(
            Check::Layering,
            412,
            vec![finding(
                Check::Layering,
                "no_upward::a->b",
                Severity::Error,
                "src/a.rs",
                3,
            )],
        ),
        CheckRun::ran(Check::Api, Some(Severity::Error), 1180, Vec::new()),
    ]);
    gate.notices = vec!["the index holds 3 of 5 episodes".to_owned()];
    let text = render::markdown(&gate);
    assert!(text.starts_with("## fathom check"), "{text}");
    assert!(text.contains(&gate.sentence()), "{text}");
    assert!(
        text.contains("| `layering` | error | ran | 412 | 1 |"),
        "{text}"
    );
    assert!(
        text.contains("| `api` | error | ran | 1180 | 0 |"),
        "{text}"
    );
    assert!(
        text.contains("| error | `layering/no_upward::a->b` |"),
        "{text}"
    );
    assert!(text.contains("the index holds 3 of 5 episodes"), "{text}");
}
