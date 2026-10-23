//! The ratchet's write path, as pure rules (`e3.10`, `reco-5ht`).
//!
//! The ticket's four acceptance criteria are all about what the *file* does over time, and the file is a
//! string, so every one of them is testable without a repository:
//!
//! * **only new violations are news** — a violation with a live allowance is never in [`Plan::uncovered`],
//!   and one without it always is (§ *news is the absence of a live row*);
//! * **the ratchet only turns one way** — a violation that disappears takes its row with it, automatically,
//!   while a violation that appears changes nothing until somebody grants it (§ *tightening is automatic*,
//!   *widening is an act*);
//! * **raising the baseline is a signed act** — a row that enters the file carries an owner, an expiry, a
//!   reason and the name of whoever asked (§ *the act is in the row*), and a key that matches nothing is
//!   refused rather than written (§ *a row nobody can match is a row nobody will delete*);
//! * **expiries are seen before they lapse** — the warning window, the back-due list, and the re-grant that
//!   replaces a lapsed row instead of duplicating it (§ *expiry*).
//!
//! What is *not* tested here is the file's own existence, the clock, and the graph read: that is
//! `crates/fathom-cli/tests/ratchet_verb.rs`, which runs the binary.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_analyze::cycles::debt::Ratchet;
use fathom_analyze::ratchet::{Evaluated, Grant, Ledger, Offence, Plan, parse_date};
use fathom_core::time::{MICROS_PER_DAY, Timestamp};

/// The instant the whole file judges against: midday on 1 April 2026. Midday rather than midnight, because
/// the ledger's day rule — an allowance is live *through* its expiry day — is only visible from inside a
/// day, and `parse_date`'s instants are all the midnight after one.
fn now() -> Timestamp {
    midday("2026-04-01")
}

/// Midday of a day, spelled the way the file spells it.
fn midday(text: &str) -> Timestamp {
    Timestamp::from_unix_micros(day(text).as_unix_micros() - MICROS_PER_DAY / 2)
}

/// A day, spelled the way the file spells it: live *through* that day.
fn day(text: &str) -> Timestamp {
    parse_date(text).expect("a YYYY-MM-DD day")
}

/// The day the act is being done on, in the file's own day-grain spelling.
fn today() -> Timestamp {
    day("2026-04-01")
}

/// One cycle offence, in the shape [`Offence::cycle`] produces: an arrow-joined path that normalises to the
/// sorted member set.
fn cycle(members: &str) -> Offence {
    Offence {
        kind: "cycle".to_string(),
        key: members.to_string(),
        body: format!("{members} — a cycle"),
    }
}

/// One layering offence, key spelled the way [`fathom_analyze::layering::finding::Violation::key`] spells
/// it.
fn layer(key: &str) -> Offence {
    Offence {
        kind: "layer".to_string(),
        key: key.to_string(),
        body: format!("{key} — a rule says not"),
    }
}

/// A baseline written by hand, as a repository would commit it.
fn baseline(text: &str) -> Ratchet {
    Ratchet::from_text(text, ".fathom/ratchet.toml")
}

/// No file at all — the state of a repository that has never run the verb. Not the same thing as an empty
/// file: this one is `baselined: false`, and that is what a report has to say.
fn no_file() -> Ratchet {
    Ratchet::empty()
}

/// The grant the verb would build: owner, reason, one quarter, granted today.
fn grant(reason: &str) -> Grant {
    Grant::new(
        "team-payments",
        reason,
        Some(day("2026-07-01")),
        today(),
        "Rue Reviewer",
    )
    .expect("an owner and a reason")
}

/// A run that judged both engines, which is the usual case.
fn both() -> Evaluated {
    Evaluated {
        cycles: true,
        layer: true,
    }
}

// ---------------------------------------------------------------------------
// tightening is automatic
// ---------------------------------------------------------------------------

