//! `fathom_analyze::ledger` — the review ledger: the durable record that a human accepted a change
//! (`e4.6`, `reco-rlf`).
//!
//! The thesis's glossary says it in one sentence: *the durable, human-readable, committable record of human
//! dispositions, keyed to episode identity plus a content fingerprint, stored as markdown under
//! `.fathom/reviews/` with a mirrored index so it can travel in a PR*. Everything in this module is one of
//! the nouns in that sentence:
//!
//! * [`entry`] — the decision, the four digests it carries, and the content digest that makes staleness
//!   decidable at all;
//! * [`format`] — the markdown: how it is appended, how it is read back, and what makes it diff-friendly;
//! * [`relink`] — the ladder that re-attaches a decision after the history that carried it was rewritten,
//!   and the verdict (`fresh` / `stale` / `orphan`, plus tampering) that a surface prints;
//! * [`collect`] — the only file in the module that holds a `Store`, and the only one that reads the mirror
//!   or writes it.
//!
//! # Why this is a *durable* record and not a column
//!
//! `review_state` (migration 3) already stores "which episodes has this human marked", and `triage_event`
//! (migration 6) already stores every act with who, when and why. Neither of them is the ledger, and the
//! difference is not accounting tidiness:
//!
//! | | `review_state` | `triage_event` | the ledger |
//! |---|---|---|---|
//! | survives a `rm -rf .fathom/` | no | no | **yes — it is committed** |
//! | travels in the PR | no | no | **yes** |
//! | says what was reviewed *and what it was* | a state | a state | **four digests and the member set** |
//! | can be forged or edited silently | in the store, out of sight | same | **detected: every entry hashes itself** |
//!
//! ADR-004 §10's sentence is the reason this ticket exists at all: *nothing inside `.fathom/` is
//! must-survive by virtue of being there* — class M is must-survive because it is committed. A sign-off that
//! lives only in a SQLite file is one careless `git clean -fdx` from never having happened, and the metric
//! this product is built on ("the human reviewed it") would keep counting it.
//!
//! # The pipeline, end to end
//!
//! ```text
//!   fathom review sign ep-…            fathom review sync
//!        │                                    │
//!        ├─ collect::episode_facts ──┐        ├─ triage acts the queue wrote (e4.5)
//!        ├─ entry::sign              │        ├─ entry::sign per act, deduped by derived id
//!        ├─ format::append_entries  ◄┘        ├─ format::append_entries
//!        │     (file first, fsynced — R8)     │
//!        └────────────────┬────────────────────┘
//!                    collect::refresh
//!        ├─ format::parse every file under .fathom/reviews/   (the file is the truth)
//!        ├─ relink::link against the index's episodes          (four tiers, published prices)
//!        ├─ store::mirror_ledger (drop every row, re-insert)   (§9.1 level 2, as the write path)
//!        ├─ store::record_ledger_rewrites                      (RL5: a re-link is a queryable fact)
//!        └─ relink::assess → the verdict every surface prints
//! ```
//!
//! # What the verdict counts, and where it stops
//!
//! [`relink::assess`] answers one question — *is every change here accepted by a human, and is what they
//! accepted still what is here?* — and it is careful about the denominator. An episode whose every path is
//! under `.fathom/` is excluded: that namespace is the tool's own bookkeeping (ADR-004 §2), and without the
//! exclusion the ledger could never close, because the commit that records the sign-offs is itself a commit,
//! which is itself an episode, which would itself want a sign-off, forever. An episode with no paths at all
//! stays in the denominator: "there was nothing to look at" is a claim worth a second look, not a review.
//!
//! The relink ladder is strong and it is not total, and the test suite says so rather than rounding it up.
//! Replaying a tail of commits under new subjects re-links on the shape digest; squashing one turn into a
//! single commit under a brand-new subject destroys the id, the shape *and* the member set at once, and
//! ADR-003 §6.2 has no rung for that — which is what its `manual` rung is for. The acceptance test
//! (`sign_offs_relink_after_amend_reword_and_squash`) rewrites twenty-one episodes three ways, asserts the
//! ticket's 95% bar, and names the one entry that legitimately needs a human. A test that asserted 100%
//! would have been passed by a relink engine that silently invented matches.
//!
//! # What is deliberately *not* here
//!
//! **No clock, no config, no git.** `fathom-cli` supplies the instant, reads `[review]` out of
//! `fathom.toml`, and asks git who the reviewer is and whether the ledger file is tracked (ADR-002 §4.4,
//! §4.5). That is what makes every rule in this module a golden test rather than a screenshot of this
//! afternoon, and what lets `fathom review` be tested against a real rewritten history in a temp directory.
//!
//! **No disposition machinery.** The four acts, their dismiss vocabulary and their snooze conditions are
//! [`crate::queue`]'s. This module *renders* the acts a reviewer took into a committable file; it does not
//! decide what a keystroke means, and there is exactly one place that decides (lint `M02`'s reason for
//! existing).
//!
//! **No signatures stronger than git's.** See [`entry`] on the tamper digest: it makes a silent edit
//! detectable, it is not a cryptographic claim, and a document that implied otherwise would be trading a
//! real property for a impressive-sounding one.

#![deny(missing_docs)]

pub mod entry;
pub mod format;
pub mod relink;

#[cfg(any(feature = "sqlite-bundled", feature = "sqlite-system"))]
pub mod collect;

pub use entry::{
    Actor, ActorKind, ContentCompare, ContentProfile, Decision, DiffTouch, EpisodeFacts,
    FileContent, FileState, PathContent, PathFact, SignError, SignOff, compare_signed,
    content_digest, entry_digest, entry_id, iso_utc, ledger_clock, parse_iso_utc, render_paths,
    sign,
};
pub use format::{
    LEDGER_DIR, LEDGER_FILE, Parsed, Problem, default_path, discover, parse, parse_text, preamble,
    render, render_entry,
};
pub use relink::{Anchor, EntryState, Link, LinkMethod, Verdict, assess, link, link_over};
