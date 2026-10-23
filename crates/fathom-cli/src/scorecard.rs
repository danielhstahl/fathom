//! `fathom architecture` — the scorecard, headless (`e3.9`, `reco-al1`).
//!
//! This is the composition root's part of the scorecard: open the store, read the config, materialise one
//! revision's graph, hand it to the four architecture lanes that already know how to judge it, write the
//! class-R row the trend is folded from, and print one document. Nothing here decides what a dimension
//! *means* — that is [`fathom_analyze::scorecard`], which is IO-free and golden-testable — and nothing here
//! prints a sentence that module has not already agreed on, because ADR-006 §5.1's binding note says one
//! fact has one wording.
//!
//! # The order the money is spent in
//!
//! ```text
//!   open store ─┬─ graph revisions (newest first)
//!               ├─ read [scorecard]/[layering]/[layers]/[hotspots] + .fathom/ratchet.toml
//!               ├─ Lane::new(&mut store)          ── one lane for the whole run
//!               │     ├─ score the head if it has no row         one chain walk
//!               │     ├─ --backfill N: score N unscored revisions  one chain walk each
//!               │     └─ read the review facts (episodes, review_state)
//!               ├─ drop lane                     ── the store is writable again
//!               ├─ write the rows (unless --read-only)
//!               ├─ read the rows back, newest first → Trend::feed (bounded memory)
//!               └─ resolve the last review → the since-review delta → print
//! ```
//!
//! The lane is dropped before the write because [`fathom_store::spine::Store`] is held mutably by the lane,
//! and a writer that fights its own reader for the connection is a deadlock with extra steps. The rows are
//! read *back* rather than kept in memory, so the trend folds exactly what the next run will find — a trend
//! assembled from what this run happened to compute is a trend that changes shape depending on the flags.
//!
//! # What "we scored it already" means
//!
//! A scorecard row is a function of (revision, rules, analyzer version), so when a row exists the report
//! uses it, including as the head's own numbers: the document cannot then disagree with its own trend. The
//! `--refresh` flag is the escape hatch for a rule change or a model bump, and it is class-R-safe: dropping
//! derived rows and re-deriving them is the honest recovery, never patching a row in place. A row written by
//! a *different* model version is refused into its own counter and named, because comparing two scorecard
//! models as if they were one is how a drift chart starts lying.

use std::path::Path;

use fathom_core::exit::ExitCode;
use fathom_store::episode::{EpisodeQuery, ReviewState};
use fathom_store::spine::Store;

use crate::verbs::ArchitectureArgs;

/// The scorecard, in the shell.
#[cfg(not(feature = "sqlite-bundled"))]
pub fn run(args: &ArchitectureArgs) -> eyre::Result<ExitCode> {
    let _ = args;
    Err(crate::verbs::feature_absent(
        "fathom architecture",
        "sqlite-bundled",
    ))
}

