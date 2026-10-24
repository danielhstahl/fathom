//! `fathom check` — the CI gate (`e6.6`, `reco-ngk`).
//!
//! One call, one exit code, five output formats. This file is the composition root's half and nothing
//! else: it reads the policy and the ratchet file, resolves the range, opens the store and the object
//! database, asks the engines that already exist what the range did, and hands the answers to
//! [`fathom_analyze::check`], which owns severity, threshold, sentence and format. ADR-002 §4.3 again:
//! the logic is downhill, and this file's size is the cost of holding a store handle, a git read and a
//! clock so the analysis does not have to.
//!
//! # The six checks, and where each one's evidence actually comes from
//!
//! | check | evidence | needs |
//! |---|---|---|
//! | `layering` | [`fathom_analyze::layering::episode`] — the delta of violations between two module-graph revisions, with `.fathom/ratchet.toml` already applied | index, `[layers]` + `[layering]` |
//! | `cycles` | [`fathom_analyze::cycles::episode`] — cycles maintained across the episode, same ledger | index |
//! | `api` | [`fathom_analyze::api::compare`] — the surface of both revisions, diffed and classified | both trees in the clone |
//! | `dependencies` | [`fathom_analyze::deps::episode_repository`] — the dependency delta over the changed paths | both trees in the clone |
//! | `tests` | the index's per-episode file rows, split by surface | index |
//! | `sign-off` | the `e4.4` score over each episode, against the review marker | index |
//!
//! # The cost, and the order it is spent in
//!
//! The acceptance criterion is 60 seconds on a medium repository. The order below is the answer to it:
//!
//! ```text
//!   policy + endpoints         milliseconds; refuses before any expensive read
//!   store-dependent checks     one chain walk each for layering and cycles; per-episode row reads
//!   api                        two tree listings + one blob per surface-bearing file, both sides —
//!                              the fast path returns on the digests alone when nothing public moved
//!   dependencies               the changed-path walk + one blob per changed dependency file
//! ```
//!
//! Nothing reads the network (`tools/check_privacy.py`'s rule, and the reason no advisory or licence
//! lookup is on this path), and nothing requires a full history: the graph checks work from what the index
//! holds, and the two tree-based checks say `blocked` with the fetch command when the clone does not have
//! the base.
//!
//! # What "no silent passes" costs, and it costs three rules
//!
//! 1. an unreadable `[check]` table is exit `5`, never a default;
//! 2. a check that could not judge is `blocked`, with a reason and a remedy — never an empty list that
//!    reads as clean;
//! 3. if nothing at all was examined and at least one check was blocked, the run **fails**. A gate that
//!    exits `0` having examined nothing is the failure mode the whole ticket exists to avoid.
//!
//! # What is deliberately not here
//!
//! **No upload.** SARIF, step annotations and the job summary are written where you tell them to be;
//! posting is your pipeline's action with your pipeline's token (ADR-001 §8, non-goal 4).
//!
//! **No write of the ratchet file.** `fathom ratchet --update-baseline` is the only act that loosens the
//! ledger, and it requires a reason. A gate that could widen its own baseline would be a gate with a
//! `--make-it-pass` flag.

use std::io::Write as _;
use std::path::Path;

use fathom_analyze::check::{
    Check, CheckRun, Evidence, Finding, Gate, Policy, Range, SarifContext, Severity, Status,
};
use fathom_analyze::ratchet::Mode;
use fathom_core::exit::ExitCode;

use crate::verbs::{CheckArgs, Pending};

/// The five formats, and the only spelling each accepts.
const FORMATS: [&str; 5] = ["human", "json", "sarif", "github", "markdown"];

/// The gate.
pub fn run(args: &CheckArgs) -> eyre::Result<ExitCode> {
    let root = Path::new(&args.root);
    let redactor = crate::sanitize::Redactor::for_root(root);
    let repository =
        fathom_ingest::repo::Repository::discover(root).map_err(crate::verbs::failed)?;
    let config = std::fs::read_to_string(root.join("fathom.toml")).unwrap_or_default();

    // The policy first: every later decision is a decision under it, and an unreadable one is a refusal
    // the operator has to fix rather than a set of defaults that happen to be permissive.
    let mut policy = fathom_analyze::check::Policy::from_config_text(&config).map_err(|err| {
        Pending::new(
            ExitCode::ConfigInvalid,
            format!("`fathom check` cannot read `[check]`: {err}"),
            Some("fix `fathom.toml`, or delete the `[check]` table to run on the documented defaults".to_owned()),
        )
    })?;
    if let Err(problem) = apply_overrides(args, &mut policy) {
        return Err(Pending::usage(problem.clone(), "see `fathom check --help`").into());
    }
    let format = choose_format(args)?;
    if args.explain {
        return Ok(explain(&policy));
    }

    // ---- the endpoints ------------------------------------------------------------------------------------------------------------------
    let (base_spec, base_from, head_spec, head_from) = endpoints(args, &policy);
    // `endpoints` reports a malformed `--range` by putting the complaint where the base's *source* goes;
    // there is no range to resolve until that is fixed, and this is the only caller.
    if let Some(problem) = base_from.strip_prefix(RANGE_ERROR) {
        return Err(Pending::usage(
            format!("`--range {problem}` is not `base..head`"),
            "e.g. `--range origin/main..HEAD`, or name the ends separately with `--base` and `--head`",
        )
        .into());
    }
    let head_oid = resolve(&repository, &head_spec).map_err(|why| {
        Pending::usage(
            format!("`--head {head_spec}` {why}"),
            "a revision git can name: `HEAD`, a branch, a tag, an oid prefix",
        )
    })?;
    let head_tree = tree_of(&repository, head_oid);
    // The base may be missing from the clone — a depth-1 checkout is the usual reason — and that is a
    // state of the pipeline, not a crash: the checks that need it report themselves blocked.
    let base = match resolve_optional(&repository, &base_spec) {
        Some(oid) => (Some(oid), tree_of(&repository, oid), None),
        None => (
            None,
            None,
            Some(format!(
                "the base revision `{base_spec}` is not in this clone{}",
                if repository.is_shallow().unwrap_or(false) {
                    " — it is a shallow clone"
                } else {
                    ""
                }
            )),
        ),
    };
    let (base_oid, base_tree, base_missing) = base;

    let mut notices: Vec<String> = policy.notices.clone();
    notices.extend(repository_notes(
        &repository,
        &head_spec,
        &base_spec,
        base_from.as_str(),
    ));

    // ---- the checks -------------------------------------------------------------------------------------------------------------------
    let mut runs = Vec::new();
    runs.extend(gather_graph(
        root,
        &config,
        &mut notices,
        args,
        &policy,
        base_tree,
        head_tree,
    ));
    runs.extend(gather_episodes(
        root,
        &config,
        args,
        &policy,
        &base_spec,
        &head_spec,
        &mut notices,
    ));
    runs.extend(gather_api(
        &repository,
        &config,
        &policy,
        base_tree,
        head_tree,
        base_missing.as_deref(),
        &mut notices,
    ));
    let dependency_run = gather_dependencies(
        &repository,
        &policy,
        base_tree,
        head_tree,
        base_missing.as_deref(),
    );
    runs.push(dependency_run);

    // The episode count the range holds, from the index: reported whatever ran, because "3 findings in
    // `main..HEAD`" means something different for a two-commit range and a two-hundred-commit one.
    let episodes = episodes_in(&repository, &base_spec, &head_spec);
    let (episodes, partial) = match episodes {
        Ok(total) => {
            if total > policy.episode_cap {
                (
                    total,
                    Some(format!(
                        "the range holds {total} episodes; the episode-shaped checks read the newest {}",
                        policy.episode_cap
                    )),
                )
            } else {
                (total, None)
            }
        }
        Err(why) => {
            notices.push(why);
            (0, None)
        }
    };

    let range = Range {
        base: short(
            base_oid
                .map(|oid| oid.hex())
                .as_deref()
                .unwrap_or(&base_spec),
        ),
        head: short(&head_oid.hex()),
        base_from,
        head_from,
        episodes,
        episodes_partial: partial,
    };
    // A row for every check the policy switched off. The alternative — a table with four rows where the
    // policy names six — is how a `--only`, a `mode = off` or a `[check.severity] tests = "off"` becomes
    // invisible to the person reading the log, which is the silent pass with the volume turned down.
    let present: Vec<Check> = runs.iter().map(|run| run.check).collect();
    runs.extend(
        Check::ALL
            .iter()
            .filter(|check| !present.contains(check) && policy.severity(**check).is_none())
            .map(|check| CheckRun::off(*check)),
    );
    let mut gate = Gate::build(policy, range, runs);
    gate.producer = format!("fathom {}", env!("CARGO_PKG_VERSION"));
    gate.notices = notices;
    let _ = &redactor;

    emit(args, &gate, &repository, head_oid, format)?;
    Ok(gate.exit_code())
}

