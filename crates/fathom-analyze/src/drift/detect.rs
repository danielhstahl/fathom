//! The six detectors, and the two measurements they all share.
//!
//! Everything here is a function of the [`Session`]s it is handed: no store, no clock, no config, no
//! feature. Each detector is small enough to argue with on its own, which is the only way a threshold in a
//! report is worth reading.
//!
//! Shared measurements, both published from this module because tests and docs quote them:
//!
//! * [`tokens`] — the content words of a piece of text: lowercased, split on punctuation and path
//!   separators, stop-words and two-letter fragments gone. It is what a plan item and a change are compared
//!   *on*; a comparison on raw strings would call "bump the parser" and "parser" unrelated because of "the".
//! * [`overlap`] — `|A∩B| / min(|A|,|B|)`. Asymmetric on purpose: a three-word plan item has to be found
//!   *inside* a change's corpus, not matched against it word for word, and a symmetric coefficient lets a
//!   long subject drown a short item.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    Episode, Finding, KINDS, Plan, Policy, Report, Rung, Side, SideKind, Tally, Unchecked,
    severity_of,
};
use crate::scope::{Deviation, deviation};

/// The base confidence each rule brings, before the intent rung scales it.
///
/// A rule that is arithmetic on a recorded fact (`out-of-scope-edit`: the path is or is not under the glob)
/// starts high; a rule that infers "nothing here resembles that" starts lower, because resemblance is the
/// weakest link in the argument.
const BASE: [(&str, f64); 6] = [
    ("out-of-scope-edit", 0.92),
    ("contradicts-signoff", 0.80),
    ("unplanned-dependency", 0.78),
    ("test-promised-not-written", 0.74),
    ("plan-unimplemented", 0.62),
    ("refactor-claimed-not-performed", 0.58),
];

/// Words that carry no comparison signal. Kept short and visible: a stop-word list nobody can read is a
/// threshold nobody can debug.
const STOP: &[&str] = &[
    "an", "and", "are", "as", "at", "be", "been", "before", "being", "but", "by", "can", "could",
    "did", "do", "does", "for", "from", "had", "have", "its", "into", "must", "not", "now", "off",
    "onto", "our", "per", "should", "than", "that", "the", "their", "them", "they", "this", "was",
    "were", "with", "would", "use", "using", "will",
];

/// The test vocabulary: a plan item saying any of these is a promise about the test surface.
const TEST_WORDS: [&str; 10] = [
    "test",
    "tests",
    "testing",
    "spec",
    "specs",
    "coverage",
    "regression",
    "assert",
    "fixture",
    "e2e",
];

/// The structural-change vocabulary, and the reason each one is here: each claims the *shape* of a surface
/// changed, which is checkable against the paths and the recorded path moves.
const REFACTOR_WORDS: [&str; 11] = [
    "refactor",
    "refactors",
    "rename",
    "renames",
    "move",
    "moves",
    "extract",
    "extracts",
    "split",
    "splits",
    "restructure",
];

/// The names a dependency manifest answers to. A path whose file name is or starts with any of these is a
/// manifest touch.
const MANIFESTS: [&str; 17] = [
    "cargo.toml",
    "cargo.lock",
    "package.json",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "pyproject.toml",
    "poetry.lock",
    "uv.lock",
    "requirements",
    "setup.py",
    "setup.cfg",
    "go.mod",
    "go.sum",
    "gemfile",
    "composer.json",
    "build.gradle",
];

/// A word that, in a plan, means a dependency change was accounted for.
const DEPENDENCY_WORDS: [&str; 9] = [
    "dependency",
    "dependencies",
    "dependabot",
    "bump",
    "upgrade",
    "lockfile",
    "lock",
    "manifest",
    "vendor",
];

/// The content words of `text`. See the module docs.
#[must_use]
pub fn tokens(text: &str) -> BTreeSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.len() >= 3 && !STOP.contains(word))
        .map(str::to_owned)
        .collect()
}

