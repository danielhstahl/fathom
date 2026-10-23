//! `relink.rs` — how a decision survives the rewrite that moved its episode (`e4.6`, `reco-rlf`).
//!
//! ADR-003 §6 is the identity half: three digests and a member-set test, with a relink pass that carries a
//! review marker from a dead episode to a live one. This file is the *ledger* half, and it has a harder job
//! for two reasons:
//!
//! 1. **The ledger must re-link from a snapshot, not from the row.** A markdown entry holds the digests as
//!    they were at sign-off, and the episode it named may be long gone from the index — including its row,
//!    which ADR-003 §6.5 prunes when nothing points at it. So the ladder here runs on the digests written in
//!    the file rather than on a join between two live tables.
//! 2. **Re-linked is not the same as still true.** The segmentation pass may carry a marker forward and be
//!    right to; a sign-off additionally has to answer "is the content I signed still the content here", and
//!    that is a *second*, orthogonal verdict ([`ContentProfile::compare`]) on top of the link. ADR-004 §6.4
//!    says the same thing in one line — "`state='stale'` is what a sign-off whose content later changed
//!    becomes — reported, never silently trusted".
//!
//! # The ladder
//!
//! Tiers in this order, first tier with a candidate wins (ADR-003 RL1 — a stronger tier's answer is never
//! revised by a weaker one):
//!
//! | `link_method` | Evidence | `link_score` | Typical rewrite |
//! |---|---|---|---|
//! | `exact` | the entry's episode id still names a **live** episode | 1.00 | a plain `rebase -i`, `--amend --no-edit`, a force-push |
//! | `fingerprint` | the entry's strong digest names a live episode whose id differs | 0.98 | a twin that lost or gained its `-2` suffix |
//! | `fingerprint` | shared `fallback_fingerprint` (author + subject) | 0.90 | an amend or squash that moved files |
//! | `shape` | shared `shape_fingerprint` (author + paths) | 0.70 | a reword, mid-rebase or not |
//! | `member-set` | at least one member commit in common | 0.60 | a split; a squash; a reword **and** a move |
//! | `none` | nothing matched | — | the change is not in this history |
//!
//! Within a tier the candidate is the **most recent** live episode (`last_utc`, then `created_utc`, then id),
//! which is ADR-003 RL3's tie-break carried over: the row just under the live one is almost always its
//! predecessor, and the id ordering makes the choice total, so the same index always produces the same link.
//!
//! A dead episode is never the *answer*, only the *explanation*. ADR-003 §6.5 keeps a dead row that carries a
//! decision so that a sign-off can be read from either end, and §5.5 says a reword moves the id and the
//! decision follows it onto the successor — so an id that resolves only to a dead row sends the ladder on to
//! the weaker tiers, and the dead row is what the orphan names when they find nothing. An act taken in the TUI
//! writes a `review_state` row, which is exactly the row that keeps a dead episode from being pruned, so a
//! ladder that stopped at a dead exact hit would orphan every sign-off the product itself produced and
//! contradict the segmentation pass, which re-links the same marker one table away (`reco-2pw`).
//!
//! # What is *not* here, and on purpose
//!
//! **No similarity.** No fuzzy digest, no "the subjects are 80% alike", no threshold tuned without data. ADR-003
//! §13.1 defers fuzzy re-linking until `s6` (`reco-f5a`) can report a false-relink rate against ground truth,
//! and the reason is the same here as there: a wrong link does not print an error, it prints "you already
//! reviewed this" about something you never saw. Every tier above is an exact digest comparison or an exact
//! commit intersection — each one falsifiable, each one auditable in the entry's own fields.
//!
//! **No silent re-pointing of a decision onto a *live* episode that already has its own sign-off.** That is
//! ADR-003 RL0's rule for markers, and the ledger version is the same: a fresh verdict by a human outranks
//! the machine's guess about which old verdict belonged where. Where a live episode already carries a fresh
//! accepted entry of its own, a weaker-tier link (anything below `fingerprint`) is refused and the entry
//! stays an orphan with that reason printed — it would be a second reviewer's claim attached to something
//! somebody already signed, which is a metric inflated by arithmetic.
//!
//! # The failure that is designed in
//!
//! Reword **and** move files **and** rewrite the oids: every digest moves, no member survives, and the entry
//! becomes an orphan. That is ADR-003 §6.3, and the ledger obeys it exactly — the reviewer re-reviews
//! something they had already signed off (annoying, visible, recoverable) rather than the tool asserting a
//! review that never happened (invisible, and the one thing this product's currency cannot survive). The
//! relink fixture in `tests/review_ledger.rs` contains one of these on purpose, which is why the ticket's
//! number is "at least 95%", and the test prints the rate rather than asserting 100% to look good.