#[test]
fn a_violation_that_goes_away_takes_its_allowance_with_it() {
    let file = "\
[[allowance]]
kind = \"cycle\"
key = \"billing → invoicing → billing\"
owner = \"team-payments\"
expires = \"2026-06-01\"
reason = \"the split lands with the ledger rewrite\"
";
    let plan = Plan::build(&baseline(file), &[], None, both(), now());
    assert_eq!(plan.removed.len(), 1, "the row whose cycle is gone leaves");
    assert!(
        plan.removed[0]
            .why
            .contains("no cycle violation in this revision matches it"),
        "{}",
        plan.removed[0].why
    );
    assert!(plan.after.is_empty(), "and it is gone from the file");
    assert!(plan.changed());
    assert!(
        !plan.render().contains("[[allowance]]"),
        "the rendered file holds no row: {}",
        plan.render()
    );
    // The header survives, because the file is still a baseline document and a reader should be told what
    // it is even when it holds nothing.
    assert!(plan.render().contains("# Fathom ratchet baseline"));
}

#[test]
fn tightening_needs_no_flag_but_widening_does() {
    let offences = [cycle("a → b → a")];
    let bare = Plan::build(&no_file(), &offences, None, both(), now());
    assert!(
        bare.added.is_empty(),
        "a bare run never writes a row that forgives something"
    );
    assert!(
        bare.notices
            .iter()
            .any(|line| line.contains("--update-baseline")),
        "and it says what to type instead: {:?}",
        bare.notices
    );
    assert!(
        !bare.baselined,
        "no file was read, and that is stated, not defaulted"
    );
    assert!(
        !bare.changed(),
        "and a run with nothing to tighten and no act to perform writes nothing, which is why a first run \
         cannot silently create a baseline"
    );

    let act = Plan::build(
        &no_file(),
        &offences,
        Some(&grant("we will fix it")),
        both(),
        now(),
    );
    assert_eq!(act.added.len(), 1, "the flag plus the reason writes it");
    assert!(
        act.changed(),
        "and that is the one time a missing file is created"
    );
}

#[test]
fn a_kind_this_run_could_not_judge_is_never_tightened() {
    // The trap: `[layering]` is switched off for an afternoon, the layer rows stop matching anything, and a
    // writer that only looks for matches deletes a repository's whole layer budget.
    let file = "\
[[allowance]]
kind = \"layer\"
key = \"no_upward::crates/store/src/read.rs->crates/core/src/engine.rs\"
owner = \"team-core\"
expires = \"2026-06-01\"
reason = \"the ranking inverts with the storage rewrite\"
";
    let plan = Plan::build(
        &baseline(file),
        &[],
        None,
        Evaluated {
            cycles: true,
            layer: false,
        },
        now(),
    );
    assert!(plan.removed.is_empty(), "the rules went away, not the debt");
    assert_eq!(plan.untouched.len(), 1);
    assert_eq!(plan.after.len(), 1, "the row is still there for tomorrow");

    // And with the rules back on, the same empty inventory does tighten it.
    let turned = Plan::build(&baseline(file), &[], None, both(), now());
    assert_eq!(turned.removed.len(), 1);
}

#[test]
fn a_row_of_a_kind_this_verb_does_not_judge_is_left_alone() {
    let file = "\
[[allowance]]
kind = \"dep\"
key = \"GPL-3.0::vendor/ffmpeg\"
owner = \"legal\"
expires = \"2026-06-01\"
reason = \"pending the re-link decision\"
";
    let plan = Plan::build(&baseline(file), &[], None, both(), now());
    assert!(plan.removed.is_empty(), "no evidence about it, no deletion");
    assert_eq!(plan.after.len(), 1, "it round-trips through the rewrite");
    assert!(
        plan.notices
            .iter()
            .any(|line| line.contains("of kind dep") && line.contains("left alone")),
        "{:?}",
        plan.notices
    );
}

// ---------------------------------------------------------------------------
// widening is an act, and the act is in the row
// ---------------------------------------------------------------------------

