//! Reading one episode's feature vector out of an index (`reco-884`).
//!
//! The scorer takes plain data; something has to turn a store row into that data, and this is that
//! something. It is the only file in `risk` that holds a [`Store`] or a [`Repository`] — the same shape
//! [`crate::detectors::collect`] chose, for the same reason: a measurement that cannot be built without a
//! database is a measurement that cannot be tested without one, and every number here has a fixture that
//! does not need either.
//!
//! # Where each measurement comes from
//!
//! | feature | read |
//! |---|---|
//! | `findings` | the `e4.3` catalogue, run over the episode with the last [`CORPUS_LIMIT`] episodes as its corpus |
//! | `surface`, `breadth`, `size`, `noise` | the episode's own per-path rows (`episode_member ⋈ git_commit_file`) |
//! | `migration` | those paths, plus the added lines of any file that reads as a migration |
//! | `test_ratio`, `deletions` | those paths, split by surface |
//! | `dependency` | those paths, split into lockfiles and manifests, plus anything a caller supplies |
//! | `attribution`, `authorship` | the `episode` row's attribution columns (`e4.1`) |
//! | `blast` | whatever a caller hands over, and otherwise a *question asked of the index* — see below |
//!
//! # The one authorship verdict that is not a measurement
//!
//! `authorship` is [`Measurement::missing`] on a `human` episode whose verdict came from the `identity`
//! rung, and the reason printed beside the `—` says why. ADR-003 §8 **H4** prices that rung at 0.50 on
//! purpose: it fires when every rung above it looked for an agent and found nothing, so it is a sentence
//! about the evidence and not about the author. Score it as `0.0` — "a person wrote this" — and every
//! repository whose harness stamps no turn and no claim trailer gets the *same* number as the one where a
//! human signed the correction saying so, including the repositories this tool was written for
//! (`reco-0er`: 102 of 109 episodes in an agent's own repository read `human` at 0.50, so the thesis axis
//! contributed 0.00 in all of them). Absent is the honest state, and it is a state with a door: turn
//! evidence, a claim trailer, a registry entry or a stored correction each put a number on the axis.
//!
//! # The one hole, asked rather than recited
//!
//! `blast` is [`Measurement::missing`] unless a caller fills [`Supply::blast_dependents`], and the reason
//! printed beside that `—` used to be a sentence about a plan: "the module graph is `e3.1`'s and the graph
//! delta is `e3.2`'s, and neither is built". Epic 3 closed that work and `e3.11` wired the graph lane into
//! `fathom index`, so the sentence became a lie that every `fathom why` run told — a reviewer was informed
//! that an axis was waiting on a graph their own index already holds.
//!
//! It is a question now. [`super::blast::Availability::probe`] asks this store whether it has a graph lane
//! and whether this episode's paths are modules in it, and the printed reason is whichever answer came
//! back: no lane, a lane that has no vertex for any of these paths, or a lane that *could* answer and was
//! not asked (the count is a chain walk, and this verb does not spend that budget). A co-change proxy —
//! "how much recent work happened in the same directories" — remains refused for the reason it was refused
//! before: it correlates with nothing a reviewer would call blast radius, and a proxy that flatters itself
//! is worse than a hole with a name. [`Supply`] is still the door a real number walks through, and the
//! calibration set scores with it, so nothing downstream changes when somebody pays for the walk.
//!
//! # Caps, and what they cost
//!
//! The content read is the detectors' own ([`DEFAULT_MAX_PATHS`]), and a read that hit a cap drops the
//! features that depend on text to a stated caveat rather than a quiet zero. The corpus read is bounded
//! so that `fathom why <episode>` costs a bounded number of queries whatever the repository's size
//! (ADR-004 §7): three for the episode, two for the corpus, one for the file rows.

use std::collections::BTreeMap;

use fathom_ingest::record::Limits;
use fathom_ingest::repo::Repository;
use fathom_store::episode::{EpisodeQuery, EpisodeRow};
use fathom_store::spine::Store;

use crate::detectors::{self, DetectorEpisode, DetectorPolicy, Finding, Report};
use crate::risk::blast::Availability;
use crate::risk::feature::{Feature, Measurement, Vector};
use crate::risk::{Surface, classify};

