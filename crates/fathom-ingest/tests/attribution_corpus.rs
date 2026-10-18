//! The attribution fixture set (`e4.1`, acceptance criterion 1 and 2).
//!
//! Every case here is a shape a real tool writes today, with the spelling it really uses. The gate is the
//! ticket's: **90% or better, per convention as well as overall**, because a classifier that averages 92%
//! by getting one harness perfect and four others wrong is a classifier that will be blamed for the four.
//!
//! What "expected" means here is the *kind* plus, when the evidence names one, the *agent id*. It never
//! means a confidence beyond the rung table: the numbers are asserted once, in
//! [`crate::attribute`'s`] own tests, and a corpus case that pinned them again would be a second copy of
//! the ladder to keep in sync.
//!
//! Criterion 2 — *ambiguous commits are labelled `unknown` with the reason shown* — is the last group:
//! every case expecting `unknown` is also asserted to carry a reason, because a bare `?` is exactly the
//! silent bucket this ticket exists to remove.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use fathom_ingest::attribute::{self, AuthorKind, Policy};
use fathom_ingest::segment::CommitFacts;

/// One fixture: who and what the commit says, and the label that is correct.
struct Case {
    /// The convention group. Cases are scored per group as well as overall.
    convention: &'static str,
    /// What the group is, in the fixture's own words, so a failure prints the claim it disproved.
    ///
    /// `trailer` marks the groups that settle the ticket's "at least six real-world trailer conventions"
    /// count; the rest are the other inputs the ticket names (author shape, committer, harness, message).
    class: &'static str,
    author: &'static str,
    email: &'static str,
    committer: &'static str,
    committer_email: &'static str,
    subject: &'static str,
    trailers: &'static [(&'static str, &'static str)],
    /// The harness session an ADR-005 event gave this commit. The store's trailer-to-field mapping lives
    /// in `fathom-analyze`; a corpus case states the *field* because that is what the classifier reads,
    /// and re-reading trailers here instead would be a second mapping to keep in sync.
    session: Option<&'static str>,
    expected: AuthorKind,
    agent: Option<&'static str>,
}

