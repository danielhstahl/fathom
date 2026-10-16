//! `verify.rs` — does the repository that came out still mean what the ground-truth file says it means?
//!
//! This is the half that keeps a golden corpus from quietly rotting. A scenario's function and its
//! ground-truth file are written at the same moment by the same person, and only one of them is executed on
//! every run: without this file, editing a scenario to add a commit would leave the ground truth describing
//! the old scenario, and every later ticket would be scoring itself against a description of a repository
//! that no longer exists.
//!
//! So the checks are the boring, absolute kind:
//!
//! * the blessed digests match, or the ground truth has not been blessed and says so;
//! * the episodes tile the history — every commit belongs to exactly one, each run is contiguous, and the
//!   author, session and turn an episode claims are the ones on its first commit;
//! * every finding names a commit that exists, an episode that exists, and paths that are actually in that
//!   repository;
//! * every claim is re-checked against the bytes the generator wrote.
//!
//! None of it is analysis of the repository in the product's sense. "These two versions differ only in
//! whitespace" is a fact about the fixture's own text and the fixture is the thing being checked; the
//! stronger claim, that an episode is *noise*, belongs to `e2.2` and is deliberately not made here
//! (ADR-002 §4.4).

use std::collections::BTreeMap;

use crate::generate::Build;
use crate::git::File;
use crate::truth::{Claim, GroundTruth};

/// The boundary rules `fathom-ingest`'s cascade can name, and therefore the only ones a ground-truth episode
/// may claim. ADR-003 §4.2 is the list; `override` is there because a scenario may document a forced split
/// even though no fixture ships a `fathom.toml` that forces one.
const KNOWN_RULES: &[&str] = &[
    "root",
    "hook",
    "trailer",
    "override",
    "branch",
    "author",
    "time-gap",
    "message-shape",
];

/// The expectations a finding may carry. They are labels for the consumer, and the set is closed so a typo
/// in a ground-truth file is an error rather than a finding nothing will ever look for.
const KNOWN_EXPECTS: &[&str] = &["fire", "silent", "collapse", "survive"];

/// Check one built fixture against its ground truth.
///
/// Every disagreement is returned as a sentence, not as the first panic: a scenario with three problems
/// should tell you about three problems.
pub(crate) fn verify(
    build: &Build,
    truth: &GroundTruth,
    digest: &str,
    events_count: usize,
) -> Vec<String> {
    let mut failures = Vec::new();
    verify_determinism(build, truth, digest, &mut failures);
    verify_episodes(build, truth, &mut failures);
    verify_findings(build, truth, &mut failures);
    verify_claims(build, truth, events_count, &mut failures);
    failures
}

fn verify_determinism(
    build: &Build,
    truth: &GroundTruth,
    digest: &str,
    failures: &mut Vec<String>,
) {
    let blessed = &truth.determinism;
    let Some(expected) = blessed.digest.as_deref() else {
        failures.push(
            "ground truth has not been blessed: run `cargo run -p fathom-fixtures -- --scenario <name> \
             --bless` to stamp the determinism block"
                .to_owned(),
        );
        return;
    };
    if digest != expected {
        failures.push(format!(
            "the repository does not match its blessed digest: expected {expected}, got {digest}"
        ));
    }
    if let Some(count) = blessed.commit_count
        && count != build.visible.len()
    {
        failures.push(format!(
            "blessed commit_count {count}, but {} commits were built",
            build.visible.len()
        ));
    }
    if let Some(head) = blessed.head.as_deref() {
        let actual = build
            .tips
            .get(&format!("refs/heads/{}", build.default_branch))
            .map(String::as_str)
            .unwrap_or_default();
        if actual != head {
            failures.push(format!("blessed head {head}, got {actual}"));
        }
    }
    for (r#ref, sha) in &blessed.tips {
        match build.tips.get(r#ref) {
            Some(actual) if actual == sha => {}
            Some(actual) => failures.push(format!("{ref}: blessed {sha}, got {actual}")),
            None => failures.push(format!(
                "{ref}: blessed, but the repository has no such ref"
            )),
        }
    }
    for (r#ref, sha) in &build.tips {
        if !blessed.tips.contains_key(r#ref) {
            failures.push(format!(
                "{ref} = {sha} exists but is not in the blessed refs"
            ));
        }
    }
}

