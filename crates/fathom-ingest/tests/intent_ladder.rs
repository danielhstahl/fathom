//! The intent ladder (`e4.2`, `reco-c54`): one shape, seven rungs, and a state for "nothing was said".
//!
//! The ticket's rule is the one every test here serves: *missing intent is a normal state, not an error*.
//! That is cheap to say and hard to build, because it means the ladder has to refuse to guess — no subject
//! line, no author, no file list — and it means the three ways an episode can have no intent text (nobody
//! said anything, something said a request happened and its text was not kept, and nothing was ever looked
//! for) come back as three different answers.
//!
//! | Criterion | Tests |
//! |---|---|
//! | prompt, plan and session come from the hooks | `the_prompt_the_harness_recorded_wins_over_everything`, `a_plan_item_named_by_the_harness_is_the_open_one` |
//! | without hooks the trailer fallback works | `an_intent_trailer_is_an_intent_not_an_afterthought`, `a_task_line_outranks_a_plan_trailer` |
//! | absent intent says so, with the reason | `nothing_anywhere_is_a_state_with_a_reason`, `a_prompt_digest_with_no_text_is_its_own_state` |
//! | declared scope is stored as globs | `declared_scope_is_normalised_sorted_and_deduplicated`, `a_scope_that_escapes_the_repository_is_refused_by_name` |
//! | the ladder is configurable in membership, not order | `a_source_a_repository_switches_off_is_reported_and_not_used`, `a_refused_statement_says_who_refused_it` |

#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_ingest::intent::{
    Intent, PlanItem, Policy, Source, State, TurnEvidence, capture, normalise_scope,
};
use fathom_ingest::segment::CommitFacts;

/// One commit with `trailers` in its trailer block. Keys are lower-cased because that is how
/// [`fathom_ingest::record::Trailers`] stores them, and the ladder reads only that spelling.
fn commit(oid: &str, trailers: &[(&str, &str)]) -> CommitFacts {
    CommitFacts {
        oid: oid.to_owned(),
        subject: format!("feat: the {oid} change"),
        trailers: trailers
            .iter()
            .map(|(key, value)| (key.to_ascii_lowercase(), (*value).to_owned()))
            .collect(),
        ..CommitFacts::default()
    }
}

/// A turn the harness described. Everything optional stays empty unless a test names it.
fn turn() -> TurnEvidence {
    TurnEvidence {
        session_id: "s1".to_owned(),
        turn_id: "s1:1".to_owned(),
        ..TurnEvidence::default()
    }
}

fn intent_of(commits: &[CommitFacts], hook: Option<&TurnEvidence>) -> Intent {
    capture(commits, hook, &Policy::default())
}

/// The published spelling of the deciding rung.
fn source(intent: &Intent) -> &str {
    intent.source.as_str()
}

// ---------------------------------------------------------------------------
// The rungs, in the order the module's table publishes them
// ---------------------------------------------------------------------------

#[test]
fn the_prompt_the_harness_recorded_wins_over_everything() {
    let commits = vec![commit(
        "c1",
        &[
            ("Intent", "a trailer about the same commit"),
            ("Task", "T-9"),
        ],
    )];
    let hook = TurnEvidence {
        prompt: Some("make the rail scroll without dropping the selection".to_owned()),
        prompt_sha256: Some("aaaaaaaa".to_owned()),
        prompt_chars: Some(54),
        intent: Some("a declared summary line".to_owned()),
        plan: vec![PlanItem::new("fix the scroll")],
        ..turn()
    };
    let intent = intent_of(&commits, Some(&hook));

    assert_eq!(source(&intent), "hook-prompt");
    assert_eq!(
        intent.text.as_deref(),
        Some("make the rail scroll without dropping the selection")
    );
    assert_eq!(intent.confidence, 0.95);
    assert_eq!(intent.state, State::Recorded.as_str());
    assert_eq!(intent.session_id.as_deref(), Some("s1"));
    assert_eq!(intent.turn_id.as_deref(), Some("s1:1"));
    assert_eq!(intent.reason, None, "an answer does not carry a reason");
    // Every rung that fired stays in the evidence list, strongest first: the card shows its work.
    let sources: Vec<&str> = intent.evidence.iter().map(|e| e.source.as_str()).collect();
    assert_eq!(
        sources,
        vec![
            "hook-prompt",
            "hook-intent",
            "agent-intent-trailer",
            "plan-item",
            "task-trailer"
        ],
        "the ladder records what it did not choose as well as what it did: {intent:?}"
    );
    assert!(
        intent
            .evidence
            .windows(2)
            .all(|w| w[0].confidence >= w[1].confidence),
        "the evidence list is priced in order: {:?}",
        intent.evidence
    );
}