/// One line per endpoint: where it came from, and what the clone could not name.
fn repository_notes(
    repository: &fathom_ingest::repo::Repository,
    head_spec: &str,
    base_spec: &str,
    base_from: &str,
) -> Vec<String> {
    let mut out = Vec::new();
    if repository.is_shallow().unwrap_or(false) {
        out.push(
            "this is a shallow clone: the checks that need a base revision can only run if the base is \
             inside the fetched depth (`actions/checkout` with `fetch-depth: 0`, or `git fetch --depth \
             <n>`)"
                .to_owned(),
        );
    }
    let _ = (repository, head_spec, base_spec, base_from);
    out
}

/// The flags over the file: `--fail-on`, `--severity`, `--only`/`--skip`, `--require`.
fn apply_overrides(args: &CheckArgs, policy: &mut Policy) -> Result<(), String> {
    if let Some(max) = args.max {
        if max == 0 {
            return Err("`--max 0` would bound the list to nothing; the counts stay whole either way, so ask for at least 1".to_owned());
        }
        policy.max_findings = max;
    }
    if let Some(text) = &args.fail_on {
        policy.fail_on = match text.as_str() {
            "never" => None,
            other => Some(Severity::parse(other).ok_or_else(|| {
                format!("`--fail-on {text}` is not note, warning, error or never")
            })?),
        };
        // Asking out loud for a threshold is asking for it to bite. A repository whose file says
        // `mode = "warn"` — the shipped default, and the honest way to adopt a gate — still gets a
        // non-blocking nightly from the same config, because that job does not pass `--fail-on`.
        // `mode = "off"` is *not* overridden: that key is the operator's kill switch, and a flag that
        // silently re-enables a deliberately disabled gate is the mistake this verb exists to avoid.
        if policy.fail_on.is_some() && policy.mode == Mode::Warn {
            policy.mode = Mode::Deny;
            policy.notices.push(
                "`--fail-on` enforces: `check.mode` was `warn`, so this run blocks at or above the \
                 threshold — `mode = \"deny\"` in `fathom.toml` is the durable way to say it"
                    .to_owned(),
            );
        }
    }
    for token in &args.severity {
        let (name, value) = token
            .split_once('=')
            .ok_or_else(|| format!("`--severity {token}` wants `check=severity`"))?;
        let check = Check::parse(name)
            .ok_or_else(|| format!("`--severity {token}` names no check: {}", names()))?;
        let severity = match value {
            "off" => None,
            other => Some(Severity::parse(other).ok_or_else(|| {
                format!("`--severity {token}` is not off, note, warning or error")
            })?),
        };
        set_severity(policy, check, severity);
    }
    let only = resolve_checks(&args.only, "only")?;
    let skip = resolve_checks(&args.skip, "skip")?;
    if !only.is_empty() {
        for check in Check::ALL {
            let wanted = only.contains(&check) && !skip.contains(&check);
            if !wanted {
                set_severity(policy, check, None);
            }
        }
    }
    for check in &skip {
        set_severity(policy, *check, None);
    }
    for check in resolve_checks(&args.require, "require")? {
        if !policy.requires(check) {
            policy.require.push(check);
        }
    }
    for pattern in &args.ignore {
        // Validated exactly as the file's `ignore` is: the family half names a check or the pattern is a
        // refusal, because a typo here would silently silence nothing.
        let family = pattern.split(':').next().unwrap_or_default();
        if Check::parse(family).is_none() {
            return Err(format!(
                "`--ignore {pattern}` is not `<check>` or `<check>:<rule>`, e.g. `tests:fixtures`"
            ));
        }
        if !policy.ignore.iter().any(|had| had == pattern) {
            policy.ignore.push(pattern.clone());
        }
    }
    Ok(())
}

/// The `--only`/`--skip`/`--require` lists, with the refusal that names the six options.
fn resolve_checks(list: &[String], flag: &str) -> Result<Vec<Check>, String> {
    let mut out = Vec::with_capacity(list.len());
    for name in list {
        out.push(
            Check::parse(name)
                .ok_or_else(|| format!("`--{flag} {name}` names no check: {}", names()))?,
        );
    }
    Ok(out)
}