/// How much of the smaller set is found in the larger. `0.0` when either side is empty — an empty item is
/// not a match, it is nothing to compare.
#[must_use]
pub fn overlap(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let divisor = usize::min(a.len(), b.len()).max(1);
    let intersection = a.iter().filter(|word| b.contains(*word)).count();
    f64::from(intersection as u32) / f64::from(divisor as u32)
}

/// One session's corpus of *things that happened*, from `from` to `to` inclusive: subjects, touched paths
/// and manifest names. Plans are excluded on purpose — a later plan is not evidence that an earlier plan
/// was done.
fn corpus(episodes: &[Episode], from: usize, to: usize) -> (BTreeSet<String>, Vec<String>) {
    let mut set = BTreeSet::new();
    let mut names = Vec::new();
    for episode in episodes.iter().take(to + 1).skip(from) {
        names.push(episode.id.clone());
        set.extend(tokens(&episode.summary));
        for path in episode.touched.iter().chain(episode.manifests.iter()) {
            set.extend(tokens(path));
        }
    }
    (set, names)
}

/// The rung's price for a claim, scaled by the rule's own base, with a floor so a trailer-only repository
/// stays reportable: `0.30` is close to `agent-intent-trailer`'s own price, so a finding never reads as
/// more certain than the rung that produced it.
fn price(rung: &Rung, kind: &str) -> f64 {
    let base = BASE
        .iter()
        .find(|(id, _)| *id == kind)
        .map_or(0.6, |(_, value)| *value);
    (rung.confidence * base).clamp(0.30, 0.95)
}

/// The stable key a dismissal is stored against: the rule and the subject it fired on — the plan item, the
/// path, the manifest. Deliberately not the episode id and not the evidence sentence: an amend changes the
/// id, and a reworded explanation is not a new contradiction, so a dismissal a human made has to survive
/// both.
fn suppression_key(kind: &str, subject: &str) -> String {
    let joined = format!("{kind}\u{1}{subject}").to_lowercase();
    let text = joined
        .split(|c: char| !c.is_alphanumeric())
        .filter(|chunk| !chunk.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("drift:{kind}:{hash:016x}")
}

/// The item's target: the surface a structural claim names. A path-looking token wins; failing that, the
/// first content word after the verb — "refactor the retry budget module" is about `retry`, which is what
/// a reviewer would go and look for.
fn target_of(text: &str) -> Option<String> {
    if let Some(path) = text
        .split_whitespace()
        .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric() && c != '/' && c != '.'))
        .find(|word| word.contains('/') || word.contains('.'))
    {
        return Some(path.to_lowercase());
    }
    ordered_tokens(text)
        .into_iter()
        .find(|word| !REFACTOR_WORDS.contains(&word.as_str()) && word.len() >= 4)
}

/// The content words of `text`, in the order they were written. [`tokens`] sorts them because comparison
/// does not care; a target has to be the first one, which is the one sitting after the verb.
fn ordered_tokens(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.len() >= 3 && !STOP.contains(word))
        .map(str::to_owned)
        .collect()
}

/// Does this item talk about the test surface?
fn about_tests(text: &str) -> bool {
    let set = tokens(text);
    TEST_WORDS.iter().any(|word| set.contains(*word))
}

/// Does this item claim a structural change?
fn about_structure(text: &str) -> bool {
    let set = tokens(text);
    REFACTOR_WORDS.iter().any(|word| set.contains(*word))
}

/// Does this plan already account for a dependency change?
fn about_dependencies(text: &str) -> bool {
    let set = tokens(text);
    DEPENDENCY_WORDS.iter().any(|word| set.contains(*word))
}

