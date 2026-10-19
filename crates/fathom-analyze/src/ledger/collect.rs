//! `collect.rs` — the ledger's only conversation with the store (`e4.6`, `reco-rlf`).
//!
//! The same discipline `crate::detectors::collect` runs: one file holds the `Store`, and everything else in
//! the module reasons over plain data ([`EpisodeFacts`], [`SignOff`], [`Link`]). That is what lets the relink
//! ladder, the staleness rule and the verdict be tested with literals, and what leaves this file testable
//! against a real repository instead of a mock that agrees with the code that wrote it.
//!
//! # The mirror is built, never patched
//!
//! [`refresh`] re-reads every ledger file and rewrites every row of `ledger_entry`, in one transaction,
//! because the file is the authority (ADR-004 §9.1 level 2 and §10). A merge would leave a row for an entry
//! that has been deleted from the file — a sign-off nobody signed — and no amount of bookkeeping fixes that
//! as well as rebuilding does. The cost is one pass over a file that is a few hundred lines on a real
//! repository.
//!
//! # The order of the two writes, and why it is never the other way round
//!
//! The markdown is written and fsynced *first* (R8, ADR-004 §7.4), and the mirror is refreshed afterwards by
//! reading it back. So a crash between the two loses an index row — class R, rebuilt on the next command —
//! and can never lose a sign-off. A refresh that "knew" the entries without re-reading the file would be a
//! mirror that can lie, which is the failure ADR-004 §10.1 says is an incident.

use std::path::Path;

use fathom_store::episode::EpisodeQuery;
use fathom_store::ledger::LedgerRow;
use fathom_store::spine::Store;

use super::entry::{ContentProfile, Decision, DiffTouch, EpisodeFacts, SignOff, sign};
use super::format::{self, Problem};
use super::relink::{self, Link, Verdict};
use crate::error::Error;

/// Every episode the index holds — dead ones included — as the plain data the ledger reasons over.
///
/// Dead episodes are included for the reason ADR-003 §6.5 gives: a sign-off whose episode was retired is a
/// fact to report ("the history that carried it was rewritten away"), not a row to forget. Content profiles
/// are built for live episodes only ([`Store::episode_diff_rows`] reads those), which is exactly the case
/// that matters: a dead episode cannot have its content compared, and the link's verdict for it is orphan
/// whatever the digests say.
///
/// # Errors
///
/// [`Error::Store`] if either read fails.
pub fn episode_facts(store: &Store) -> Result<Vec<EpisodeFacts>, Error> {
    let rows = store.episodes(&EpisodeQuery {
        include_dead: true,
        ..EpisodeQuery::default()
    })?;
    let diffs = store.episode_diff_rows()?;

    // The rows arrive in `(episode, ordinal, path)` order, so one forward fold builds every profile.
    let mut grouped: Vec<(String, Vec<DiffRow>)> = Vec::new();
    for row in diffs.rows {
        let episode = row.episode;
        let touch = DiffRow {
            ordinal: row.ordinal,
            path: row.path,
            change_kind: row.change_kind,
            add: row.add,
            del: row.del,
            blob_new: row.blob_new,
        };
        match grouped.last_mut() {
            Some((name, touches)) if *name == episode => touches.push(touch),
            _ => grouped.push((episode, vec![touch])),
        }
    }
    let mut profiles: std::collections::BTreeMap<String, ContentProfile> = grouped
        .into_iter()
        .map(|(episode, touches)| {
            let borrowed: Vec<DiffTouch<'_>> = touches
                .iter()
                .map(|touch| DiffTouch {
                    ordinal: touch.ordinal,
                    path: touch.path.as_str(),
                    change_kind: touch.change_kind.as_str(),
                    add: touch.add,
                    del: touch.del,
                    blob: touch.blob_new.as_deref(),
                })
                .collect();
            (
                episode,
                ContentProfile::from_diff_rows(&borrowed, !diffs.truncated),
            )
        })
        .collect();

    Ok(rows
        .into_iter()
        .map(|row| EpisodeFacts {
            id: row.id.clone(),
            kind: row.kind,
            live: row.live,
            fingerprint: row.fingerprint,
            fallback_fingerprint: row.fallback_fingerprint,
            shape_fingerprint: row.shape_fingerprint,
            summary: row.summary,
            members: row.members,
            lineage_seq: row.lineage_seq,
            last_utc: row.last_utc,
            created_utc: row.created_utc,
            content: profiles.remove(&row.id).unwrap_or_default(),
            risk: None,
            risk_model: String::new(),
        })
        .collect())
}

/// One member's touch of one path, as the store read it.
struct DiffRow {
    ordinal: i64,
    path: String,
    change_kind: String,
    add: i64,
    del: i64,
    blob_new: Option<String>,
}