const CASES: &[Case] = &[
    // ── trailer: `Co-authored-by` naming Claude ────────────────────────────────────────────────
    Case {
        convention: "co-authored-by: claude",
        class: "trailer",
        author: "Daniel Stahl",
        email: "daniel@fathom.dev",
        committer: "Daniel Stahl",
        committer_email: "daniel@fathom.dev",
        subject: "fix: close the torn-write path in the spine",
        trailers: &[("co-authored-by", "Claude <noreply@anthropic.com>")],
        session: None,
        expected: AuthorKind::Agent,
        agent: Some("claude"),
    },
    Case {
        // The `Co-Authored-By` capitalisation is what Claude Code writes; git folds the key either way.
        convention: "co-authored-by: claude",
        class: "trailer",
        author: "Daniel Stahl",
        email: "daniel@fathom.dev",
        committer: "Daniel Stahl",
        committer_email: "daniel@fathom.dev",
        subject: "feat: attribution ladder",
        trailers: &[("co-authored-by", "Claude <noreply@anthropic.com>")],
        session: None,
        expected: AuthorKind::Agent,
        agent: Some("claude"),
    },
    // ── trailer: `Co-authored-by` naming Copilot, including its bot address ─────────────────────
    Case {
        convention: "co-authored-by: copilot",
        class: "trailer",
        author: "Priya Raman",
        email: "priya@acme.co",
        committer: "Priya Raman",
        committer_email: "priya@acme.co",
        subject: "Add retry with backoff to the uploader",
        trailers: &[(
            "co-authored-by",
            "Copilot <28198790+copilot-swe-agent[bot]@users.noreply.github.com>",
        )],
        session: None,
        expected: AuthorKind::Agent,
        agent: Some("copilot"),
    },
    Case {
        convention: "co-authored-by: copilot",
        class: "trailer",
        author: "Priya Raman",
        email: "priya@acme.co",
        committer: "Priya Raman",
        committer_email: "priya@acme.co",
        subject: "Update src/uploader.ts via Copilot",
        trailers: &[],
        session: None,
        expected: AuthorKind::Agent,
        agent: Some("copilot"),
    },
    // ── trailer: `Generated-by`, the convention the ticket names by hand ────────────────────────
    Case {
        convention: "generated-by",
        class: "trailer",
        author: "Tomasz Nowak",
        email: "tomasz@acme.co",
        committer: "Tomasz Nowak",
        committer_email: "tomasz@acme.co",
        subject: "chore: regenerate the client",
        trailers: &[("generated-by", "claude-code:2.0.14")],
        session: None,
        expected: AuthorKind::Agent,
        agent: Some("claude"),
    },
    Case {
        // OpenAI's Codex CLI signs with its own address rather than a co-author line.
        convention: "generated-by",
        class: "trailer",
        author: "Tomasz Nowak",
        email: "tomasz@acme.co",
        committer: "Tomasz Nowak",
        committer_email: "tomasz@acme.co",
        subject: "fix: honour the proxy env pair",
        trailers: &[("generated-by", "openai-codex")],
        session: None,
        expected: AuthorKind::Agent,
        agent: Some("codex"),
    },
    // ── trailer: `Assisted-by`, the spelling fathom's own fixture corpus writes ─────────────────
    Case {
        convention: "assisted-by",
        class: "trailer",
        author: "Fathom Fixture",
        email: "fixture@fathom.dev",
        committer: "Fathom Fixture",
        committer_email: "fixture@fathom.dev",
        subject: "feat: add the third index column",
        trailers: &[("assisted-by", "claude-code")],
        session: None,
        expected: AuthorKind::Agent,
        agent: Some("claude"),
    },
    Case {
        convention: "assisted-by",
        class: "trailer",
        author: "Fathom Fixture",
        email: "fixture@fathom.dev",
        committer: "Fathom Fixture",
        committer_email: "fixture@fathom.dev",
        subject: "fix: widen the hunk cap",
        trailers: &[("assisted-by", "cursor-agent <agent@cursor.com>")],
        session: None,
        expected: AuthorKind::Agent,
        agent: Some("cursor"),
    },
    // ── trailer: the bare `Agent:` key a harness writes when it declares itself ────────────────
    Case {
        convention: "agent",
        class: "trailer",
        author: "Priya Raman",
        email: "priya@acme.co",
        committer: "Priya Raman",
        committer_email: "priya@acme.co",
        subject: "feat: batch the review queue",
        trailers: &[("agent", "openhands <agent@all-hands.dev>")],
        session: None,
        expected: AuthorKind::Agent,
        agent: Some("openhands"),
    },
    Case {
        convention: "agent",
        class: "trailer",
        author: "Priya Raman",
        email: "priya@acme.co",
        committer: "Priya Raman",
        committer_email: "priya@acme.co",
        subject: "fix: the flaky reader",
        trailers: &[("agent", "devin <devin@cognition.ai>")],
        session: None,
        expected: AuthorKind::Agent,
        agent: Some("devin"),
    },
    // ── trailer: `Co-authored-by` naming Cursor / Aider / SWE-agent ────────────────────────────
    Case {
        convention: "co-authored-by: cursor",
        class: "trailer",
        author: "Mei Chen",
        email: "mei@acme.co",
        committer: "Mei Chen",
        committer_email: "mei@acme.co",
        subject: "refactor: pull the parser out",
        trailers: &[("co-authored-by", "Cursor <cursoragent@cursor.com>")],
        session: None,
        expected: AuthorKind::Agent,
        agent: Some("cursor"),
    },
    Case {
        convention: "co-authored-by: aider",
        class: "trailer",
        author: "Mei Chen",
        email: "mei@acme.co",
        committer: "Mei Chen",
        committer_email: "mei@acme.co",
        subject: "Drop the unused dependency",
        trailers: &[("co-authored-by", "aider <aider@aider.chat>")],
        session: None,
        expected: AuthorKind::Agent,
        agent: Some("aider"),
    },
    Case {
        convention: "co-authored-by: swe-agent",
        class: "trailer",
        author: "Mei Chen",
        email: "mei@acme.co",
        committer: "Mei Chen",
        committer_email: "mei@acme.co",
        subject: "fix: bounds check before the copy",
        trailers: &[("co-authored-by", "swe-agent <swe-agent@princeton.edu>")],
        session: None,
        expected: AuthorKind::Agent,
        agent: Some("swe-agent"),
    },
    // ── trailer: a bot's `Signed-off-by` (the DCO line, which humans also sign) ────────────────
    Case {
        convention: "signed-off-by: bot",
        class: "trailer",
        author: "Renovate Bot",
        email: "renovate@whitesourcesoftware.com",
        committer: "Renovate Bot",
        committer_email: "renovate@whitesourcesoftware.com",
        subject: "chore(deps): bump serde to 2.0",
        trailers: &[(
            "signed-off-by",
            "renovate-bot <renovate@whitesourcesoftware.com>",
        )],
        session: None,
        expected: AuthorKind::Agent,
        agent: Some("renovate"),
    },
    Case {
        convention: "signed-off-by: bot",
        class: "trailer",
        author: "CI",
        email: "ci@acme.co",
        committer: "CI",
        committer_email: "ci@acme.co",
        subject: "chore: apply the format job",
        trailers: &[(
            "signed-off-by",
            "github-actions[bot] <41898282+github-actions[bot]@users.noreply.github.com>",
        )],
        session: None,
        expected: AuthorKind::Agent,
        agent: Some("github-actions"),
    },
    // ── author shape: git's `[bot]` convention and agent-named identities ──────────────────────
    Case {
        convention: "author: bot suffix",
        class: "author",
        author: "dependabot[bot]",
        email: "49699333+dependabot[bot]@users.noreply.github.com",
        committer: "dependabot[bot]",
        committer_email: "49699333+dependabot[bot]@users.noreply.github.com",
        subject: "build(deps): bump clap from 4.5 to 4.6",
        trailers: &[],
        session: None,
        expected: AuthorKind::Agent,
        agent: Some("dependabot"),
    },
    Case {
        convention: "author: bot suffix",
        class: "author",
        author: "copilot-swe-agent[bot]",
        email: "28198790+copilot-swe-agent[bot]@users.noreply.github.com",
        committer: "copilot-swe-agent[bot]",
        committer_email: "28198790+copilot-swe-agent[bot]@users.noreply.github.com",
        subject: "Fix the null check in the session guard",
        trailers: &[],
        session: None,
        expected: AuthorKind::Agent,
        agent: Some("copilot"),
    },
    Case {
        convention: "author: agent name",
        class: "author",
        author: "devin-ai-integration[bot]",
        email: "devin-ai-integration[bot]@users.noreply.github.com",
        committer: "devin-ai-integration[bot]",
        committer_email: "devin-ai-integration[bot]@users.noreply.github.com",
        subject: "Add the export endpoint",
        trailers: &[],
        session: None,
        expected: AuthorKind::Agent,
        agent: Some("devin"),
    },
    Case {
        convention: "author: agent name",
        class: "author",
        author: "codex",
        email: "codex@openai.com",
        committer: "codex",
        committer_email: "codex@openai.com",
        subject: "fix: reset the offset on a torn line",
        trailers: &[],
        session: None,
        expected: AuthorKind::Agent,
        agent: Some("codex"),
    },
    // ── committer shape: a bot applied a human-authored change ──────────────────────────────────
    Case {
        convention: "committer: bot",
        class: "committer",
        author: "Priya Raman",
        email: "priya@acme.co",
        committer: "github-actions[bot]",
        committer_email: "41898282+github-actions[bot]@users.noreply.github.com",
        subject: "feat: the flag parser",
        trailers: &[],
        session: None,
        expected: AuthorKind::Agent,
        agent: Some("github-actions"),
    },
    // ── harness: an ADR-005 turn or session names the commit ───────────────────────────────────
    Case {
        convention: "harness: session",
        class: "harness",
        author: "Daniel Stahl",
        email: "daniel@fathom.dev",
        committer: "Daniel Stahl",
        committer_email: "daniel@fathom.dev",
        subject: "feat: the second index column",
        trailers: &[("agent-session", "s1")],
        session: Some("s1"),
        expected: AuthorKind::Agent,
        agent: None,
    },
    // ── message shape: the subject says it in words ────────────────────────────────────────────
    Case {
        convention: "message: states it",
        class: "message",
        author: "Daniel Stahl",
        email: "daniel@fathom.dev",
        committer: "Daniel Stahl",
        committer_email: "daniel@fathom.dev",
        subject: "\u{1f916} Generated with [Claude Code]: fix the panic",
        trailers: &[],
        session: None,
        expected: AuthorKind::Agent,
        agent: Some("claude"),
    },
    Case {
        convention: "message: states it",
        class: "message",
        author: "Mei Chen",
        email: "mei@acme.co",
        committer: "Mei Chen",
        committer_email: "mei@acme.co",
        subject: "Reduce the retry window, generated with aider",
        trailers: &[],
        session: None,
        expected: AuthorKind::Agent,
        agent: Some("aider"),
    },
    // ── human conventions: the shapes that must NOT be read as an agent ─────────────────────────
    Case {
        convention: "human: ordinary commit",
        class: "human",
        author: "Daniel Stahl",
        email: "daniel@fathom.dev",
        committer: "Daniel Stahl",
        committer_email: "daniel@fathom.dev",
        subject: "fix: the typo I could not stand",
        trailers: &[],
        session: None,
        expected: AuthorKind::Human,
        agent: None,
    },
    Case {
        // The DCO line, signed by a person: the key is on the claim list and the value says otherwise.
        convention: "human: signed-off-by",
        class: "human",
        author: "Daniel Stahl",
        email: "daniel@fathom.dev",
        committer: "Daniel Stahl",
        committer_email: "daniel@fathom.dev",
        subject: "feat: the review ledger",
        trailers: &[("signed-off-by", "Daniel Stahl <daniel@fathom.dev>")],
        session: None,
        expected: AuthorKind::Human,
        agent: None,
    },
    Case {
        // Two people, one commit. `Co-authored-by` is not by itself an agent claim.
        convention: "human: co-authored-by",
        class: "human",
        author: "Daniel Stahl",
        email: "daniel@fathom.dev",
        committer: "Daniel Stahl",
        committer_email: "daniel@fathom.dev",
        subject: "feat: pair the exporter",
        trailers: &[("co-authored-by", "Jordan Ellis <jordan@fathom.dev>")],
        session: None,
        expected: AuthorKind::Human,
        agent: None,
    },
    Case {
        convention: "human: assisted-by",
        class: "human",
        author: "Mei Chen",
        email: "mei@acme.co",
        committer: "Mei Chen",
        committer_email: "mei@acme.co",
        subject: "docs: the reviewer's guide",
        trailers: &[("assisted-by", "Jordan Ellis <jordan@fathom.dev>")],
        session: None,
        expected: AuthorKind::Human,
        agent: None,
    },
    Case {
        convention: "human: reviewed-by",
        class: "human",
        author: "Tomasz Nowak",
        email: "tomasz@acme.co",
        committer: "Tomasz Nowak",
        committer_email: "tomasz@acme.co",
        subject: "fix: the off-by-one in the rail",
        trailers: &[("reviewed-by", "Priya Raman <priya@acme.co>")],
        session: None,
        expected: AuthorKind::Human,
        agent: None,
    },
    Case {
        // A person who works *at* a hosting name that happens to be a marker word is not a marker.
        convention: "human: subject mentions agents",
        class: "human",
        author: "Tomasz Nowak",
        email: "tomasz@acme.co",
        committer: "Tomasz Nowak",
        committer_email: "tomasz@acme.co",
        subject: "docs: how to review an agent's commit",
        trailers: &[],
        session: None,
        expected: AuthorKind::Human,
        agent: None,
    },
    // ── ambiguous: `unknown`, with the reason printed (criterion 2) ─────────────────────────────
    Case {
        // The only marker hit is inside a domain: `assistant.fr` is a hosting name, not an assistant.
        convention: "ambiguous: marker in a domain",
        class: "ambiguous",
        author: "Jane Whitlock",
        email: "jane@assistant.fr",
        committer: "Jane Whitlock",
        committer_email: "jane@assistant.fr",
        subject: "fix: the retry window",
        trailers: &[],
        session: None,
        expected: AuthorKind::Unknown,
        agent: None,
    },
    Case {
        convention: "ambiguous: no identity",
        class: "ambiguous",
        author: "",
        email: "",
        committer: "",
        committer_email: "",
        subject: "commit from a stripped header",
        trailers: &[],
        session: None,
        expected: AuthorKind::Unknown,
        agent: None,
    },
];