/// How many earlier episodes the detectors are shown as history.
///
/// Two of them need a sequence to answer at all, and a repository with fifty thousand commits does not
/// need fifty thousand of them read to tell whether the last three episodes kept returning to one file.
/// Newest-first from the store, reversed into the order [`crate::detectors::model::Corpus`] asks for.
pub const CORPUS_LIMIT: i64 = 60;

/// What a caller can hand over that this build cannot measure itself.
///
/// Every field is a door a finished ticket will walk through: `blast` is a dependent count from the graph
/// lane (`fathom index` writes it; nothing walks it for a score yet), the dependency counts are `e2.6`'s
/// delta when a caller has run it, and `migration_reversible` is `e2.8`'s non-code rule. Nothing here is
/// guessed at in this crate's absence; [`Supply::nothing`] is the honest default and the one the shipped
/// verb uses.
#[derive(Debug, Clone, Copy, Default)]
pub struct Supply {
    /// How many paths depend on what this episode touched.
    pub blast_dependents: Option<u64>,
    /// How sure the caller is of that count, `0.0..=1.0`, printed with it.
    pub blast_confidence: Option<f64>,
    /// Direct dependencies the episode added.
    pub dependency_direct_adds: Option<u64>,
    /// Dependencies that moved a major version.
    pub dependency_major_moves: Option<u64>,
    /// `e2.8`'s answer about a migration this episode touched: `Some(false)` means "the tool read the
    /// history and there is no reversal".
    pub migration_reversible: Option<bool>,
}

impl Supply {
    /// Nothing supplied — what `fathom why` uses today.
    #[must_use]
    pub const fn nothing() -> Self {
        Supply {
            blast_dependents: None,
            blast_confidence: None,
            dependency_direct_adds: None,
            dependency_major_moves: None,
            migration_reversible: None,
        }
    }
}

/// One episode read, and everything the vector is built from.
#[derive(Debug)]
pub struct Read {
    /// The vector: twelve measurements, all of them accounted for.
    pub vector: Vector,
    /// The detector report behind the `findings` measurement, kept so a surface can print the findings
    /// themselves beside the number they produced.
    pub report: Report,
}

/// Read one episode's feature vector.
///
/// `Ok(None)` when the index has no such episode — the caller turns that into the verb's refusal, which
/// names the remedy.
///
/// # Errors
///
/// [`fathom_store::error::Error`] when a read fails. A read that *partly* failed is not an error: it is a
/// caveat on a measurement, because half a score with a stated hole beats no score at all.
pub fn read(
    store: &Store,
    repo: &Repository,
    id: &str,
    policy: &DetectorPolicy,
    supply: &Supply,
) -> Result<Option<Read>, fathom_store::error::Error> {
    let Some(row) = store.episode(id)? else {
        return Ok(None);
    };
    let reader = detectors::collect::Reader::new(store, repo, Limits::default());
    let episode = match reader.one(id)? {
        Some(episode) => episode,
        None => return Ok(None),
    };
    let corpus_episodes = corpus(store)?;
    let corpus = detectors::model::Corpus::of(&corpus_episodes);
    let report = detectors::scan(&episode, &corpus, policy);
    // Asked of the index, on the paths of the episode being scored, so that the reason printed beside a
    // missing `blast` is an answer about this store rather than a sentence about a plan (`reco-j1o`).
    let blast = Availability::probe(store, episode.files.iter().map(|file| file.path.as_str()))?;
    let vector = vector(&row, &episode, &report, supply, &blast);
    Ok(Some(Read { vector, report }))
}