/// Compare the sessions and report the contradictions.
///
/// One pass per session, six detectors per episode, then the caps and the reading order. Nothing here reads
/// a store or a clock: the caller hands over the sessions it read, in the order they happened.
#[must_use]
pub fn analyse(sessions: &[super::Session], policy: &Policy) -> Report {
    // `cross_session` is not a second algorithm: it is the decision that the run in front of this caller is
    // one conversation rather than several, so the sessions are compared as one window and everything else
    // — the window length, the caps, the ordering — is unchanged.
    let merged: Option<super::Session> = if policy.cross_session && sessions.len() > 1 {
        Some(super::Session {
            id: sessions.iter().find_map(|session| session.id.clone()),
            episodes: sessions
                .iter()
                .flat_map(|session| session.episodes.iter().cloned())
                .collect(),
        })
    } else {
        None
    };
    let effective: &[super::Session] = merged
        .as_ref()
        .map_or(sessions, |session| std::slice::from_ref(session));
    let input_sessions = sessions.len();
    let mut report = Report::empty(policy);
    let mut picked: BTreeMap<String, Vec<Finding>> = BTreeMap::new();
    let mut suppressed: Vec<String> = Vec::new();

    for session in effective {
        let episodes = &session.episodes;
        report.sessions += 1;
        report.episodes += episodes.len();
        let planned = episodes
            .iter()
            .filter(|episode| episode.plan.is_some())
            .count();
        report.planned += planned;
        if episodes.is_empty() {
            continue;
        }
        if planned == 0 {
            report.unchecked.push(Unchecked {
                subject: session
                    .id
                    .clone()
                    .map_or_else(|| "a session".to_owned(), |id| format!("session {id}")),
                reason:
                    "no episode in this session stated a plan, so there is nothing to compare the \
                         changes against"
                        .to_owned(),
                owner: Some("e4.2".to_owned()),
            });
            continue;
        }

        // One finding per plan item, whichever detector reaches it first: the specialised readings (a test
        // promise, a structural claim) say more than "nothing resembling this happened", so they run first
        // and mark the item claimed.
        let mut fired: BTreeSet<(usize, usize)> = BTreeSet::new();
        for index in 0..episodes.len() {
            let episode = &episodes[index];
            let Some(plan) = &episode.plan else {
                report.unchecked.push(Unchecked {
                    subject: episode.id.clone(),
                    reason: "no intent, no `Intent:` trailer and no plan item recorded this, so there is \
                            no plan to compare against"
                        .to_owned(),
                    owner: Some("e4.2".to_owned()),
                });
                continue;
            };
            if plan.scope.is_empty() {
                report.unchecked.push(Unchecked {
                    subject: episode.id.clone(),
                    reason: "the recorded intent declared no scope, so an edit outside the plan cannot be \
                            told from an edit inside it"
                        .to_owned(),
                    owner: Some("e4.2".to_owned()),
                });
            }

            let to = (index + policy.window).min(episodes.len().saturating_sub(1));
            let horizon = to - index;
            let (_, names) = corpus(episodes, index, to);
            let searched = names.join(", ");
            let count = names.len();

            for (slot, item) in plan.items.iter().enumerate() {
                if item.done {
                    continue;
                }
                let where_ = (index, slot);
                let quoted = format!("“{}”", item.text);

                // 1. A promise about tests, and no test surface touched in the window.
                if about_tests(&item.text) && fired.insert(where_) {
                    match tests_touched_in(episodes, index, to) {
                        Some(false) => take(
                            &mut picked,
                            &mut suppressed,
                            policy,
                            finding(
                                "test-promised-not-written",
                                episode,
                                session,
                                plan,
                                &item.text,
                                &format!(
                                    "planned {quoted} and no test surface was touched in this episode \
                                     or the next {horizon}: no test was written"
                                ),
                                Side::plan(&episode.id, &item.text),
                                Side::absence(
                                    &episode.id,
                                    format!("no test file in {count} episode(s): {searched}"),
                                ),
                                None,
                            ),
                        ),
                        Some(true) => {}
                        None => report.unchecked.push(Unchecked {
                            subject: episode.id.clone(),
                            reason: format!(
                                "planned {quoted} about tests, and the caller could not say whether any \
                                 test surface was touched"
                            ),
                            owner: Some("e2.9".to_owned()),
                        }),
                    }
                }

                // 2. A claimed structural change to a named surface, and that surface untouched.
                if about_structure(&item.text) && fired.insert(where_) {
                    match target_of(&item.text) {
                        Some(target) => {
                            let reached = |path: &str| path.to_lowercase().contains(&target);
                            let touched_it = episodes
                                .iter()
                                .take(to + 1)
                                .skip(index)
                                .any(|later| later.touched.iter().any(|path| reached(path.as_str())));
                            let moved = episodes
                                .iter()
                                .take(to + 1)
                                .skip(index)
                                .any(|later| later.moved_paths == Some(true));
                            if !touched_it && !moved {
                                take(
                                    &mut picked,
                                    &mut suppressed,
                                    policy,
                                    finding(
                                        "refactor-claimed-not-performed",
                                        episode,
                                        session,
                                        plan,
                                        &item.text,
                                        &format!(
                                            "planned {quoted} but nothing in this episode or the next \
                                             {horizon} touched `{target}`, and no path move was recorded"
                                        ),
                                        Side::plan(&episode.id, &item.text),
                                        Side::absence(
                                            &episode.id,
                                            format!(
                                                "`{target}` is in no touched path of {count} episode(s)"
                                            ),
                                        ),
                                        None,
                                    ),
                                );
                            }
                        }
                        None => report.unchecked.push(Unchecked {
                            subject: episode.id.clone(),
                            reason: format!(
                                "planned {quoted} as a structural change and named no surface to check"
                            ),
                            owner: None,
                        }),
                    }
                }

                // 3. An open item with nothing in the window that resembles it.
                if fired.insert(where_) {
                    let (score, best) = best_match(&tokens(&item.text), episodes, index, to);
                    if score < policy.min_overlap {
                        take(
                            &mut picked,
                            &mut suppressed,
                            policy,
                            finding(
                                "plan-unimplemented",
                                episode,
                                session,
                                plan,
                                &item.text,
                                &format!(
                                    "planned {quoted} and no change in this episode or the next \
                                     {horizon} addresses it (closest match {score:.2}, threshold \
                                     {:.2})",
                                    policy.min_overlap
                                ),
                                Side::plan(&episode.id, &item.text),
                                Side::absence(
                                    &episode.id,
                                    format!(
                                        "{count} episode(s) searched ({searched}); best {score:.2} on {best}"
                                    ),
                                ),
                                None,
                            ),
                        );
                    }
                }
            }

            // 4. A touched path outside the declared scope.
            if !plan.scope.is_empty() {
                match deviation(&plan.scope, &episode.touched) {
                    Deviation::Outside(paths) => {
                        for path in paths {
                            // A plan item that names the path means the glob is too narrow, not that the
                            // work went outside it — and deciding which is the reviewer's call, not this
                            // detector's.
                            let planned = plan.items.iter().any(|item| {
                                overlap(&tokens(&item.text), &tokens(&path)) >= policy.min_overlap
                            });
                            if planned {
                                continue;
                            }
                            take(
                                &mut picked,
                                &mut suppressed,
                                policy,
                                finding(
                                    "out-of-scope-edit",
                                    episode,
                                    session,
                                    plan,
                                    &path,
                                    &format!(
                                        "touched `{path}`, outside every declared scope glob — the \
                                         work was bounded to `{}`",
                                        plan.scope.join(", ")
                                    ),
                                    Side::scope(&episode.id, plan.scope.join("; ")),
                                    Side::diff(&episode.id, &path),
                                    None,
                                ),
                            );
                        }
                    }
                    Deviation::Unknown => report.unchecked.push(Unchecked {
                        subject: episode.id.clone(),
                        reason: "the declared scope globs do not compile, so inside and outside cannot be \
                                told apart"
                            .to_owned(),
                        owner: Some("e4.2".to_owned()),
                    }),
                    Deviation::Inside => {}
                }
            }

            // 5. A dependency manifest nobody planned.
            for manifest in episode.manifests.iter().filter(|path| is_manifest(path)) {
                let accounted = plan.scope.iter().any(|glob| {
                    deviation(std::slice::from_ref(glob), std::slice::from_ref(manifest))
                        == Deviation::Inside
                }) || plan.items.iter().any(|item| {
                    about_dependencies(&item.text)
                        || overlap(&tokens(&item.text), &tokens(manifest)) >= policy.min_overlap
                }) || plan.text.as_deref().is_some_and(about_dependencies);
                if accounted {
                    continue;
                }
                take(
                    &mut picked,
                    &mut suppressed,
                    policy,
                    finding(
                        "unplanned-dependency",
                        episode,
                        session,
                        plan,
                        manifest,
                        &format!(
                            "touched the dependency manifest `{manifest}` with no plan item and no \
                             scope glob accounting for it; this build reads the manifest, not the \
                             resolved graph"
                        ),
                        Side::plan(
                            &episode.id,
                            plan.text
                                .clone()
                                .unwrap_or_else(|| "(no plan text)".to_owned()),
                        ),
                        Side {
                            kind: SideKind::Dependency,
                            episode: episode.id.clone(),
                            quote: manifest.clone(),
                            address: format!("fathom show {} # {manifest}", episode.id),
                        },
                        None,
                    ),
                );
            }

            // 6. A plan item that re-opens work this same episode was signed off as doing.
            if let Some(signoff) = episode.signoff.clone() {
                let signoff_tokens = tokens(&signoff);
                for item in &plan.items {
                    if item.done {
                        continue;
                    }
                    let quoted = format!("“{}”", item.text);
                    let score = overlap(&tokens(&item.text), &signoff_tokens);
                    if score < policy.min_overlap {
                        continue;
                    }
                    let earlier = plan.items.iter().find(|done| {
                        done.done
                            && overlap(&tokens(&done.text), &signoff_tokens) >= policy.min_overlap
                    });
                    take(
                        &mut picked,
                        &mut suppressed,
                        policy,
                        finding(
                            "contradicts-signoff",
                            episode,
                            session,
                            plan,
                            &item.text,
                            &format!(
                                "plans {quoted} as still to do, and this episode was signed off as \
                                 doing it (overlap {score:.2}): either the sign-off was premature or \
                                 the plan is stale"
                            ),
                            Side::plan(&episode.id, &item.text),
                            Side::ledger(&episode.id, &signoff),
                            earlier.map(|done| Side::plan(&episode.id, &done.text)),
                        ),
                    );
                }
            }
        }
    }

    let mut out = finish(report, policy, picked, suppressed);
    out.sessions = input_sessions;
    out
}