/// Read every ledger file under `.fathom/reviews/`.
///
/// Returns the entries and every problem found, including the files that could not be read at all. A missing
/// directory is the empty case, not an error: a repository nobody has reviewed yet is a real repository in a
/// real state.
#[must_use]
pub fn read(root: &Path) -> (Vec<SignOff>, Vec<Problem>) {
    let mut entries = Vec::new();
    let mut problems = Vec::new();
    for path in format::discover(root) {
        let parsed = format::parse(&path, root);
        let display = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        problems.extend(parsed.problems);
        for mut entry in parsed.entries {
            entry.file = display.clone();
            entries.push(entry);
        }
    }
    (entries, problems)
}

/// The mirror rows for a set of entries and their verdicts.
///
/// `scope` is the ADR's JSON document: the members and paths the sign-off covered, and the scale it covered
/// them at. The lists are the full ones from the entry, not the printed prefixes: the file prints what a
/// person reads, and the mirror holds what a query joins on.
#[must_use]
pub fn mirror_rows(entries: &[SignOff], links: &[Link]) -> Vec<LedgerRow> {
    entries
        .iter()
        .zip(links)
        .map(|(entry, link)| {
            let scope = serde_json::json!({
                "members": entry.members,
                "paths": entry.paths.iter().map(|fact| fact.display()).collect::<Vec<_>>(),
                "n_commits": entry.scale.commits,
                "n_files": entry.scale.files,
                "n_add": entry.scale.add,
                "n_del": entry.scale.del,
                "risk": entry.risk,
                "risk_model": entry.risk_model,
            });
            LedgerRow {
                id: entry.id.clone(),
                file: entry.file.clone(),
                anchor: format!("{}-{}", entry.file, entry.id),
                episode_id: link.episode.clone().or_else(|| Some(entry.episode.clone())),
                fingerprint: entry.fingerprint.clone(),
                fallback_fingerprint: entry.fallback.clone(),
                shape_fingerprint: entry.shape.clone(),
                content_fingerprint: entry.content.clone(),
                linked_fingerprint: link.linked_fingerprint.clone(),
                decision: entry.decision.as_str().to_owned(),
                reason_code: if entry.reason.is_empty() {
                    None
                } else {
                    Some(entry.reason.clone())
                },
                note: entry.note.clone(),
                actor: entry.actor.display.clone(),
                actor_kind: entry.actor.kind.as_str().to_owned(),
                signed_utc: entry.signed_utc,
                scope: scope.to_string(),
                content_sha: entry.digest.clone(),
                link_method: link.method.as_str().to_owned(),
                link_score: link.score,
                state: link.state.as_str().to_owned(),
                stale_reason: if link.reason.is_empty() {
                    None
                } else {
                    Some(link.reason.clone())
                },
                relinked_from: link.relinked_from.clone(),
                tampered: link.tampered,
                dead: link.dead,
                n_members: entry.scale.commits,
                n_files: entry.scale.files,
                mirrored_utc: 0,
            }
        })
        .collect()
}

/// Everything one refresh produced.
#[derive(Debug)]
pub struct Report {
    /// The entries read out of the files, oldest first.
    pub entries: Vec<SignOff>,
    /// One verdict per entry, same order.
    pub links: Vec<Link>,
    /// What is wrong with the files themselves.
    pub problems: Vec<Problem>,
    /// The counts and blockers, ready to print or to serialise.
    pub verdict: Verdict,
    /// Rows written to the mirror.
    pub mirrored: usize,
    /// `episode_rewrite` rows this refresh recorded.
    pub rewrites_recorded: usize,
    /// The ledger files that were read.
    pub files: Vec<std::path::PathBuf>,
}

impl Report {
    /// Every complaint worth printing, newest problems first: the file-level ones and the entry verdicts
    /// that are not fresh, in the order a reviewer would want them.
    #[must_use]
    pub fn complaints(&self) -> Vec<String> {
        let mut out: Vec<String> = self.problems.iter().map(Problem::line).collect();
        for (entry, link) in self.entries.iter().zip(&self.links) {
            if link.state == relink::EntryState::Fresh && !link.tampered {
                continue;
            }
            out.push(format!(
                "{}: {} {}",
                entry.file,
                link.verdict(),
                if link.reason.is_empty() {
                    link.summary.clone()
                } else {
                    link.reason.clone()
                }
            ));
        }
        out
    }
}