/// The scorecard, in a build that can read the index.
#[cfg(feature = "sqlite-bundled")]
pub fn run(args: &ArchitectureArgs) -> eyre::Result<ExitCode> {
    use crate::output;
    use crate::verbs::{Pending, failed};
    use fathom_analyze::scorecard::Delta;
    use fathom_analyze::scorecard::lane::{Knobs, score};
    use fathom_analyze::scorecard::policy::policy_from_config_text;
    use fathom_analyze::scorecard::report::{Index, Report};
    use fathom_analyze::scorecard::review::resolve;
    use fathom_analyze::scorecard::score::MODEL;
    use fathom_analyze::scorecard::trend::{Parse, Row, Trend};
    use fathom_core::exit::ExitCode as Code;

    let root = Path::new(&args.root);
    let redactor = crate::sanitize::Redactor::for_root(root);
    fathom_ingest::repo::Repository::discover(root).map_err(failed)?;
    let store_path = fathom_analyze::index::default_store_path(root);

    // The knobs, read once: `fathom.toml` for the rules and the weights, `.fathom/ratchet.toml` for the
    // allowances both rule-based dimensions need in order to forgive what has been forgiven.
    let config = std::fs::read_to_string(root.join("fathom.toml")).unwrap_or_default();
    let (mut policy, policy_notices) = policy_from_config_text(&config);
    policy.notices = policy_notices;
    let hotspots_policy = fathom_analyze::hotspots::policy_from_config_text(&config).0;
    let (layers, _) = fathom_analyze::graph::layers::Layers::from_repo_config_text(&config);
    let (rules, rule_notices) =
        fathom_analyze::layering::Rules::from_repo_config_text(&layers, &config);
    let (_, baseline_path, _) = fathom_analyze::cycles::debt::Ratchet::config(&config);
    let ratchet = match std::fs::read_to_string(root.join(&baseline_path)) {
        Ok(text) => fathom_analyze::cycles::debt::Ratchet::from_text(&text, &baseline_path),
        Err(_) => fathom_analyze::cycles::debt::Ratchet::empty(),
    };
    let now = fathom_core::time::Timestamp::from_unix_micros(
        crate::clock::now_millis().saturating_mul(1_000),
    );
    if args.explain {
        let knobs = Knobs {
            policy: policy.clone(),
            rules,
            ratchet,
            hotspots: hotspots_policy,
            now,
            notices: rule_notices,
        };
        print_explain(&knobs);
        return Ok(ExitCode::Ok);
    }
    // Below here the answer needs the graph, so a missing index is the finding it has to report. Above
    // here it does not: `--explain` is the arithmetic and the knobs, and it answers in a repository that has
    // never been indexed, which is where the question "what would you score, and how?" actually gets asked.
    if !store_path.is_file() {
        return Err(Pending::new(
            Code::IndexDegraded,
            format!(
                "no index at {} yet, so there is no architecture scorecard to print",
                redactor.path(&store_path)
            ),
            Some(crate::state::REMEDY_INDEX.to_owned()),
        )
        .into());
    }

    let mut store = Store::open(&store_path).map_err(failed)?;
    if args.refresh && !args.read_only {
        // Class R means this is allowed to be this simple: drop the derived rows and re-derive them.
        store.scorecard_clear().map_err(failed)?;
    }
    let revisions = store.graph_revisions().map_err(failed)?;
    if revisions.is_empty() {
        return Err(Pending::new(
            Code::IndexDegraded,
            "this index holds no module graph, so there is nothing to score — `fathom index` writes graph \
             snapshots",
            Some(crate::state::REMEDY_INDEX.to_owned()),
        )
        .into());
    }
    let head = match resolve_revision(&revisions, args.revision.as_deref()) {
        Ok(tree) => tree,
        Err(why) => {
            return Err(Pending::usage(
                format!("`--revision` {why}"),
                "the newest 5 revisions are \
            listed below",
            )
            .into());
        }
    };
    let previous = revisions
        .iter()
        .position(|tree| tree == &head)
        .and_then(|at| revisions.get(at + 1))
        .cloned();

    // Everything the five dimensions need, assembled once.
    let knobs = Knobs {
        policy: policy.clone(),
        rules,
        ratchet,
        hotspots: hotspots_policy,
        now,
        notices: rule_notices,
    };

    // One lane for the whole run: it holds the store mutably, so every read that needs the graph happens
    // inside this block and every write happens after it.
    let (head_card, head_from_store, rows_to_write, mut notices) = {
        let builder = fathom_analyze::graph::builder_from_config_text(&config).0;
        let lane = fathom_analyze::graph::Lane::new(&mut store, builder).map_err(failed)?;
        let mut notices: Vec<String> = Vec::new();
        let stored = lane.read().scorecard_get(&head).map_err(failed)?;
        let mut head_card = None;
        let mut head_from_store = true;
        if let Some(row) = &stored {
            match Row::parse(&row.tree_oid, row.built_utc, &row.dims) {
                Parse::Row(parsed) => head_card = Some(parsed.scorecard),
                Parse::Foreign { model } => notices.push(format!(
                    "the stored scorecard for {} was written by model `{model}`, not `{MODEL}`, so it was \
                     re-scored here — re-run with `--refresh` to re-derive the whole history",
                    short(&head)
                )),
                Parse::Unreadable => notices.push(format!(
                    "the stored scorecard for {} would not parse and was re-scored here",
                    short(&head)
                )),
            }
        }
        let mut rows_to_write: Vec<fathom_store::scorecard::ScorecardRow> = Vec::new();
        if head_card.is_none() {
            head_from_store = false;
            let measured = score(&lane, &head, previous.as_deref(), &knobs).map_err(|partial| {
                Pending::new(
                    Code::IndexDegraded,
                    format!(
                        "the head revision's graph would not materialise: {}",
                        partial.sentence()
                    ),
                    Some(crate::state::REMEDY_INDEX.to_owned()),
                )
            })?;
            let mut card = measured.scorecard;
            card.episode = episode_of(lane.read(), &head);
            notices.extend(measured.notices);
            head_card = Some(card);
        }
        // The backfill: the newest revisions with no row yet. Each one is one chain walk, which is why the
        // flag defaults to none and why the report counts what it did.
        if args.backfill > 0 && !args.read_only {
            let scored: std::collections::BTreeSet<String> = lane
                .read()
                .scorecard_rows(0)
                .map_err(failed)?
                .into_iter()
                .map(|row| row.tree_oid)
                .collect();
            let due: Vec<String> = revisions
                .iter()
                .filter(|tree| !scored.contains(*tree))
                .take(args.backfill)
                .cloned()
                .collect();
            let (rows, stops, _) = fathom_analyze::scorecard::lane::backfill(&lane, &due, &knobs);
            notices.extend(stops);
            rows_to_write = rows;
        }
        // Structurally guaranteed by the block above — it scores a card whenever the store had none. Stated
        // as an error rather than an `unwrap` because the alternative is a panic in the verb a CI pipeline
        // runs, and because a head with no card and no explanation is exactly the lie this report exists to
        // avoid.
        let head_card = match head_card {
            Some(card) => card,
            None => {
                return Err(Pending::new(
                    Code::IndexDegraded,
                    format!("no scorecard could be derived for {}", short(&head)),
                    Some(crate::state::REMEDY_INDEX.to_owned()),
                )
                .into());
            }
        };
        (head_card, head_from_store, rows_to_write, notices)
    };
    let backfilled = rows_to_write.len();
    if !args.read_only {
        for row in rows_to_write {
            store.scorecard_write(&row).map_err(failed)?;
        }
        if !head_from_store {
            let row = fathom_store::scorecard::ScorecardRow {
                tree_oid: head_card.revision.clone(),
                dims: head_card.to_stored(),
                // Filled below, once the anchor is known, and written by the next run that scores this
                // revision: a `since` recorded before the trend was read would be a guess about its own
                // ordering.
                since: None,
                built_utc: head_card.built_utc,
            };
            store.scorecard_write(&row).map_err(failed)?;
        }
    }

    // The trend: what the index holds, newest first, folded through a bounded sample.
    let stored_rows = store.scorecard_rows(args.history).map_err(failed)?;
    let scored_total = store.scorecard_count().map_err(failed)?;
    let mut trend = Trend::new(&policy);
    let mut rows: Vec<Row> = Vec::with_capacity(stored_rows.len());
    for row in &stored_rows {
        match Row::parse(&row.tree_oid, row.built_utc, &row.dims) {
            Parse::Row(parsed) => {
                trend.feed(&parsed);
                rows.push(parsed);
            }
            Parse::Foreign { .. } => trend.refuse_foreign(),
            Parse::Unreadable => trend.count_unreadable(),
        }
    }
    if rows.iter().all(|row| row.revision != head) {
        // The head is in the report but not in the trend, which can only happen when it could not be or was
        // not written. Say it, rather than letting a chart silently start one revision late.
        notices.push(format!(
            "{} is scored in this run but carries no stored row, so the trend starts at {}",
            short(&head),
            rows.first()
                .map_or_else(|| "nothing".to_owned(), |row| short(&row.revision))
        ));
    }

    let review = last_review(&store)?;
    let anchor = resolve(review.as_ref(), &rows);
    let since = rows
        .iter()
        .find(|row| anchor.revision.as_deref() == Some(row.revision.as_str()))
        .map(|anchor_row| {
            let revisions = rows
                .iter()
                .take_while(|row| row.revision != anchor_row.revision)
                .count()
                + 1;
            Delta::between(
                &anchor_row.scorecard,
                &head_card,
                anchor.clone(),
                revisions,
                policy.flat_within,
            )
        });
    let previous_card = rows
        .iter()
        .find(|row| row.revision != head)
        .map(|row| row.scorecard.clone());

    let index = Index {
        revisions: revisions.len(),
        scored: usize::try_from(scored_total).unwrap_or(usize::MAX),
        read: trend.rows_read,
        newest: revisions.first().cloned().unwrap_or_default(),
        head: head.clone(),
        episode: head_card.episode.clone(),
        unscored: revisions
            .len()
            .saturating_sub(usize::try_from(scored_total).unwrap_or(usize::MAX)),
    };
    if backfilled > 0 {
        notices.push(format!(
            "{backfilled} revision(s) scored by this run's --backfill"
        ));
    }
    if head_from_store {
        notices.push(
            "the head's numbers are the stored row's, so the report and the trend agree; \
             --refresh re-derives them from the current rules"
                .to_owned(),
        );
    }
    // The durable companion. Written here rather than at insert time because the anchor is only known once
    // the trend has been read back, and read back here so the printed delta and the stored one are the same
    // computation from the same run. `--read-only` writes nothing, which means what it prints about a
    // *recorded* companion is whatever an earlier scoring run left behind — true of that run, and labelled
    // as such by the report's wording.
    if let Some(delta) = &since
        && !args.read_only
    {
        store
            .scorecard_set_since(&head, &delta.companion_json())
            .map_err(failed)?;
    }
    let since_recorded = store
        .scorecard_get(&head)
        .ok()
        .flatten()
        .and_then(|row| row.since)
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());

    let report = Report {
        head: head_card,
        previous: previous_card,
        since,
        since_recorded,
        trend,
        policy,
        index,
        notices,
    };

    if args.json {
        output::line(format_args!("{}", report.to_json()));
    } else {
        for line in report.lines() {
            output::line(format_args!("{line}"));
        }
    }
    Ok(ExitCode::Ok)
}