#[test]
fn an_act_without_a_reason_cannot_be_constructed() {
    for (owner, reason) in [
        ("team-payments", ""),
        ("team-payments", "   "),
        ("", "the split lands with the ledger rewrite"),
    ] {
        let err = Grant::new(owner, reason, Some(day("2026-07-01")), today(), "Rue")
            .expect_err("an owner and a reason are both required");
        assert!(
            err.contains(if reason.trim().is_empty() {
                "reason"
            } else {
                "owner"
            }),
            "{err}"
        );
    }
}

#[test]
fn a_granted_row_carries_who_and_why_and_survives_the_round_trip() {
    let offences = [cycle("billing → invoicing → billing")];
    let plan = Plan::build(
        &baseline(""),
        &offences,
        Some(&grant("the split lands in July")),
        both(),
        now(),
    );
    let written = plan.render();
    for expect in [
        "kind = \"cycle\"",
        "owner = \"team-payments\"",
        "expires = \"2026-07-01\"",
        "reason = \"the split lands in July\"",
        "granted = \"2026-04-01\"",
        "granted_by = \"Rue Reviewer\"",
    ] {
        assert!(
            written.contains(expect),
            "{expect} missing from:\n{written}"
        );
    }
    assert!(
        plan.diff()[0].starts_with("+ "),
        "the diff marks it as an addition"
    );
    assert!(
        plan.diff()[0].contains("reason: the split lands in July"),
        "and the diff line says who and why: {}",
        plan.diff()[0]
    );

    // Reading the file back and running the same facts again changes nothing: which is the same statement as
    // "CI is green the second time", and the reason the file's spelling has to be the reader's spelling.
    let again = Plan::build(&baseline(&written), &offences, None, both(), now());
    assert!(!again.changed(), "{:?}", again.diff());
    assert_eq!(again.render(), written);
    assert!(
        again.uncovered.is_empty(),
        "the baselined cycle is debt, not news"
    );
    assert_eq!(again.baselined_count(), 1);
}

#[test]
fn a_key_that_matches_nothing_is_refused_rather_than_written() {
    let offences = [cycle("a → b → a")];
    let act = Plan::build(
        &no_file(),
        &offences,
        Some(
            &grant("one of these is a typo")
                .with_keys(&["a → b → a".to_string(), "nope".to_string()]),
        ),
        both(),
        now(),
    );
    assert_eq!(act.added.len(), 1, "the real key was written");
    assert_eq!(
        act.refused.len(),
        1,
        "and the typo was said out loud, not skipped"
    );
    assert!(act.refused[0].contains("`nope`"), "{}", act.refused[0]);
    assert!(!act.render().contains("nope"), "nothing was written for it");
}

#[test]
fn a_key_names_its_kind_when_two_engines_could_mean_the_same_words() {
    let offences = [cycle("a → b → a"), layer("no_upward::a->b")];
    let one = Plan::build(
        &no_file(),
        &offences,
        Some(&grant("only the edge").with_keys(&["layer=no_upward::a->b".to_string()])),
        both(),
        now(),
    );
    assert_eq!(one.added.len(), 1);
    assert_eq!(one.added[0].kind, "layer");
    assert_eq!(one.added[0].pattern, "no_upward::a->b");

    // A bare key is tried as the offence's own grammar, because a pasted key is not retyped.
    let either = Plan::build(
        &no_file(),
        &offences,
        Some(&grant("both").with_keys(&["no_upward::a->b".to_string(), "a → b → a".to_string()])),
        both(),
        now(),
    );
    assert_eq!(either.added.len(), 2);
}

