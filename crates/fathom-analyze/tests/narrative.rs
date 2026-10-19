//! The episode narrative's acceptance criteria (`e4.7`, `reco-2rx`).
//!
//! Four sentences came off the ticket and each is proved here in the order it was written:
//!
//! * **byte-deterministic, no network** — composed twice, through a JSON round-trip, and compared byte for
//!   byte over a corpus of six documents. No clock, no locale, no provider: the module has no I/O to opt out
//!   of, and the absence is asserted as a fact about the source ([`no_model_path_is_wired_into_the_prose`]);
//! * **every claim carries a resolvable evidence reference** — [`Narrative::verify`] resolves each citation
//!   against the delta document itself, and the tamper tests show the auditor is awake rather than tidy;
//! * **the opt-in LLM path is watermarked** — `s5` decided it does not ship (`reco-nv0`), so what is proved
//!   here is the decision and the stamp: [`llm::SHIPPED`] is false, the gates are quoted, and no code path
//!   reaches an HTTP client from this crate;
//! * **the narrative is the text the export uses** — [`crate::export::narrative_markdown`] is pinned to the
//!   same bytes as [`Narrative::to_markdown`].
//!
//! Plus the three things spike `s5` said the template was bad at, each pinned so it cannot come back: the
//! order is by importance and not by document order, nothing is said twice, and a gap is phrased as what the
//! reader cannot do.
//!
//! The golden file at `tests/golden/narrative/measured.md` is the reviewable form of all of it: re-bless
//! with `FATHOM_GOLDEN=write` and read the diff.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use fathom_analyze::export;
use fathom_analyze::insight::{
    self, AttributionEvidence, AttributionRef, EpisodeDelta, EpisodeRef, FileDelta, Input,
    Provenance, RiskView, Supplied, SuppliedBlock,
};
use fathom_analyze::narrative::{self, Narrative, Options, PRIORITY_FLOOR, TOP_MAX, llm};
use fathom_analyze::risk::{FileChange, Signals, rank};
use fathom_ingest::intent::{self, Intent, PlanItem, PromptRecord};

/// Golden-file mode, spelled the way `e4.6` spells it.
fn bless() -> bool {
    std::env::var("FATHOM_GOLDEN").is_ok_and(|value| value == "write")
}