/// The reviewer's last act, as the scorecard's anchor ladder needs it.
///
/// Newest *live* episode with a review marker that is not `unread`, by the instant its own work ended. The
/// store has already re-attached the marker across an amend, a reword or a squash when it could
/// (`relink_review_state`, migration 3), which is what makes this a content anchor and not a position — the
/// acceptance criterion's "correct across a rebase" rides on that, and on the rung ladder in
/// [`fathom_analyze::scorecard::review`] for the cases it cannot.
#[cfg(feature = "sqlite-bundled")]
fn last_review(store: &Store) -> eyre::Result<Option<fathom_analyze::scorecard::review::Review>> {
    use crate::verbs::failed;
    use fathom_analyze::scorecard::review::Review;

    let states = store.review_states().map_err(failed)?;
    if states.is_empty() {
        return Ok(None);
    }
    let episodes = store
        .episodes(&EpisodeQuery {
            include_dead: true,
            ..EpisodeQuery::default()
        })
        .map_err(failed)?;
    let best = episodes
        .iter()
        .filter(|episode| {
            states
                .get(&episode.id)
                .is_some_and(|state| *state != ReviewState::Unread)
        })
        .max_by(|left, right| {
            left.last_utc
                .cmp(&right.last_utc)
                .then_with(|| left.id.cmp(&right.id))
        });
    let Some(episode) = best else {
        return Ok(None);
    };
    let state = states
        .get(&episode.id)
        .copied()
        .unwrap_or(ReviewState::Unread);
    // The actor is read from the marker itself, not inferred: an author, a committer or a guess would be
    // ADR-001 §7.7's forbidden invention, and "who reviewed this?" is the question being answered.
    let marker = store
        .review_marker(&episode.id)
        .map_err(failed)?
        .unwrap_or_else(|| fathom_store::episode::ReviewMarker {
            state,
            actor: String::new(),
            note: String::new(),
            updated_utc: episode.last_utc,
            fingerprint: episode.fingerprint.clone(),
        });
    let actor = if marker.actor.is_empty() {
        // A marker written without a name is still a marker; the triage ledger may know who pressed the key.
        store
            .last_triage(&episode.id)
            .ok()
            .flatten()
            .map(|event| event.actor)
            .unwrap_or_default()
    } else {
        marker.actor.clone()
    };
    // The revision the reviewed work ended at, if this index still holds that commit. After a rewrite that
    // dropped it this is `None`, which is what rung 2 of the ladder is for.
    let revision = match store.read_commit(&episode.last_oid) {
        Ok(commit) => Some(commit.tree_oid),
        Err(_) => None,
    };
    Ok(Some(Review {
        episode: episode.id.clone(),
        fingerprint: episode.fingerprint.clone(),
        state: state.as_str().to_owned(),
        actor,
        revision,
        instant: episode.last_utc.saturating_mul(1_000),
        relinked_via: String::new(),
    }))
}

