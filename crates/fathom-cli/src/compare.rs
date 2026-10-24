//! `fathom cmp` — the composition root's half of compare mode (`e5.3`, `reco-ifo`).
//!
//! [`fathom_analyze::compare`] owns everything a comparison *means*: the spec grammar, the labelled
//! endpoints, and a delta that is the composition of the per-episode delta documents `fathom analyze`
//! prints. What it deliberately reads nothing of is a repository, because a module that composes two
//! documents has no business opening a store. This file is that missing piece and nothing more: resolve
//! two endpoints, walk the episodes between them, hand each one to the insight façade, and call
//! [`Comparison::compose`]. There is no second delta engine on this path either — the same
//! [`insight::for_rows`] call the timeline, the export and the search corpus use builds every row.
//!
//! # The two shapes a span can take, and why they are different
//!
//! * **One lineage.** `fathom cmp HEAD~9..HEAD` is a walk: both endpoints sit on one ref, each episode
//!   carries its ordinal on that ref ([`EpisodeRow::lineage_seq`]), and the span is the episodes strictly
//!   after the left endpoint up to and including the right one — git's `a..b`, exclusive of `a`, because
//!   the question being asked is "what happened *since* the point I last looked".
//! * **Two lineages.** `fathom cmp main@1a2b..feat/x@9f8e` is not a walk: there is no ordinal that runs
//!   from one branch to the other. The span is then the window of *time* between the endpoints, which is
//!   what a reviewer away for a week actually means — everything that landed anywhere in between — and it
//!   is the case [`Comparison::notes`] names every contributing lineage for. The endpoints carry the branch
//!   the human typed, so a pasted `fathom cmp` line cannot be read as one branch's history by the next
//!   person to read it.
//!
//! # What a run costs, and where the bound is stated
//!
//! Two index reads (the episode rows, and one batched file rollup) plus one delta document per episode in
//! the span. The detectors are run for the first [`FINDINGS_MAX`] episodes of the range and no further:
//! like `fathom export`, the history-shaped detectors re-read their corpus per episode, so an uncapped
//! range is quadratic. Past the cap — and past [`RANGE_MAX`] episodes overall — the comparison says so in
//! its own gap list rather than going quiet, which is the difference between a partial answer and a wrong
//! one.
//!
//! [`EpisodeRow::lineage_seq`]: fathom_store::episode::EpisodeRow::lineage_seq

use std::path::Path;

use fathom_analyze::compare::{Comparison, Contribution, Endpoint, Spec};
use fathom_analyze::insight::{self, EpisodeDelta, Evidence, Finding, Gap};
use fathom_core::exit::ExitCode;
// Every read below is a store read, and a `minimal` build has no store to read: the imports are gated with
// the feature rather than the functions alone, so the module compiles to its own refusal (ADR-002 §6.2 —
// a compiled-out verb says "compiled out", it does not fail to build).
#[cfg(feature = "sqlite-bundled")]
use fathom_store::episode::{EpisodeQuery, EpisodeRow, ReviewState};
#[cfg(feature = "sqlite-bundled")]
use fathom_store::spine::Store;

use crate::output;
use crate::verbs::{CmpArgs, Pending};

/// The most episodes one comparison composes.
///
/// A comparison is read by a person, and nobody reads two hundred episodes' worth of merged sections; the
/// number exists so that a `fathom cmp v0.1..HEAD` over a year of history returns in seconds and prints
/// which end of its own range it left out.
const RANGE_MAX: usize = 200;

/// The most episodes one run runs the detectors over — `fathom export`'s cap, for the same reason.
const FINDINGS_MAX: usize = 64;