#[test]
fn a_declared_intent_line_is_an_intent_even_with_no_prompt_text() {
    // ADR-005 §2.1's shape: `turn_start` carries a prompt *digest*, and some harnesses send a summary line
    // instead of the prompt. The summary is what was said, so it decides — and the digest stays beside it.
    let hook = TurnEvidence {
        intent: Some("teach the exporter to skip noise".to_owned()),
        prompt_sha256: Some("b".repeat(64)),
        prompt_chars: Some(612),
        ..turn()
    };
    let intent = intent_of(&[commit("c1", &[])], Some(&hook));
    assert_eq!(source(&intent), "hook-intent");
    assert_eq!(
        intent.text.as_deref(),
        Some("teach the exporter to skip noise")
    );
    let prompt = intent
        .prompt
        .expect("the digest survived alongside the text");
    assert!(
        !prompt.text_captured,
        "the text came from the intent line, not from the prompt"
    );
    assert_eq!(prompt.chars, Some(612));
}

#[test]
fn an_intent_trailer_is_an_intent_not_an_afterthought() {
    // The ticket's second criterion: a team with no hooks at all still gets a real intent.
    let commits = vec![commit(
        "c1",
        &[("Intent", "keep the parser inside the grammar file")],
    )];
    let intent = intent_of(&commits, None);
    assert_eq!(source(&intent), "agent-intent-trailer");
    assert_eq!(
        intent.text.as_deref(),
        Some("keep the parser inside the grammar file")
    );
    assert_eq!(intent.confidence, 0.90);
    assert_eq!(intent.state, State::Recorded.as_str());
    assert!(
        intent
            .evidence
            .first()
            .expect("one rung fired")
            .detail
            .contains("Intent:"),
        "the evidence line quotes the trailer key the commit actually carried"
    );
}

#[test]
fn agent_intent_is_a_trailer_this_build_reads() {
    // The fixture corpus writes `Agent-Intent:`; the corpus's history is the trailer test.
    let intent = intent_of(
        &[commit("c1", &[("Agent-Intent", "stop the rail jumping")])],
        None,
    );
    assert_eq!(source(&intent), "agent-intent-trailer");
    assert_eq!(intent.text.as_deref(), Some("stop the rail jumping"));
}

#[test]
fn a_task_line_outranks_a_plan_trailer() {
    // Rows 5 and 6 of the published table: a task names the work (0.70), a transcribed plan names one
    // step of it (0.65). Both are "named", neither is "stated", and the order is not negotiable.
    let commits = vec![commit(
        "c1",
        &[
            ("Plan", "read the diff; write the card"),
            ("Task", "T-42 write the card pane"),
        ],
    )];
    let intent = intent_of(&commits, None);
    assert_eq!(source(&intent), "task-trailer");
    assert_eq!(intent.text.as_deref(), Some("T-42 write the card pane"));
    assert_eq!(intent.confidence, 0.70);
    assert_eq!(
        intent
            .plan
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>(),
        vec!["read the diff", "write the card"],
        "the plan list is kept whole even when a task decided the text: {:?}",
        intent.plan
    );
}

#[test]
fn a_plan_trailer_alone_is_the_weakest_stated_thing_there_is() {
    let intent = intent_of(
        &[commit("c1", &[("Plan", "read the diff; write the card")])],
        None,
    );
    assert_eq!(source(&intent), "plan-item");
    assert_eq!(intent.text.as_deref(), Some("read the diff"));
    assert_eq!(intent.confidence, 0.65);
}