/// The corpus the history-shaped detectors are allowed to see: recent episodes, as rows, oldest first.
///
/// Rows only — paths, dates and authorship. Rendering hunks for sixty episodes to answer one question
/// about one of them is a cost the verb does not pay, and the detectors that read history read paths.
fn corpus(store: &Store) -> Result<Vec<DetectorEpisode>, fathom_store::error::Error> {
    let query = EpisodeQuery {
        limit: CORPUS_LIMIT,
        ..EpisodeQuery::default()
    };
    let rows = store.episodes(&query)?;
    let ids: Vec<String> = rows.iter().map(|row| row.id.clone()).collect();
    let files = store.episode_files(&ids)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows.iter().rev() {
        let empty = Vec::new();
        let rows_for = files.by_episode.get(&row.id).unwrap_or(&empty);
        out.push(DetectorEpisode {
            id: row.id.clone(),
            summary: row.summary.clone(),
            author_kind: row.author_kind.clone(),
            when_utc: row.first_utc,
            lineage_ref: row.lineage_ref.clone(),
            lineage_seq: row.lineage_seq,
            session_id: row.session_id.clone(),
            n_commits: row.n_commits(),
            declared_scope: Vec::new(),
            files: rows_for
                .iter()
                .map(|file| {
                    let mut change =
                        detectors::model::FileChange::row(&file.path, file.add, file.del);
                    change.path_class = file.path_class.clone();
                    change.noise_class = file.noise_class.clone();
                    change.binary = file.binary;
                    change
                })
                .collect(),
            content: detectors::ContentState::Absent,
            ..DetectorEpisode::new(row.id.clone())
        });
    }
    Ok(out)
}

/// Build the vector from the rows, the content read and the scan.
///
/// `blast` is what the index answered when the read asked it whether the module graph could say who
/// depends on this episode's paths ([`Availability::probe`]); a caller that has no store to ask passes
/// [`Availability::NotProbed`] and gets a reason that says exactly that.
#[must_use]
pub fn vector(
    row: &EpisodeRow,
    episode: &DetectorEpisode,
    report: &Report,
    supply: &Supply,
    blast: &Availability,
) -> Vector {
    let mut measurements = Vec::with_capacity(Feature::ALL.len());
    let mut caveats: Vec<String> = Vec::new();
    caveats.extend(report.gaps.clone());
    caveats.extend(episode.notices.clone());

    // Per-surface rollups, in one pass over the paths. `BTreeMap` so the order is the surface order and
    // not a hash's: these rollups are the printed evidence.
    let mut by_surface: BTreeMap<Surface, (u64, u64, usize)> = BTreeMap::new();
    for file in &episode.files {
        let surface = classify(&file.path, &file.path_class, &file.noise_class);
        let entry = by_surface.entry(surface).or_insert((0, 0, 0));
        entry.0 = entry
            .0
            .saturating_add(u64::try_from(file.add.max(0)).unwrap_or(u64::MAX));
        entry.1 = entry
            .1
            .saturating_add(u64::try_from(file.del.max(0)).unwrap_or(u64::MAX));
        entry.2 = entry.2.saturating_add(1);
    }
    let churn = u64::try_from(row.n_add.max(0))
        .unwrap_or(0)
        .saturating_add(u64::try_from(row.n_del.max(0)).unwrap_or(0));
    let sum = |want: fn(Surface) -> bool| -> (u64, u64, usize) {
        by_surface
            .iter()
            .filter(|(surface, _)| want(**surface))
            .fold((0, 0, 0), |acc, (_, (add, del, files))| {
                (
                    acc.0.saturating_add(*add),
                    acc.1.saturating_add(*del),
                    acc.2.saturating_add(*files),
                )
            })
    };

    measurements.push(findings(report));
    measurements.push(surface(&by_surface, episode, resolver_agent(row)));
    measurements.push(migration(
        row,
        episode,
        by_surface.get(&Surface::Migration).copied(),
        supply,
    ));
    measurements.push(blast_measurement(supply, blast));
    measurements.push(test_ratio(&sum, churn));
    measurements.push(size(row, &sum, churn));
    measurements.push(dependency(episode, &sum, supply));
    measurements.push(deletions(&sum, churn, row));
    measurements.push(noise(row, &sum, churn));
    measurements.push(breadth(&by_surface, row));
    measurements.push(attribution(row));
    measurements.push(authorship(row));

    let mut vector = Vector::new(row.id.clone(), measurements);
    if !episode.content.examined() {
        caveats.push(format!(
            "the hunks were not read ({}), so every measurement that reads text says so instead of \
             claiming to be clean",
            episode.content.as_str()
        ));
    }
    vector.caveats = caveats;
    vector
}

/// How much of the diff a finding is worth: `high` is three points, `medium` two, `low` one.
fn severity_points(finding: &Finding) -> u64 {
    match finding.severity {
        detectors::Severity::High => 3,
        detectors::Severity::Medium => 2,
        detectors::Severity::Low => 1,
    }
}