use serde::Serialize;

use super::entry::{ContentCompare, EpisodeFacts, PathFact, SignOff};

/// What the ladder needs to know about an entry, whatever kind of entry it is.
///
/// The ladder was written for sign-offs, and `e6.5`'s review memory needs exactly the same re-link: a note
/// attached to an episode has to follow that episode through a rebase for the same reason a sign-off does
/// (ADR-003 §6), and a second copy of the rungs would be a second answer to one question — the day they
/// disagree, nobody could say which entry kind got the better ladder. So the two entry types are the same
/// six facts here: the id it wrote, the episode it named, the three digests it can be re-found by, and the
/// member set it can be re-found without.
///
/// The three extra methods are the places the two kinds genuinely differ, and they are named rather than
/// inferred from a field that happens to be empty:
///
/// * [`Anchor::claims_episode`] is ADR-003 RL0's guard: only a *decision* that a human made about an episode
///   counts as "this episode has been signed", so a note pinned to an episode must not shield it from a
///   weaker-tier re-link of somebody else's sign-off.
/// * [`Anchor::signed_content`] is the content claim staleness is measured against. A note carries a content
///   digest when it was written against an episode, and none when it was pinned to a bare path — and "there
///   is no content claim here" is not the same sentence as "the content moved".
/// * [`Anchor::intact`] is the tamper check, which is per-kind because each kind frames its own digest.
pub trait Anchor {
    /// The entry's own id (`sg-…` for a sign-off, `nt-…` for a note).
    fn anchor_id(&self) -> &str;
    /// The episode the entry names as written, before any re-link.
    fn anchor_episode(&self) -> &str;
    /// ADR-003's strong digest.
    fn anchor_fingerprint(&self) -> &str;
    /// ADR-003's author+subject digest.
    fn anchor_fallback(&self) -> &str;
    /// ADR-003's author+paths digest.
    fn anchor_shape(&self) -> &str;
    /// The member commits the entry lists, in the printed (short) form.
    fn anchor_members(&self) -> &[String];
    /// Whether this entry is a disposition a human made about the episode — RL0's guard.
    fn claims_episode(&self) -> bool;
    /// Whether the entry's own text still hashes to the digest it carries.
    fn intact(&self) -> bool;
    /// The content digest and path facts the entry was written against; `None` when it makes no content
    /// claim at all.
    fn signed_content(&self) -> Option<(&str, &[PathFact])>;
    /// What a surface prints about the entry when no live episode can be described instead.
    fn fallback_summary(&self) -> &str;
}

impl Anchor for SignOff {
    fn anchor_id(&self) -> &str {
        &self.id
    }
    fn anchor_episode(&self) -> &str {
        &self.episode
    }
    fn anchor_fingerprint(&self) -> &str {
        &self.fingerprint
    }
    fn anchor_fallback(&self) -> &str {
        &self.fallback
    }
    fn anchor_shape(&self) -> &str {
        &self.shape
    }
    fn anchor_members(&self) -> &[String] {
        &self.members
    }
    fn claims_episode(&self) -> bool {
        self.decision.is_sign_off()
    }
    fn intact(&self) -> bool {
        super::entry::entry_digest(&self.id, &self.body) == self.digest
    }
    fn signed_content(&self) -> Option<(&str, &[PathFact])> {
        Some((&self.content, &self.paths))
    }
    fn fallback_summary(&self) -> &str {
        &self.note
    }
}