/// `fathom cmp …`.
///
/// # Errors
///
/// Exit `2` for a spec that is not `a..b` and for a name that was never saved, exit `1` for an endpoint
/// that names nothing in this repository, exit `12` for a repository with no index (ADR-002 §7.3).
pub fn run(args: &CmpArgs) -> eyre::Result<ExitCode> {
    #[cfg(not(feature = "sqlite-bundled"))]
    {
        let _ = args;
        Err(crate::verbs::feature_absent("fathom cmp", "sqlite-bundled"))
    }

    #[cfg(feature = "sqlite-bundled")]
    {
        let root = std::path::Path::new(&args.root);
        if args.list {
            return list(root, &store_path(root), args.json);
        }
        let Some(asked) = args.spec.as_deref() else {
            return Err(Pending::new(
                ExitCode::Usage,
                "a comparison needs two endpoints",
                Some(
                    "the shape is `fathom cmp <from>..<to>` — `HEAD~9..HEAD`, `77aa0c..e8f1a2`, \
                     `main@1a2b3c..feat/x@9f8e7d`, or a name saved with `fathom cmp --save`; \
                     `fathom cmp --list` prints the names this repository has"
                        .to_owned(),
                ),
            )
            .into());
        };
        run_spec(root, asked, args)
    }
}

/// Where this repository's index — and so its session rows — live.
///
/// The same path every verb reads (`fathom_analyze::index::default_store_path`, which is what
/// `fathom-cli`'s own `session::store_path` delegates to); named once here so the compare verb does not
/// acquire a dependency on the shell's module in a build that has no shell.
#[cfg(feature = "sqlite-bundled")]
fn store_path(root: &Path) -> std::path::PathBuf {
    fathom_analyze::index::default_store_path(root)
}

/// One comparison, from a spec or a saved name to a printed answer.
#[cfg(feature = "sqlite-bundled")]
fn run_spec(root: &Path, asked: &str, args: &CmpArgs) -> eyre::Result<ExitCode> {
    let mut opened = open(root, &store_path(root))?;
    let store = &opened.store;
    let repo = fathom_ingest::repo::Repository::discover(root).map_err(crate::verbs::failed)?;

    // A saved name is tried first, because the whole point of naming a comparison is to stop retyping it,
    // and a name that has no `..` in it cannot collide with a spec.
    let saved = find_saved(store, asked)?;
    let spec_text = saved.as_ref().map_or(asked, |saved| saved.spec.as_str());
    let spec = Spec::parse(spec_text).map_err(|why| {
        Pending::new(
            ExitCode::Usage,
            why,
            Some("`fathom cmp --list` prints the saved names; a spec is `<from>..<to>`".to_owned()),
        )
    })?;

    let from = resolve(store, &repo, &spec.from)?;
    let to = resolve(store, &repo, &spec.to)?;
    let span = between(store, &from, &to)?;
    let policy = fathom_analyze::detectors::policy_from_toml(
        &std::fs::read_to_string(root.join("fathom.toml")).unwrap_or_default(),
    )
    .policy;
    let composed = compose(store, &repo, from.endpoint, to.endpoint, &span, &policy)?;

    if let Some(name) = args.save.as_deref() {
        save(&mut opened.store, name, &spec, args.note.as_deref())?;
    }
    if args.json {
        // One line, and the only thing on stdout (`e1.10`'s contract, the same one `fathom analyze
        // --json` keeps): a script pipes this into `jq` and a second line breaks the pipe.
        //
        // The spec the reviewer *asked for* is put on the front of the payload, beside the endpoints it
        // resolved to: a script that asked for `since-the-refactor` needs to see that name in the answer it
        // got, and the two endpoint objects below already carry the specs they were resolved from.
        let mut payload = serde_json::to_value(&composed)
            .map_err(|err| eyre::eyre!("the comparison could not be serialised: {err}"))?;
        if let Some(map) = payload.as_object_mut() {
            let mut head = serde_json::Map::new();
            head.insert(
                "contract".to_owned(),
                serde_json::json!(fathom_analyze::compare::CONTRACT),
            );
            // `spec` is what the endpoints were written as — the resolved text when the reviewer typed a
            // saved name — and `asked` is what they actually typed, so `jq` can join on either.
            head.insert("spec".to_owned(), serde_json::json!(spec_text));
            head.insert("asked".to_owned(), serde_json::json!(asked));
            if let Some(saved) = saved.as_ref() {
                head.insert("saved_as".to_owned(), serde_json::json!(saved.name));
            }
            for (key, value) in head {
                map.insert(key, value);
            }
        }
        let text = serde_json::to_string(&payload)
            .map_err(|err| eyre::eyre!("the comparison could not be serialised: {err}"))?;
        output::line(format_args!("{text}"));
    } else {
        if let Some(saved) = &saved {
            output::line(format_args!(
                "saved comparison `{}` — {}",
                saved.name, saved.spec
            ));
        }
        for line in composed.render() {
            output::line(format_args!("{line}"));
        }
        output::line(format_args!(
            "\nfathom cmp {} --json  ·  the contract is {}",
            spec.display(),
            fathom_analyze::compare::CONTRACT
        ));
        if let Some(name) = args.save.as_deref() {
            output::line(format_args!(
                "saved as `{name}` — `fathom cmp {name}` in this repository, and in the `,` palette"
            ));
        }
    }
    Ok(opened.snapshot.worst_code())
}