#[test]
fn a_plan_item_named_by_the_harness_is_the_open_one() {
    let hook = TurnEvidence {
        plan: vec![
            PlanItem {
                text: "capture the prompt".to_owned(),
                status: Some("completed".to_owned()),
            },
            PlanItem {
                text: "link the session id".to_owned(),
                status: Some("in_progress".to_owned()),
            },
        ],
        ..turn()
    };
    let intent = intent_of(&[commit("c1", &[])], Some(&hook));
    assert_eq!(source(&intent), "plan-item");
    assert_eq!(intent.text.as_deref(), Some("link the session id"));
    assert_eq!(intent.confidence, 0.70);
    assert!(intent.plan[0].is_done(), "the harness said so");
    assert!(!intent.plan[1].is_done());
}

#[test]
fn a_bare_plan_line_is_open_not_finished() {
    // No status is *open*: an unclaimed "still to do" is the honest reading of a plan line, and the text
    // the ladder falls back to is picked from the open items.
    let intent = intent_of(&[commit("c1", &[("Plan", "one item")])], None);
    assert!(!intent.plan[0].is_done());
    assert_eq!(intent.text.as_deref(), Some("one item"));
    // A plan whose every item is done still says what the turn was for, so it quotes the first one.
    let all_done = TurnEvidence {
        plan: vec![PlanItem {
            text: "finished the thing".to_owned(),
            status: Some("done".to_owned()),
        }],
        ..turn()
    };
    let intent = intent_of(&[commit("c1", &[])], Some(&all_done));
    assert_eq!(intent.text.as_deref(), Some("finished the thing"));
}

#[test]
fn the_first_commit_that_stated_an_intent_owns_it() {
    // Intent is a property of the *turn*. A later "address review" commit that quotes a different request
    // must not rename the one that opened the episode; its line is still evidence.
    let commits = vec![
        commit("c1", &[("Intent", "the original ask")]),
        commit("c2", &[("Intent", "what the reviewer asked for")]),
    ];
    let intent = intent_of(&commits, None);
    assert_eq!(intent.text.as_deref(), Some("the original ask"));
    assert_eq!(
        intent
            .evidence
            .iter()
            .filter(|e| e.source == "agent-intent-trailer")
            .count(),
        2,
        "both trailers are recorded: {:?}",
        intent.evidence
    );
}

// ---------------------------------------------------------------------------
// The three ways there is no text
// ---------------------------------------------------------------------------

#[test]
fn nothing_anywhere_is_a_state_with_a_reason() {
    let intent = intent_of(&[commit("c1", &[])], None);
    assert_eq!(intent.state, State::Absent.as_str());
    assert_eq!(source(&intent), "none");
    assert_eq!(intent.confidence, 0.0);
    assert_eq!(intent.text, None);
    let reason = intent
        .reason
        .as_deref()
        .expect("an absence names what was looked for");
    assert!(
        reason.contains("no harness events") && reason.contains("Intent:"),
        "the reason is a check, not a shrug: {reason}"
    );
    assert!(!intent.is_recorded());
    assert!(
        intent.text.is_none() && intent.evidence.is_empty(),
        "nothing was invented to fill the gap: {intent:?}"
    );
}

#[test]
fn a_turn_that_stated_nothing_is_a_different_absence() {
    // A repository *with* hooks whose turn said nothing. Same state, different sentence: one of these is
    // "the harness is not installed" and the other is "the harness saw the turn and it had no words".
    let intent = intent_of(&[commit("c1", &[])], Some(&turn()));
    assert_eq!(intent.state, State::Absent.as_str());
    assert!(
        intent
            .reason
            .as_deref()
            .expect("a reason")
            .contains("the harness recorded no prompt text"),
        "{:?}",
        intent.reason
    );
    assert_eq!(
        intent.session_id.as_deref(),
        Some("s1"),
        "the turn is still the turn's, even with no words in it"
    );
}