/// The episode a graph revision belongs to, if the index can name one.
#[cfg(feature = "sqlite-bundled")]
fn episode_of(store: &Store, tree: &str) -> Option<String> {
    let oids = store.commits_with_tree(tree).ok()?;
    for oid in oids {
        if let Ok(Some(episode)) = store.episode_for_commit(&oid) {
            return Some(episode.id);
        }
    }
    None
}

/// Which revision to score: the newest one, or the caller's, matched against the graph revisions as an
/// exact oid or an unambiguous prefix.
#[cfg(feature = "sqlite-bundled")]
fn resolve_revision(revisions: &[String], wanted: Option<&str>) -> Result<String, String> {
    let Some(wanted) = wanted else {
        return Ok(revisions[0].clone());
    };
    let wanted = wanted.trim().to_ascii_lowercase();
    if revisions.iter().any(|tree| tree == &wanted) {
        return Ok(wanted);
    }
    let matches: Vec<&String> = revisions
        .iter()
        .filter(|tree| tree.starts_with(wanted.as_str()))
        .collect();
    match matches.len() {
        1 => Ok(matches[0].clone()),
        0 => Err(format!(
            "{wanted} is not a revision this index has a graph for"
        )),
        n => Err(format!(
            "{wanted} matches {n} revisions — give a longer prefix"
        )),
    }
}