fn findings(report: &Report) -> Measurement {
    let ran = report
        .runs
        .iter()
        .any(|run| run.outcome == detectors::Outcome::Ran);
    if !ran {
        return Measurement::missing(
            Feature::Findings,
            "no detector answered for this episode: its content was never read, so the catalogue has \
             no answer rather than a clean one",
            "e4.3",
        );
    }
    let points: u64 = report.findings.iter().map(severity_points).sum();
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    for finding in &report.findings {
        *counts.entry(finding.severity.as_str()).or_insert(0) += 1;
    }
    let detail = if report.findings.is_empty() {
        format!(
            "the catalogue ran and raised nothing ({} of {} detectors answered)",
            report
                .runs
                .iter()
                .filter(|run| run.outcome == detectors::Outcome::Ran)
                .count(),
            report.runs.len()
        )
    } else {
        let parts: Vec<String> = counts
            .iter()
            .map(|(severity, count)| format!("{count} {severity}"))
            .collect();
        let worst = report
            .findings
            .iter()
            .take(2)
            .map(|finding| format!("{} on {}", finding.detector_id, finding.subject))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "{} finding(s): {} · {}",
            report.findings.len(),
            parts.join(", "),
            worst
        )
    };
    Measurement::measured(Feature::Findings, points as f64, "e4.3 catalogue", detail)
}

fn surface(
    by_surface: &BTreeMap<Surface, (u64, u64, usize)>,
    episode: &DetectorEpisode,
    resolver: Option<&str>,
) -> Measurement {
    let Some((top, _)) = by_surface
        .iter()
        .max_by_key(|(surface, _)| (surface.weight(), *surface))
    else {
        return Measurement::missing(
            Feature::Surface,
            "the index recorded no file rows for this episode, so there is no kind of file to name",
            "e4.4",
        );
    };
    // A bumping service moved the pins, so the surfaces whose claim is "an operator must act on it" are
    // read as what the episode actually did: the build now depends on different versions. See
    // [`Surface::resolver_capped`] and [`fathom_ingest::attribute::RESOLVER_AGENTS`].
    let claimed = resolver.map_or(*top, |_| top.resolver_capped());
    let capped = (claimed != *top).then_some(*top);
    let quoted = episode
        .files
        .iter()
        .filter(|file| classify(&file.path, &file.path_class, &file.noise_class) == *top)
        .max_by_key(|file| file.churn())
        .map(|file| file.path.clone());
    let mut detail = match quoted {
        Some(path) => format!("{path} — {}", claimed.describe()),
        None => claimed.describe().to_owned(),
    };
    if claimed.operator() {
        detail.push_str(" · an operator must act on it");
    }
    if let (Some(from), Some(agent)) = (capped, resolver) {
        detail.push_str(&format!(
            " · {agent} moved pins under a {} path: the pins are the change, and no {} act is \
             claimed for a resolver's bump",
            from.as_str(),
            from.as_str()
        ));
    }
    let others: Vec<&str> = by_surface
        .keys()
        .filter(|surface| *surface != top)
        .map(|surface| surface.as_str())
        .collect();
    if !others.is_empty() {
        let shown: Vec<&str> = others.iter().copied().take(4).collect();
        detail.push_str(&format!(" · also touched: {}", shown.join(", ")));
    }
    Measurement::measured(
        Feature::Surface,
        claimed.weight() as f64,
        "path class",
        detail,
    )
}

/// The attributed agent, when the attribution named one of the bumping services.
///
/// `e4.1` writes the registry id into `episode.attribution_agent`, so this reads a stored fact rather
/// than pattern-matching an author string a second time — the same fact the card prints as "written by
/// dependabot", read here for the one question the ranking asks of it.
fn resolver_agent(row: &EpisodeRow) -> Option<&str> {
    fathom_ingest::attribute::is_resolver_agent(&row.attribution_agent)
        .then_some(row.attribution_agent.as_str())
}

/// The statements in a migration that may not be reversible.
const DESTRUCTIVE: [&str; 6] = [
    "drop table",
    "drop column",
    "alter table",
    "delete from",
    "truncate",
    "drop index",
];