#[test]
fn a_trailer_naming_a_turn_the_stream_does_not_describe_says_so() {
    // The dangling-trailer case: the commit claims a harness turn, the events file has never heard of it.
    let commits = vec![commit(
        "c1",
        &[("Agent-Session", "s9"), ("Agent-Turn", "4")],
    )];
    let intent = intent_of(&commits, None);
    assert_eq!(intent.state, State::Absent.as_str());
    assert!(
        intent
            .reason
            .as_deref()
            .expect("a reason")
            .contains("does not describe"),
        "{:?}",
        intent.reason
    );
    assert_eq!(intent.session_id.as_deref(), Some("s9"));
    assert_eq!(intent.turn_id.as_deref(), Some("4"));
}

#[test]
fn a_prompt_digest_with_no_text_is_its_own_state() {
    // Every fixture in the reference corpus is written this way on purpose (ADR-005 §2.1): a prompt is
    // stored as a digest so rewording a subject line cannot move a corpus digest. A digest proves a request
    // happened. It does not say what it was, so it never becomes text.
    let hook = TurnEvidence {
        prompt_sha256: Some("c".repeat(64)),
        prompt_chars: Some(120),
        ..turn()
    };
    let intent = intent_of(&[commit("c1", &[])], Some(&hook));
    assert_eq!(intent.state, State::Digest.as_str());
    assert_eq!(source(&intent), "prompt-digest");
    assert_eq!(intent.text, None, "a hash is not a sentence");
    assert_eq!(intent.confidence, 0.30);
    assert!(intent.is_recorded(), "something *was* recorded");
    assert!(
        intent
            .reason
            .as_deref()
            .expect("the digest is the reason")
            .contains("120 chars"),
        "{:?}",
        intent.reason
    );
    assert!(
        !intent.prompt.as_ref().expect("prompt").text_captured,
        "and the document says the text is missing"
    );
}

#[test]
fn a_repository_can_refuse_to_count_a_digest_as_an_intent() {
    // `[intent] digest_counts = false`: a repository that would rather print `no recorded intent` than a
    // hash gets exactly that, and keeps the digest as evidence.
    let hook = TurnEvidence {
        prompt_sha256: Some("c".repeat(64)),
        ..turn()
    };
    let policy = Policy {
        digest_counts: false,
        ..Policy::default()
    };
    let intent = capture(&[commit("c1", &[])], Some(&hook), &policy);
    assert_eq!(intent.state, State::Absent.as_str());
    assert_eq!(source(&intent), "none");
    assert_eq!(intent.confidence, 0.0);
    assert!(
        intent.evidence.iter().any(|e| e.source == "prompt-digest"),
        "it still happened: {:?}",
        intent.evidence
    );
}

// ---------------------------------------------------------------------------
// Configuration: membership, not order
// ---------------------------------------------------------------------------

#[test]
fn a_source_a_repository_switches_off_is_reported_and_not_used() {
    // `[intent] sources = ["agent-intent-trailer"]` — the only interesting use of the key is to turn the
    // prompt off, and the turn it hides must still be visible in the evidence list.
    let hook = TurnEvidence {
        prompt: Some("a name this repository does not want quoted".to_owned()),
        ..turn()
    };
    let policy = Policy {
        sources: vec!["agent-intent-trailer".to_owned()],
        ..Policy::default()
    };
    let commits = vec![commit("c1", &[("Intent", "the trailer's own words")])];
    let intent = capture(&commits, Some(&hook), &policy);
    assert_eq!(source(&intent), "agent-intent-trailer");
    assert_eq!(intent.text.as_deref(), Some("the trailer's own words"));
    assert!(
        intent.evidence.iter().any(|e| e.source == "hook-prompt"
            && e.detail.contains("not used")
            && e.detail
                .contains("a name this repository does not want quoted")),
        "existing-but-not-allowed is not invisible, and says which it is: {:?}",
        intent.evidence
    );
}