fn verify_episodes(build: &Build, truth: &GroundTruth, failures: &mut Vec<String>) {
    let mut seen_ids: Vec<&str> = Vec::new();
    let mut claimed: BTreeMap<usize, String> = BTreeMap::new();
    let mut by_lineage: BTreeMap<Option<&str>, Vec<usize>> = BTreeMap::new();
    for episode in &truth.episodes {
        if seen_ids.contains(&episode.id.as_str()) {
            failures.push(format!("episode id `{}` appears twice", episode.id));
        }
        seen_ids.push(&episode.id);
        if !KNOWN_RULES.contains(&episode.rule.as_str()) {
            failures.push(format!(
                "{}: rule `{}` is not one of {}",
                episode.id,
                episode.rule,
                KNOWN_RULES.join(", ")
            ));
        }
        if episode.commits.is_empty() {
            failures.push(format!("{}: covers no commits", episode.id));
            continue;
        }
        for (step, index) in episode.commits.iter().enumerate() {
            if *index >= build.visible.len() {
                failures.push(format!(
                    "{}: names commit {index}, and the fixture has {} commits",
                    episode.id,
                    build.visible.len()
                ));
                continue;
            }
            if step > 0 && *index != episode.commits[step - 1] + 1 {
                failures.push(format!(
                    "{}: commits {:?} are not a contiguous run of the walk",
                    episode.id, episode.commits
                ));
            }
            if let Some(other) = claimed.insert(*index, episode.id.clone()) {
                failures.push(format!(
                    "commit {index} is in both `{other}` and `{}`",
                    episode.id
                ));
            }
        }
        if let Some(first) = episode
            .commits
            .first()
            .and_then(|index| build.entries.get(*index))
        {
            if let Some(author) = &episode.author
                && first.author != author.as_str()
            {
                failures.push(format!(
                    "{}: says `{author}` opened it, commit {} was written by {}",
                    episode.id, first.index, first.author
                ));
            }
            if let Some(kind) = &episode.kind
                && first.author_kind != kind.as_str()
            {
                failures.push(format!(
                    "{}: says kind `{kind}`, commit {} reads `{}`",
                    episode.id, first.index, first.author_kind
                ));
            }
            if let Some(session) = &episode.session
                && first.session.as_ref() != Some(session)
            {
                failures.push(format!(
                    "{}: claims session `{session}`, commit {} has {:?}",
                    episode.id, first.index, first.session
                ));
            }
            if let Some(turn) = &episode.turn
                && first.turn.as_ref() != Some(turn)
            {
                failures.push(format!(
                    "{}: claims turn `{turn}`, commit {} has {:?}",
                    episode.id, first.index, first.turn
                ));
            }
        }
        by_lineage
            .entry(episode.lineage.as_deref())
            .or_default()
            .extend(episode.commits.iter().copied());
    }
    for (lineage, mut covered) in by_lineage {
        covered.sort_unstable();
        covered.dedup();
        let expected: Vec<usize> = (0..build.visible.len())
            .filter(|position| match lineage {
                None => true,
                Some(branch) => build
                    .visible
                    .get(*position)
                    .and_then(|index| build.entries.get(*index))
                    .is_some_and(|entry| entry.branch == branch),
            })
            .collect();
        if covered != expected {
            failures.push(format!(
                "episodes {} do not cover the lineage exactly: covered {covered:?}, expected {expected:?}",
                match lineage {
                    None => "of the walk".to_owned(),
                    Some(branch) => format!("on branch `{branch}`"),
                }
            ));
        }
    }
}

fn verify_findings(build: &Build, truth: &GroundTruth, failures: &mut Vec<String>) {
    let episode_ids: Vec<&str> = truth
        .episodes
        .iter()
        .map(|episode| episode.id.as_str())
        .collect();
    for finding in &truth.findings {
        if !KNOWN_EXPECTS.contains(&finding.expect.as_str()) {
            failures.push(format!(
                "finding `{}`: expect `{}` is not one of {}",
                finding.kind,
                finding.expect,
                KNOWN_EXPECTS.join(", ")
            ));
        }
        if let Some(episode) = &finding.episode
            && !episode_ids.contains(&episode.as_str())
        {
            failures.push(format!(
                "finding `{}` names episode `{episode}`, which no episode is called",
                finding.kind
            ));
        }
        let Some(at) = finding.at else { continue };
        if at >= build.visible.len() {
            failures.push(format!(
                "finding `{}` names commit {at}, and the fixture has {} commits",
                finding.kind,
                build.visible.len()
            ));
            continue;
        }
        for path in &finding.paths {
            if !path_is_known(build, at, path) {
                failures.push(format!(
                    "finding `{}` names `{path}` at commit {at}, and that path is nowhere in that commit's \
                     story: {}",
                    finding.kind, finding.detail
                ));
            }
        }
        for [from, to] in &finding.pairs {
            for path in [from, to] {
                if !path_is_known(build, at, path) {
                    failures.push(format!(
                        "finding `{}` names the rename `{from}` → `{to}` at commit {at}, and `{path}` is \
                         not in that commit's story",
                        finding.kind
                    ));
                }
            }
        }
    }
}