fn migration(
    row: &EpisodeRow,
    episode: &DetectorEpisode,
    touched: Option<(u64, u64, usize)>,
    supply: &Supply,
) -> Measurement {
    let files = touched.map_or(0, |(_, _, files)| files);
    // The text half: what did the added lines of the migration actually say. Only trusted when the hunks
    // were read at all — an unread file is not a clean file.
    let mut statement: Option<String> = None;
    if episode.content.examined() {
        for file in episode.files.iter().filter(|file| {
            classify(&file.path, &file.path_class, &file.noise_class) == Surface::Migration
        }) {
            for line in file.added() {
                let lower = line.to_ascii_lowercase();
                if let Some(found) = DESTRUCTIVE.iter().find(|needle| lower.contains(**needle)) {
                    let trimmed = line.trim();
                    let shown: String = trimmed.chars().take(60).collect();
                    statement = Some(format!("{found}: `{shown}` in {}", file.path));
                    break;
                }
            }
            if statement.is_some() {
                break;
            }
        }
    }
    let reversible = match supply.migration_reversible {
        Some(false) => Some("e2.8 read the history and found no reversal"),
        Some(true) => None,
        None => None,
    };
    let (raw, detail, basis) = if let Some(statement) = statement {
        (
            1.0,
            format!(
                "{statement} · the reversal is not in the diff{}",
                reversible.map_or(String::new(), |note| format!(" · and {note}"))
            ),
            "the added lines of a migration".to_owned(),
        )
    } else if files > 0 {
        (
            f64::from(u32::try_from(files).unwrap_or(u32::MAX)) * 0.55,
            format!(
                "{files} migration file(s) touched, nothing in the added lines that names a \
                 destructive statement"
            ),
            "path class".to_owned(),
        )
    } else {
        (
            0.0,
            "no schema history in this change".to_owned(),
            "path class".to_owned(),
        )
    };
    let _ = row;
    Measurement::measured(Feature::Migration, raw, basis, detail)
}

fn blast_measurement(supply: &Supply, index: &Availability) -> Measurement {
    match supply.blast_dependents {
        Some(dependents) => {
            let confidence = supply.blast_confidence.unwrap_or(1.0);
            Measurement::measured(
                Feature::Blast,
                f64::from(u32::try_from(dependents).unwrap_or(u32::MAX)),
                format!("supplied at confidence {confidence:.2}"),
                format!(
                    "{dependents} path(s) depend on what this touched ({} the caller's graph)",
                    if confidence >= 0.9 {
                        "measured in"
                    } else {
                        "estimated from"
                    }
                ),
            )
        }
        None => Measurement::missing(Feature::Blast, index.sentence(), index.owner()),
    }
}

/// The additions that are logic, and the additions that are its cover.
fn test_ratio(sum: &impl Fn(fn(Surface) -> bool) -> (u64, u64, usize), churn: u64) -> Measurement {
    let (test_add, test_del, _) = sum(Surface::is_test);
    let (source_add, source_del, _) = sum(Surface::is_source);
    let new_logic = source_add;
    let new_cover = test_add;
    let shrank = test_del >= 20 && test_del > test_add;
    if new_logic + new_cover == 0 && !shrank {
        return Measurement::missing(
            Feature::TestRatio,
            "neither logic nor tests arrived, so coverage is not a question this episode asks",
            "e4.4",
        );
    }
    let share = if new_logic + new_cover == 0 {
        0.0
    } else {
        f64::from(u32::try_from(new_cover).unwrap_or(u32::MAX))
            / f64::from(u32::try_from(new_logic + new_cover).unwrap_or(u32::MAX))
    };
    let detail = if shrank {
        format!(
            "the test surface shrank: −{test_del} against +{test_add} on tests while source moved \
             +{source_add} −{source_del} · coverage is a floor of 0.75 here, not a share"
        )
    } else if new_logic == 0 {
        format!("+{new_cover} lines of tests and no new source: the cover grew")
    } else {
        format!(
            "of +{} on logic, +{} was tests ({:.0}% covered) over {churn} lines of churn",
            new_logic,
            new_cover,
            share * 100.0
        )
    };
    let raw = if shrank {
        0.75_f64.min(1.0 - share)
    } else {
        share
    };
    Measurement::measured(
        Feature::TestRatio,
        raw,
        "per-path add/del by surface",
        detail,
    )
}