#[test]
fn a_refused_statement_says_who_refused_it() {
    // The other half of the same rule: when the *only* thing that stated an intent is switched off, the
    // episode is still an absence — but the reason names the config, not an empty harness.
    let hook = TurnEvidence {
        prompt: Some("the only statement there was".to_owned()),
        ..turn()
    };
    let policy = Policy {
        sources: vec!["task-trailer".to_owned()],
        ..Policy::default()
    };
    let intent = capture(&[commit("c1", &[])], Some(&hook), &policy);
    assert_eq!(intent.state, State::Absent.as_str());
    let reason = intent.reason.clone().expect("a reason");
    assert!(
        reason.contains("hook-prompt") && reason.contains("sources"),
        "the reader can fix this one: {reason}"
    );
    assert!(
        !reason.contains("no harness events"),
        "and it is not the same sentence an empty repository gets: {reason}"
    );
}

#[test]
fn a_source_list_naming_nothing_this_build_reads_is_notice_not_error() {
    // `review-note` and `manual-note` are spellings `[deps.provenance] intent_sources` already lists and
    // this ladder has no rung for. A config written against the other list still parses.
    let policy = Policy {
        sources: vec!["review-note".to_owned(), "task-trailer".to_owned()],
        ..Policy::default()
    };
    let commits = vec![commit("c1", &[("Task", "T-1")])];
    let intent = capture(&commits, None, &policy);
    assert_eq!(source(&intent), "task-trailer");
    assert!(!policy.allows(Source::HookPrompt));
    assert!(policy.allows(Source::TaskTrailer));
}

#[test]
fn every_rung_has_a_published_spelling_that_round_trips() {
    for source in [
        Source::HookPrompt,
        Source::HookIntent,
        Source::IntentTrailer,
        Source::HookPlan,
        Source::TaskTrailer,
        Source::PromptDigest,
        Source::None,
    ] {
        assert_eq!(
            Source::parse(source.as_str()),
            source,
            "{} does not round-trip",
            source.as_str()
        );
    }
    // A row written by a newer fathom reads as absence, not as a failed open (ADR-004 §9.5).
    assert_eq!(Source::parse("a-rung-not-invented-yet"), Source::None);
    assert_eq!(State::parse("digest-only"), State::Digest);
    assert_eq!(State::parse("nonsense"), State::Absent);
    assert!(Source::HookPrompt.is_stated());
    assert!(!Source::TaskTrailer.is_stated());
    assert!(!Source::PromptDigest.is_stated());
}

// ---------------------------------------------------------------------------
// The caps, and saying when one bit
// ---------------------------------------------------------------------------

#[test]
fn text_is_capped_and_the_cut_is_said_out_loud() {
    let hook = TurnEvidence {
        prompt: Some("x".repeat(200)),
        ..turn()
    };
    let policy = Policy {
        max_text_bytes: 64,
        ..Policy::default()
    };
    let intent = capture(&[commit("c1", &[])], Some(&hook), &policy);
    assert_eq!(
        intent.text.as_deref().expect("text").len(),
        64,
        "the cap is a cap"
    );
    assert!(intent.truncated, "and the document admits it");
    assert!(
        intent.evidence.iter().all(|e| e.detail.len() <= 64),
        "the evidence lines are inside the same cap: {:?}",
        intent.evidence
    );
}

#[test]
fn a_cap_too_small_for_a_word_is_floored_and_the_cut_is_still_clean() {
    // The cap is a UTF-8 cap: a cut mid-emoji is a corrupted document, not a bounded one. And a cap below
    // sixteen bytes is not honoured — a prompt that cannot hold a word is a misconfiguration, and the
    // floor is stated here rather than left to a `Policy` field nobody reads.
    let hook = TurnEvidence {
        prompt: Some("🎯".repeat(40)),
        ..turn()
    };
    let policy = Policy {
        max_text_bytes: 10,
        ..Policy::default()
    };
    let intent = capture(&[commit("c1", &[])], Some(&hook), &policy);
    let text = intent.text.as_deref().expect("text");
    assert_eq!(text.len(), 16, "the floor, not the misconfiguration");
    assert!(
        text.is_char_boundary(text.len()),
        "a cut that splits a character is a corrupted document: {text:?}"
    );
    assert!(intent.truncated);
}