/// How a ledger entry got attached to an episode — `ledger_entry.link_method`'s vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum LinkMethod {
    /// The id still names an episode.
    Exact,
    /// A digest matched: the strong one on a different id, or the author+subject one.
    Fingerprint,
    /// The author+paths digest matched.
    Shape,
    /// A member commit was in common.
    MemberSet,
    /// A human re-attached it by hand (ADR-003's `method='manual'`; no writer claims it yet, and the
    /// vocabulary is listed here so the day it lands is not the day somebody invents the spelling).
    Manual,
    /// Nothing matched.
    None,
}

impl LinkMethod {
    /// The store's spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            LinkMethod::Exact => "exact",
            LinkMethod::Fingerprint => "fingerprint",
            LinkMethod::Shape => "shape",
            LinkMethod::MemberSet => "member-set",
            LinkMethod::Manual => "manual",
            LinkMethod::None => "none",
        }
    }

    /// Parse the store's spelling; an unknown word is `None`, which is the honest reading of a link this
    /// build cannot account for.
    #[must_use]
    pub fn parse(value: &str) -> Self {
        match value {
            "exact" => LinkMethod::Exact,
            "fingerprint" => LinkMethod::Fingerprint,
            "shape" => LinkMethod::Shape,
            "member-set" => LinkMethod::MemberSet,
            "manual" => LinkMethod::Manual,
            _ => LinkMethod::None,
        }
    }
}

/// What the ledger thinks of an entry now: the three states of `ledger_entry.state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum EntryState {
    /// Linked, and the content is what was signed.
    Fresh,
    /// Linked, and it is not — or could not be checked, which is reported rather than trusted.
    Stale,
    /// Not linked to anything live.
    Orphan,
}

impl EntryState {
    /// The store's spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            EntryState::Fresh => "fresh",
            EntryState::Stale => "stale",
            EntryState::Orphan => "orphan",
        }
    }
}

/// One entry's verdict.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Link {
    /// The entry being judged.
    pub entry: String,
    /// The episode it now names, if any.
    pub episode: Option<String>,
    /// The episode it was signed against, when the link moved it.
    pub relinked_from: Option<String>,
    /// How it was linked.
    pub method: LinkMethod,
    /// How much to trust the link — the ladder's published prices.
    pub score: f64,
    /// The state verdict.
    pub state: EntryState,
    /// Why it is stale or orphaned, in a sentence; empty when fresh.
    pub reason: String,
    /// The live episode's content digest, so a surface can print what it now points at.
    pub linked_fingerprint: String,
    /// The episode exists but no current commit belongs to it — the history that carried it is gone.
    pub dead: bool,
    /// The entry's own text no longer hashes to its `digest:` field. Orthogonal to `state` by design: see
    /// [`crate::ledger`] on why tampering is not a fourth state.
    pub tampered: bool,
    /// The scale the sign-off covered, in case a surface prints it with the verdict.
    pub summary: String,
}

impl Link {
    /// The line a reviewer reads. Empty reason means fresh and says nothing extra.
    #[must_use]
    pub fn verdict(&self) -> String {
        let state = match self.state {
            EntryState::Fresh => "fresh",
            EntryState::Stale => "STALE",
            EntryState::Orphan => "ORPHAN",
        };
        if self.tampered {
            return format!("{state} (TAMPERED)");
        }
        state.to_owned()
    }
}

/// Re-link every entry against the episodes the index currently holds.
///
/// `entries` and the returned links are in the same order, so a caller can zip them. `episodes` may include
/// dead ones: an entry that points only at a dead episode is an orphan *with a reason*, which is a different
/// sentence from "nothing here matches" and the more useful one — but only after every tier has been asked
/// whether the change has a live successor.
#[must_use]
pub fn link(entries: &[SignOff], episodes: &[EpisodeFacts]) -> Vec<Link> {
    link_over(entries, episodes)
}

