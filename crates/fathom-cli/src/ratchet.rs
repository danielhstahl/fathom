//! `fathom ratchet` — the baseline file, and the act of turning it (`e3.10`, `reco-5ht`).
//!
//! The rule this verb exists to enforce is one sentence: **the file may only tighten on its own, and it may
//! only widen when somebody signs for it.** Everything here is that sentence plus the plumbing it needs — one
//! lane read to know what the repository owes, one pure [`fathom_analyze::ratchet::Plan`] to decide what the
//! file becomes, and one write that leaves a diff a human has to look at.
//!
//! ```text
//!   fathom.toml ─┬─ [layers] / [layering] ─► Rules      what the architecture claims
//!                └─ [ratchet]             ─► Mode, path what a new violation does
//!   .fathom/ratchet.toml ─────────────────► Ratchet     what has been forgiven
//!   index graph ──────────────────────────► Inventory   what is owed right now
//!                                             │
//!                                        Plan::build ──► file, diff, exit code
//! ```
//!
//! # Why the write lives here and the policy does not
//!
//! [`fathom_analyze::ratchet`] decides fates: which rows leave, which rows enter, which are about to lapse.
//! This module holds the three things that crate is not allowed to touch — a filesystem, a wall clock, a
//! store handle — and the one decision that belongs to the composition root: **whether to write at all**. A
//! bare run writes only what the ratchet may do unprompted; `--dry-run` reports and touches nothing;
//! `--update-baseline` is the only path that makes the file looser, and it cannot be reached without a
//! reason.
//!
//! # The exit code
//!
//! `ratchet.mode` decides it, exactly as the two debt ledgers promise: `off` and `warn` exit 0 whatever the
//! ledger says, `deny` exits 7 when a violation has no live allowance. That is the acceptance criterion CI
//! reads — existing violations baselined ⇒ green, one new violation ⇒ red — and it is why the verb is useful
//! before `fathom check` (**reco-ngk**) grows a policy table that reads the same ledger.

use std::path::Path;

use fathom_core::exit::ExitCode;
use fathom_core::time::{MICROS_PER_DAY, Timestamp};

use crate::verbs::RatchetArgs;

/// The ratchet, in a build with no index to read the architecture out of.
#[cfg(not(feature = "sqlite-bundled"))]
pub fn run(args: &RatchetArgs) -> eyre::Result<ExitCode> {
    let _ = args;
    Err(crate::verbs::feature_absent(
        "fathom ratchet",
        "sqlite-bundled",
    ))
}