fn facts(case: &Case) -> CommitFacts {
    CommitFacts {
        oid: "0".repeat(40),
        author_name: case.author.to_owned(),
        author_email: case.email.to_owned(),
        committer_name: case.committer.to_owned(),
        committer_email: case.committer_email.to_owned(),
        subject: case.subject.to_owned(),
        session_id: case.session.map(str::to_owned),
        trailers: case
            .trailers
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect::<BTreeMap<_, _>>(),
        ..CommitFacts::default()
    }
}

/// Every group's accuracy, plus the overall figure, as the test prints them.
#[test]
fn the_fixture_set_is_classified_at_ninety_percent_or_better() {
    let policy = Policy::default();
    let mut wrong: Vec<String> = Vec::new();
    let mut groups: Vec<(&str, usize, usize)> = Vec::new(); // (group, right, seen)

    for case in CASES {
        let got = attribute::attribute_one(&facts(case), &policy);
        let mut bad: Option<String> = None;
        if got.kind != case.expected {
            bad = Some(format!(
                "{} [{}/{}]: expected {}, got {} ({}) — evidence {}",
                case.convention,
                case.class,
                case.author,
                case.expected,
                got.kind,
                got.reason.as_deref().unwrap_or("no reason given"),
                got.evidence_line()
            ));
        } else if case.agent.is_some() && got.agent.as_deref() != case.agent {
            bad = Some(format!(
                "{} [{}]: expected the agent {:?}, got {:?} — evidence {}",
                case.convention,
                case.class,
                case.agent,
                got.agent,
                got.evidence_line()
            ));
        }
        let right = bad.is_none();
        if let Some(bad) = bad {
            wrong.push(bad);
        }
        if let Some(group) = groups
            .iter_mut()
            .find(|(name, _, _)| *name == case.convention)
        {
            group.1 += usize::from(right);
            group.2 += 1;
        } else {
            groups.push((case.convention, usize::from(right), 1));
        }
    }

    let report = groups
        .iter()
        .map(|(name, right, seen)| format!("{name} {right}/{seen}"))
        .collect::<Vec<_>>()
        .join("\n  ");
    assert!(
        wrong.is_empty(),
        "{} case(s) misclassified:\n  {}\n\ndid score:\n  {}",
        wrong.len(),
        wrong.join("\n  "),
        report
    );
}