#[test]
fn the_same_fact_written_by_two_runs_produces_the_same_file() {
    let offences = [
        layer("no_upward::a->b"),
        cycle("x → y → x"),
        layer("door::p->q"),
    ];
    let first = Plan::build(
        &no_file(),
        &offences,
        Some(&grant("day one")),
        both(),
        now(),
    );
    let second = Plan::build(
        &no_file(),
        &offences,
        Some(&grant("day one")),
        both(),
        now(),
    );
    assert_eq!(
        first.render(),
        second.render(),
        "sorted, so a re-run is an empty diff"
    );
    assert!(
        first.render().find("kind = \"cycle\"") < first.render().find("kind = \"layer\""),
        "and the order is the file's, not the engines': {}",
        first.render()
    );
}

// ---------------------------------------------------------------------------
// news is the absence of a live row
// ---------------------------------------------------------------------------

#[test]
fn a_baselined_violation_is_debt_and_a_new_one_is_news() {
    let file = "\
[[allowance]]
kind = \"cycle\"
key = \"a|b\"
owner = \"team-payments\"
expires = \"2026-06-01\"
reason = \"the split lands with the ledger rewrite\"
";
    let offences = [cycle("a → b → a"), cycle("c → d → c")];
    let plan = Plan::build(&baseline(file), &offences, None, both(), now());
    assert_eq!(plan.uncovered.len(), 1, "one baselined, one not");
    assert_eq!(plan.uncovered[0].key, "c → d → c");
    assert_eq!(plan.mode, fathom_analyze::ratchet::Mode::Warn);
    assert_eq!(plan.exit_code(), None, "warn does not decide exit codes");
    assert_eq!(
        plan.summary(),
        "1 allowance(s) live in .fathom/ratchet.toml — 0 expiring within 30 day(s), 0 back due, 1 \
         violation(s) uncovered; the file is unchanged"
    );
}

#[test]
fn deny_mode_is_what_makes_an_uncovered_violation_fail_a_run() {
    let offences = [cycle("a → b → a")];
    let mut ledger = no_file();
    ledger.mode = fathom_analyze::ratchet::Mode::Deny;
    let plan = Plan::build(&ledger, &offences, None, both(), now());
    assert_eq!(
        plan.exit_code().map(|code| code.as_i32()),
        Some(7),
        "the code CI branches on"
    );
    assert!(plan.alarms());
    let mut off = ledger.clone();
    off.mode = fathom_analyze::ratchet::Mode::Off;
    assert!(
        Plan::build(&off, &offences, None, both(), now())
            .exit_code()
            .is_none(),
        "and `off` is a decision, not a bug: it exits 0 and says so"
    );
}

// ---------------------------------------------------------------------------
// expiry: seen before it lapses, and back due after
// ---------------------------------------------------------------------------

#[test]
fn an_allowance_is_named_while_it_still_has_days_left() {
    let offences = [cycle("a → b → a")];
    let soon = Grant::new(
        "team-payments",
        "the split lands in April",
        Some(day("2026-04-10")),
        today(),
        "Rue",
    )
    .expect("a grant");
    let plan = Plan::build(&no_file(), &offences, Some(&soon), both(), now());
    assert_eq!(plan.expiring.len(), 1, "nine days out is inside the window");
    let line = &plan.expiring_lines()[0];
    assert!(line.contains("expires 2026-04-10"), "{line}");
    assert!(line.contains("9 day(s) left"), "{line}");
    assert!(line.contains("team-payments"), "{line}");

    // Outside the window the same row is quiet: a warning that is always on is noise, and noise is what this
    // whole feature is a rebuttal of.
    let far = Plan::build(
        &no_file(),
        &offences,
        Some(
            &Grant::new(
                "team-payments",
                "a year",
                Some(day("2027-04-01")),
                today(),
                "Rue",
            )
            .expect("a grant"),
        ),
        both(),
        now(),
    );
    assert!(far.expiring.is_empty());
}