/// The corpus. Six documents, deliberately uneven: an empty episode, a ranked one, one with rows, one with
/// a capped read, one where the analyses were lent, and one with intent, attribution and findings — because
/// a template that only works on the rich case is a template that hides the poor one.
fn corpus() -> Vec<(&'static str, EpisodeDelta)> {
    let mut corpus: Vec<(&'static str, EpisodeDelta)> = vec![
        (
            "empty",
            EpisodeDelta::sparse("e-aaaa1111", "open the store read-only", None),
        ),
        (
            "ranked",
            EpisodeDelta::sparse(
                "e-bbbb2222",
                "make the epoch cache aware of a rebuilt analyzer",
                Some(RiskView::of(&ranking(), Some(2), Some(9))),
            ),
        ),
    ];

    let mut input = Input::new(EpisodeRef::sparse(
        "e-dddd4444",
        "split the ingest worker pool so a slow clone cannot starve the rail",
    ));
    input.totals.n_files = 5;
    input.totals.n_add = 552;
    input.totals.n_del = 105;
    input.totals.n_binary = 1;
    input.episode.n_commits = 4;
    input.episode.author_kind = "agent".to_owned();
    input.episode.boundary_rule = "turn-trailer".to_owned();
    input.episode.boundary_confidence = Some(0.9);
    input.episode.session = Some("ses-1".to_owned());
    input.episode.turn = Some("ses-1:t3".to_owned());
    input.episode.review = "unread".to_owned();
    input.episode.attribution = Some(AttributionRef {
        kind: "agent".to_owned(),
        source: "trailer".to_owned(),
        agent: Some("looprs".to_owned()),
        confidence: 0.9,
        reason: None,
        evidence: vec![AttributionEvidence {
            source: "trailer".to_owned(),
            detail: "Co-authored-by trailer names looprs".to_owned(),
            confidence: 0.9,
        }],
    });
    input.episode.intent = Some(recorded_intent());
    input.files = rows();
    input.risk = Some(RiskView::of(&ranking(), Some(2), Some(9)));
    let measured = insight::compose(&input);
    corpus.push(("measured", measured));

    let mut capped = input.clone();
    capped.totals.n_files = 400;
    capped.totals.files_truncated = true;
    capped.provenance.files_detail = Provenance::DETAIL_ROLLUP.to_owned();
    corpus.push(("truncated-read", insight::compose(&capped)));

    let mut lent = input.clone();
    lent.supplied = Supplied {
        dependencies: Some(SuppliedBlock {
            headline: "2 added, 1 removed, 3 bumped".to_owned(),
            basis: "resolved imports".to_owned(),
            figures: BTreeMap::from([
                ("added".to_owned(), "2 crates".to_owned()),
                ("bumped".to_owned(), "3 crates".to_owned()),
            ]),
            confidence: Some(0.9),
            owner: "e2.6".to_owned(),
        }),
        tests: Some(SuppliedBlock {
            headline: "1 case deleted, none added".to_owned(),
            basis: "test names".to_owned(),
            figures: BTreeMap::from([("deleted".to_owned(), "1 case".to_owned())]),
            confidence: None,
            owner: "e2.9".to_owned(),
        }),
        blast: Some(SuppliedBlock {
            headline: "4 modules reach a changed symbol".to_owned(),
            basis: "module graph".to_owned(),
            figures: BTreeMap::from([("dependents".to_owned(), "4 modules".to_owned())]),
            confidence: Some(0.75),
            owner: "e3.2".to_owned(),
        }),
        surface: None,
        structure: Some(SuppliedBlock {
            headline: "one module split, one flatten".to_owned(),
            basis: "path shape".to_owned(),
            figures: BTreeMap::new(),
            confidence: None,
            owner: "e2.3".to_owned(),
        }),
        findings: vec![
            insight::Finding {
                kind: "secret-string".to_owned(),
                subject: "crates/fathom-store/src/pool.rs:88".to_owned(),
                message: "a 40-character hex literal that no config key supplies".to_owned(),
                severity: "high".to_owned(),
                evidence: insight::Evidence::external(
                    "crates/fathom-store/src/pool.rs:88".to_owned(),
                ),
                owner: "e4.3".to_owned(),
            },
            insight::Finding {
                kind: "untested-addition".to_owned(),
                subject: "crates/fathom-store/src/pool.rs".to_owned(),
                message: "312 lines added with no test file in the episode".to_owned(),
                severity: "notable".to_owned(),
                evidence: insight::Evidence::inside("#/files".to_owned()),
                owner: "e4.3".to_owned(),
            },
        ],
    };
    corpus.push(("lent", insight::compose(&lent)));

    // The quiet case: no rows, no ranking, no findings. It must produce a narrative that says so, and it
    // must not invent a "read this first" paragraph out of nothing.
    let mut quiet = Input::new(EpisodeRef::sparse(
        "e-eeee5555",
        "typo: fix the em-dash in the CHANGELOG header",
    ));
    quiet.episode.boundary_rule = "gap-3h".to_owned();
    quiet.totals.n_files = 1;
    quiet.totals.n_add = 1;
    quiet.totals.n_del = 1;
    quiet.files = vec![FileDelta {
        path: "CHANGELOG.md".to_owned(),
        old_path: None,
        change: "modified".to_owned(),
        add: 1,
        del: 1,
        binary: false,
        commits: 1,
        path_class: "docs".to_owned(),
        noise_class: "none".to_owned(),
        surface: "docs".to_owned(),
        basis: Provenance::DETAIL_FULL.to_owned(),
    }];
    corpus.push(("quiet", insight::compose(&quiet)));
    corpus
}

fn rows() -> Vec<FileDelta> {
    [
        (
            "src/store/epoch.rs",
            412,
            90,
            false,
            "source",
            "none",
            "modified",
        ),
        (
            "tests/store/epoch.rs",
            88,
            12,
            false,
            "test",
            "none",
            "modified",
        ),
        (
            "migrations/0007.sql",
            40,
            0,
            false,
            "migration",
            "none",
            "added",
        ),
        (
            "assets/blob.bin",
            0,
            0,
            true,
            "binary",
            "generated",
            "modified",
        ),
        (
            "src/pool/worker.rs",
            12,
            3,
            false,
            "source",
            "none",
            "renamed",
        ),
    ]
    .iter()
    .map(|(path, add, del, binary, class, noise, change)| FileDelta {
        path: (*path).to_owned(),
        old_path: (*change == "renamed").then(|| "src/worker.rs".to_owned()),
        change: (*change).to_owned(),
        add: i64::from(*add),
        del: i64::from(*del),
        binary: *binary,
        commits: 1,
        path_class: (*class).to_owned(),
        noise_class: (*noise).to_owned(),
        surface: fathom_analyze::risk::classify(path, class, noise)
            .as_str()
            .to_owned(),
        basis: Provenance::DETAIL_FULL.to_owned(),
    })
    .collect()
}