/// One comparison for the MCP server (`e6.4`), by the two endpoints an agent named.
///
/// The same walk `fathom cmp` does — resolve, span, compose — with the printing left out and the errors
/// returned as one sentence instead of as a process exit, because an MCP tool result is a value
/// (ADR-005 §4.1: "nothing throws at the caller"). One `Source` method uses this for `changes_between`;
/// there is no second range walk anywhere, which is the point of putting it here rather than writing it
/// again next door.
///
/// # Errors
///
/// One sentence per failure: what did not resolve, and the verb that would fix it.
// `sqlite-bundled` because it reads the index, `mcp` because the MCP door is its only caller: a headless
// build without the protocol crate has nothing to call it, and says so by not containing it.
#[cfg(all(feature = "sqlite-bundled", feature = "mcp"))]
pub(crate) fn mcp_compare(
    root: &Path,
    from: &str,
    to: &str,
) -> Result<fathom_analyze::compare::Comparison, String> {
    let opened = open(root, &store_path(root)).map_err(|err| err.to_string())?;
    let store = &opened.store;
    let repo = fathom_ingest::repo::Repository::discover(root).map_err(|err| err.to_string())?;
    let from = resolve(store, &repo, from).map_err(|err| err.to_string())?;
    let to = resolve(store, &repo, to).map_err(|err| err.to_string())?;
    let span = between(store, &from, &to).map_err(|err| err.to_string())?;
    let policy = fathom_analyze::detectors::policy_from_toml(
        &std::fs::read_to_string(root.join("fathom.toml")).unwrap_or_default(),
    )
    .policy;
    compose(store, &repo, from.endpoint, to.endpoint, &span, &policy).map_err(|err| err.to_string())
}

/// `fathom cmp --list`: the saved comparisons this repository has, in the order the palette prints them.
#[cfg(feature = "sqlite-bundled")]
fn list(root: &Path, store_path: &Path, as_json: bool) -> eyre::Result<ExitCode> {
    let opened = open(root, store_path)?;
    let entries = opened
        .store
        .ui_state_rows(fathom_analyze::compare::SAVED_PREFIX)
        .map_err(crate::verbs::failed)?;
    let saved = fathom_analyze::compare::saved_from_session(
        &entries
            .iter()
            .map(|(key, value, _)| (key.clone(), value.clone()))
            .collect::<Vec<(String, String)>>(),
    );
    if as_json {
        let payload = serde_json::json!({
            "contract": fathom_analyze::compare::CONTRACT,
            "action": "list",
            "saved": saved
                .iter()
                .map(|item| serde_json::json!({
                    "name": item.name, "spec": item.spec, "note": item.note,
                }))
                .collect::<Vec<_>>(),
        });
        output::line(format_args!("{payload}"));
        return Ok(opened.snapshot.worst_code());
    }
    if saved.is_empty() {
        output::line(format_args!(
            "no saved comparisons yet — `fathom cmp --save since-the-refactor HEAD~9..HEAD` makes one"
        ));
        return Ok(opened.snapshot.worst_code());
    }
    for item in &saved {
        let note = if item.note.is_empty() {
            String::new()
        } else {
            format!("  — {}", item.note)
        };
        output::line(format_args!("{:<24} {}{}", item.name, item.spec, note));
    }
    output::line(format_args!(
        "\n{} saved; `fathom cmp <name>` compares one, and `,` lists them in the shell",
        saved.len()
    ));
    Ok(opened.snapshot.worst_code())
}