/// Sort out one candidate finding: a switched-off kind disappears, a dismissed key is counted rather than
/// reported, and everything else joins its kind's bucket.
fn take(
    picked: &mut BTreeMap<String, Vec<Finding>>,
    suppressed: &mut Vec<String>,
    policy: &Policy,
    finding: Finding,
) {
    if !policy.off.is_empty() && policy.off.contains(&finding.kind) {
        return;
    }
    if policy.dismissed.contains(finding.suppression_key.as_str()) {
        suppressed.push(finding.suppression_key);
        return;
    }
    picked
        .entry(finding.kind.clone())
        .or_default()
        .push(finding);
}

/// Apply the caps, build the tallies, and put the findings in the order a reviewer should read them.
fn finish(
    mut report: Report,
    policy: &Policy,
    picked: BTreeMap<String, Vec<Finding>>,
    suppressed: Vec<String>,
) -> Report {
    let mut order: Vec<&str> = KINDS.iter().map(|(id, _, _, _)| *id).collect();
    for kind in picked.keys() {
        if !order.contains(&kind.as_str()) {
            order.push(kind.as_str());
        }
    }
    let mut kept: Vec<Finding> = Vec::new();
    let mut capped_any = false;
    for kind in &order {
        let bucket: &[Finding] = picked.get(*kind).map_or(&[], Vec::as_slice);
        let capped = bucket.len().saturating_sub(policy.max_per_kind);
        capped_any |= capped > 0;
        kept.extend(bucket.iter().take(policy.max_per_kind).cloned());
        report.tallies.push(Tally {
            kind: (*kind).to_owned(),
            reported: bucket.len().min(policy.max_per_kind),
            dismissed: suppressed
                .iter()
                .filter(|key| key.starts_with(&format!("drift:{kind}:")))
                .count(),
            capped,
            enabled: !policy.off.contains(*kind),
        });
    }
    let truncated = kept.len() > policy.max_findings;
    kept.truncate(policy.max_findings);
    // The reading order: the worst first, then the kind's place in the published list, then the order the
    // sessions were given in (`sort_by` is stable, so that order survives).
    kept.sort_by(|a, b| {
        rank(&a.severity).cmp(&rank(&b.severity)).then_with(|| {
            order
                .iter()
                .position(|kind| *kind == a.kind)
                .cmp(&order.iter().position(|kind| *kind == b.kind))
        })
    });
    report.findings = kept;
    report.truncated = truncated || capped_any;
    report.suppressed = suppressed;
    report
}