/// Print the knobs and the five questions, and nothing else: `--explain` is the answer to "why is it 61"
/// before anyone has a repository to ask about.
#[cfg(feature = "sqlite-bundled")]
fn print_explain(knobs: &fathom_analyze::scorecard::lane::Knobs) {
    use crate::output;
    use fathom_analyze::scorecard::dimension::Dimension;

    // The five questions, their curves and the total's arithmetic — generated from the constants the
    // arithmetic itself uses, so this cannot drift from the code that produced the number being asked about.
    for paragraph in Dimension::explain_all(&knobs.policy) {
        output::line(format_args!("{paragraph}"));
    }
    for line in knobs.policy.lines() {
        output::line(format_args!("{line}"));
    }
    output::line(format_args!(
        "  boundaries rules in force: {} · declared layers: {} · ratchet: {}",
        knobs.rules.active().len(),
        knobs.rules.layers_declared(),
        knobs
            .ratchet
            .source
            .as_deref()
            .unwrap_or("no .fathom/ratchet.toml")
    ));
    for notice in knobs.notices.iter().chain(knobs.policy.notices.iter()) {
        output::line(format_args!("  notice: {notice}"));
    }
}

/// A revision shortened the way every other pane shortens one.
#[cfg(feature = "sqlite-bundled")]
fn short(revision: &str) -> String {
    if revision.chars().count() <= 10 {
        return revision.to_owned();
    }
    let cut: String = revision.chars().take(10).collect();
    format!("{cut}…")
}