fn size(
    row: &EpisodeRow,
    sum: &impl Fn(fn(Surface) -> bool) -> (u64, u64, usize),
    churn: u64,
) -> Measurement {
    let (noisy_churn, _, _) = sum(|surface| surface.noisy() || surface == Surface::Binary);
    let binary = u64::try_from(row.n_binary.max(0)).unwrap_or(0);
    let discounted = churn.saturating_sub(noisy_churn.saturating_mul(3) / 4);
    let detail = if noisy_churn > 0 {
        format!(
            "+{} −{} = {churn} churn, of which {discounted} is reviewable ({noisy_churn} discounted \
             as generated, lockfile, vendored or binary; {binary} binary file(s))",
            row.n_add.max(0),
            row.n_del.max(0)
        )
    } else {
        format!(
            "+{} −{} = {churn} churn, all of it hand-written",
            row.n_add.max(0),
            row.n_del.max(0)
        )
    };
    Measurement::measured(Feature::Size, discounted as f64, "episode rollup", detail)
}

/// Lockfiles and manifests, told apart because they ask for different attention: a lockfile records what
/// a resolver chose, a manifest records what a person asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyFile {
    /// `uv.lock`, `Cargo.lock`, `go.sum`, `package-lock.json`, `requirements.txt`.
    Lock,
    /// `pyproject.toml`, `package.json`, `Cargo.toml`, `go.mod`, `pom.xml`.
    Manifest,
    /// Not a dependency surface at all.
    NotDependency,
}

/// Which half of the dependency surface a path is, by name.
#[must_use]
pub fn dependency_shape(path: &str) -> DependencyFile {
    let lower = path.to_ascii_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(lower.as_str());
    const LOCKS: [&str; 14] = [
        "uv.lock",
        "cargo.lock",
        "package-lock.json",
        "yarn.lock",
        "pnpm-lock.yaml",
        "poetry.lock",
        "pipfile.lock",
        "gemfile.lock",
        "composer.lock",
        "mix.lock",
        "go.sum",
        "packages.lock.json",
        "gradle.lockfile",
        "requirements",
    ];
    const MANIFESTS: [&str; 10] = [
        "pyproject.toml",
        "package.json",
        "cargo.toml",
        "go.mod",
        "gemfile",
        "pom.xml",
        "build.gradle",
        "build.gradle.kts",
        "setup.py",
        "requirements.txt",
    ];
    if LOCKS
        .iter()
        .any(|lock| name == *lock || name.starts_with(lock))
    {
        DependencyFile::Lock
    } else if MANIFESTS.contains(&name) {
        DependencyFile::Manifest
    } else {
        DependencyFile::NotDependency
    }
}

fn dependency(
    episode: &DetectorEpisode,
    sum: &impl Fn(fn(Surface) -> bool) -> (u64, u64, usize),
    supply: &Supply,
) -> Measurement {
    let (_, _, touched) = sum(|surface| surface == Surface::Dependency);
    let mut locks = 0_usize;
    let mut manifests = 0_usize;
    let mut named: Vec<String> = Vec::new();
    for file in &episode.files {
        match dependency_shape(&file.path) {
            DependencyFile::Lock => {
                locks += 1;
                named.push(file.path.clone());
            }
            DependencyFile::Manifest => {
                manifests += 1;
                named.push(file.path.clone());
            }
            DependencyFile::NotDependency => {}
        }
    }
    let adds = supply.dependency_direct_adds.unwrap_or(0);
    let majors = supply.dependency_major_moves.unwrap_or(0);
    let raw = if majors > 0 {
        1.0
    } else if adds > 0 {
        0.8
    } else {
        f64::from(u32::try_from(locks).unwrap_or(u32::MAX)) * 0.55
            + f64::from(u32::try_from(manifests).unwrap_or(u32::MAX)) * 0.35
    };
    let detail = if touched == 0 {
        "no dependency manifest or lockfile in this change".to_owned()
    } else {
        let shown: Vec<&str> = named.iter().map(String::as_str).take(3).collect();
        format!(
            "{locks} lockfile(s) and {manifests} manifest(s): {}{} · the resolution is what the lock \
             recorded, not a re-solve",
            shown.join(", "),
            if named.len() > shown.len() {
                format!(" and {} more", named.len() - shown.len())
            } else {
                String::new()
            }
        )
    };
    Measurement::measured(Feature::Dependency, raw, "path class", detail)
}