/// The six check names, for the refusal that needs them.
fn names() -> String {
    Check::ALL
        .iter()
        .map(|check| format!("`{}`", check.as_str()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Write one check's volume, by its position in [`Check::ALL`].
fn set_severity(policy: &mut Policy, check: Check, severity: Option<Severity>) {
    if let Some((index, _)) = Check::ALL.iter().enumerate().find(|(_, c)| **c == check) {
        policy.severity[index] = severity;
    }
}

/// The output format: `--format`, or `--json` as its shortcut, defaulting to the human log.
fn choose_format(args: &CheckArgs) -> Result<&'static str, eyre::Report> {
    let asked = if args.json && args.format == "human" {
        "json"
    } else {
        args.format.as_str()
    };
    if args.json && args.format != "human" && args.format != "json" {
        return Err(Pending::usage(
            "`--json` and `--format` are the same knob; pick one",
            "drop --json, or drop --format",
        )
        .into());
    }
    FORMATS
        .iter()
        .find(|format| **format == asked)
        .copied()
        .ok_or_else(|| {
            Pending::usage(
                format!("`--format {}` is not a format", args.format),
                "the formats are human, json, sarif, github and markdown",
            )
            .into()
        })
}

/// The endpoints, and where each came from.
/// The marker `endpoints` puts in front of the base's *source* when `--range` could not be split.
///
/// A four-string tuple already carries four things nobody may confuse; a fifth for one malformed flag would
/// have been a `Result` bolted onto a struct, and the flag is one line of grammar.
const RANGE_ERROR: &str = "\x00range:";

fn endpoints(args: &CheckArgs, policy: &Policy) -> (String, String, String, String) {
    let mut base = args.base.clone();
    let mut head = args.head.clone();
    let mut base_from = "--base".to_owned();
    let mut head_from = "--head".to_owned();
    if let Some(range) = &args.range {
        // A `--range` with no `..` is a mistyped value, and quietly ignoring the flag would run a different
        // comparison than the one that was asked for — the base would come from the file or from `HEAD~1`
        // and the log would say so only in a line nobody reads.
        let Some((from, to)) = range.split_once("..") else {
            return (
                String::new(),
                format!("{RANGE_ERROR}{range}"),
                String::new(),
                String::new(),
            );
        };
        base.get_or_insert_with(|| from.to_owned());
        head.get_or_insert_with(|| to.to_owned());
        base_from = "--range".to_owned();
        head_from = "--range".to_owned();
    }
    if base.is_none() {
        if let Some(text) = &policy.base {
            base = Some(text.clone());
            base_from = "`[check] base` in fathom.toml".to_owned();
        } else {
            base = Some("HEAD~1".to_owned());
            base_from = "the built-in default `HEAD~1`".to_owned();
        }
    }
    let head = head.unwrap_or_else(|| policy.head.clone());
    let head_from = if head_from == "--head" && args.head.is_none() {
        if policy.head == "HEAD" {
            "the built-in default `HEAD`".to_owned()
        } else {
            "`[check] head` in fathom.toml".to_owned()
        }
    } else {
        head_from
    };
    (
        base.unwrap_or_else(|| "HEAD~1".to_owned()),
        base_from,
        head,
        head_from,
    )
}

/// Resolve a revision, or say why it names nothing.
fn resolve(
    repository: &fathom_ingest::repo::Repository,
    spec: &str,
) -> Result<fathom_ingest::oid::Oid, String> {
    fathom_ingest::revspec::resolve(repository, spec)
        .map_err(|err| format!("names no commit here: {err}"))
}

/// Resolve a revision, `None` when the clone cannot name it.
fn resolve_optional(
    repository: &fathom_ingest::repo::Repository,
    spec: &str,
) -> Option<fathom_ingest::oid::Oid> {
    resolve(repository, spec).ok()
}

/// The tree a commit points at, `None` when the object is missing or is not a commit.
fn tree_of(
    repository: &fathom_ingest::repo::Repository,
    oid: fathom_ingest::oid::Oid,
) -> Option<fathom_ingest::oid::Oid> {
    repository
        .commit(oid)
        .ok()
        .flatten()
        .map(|commit| commit.tree)
}

/// A short oid, or the spec a caller passed when there is no oid to shorten.
fn short(oid: &str) -> String {
    oid.chars().take(12).collect()
}

// ---------------------------------------------------------------------------
// The graph checks: layering and cycles, from one materialised revision pair.
// ---------------------------------------------------------------------------

/// Layering and cycles, in a build that can read the module graph.
#[cfg(feature = "sqlite-bundled")]
fn gather_graph(
    root: &Path,
    config: &str,
    notices: &mut Vec<String>,
    _args: &CheckArgs,
    policy: &Policy,
    base_tree: Option<fathom_ingest::oid::Oid>,
    head_tree: Option<fathom_ingest::oid::Oid>,
) -> Vec<CheckRun> {
    use fathom_analyze::cycles::debt::Ratchet;
    use fathom_store::spine::Store;

    let layering_volume = policy.severity(Check::Layering);
    let cycle_volume = policy.severity(Check::Cycles);
    if layering_volume.is_none() && cycle_volume.is_none() {
        return Vec::new();
    }
    let store_path = fathom_analyze::index::default_store_path(root);
    if !store_path.is_file() {
        return blocked_pair(
            policy,
            format!(
                "no index at {} yet, so the module graph is not visible",
                crate::sanitize::Redactor::for_root(root).path(&store_path)
            ),
        );
    }
    let mut store = match Store::open(&store_path) {
        Ok(store) => store,
        Err(err) => {
            notices.push(format!("the store would not open: {err}"));
            return blocked_pair(policy, "the index store would not open".to_owned());
        }
    };
    let revisions = match store.graph_revisions() {
        Ok(revisions) => revisions,
        Err(err) => {
            notices.push(format!("the store's graph revisions would not read: {err}"));
            return blocked_pair(
                policy,
                "the index's graph revisions would not read".to_owned(),
            );
        }
    };
    if revisions.is_empty() {
        // The remedy here is deliberately *not* a shrug. `fathom index` walks commits, blobs and
        // structure, and since `e3.11` it also runs the graph pass that writes `graph_snapshot` rows, so
        // "run `fathom index`" is a remedy the reader can try and that works.
        // The remedy here is `fathom index`, and since `e3.11` (`reco-dlp`) that is a remedy that works:
        // the index's graph pass is what writes these rows. Two states are still said separately, because
        // each has a different next act: a repository that turned the pass off (`graph.enabled = false`)
        // has made a choice rather than left work undone, and an index that has every revision but the one
        // being judged is a window problem, not an index problem.
        let disabled = fathom_analyze::graph::GraphPolicy::from_repo_config_text(config)
            .0
            .disabled_reason()
            .map(str::to_owned);
        return blocked_pair_with(
            policy,
            "this index holds no module graph, so neither layering nor cycles can be judged",
            match disabled {
                Some(why) => format!(
                    "the graph pass is switched off for this repository — {why}. Set it back to true and \
                     run `fathom index`, or pass `--skip layering --skip cycles` to say out loud that \
                     these two checks are not part of this gate"
                ),
                None => "run `fathom index`: the module graph is written by its graph pass, which caps \
                        itself at `graph.window` revisions and `graph.max_corpus_mib` of source, so a \
                        refusal there is printed by the index and not invented here"
                    .to_owned(),
            },
        );
    }
    // The pair to compare, in the store's own spellings (tree oids, newest first). A revision the index
    // has never seen cannot be judged, so the head falls back to the newest row and says so; the base
    // falls back to the row before it, which is the most honest "what did this turn do" a shallow clone
    // can still answer.
    let head_hex = head_tree.map(|tree| tree.hex()).unwrap_or_default();
    let head_index = revisions
        .iter()
        .position(|tree| *tree == head_hex)
        .unwrap_or(0);
    let to = revisions[head_index].clone();
    let from = base_tree
        .and_then(|tree| {
            revisions
                .iter()
                .find(|candidate| **candidate == tree.hex())
                .cloned()
        })
        .or_else(|| revisions.get(head_index + 1).cloned())
        .unwrap_or_else(|| to.clone());
    let mut run_notices = Vec::new();
    if head_index != 0 && !head_hex.is_empty() {
        run_notices.push(format!(
            "the head revision is not the newest graph row the index holds; judged `{}`",
            short(&to)
        ));
    }
    if from == to {
        return vec![
            blocked_run(
                Check::Layering,
                layering_volume,
                "the index holds one graph revision, so there is nothing to compare this revision against — index one commit before it",
            ),
            blocked_run(
                Check::Cycles,
                cycle_volume,
                "the index holds one graph revision, so a new cycle cannot be told from an old one",
            ),
        ];
    }

    let (layers, _) = fathom_analyze::graph::layers::Layers::from_repo_config_text(config);
    let (rules, rule_notices) =
        fathom_analyze::layering::Rules::from_repo_config_text(&layers, config);
    let (_, baseline_path, _) = Ratchet::config(config);
    let ratchet = match std::fs::read_to_string(root.join(&baseline_path)) {
        Ok(text) => Ratchet::from_text(&text, &baseline_path),
        Err(_) => Ratchet::empty(),
    };
    let now = fathom_core::time::Timestamp::from_unix_micros(
        crate::clock::now_millis().saturating_mul(1_000),
    );
    let builder = fathom_analyze::graph::builder_from_config_text(config).0;
    let lane = match fathom_analyze::graph::Lane::new(&mut store, builder) {
        Ok(lane) => lane,
        Err(err) => {
            notices.push(format!("the graph lane would not open: {err}"));
            return blocked_pair(policy, "the graph lane would not open".to_owned());
        }
    };

    let mut out = Vec::new();
    // Layering. The report's own delta and verdict already applied the ratchet, which is the whole
    // reason the gate reads it rather than the raw assessment.
    if let Some(volume) = layering_volume {
        match fathom_analyze::layering::episode(
            &lane,
            &from,
            &to,
            &rules,
            &fathom_analyze::layering::Locate::empty(),
            &ratchet,
            now,
        ) {
            Ok(report) => {
                let examined = report.counters.checked;
                let mut findings = Vec::new();
                for entry in &report.verdict.violations {
                    if !entry.introduced {
                        // Pre-existing and unbudgeted is a fact about the repository, not about this
                        // range; the ratchet's own verb reports it, and the gate reports news.
                        continue;
                    }
                    let violation = &entry.violation;
                    let line = violation.line.map(|line| line as u32);
                    findings.push(
                        Finding::new(
                            Check::Layering,
                            // The engine's *key*, not its rule name: the ratchet ledger is keyed the same
                            // way, so a `check.ignore` entry can be copied out of `.fathom/ratchet.toml` and
                            // silence one edge — or, as a prefix (`layering:no_upward`), a whole rule.
                            violation.key(),
                            volume,
                            entry.sentence(),
                        )
                        .with_evidence(Evidence::file(
                            violation.src.path.clone(),
                            line,
                            format!("graph:layering ({})", violation.edge),
                        ))
                        .with_detail(format!("why: {}", violation.why))
                        .with_detail(format!("fix: {}", violation.fix))
                        .with_detail(format!("rule declared in {}", violation.declared_in)),
                    );
                }
                for back in &report.verdict.back {
                    findings.push(
                        Finding::new(Check::Layering, "allowance-lapsed", volume, back.sentence())
                            .with_evidence(Evidence::pointer(format!("ratchet:{}", baseline_path))),
                    );
                }
                let mut run = CheckRun::ran(Check::Layering, Some(volume), examined, findings);
                run.notes = rule_notices.clone();
                run.notes.extend(report.notices.clone());
                // *"using ratchet baselines so existing debt does not block"* has to be visible in the row,
                // or a green run reads as "there was nothing to forgive" and the ledger looks decorative.
                let forgiven = report.baselined_violations();
                if forgiven > 0 {
                    run.notes.push(format!(
                        "{forgiven} violation(s) are baselined in {}, so they were not news to this range",
                        baseline_path
                    ));
                }
                run.notes.push(format!(
                    "compared graph revisions {} → {}; line evidence is the graph's, so findings carry \
                     a path and not a line unless the extractor recorded one",
                    short(&from),
                    short(&to)
                ));
                if rules.is_inert() {
                    run.status = Status::Blocked {
                        reason: "fathom.toml declares no [layering] rules, so nothing is enforced"
                            .to_owned(),
                        remedy: Some(
                            "declare `[layers]` and `[layering]`, or set \
                             `[check.severity] layering = \"off\"` to say you meant it"
                                .to_owned(),
                        ),
                    };
                    run.findings.clear();
                    run.total = 0;
                }
                out.push(run);
            }
            Err(partial) => out.push(
                blocked_run(
                    Check::Layering,
                    Some(volume),
                    format!(
                        "the graph could not answer between {} and {}: {}",
                        short(&from),
                        short(&to),
                        partial.sentence()
                    ),
                )
                .with_remedy("run `fathom index` so the chain reaches an anchor"),
            ),
        }
    }
    // Cycles. Same pair, same ledger, the engine's own `introduced` flag deciding news.
    if let Some(volume) = cycle_volume {
        match fathom_analyze::cycles::episode(
            &lane,
            &from,
            &to,
            &ratchet,
            fathom_analyze::cycles::Options::default(),
            now,
        ) {
            Ok(report) => {
                let examined = report.nodes_in_cycles().max(report.cycle_count());
                let findings: Vec<Finding> = report
                    .verdict
                    .violations
                    .iter()
                    .filter(|violation| violation.introduced)
                    .map(|violation| {
                        Finding::new(
                            Check::Cycles,
                            // The cycle itself, not a generic label: `check.ignore =
                            // ["cycles:billing"]` then silences one cycle and `["cycles"]` silences the
                            // family, which is the pair of granularities a reviewer actually wants.
                            violation.cycle.key(),
                            volume,
                            violation.sentence(),
                        )
                        .with_evidence(Evidence::pointer(format!(
                            "graph:cycle {}",
                            violation.cycle.display()
                        )))
                        .with_detail(format!("members: {}", violation.cycle.members.join(" ↔ ")))
                    })
                    .collect();
                let mut run = CheckRun::ran(Check::Cycles, Some(volume), examined, findings);
                run.notes = vec![format!(
                    "{} cycle(s) in the revision, {} inside one; {} baselined as debt",
                    report.cycle_count(),
                    report.nodes_in_cycles(),
                    report.verdict.debt.len()
                )];
                out.push(run);
            }
            Err(partial) => out.push(
                blocked_run(
                    Check::Cycles,
                    Some(volume),
                    format!(
                        "the graph could not answer between {} and {}",
                        short(&from),
                        short(&to)
                    ),
                )
                .with_remedy(format!("the chain walk stopped: {}", partial.sentence())),
            ),
        }
    }
    let _ = base_tree;
    out
}

/// The two graph checks, blocked for one reason.
#[cfg(feature = "sqlite-bundled")]
fn blocked_pair(policy: &Policy, reason: String) -> Vec<CheckRun> {
    blocked_pair_with(policy, reason, crate::state::REMEDY_INDEX.to_owned())
}

/// The two graph rows, blocked, with a remedy chosen for the reason.
///
/// The reason and the remedy are separate arguments because they are separately read: the reason goes into
/// the log line a maintainer screenshots, the remedy is the sentence that decides whether they can fix it
/// themselves or have to file something.
fn blocked_pair_with(
    policy: &Policy,
    reason: impl Into<String>,
    remedy: impl Into<String>,
) -> Vec<CheckRun> {
    let reason = reason.into();
    let remedy = remedy.into();
    let mut out = Vec::new();
    if let Some(volume) = policy.severity(Check::Layering) {
        out.push(
            CheckRun::blocked(Check::Layering, Some(volume), reason.clone())
                .with_remedy(remedy.clone()),
        );
    }
    if let Some(volume) = policy.severity(Check::Cycles) {
        out.push(CheckRun::blocked(Check::Cycles, Some(volume), reason).with_remedy(remedy));
    }
    out
}

/// One blocked run, with the default remedy for a missing index.
#[cfg(feature = "sqlite-bundled")]
fn blocked_run(check: Check, volume: Option<Severity>, reason: impl Into<String>) -> CheckRun {
    CheckRun::blocked(check, volume, reason).with_remedy(crate::state::REMEDY_INDEX)
}

/// The graph checks in a build with no store to read them from.
#[cfg(not(feature = "sqlite-bundled"))]
fn gather_graph(
    _root: &Path,
    _config: &str,
    _notices: &mut Vec<String>,
    _args: &CheckArgs,
    policy: &Policy,
    _base_tree: Option<fathom_ingest::oid::Oid>,
    _head_tree: Option<fathom_ingest::oid::Oid>,
) -> Vec<CheckRun> {
    let reason = "this binary was built without the `sqlite-bundled` feature, so it cannot read the \
                  module graph";
    let mut out = Vec::new();
    if let Some(volume) = policy.severity(Check::Layering) {
        out.push(
            CheckRun::blocked(Check::Layering, Some(volume), reason).with_remedy(REBUILD_HEADLESS),
        );
    }
    if let Some(volume) = policy.severity(Check::Cycles) {
        out.push(
            CheckRun::blocked(Check::Cycles, Some(volume), reason).with_remedy(REBUILD_HEADLESS),
        );
    }
    out
}

/// The build a headless CI job wants: no terminal, everything else.
#[cfg_attr(feature = "sqlite-bundled", allow(dead_code))]
const REBUILD_HEADLESS: &str = "build with `cargo build --no-default-features --features \
                                sqlite-bundled,langs-all,deps-all -p fathom-cli`";

// ---------------------------------------------------------------------------
// The episode checks: untested new logic, and high-risk episodes without sign-off.
// ---------------------------------------------------------------------------

/// The two episode-shaped checks.
#[cfg(feature = "sqlite-bundled")]
fn gather_episodes(
    root: &Path,
    config: &str,
    _args: &CheckArgs,
    policy: &Policy,
    base_spec: &str,
    head_spec: &str,
    notices: &mut Vec<String>,
) -> Vec<CheckRun> {
    use fathom_store::spine::Store;

    let tests_volume = policy.severity(Check::Tests);
    let sign_volume = policy.severity(Check::SignOff);
    if tests_volume.is_none() && sign_volume.is_none() {
        return Vec::new();
    }
    let store_path = fathom_analyze::index::default_store_path(root);
    if !store_path.is_file() {
        let mut out = Vec::new();
        let reason = "no index yet, so the range's episodes are unknown";
        if let Some(volume) = tests_volume {
            out.push(blocked_run(Check::Tests, Some(volume), reason));
        }
        if let Some(volume) = sign_volume {
            out.push(blocked_run(Check::SignOff, Some(volume), reason));
        }
        return out;
    }
    let store = match Store::open(&store_path) {
        Ok(store) => store,
        Err(err) => {
            notices.push(format!("the store would not open: {err}"));
            return Vec::new();
        }
    };
    let repository = match fathom_ingest::repo::Repository::discover(root) {
        Ok(repository) => repository,
        Err(err) => {
            notices.push(format!("the repository would not open: {err}"));
            return Vec::new();
        }
    };
    let ids = match episode_ids(&store, &repository, base_spec, head_spec, policy) {
        Ok(ids) => ids,
        Err(err) => {
            notices.push(err);
            return Vec::new();
        }
    };
    let files = match store.episode_files(&ids) {
        Ok(files) => files,
        Err(err) => {
            notices.push(format!("the episodes' file rows would not read: {err}"));
            return Vec::new();
        }
    };
    let mut out = Vec::new();

    // Untested new logic: the added lines that read as logic, against the added lines that read as
    // tests, per episode. Both numbers come from the same index rows `fathom why` prints, so a reader can
    // check the arithmetic against a verb they already trust.
    if let Some(volume) = tests_volume {
        let mut findings = Vec::new();
        for id in &ids {
            let rows = files.by_episode.get(id).map(Vec::as_slice).unwrap_or(&[]);
            let mut logic: i64 = 0;
            let mut tests: i64 = 0;
            let mut largest: Option<(i64, String)> = None;
            for row in rows {
                let surface =
                    fathom_analyze::risk::classify(&row.path, &row.path_class, &row.noise_class);
                let added = row.add.max(0);
                match surface {
                    // `Source` *and* a language that can hold a function. `risk::classify` calls a
                    // `.toml`/`.json`/`.yaml` file source because for the risk score a changed manifest *is*
                    // code-shaped risk, and it is right to; here the question is "did logic arrive without
                    // tests", and 30 lines of `fathom.toml` is not logic nobody tested.
                    fathom_analyze::risk::Surface::Source if is_logic_path(&row.path) => {
                        logic += added;
                        if largest.as_ref().is_none_or(|(best, _)| added > *best) {
                            largest = Some((added, row.path.clone()));
                        }
                    }
                    fathom_analyze::risk::Surface::Test => tests += added,
                    _ => {}
                }
            }
            if logic < i64::try_from(policy.tests.min_new_lines).unwrap_or(i64::MAX) {
                continue;
            }
            let ratio = if logic == 0 {
                1.0
            } else {
                f64::from(u32::try_from(tests).unwrap_or(u32::MAX))
                    / f64::from(u32::try_from(logic).unwrap_or(u32::MAX))
            };
            if ratio >= policy.tests.min_tested_ratio {
                continue;
            }
            let finding = Finding::new(
                Check::Tests,
                "untested-logic",
                volume,
                format!(
                    "episode {} adds {logic} lines of new logic with {tests} test lines beside it, under \
                     the gate's floor of {ratio:.0}% tested",
                    short(id),
                    ratio = policy.tests.min_tested_ratio * 100.0,
                ),
            )
            .with_evidence(match largest {
                Some((_, path)) => Evidence::file(
                    path,
                    None,
                    format!("index:episode {}", short(id)),
                ),
                None => Evidence::pointer(format!("index:episode {}", short(id))),
            })
            .with_detail(format!(
                "policy: [check.tests] min_new_lines = {}, min_tested_ratio = {}",
                policy.tests.min_new_lines, policy.tests.min_tested_ratio
            ))
            .with_detail(
                "fix: add the suite this change should have arrived with, or say why it does not need \
                 one in the episode's own review note",
            );
            findings.push(finding);
        }
        let mut run = CheckRun::ran(Check::Tests, Some(volume), ids.len(), findings);
        let runs_note = if ids.len() == 1 {
            "1 episode".to_owned()
        } else {
            format!("{} episodes", ids.len())
        };
        run.notes.push(format!(
            "{runs_note} judged by added lines per surface, from the index rows `fathom why` uses"
        ));
        out.push(run);
    }

    // High-risk episodes without a sign-off. The score is `e4.4`'s, the marker is the store's, and the
    // pair is what the pipeline recipe insists on: never gate on the score alone.
    if let Some(volume) = sign_volume {
        let scored: Vec<String> = ids.iter().take(policy.risk_episode_cap).cloned().collect();
        if scored.len() < ids.len() {
            notices.push(format!(
                "`check.risk_episodes` is {}, so the newest {} of {} episodes were scored for \
                 sign-off",
                policy.risk_episode_cap,
                scored.len(),
                ids.len()
            ));
        }
        let marks = store.review_states().unwrap_or_default();
        // Filled on first use: reading and linking the ledger is a pass over its files, and a range with no
        // high-risk episode should not pay for it.
        let mut signed: Option<std::collections::BTreeSet<String>> = None;
        let weights = fathom_analyze::risk::weights::weights_from_toml(config).weights;
        let detector = fathom_analyze::detectors::DetectorPolicy::default();
        let mut findings = Vec::new();
        let mut caveats: Vec<String> = Vec::new();
        let mut unscored = 0_usize;
        for id in &scored {
            let read = fathom_analyze::risk::collect::read(
                &store,
                &repository,
                id,
                &detector,
                &fathom_analyze::risk::collect::Supply::nothing(),
            )
            .unwrap_or(None);
            let Some(read) = read else {
                unscored += 1;
                continue;
            };
            let score = fathom_analyze::risk::score::score(&read.vector, &weights);
            if !score.scored {
                unscored += 1;
                caveats.push(
                    score
                        .unscored
                        .clone()
                        .unwrap_or_else(|| "the episode could not be scored".to_owned()),
                );
                continue;
            }
            if score.score < policy.risk.threshold {
                continue;
            }
            let marker = marks.get(id).copied().unwrap_or_default();
            // The *ledger* is the sign-off; the index column is a cache of it that only catches up when the
            // ledger is refreshed. Reading the cache alone would fail a run that a human has genuinely signed
            // off, which is the worst kind of false positive: it punishes the review for happening.
            if marker == fathom_store::episode::ReviewState::Accepted
                || signed_off(root, &store, id, &mut signed)
            {
                continue;
            }
            findings.push(
                Finding::new(
                    Check::SignOff,
                    "high-risk-unsigned",
                    volume,
                    format!(
                        "episode {} scores {} of 100 (at or over the gate's {}) and its review marker \
                         is `{}`, not a sign-off",
                        short(id),
                        score.score,
                        policy.risk.threshold,
                        marker.as_str()
                    ),
                )
                .with_evidence(Evidence::pointer(format!("index:episode {}", short(id))))
                .with_detail(format!(
                    "score model {} over {}/{} features; top: {}",
                    score.model,
                    score.measured,
                    score.features,
                    score
                        .reasons()
                        .first()
                        .map(|term| format!("{} ({} pts)", term.feature.id(), term.points.0))
                        .unwrap_or_else(|| "none".to_owned())
                ))
                .with_detail("fix: read the episode in `fathom show` and sign it off with `fathom review sign` — the gate objects to the missing review, not to the size of the change")
            );
        }
        let mut run = CheckRun::ran(Check::SignOff, Some(volume), scored.len(), findings);
        if unscored > 0 {
            run.notes.push(format!(
                "{unscored} episode(s) could not be scored and are not claimed clean: {}",
                caveats
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "no reason recorded".to_owned())
            ));
        }
        out.push(run);
    }
    out
}

/// The episode checks where there is no store to read them from.
#[cfg(not(feature = "sqlite-bundled"))]
fn gather_episodes(
    _root: &Path,
    _config: &str,
    _args: &CheckArgs,
    policy: &Policy,
    _base_spec: &str,
    _head_spec: &str,
    _notices: &mut Vec<String>,
) -> Vec<CheckRun> {
    let mut out = Vec::new();
    if let Some(volume) = policy.severity(Check::Tests) {
        out.push(
            CheckRun::blocked(
                Check::Tests,
                Some(volume),
                "this binary was built without the `sqlite-bundled` feature, so it cannot read the index",
            )
            .with_remedy(REBUILD_HEADLESS),
        );
    }
    if let Some(volume) = policy.severity(Check::SignOff) {
        out.push(
            CheckRun::blocked(
                Check::SignOff,
                Some(volume),
                "this binary was built without the `sqlite-bundled` feature, so it cannot read the index",
            )
            .with_remedy(REBUILD_HEADLESS),
        );
    }
    out
}

/// Whether one episode has a live, accepted sign-off in `.fathom/reviews/ledger.md`.
///
/// The ledger is the committed record of *"a human looked at this"* (ADR-003), and it is what this reads
/// rather than the index's review column: that column is a mirror which only catches up when the ledger is
/// refreshed, and a gate that read only the mirror would fail a run in the window between a sign-off and the
/// next `fathom review` — punishing the review for having happened.
///
/// Freshness is the ledger linker's own verdict: a sign-off whose content moved since it was written is
/// `Stale`, and a stale sign-off signed a *different* change, so it does not count here. The whole ledger is
/// read once per run and cached, because the first high-risk episode pays for the pass and the ninth is free.
#[cfg(feature = "sqlite-bundled")]
fn signed_off(
    root: &Path,
    store: &fathom_store::spine::Store,
    episode: &str,
    cache: &mut Option<std::collections::BTreeSet<String>>,
) -> bool {
    use fathom_analyze::ledger::{EntryState, collect, link};
    if cache.is_none() {
        let (entries, _problems) = collect::read(root);
        let facts = collect::episode_facts(store).unwrap_or_default();
        let live: std::collections::BTreeSet<String> = entries
            .iter()
            .zip(link(&entries, &facts))
            .filter(|(entry, linked)| {
                entry.decision == fathom_analyze::ledger::Decision::Accepted
                    && linked.state == EntryState::Fresh
            })
            .filter_map(|(entry, linked)| {
                linked
                    .episode
                    .clone()
                    .or_else(|| (!entry.episode.is_empty()).then(|| entry.episode.clone()))
            })
            .collect();
        *cache = Some(live);
    }
    cache
        .as_ref()
        .is_some_and(|set| set.iter().any(|id| id == episode))
}

/// Whether a path is a file that can *hold* logic, as opposed to a data file `risk::classify` groups with
/// source for its own purposes.
///
/// The list is the languages a parser exists for plus the shell — the file types in which "you wrote a
/// function and did not test it" is a coherent sentence. Anything else (a manifest, a schema, a config, a
/// generated JSON blob) is counted under the surface the risk model gave it and not here.
fn is_logic_path(path: &str) -> bool {
    const CODE: &[&str] = &[
        ".rs", ".py", ".go", ".java", ".kt", ".kts", ".scala", ".rb", ".php", ".js", ".jsx",
        ".mjs", ".cjs", ".ts", ".tsx", ".cts", ".mts", ".c", ".h", ".cc", ".cpp", ".cxx", ".hpp",
        ".hh", ".cs", ".swift", ".m", ".mm", ".ex", ".exs", ".erl", ".hrl", ".hs", ".ml", ".mli",
        ".lua", ".pl", ".pm", ".r", ".R", ".sh", ".bash", ".zsh", ".fish", ".dart", ".vue",
        ".svelte", ".clj", ".cljs", ".edn", ".groovy", ".gradle", ".nim", ".zig", ".odin", ".sql",
        ".tf",
    ];
    let lower = path.to_ascii_lowercase();
    CODE.iter().any(|suffix| lower.ends_with(suffix))
}

/// The episodes in the range, newest first, capped by the policy.
#[cfg(feature = "sqlite-bundled")]
fn episode_ids(
    store: &fathom_store::spine::Store,
    repository: &fathom_ingest::repo::Repository,
    base_spec: &str,
    head_spec: &str,
    policy: &Policy,
) -> Result<Vec<String>, String> {
    let commits = commit_oids(repository, base_spec, head_spec)?;
    let mut ids: Vec<String> = Vec::new();
    for oid in commits {
        let row = store
            .episode_for_commit(&oid)
            .map_err(|err| format!("the store could not name the episode for {oid}: {err}"))?;
        if let Some(row) = row
            && !ids.iter().any(|existing| existing == &row.id)
        {
            ids.push(row.id);
        }
    }
    if ids.len() > policy.episode_cap {
        ids.truncate(policy.episode_cap);
    }
    Ok(ids)
}

/// The commits in `base..head`, as hex oids, newest first.
fn commit_oids(
    repository: &fathom_ingest::repo::Repository,
    base_spec: &str,
    head_spec: &str,
) -> Result<Vec<String>, String> {
    use fathom_ingest::graph::WalkOptions;
    let head =
        resolve(repository, head_spec).map_err(|err| format!("`--head {head_spec}` {err}"))?;
    let hides: Vec<fathom_ingest::oid::Oid> = match resolve_optional(repository, base_spec) {
        Some(base) => vec![base],
        None => Vec::new(),
    };
    let outcome = repository
        .log(
            WalkOptions::head()
                .starting_at([head])
                .hiding(hides)
                .first_parent(),
        )
        .finish()
        .map_err(|err| format!("the range {base_spec}..{head_spec} could not be walked: {err}"))?;
    Ok(outcome
        .entries
        .iter()
        .map(|entry| entry.oid.hex())
        .collect())
}

/// How many episodes the range holds, for the header. `Err` is a sentence, not a crash.
fn episodes_in(
    repository: &fathom_ingest::repo::Repository,
    base_spec: &str,
    head_spec: &str,
) -> Result<usize, String> {
    commit_oids(repository, base_spec, head_spec).map(|commits| commits.len())
}

// ---------------------------------------------------------------------------
// The api check: the surface of both revisions, diffed.
// ---------------------------------------------------------------------------

/// The public-API check.
fn gather_api(
    repository: &fathom_ingest::repo::Repository,
    config: &str,
    policy: &Policy,
    base_tree: Option<fathom_ingest::oid::Oid>,
    head_tree: Option<fathom_ingest::oid::Oid>,
    base_missing: Option<&str>,
    notices: &mut Vec<String>,
) -> Vec<CheckRun> {
    let Some(volume) = policy.severity(Check::Api) else {
        return Vec::new();
    };
    let Some(base_tree) = base_tree else {
        return vec![CheckRun::blocked(
            Check::Api,
            Some(volume),
            base_missing.unwrap_or("the base revision is not in this clone").to_owned(),
        )
        .with_remedy(
            "fetch the base (`actions/checkout` with `fetch-depth: 0`, or `git fetch --deepen 100`), \
             or point --base at a revision this clone has",
        )];
    };
    let Some(head_tree) = head_tree else {
        return vec![CheckRun::blocked(
            Check::Api,
            Some(volume),
            "the head revision has no tree",
        )];
    };
    let (api_policy, api_notices) =
        fathom_analyze::api::policy::ApiPolicy::from_repo_config_text(config);
    notices.extend(api_notices);
    let read = |tree| read_surface_inputs(repository, tree, policy.api_file_cap);
    let before_files = match read(base_tree) {
        Ok(files) => files,
        Err(why) => {
            notices.push(format!(
                "the base revision's files could not be read: {why}"
            ));
            return vec![CheckRun::blocked(
                Check::Api,
                Some(volume),
                "the base revision's files could not be read from this clone",
            )];
        }
    };
    let after_files = match read(head_tree) {
        Ok(files) => files,
        Err(why) => {
            notices.push(format!(
                "the head revision's files could not be read: {why}"
            ));
            return vec![CheckRun::blocked(
                Check::Api,
                Some(volume),
                "the head revision's files could not be read from this clone",
            )];
        }
    };
    let (before_sources, before_manifests) =
        fathom_analyze::api::extract::Inputs::split(&before_files);
    let (after_sources, after_manifests) =
        fathom_analyze::api::extract::Inputs::split(&after_files);
    let extract_policy = fathom_analyze::structure::ExtractPolicy::default();
    let before = fathom_analyze::api::extract::Inputs {
        sources: &before_sources,
        manifests: &before_manifests,
        cargo_metadata: None,
        rustdoc_json: None,
        policy: &api_policy,
        extract: extract_policy.clone(),
    };
    let after = fathom_analyze::api::extract::Inputs {
        sources: &after_sources,
        manifests: &after_manifests,
        cargo_metadata: None,
        rustdoc_json: None,
        policy: &api_policy,
        extract: extract_policy,
    };
    let answer = fathom_analyze::api::compare(&before, &after, None);
    let mut findings = Vec::new();
    let mut notes = Vec::new();
    let examined;
    match &answer {
        fathom_analyze::api::Answer::NoPublicApiChange { quick } => {
            notes.push(quick.sentence());
            // The fast path examined the whole candidate set and found nothing surface-bearing: one
            // "question" judged, and the note says which files it hashed. Zero here would read as a
            // blocked check, and it is the opposite — the cheapest complete answer there is.
            examined = 1;
        }
        fathom_analyze::api::Answer::Changed { diff, quick } => {
            examined = diff.cost.items_compared;
            notes.push(quick.sentence());
            notes.push(diff.cost.sentence());
            for change in diff.changes() {
                let severity = match change.class {
                    fathom_analyze::api::Class::Breaking => volume,
                    fathom_analyze::api::Class::Indeterminate => Severity::Note,
                    _ => continue,
                };
                let (path, line) = split_location(&change.location);
                findings.push(
                    Finding::new(
                        Check::Api,
                        match change.class {
                            fathom_analyze::api::Class::Breaking => "breaking",
                            _ => "indeterminate",
                        },
                        severity,
                        change.sentence(),
                    )
                    .with_evidence(Evidence::file(
                        path.unwrap_or(change.location.clone()),
                        line,
                        format!("api:package {}", change.package),
                    ))
                    .with_detail(format!("class: {}", change.class.as_str()))
                    .with_detail(format!("access: {:?}", change.access)),
                );
            }
            for gap in &diff.gaps {
                notes.push(format!("the read had a gap: {gap}"));
            }
            // The caveats are the part of the answer a reader needs in order to not over-read it:
            // "no breaking change" and "211 test files were declined from the surface" are one
            // sentence, not two findings. Capped, because a note list nobody reads is a footnote.
            for caveat in diff.caveats.iter().take(4) {
                notes.push(caveat.clone());
            }
            if diff.caveats.len() > 4 {
                notes.push(format!(
                    "… and {} further caveat(s) in the api report",
                    diff.caveats.len() - 4
                ));
            }
        }
    }
    let mut run = CheckRun::ran(Check::Api, Some(volume), examined, findings);
    run.notes = notes;
    if let Some(reason) = capped_note(&before_files, &after_files, policy.api_file_cap) {
        run.status = Status::Blocked {
            reason,
            remedy: Some(
                "raise `[check] api_files`, or narrow the tracked surface with `[api] ignore`"
                    .to_owned(),
            ),
        };
        run.findings.clear();
        run.total = 0;
    }
    let _ = &api_policy;
    vec![run]
}

/// The files of a tree the surface extractor could care about, as `(path, bytes)`.
fn read_surface_inputs(
    repository: &fathom_ingest::repo::Repository,
    tree: fathom_ingest::oid::Oid,
    cap: usize,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    use fathom_analyze::api::package::is_manifest;
    use fathom_analyze::structure::Language;
    use fathom_ingest::repo::BlobRead;

    let listing = repository
        .tree_files(tree, Some(cap.saturating_mul(4)))
        .map_err(|err| format!("the tree would not enumerate: {err}"))?;
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    for file in &listing.files {
        let path = file.path.to_string_lossy().replace('\\', "/");
        let known = Language::detect(&path).is_known();
        if !known && !is_manifest(&path) {
            continue;
        }
        if file.size > 2 * 1024 * 1024 {
            continue;
        }
        if out.len() >= cap {
            break;
        }
        match repository.blob(file.oid, 2 * 1024 * 1024) {
            Ok(BlobRead::Present { bytes, .. }) => out.push((path, bytes)),
            Ok(_) => {}
            Err(_) => {}
        }
    }
    Ok(out)
}

/// Whether the cap bit, in the check's own words.
fn capped_note(
    before: &[(String, Vec<u8>)],
    after: &[(String, Vec<u8>)],
    cap: usize,
) -> Option<String> {
    if before.len() >= cap || after.len() >= cap {
        Some(format!(
            "the surface read stopped at {cap} files per side, so a \"no breaking change\" answer here \
             would be an answer about a surface this run did not read"
        ))
    } else {
        None
    }
}

/// `src/lib.rs:41` → `("src/lib.rs", Some(41))`.
fn split_location(text: &str) -> (Option<String>, Option<u32>) {
    match text.rsplit_once(':') {
        Some((path, tail)) => {
            let line = tail.trim().parse::<u32>().ok();
            if line.is_some() {
                (Some(path.to_owned()), line)
            } else {
                (Some(text.to_owned()), None)
            }
        }
        None => (Some(text.to_owned()), None),
    }
}

// ---------------------------------------------------------------------------
// The dependency check.
// ---------------------------------------------------------------------------

/// The dependency-policy check.
fn gather_dependencies(
    repository: &fathom_ingest::repo::Repository,
    policy: &Policy,
    base_tree: Option<fathom_ingest::oid::Oid>,
    head_tree: Option<fathom_ingest::oid::Oid>,
    base_missing: Option<&str>,
) -> CheckRun {
    let Some(volume) = policy.severity(Check::Dependencies) else {
        return CheckRun::off(Check::Dependencies);
    };
    let (Some(base_tree), Some(head_tree)) = (base_tree, head_tree) else {
        return CheckRun::blocked(
            Check::Dependencies,
            Some(volume),
            base_missing
                .unwrap_or("this check needs both endpoints' trees in the clone")
                .to_owned(),
        )
        .with_remedy(
            "fetch the base (`actions/checkout` with `fetch-depth: 0`, or `git fetch --deepen 100`)",
        );
    };
    let episode = fathom_analyze::deps::dependency_episode_repository(
        repository,
        Some(base_tree),
        Some(head_tree),
        &fathom_analyze::deps::DependencyOptions::default(),
    );
    let model = match episode {
        Ok(episode) => episode.model,
        Err(err) => {
            return CheckRun::blocked(
                Check::Dependencies,
                Some(volume),
                format!("the dependency delta could not be computed: {err}"),
            );
        }
    };
    let mut findings: Vec<Finding> = Vec::new();
    let proof = |change: &fathom_analyze::deps::DepChange| match &change.proof {
        Some(proof) => Evidence::file(proof.path.clone(), None, format!("hunk:{}", proof.header)),
        None => Evidence::pointer(format!("manifest:{}", change.scope)),
    };
    for change in &model.changes {
        let direct = change.attribution == fathom_analyze::deps::Attribution::Direct;
        if change.kind == fathom_analyze::deps::DependencyKind::Added {
            if let Some(severity) = policy.dependencies.new_direct.filter(|_| direct) {
                findings.push(
                    Finding::new(
                        Check::Dependencies,
                        "new-direct",
                        severity,
                        format!(
                            "{} {} is a new direct dependency ({}: {})",
                            change.name,
                            change.after.clone().unwrap_or_default(),
                            change.ecosystem.as_str(),
                            change.detail
                        ),
                    )
                    .with_evidence(proof(change))
                    .with_detail(format!("scope: {}", change.scope)),
                );
            }
            if let Some(name) = forbidden(&policy.dependencies.forbid, &change.name) {
                findings.push(
                    Finding::new(
                        Check::Dependencies,
                        "forbidden-package",
                        Severity::Error,
                        format!(
                            "`{}` is refused by `[check.dependencies] forbid` and this range adds it \
                             ({}: {})",
                            change.name,
                            name,
                            change.ecosystem.as_str()
                        ),
                    )
                    .with_evidence(proof(change)),
                );
            }
        }
        let moved = matches!(
            change.kind,
            fathom_analyze::deps::DependencyKind::Upgraded
                | fathom_analyze::deps::DependencyKind::VersionChanged
        );
        if moved
            && direct
            && let Some(severity) = policy
                .dependencies
                .major_change
                .filter(|_| major_moved(change.before.as_deref(), change.after.as_deref()))
        {
            findings.push(
                Finding::new(
                    Check::Dependencies,
                    "major-version",
                    severity,
                    format!(
                        "{} moved a major version ({} → {})",
                        change.name,
                        change.before.clone().unwrap_or_default(),
                        change.after.clone().unwrap_or_default()
                    ),
                )
                .with_evidence(proof(change))
                .with_detail(format!(
                    "asked for by: {}",
                    if change.asked_by.is_empty() {
                        "the manifest directly".to_owned()
                    } else {
                        change.asked_by.join(" → ")
                    }
                )),
            );
        }
        if change.kind == fathom_analyze::deps::DependencyKind::Downgraded
            && direct
            && let Some(severity) = policy.dependencies.downgrade
        {
            findings.push(
                Finding::new(
                    Check::Dependencies,
                    "downgrade",
                    severity,
                    format!(
                        "{} moved backwards ({} → {})",
                        change.name,
                        change.before.clone().unwrap_or_default(),
                        change.after.clone().unwrap_or_default()
                    ),
                )
                .with_evidence(proof(change)),
            );
        }
    }
    if let Some(severity) = policy.dependencies.new_source {
        for source in &model.sources {
            findings.push(
                Finding::new(
                    Check::Dependencies,
                    "new-source",
                    severity,
                    format!(
                        "a new dependency source `{}` is used by this range ({})",
                        source.identity,
                        source
                            .via
                            .clone()
                            .unwrap_or_else(|| "no package named".to_owned())
                    ),
                )
                .with_evidence(match &source.proof {
                    Some(proof) => {
                        Evidence::file(proof.path.clone(), None, format!("hunk:{}", proof.header))
                    }
                    None => Evidence::pointer(format!("source:{}", source.kind)),
                }),
            );
        }
    }
    let mut run = CheckRun::ran(
        Check::Dependencies,
        Some(volume),
        model.changes.len(),
        findings,
    );
    run.notes.push(model.headline.clone());
    if !model.complete {
        run.notes.push(
            "one or both sides of the dependency comparison were read only partially".to_owned(),
        );
    }
    for note in model.notes {
        run.notes.push(note);
    }
    run
}

/// The first `forbid` entry a package name matches, exact or glob.
fn forbidden(patterns: &[String], name: &str) -> Option<String> {
    patterns
        .iter()
        .find(|pattern| {
            if pattern.contains('*') {
                glob_match(pattern, name)
            } else {
                pattern.as_str() == name
            }
        })
        .cloned()
}

/// A small glob matcher for `forbid` patterns, reusing the graph layer's own.
fn glob_match(pattern: &str, text: &str) -> bool {
    fathom_analyze::graph::layers::glob_match(pattern, text)
}

/// Whether a version moved across a major boundary, by the leading numeric component.
///
/// Deliberately crude and stated as such: the engines do not resolve versions (that is the package
/// manager's), so the gate compares the first dotted component of what the lockfile wrote. A scheme this
/// check cannot order (a commit sha, a wheel tag) is not a major move as far as it is concerned, and the
/// dependency row is still reported by whatever other rule it trips.
fn major_moved(before: Option<&str>, after: Option<&str>) -> bool {
    let major = |text: &str| {
        text.trim()
            .trim_start_matches('v')
            .split('.')
            .next()
            .and_then(|head| head.parse::<u64>().ok())
    };
    match (before, after) {
        (Some(from), Some(to)) => match (major(from), major(to)) {
            (Some(from), Some(to)) => from != to,
            _ => false,
        },
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Output.
// ---------------------------------------------------------------------------

/// `fathom check --explain`: the policy in force, in the terminal's own words, no analysis run.
fn explain(policy: &Policy) -> ExitCode {
    use crate::output;
    output::line(format_args!(
        "fathom check policy v{} · mode {} · fail at {}",
        policy.version,
        policy.mode.as_str(),
        policy.fail_on.map_or("never", |severity| severity.as_str())
    ));
    for check in Check::ALL {
        output::line(format_args!(
            "  {:<13} {:<8} {}",
            check.as_str(),
            policy.severity(check).map_or("off", Severity::as_str),
            check.title()
        ));
    }
    output::line(format_args!(
        "  tests: min_new_lines = {}, min_tested_ratio = {}",
        policy.tests.min_new_lines, policy.tests.min_tested_ratio
    ));
    output::line(format_args!(
        "  risk: threshold = {}, episodes = {}",
        policy.risk.threshold, policy.risk_episode_cap
    ));
    output::line(format_args!(
        "  dependencies: new_direct = {}, major_change = {}, new_source = {}, downgrade = {}, forbid = {}",
        policy
            .dependencies
            .new_direct
            .map_or("off", Severity::as_str),
        policy
            .dependencies
            .major_change
            .map_or("off", Severity::as_str),
        policy
            .dependencies
            .new_source
            .map_or("off", Severity::as_str),
        policy
            .dependencies
            .downgrade
            .map_or("off", Severity::as_str),
        if policy.dependencies.forbid.is_empty() {
            "none".to_owned()
        } else {
            policy.dependencies.forbid.join(", ")
        }
    ));
    output::line(format_args!(
        "  bounds: max_findings = {}, api_files = {}, max_episodes = {}",
        policy.max_findings, policy.api_file_cap, policy.episode_cap
    ));
    if !policy.require.is_empty() {
        let required: Vec<&str> = policy.require.iter().map(|check| check.as_str()).collect();
        output::line(format_args!("  required: {}", required.join(", ")));
    }
    if !policy.ignore.is_empty() {
        output::line(format_args!("  silenced: {}", policy.ignore.join(", ")));
    }
    for notice in &policy.notices {
        output::line(format_args!("  note: {notice}"));
    }
    ExitCode::Ok
}

/// Write the artefact in the chosen format, then the step annotations and the job summary.
fn emit(
    args: &CheckArgs,
    gate: &Gate,
    repository: &fathom_ingest::repo::Repository,
    head_oid: fathom_ingest::oid::Oid,
    format: &str,
) -> eyre::Result<ExitCode> {
    use fathom_analyze::check::{render, sarif};
    let head_hex = head_oid.hex();
    let remote = repository.config_string("remote.origin.url");
    let body = match format {
        "json" => gate.to_json().to_string(),
        "sarif" => {
            let context = SarifContext {
                fathom_version: env!("CARGO_PKG_VERSION"),
                revision: Some(head_hex.as_str()),
                repository_uri: remote.as_deref(),
            };
            sarif::log(gate, &context, &|path| {
                blob_exists(repository, head_oid, path)
            })
            .to_string()
        }
        "github" => render::annotations(gate).join("\n"),
        "markdown" => render::markdown(gate),
        _ => render::human(gate, gate.policy.max_findings).join("\n"),
    };
    if let Some(path) = &args.out {
        if let Some(parent) = Path::new(path).parent()
            && !parent.as_os_str().is_empty()
            && !parent.exists()
        {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(path, format!("{body}\n")).map_err(|err| {
            Pending::new(
                ExitCode::Error,
                format!("`{path}` could not be written: {err}"),
                Some("the directory must exist or be creatable".to_owned()),
            )
        })?;
        crate::output::problem(format_args!("fathom check · wrote {path} ({format})"));
    } else {
        crate::output::line(format_args!("{body}"));
    }

    // Step annotations, alongside the human log: a CI log wants the prose *and* the markers GitHub can
    // attach to the diff. They are **not** emitted after a machine format — a JSON document with a
    // `::error` line stapled to the end is not parseable, and a SARIF file is uploaded by a step that has no
    // business reading past its first byte. `--no-annotations` is the switch for the log case too.
    if matches!(format, "human" | "github") && !args.no_annotations && format != "github" {
        for line in render::annotations(gate) {
            crate::output::line(format_args!("{line}"));
        }
    }
    write_summary(args, gate)?;
    Ok(gate.exit_code())
}

/// Whether a path is present in the head tree — the SARIF location filter.
fn blob_exists(
    repository: &fathom_ingest::repo::Repository,
    head_oid: fathom_ingest::oid::Oid,
    path: &str,
) -> bool {
    let Some(tree) = tree_of(repository, head_oid) else {
        return false;
    };
    repository
        .blob_of(tree, &fathom_ingest::paths::RepoPath::new(path))
        .ok()
        .flatten()
        .is_some()
}

/// The markdown job summary: `--summary <path>`, else `$GITHUB_STEP_SUMMARY` when the runner set one.
fn write_summary(args: &CheckArgs, gate: &Gate) -> eyre::Result<()> {
    use fathom_analyze::check::render;
    if args.no_summary {
        return Ok(());
    }
    let target = args.summary.clone().or_else(|| {
        std::env::var("GITHUB_STEP_SUMMARY")
            .ok()
            .filter(|text| !text.is_empty())
    });
    let Some(path) = target else {
        return Ok(());
    };
    let body = render::markdown(gate);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|err| {
            Pending::new(
                ExitCode::Error,
                format!("the job summary `{path}` could not be opened: {err}"),
                Some("pass --summary <path>, or unset GITHUB_STEP_SUMMARY".to_owned()),
            )
        })?;
    file.write_all(body.as_bytes()).map_err(|err| {
        Pending::new(
            ExitCode::Error,
            format!("the job summary `{path}` could not be written: {err}"),
            None,
        )
    })?;
    Ok(())
}