#[test]
fn a_plan_that_overflows_its_cap_says_how_much_was_dropped() {
    let hook = TurnEvidence {
        plan: (0..10)
            .map(|i| PlanItem::new(format!("item {i}")))
            .collect(),
        ..turn()
    };
    let policy = Policy {
        plan_cap: 4,
        ..Policy::default()
    };
    let intent = capture(&[commit("c1", &[])], Some(&hook), &policy);
    assert_eq!(intent.plan.len(), 4);
    assert!(
        intent
            .scope_rejected
            .iter()
            .any(|line| line.contains("plan list truncated to 4 items")),
        "a plan that silently ends is a plan that was not finished: {:?}",
        intent.scope_rejected
    );
}

// ---------------------------------------------------------------------------
// Declared scope
// ---------------------------------------------------------------------------

#[test]
fn declared_scope_is_normalised_sorted_and_deduplicated() {
    let (kept, rejected) = normalise_scope(
        &[
            " src/parser/** ".to_owned(),
            "./src/parser/**".to_owned(),
            "\"docs/*.md\"".to_owned(),
            "docs/*.md".to_owned(),
            "".to_owned(),
        ],
        10,
    );
    assert_eq!(kept, vec!["docs/*.md", "src/parser/**"]);
    assert!(
        rejected.is_empty(),
        "nothing here is dangerous: {rejected:?}"
    );
}

#[test]
fn a_scope_that_escapes_the_repository_is_refused_by_name() {
    // A refusal folded into an empty scope would read as "nothing was declared", which is a different —
    // and much more reassuring — fact than somebody asking for `/`.
    let (kept, rejected) = normalise_scope(
        &[
            "/".to_owned(),
            "../secrets".to_owned(),
            "~/private".to_owned(),
            "src\\app.rs".to_owned(),
            "src/a\0b".to_owned(),
            "src/ok.rs".to_owned(),
        ],
        10,
    );
    assert_eq!(kept, vec!["src/ok.rs"]);
    assert_eq!(
        rejected,
        vec!["/", "../secrets", "~/private", "src\\app.rs", "src/a\0b"]
    );
}

#[test]
fn a_scope_over_its_cap_lands_in_the_refusals_not_in_the_bin() {
    let many: Vec<String> = (0..6).map(|i| format!("src/{i}.rs")).collect();
    let (kept, rejected) = normalise_scope(&many, 3);
    assert_eq!(kept, vec!["src/0.rs", "src/1.rs", "src/2.rs"]);
    assert_eq!(rejected.len(), 3, "counted, not lost: {rejected:?}");
}

#[test]
fn scope_comes_from_the_hook_when_the_hook_stated_one() {
    let hook = TurnEvidence {
        scope: vec!["src/parser/**".to_owned()],
        ..turn()
    };
    let commits = vec![commit("c1", &[("Scope", "docs/**")])];
    let intent = intent_of(&commits, Some(&hook));
    assert_eq!(intent.scope, vec!["src/parser/**"]);
    assert_eq!(
        intent.state, "none",
        "a declared scope is not an intent, and does not make one"
    );
}

#[test]
fn a_trailer_scope_is_split_into_globs_and_refused_in_public() {
    let commits = vec![commit(
        "c1",
        &[("Scope", "src/parser/**, /etc/passwd; docs/*.md")],
    )];
    let intent = intent_of(&commits, None);
    assert_eq!(intent.scope, vec!["docs/*.md", "src/parser/**"]);
    assert_eq!(intent.scope_rejected, vec!["/etc/passwd"]);
}

// ---------------------------------------------------------------------------
// The document itself
// ---------------------------------------------------------------------------