/// The same ladder over any [`Anchor`]: sign-offs, review notes, or a later entry kind with the same
/// digests. Kept generic rather than duplicated so there is exactly one rung list in the tree.
#[must_use]
pub fn link_over<E: Anchor>(entries: &[E], episodes: &[EpisodeFacts]) -> Vec<Link> {
    // Which live episodes already carry a fresh accepted entry of their own — the RL0 guard below. Computed
    // once, over the entries whose *own* id matched exactly, so a weak-tier guess cannot be justified by an
    // entry that only a weak-tier guess put there.
    let mut claimed: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for entry in entries {
        if entry.claims_episode()
            && episodes
                .iter()
                .any(|episode| episode.live && episode.id == entry.anchor_episode())
        {
            claimed.insert(entry.anchor_episode());
        }
    }

    entries
        .iter()
        .map(|entry| {
            let tampered = !entry.intact();
            let candidates = |tier: Tier| {
                episodes
                    .iter()
                    .filter(|episode| tier.matches(entry, episode))
                    .collect::<Vec<_>>()
            };
            // Tier 0: the id itself — but only a *live* episode under that id answers the question the
            // verdict asks, which is "is the change this reviewer signed still in this history, and is
            // what they signed still what is here?". A dead row with that id is not an answer, it is the
            // absence of one: ADR-003 §6.5 keeps it precisely so that a sign-off can be read from either
            // end, and §5.5 is explicit that a reword moves the id and the decision follows it onto the
            // successor. So a dead exact hit is remembered, not returned: it is what the orphan reason
            // names when no tier finds a successor, and nothing else. Stopping here instead — which is
            // what this ladder did — orphaned every sign-off the product itself had written, because the
            // `review_state` row a TUI act leaves behind is exactly the reason the dead row is kept
            // (`reco-2pw`).
            let by_id = |live: bool| {
                episodes
                    .iter()
                    .find(|episode| episode.id == entry.anchor_episode() && episode.live == live)
            };
            let dead_exact = by_id(false);
            let (chosen, method, score) = if let Some(episode) = by_id(true) {
                (Some((*episode).clone()), LinkMethod::Exact, 1.0)
            } else {
                let mut chosen = None;
                let mut method = LinkMethod::None;
                let mut score = 0.0;
                for (tier, price) in [
                    (Tier::StrongDigest, 0.98),
                    (Tier::AuthorSubject, 0.90),
                    (Tier::AuthorPaths, 0.70),
                    (Tier::MemberSet, 0.60),
                ] {
                    let found = candidates(tier);
                    let live: Vec<&EpisodeFacts> =
                        found.iter().copied().filter(|episode| episode.live).collect();
                    if live.is_empty() {
                        continue;
                    }
                    // RL0's guard, in ledger terms: a weaker tier may not attach an entry to an episode that
                    // already has its own accepted sign-off.
                    if matches!(tier, Tier::AuthorPaths | Tier::MemberSet)
                        && live
                            .iter()
                            .all(|episode| claimed.contains(episode.id.as_str()))
                    {
                        continue;
                    }
                    chosen = live
                        .into_iter()
                        .max_by(|a, b| prefer(a, b))
                        .map(|episode| (*episode).clone());
                    method = match tier {
                        Tier::StrongDigest | Tier::AuthorSubject => LinkMethod::Fingerprint,
                        Tier::AuthorPaths => LinkMethod::Shape,
                        Tier::MemberSet => LinkMethod::MemberSet,
                    };
                    score = price;
                    break;
                }
                (chosen, method, score)
            };

            let Some(episode) = chosen else {
                // Nothing live matched, so the entry is an orphan. It is still two different sentences
                // — "the id names a row this history no longer carries" and "nothing here matches at
                // all" — and only the first one tells the reviewer their review was real.
                if let Some(dead) = dead_exact {
                    return Link {
                        entry: entry.anchor_id().to_owned(),
                        episode: Some(dead.id.clone()),
                        // The link did not move: this is the episode the entry named, still named, and
                        // still dead. `relinked_from` is for a decision that *arrived* somewhere else.
                        relinked_from: None,
                        method: LinkMethod::Exact,
                        score: 1.0,
                        state: EntryState::Orphan,
                        reason: "the episode is in the index but no current commit belongs to it — the \
                                 history that carried it was rewritten away, and no re-link tier found a \
                                 successor for it in this one"
                            .to_owned(),
                        linked_fingerprint: dead.fingerprint.clone(),
                        dead: true,
                        tampered,
                        summary: dead.summary.clone(),
                    };
                }
                return Link {
                    entry: entry.anchor_id().to_owned(),
                    episode: None,
                    // Nothing was linked, so nothing was linked *from*: `relinked_from` names the episode a
                    // carried-forward sign-off used to point at, and an orphan never arrived anywhere. Leaving
                    // it `None` is what keeps `fathom review list` from printing a move that did not happen,
                    // and keeps the mirror's `relinked_from` column honest.
                    relinked_from: None,
                    method: LinkMethod::None,
                    score: 0.0,
                    state: EntryState::Orphan,
                    reason: orphan_reason(entry),
                    linked_fingerprint: String::new(),
                    dead: false,
                    tampered,
                    summary: entry.fallback_summary().to_owned(),
                };
            };

            let relinked_from = if episode.id == entry.anchor_episode() {
                None
            } else {
                Some(entry.anchor_episode().to_owned())
            };
            // Every candidate the ladder can choose is live — a dead one is either the exact hit handled
            // above or filtered out of the tiers — so a link that got here points at a reviewable episode.
            let (state, reason) = match entry.signed_content() {
                None => (EntryState::Fresh, String::new()),
                Some((content, paths)) => {
                    match super::entry::compare_signed(content, paths, &episode.content) {
                        ContentCompare::Same => (EntryState::Fresh, String::new()),
                        ContentCompare::Unmeasured(why) => (EntryState::Stale, why),
                        changed => (EntryState::Stale, changed.reason()),
                    }
                }
            };
            Link {
                entry: entry.anchor_id().to_owned(),
                episode: Some(episode.id.clone()),
                relinked_from,
                method,
                score,
                state,
                reason,
                linked_fingerprint: episode.fingerprint.clone(),
                dead: false,
                tampered,
                summary: episode.summary.clone(),
            }
        })
        .collect()
}