/// Re-read the ledger, re-link it against the index, rewrite the mirror, and assess it.
///
/// `extra` carries blockers only the caller can know — a ledger file git does not hold, say — and lands in
/// [`Verdict::blocking`] alongside everything else so there is one list to print and one list to serialise.
///
/// # Errors
///
/// [`Error::Store`] if the index will not be read or written. The ledger files themselves never produce an
/// error here: an unreadable file is a [`Problem`], because "the ledger holds 3 reviews and one file is
/// unreadable" is a report the reviewer can act on and "error: parse" is not.
pub fn refresh(
    store: &mut Store,
    root: &Path,
    now: i64,
    extra: &[String],
) -> Result<Report, Error> {
    let files = format::discover(root);
    let (entries, problems) = read(root);
    let episodes = episode_facts(store)?;
    let links = relink::link(&entries, &episodes);
    let rows = mirror_rows(&entries, &links);
    let mirrored = store.mirror_ledger(now, &rows)?;

    // RL5: a re-link the ledger proved is a rewrite fact, with the same closed vocabularies the segmentation
    // pass uses. `relation` follows ADR-003 §6.2's prices: a digest carry is a rebase, a member-set carry is
    // a squash. An `exact` link moved nothing, so it writes nothing.
    let rewrites: Vec<(String, String, String, String, f64)> = links
        .iter()
        .filter(|link| {
            link.relinked_from.is_some()
                && link.episode.is_some()
                && link.method != relink::LinkMethod::Exact
        })
        .map(|link| {
            let relation = if link.method == relink::LinkMethod::MemberSet {
                "squash"
            } else {
                "rebase"
            };
            (
                link.relinked_from.clone().unwrap_or_default(),
                link.episode.clone().unwrap_or_default(),
                relation.to_owned(),
                link.method.as_str().to_owned(),
                link.score,
            )
        })
        .collect();
    let rewrites_recorded = store.record_ledger_rewrites(&rewrites, now)?;

    let verdict = relink::assess(&entries, &links, &episodes, extra);
    Ok(Report {
        entries,
        links,
        problems,
        verdict,
        mirrored,
        rewrites_recorded,
        files,
    })
}

/// The queue's acts that the ledger has never rendered, and the acts that could not be rendered at all.
///
/// This is `fathom review sync`'s whole question, and it lives here rather than in the verb for the reason
/// ADR-002 §4 gives: a verb owns the clock, the paths and the printing, and the *rule* is analysis. It also
/// means the rule is testable against a real act log instead of a mock that agrees with it.
///
/// An entry's id is derived from the act — episode, content digest, actor, decision, instant — so "have I
/// seen this act" is a membership test on the act's own identity rather than a marker written somewhere else.
/// That is what makes the command re-runnable: twice in a row renders nothing the second time, and running it
/// after a `git clean -fdx` re-renders exactly what was lost and nothing more.
///
/// The second element of the returned pair counts the acts deliberately left unrendered: `unread` and `read`
/// are not decisions, and an act whose episode has left the index cannot be described well enough to sign off
/// — the act row remains the record that it happened.
///
/// # Errors
///
/// [`Error::Store`] if the act log or the episodes cannot be read. The ledger files cannot produce an error:
/// an unreadable file yields a [`Problem`] from [`read`], and a missing ledger is simply an empty one.
pub fn unsettled(
    store: &Store,
    root: &Path,
    episodes: &[EpisodeFacts],
) -> Result<(Vec<SignOff>, usize), Error> {
    let acts = store.triage_events(5_000)?;
    let (existing, _) = read(root);
    let mut seen: std::collections::BTreeSet<(String, String, String, String, i64)> = existing
        .iter()
        .map(|entry| {
            (
                entry.episode.clone(),
                entry.content.clone(),
                entry.actor.display.clone(),
                entry.decision.as_str().to_owned(),
                entry.signed_utc,
            )
        })
        .collect();

    let mut fresh: Vec<SignOff> = Vec::new();
    let mut skipped = 0_usize;
    for act in &acts {
        let Some(decision) = decision_of(act.disposition) else {
            continue;
        };
        let Some(episode) = episodes
            .iter()
            .find(|candidate| candidate.id == act.episode)
        else {
            skipped += 1;
            continue;
        };
        let key = (
            episode.id.clone(),
            episode.content.digest.clone(),
            act.actor.clone(),
            decision.as_str().to_owned(),
            act.utc,
        );
        if seen.contains(&key) {
            continue;
        }
        let actor = super::entry::Actor {
            display: act.actor.clone(),
            kind: super::entry::ActorKind::of_actor_string(&act.actor),
        };
        let mut stamp = episode.clone();
        stamp.risk = act.risk;
        stamp.risk_model = act.risk_model.clone();
        let note = if act.note.is_empty() {
            act.reason.as_str()
        } else {
            act.note.as_str()
        };
        if let Ok(entry) = sign(&stamp, &actor, decision, note, act.utc) {
            seen.insert(key);
            fresh.push(entry);
        } else {
            skipped += 1;
        }
    }
    fresh.sort_by(|a, b| (a.signed_utc, &a.id).cmp(&(b.signed_utc, &b.id)));
    Ok((fresh, skipped))
}

/// The queue's disposition in the ledger's vocabulary.
///
/// `unread` and `read` are movements of the cursor, not decisions a human made, so they render nothing: the
/// ledger records acceptances and refusals, not that a row was scrolled past.
fn decision_of(state: fathom_store::episode::ReviewState) -> Option<Decision> {
    use fathom_store::episode::ReviewState as S;
    match state {
        S::Accepted => Some(Decision::Accepted),
        S::Flagged => Some(Decision::Flagged),
        S::Dismissed => Some(Decision::Dismissed),
        S::Snoozed => Some(Decision::Snoozed),
        S::Unread | S::Read => None,
    }
}