fn deletions(
    sum: &impl Fn(fn(Surface) -> bool) -> (u64, u64, usize),
    churn: u64,
    row: &EpisodeRow,
) -> Measurement {
    let (test_add, test_del, _) = sum(Surface::is_test);
    let (source_add, source_del, _) = sum(Surface::is_source);
    let removed = u64::try_from(row.n_del.max(0)).unwrap_or(0);
    let share = if churn == 0 {
        0.0
    } else {
        f64::from(u32::try_from(removed).unwrap_or(u32::MAX))
            / f64::from(u32::try_from(churn).unwrap_or(u32::MAX))
    };
    let shrank = test_del >= 20 && test_del > test_add;
    let base = if churn < 40 {
        0.0
    } else if share >= 0.6 {
        0.6
    } else {
        share / 0.6 * 0.6
    };
    let raw = if shrank { base.max(0.75) } else { base };
    let detail = if shrank {
        format!(
            "the test surface shrank while source moved: −{test_del} tests against +{source_add} \
             source (−{source_del}), and {removed} of {churn} churn lines are removal"
        )
    } else if churn == 0 {
        "no churn to weigh".to_owned()
    } else {
        format!(
            "{:.0}% of the churn is removal (−{} against +{})",
            share * 100.0,
            removed,
            u64::try_from(row.n_add.max(0)).unwrap_or(0)
        )
    };
    Measurement::measured(
        Feature::Deletions,
        raw,
        "per-path add/del by surface",
        detail,
    )
}

fn noise(
    row: &EpisodeRow,
    sum: &impl Fn(fn(Surface) -> bool) -> (u64, u64, usize),
    churn: u64,
) -> Measurement {
    let (noise_add, noise_del, noise_files) =
        sum(|surface| surface.noisy() || surface == Surface::Binary);
    let noise_churn = noise_add.saturating_add(noise_del);
    // `e2.2`'s measured share wins when the indexer stamped one; a stamp from the noise policy beats a
    // path guess, and says so.
    if row.noise_share > 0.0 {
        return Measurement::measured(
            Feature::Noise,
            row.noise_share,
            "`episode.noise_share` (e2.2, measured)",
            format!(
                "{:.0}% of this episode's churn is noise by the repository's noise policy",
                row.noise_share * 100.0
            ),
        );
    }
    let share = if churn == 0 {
        0.0
    } else {
        f64::from(u32::try_from(noise_churn).unwrap_or(u32::MAX))
            / f64::from(u32::try_from(churn).unwrap_or(u32::MAX))
    };
    let detail = if noise_churn == 0 {
        "nothing in this change is generated, locked, vendored or binary".to_owned()
    } else {
        format!(
            "{noise_churn} of {churn} churn lines sit on generated, lockfile, vendored or binary \
             paths ({noise_files} file(s)) · classified by path, not by token comparison"
        )
    };
    Measurement::measured(Feature::Noise, share, "path class", detail)
}

fn breadth(by_surface: &BTreeMap<Surface, (u64, u64, usize)>, row: &EpisodeRow) -> Measurement {
    let surfaces = by_surface.len();
    let files = u64::try_from(row.n_files.max(0)).unwrap_or(0);
    let spread =
        f64::from(u32::try_from(surfaces.saturating_sub(1).min(5)).unwrap_or(u32::MAX)) * 0.15;
    let files_share = if files >= 30 {
        0.4
    } else if files >= 12 {
        0.2
    } else {
        0.0
    };
    let commits = u64::try_from(row.n_commits()).unwrap_or(0);
    let raw = spread + files_share;
    Measurement::measured(
        Feature::Breadth,
        raw,
        "episode rollup",
        format!(
            "{surfaces} surface(s) over {files} file(s) in {commits} commit(s){}",
            if commits > 3 {
                " · one episode that kept coming back"
            } else {
                ""
            }
        ),
    )
}

fn attribution(row: &EpisodeRow) -> Measurement {
    let kind = row.author_kind.trim().to_ascii_lowercase();
    let confidence = if kind == "unknown" {
        0.0
    } else {
        row.author_confidence.clamp(0.0, 1.0)
    };
    let detail = if kind == "unknown" {
        let reason = if row.attribution_reason.is_empty() {
            "and nothing says why".to_owned()
        } else {
            row.attribution_reason.clone()
        };
        format!("nobody is credited, which is its own risk (ADR-001 §7.7) · {reason}")
    } else {
        let source = if row.attribution_source.is_empty() {
            "an unrecorded rule"
        } else {
            row.attribution_source.as_str()
        };
        format!(
            "credited to {kind} by {source} at confidence {confidence:.2} · the doubt is what this \
             axis measures, not the label"
        )
    };
    Measurement::measured(
        Feature::Attribution,
        confidence,
        "`episode.author_confidence`",
        detail,
    )
}