/// Was this path part of that commit, or present in it or its parent?
fn path_is_known(build: &Build, position: usize, path: &str) -> bool {
    let entry = match build
        .visible
        .get(position)
        .and_then(|index| build.entries.get(*index))
    {
        Some(entry) => entry,
        None => return false,
    };
    if entry
        .changes
        .iter()
        .any(|change| change.path == path || change.from.as_deref() == Some(path))
    {
        return true;
    }
    if build
        .tree_at(position)
        .is_some_and(|tree| tree.contains_key(path))
    {
        return true;
    }
    position
        .checked_sub(1)
        .and_then(|previous| build.tree_at(previous))
        .is_some_and(|tree| tree.contains_key(path))
}

fn verify_claims(
    build: &Build,
    truth: &GroundTruth,
    events_count: usize,
    failures: &mut Vec<String>,
) {
    for claim in &truth.claims {
        match claim {
            Claim::PathsChanged { commit, paths } => {
                let Some(entry) = entry_at(build, *commit) else {
                    failures.push(format!("claim paths_changed: no commit {commit}"));
                    continue;
                };
                let actual: Vec<&str> = entry
                    .changes
                    .iter()
                    .map(|change| change.path.as_str())
                    .collect();
                let mut expected = paths.clone();
                expected.sort();
                let mut actual = actual
                    .iter()
                    .map(|path| (*path).to_owned())
                    .collect::<Vec<String>>();
                actual.sort();
                if expected != actual {
                    failures.push(format!(
                        "claim paths_changed at commit {commit}: the ground truth says {expected:?}, the \
                         commit changed {actual:?}"
                    ));
                }
            }
            Claim::WhitespaceOnlyChange { commit, paths } => {
                let Some(entry) = entry_at(build, *commit) else {
                    failures.push(format!("claim whitespace_only_change: no commit {commit}"));
                    continue;
                };
                // The claim is about these paths, not about the whole commit: "this commit reformatted this
                // file and also fixed something" is a true and useful thing to say, and `paths_changed` is
                // how a file says "and nothing else".
                for path in paths {
                    if !entry.changes.iter().any(|change| &change.path == path) {
                        failures.push(format!(
                            "claim whitespace_only_change at commit {commit}: `{path}` is not a path this \
                             commit changed"
                        ));
                    }
                }
                for change in entry
                    .changes
                    .iter()
                    .filter(|change| paths.contains(&change.path))
                {
                    let Some(after) = build
                        .tree_at(*commit)
                        .and_then(|tree| tree.get(&change.path))
                        .map(|file| file.blob.clone())
                    else {
                        continue;
                    };
                    let Some(before) = change
                        .from
                        .as_ref()
                        .and_then(|from| {
                            tree_at(build, commit.saturating_sub(1)).and_then(|t| t.get(from))
                        })
                        .or_else(|| {
                            tree_at(build, commit.saturating_sub(1))
                                .and_then(|t| t.get(&change.path))
                        })
                        .map(|file| file.blob.clone())
                    else {
                        failures.push(format!(
                            "claim whitespace_only_change at commit {commit}: `{}` has no before side",
                            change.path
                        ));
                        continue;
                    };
                    if normalised(&before) != normalised(&after) {
                        failures.push(format!(
                            "claim whitespace_only_change at commit {commit}: `{}` differs by more than \
                             whitespace, which is exactly the lie this claim exists to catch",
                            change.path
                        ));
                    }
                }
            }
            Claim::Rename { commit, pairs } => {
                let Some(entry) = entry_at(build, *commit) else {
                    failures.push(format!("claim rename: no commit {commit}"));
                    continue;
                };
                for [from, to] in pairs {
                    let reported = entry.changes.iter().any(|change| {
                        change.change == "renamed"
                            && change.from.as_ref() == Some(from)
                            && &change.path == to
                    });
                    let same_content = pair_content_equal(build, *commit, from, to);
                    if !reported || !same_content {
                        failures.push(format!(
                            "claim rename at commit {commit}: `{from}` → `{to}` is not a reported rename \
                             with unchanged content (reported {reported}, same content {same_content})"
                        ));
                    }
                }
            }
            Claim::TextContains {
                commit,
                path,
                needle,
            } => match text_at(build, *commit, path) {
                Some(text) if text.contains(needle.as_str()) => {}
                Some(_) => failures.push(format!(
                    "claim text_contains at commit {commit}: `{path}` does not contain `{needle}`"
                )),
                None => failures.push(format!(
                    "claim text_contains at commit {commit}: no `{path}` in that tree"
                )),
            },
            Claim::TextAbsent {
                commit,
                path,
                needle,
            } => {
                if let Some(text) = text_at(build, *commit, path)
                    && text.contains(needle.as_str())
                {
                    failures.push(format!(
                        "claim text_absent at commit {commit}: `{path}` does contain `{needle}`"
                    ));
                }
            }
            Claim::FileAtLeast {
                commit,
                path,
                bytes,
            } => {
                let size = tree_at(build, *commit)
                    .and_then(|tree| tree.get(path))
                    .map(|file| file.blob.len());
                match size {
                    Some(size) if size >= *bytes => {}
                    Some(size) => failures.push(format!(
                        "claim file_at_least at commit {commit}: `{path}` is {size} bytes, not {bytes}"
                    )),
                    None => failures.push(format!(
                        "claim file_at_least at commit {commit}: no `{path}` in that tree"
                    )),
                }
            }
            Claim::BinaryAt { commit, path } => {
                let binary = tree_at(build, *commit)
                    .and_then(|tree| tree.get(path))
                    .is_some_and(File::looks_binary);
                if !binary {
                    failures.push(format!(
                        "claim binary_at at commit {commit}: `{path}` does not read as binary"
                    ));
                }
            }
            Claim::TipIs { r#ref, commit } => {
                let expected = entry_at(build, *commit).map(|entry| entry.sha.clone());
                match (build.tips.get(r#ref), expected) {
                    (Some(actual), Some(expected)) if actual == &expected => {}
                    (Some(actual), Some(expected)) => failures.push(format!(
                        "claim tip_is: {ref} is {actual}, and commit {commit} is {expected}"
                    )),
                    (None, _) => failures.push(format!("claim tip_is: no {ref} in the repository")),
                    (_, None) => failures.push(format!("claim tip_is: no commit {commit}")),
                }
            }
            Claim::RefExists { r#ref } => {
                if !build.tips.contains_key(r#ref) {
                    failures.push(format!("claim ref_exists: no {ref} in the repository"));
                }
            }
            Claim::Gitlink {
                commit,
                path,
                companion,
                at,
            } => {
                let expected = build
                    .companions
                    .get(companion)
                    .and_then(|built| built.entries.get(*at))
                    .map(|entry| entry.sha.clone());
                let actual = entry_at(build, *commit)
                    .and_then(|entry| {
                        entry.gitlinks.iter().find(|gitlink| {
                            &gitlink.path == path && &gitlink.companion == companion
                        })
                    })
                    .map(|gitlink| gitlink.sha.clone());
                match (expected, actual) {
                    (Some(expected), Some(actual)) if expected == actual => {}
                    (Some(expected), Some(actual)) => failures.push(format!(
                        "claim gitlink at commit {commit}: `{path}` points at {actual}, companion \
                         `{companion}` commit {at} is {expected}"
                    )),
                    (None, _) => failures.push(format!(
                        "claim gitlink: companion `{companion}` has no commit {at}"
                    )),
                    (_, None) => failures.push(format!(
                        "claim gitlink at commit {commit}: no gitlink at `{path}` from `{companion}`"
                    )),
                }
            }
            Claim::IsShallow { tips } => {
                let expected: Vec<String> = tips
                    .iter()
                    .filter_map(|position| {
                        entry_at(build, *position).map(|entry| entry.sha.clone())
                    })
                    .collect();
                if build.shallow != expected {
                    failures.push(format!(
                        "claim is_shallow: .git/shallow is {:?}, expected {expected:?}",
                        build.shallow
                    ));
                }
            }
            Claim::EventCount { events } => {
                if &events_count != events {
                    failures.push(format!(
                        "claim event_count: the stream has {events_count} events, the ground truth says {events}"
                    ));
                }
            }
        }
    }
}

fn entry_at(build: &Build, position: usize) -> Option<&crate::generate::CommitEntry> {
    build
        .visible
        .get(position)
        .and_then(|index| build.entries.get(*index))
}

fn tree_at(build: &Build, position: usize) -> Option<&BTreeMap<String, File>> {
    build
        .visible
        .get(position)
        .and_then(|index| build.maps.get(*index))
}

fn text_at(build: &Build, position: usize, path: &str) -> Option<String> {
    tree_at(build, position)
        .and_then(|tree| tree.get(path))
        .map(|file| String::from_utf8_lossy(&file.blob).into_owned())
}

fn pair_content_equal(build: &Build, position: usize, from: &str, to: &str) -> bool {
    let before = position
        .checked_sub(1)
        .and_then(|previous| tree_at(build, previous));
    let after = tree_at(build, position);
    match (before, after) {
        (Some(before), Some(after)) => {
            before.get(from).map(|file| &file.blob) == after.get(to).map(|file| &file.blob)
        }
        _ => false,
    }
}

/// A file with every horizontal whitespace stripped from every line and the blank lines dropped.
///
/// This is the whole of the reformat claim this crate is allowed to make: two versions that differ only this
/// way differ only in whitespace. The token-level, per-language claim `e2.2` makes is deliberately stronger
/// than this, and is made there.
fn normalised(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(|line| {
            line.chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>()
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<String>>()
        .join("\n")
}