#[test]
fn an_expired_allowance_is_back_due_and_a_regrant_replaces_it() {
    let file = "\
[[allowance]]
kind = \"cycle\"
key = \"a|b\"
owner = \"team-payments\"
expires = \"2026-03-01\"
reason = \"the split landed late\"
";
    let offences = [cycle("a → b → a")];
    let plan = Plan::build(&baseline(file), &offences, None, both(), now());
    assert_eq!(plan.lapsed.len(), 1, "the day passed and the cycle stayed");
    assert_eq!(
        plan.uncovered.len(),
        1,
        "so it is news again, with its date named"
    );
    assert!(
        plan.lapsed_lines()[0].contains("expired 2026-03-01"),
        "{}",
        plan.lapsed_lines()[0]
    );
    assert!(
        plan.lapsed_lines()[0].contains("the split landed late"),
        "the reason it was forgiven is printed beside the fact it was not paid"
    );

    // Re-granting is a new act and replaces the row: two rows for one cycle would make the next verdict
    // depend on which one a reader found first.
    let re = Plan::build(
        &baseline(file),
        &offences,
        Some(&grant("it slipped again")),
        both(),
        now(),
    );
    assert_eq!(re.renewed.len(), 1);
    assert!(re.added.is_empty(), "a re-grant is not a new allowance");
    assert_eq!(re.after.len(), 1, "and the file holds one row for the fact");
    assert!(re.lapsed.is_empty(), "the replaced row is live now");
    assert!(re.render().matches("[[allowance]]").count() == 1);
    assert!(re.render().contains("expires = \"2026-07-01\""));
}

#[test]
fn an_allowance_is_live_through_the_whole_day_it_was_written_for() {
    // `expires = "2026-04-01"` means all of 1 April, because a date a reviewer misreads by one day is a
    // date that starts an argument instead of settling one.
    let file = "\
[[allowance]]
kind = \"cycle\"
key = \"a|b\"
owner = \"team-payments\"
expires = \"2026-04-01\"
reason = \"the last day is the day itself\"
";
    let offences = [cycle("a → b → a")];
    assert!(
        Plan::build(
            &baseline(file),
            &offences,
            None,
            both(),
            midday("2026-04-01")
        )
        .uncovered
        .is_empty()
    );
    assert_eq!(
        Plan::build(
            &baseline(file),
            &offences,
            None,
            both(),
            midday("2026-04-02")
        )
        .uncovered
        .len(),
        1,
        "and it is a violation on the 2nd"
    );
}

// ---------------------------------------------------------------------------
// the document every surface reads
// ---------------------------------------------------------------------------

#[test]
fn the_json_document_names_its_contract_and_every_box() {
    let file = "\
[[allowance]]
kind = \"cycle\"
key = \"a|b\"
owner = \"team-payments\"
expires = \"2026-04-10\"
reason = \"the split lands in April\"
";
    let offences = [cycle("a → b → a"), layer("door::p->q")];
    let json = Plan::build(&baseline(file), &offences, None, both(), now()).to_json();
    assert_eq!(json["contract"], "fathom-ratchet/1");
    assert_eq!(json["allowances"].as_array().unwrap().len(), 1);
    assert_eq!(json["uncovered"].as_array().unwrap().len(), 1);
    assert_eq!(json["uncovered"][0]["kind"], "layer");
    assert_eq!(json["expiring"].as_array().unwrap().len(), 1);
    assert_eq!(json["expiring"][0]["days_left"], 9);
    assert_eq!(json["expiring"][0]["owner"], "team-payments");
    assert_eq!(
        json["expiring"][0]["expires_never"],
        serde_json::json!(false),
        "the key is on every row, not only the permanent ones"
    );
    assert_eq!(json["exit"], serde_json::Value::Null, "warn does not exit");
    assert_eq!(json["changed"], serde_json::json!(false));
    assert!(
        json["summary"]
            .as_str()
            .unwrap()
            .contains("1 violation(s) uncovered"),
        "the one-line state is in the payload too: {}",
        json["summary"]
    );
    // The alarms are four named boxes, because "a cycle is uncovered" and "an allowance lapses on Thursday"
    // are different pages to file, and one `true` cannot tell them apart.
    assert_eq!(json["alarms"]["cycle"], serde_json::json!(false));
    assert_eq!(json["alarms"]["layer"], serde_json::json!(true));
    assert_eq!(json["alarms"]["expiring"], serde_json::json!(true));
    assert_eq!(json["alarms"]["back_due"], serde_json::json!(false));

    // And `off` means off: the same facts raise no alarm, because the mode is a decision to stop asking.
    let mut off = baseline(file);
    off.mode = fathom_analyze::ratchet::Mode::Off;
    let off = Plan::build(&off, &offences, None, both(), now());
    assert_eq!(off.to_json()["alarms"]["layer"], serde_json::json!(false));
    assert_eq!(
        off.to_json()["alarms"]["expiring"],
        serde_json::json!(false)
    );

    // `deny` is what turns the same ledger into a failing run, and its alarm is the box CI branches on.
    let mut deny = baseline(file);
    deny.mode = fathom_analyze::ratchet::Mode::Deny;
    let deny = Plan::build(&deny, &offences, None, both(), now());
    assert_eq!(deny.to_json()["alarms"]["layer"], serde_json::json!(true));
    assert_eq!(deny.to_json()["exit"], serde_json::json!(7));
}