/// The store, or the reason there is not one (the same sentence `fathom analyze` prints, so that one
/// broken repository produces one sentence and not two that nearly agree).
#[cfg(feature = "sqlite-bundled")]
struct Opened {
    store: Store,
    snapshot: crate::state::Snapshot,
}

#[cfg(feature = "sqlite-bundled")]
fn open(root: &Path, store_path: &Path) -> eyre::Result<Opened> {
    let snapshot = crate::state::gather(root, false);
    if !store_path.exists() {
        let redactor = crate::sanitize::Redactor::for_root(root);
        return Err(Pending::new(
            ExitCode::IndexDegraded,
            format!(
                "no index at {} yet, so there is nothing to compare",
                redactor.path(store_path)
            ),
            Some(crate::state::REMEDY_INDEX.to_owned()),
        )
        .into());
    }
    Ok(Opened {
        store: Store::open(store_path).map_err(crate::verbs::failed)?,
        snapshot,
    })
}

// ---------------------------------------------------------------------------
// endpoints
// ---------------------------------------------------------------------------

/// An endpoint, and the row it resolved to — the row is needed to match the span against the index, while
/// the [`Endpoint`] is what the reader is shown (and may name a branch the index never called that).
#[cfg(feature = "sqlite-bundled")]
struct Resolved {
    endpoint: Endpoint,
    row: EpisodeRow,
}

/// Resolve one endpoint token to an episode.
///
/// The grammar is every other single-episode verb's — an episode id, `HEAD`, `main~3`, an oid prefix — plus
/// `branch@rev`, which is the spelling that makes a cross-branch comparison readable in one line. The
/// branch half is a *label*: the rev is what gets resolved, and if the index says the episode it points at
/// belongs to a different lineage, the label still wins on the printed endpoint, because it is what the
/// human typed and typed things are what a shared spec has to agree about.
#[cfg(feature = "sqlite-bundled")]
fn resolve(
    store: &Store,
    repo: &fathom_ingest::repo::Repository,
    token: &str,
) -> eyre::Result<Resolved> {
    use fathom_ingest::revspec;

    let (branch, handle) = match token.split_once('@') {
        Some((branch, handle)) if !branch.is_empty() && !handle.is_empty() => {
            (Some(branch.to_owned()), handle)
        }
        _ => (None, token),
    };
    let row = match store.episode(handle).map_err(crate::verbs::failed)? {
        Some(row) => row,
        None => {
            let oid = revspec::resolve(repo, handle).map_err(|err| {
                Pending::new(
                    ExitCode::Error,
                    format!("`{token}` names no episode and no commit in this repository: {err}"),
                    Some(
                        "the grammar is an episode id, `HEAD`, `main~3`, an oid prefix, or \
                         `branch@rev`; `fathom show HEAD` prints the newest episode"
                            .to_owned(),
                    ),
                )
            })?;
            store
                .episode_for_commit(&oid.hex())
                .map_err(crate::verbs::failed)?
                .ok_or_else(|| {
                    Pending::new(
                        ExitCode::IndexDegraded,
                        format!(
                            "`{token}` is a commit in this repository with no episode row in the index"
                        ),
                        Some(crate::state::REMEDY_INDEX.to_owned()),
                    )
                })?
        }
    };
    let endpoint = Endpoint {
        spec: token.to_owned(),
        episode: row.id.clone(),
        summary: row.summary.clone(),
        lineage: branch.unwrap_or_else(|| short_lineage(&row.lineage_ref)),
        lineage_seq: row.lineage_seq,
        utc: row.last_utc,
    };
    Ok(Resolved { endpoint, row })
}