/// The ratchet, in a build that can read the module graph.
#[cfg(feature = "sqlite-bundled")]
pub fn run(args: &RatchetArgs) -> eyre::Result<ExitCode> {
    use fathom_analyze::cycles::track::Options;
    use fathom_analyze::graph::builder_from_config_text;
    use fathom_analyze::graph::layers::Layers;
    use fathom_analyze::layering::Rules;
    use fathom_analyze::ratchet::{Plan, Ratchet, inventory};
    use fathom_store::spine::Store;

    let root = Path::new(&args.root);
    fathom_ingest::repo::Repository::discover(root).map_err(crate::verbs::failed)?;
    let config = std::fs::read_to_string(root.join("fathom.toml")).unwrap_or_default();
    // The ledger and the two `[ratchet]` keys, read the way every other reader reads them: a missing file is
    // a state, not an error, and a typo'd mode is a notice rather than a silent change of policy.
    let (ratchet, mut notices) = Ratchet::read(root, &config);
    let now = Timestamp::from_unix_micros(crate::clock::now_millis().saturating_mul(1_000));
    if args.reason.is_some() && !args.update_baseline {
        return Err(crate::verbs::Pending::usage(
            "`--reason` is the reason for raising the baseline, and no baseline is being raised",
            "add --update-baseline, or drop --reason",
        )
        .into());
    }
    let grant = grant(args, root, now)?;

    // What the repository owes. Everything from here needs the graph, so a missing index is the finding this
    // verb reports rather than a plan built out of an empty inventory.
    let (layers, _) = Layers::from_repo_config_text(&config);
    let (rules, rule_notices) = Rules::from_repo_config_text(&layers, &config);
    notices.extend(rule_notices);
    let store_path = fathom_analyze::index::default_store_path(root);
    if !store_path.is_file() {
        return Err(crate::verbs::Pending::new(
            ExitCode::IndexDegraded,
            format!(
                "no index at {} yet, so the ratchet cannot see what this repository owes",
                crate::sanitize::Redactor::for_root(root).path(&store_path)
            ),
            Some(crate::state::REMEDY_INDEX.to_owned()),
        )
        .into());
    }
    let mut store = Store::open(&store_path).map_err(crate::verbs::failed)?;
    let revisions = store.graph_revisions().map_err(crate::verbs::failed)?;
    if revisions.is_empty() {
        return Err(crate::verbs::Pending::new(
            ExitCode::IndexDegraded,
            "this index holds no module graph, so there is nothing to baseline — `fathom index` writes \
             graph snapshots",
            Some(crate::state::REMEDY_INDEX.to_owned()),
        )
        .into());
    }
    let head = resolve_revision(&revisions, args.revision.as_deref()).map_err(|why| {
        crate::verbs::Pending::usage(
            format!("`--revision` {why}"),
            "run without --revision to judge the newest graph revision the index holds",
        )
    })?;
    let read = {
        let builder = builder_from_config_text(&config).0;
        let lane =
            fathom_analyze::graph::Lane::new(&mut store, builder).map_err(crate::verbs::failed)?;
        inventory(&lane, &head, &rules, &Options::default())
    };
    let read = match read {
        Ok(read) => read,
        Err(partial) => {
            return Err(crate::verbs::Pending::new(
                ExitCode::IndexDegraded,
                format!(
                    "the revision's graph would not materialise: {}",
                    partial.sentence()
                ),
                Some(crate::state::REMEDY_INDEX.to_owned()),
            )
            .into());
        }
    };
    notices.extend(read.notices.clone());

    let plan = Plan::build(
        &ratchet,
        &read.offences,
        grant.as_ref(),
        read.evaluated,
        now,
    );

    // The write. The file is rewritten only when the plan changes it, and only the tightening half of a bare
    // run happens unprompted — creating a baseline that did not exist is a widening act, so it needs the flag
    // even when it would write nothing new.
    let path = root.join(&plan.source);
    let existing = std::fs::read_to_string(&path).ok();
    let rendered = plan.render();
    let may_write = plan.changed() && !args.dry_run && (existing.is_some() || grant.is_some());
    let mut wrote = false;
    if may_write && existing.as_deref() != Some(rendered.as_str()) {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|why| {
                crate::verbs::Pending::new(
                    ExitCode::Error,
                    format!("{}: {why}", path.display()),
                    None,
                )
            })?;
        }
        std::fs::write(&path, &rendered).map_err(|why| {
            crate::verbs::Pending::new(
                ExitCode::Error,
                format!(
                    "the baseline {} would not take the write: {why}",
                    path.display()
                ),
                Some("is the directory writable? the file is the record".to_owned()),
            )
        })?;
        wrote = true;
    }

    // The interlock `e6.5` asked for: an allowance is a decision, so the decision file gets a copy of the
    // reason with an author and a date on it. Best-effort, and said either way.
    let note = if wrote && grant.is_some() && !args.no_note {
        record_decision(root, &mut store, &plan, now)
    } else {
        None
    };

    if args.json {
        print_json(&plan, &read, &head, &path, wrote, note.as_deref());
    } else {
        print_text(&plan, &read, &path, wrote, note.as_deref(), &notices);
    }
    Ok(plan.exit_code().unwrap_or(ExitCode::Ok))
}