/// The read-only ledger read (`ratchet::Ledger`) is what a surface with no graph hands to its widgets — the
/// architecture map, the artefact — so it is tested here rather than in each of them. The contract: a file and
/// a clock, nothing else; the two lists a reviewer acts on; and **no claim about what is still owed**, which
/// needs the graph and belongs to [`Plan`].
#[test]
fn a_ledger_read_on_its_own_names_what_is_running_out() {
    let file = "\
[[allowance]]
kind = \"cycle\"
key = \"a|b\"
owner = \"team-payments\"
expires = \"2026-04-10\"
reason = \"the split lands in April\"

[[allowance]]
kind = \"layer\"
key = \"door::p->q\"
owner = \"team-storage\"
expires = \"2026-01-20\"
reason = \"the seam is next quarter\"

[[allowance]]
kind = \"cycle\"
key = \"c|d\"
owner = \"nobody-yet\"
reason = \"a row written to last forever, and said so\"
";
    let ledger = Ledger::of(&baseline(file), now());
    assert!(ledger.read, "a file was there");
    assert_eq!(ledger.rows, 3);
    assert_eq!(ledger.live, 2, "the January row has had its day");
    assert_eq!(
        ledger.expiring.len(),
        1,
        "only the April row is inside the window"
    );
    assert_eq!(ledger.lapsed.len(), 1);
    let expiring = &ledger.expiring_lines()[0];
    assert!(expiring.contains("2026-04-10"), "{expiring}");
    assert!(expiring.contains("team-payments"), "{expiring}");
    assert!(expiring.contains("day(s) left"), "{expiring}");
    let lapsed = &ledger.lapsed_lines()[0];
    assert!(lapsed.contains("2026-01-20"), "{lapsed}");
    assert!(lapsed.contains("the seam is next quarter"), "{lapsed}");
    assert!(ledger.speaks());
    assert!(
        ledger.sentence().contains("2 allowance(s) live"),
        "{}",
        ledger.sentence()
    );
    assert!(
        ledger.sentence().contains("expiring within 30 day(s)"),
        "{}",
        ledger.sentence()
    );
    // The permanent row is neither list: it is live, it has no date, and it is not a warning.
    let json = ledger.to_json();
    assert_eq!(json["live"], serde_json::json!(2));
    assert_eq!(json["expiring"].as_array().unwrap().len(), 1);
    assert_eq!(
        json["lapsed"].as_array().unwrap()[0]["expires"],
        "2026-01-20"
    );

    // And an absent ledger is a state with a sentence, not an error.
    let none = Ledger::of(&no_file(), now());
    assert!(!none.read);
    assert!(
        !none.speaks(),
        "a repository with no ledger has no dates to warn about"
    );
    assert!(
        none.sentence().contains("no ratchet baseline"),
        "{}",
        none.sentence()
    );
}