/// The ladder's tiers, as data, so the order of the array *is* the order of the ladder.
#[derive(Debug, Clone, Copy)]
enum Tier {
    StrongDigest,
    AuthorSubject,
    AuthorPaths,
    MemberSet,
}

impl Tier {
    fn matches(self, entry: &dyn Anchor, episode: &EpisodeFacts) -> bool {
        match self {
            Tier::StrongDigest => {
                !entry.anchor_fingerprint().is_empty()
                    && entry.anchor_fingerprint() == episode.fingerprint
            }
            Tier::AuthorSubject => {
                !entry.anchor_fallback().is_empty()
                    && entry.anchor_fallback() == episode.fallback_fingerprint
            }
            Tier::AuthorPaths => {
                !entry.anchor_shape().is_empty()
                    && entry.anchor_shape() == episode.shape_fingerprint
            }
            Tier::MemberSet => episode.members.iter().any(|oid| {
                // The printed `members:` field is the eight-hex prefix form (see `entry::join8`), so the
                // intersection is on prefixes; the full oids are in the mirror row's `scope` JSON.
                entry
                    .anchor_members()
                    .iter()
                    .any(|mine| !mine.is_empty() && oid.starts_with(mine.as_str()))
            }),
        }
    }
}

/// ADR-003 RL3's tie-break, as a total order: newest first, then oldest-created first, then id.
fn prefer(a: &EpisodeFacts, b: &EpisodeFacts) -> std::cmp::Ordering {
    (a.last_utc, a.created_utc, a.id.as_str()).cmp(&(b.last_utc, b.created_utc, b.id.as_str()))
}

/// Why nothing matched, phrased as what the reviewer can do about it.
fn orphan_reason(entry: &dyn Anchor) -> String {
    format!(
        "no episode in the current index carries this entry: its id, its two weaker digests and its {} \
         member commit(s) all match nothing — the change was rewritten away or is not on this ref",
        entry.anchor_members().len()
    )
}

/// The ledger's half of "is everything reviewed": the counts, the blockers, and what is still unsigned.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Verdict {
    /// Entries mirrored.
    pub entries: usize,
    /// …of which fresh / stale / orphan, and how many were tampered.
    pub fresh: usize,
    /// See [`Verdict::fresh`].
    pub stale: usize,
    /// See [`Verdict::fresh`].
    pub orphan: usize,
    /// See [`Verdict::fresh`].
    pub tampered: usize,
    /// Live committed episodes with no fresh accepted sign-off.
    pub unsigned: usize,
    /// The first few unsigned ids, so the line is actionable without a second verb.
    pub unsigned_examples: Vec<String>,
    /// Live committed episodes in the index — the denominator `unsigned` is a count out of.
    pub live_episodes: usize,
    /// The blocking reasons, each a sentence. Empty is the definition of clean.
    pub blocking: Vec<String>,
    /// `blocking.is_empty()`. A field rather than a method so the `--json` payload carries the same answer
    /// the printer printed and cannot disagree with it.
    pub clean: bool,
}