fn ranking() -> fathom_analyze::risk::Risk {
    let files = rows();
    let changes: Vec<FileChange<'_>> = files
        .iter()
        .map(|file| FileChange {
            path: &file.path,
            add: u64::try_from(file.add).unwrap_or(0),
            del: u64::try_from(file.del).unwrap_or(0),
            binary: file.binary,
            path_class: &file.path_class,
            noise_class: &file.noise_class,
        })
        .collect();
    rank(&Signals {
        files: &changes,
        n_add: 552,
        n_del: 105,
        n_files: 5,
        n_binary: 1,
        n_commits: 4,
        author: "agent",
        partial_files: false,
    })
}

fn recorded_intent() -> Intent {
    Intent {
        schema: intent::INTENT_SCHEMA.to_owned(),
        state: intent::State::Recorded.as_str().to_owned(),
        text: Some("split the worker pool so a slow clone cannot starve the rail".to_owned()),
        source: intent::Source::HookPrompt.as_str().to_owned(),
        confidence: intent::CONFIDENCE_PROMPT,
        session_id: Some("ses-1".to_owned()),
        turn_id: Some("ses-1:t3".to_owned()),
        prompt: Some(PromptRecord {
            sha256: Some("b3c1".to_owned()),
            chars: Some(74),
            text_captured: true,
        }),
        plan: vec![
            PlanItem::new("move the clone call off the UI thread"),
            PlanItem {
                text: "add a regression test for the starvation case".to_owned(),
                status: Some("pending".to_owned()),
            },
        ],
        scope: vec!["src/pool/**".to_owned(), "tests/pool/**".to_owned()],
        evidence: vec![],
        reason: None,
        truncated: false,
        scope_rejected: vec![],
    }
}

// --- acceptance criterion 1: byte-deterministic, and no network in the path -------------------------

#[test]
fn the_same_document_renders_the_same_bytes_every_time() {
    for (name, delta) in corpus() {
        let first = narrative::compose(&delta, &Options::new());
        let second = narrative::compose(&delta, &Options::new());
        assert_eq!(
            first.to_markdown(),
            second.to_markdown(),
            "{name}: markdown moved between two compositions of one document"
        );
        assert_eq!(first.to_text(), second.to_text(), "{name}: text moved");
        assert_eq!(first.to_json(), second.to_json(), "{name}: json moved");

        // Through the wire and back: a re-parsed narrative renders identically, which is what lets `e5.7`
        // bless an artefact it did not compose itself.
        let round = Narrative::from_json(&first.to_json()).expect("round-trip");
        assert_eq!(
            round.to_markdown(),
            first.to_markdown(),
            "{name}: the artefact would change by re-serialising alone"
        );
        assert_eq!(round, first, "{name}: the payload is lossy");
    }
}

#[test]
fn a_composition_is_the_same_through_every_option_it_advertises() {
    let (_, delta) = corpus()
        .into_iter()
        .find(|(name, _)| *name == "lent")
        .expect("the lent fixture exists");
    let wide = narrative::compose(&delta, &Options::new().with_top(TOP_MAX));
    let shipped = narrative::compose(&delta, &Options::new());
    // More slots can only mean more promoted claims, never fewer claims overall.
    assert!(wide.claims.len() >= shipped.claims.len());
    assert!(!wide.claims.is_empty());
}

#[test]
#[allow(clippy::assertions_on_constants)]
fn no_model_path_is_wired_into_the_prose() {
    // The decision, as a buildable fact: spike `s5` shipped the template and none of the polish, and the
    // seam must stay unwired until every gate in `reco-nv0` is cleared. Walking the crate's own source is
    // the only honest way to assert an absence.
    assert!(!llm::SHIPPED);
    assert_eq!(llm::GATES.len(), 5);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut walked = 0_usize;
    let mut wired = Vec::new();
    for file in rust_files(&root) {
        let text = std::fs::read_to_string(&file).expect("read the source file");
        walked += 1;
        wired.extend(llm::find_wired_paths(&file.display().to_string(), &text));
    }
    assert!(
        walked > 20,
        "the walk found only {walked} files — is the root right?"
    );
    assert!(
        wired.is_empty(),
        "the LLM narrative seam has consumers: {wired:?}"
    );
}