/// `refs/heads/main` → `main`, so an endpoint label is `main@1a2b3c4d` and not a ref name that eats the
/// header line. A ref that is not under `refs/heads/` keeps its own spelling: `HEAD` is not a branch and
/// pretending it is would be a guess about the reviewer's repository.
#[must_use]
pub fn short_lineage(reference: &str) -> String {
    reference
        .strip_prefix("refs/heads/")
        .unwrap_or(reference)
        .to_owned()
}

// ---------------------------------------------------------------------------
// the span
// ---------------------------------------------------------------------------

/// The episodes between two endpoints, oldest first.
///
/// One lineage gives a walk on its ordinals; two give the window between their dates. Both branches return
/// rows in the order `Comparison::compose` is promised, and neither invents an order of its own — see the
/// module's two-shapes note.
#[cfg(feature = "sqlite-bundled")]
fn between(store: &Store, from: &Resolved, to: &Resolved) -> eyre::Result<Vec<EpisodeRow>> {
    let rows = store
        .episodes(&EpisodeQuery::default())
        .map_err(crate::verbs::failed)?;
    let same = from.row.lineage_ref == to.row.lineage_ref;
    let mut span: Vec<EpisodeRow> = if same {
        let (low, high) = if from.row.lineage_seq <= to.row.lineage_seq {
            (from.row.lineage_seq, to.row.lineage_seq)
        } else {
            (to.row.lineage_seq, from.row.lineage_seq)
        };
        rows.into_iter()
            .filter(|row| row.lineage_ref == from.row.lineage_ref)
            .filter(|row| row.lineage_seq > low && row.lineage_seq <= high)
            .collect()
    } else {
        // The window, not the walk: everything that happened between the two instants, on whichever
        // lineage it happened. `<=` on the far end so the right endpoint's own episode is in the answer.
        let (low, high) = (from.row.last_utc, to.row.last_utc);
        let (low, high) = if low <= high {
            (low, high)
        } else {
            (high, low)
        };
        rows.into_iter()
            .filter(|row| row.last_utc >= low && row.last_utc <= high && row.id != from.row.id)
            .collect()
    };
    span.sort_by(|left, right| {
        (left.last_utc, &left.lineage_ref, left.lineage_seq).cmp(&(
            right.last_utc,
            &right.lineage_ref,
            right.lineage_seq,
        ))
    });
    Ok(span)
}