/// Whether an episode is a change a person is expected to review.
///
/// See [`assess`] for why `.fathom/`-only episodes are not: they are the tool writing in its own notebook.
fn reviewable(episode: &EpisodeFacts) -> bool {
    if episode.content.files.is_empty() {
        return true;
    }
    !episode
        .content
        .files
        .iter()
        .all(|file| file.path.starts_with(TOOL_OWNED))
}

/// The path prefix that belongs to Fathom itself rather than to the repository it is reviewing.
const TOOL_OWNED: &str = ".fathom/";

/// Build the verdict from the entries, their links and the episodes.
///
/// `extra` is for the blockers only the caller can know — a ledger file git does not hold, say, which is
/// ADR-004 §9.6's warning and belongs in the same list rather than in a second one a reader has to merge.
#[must_use]
pub fn assess(
    entries: &[SignOff],
    links: &[Link],
    episodes: &[EpisodeFacts],
    extra: &[String],
) -> Verdict {
    let mut verdict = Verdict::default();
    let mut signed: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for (entry, link) in entries.iter().zip(links) {
        verdict.entries += 1;
        if link.tampered {
            verdict.tampered += 1;
        }
        match link.state {
            EntryState::Fresh => {
                verdict.fresh += 1;
                if entry.decision.is_sign_off()
                    && let Some(id) = link.episode.clone()
                {
                    signed.insert(id);
                }
            }
            EntryState::Stale => verdict.stale += 1,
            EntryState::Orphan => verdict.orphan += 1,
        }
    }
    // The denominator is *reviewable* live episodes, which excludes one thing: an episode whose every path is
    // under `.fathom/`. That namespace is the tool's own bookkeeping (ADR-004 §2's layout) — the committed
    // ledger, the mirror's strays — and without the exclusion the ledger could never reach "all reviewed":
    // committing the sign-offs is a commit, the commit is an episode, and the episode would demand a sign-off
    // of its own, forever. An episode that reviewed nothing (no paths at all — an empty commit, or a diff the
    // indexer could not read) stays in, because "nothing to look at" is a claim that deserves a look.
    let live: Vec<&EpisodeFacts> = episodes
        .iter()
        .filter(|episode| episode.live && episode.kind == "committed" && reviewable(episode))
        .collect();
    verdict.live_episodes = live.len();
    verdict.unsigned_examples = live
        .iter()
        .filter(|episode| !signed.contains(&episode.id))
        .map(|episode| episode.id.clone())
        .collect();
    verdict.unsigned = verdict.unsigned_examples.len();
    verdict.unsigned_examples.truncate(5);

    if verdict.tampered > 0 {
        verdict.blocking.push(format!(
            "{} ledger entr{} tampered: its text no longer hashes to its own `digest:` — run `fathom review \
             verify` for the line numbers",
            verdict.tampered,
            if verdict.tampered == 1 { "y is" } else { "ies are" }
        ));
    }
    if verdict.orphan > 0 {
        verdict.blocking.push(format!(
            "{} sign-off{} no longer names anything in this history",
            verdict.orphan,
            if verdict.orphan == 1 { "" } else { "s" }
        ));
    }
    if verdict.stale > 0 {
        verdict.blocking.push(format!(
            "{} sign-off{} stale — the content changed after {} was signed",
            verdict.stale,
            if verdict.stale == 1 { " is" } else { "s are" },
            if verdict.stale == 1 { "it" } else { "them" }
        ));
    }
    if verdict.unsigned > 0 {
        verdict.blocking.push(format!(
            "{} of {} live episodes have no fresh sign-off",
            verdict.unsigned, verdict.live_episodes
        ));
    }
    verdict.blocking.extend(extra.iter().cloned());
    verdict.clean = verdict.blocking.is_empty();
    verdict
}