/// The deliberate act, or the reason the command line did not describe one.
#[cfg(feature = "sqlite-bundled")]
fn grant(
    args: &RatchetArgs,
    root: &Path,
    now: Timestamp,
) -> Result<Option<fathom_analyze::ratchet::Grant>, eyre::Report> {
    use fathom_analyze::ratchet::{DEFAULT_GRACE_DAYS, Grant, parse_date};

    if !args.update_baseline {
        if !args.key.is_empty() {
            return Err(crate::verbs::Pending::usage(
                "`--key` names the violations to grant, and without --update-baseline nothing is being \
                 granted",
                "add --update-baseline --reason \"...\"",
            )
            .into());
        }
        return Ok(None);
    }
    let reason = args
        .reason
        .as_deref()
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| {
            crate::verbs::Pending::usage(
                "`--update-baseline` raises the baseline, so it has to say why",
                "fathom ratchet --update-baseline --reason \"the split lands with the ledger rewrite\"",
            )
        })?;
    // The two "who"s of the act: the owner is who pays it off, the granter is who spent the permission. Both
    // default to the reviewer's own identity, which is the same ladder `fathom review sign` climbs.
    let (owner, _) = crate::review::reviewer(root, args.owner.as_deref(), None);
    let (granter, _) = crate::review::reviewer(root, args.granted_by.as_deref(), None);
    let expires = match args.expires.as_deref() {
        None => Some(day_after(now, DEFAULT_GRACE_DAYS)),
        Some(text) if text.eq_ignore_ascii_case("never") => None,
        Some(text) => match text.strip_prefix('+').map(str::to_string) {
            Some(days) => {
                let days: i64 = days.parse().map_err(|_| {
                    crate::verbs::Pending::usage(
                        format!("`--expires {text}` is not `YYYY-MM-DD`, `never` or `+N` days"),
                        "a quarter is the default: leave the flag out",
                    )
                })?;
                Some(day_after(now, days))
            }
            None => Some(parse_date(text).ok_or_else(|| {
                crate::verbs::Pending::usage(
                    format!("`--expires {text}` is not a `YYYY-MM-DD` day"),
                    "the day the allowance is live *through*, not the day after it",
                )
            })?),
        },
    };
    let grant = Grant::new(
        &owner.display,
        reason,
        expires,
        day_after(now, 0),
        &granter.display,
    )
    .map_err(|why| {
        crate::verbs::Pending::usage(why, "an owner and a reason, both of them non-empty")
    })?
    .with_keys(&args.key);
    Ok(Some(grant))
}

/// The first instant after the day `days` from now, which is how this file stores a day.
#[cfg(feature = "sqlite-bundled")]
fn day_after(now: Timestamp, days: i64) -> Timestamp {
    use fathom_analyze::ratchet::parse_date;
    let (year, month, day) =
        Timestamp::from_unix_micros(now.as_unix_micros() + days * MICROS_PER_DAY).civil();
    parse_date(&format!("{year:04}-{month:02}-{day:02}")).unwrap_or(now)
}

/// Append the decision note the loosening deserves, or say why it did not happen.
#[cfg(feature = "sqlite-bundled")]
fn record_decision(
    root: &Path,
    store: &mut fathom_store::spine::Store,
    plan: &fathom_analyze::ratchet::Plan,
    now: Timestamp,
) -> Option<String> {
    use fathom_analyze::notes::collect;
    use fathom_analyze::notes::{Draft, Kind, NoteTarget};

    let millis = now.as_unix_micros() / 1_000;
    let (author, _) = crate::review::reviewer(root, None, None);
    const SHOWN: usize = 6;
    let rows: Vec<String> = plan
        .added
        .iter()
        .chain(plan.renewed.iter())
        .take(SHOWN)
        .map(|row| {
            format!(
                "{} {} (owner {}, until {})",
                row.kind,
                row.key,
                if row.owner.is_empty() {
                    "nobody"
                } else {
                    &row.owner
                },
                row.expiry_day().unwrap_or_else(|| "no end".to_string())
            )
        })
        .collect();
    let granted = plan.added.len() + plan.renewed.len();
    let mut text = format!(
        "Ratchet raised in {}: {} — {}. Reason: {}",
        plan.source,
        if plan.renewed.is_empty() {
            format!("{granted} allowance(s) granted")
        } else {
            format!(
                "{} granted, {} re-granted",
                plan.added.len(),
                plan.renewed.len()
            )
        },
        rows.join("; "),
        plan.added
            .first()
            .or(plan.renewed.first())
            .map(|row| row.reason.clone())
            .unwrap_or_default(),
    );
    if granted > SHOWN {
        text.push_str(&format!(" ({} more in the file)", granted - SHOWN));
    }
    let target = NoteTarget::path(&plan.source, None);
    let draft = Draft {
        via: "fathom ratchet --update-baseline",
        ..Draft::new(Kind::Decision, &target, &author, &text, millis)
    };
    match collect::append(store, root, &draft, millis) {
        Ok((note, _)) => Some(note.id),
        Err(why) => {
            crate::output::problem(format_args!(
                "note: the decision note was not written ({why}) — the baseline row is still committed, \
                 which is the half that enforces anything"
            ));
            None
        }
    }
}