fn rust_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(rust_files(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    out.sort();
    out
}

// --- acceptance criterion 2: every claim carries a resolvable reference -----------------------------

#[test]
fn every_claim_carries_evidence_and_the_evidence_resolves() {
    for (name, delta) in corpus() {
        let narrative = narrative::compose(&delta, &Options::new());
        assert!(
            !narrative.claims.is_empty(),
            "{name}: a narrative with no claims is a blank, and a blank is a lie"
        );
        assert_eq!(
            narrative.verify(&delta),
            Vec::<String>::new(),
            "{name}: the narrative failed its own audit"
        );
        assert!(narrative.integrity.verified, "{name}: not verified");
        for claim in &narrative.claims {
            assert!(!claim.cites.is_empty(), "{name}: an unattached assertion");
            assert!(
                claim.text.chars().next().is_some_and(char::is_uppercase),
                "{name}: a claim should read as a sentence: {:?}",
                claim.text
            );
        }
        // Every citation of document kind must address something the reader can open.
        for citation in &narrative.citations {
            match &citation.address {
                narrative::Address::Document(pointer) => {
                    assert!(
                        delta.resolves(pointer),
                        "{name}: {pointer} resolves to nothing"
                    );
                }
                narrative::Address::Locator(run) => {
                    assert!(
                        narrative::locator_shaped(run),
                        "{name}: {run} is not a locator"
                    );
                }
                narrative::Address::Ticket(ticket) => {
                    assert!(
                        narrative::plan_key(ticket),
                        "{name}: {ticket} is not a plan key"
                    );
                }
            }
        }
    }
}

#[test]
fn the_auditor_is_awake_not_tidy() {
    // A green audit means nothing until it has been seen to go red. Four tampering, one per load-bearing
    // rule, in the order the checks are written.
    let delta = corpus()
        .into_iter()
        .find(|(name, _)| *name == "lent")
        .map(|(_, delta)| delta)
        .expect("the lent fixture exists");
    let clean = narrative::compose(&delta, &Options::new());
    assert!(clean.verify(&delta).is_empty());

    // A number the citations do not print.
    let mut invented = clean.clone();
    invented.claims[0]
        .text
        .push_str(", and 987 654 lines of it");
    assert!(
        clean
            .verify(&delta)
            .iter()
            .all(|problem| !problem.contains("987"))
    );
    let problems = invented.verify(&delta);
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("asserts `987654`")),
        "the numeral audit missed an invented figure: {problems:?}"
    );

    // A citation that goes nowhere.
    let mut dangling = clean.clone();
    let index = dangling
        .citations
        .iter()
        .position(|citation| matches!(citation.address, narrative::Address::Document(_)))
        .expect("the fixture cites the document");
    dangling.citations[index].address = narrative::Address::Document("#/nope/not/here".to_owned());
    assert!(
        dangling
            .verify(&delta)
            .iter()
            .any(|problem| problem.contains("resolves to nothing"))
    );

    // A sentence with nothing attached.
    let mut unattached = clean.clone();
    unattached.claims[0].cites.clear();
    assert!(
        unattached
            .verify(&delta)
            .iter()
            .any(|problem| problem.contains("unattached assertion"))
    );

    // A citation nobody leans on, and prose verified against a different document.
    let mut decoration = clean.clone();
    decoration.claims.retain(|claim| claim.cites.len() > 1);
    assert!(
        decoration
            .verify(&delta)
            .iter()
            .any(|problem| problem.contains("is cited by no claim"))
    );
    let other = EpisodeDelta::sparse("e-ffff6666", "a different episode", None);
    assert!(
        clean
            .verify(&other)
            .iter()
            .any(|problem| problem.contains("but verified against"))
    );
}

// --- acceptance criterion 3: the watermark, and the decision behind it ------------------------------

#[test]
fn the_stamp_names_the_generator_and_the_evidence_it_copied() {
    let (name, delta) = corpus()
        .into_iter()
        .find(|(name, _)| *name == "measured")
        .expect("the measured fixture exists");
    let narrative = narrative::compose(&delta, &Options::new());
    let markdown = narrative.to_markdown();
    assert!(
        markdown.contains(narrative::GENERATOR),
        "{name}: no watermark"
    );
    assert!(
        markdown.contains(&format!("`{}`", insight::CONTRACT)),
        "{name}: the stamp does not say which evidence it copied"
    );
    assert!(
        markdown.contains(&narrative.integrity_line()),
        "{name}: the verdict is not printed with the prose"
    );
    assert!(markdown.contains("Evidence"), "{name}: no evidence table");
}