/// Build the comparison: one delta document per episode in the span, plus the findings the range earned.
///
/// The delta documents come from the batched reads — every episode's file rollup in one query, the same
/// [`insight::for_rows`] call the search corpus and the rail make — so the cost of a 40-episode range is
/// two store reads and not a hundred and twenty.
#[cfg(feature = "sqlite-bundled")]
fn compose(
    store: &Store,
    repo: &fathom_ingest::repo::Repository,
    from: Endpoint,
    to: Endpoint,
    span: &[EpisodeRow],
    policy: &fathom_analyze::detectors::DetectorPolicy,
) -> eyre::Result<Comparison> {
    let truncated = span.len() > RANGE_MAX;
    let walked = if truncated {
        &span[span.len().saturating_sub(RANGE_MAX)..]
    } else {
        span
    };
    let ids: Vec<String> = walked.iter().map(|row| row.id.clone()).collect();
    let files = store.episode_files(&ids).map_err(crate::verbs::failed)?;
    let states = store.review_states().map_err(crate::verbs::failed)?;
    let mut items: Vec<Contribution> = Vec::with_capacity(walked.len());
    for (index, row) in walked.iter().enumerate() {
        let stats: &[fathom_store::episode::EpisodeFile] =
            files.by_episode.get(&row.id).map_or(&[], Vec::as_slice);
        let delta: EpisodeDelta = insight::for_rows(
            row,
            stats,
            states.get(&row.id).copied().unwrap_or(ReviewState::Unread),
            files.truncated,
        );
        // The detectors are the one part of a range that is quadratic — the history-shaped ones re-read
        // their corpus per episode — so they run for the near end of the range and the gap says which end
        // that was. A comparison that said "no findings" after skipping half the range would be the worse
        // kind of partial: the readable kind.
        let findings = if index < FINDINGS_MAX {
            findings_for(store, repo, &row.id, policy)?
        } else {
            Vec::new()
        };
        items.push(Contribution { delta, findings });
    }
    let mut comparison = Comparison::compose(from, to, &items);
    if truncated {
        comparison.delta.gaps.push(Gap {
            section: "compare".to_owned(),
            short: format!(
                "the range holds {} episodes; the first {} were left out of this composition",
                span.len(),
                span.len() - RANGE_MAX
            ),
            reason: format!("`fathom cmp` composes at most {RANGE_MAX} episodes in one run"),
            owner: "e5.3".to_owned(),
            remedy: Some("narrow it: fathom cmp <a>..<b> with a nearer left endpoint".to_owned()),
        });
    }
    if span.len() > FINDINGS_MAX {
        comparison.delta.gaps.push(Gap {
            section: "attention".to_owned(),
            short: format!(
                "findings were evaluated for the first {FINDINGS_MAX} episodes of {} only",
                span.len()
            ),
            reason: "the history-shaped detectors re-read their corpus per episode".to_owned(),
            owner: "e4.3".to_owned(),
            remedy: Some("narrow the range, or run `fathom why <episode>` per episode".to_owned()),
        });
    }
    Ok(comparison)
}

/// The findings one episode of the range raised, in the delta model's shape.
///
/// The detectors' own richer fields (confidence, evidence locators, the jump target) stay with `fathom why`
/// and the export, which carry them; a comparison dedupes findings by kind and subject, so that is the
/// half worth carrying across a range of forty episodes.
#[cfg(feature = "sqlite-bundled")]
fn findings_for(
    store: &Store,
    repo: &fathom_ingest::repo::Repository,
    id: &str,
    policy: &fathom_analyze::detectors::DetectorPolicy,
) -> eyre::Result<Vec<Finding>> {
    use fathom_analyze::risk::collect::Supply;

    let Some(read) =
        fathom_analyze::risk::collect::read(store, repo, id, policy, &Supply::nothing())
            .map_err(crate::verbs::failed)?
    else {
        return Ok(Vec::new());
    };
    Ok(read
        .report
        .findings
        .iter()
        .map(|finding| Finding {
            kind: finding.detector_id.clone(),
            subject: finding.subject.clone(),
            message: finding.message.clone(),
            severity: finding.severity.as_str().to_owned(),
            evidence: Evidence::owner("e4.3"),
            owner: "e4.3".to_owned(),
        })
        .collect())
}

// ---------------------------------------------------------------------------
// saved comparisons
// ---------------------------------------------------------------------------

/// The saved comparison named `name`, if this repository has one.
#[cfg(feature = "sqlite-bundled")]
fn find_saved(store: &Store, name: &str) -> eyre::Result<Option<fathom_analyze::compare::Saved>> {
    // A spec, not a name: `..` is what tells them apart, and a name may not contain it.
    if name.contains("..") {
        return Ok(None);
    }
    let rows = store
        .ui_state_rows(&fathom_analyze::compare::saved_key(name))
        .map_err(crate::verbs::failed)?;
    let entries: Vec<(String, String)> = rows
        .iter()
        .map(|(key, value, _)| (key.clone(), value.clone()))
        .collect();
    // No match is not an error: `fathom cmp foo` with no such name falls through to the spec parser,
    // which is the sentence that can explain the grammar. `--list` is where the names live.
    Ok(fathom_analyze::compare::saved_from_session(&entries)
        .into_iter()
        .find(|item| item.name == name))
}