/// The severity ordering the report reads in.
fn rank(severity: &str) -> u8 {
    match severity {
        "high" => 0,
        "notable" => 1,
        _ => 2,
    }
}

/// Assemble one finding: the kind's defaults, the rung's price, and the suppression key.
#[allow(clippy::too_many_arguments)]
fn finding(
    kind: &str,
    episode: &Episode,
    session: &super::Session,
    plan: &Plan,
    subject: &str,
    message: &str,
    claim: Side,
    evidence: Side,
    corroborating: Option<Side>,
) -> Finding {
    let key = suppression_key(kind, subject);
    Finding {
        kind: kind.to_owned(),
        rule_version: super::VERSION,
        episode: episode.id.clone(),
        session: session.id.clone(),
        subject: subject.to_owned(),
        message: message.to_owned(),
        severity: severity_of(kind).to_owned(),
        confidence: price(&plan.rung, kind),
        intent_source: plan.rung.source.clone(),
        claim,
        evidence,
        corroborating,
        suppression_key: key,
        owner: super::OWNER.to_owned(),
    }
}

/// The best overlap a plan item gets from anything in the window, and the episode it came from.
fn best_match(
    item: &BTreeSet<String>,
    episodes: &[Episode],
    from: usize,
    to: usize,
) -> (f64, String) {
    let mut best = (0.0_f64, String::new());
    for episode in episodes.iter().take(to + 1).skip(from) {
        let mut here = tokens(&episode.summary);
        for path in episode.touched.iter().chain(episode.manifests.iter()) {
            here.extend(tokens(path));
        }
        let score = overlap(item, &here);
        if score > best.0 {
            best = (score, episode.id.clone());
        }
    }
    best
}

/// Was a test surface touched anywhere in the window? `None` when any episode in it could not say — and
/// "could not tell" is never read as "no".
fn tests_touched_in(episodes: &[Episode], from: usize, to: usize) -> Option<bool> {
    let mut any = false;
    for episode in episodes.iter().take(to + 1).skip(from) {
        match episode.tests_touched {
            Some(true) => return Some(true),
            Some(false) => any = true,
            None => return None,
        }
    }
    any.then_some(false)
}

/// Does this path name a dependency manifest?
fn is_manifest(path: &str) -> bool {
    let lower = path.to_lowercase();
    let file = lower.rsplit('/').next().unwrap_or(lower.as_str());
    MANIFESTS
        .iter()
        .any(|name| file == *name || file.starts_with(name))
}