/// The price ADR-003 §8 **H4** puts on "a real identity with no agent evidence".
///
/// Named here rather than typed so the two places that read it — the rung that sets it and the axis that
/// refuses to score it — cannot drift apart.
const CONFIDENCE_IDENTITY: f64 = 0.50;

/// Did this row's authorship verdict come from evidence, or from the absence of it?
///
/// `identity` is the attribution ladder's fall-through: nothing above it found an agent, so the classifier
/// said "there is a name here, so probably a person" at 0.50 and marked the guess (`e4.1`). A *stated*
/// `human` — a stored correction, the one rung that can assert a person — arrives with its own source and
/// its 1.00, and measures.
#[must_use]
pub fn verdict_without_evidence(row: &EpisodeRow) -> bool {
    if row.author_confidence.clamp(0.0, 1.0) > CONFIDENCE_IDENTITY {
        return false;
    }
    let source = row.attribution_source.trim().to_ascii_lowercase();
    // An empty source is the same case in older rows: no rung was recorded, and the only rung that can
    // leave a `human` at 0.50 or below is the one that recorded nothing because it found nothing.
    source.is_empty() || source == "identity"
}

fn authorship(row: &EpisodeRow) -> Measurement {
    let kind = row.author_kind.trim().to_ascii_lowercase();
    if kind == "human" && verdict_without_evidence(row) {
        return Measurement::missing(
            Feature::Authorship,
            "nothing on this episode says who wrote it: `human` is what the ladder falls through to when \
             no agent evidence exists, at 0.50, and reading that as a person would credit somebody who \
             was never named. Turn evidence from a harness, a claim trailer naming an agent, \
             `attribution.agents` naming this identity, or a stored `fathom attribution correct` each \
             put a measurement on this axis",
            "the harness",
        );
    }
    let (raw, detail) = match kind.as_str() {
        "agent" => {
            let agent = if row.attribution_agent.is_empty() {
                "an agent"
            } else {
                row.attribution_agent.as_str()
            };
            (
                0.7,
                format!("{agent} wrote this, which is the reason this tool exists (ADR-001 §7.3)"),
            )
        }
        "human" => (0.0, "a person wrote this, and is named as having done so".to_owned()),
        _ => (
            1.0,
            "nobody is credited: the attention is for the unknown, not for the person who might be \
             behind it"
                .to_owned(),
        ),
    };
    Measurement::measured(Feature::Authorship, raw, "`episode.author_kind`", detail)
}

#[cfg(test)]
mod tests {
    use super::verdict_without_evidence;
    use fathom_store::episode::EpisodeRow;

    /// A row carrying only what the authorship axis reads.
    fn row(kind: &str, source: &str, confidence: f64) -> EpisodeRow {
        EpisodeRow {
            author_kind: kind.to_owned(),
            attribution_source: source.to_owned(),
            author_confidence: confidence,
            ..EpisodeRow::default()
        }
    }

    #[test]
    fn the_fall_through_is_the_only_human_that_measures_nothing() {
        // The rung that fires when nothing above it found an agent (`reco-0er`'s 102 of 109).
        assert!(verdict_without_evidence(&row("human", "identity", 0.50)));
        // A row written before the rung was recorded is read the same way: nothing was said.
        assert!(verdict_without_evidence(&row("human", "", 0.50)));
        // A human a person *said* so — the one rung that can assert a person — is evidence.
        assert!(!verdict_without_evidence(&row("human", "correction", 1.00)));
        // An agent verdict came from a rung that found something, whatever its price.
        assert!(!verdict_without_evidence(&row("agent", "trailer", 0.95)));
        assert!(!verdict_without_evidence(&row("agent", "author", 0.85)));
        // `unknown` is its own answer — nobody is credited, which is a fact about the row, not an absence.
        assert!(!verdict_without_evidence(&row("unknown", "absent", 0.0)));
    }
}