#[test]
fn the_llm_path_is_refused_by_name_and_not_by_silence() {
    // The AC asked for a mocked-provider test and a watermark on the polished text. `s5` decided the
    // polished text does not ship, so what ships in its place is a refusal a surface can print and five
    // gates a reviewer can hold the next proposal against.
    assert_eq!(llm::RESERVED_FEATURE, "net-llm");
    assert_eq!(llm::RESERVED_KEY, "privacy.llm");
    let state = llm::state();
    assert_eq!(
        state
            .iter()
            .find(|(key, _)| *key == "llm_narrative")
            .map(|(_, value)| *value),
        Some("not shipped")
    );
    assert_eq!(llm::rejection_reasons().len(), llm::GATES.len());
    assert!(llm::REFUSAL.contains("s5"));
}

// --- acceptance criterion 4: one text for the screen and the artefact --------------------------------

#[test]
fn the_export_body_is_the_narrative_and_nothing_else() {
    for (name, delta) in corpus() {
        let narrative = narrative::compose(&delta, &Options::new());
        assert_eq!(
            export::narrative_markdown(&delta),
            narrative.to_markdown(),
            "{name}: the export and the screen would print two different texts"
        );
        assert_eq!(
            export::narrative_text(&delta),
            narrative.to_text(),
            "{name}: the plain-text export disagrees with the terminal"
        );
    }
}

// --- the three things spike `s5` measured the template doing badly -----------------------------------

#[test]
fn the_order_is_by_importance_and_not_by_document_order() {
    let delta = corpus()
        .into_iter()
        .find(|(name, _)| *name == "lent")
        .map(|(_, delta)| delta)
        .expect("the lent fixture exists");
    let narrative = narrative::compose(&delta, &Options::new());
    // The `high` finding outranks everything above a `basis` row in the same paragraph, whatever order the
    // document put them in.
    let position = |needle: &str| {
        narrative
            .claims
            .iter()
            .position(|claim| claim.text.contains(needle))
    };
    let finding = position("`secret-string`").expect("the finding is in the narrative");
    let basis = position("Basis:").unwrap_or(usize::MAX);
    assert!(
        finding < basis,
        "document order leaked through: {:?}\n{:?}",
        finding,
        narrative.explain_lines()
    );
    // Within every paragraph the weights do not go up.
    let mut by_section: BTreeMap<&str, Vec<i64>> = BTreeMap::new();
    for claim in &narrative.claims {
        by_section
            .entry(&claim.section)
            .or_default()
            .push(claim.weight);
    }
    for (section, weights) in by_section {
        let ranked: Vec<bool> = weights
            .iter()
            .scan(i64::MAX, |floor, weight| {
                let ok = *weight <= *floor;
                *floor = (*floor).min(*weight);
                Some(ok)
            })
            .collect();
        assert!(
            ranked.iter().all(|ok| *ok),
            "`{section}` is not ranked: {weights:?}"
        );
    }
}

#[test]
fn nothing_is_said_twice_and_the_read_first_paragraph_is_not_repeated() {
    let delta = corpus()
        .into_iter()
        .find(|(name, _)| *name == "lent")
        .map(|(_, delta)| delta)
        .expect("the lent fixture exists");
    let narrative = narrative::compose(&delta, &Options::new());
    let promoted: Vec<&str> = narrative
        .claims
        .iter()
        .filter(|claim| claim.section == narrative::HEADLINE)
        .map(|claim| claim.text.as_str())
        .collect();
    assert!(!promoted.is_empty(), "the rich fixture earns a headline");
    for text in &promoted {
        assert_eq!(
            narrative
                .claims
                .iter()
                .filter(|claim| claim.text == *text)
                .count(),
            1,
            "a promoted claim was relayed a second time: {text}"
        );
    }
    // The pair the document states twice is folded into one sentence that cites both.
    let churn_lines: Vec<&String> = narrative
        .claims
        .iter()
        .filter(|claim| claim.text.contains("with total churn"))
        .map(|claim| &claim.text)
        .collect();
    assert_eq!(
        churn_lines.len(),
        1,
        "size got printed {churn_lines:?} times instead of once"
    );
    assert_eq!(
        narrative
            .claims
            .iter()
            .find(|claim| claim.text.contains("with total churn"))
            .map(|claim| claim.cites.len()),
        Some(2),
        "the folded sentence must cite both figures it folded"
    );
    // The blind spots live in the gap paragraph, not in the attention one.
    assert!(
        !narrative
            .claims
            .iter()
            .any(|claim| claim.section == "attention" && claim.text.contains("cannot see")),
        "the blind-spot list is a limit on the reader's view, so it belongs to the gap paragraph"
    );
}