/// Write one saved comparison.
///
/// A name may be re-saved — that is the feature, a name a team can move as the project moves — and the row
/// is written through the same `session_state` table the shell reads, which is the only reason it is
/// standing in the `,` palette next time one is opened (ADR-004 §4.7).
#[cfg(feature = "sqlite-bundled")]
fn save(store: &mut Store, name: &str, spec: &Spec, note: Option<&str>) -> eyre::Result<()> {
    let cleaned = name.trim().replace(' ', "-");
    if cleaned.is_empty() {
        return Err(Pending::new(
            ExitCode::Usage,
            "a saved comparison needs a name",
            Some("`fathom cmp --save since-the-refactor HEAD~9..HEAD`".to_owned()),
        )
        .into());
    }
    let saved = fathom_analyze::compare::Saved {
        name: cleaned,
        spec: spec.display(),
        note: note.unwrap_or_default().trim().to_owned(),
        created_utc: None,
    };
    let (key, value) = fathom_analyze::compare::saved_entry(&saved);
    store
        .set_ui_state(&key, Some(&value), crate::clock::now_millis())
        .map_err(crate::verbs::failed)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// the shell's half (`reco-mdb`, and the shell's `c`)
// ---------------------------------------------------------------------------

/// The comparison the shell shows, as text: `c` between the mark and the row under the cursor, and a saved
/// comparison chosen from the palette.
///
/// The shell may not open a store (`ADR-002 §5.2`), so it asks for a spec and gets back the lines to draw —
/// the same `Comparison::render` the CLI prints, which is what keeps the two surfaces from disagreeing about
/// what a comparison says.
#[cfg(feature = "tui")]
pub fn render_spec(root: &Path, spec: &str) -> Result<Vec<String>, String> {
    let opened = open(root, &store_path(root)).map_err(|err| format!("{err:#}"))?;
    let store = &opened.store;
    let repo = fathom_ingest::repo::Repository::discover(root).map_err(|err| format!("{err}"))?;
    let saved = find_saved(store, spec).map_err(|err| format!("{err:#}"))?;
    let text = saved.map_or_else(|| spec.to_owned(), |saved| saved.spec);
    let spec = Spec::parse(&text).map_err(|why| why.to_string())?;
    let from = resolve(store, &repo, &spec.from).map_err(|err| format!("{err:#}"))?;
    let to = resolve(store, &repo, &spec.to).map_err(|err| format!("{err:#}"))?;
    let span = between(store, &from, &to).map_err(|err| format!("{err:#}"))?;
    let policy = fathom_analyze::detectors::policy_from_toml(
        &std::fs::read_to_string(root.join("fathom.toml")).unwrap_or_default(),
    )
    .policy;
    let comparison = compose(store, &repo, from.endpoint, to.endpoint, &span, &policy)
        .map_err(|err| format!("{err:#}"))?;
    let mut out = comparison.render();
    out.push(String::new());
    out.push(format!(
        "the same answer, for a script: fathom cmp {} --json",
        spec.display()
    ));
    Ok(out)
}

/// The provider the shell asks when `c` is pressed, or when a saved comparison is chosen from the palette.
///
/// Gated with the shell because nothing else in the binary asks for a comparison in text form — a `minimal`
/// build has no `c`, and `fathom cmp` is its own verb.
#[cfg(feature = "tui")]
pub fn provider(root: &Path) -> Option<fathom_tui::compare::Provider> {
    Some(fathom_tui::compare::Provider::new(StoreCompare {
        root: root.to_path_buf(),
    }))
}

/// The provider itself: a path, and the store read behind every question.
#[cfg(feature = "tui")]
struct StoreCompare {
    root: std::path::PathBuf,
}

#[cfg(feature = "tui")]
impl fathom_tui::compare::CompareProvider for StoreCompare {
    fn compare(&self, spec: &str) -> Result<Vec<String>, fathom_tui::diff::Unavailable> {
        render_spec(&self.root, spec).map_err(fathom_tui::diff::Unavailable::new)
    }
}