/// The human report, in the order a reviewer acts on it.
#[cfg(feature = "sqlite-bundled")]
fn print_text(
    plan: &fathom_analyze::ratchet::Plan,
    read: &fathom_analyze::ratchet::Inventory,
    path: &Path,
    wrote: bool,
    note: Option<&str>,
    notices: &[String],
) {
    use crate::output;
    let mut printed: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    output::line(format_args!(
        "ratchet  mode {} · baseline {}{}",
        plan.mode.as_str(),
        plan.source,
        if plan.baselined { "" } else { " (none yet)" }
    ));
    output::line(format_args!("  {}", read.sentence()));
    output::line(format_args!("  {}", plan.summary()));
    print_block("what changed in the file", &plan.diff());
    if !plan.uncovered.is_empty() {
        let lines: Vec<String> = plan
            .uncovered
            .iter()
            .map(|offence| format!("{}/{} — {}", offence.kind, offence.key, offence.body))
            .collect();
        print_block(
            &format!("uncovered ({}, news unless the mode is off)", lines.len()),
            &lines,
        );
    }
    if !plan.expiring.is_empty() {
        print_block(
            &format!(
                "expiring within {} day(s) ({})",
                fathom_analyze::ratchet::EXPIRY_WARNING_DAYS,
                plan.expiring.len()
            ),
            &plan.expiring_lines(),
        );
    }
    if !plan.lapsed.is_empty() {
        print_block(
            &format!("back due ({})", plan.lapsed.len()),
            &plan.lapsed_lines(),
        );
    }
    for line in &plan.refused {
        output::line(format_args!("  refused: {line}"));
    }
    // The plan carries the ledger reader's own notices plus the ones only a plan can tell; the verb's list is
    // the rule and graph reads' caveats. Neither may be swallowed, and neither may be printed twice.
    for line in plan.notices.iter().chain(notices.iter()) {
        if printed.insert(line.clone()) {
            output::line(format_args!("  notice: {line}"));
        }
    }
    if wrote {
        output::line(format_args!(
            "wrote {} ({} row(s)); the diff is the review",
            path.display(),
            plan.after.len()
        ));
    } else if plan.changed() {
        output::line(format_args!(
            "nothing written: {} row(s) would change and this run was told not to write",
            plan.diff().len()
        ));
    }
    if let Some(id) = note {
        output::line(format_args!(
            "decision note {id} — the reason is in the review memory as well as the file"
        ));
    }
}

/// One labelled block, or nothing when there is nothing to say.
#[cfg(feature = "sqlite-bundled")]
fn print_block(label: &str, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    crate::output::line(format_args!("  {label}:"));
    for line in lines.iter().take(20) {
        crate::output::line(format_args!("    {line}"));
    }
    if lines.len() > 20 {
        crate::output::line(format_args!("    … and {} more", lines.len() - 20));
    }
}

/// The machine report: the plan's own document, plus the facts only the composition root knows.
#[cfg(feature = "sqlite-bundled")]
fn print_json(
    plan: &fathom_analyze::ratchet::Plan,
    read: &fathom_analyze::ratchet::Inventory,
    head: &str,
    path: &Path,
    wrote: bool,
    note: Option<&str>,
) {
    use serde_json::json;
    let mut payload = plan.to_json();
    if let Some(map) = payload.as_object_mut() {
        map.insert("revision".to_string(), json!(short(head)));
        map.insert("revision_full".to_string(), json!(head));
        map.insert(
            "inventory".to_string(),
            json!({
                "cycles": read.cycles,
                "layer_violations": read.layer_violations,
                "judged_cycles": read.evaluated.cycles,
                "judged_layer": read.evaluated.layer,
            }),
        );
        map.insert("path".to_string(), json!(path.display().to_string()));
        map.insert("wrote".to_string(), json!(wrote));
        map.insert("decision_note".to_string(), json!(note));
    }
    crate::output::line(format_args!("{payload}"));
}

/// Which revision to judge: the newest, or the caller's, matched as an exact oid or an unambiguous prefix.
#[cfg(feature = "sqlite-bundled")]
fn resolve_revision(revisions: &[String], wanted: Option<&str>) -> Result<String, String> {
    let Some(wanted) = wanted else {
        return Ok(revisions[0].clone());
    };
    let wanted = wanted.trim().to_ascii_lowercase();
    if revisions.iter().any(|tree| tree == &wanted) {
        return Ok(wanted.clone());
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

#[cfg(feature = "sqlite-bundled")]
fn short(revision: &str) -> String {
    if revision.chars().count() <= 10 {
        return revision.to_owned();
    }
    revision.chars().take(7).collect()
}