/// The acceptance bar itself, stated as a number rather than as "everything passed".
#[test]
fn every_convention_clears_the_ninety_percent_bar() {
    let policy = Policy::default();
    let mut groups: Vec<(&'static str, usize, usize)> = Vec::new();
    for case in CASES {
        let got = attribute::attribute_one(&facts(case), &policy);
        let right = got.kind == case.expected
            && (case.agent.is_none() || got.agent.as_deref() == case.agent);
        if let Some(entry) = groups
            .iter_mut()
            .find(|(name, _, _)| *name == case.convention)
        {
            entry.1 += usize::from(right);
            entry.2 += 1;
        } else {
            groups.push((case.convention, usize::from(right), 1));
        }
    }

    let total: usize = groups.iter().map(|(_, _, seen)| seen).sum();
    let correct: usize = groups.iter().map(|(_, right, _)| right).sum();
    let overall = correct as f64 / total as f64;
    assert!(
        overall >= 0.90,
        "overall accuracy {overall:.3} is under the 0.90 bar ({correct}/{total})"
    );

    for (name, right, seen) in &groups {
        let score = *right as f64 / *seen as f64;
        assert!(
            score >= 0.90,
            "convention `{name}` scores {score:.3} ({right}/{seen}), under the 0.90 bar the ticket \
             sets per convention — an average that hides one harness nobody can use"
        );
    }

    let trailer_groups: Vec<&&str> = groups
        .iter()
        .filter(|(name, _, _)| {
            CASES
                .iter()
                .any(|case| &case.convention == name && case.class == "trailer")
        })
        .map(|(name, _, _)| name)
        .collect();
    assert!(
        trailer_groups.len() >= 6,
        "the ticket asks for at least six real-world *trailer* conventions; the set names {} — {:?}",
        trailer_groups.len(),
        trailer_groups
    );
}

/// Criterion 2's second half: an `unknown` arrives with the reason shown, never as a bare `?`.
#[test]
fn an_ambiguous_case_comes_with_a_reason_and_never_a_guess() {
    let policy = Policy::default();
    let ambiguous: Vec<&Case> = CASES
        .iter()
        .filter(|case| case.class == "ambiguous")
        .collect();
    assert!(
        ambiguous.len() >= 2,
        "criterion 2 needs ambiguous cases in the set, and it has {}",
        ambiguous.len()
    );
    for case in ambiguous {
        let got = attribute::attribute_one(&facts(case), &policy);
        assert_eq!(
            got.kind,
            AuthorKind::Unknown,
            "`{}` must read unknown",
            case.convention
        );
        let reason = got.reason.as_deref().unwrap_or_else(|| {
            panic!(
                "`{}` printed no reason, which is the silent bucket",
                case.convention
            )
        });
        assert!(
            reason.len() > 20,
            "`{}` printed {reason:?}, which is not a reason a reviewer can act on",
            case.convention
        );
        assert!(
            !got.evidence.is_empty(),
            "`{}` printed no evidence either",
            case.convention
        );
    }
}

/// The confidence ladder, asserted once so the corpus does not have to restate it per case.
#[test]
fn the_ladder_prices_each_rung_the_way_adr003_published() {
    let policy = Policy::default();
    let trailer = Case {
        convention: "ladder",
        class: "trailer",
        author: "Daniel Stahl",
        email: "daniel@fathom.dev",
        committer: "Daniel Stahl",
        committer_email: "daniel@fathom.dev",
        subject: "feat: x",
        trailers: &[("co-authored-by", "Claude <noreply@anthropic.com>")],
        session: None,
        expected: AuthorKind::Agent,
        agent: Some("claude"),
    };
    let author = Case {
        convention: "ladder",
        class: "author",
        author: "codex",
        email: "codex@openai.com",
        committer: "codex",
        committer_email: "codex@openai.com",
        subject: "feat: x",
        trailers: &[],
        session: None,
        expected: AuthorKind::Agent,
        agent: Some("codex"),
    };
    let committer = Case {
        convention: "ladder",
        class: "committer",
        author: "Daniel Stahl",
        email: "daniel@fathom.dev",
        committer: "renovate-bot",
        committer_email: "renovate@whitesourcesoftware.com",
        subject: "feat: x",
        trailers: &[],
        session: None,
        expected: AuthorKind::Agent,
        agent: Some("renovate"),
    };
    let human = Case {
        convention: "ladder",
        class: "human",
        author: "Daniel Stahl",
        email: "daniel@fathom.dev",
        committer: "Daniel Stahl",
        committer_email: "daniel@fathom.dev",
        subject: "feat: x",
        trailers: &[],
        session: None,
        expected: AuthorKind::Human,
        agent: None,
    };

    assert_eq!(
        attribute::attribute_one(&facts(&trailer), &policy).confidence,
        attribute::CONFIDENCE_TRAILER,
        "ADR-003 §8 H4: trailer evidence prices at 0.95"
    );
    assert_eq!(
        attribute::attribute_one(&facts(&author), &policy).confidence,
        attribute::CONFIDENCE_TRAILER,
        "a registry entry is declared evidence wherever it appears, so it rides the 0.95 rung \
         rather than the 0.85 one a bare marker earns"
    );
    assert_eq!(
        attribute::attribute_one(&facts(&committer), &policy).confidence,
        attribute::CONFIDENCE_TRAILER
    );
    let plain = attribute::attribute_one(&facts(&human), &policy);
    assert_eq!(
        plain.confidence,
        attribute::CONFIDENCE_IDENTITY,
        "ADR-003 §8 H4: a real identity with no agent evidence is 0.50, 'probably human, do not \
         bet on it'"
    );
    assert_eq!(plain.reason, None, "and it is a verdict, not an apology");

    // The 0.85 rung is a marker hit that is *not* in the registry: a name that reads as an agent.
    let marker = CommitFacts {
        author_name: "llm-runner".to_owned(),
        author_email: "runner@acme.co".to_owned(),
        subject: "fix: x".to_owned(),
        ..CommitFacts::default()
    };
    let got = attribute::attribute_one(&marker, &policy);
    assert_eq!(got.kind, AuthorKind::Agent);
    assert_eq!(got.confidence, attribute::CONFIDENCE_AUTHOR);

    // A marker a repository configures is worth the rung the table gives it, and it reaches the
    // classifier through `Policy`, not through a second compiled-in copy.
    let mut configured = Policy::default();
    configured.markers.push("myloop".to_owned());
    let mine = CommitFacts {
        author_name: "myloop".to_owned(),
        author_email: "myloop@example.com".to_owned(),
        subject: "fix: x".to_owned(),
        ..CommitFacts::default()
    };
    assert_eq!(
        attribute::attribute_one(&mine, &Policy::default()).kind,
        AuthorKind::Human,
        "before the repository names it, nothing does — which is why the notice says what was configured"
    );
    assert_eq!(
        attribute::attribute_one(&mine, &configured).kind,
        AuthorKind::Agent,
        "after it does, the classifier sees it"
    );
}

/// `attribution.ambiguous_below` is the cautious repository's dial: a weak claim becomes `unknown`
/// with the bar printed, rather than a claim nobody asked for.
#[test]
fn a_repository_can_refuse_to_be_told_a_weak_claim() {
    let policy = Policy {
        ambiguous_below: 0.8,
        ..Policy::default()
    };
    let case = &CASES
        .iter()
        .find(|case| case.convention == "human: ordinary commit")
        .expect("the plain human case is in the set");
    let got = attribute::attribute_one(&facts(case), &policy);
    assert_eq!(got.kind, AuthorKind::Unknown);
    let reason = got.reason.expect("a demotion says which bar demoted it");
    assert!(
        reason.contains("ambiguous_below") && reason.contains("0.80"),
        "{reason:?} does not name the bar"
    );
}

/// Two agents both claiming a commit is an answered question with an unanswered one next to it:
/// the kind stands, the identity does not, and the reason says which two it would have had to pick
/// between.
#[test]
fn two_agents_claiming_one_commit_names_the_doubt_instead_of_picking_one() {
    let commit = CommitFacts {
        author_name: "Mei Chen".to_owned(),
        author_email: "mei@acme.co".to_owned(),
        subject: "feat: the whole thing".to_owned(),
        trailers: [
            (
                "co-authored-by".to_owned(),
                "Claude <noreply@anthropic.com>".to_owned(),
            ),
            (
                "assisted-by".to_owned(),
                "aider <aider@aider.chat>".to_owned(),
            ),
        ]
        .into_iter()
        .collect(),
        ..CommitFacts::default()
    };
    let got = attribute::attribute_one(&commit, &Policy::default());
    assert_eq!(got.kind, AuthorKind::Agent, "an agent did write this");
    assert_eq!(got.agent, None, "and which one is not known");
    let reason = got
        .reason
        .expect("the reason for the missing agent is printed");
    assert!(
        reason.contains("claude") && reason.contains("aider"),
        "{reason:?} does not name the two claimants"
    );
}

/// A turn is one attribution, so every member's evidence counts — the reason the classifier reads
/// the episode and not just its head commit.
#[test]
fn one_quiet_commit_does_not_outvote_one_statement() {
    let quiet = CommitFacts {
        oid: "a".repeat(40),
        author_name: "Daniel Stahl".to_owned(),
        author_email: "daniel@fathom.dev".to_owned(),
        subject: "feat: the first half".to_owned(),
        ..CommitFacts::default()
    };
    let stated = CommitFacts {
        oid: "b".repeat(40),
        author_name: "Daniel Stahl".to_owned(),
        author_email: "daniel@fathom.dev".to_owned(),
        subject: "feat: the second half".to_owned(),
        trailers: [(
            "co-authored-by".to_owned(),
            "Claude <noreply@anthropic.com>".to_owned(),
        )]
        .into_iter()
        .collect(),
        ..CommitFacts::default()
    };
    let got = attribute::attribute(&[quiet.clone(), stated.clone()], &Policy::default());
    assert_eq!(got.kind, AuthorKind::Agent);
    assert_eq!(got.agent.as_deref(), Some("claude"));
    assert_eq!(
        got.confidence,
        attribute::CONFIDENCE_TRAILER,
        "one statement outranks two silences (ADR-003's stated-beats-inferred argument)"
    );
    assert!(
        got.evidence_line().contains(&"b".repeat(7)),
        "with more than one member the evidence says which commit carried it: {}",
        got.evidence_line()
    );
    // The head commit alone would have read human; that is the bug this roll-up kills.
    assert_eq!(
        attribute::attribute_one(&quiet, &Policy::default()).kind,
        AuthorKind::Human
    );
}

/// The signature is what a correction keys on, so it has to name every identity a user could mean.
#[test]
fn the_signature_lists_every_identity_a_correction_could_key_on() {
    let commit = CommitFacts {
        author_name: "Daniel Stahl".to_owned(),
        author_email: "daniel@fathom.dev".to_owned(),
        committer_name: "github-actions[bot]".to_owned(),
        committer_email: "41898282+github-actions[bot]@users.noreply.github.com".to_owned(),
        subject: "feat: x".to_owned(),
        trailers: [(
            "co-authored-by".to_owned(),
            "Claude <noreply@anthropic.com>".to_owned(),
        )]
        .into_iter()
        .collect(),
        ..CommitFacts::default()
    };
    let keys = attribute::signatures(&[commit]);
    assert_eq!(
        keys,
        vec![
            "identity:41898282+github-actions[bot]@users.noreply.github.com",
            "identity:daniel@fathom.dev",
            "identity:noreply@anthropic.com",
        ],
        "the author, the committer and the co-author, in a stable order"
    );
}