#[test]
fn the_search_body_holds_the_text_the_plan_and_the_scope() {
    // `e5.2`'s search reads this body, not the winning line: the plan is where a harness usually *puts*
    // the intent, and a search that indexed only the rung that won would miss the items that lost.
    let hook = TurnEvidence {
        intent: Some("make the card scroll".to_owned()),
        plan: vec![PlanItem::new("add the pane")],
        scope: vec!["crates/fathom-tui/**".to_owned()],
        ..turn()
    };
    let intent = intent_of(&[commit("c1", &[])], Some(&hook));
    let body = intent.search_body();
    for needle in [
        "make the card scroll",
        "add the pane",
        "crates/fathom-tui/**",
    ] {
        assert!(body.contains(needle), "{needle} is missing from {body:?}");
    }
}

#[test]
fn the_document_round_trips_through_the_store_column() {
    let hook = TurnEvidence {
        prompt: Some("the ask".to_owned()),
        plan: vec![PlanItem::new("a step")],
        scope: vec!["src/**".to_owned()],
        ..turn()
    };
    let intent = intent_of(&[commit("c1", &[])], Some(&hook));
    let text = intent.to_json();
    let again = Intent::from_json(&text).expect("the document reads back");
    assert_eq!(again, intent);
    assert_eq!(again.schema, "fathom-intent/1");
    assert_eq!(again.state_of(), State::Recorded);
    // An empty column is `None` — "never written" — which is not the same as an `absent` document.
    assert!(Intent::from_json("").is_none());
    assert!(Intent::from_json("   ").is_none());
    assert!(Intent::from_json("{ not json").is_none());
    let absent = Intent::absent("nothing was said");
    assert_eq!(absent.state, State::Absent.as_str());
    assert_eq!(absent.confidence, 0.0);
    assert_eq!(absent.reason.as_deref(), Some("nothing was said"));
}

// ---------------------------------------------------------------------------
// Reading a vendor payload
// ---------------------------------------------------------------------------

#[test]
fn a_vendor_payload_is_read_in_whatever_shape_it_arrives() {
    use serde_json::json;
    let payload = json!({
        "prompt": "  make the exporter skip noise  ",
        "prompt_sha256": "d".repeat(64),
        "prompt_chars": 26,
        "plan_items": [
            {"text": "read the diff", "status": "completed"},
            {"content": "write the card"},
            "a bare string item"
        ],
        "declared_scope": "src/parser/**, docs/*.md",
    });
    let evidence = TurnEvidence::from_payload(&payload, 4096);
    assert_eq!(
        evidence.prompt.as_deref(),
        Some("make the exporter skip noise"),
        "trimmed, not padded"
    );
    assert_eq!(evidence.prompt_chars, Some(26));
    assert_eq!(evidence.plan.len(), 3);
    assert_eq!(evidence.plan[0].status.as_deref(), Some("completed"));
    assert_eq!(evidence.plan[1].text, "write the card");
    assert_eq!(evidence.scope, vec!["src/parser/**", "docs/*.md"]);

    let intent = capture(&[commit("c1", &[])], Some(&evidence), &Policy::default());
    assert_eq!(source(&intent), "hook-prompt");
    assert_eq!(
        intent
            .plan
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>(),
        vec!["read the diff", "write the card", "a bare string item"]
    );
}

#[test]
fn a_malformed_payload_field_is_no_intent_not_a_failed_run() {
    use serde_json::json;
    // Numbers where text belongs, an object where a list belongs: none of these is a reason to stop, and
    // every one of them is a state the ladder already models.
    let payload = json!({
        "prompt": 42,
        "plan": {"not": "a list"},
        "scope": [1, 2, 3],
        "intent": "   ",
    });
    let evidence = TurnEvidence::from_payload(&payload, 4096);
    assert_eq!(evidence.prompt, None);
    assert!(evidence.plan.is_empty());
    assert!(evidence.scope.is_empty());
    assert_eq!(evidence.intent, None);
    let intent = capture(&[commit("c1", &[])], Some(&evidence), &Policy::default());
    assert_eq!(intent.state, State::Absent.as_str());
}