#[test]
fn a_quiet_episode_prints_no_headline_and_says_what_it_cannot_tell() {
    let delta = corpus()
        .into_iter()
        .find(|(name, _)| *name == "quiet")
        .map(|(_, delta)| delta)
        .expect("the quiet fixture exists");
    let narrative = narrative::compose(&delta, &Options::new());
    assert!(
        narrative
            .claims
            .iter()
            .all(|claim| claim.weight < PRIORITY_FLOOR),
        "a one-line documentation fix must not be dressed up: {:?}",
        narrative.explain_lines()
    );
    assert!(
        !narrative
            .claims
            .iter()
            .any(|claim| claim.section == narrative::HEADLINE)
    );
    let markdown = narrative.to_markdown();
    assert!(
        markdown.contains(narrative::GAP_TITLE),
        "gaps are what the reader cannot do, and that paragraph is never empty here"
    );
    assert!(markdown.contains("You cannot tell"));
    // Every gap's owning plan key is printed, because a refusal is a pointer.
    for gap in &delta.gaps {
        assert!(
            markdown.contains(&gap.owner),
            "the narrative hid who owns `{}`",
            gap.section
        );
    }
}

#[test]
fn the_intent_and_the_attribution_are_quoted_not_paraphrased() {
    let delta = corpus()
        .into_iter()
        .find(|(name, _)| *name == "measured")
        .map(|(_, delta)| delta)
        .expect("the measured fixture exists");
    let narrative = narrative::compose(&delta, &Options::new());
    let intent = recorded_intent();
    let text = intent.text.clone().unwrap_or_default();
    let quoted = narrative
        .claims
        .iter()
        .any(|claim| claim.text.contains(&text));
    assert!(
        quoted,
        "the asker's words were rewritten: {:?}",
        narrative.claims
    );
    assert!(
        narrative
            .claims
            .iter()
            .any(|claim| claim.text.contains("looprs"))
    );
    // The plan and the scope are cited as their own rows: they are checked as well as read.
    assert!(
        narrative
            .claims
            .iter()
            .any(|claim| claim.text.contains("src/pool/**"))
    );
    assert!(
        narrative
            .claims
            .iter()
            .any(|claim| claim.text.contains("regression test"))
    );
}

#[test]
fn the_explanation_names_the_weight_behind_the_order() {
    let delta = corpus()
        .into_iter()
        .find(|(name, _)| *name == "lent")
        .map(|(_, delta)| delta)
        .expect("the lent fixture exists");
    let narrative = narrative::compose(&delta, &Options::new());
    let lines = narrative.explain_lines();
    assert_eq!(lines.len(), narrative.claims.len());
    assert!(
        lines[0].starts_with(' ') || lines[0].chars().next().is_some_and(|c| c.is_ascii_digit())
    );
    // The weights are the price list, not a mystery: the highest claim in the narrative is priced.
    let top = narrative
        .claims
        .iter()
        .map(|claim| claim.weight)
        .max()
        .unwrap_or(0);
    assert!(top >= PRIORITY_FLOOR);
}

// --- the golden file ---------------------------------------------------------------------------------

#[test]
fn the_golden_narrative_has_not_moved() {
    let delta = corpus()
        .into_iter()
        .find(|(name, _)| *name == "measured")
        .map(|(_, delta)| delta)
        .expect("the measured fixture exists");
    let rendered = narrative::compose(&delta, &Options::new()).to_markdown();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
        .join("narrative")
        .join("measured.md");
    if bless() {
        std::fs::create_dir_all(path.parent().expect("golden dir")).expect("create golden dir");
        std::fs::write(&path, &rendered).expect("write the golden file");
    }
    let checked_in = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("the golden file at {path:?} is checked in"));
    assert_eq!(
        rendered, checked_in,
        "the narrative moved; re-bless with FATHOM_GOLDEN=write and read the diff"
    );
}
