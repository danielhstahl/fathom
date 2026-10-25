//! `fathom doctor`: environment and repository diagnostics, each with the thing to do about it.
//!
//! The contract has three parts, and the second two are the ones that make it more than a `--version`
//! with extra steps:
//!
//! * **It says what this binary is.** Version, resolved feature set, SQLite provenance and its capability
//!   probe, OS and arch, terminal. A bug report that cannot say what was compiled is a bug report that gets
//!   closed with "works for me" (ADR-002 §6.4).
//! * **It is safe to attach.** Every path is repository-relative or `~`-relative, every URL's userinfo is
//!   gone, and no config *value* is ever printed — only its path, size and modification time. See
//!   [`crate::sanitize`].
//! * **Every finding names a remedy.** And the exit code is *off by default*: `fathom doctor` is a command
//!   a user runs when something is wrong, and a diagnostic that exits 7 makes `fathom doctor` itself the
//!   thing that is wrong in a script. `--strict` turns the findings into the published exit code for CI and
//!   for a pre-flight check; the code each finding carries is printed either way.
//! * **A config key that configures nothing is named here** (the `ignored_config_keys` reader,
//!   `reco-gna`). Every verb that reads a weight says so at the bottom of its own output, which is right for
//!   a score and useless for "why did my ranking not move" asked on its own; doctor is the one verb whose
//!   whole job is the configuration, and `--strict` is the form that refuses it in CI.
//!
//! The repository, index and cache sections are [`crate::state`]'s, so `fathom doctor` and `fathom stats`
//! cannot disagree about whether the index is healthy — which is the first question a triager asks when
//! they are pasted next to each other.

use crate::features;
use crate::output;
use crate::query::DOCTOR_CONTRACT;
use crate::sanitize::Redactor;
use crate::state;
use fathom_core::exit::ExitCode;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// `fathom doctor`, in both renderings.
pub fn doctor(
    root: &str,
    privacy: bool,
    as_json: bool,
    deep: bool,
    strict: bool,
) -> eyre::Result<ExitCode> {
    let root_path = Path::new(root);
    let redactor = Redactor::for_root(root_path);
    // Doctor is the verb ADR-004 §9.6 gives the whole-database checks to, and it is the only one that asks
    // for them: `fathom doctor` runs `quick_check`, `--deep` adds `integrity_check`, and the read verbs
    // (`stats`, `show`, `analyze`, …) run the checks that do not cost the size of the repository.
    let mut snapshot = state::gather_with(
        root_path,
        if deep {
            state::Check::Full
        } else {
            state::Check::Quick
        },
    );
    let build = build_section();
    let config = config_section(&redactor);
    // One finding per key the repository's own `fathom.toml` sets and this build cannot use. Untidy rather
    // than degraded: every answer the binary gives is still the answer, and what the finding asks for is a
    // line edited — which is exactly the class `--strict` exists to fail on and `fathom stats` has no
    // business exiting for. Printed once, here, with its remedy: every verb that reads a weight already
    // says the same sentence beside its own output, and this is the copy for the person who has no score
    // on the screen and only wants to know why the ranking did not move.
    let config_text =
        std::fs::read_to_string(redactor.root.join("fathom.toml")).unwrap_or_default();
    for notice in ignored_config_keys(&config_text) {
        snapshot.findings.push(state::Finding::untidy(
            "config_ignored_key",
            "config",
            redactor.text(&notice),
            REMEDY_IGNORED_CONFIG_KEY,
            ExitCode::ConfigInvalid,
        ));
    }
    let layout = fathom_layout(&redactor);
    let watch = watch_section(root_path);
    let hooks_stream = crate::harness::doctor_json(root_path);
    let frames = crate::verbs::frame_json(root_path);
    let consent = consent_section();
    // The code `--strict` will leave with, printed either way. It counts the untidy findings too — a store
    // holding rows for commits that no longer exist is not a broken index, but it is a thing to fix, and
    // "fix this or say why not" is exactly what a pre-flight check is for.
    let code = snapshot.worst_code_strict();

    if as_json {
        let report = json!({
            "contract": DOCTOR_CONTRACT,
            "version": features::VERSION,
            "features": features::compiled(),
            "schema_version": fathom_analyze::insight::status().schema_version,
            "views_ready": fathom_analyze::insight::status().views_ready,
            "build": build,
            "sqlite": sqlite_section(),
            "repository": snapshot.repository,
            "index": snapshot.index,
            "cache": snapshot.cache,
            "config": config,
            "fathom_dir": layout,
            "watch": watch,
            "hooks": hooks_stream,
            "frames": frames,
            "network_features": consent,
            "privacy": privacy,
            "privacy_posture": privacy_posture(&redactor),
            "strict": strict,
            "exit_code": code.as_i32(),
            "exit_name": code.name(),
            "findings": snapshot.findings_json(),
        });
        output::line(format_args!("{report}"));
        return Ok(if strict { code } else { ExitCode::Ok });
    }

    output::line(format_args!("fathom {}", features::version_line()));
    let status = fathom_analyze::insight::status();
    output::line(format_args!(
        "review model: schema {}, views ready: {}",
        status.schema_version, status.views_ready
    ));
    for line in build_lines(&build) {
        output::line(format_args!("{line}"));
    }
    for line in sqlite_lines() {
        output::line(format_args!("{line}"));
    }
    for line in crate::query::stats_text(&snapshot) {
        output::line(format_args!("{line}"));
    }
    for line in config_lines(&config) {
        output::line(format_args!("{line}"));
    }
    for line in layout_lines(&layout) {
        output::line(format_args!("{line}"));
    }
    for line in crate::verbs::watch_lines(root_path) {
        output::line(format_args!("{line}"));
    }
    for line in crate::harness::doctor_lines(root_path) {
        output::line(format_args!("{line}"));
    }
    for line in crate::verbs::frame_lines(root_path) {
        output::line(format_args!("{line}"));
    }
    tracing::debug!(features = ?features::compiled(), "build report");

    if privacy {
        privacy_table(&redactor, &layout);
    } else {
        output::line(format_args!(
            "pass --privacy for the network-feature table, or --json for the same report as data"
        ));
    }
    for line in snapshot.finding_lines() {
        output::line(format_args!("{line}"));
    }
    output::line(format_args!(
        "exit {} — {}",
        code.as_i32(),
        if code == ExitCode::Ok {
            "nothing found".to_owned()
        } else if strict {
            "this is the code this run exits with".to_owned()
        } else {
            "not an error unless --strict is passed".to_owned()
        }
    ));
    Ok(if strict { code } else { ExitCode::Ok })
}

/// The `--privacy` report: what this binary can reach, what the user has consented to, what it keeps
/// locally, and the claim about what never leaves the machine (`e6.8`, ADR-002 §6.3).
///
/// One row per gate — compiled in, consented to — plus the verdict half the gates add up to, and then the
/// two questions a person actually has after reading a table: *where do I write this if I want it on*,
/// and *what is this keeping about me*. The consent keys themselves are a published contract
/// (`fathom_core::privacy`), so this is where a user checks which half of the double gate is holding
/// rather than finding out from an error message.
fn privacy_table(redactor: &Redactor, layout: &Value) {
    let rows = features::privacy_table();
    let consent = features::consent_states();
    output::line(format_args!(
        "privacy: what this build can reach, and what it will not do unasked"
    ));
    output::line(format_args!(
        "{:<20} {:<16} {:<9} {:<8} {:<8} {:<28} {:<8} {}",
        "FEATURE", "OWNER", "COMPILED", "CONSENT", "STATE", "CONSENT KEY", "SET IN", "OWNER TICKET"
    ));
    for row in &rows {
        let state = consent
            .iter()
            .find(|candidate| candidate.key == row.consent_key);
        let granted = state.is_some_and(|candidate| candidate.granted);
        // Two gates, two columns, and a verdict that names which half is holding (ADR-002 §6.3). A row
        // that reads `no / yes` is a real state — consent left on from a build that no longer compiles
        // the feature — and it is `off`, because code that is not in the binary cannot be switched on.
        let verdict = match (row.compiled_in, granted) {
            (true, true) => "ON",
            (true, false) => "waiting",
            (false, _) => "off",
        };
        output::line(format_args!(
            "{:<20} {:<16} {:<9} {:<8} {:<8} {:<28} {:<8} {}",
            row.feature,
            row.owner,
            if row.compiled_in { "yes" } else { "no" },
            if granted { "yes" } else { "no" },
            verdict,
            row.consent_key,
            state.map_or("default", |state| state.layer),
            row.ticket
        ));
    }
    let net_compiled: Vec<&str> = rows
        .iter()
        .filter(|row| row.compiled_in)
        .map(|row| row.feature)
        .collect();
    output::line(format_args!(
        "outbound features compiled in: {}",
        if net_compiled.is_empty() {
            "none — no HTTP client and no listener in this binary (ADR-002 R6)".to_owned()
        } else {
            net_compiled.join(", ")
        }
    ));
    // "consent is off" is only actionable if the user knows which file to open, and `privacy.*` is
    // user-scoped, so the answer is never the repository's own file (ADR-001 §4.6).
    let files: Vec<String> = features::consent_files()
        .iter()
        .map(|(layer, path)| {
            format!(
                "{layer}={} ({})",
                redactor.path(path),
                if path.exists() { "exists" } else { "absent" }
            )
        })
        .collect();
    output::line(format_args!(
        "consent is user-scoped: set `privacy.<key> = true` in {} — a repository's own fathom.toml \
         cannot grant it, because a clone must not consent for you",
        files.join(", ")
    ));
    let attempted = features::consent_the_repository_tried(&redactor.root);
    if !attempted.is_empty() {
        output::line(format_args!(
            "ignored: this repository's fathom.toml sets {} — that is read and discarded; set it in \
             your own config if you meant it",
            attempted.join(", ")
        ));
    }
    output::line(format_args!(
        "local records: {} ({}) — read them with `fathom stats` or `fathom doctor --json`, write them \
         out with `fathom export`, delete them by removing the directory",
        layout["path"].as_str().unwrap_or(".fathom"),
        if layout["present"].as_bool() == Some(true) {
            format!(
                "{} file(s), {} B on disk",
                layout["entries"].as_array().map(Vec::len).unwrap_or(0),
                layout["bytes"].as_u64().unwrap_or(0)
            )
        } else {
            "nothing there yet".to_owned()
        }
    ));
    output::line(format_args!(
        "usage metrics: nothing leaves this machine — {}",
        features::LOCAL_ONLY_SENTENCE
    ));
}

/// What this process is: platform, arch, terminal, and the instant the report describes.
fn build_section() -> Value {
    json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "family": std::env::consts::FAMILY,
        "libc": if cfg!(target_os = "linux") { "gnu-or-musl (unprobed)" } else { "n/a" },
        "terminal": {
            "name": std::env::var("TERM").unwrap_or_else(|_| "unset".to_owned()),
            "tty": std::io::IsTerminal::is_terminal(&std::io::stdout()),
            "columns": std::env::var("COLUMNS").ok().and_then(|text| text.parse::<u16>().ok()),
            "no_color": std::env::var_os("NO_COLOR").is_some(),
            // OSC-8 (`e5.8`): the same question the printers asked, answered where a bug report can
            // quote it. Whether a path was clickable is the first thing a "links do not work" report
            // needs, and there is no other place this is written down.
            "hyperlinks": hyperlinks(),
        },
        "now_utc": crate::clock::now_millis(),
    })
}

fn build_lines(build: &Value) -> Vec<String> {
    vec![format!(
        "build: {} {} ({}), terminal {} tty {}, now {}",
        build["os"].as_str().unwrap_or("?"),
        build["arch"].as_str().unwrap_or("?"),
        build["libc"].as_str().unwrap_or("?"),
        build["terminal"]["name"].as_str().unwrap_or("unset"),
        yes_no(build["terminal"]["tty"].as_bool()),
        build["now_utc"]
            .as_i64()
            .map_or_else(|| "?".to_owned(), crate::query::format_utc),
    )]
}

/// The OSC-8 answer, as the two facts and one sentence a report needs (`e5.8`).
///
/// Whether a path was clickable is the first thing a "the links do not work" report has to answer, and
/// until now nothing wrote the decision down anywhere a bug report could quote.
fn hyperlinks() -> Value {
    let links = crate::links::Printer::for_root(std::path::Path::new("."));
    json!({
        "emit": links.enabled(),
        "advertised": links.advertised(),
        "why": links.why(),
    })
}

/// The SQLite this binary carries, with ADR-004 §4.0's capability probe.
fn sqlite_section() -> Value {
    let built = crate::verbs::sqlite_provenance_string();
    #[cfg(feature = "sqlite-bundled")]
    {
        match fathom_store::spine::Store::open_in_memory()
            .and_then(|store| store.sqlite_provenance())
        {
            Ok(provenance) => json!({
                "built": built,
                "version": provenance.version,
                "compile_options": provenance.compile_options,
                "journal_mode": provenance.journal_mode,
                "page_size": provenance.page_size,
                "encoding": provenance.encoding,
                "capabilities": provenance.capabilities,
            }),
            Err(err) => json!({ "built": built, "error": err.to_string() }),
        }
    }
    #[cfg(not(feature = "sqlite-bundled"))]
    {
        json!({ "built": built })
    }
}

fn sqlite_lines() -> Vec<String> {
    let section = sqlite_section();
    let mut out = vec![format!(
        "sqlite: {}",
        crate::verbs::sqlite_provenance_string()
    )];
    if let Some(version) = section["version"].as_str() {
        let capabilities = section["capabilities"]
            .as_object()
            .map(|options| {
                options
                    .iter()
                    .map(|(name, ok)| format!("{name} {}", yes_no(ok.as_bool())))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        out.push(format!(
            "        {version}, journal {}, page {} B, probe: {}",
            section["journal_mode"].as_str().unwrap_or("?"),
            section["page_size"].as_u64().unwrap_or(0),
            capabilities
        ));
    }
    out
}

/// The remedy for a key that was read and could not be honoured (`reco-gna`).
///
/// It names the verb that prints the vocabulary, because the twelve features are the vector's, not this
/// file's, and a remedy that pasted them here would be a second copy of a list that already has a home.
const REMEDY_IGNORED_CONFIG_KEY: &str = "rename the key to one this build reads, or delete the line \
     (`fathom why --weights` prints every key `[risk.weights]` may name, with its default); the run that \
     ignored it prints the same sentence beside the score";

/// The config layers `fathom` would read, described but never read.
///
/// The point of printing a *digest-shaped* summary rather than the contents is that `fathom.toml` is where
/// a user may have put a GitHub token or an LLM key: the report needs to say "the file you think is in
/// effect is 2 148 bytes and changed at 09:12", and it never needs the bytes. What the file *means* is a
/// finding, not a field here — see `ignored_config_keys` — and a finding is a sentence about a key, never
/// a value.
fn config_section(redactor: &Redactor) -> Value {
    let layers: Vec<Value> = config_candidates(redactor)
        .into_iter()
        .map(|(layer, path)| file_fact(&layer, &path, redactor))
        .collect();
    json!({
        "layers": layers,
        "environment": std::env::vars_os()
            .filter(|(key, _value)| key.to_string_lossy().starts_with("FATHOM_"))
            .map(|(key, _value)| key.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
    })
}

/// Which keys of the repository's `fathom.toml` this build read and could not use (`reco-gna`).
///
/// A key that configures nothing is a decision recorded in the wrong drawer. Whoever wrote it believes the
/// ranking moved; the next person to "raise the dependency weight" types into the dead name and changes
/// nothing, and nothing in the repository ever says so. `fathom why` has always said it, at the bottom of
/// every score it printed — which is the right place for a score and no help at all to the reviewer who
/// asks the question on its own. This is the same sentence, from the same parser, in the verb whose whole
/// subject is the configuration.
///
/// `[risk.weights]` and `risk.tau` are what answers here, because that is the table the config layer
/// cannot police: the key spec declares it as the open map `risk.weights.<weight>`, so any name under it
/// resolves happily and only the vector knows what a real one is. The declared keys are policed where they
/// are declared (`python3 -m recommender.config lint` names the nearest key for a typo) and the budget keys
/// are already said in this report's `memory` section. `[detectors]` is deliberately *not* collected: its
/// reader mixes refusals with narration about switches that worked — "`detectors.disable` turned `x` off
/// for this repository" — and a list this verb turns into a `--strict` failure may only hold refusals.
/// Separating the two is a change to `e6.7`'s policy reader, not to this file.
///
/// Text in, not a path, for the same reason [`fathom_analyze::risk::weights::weights_from_toml`] takes
/// text: ADR-002 §4.5 gives only this crate the right to open the file, and a reader that takes text can
/// be asked what a file means without being the repository it describes.
fn ignored_config_keys(text: &str) -> Vec<String> {
    fathom_analyze::risk::weights::weights_from_toml(text).notices
}

/// The layers ADR-002 §5's config order names, in precedence order.
fn config_candidates(redactor: &Redactor) -> Vec<(String, PathBuf)> {
    let mut out = vec![("repo".to_owned(), redactor.root.join("fathom.toml"))];
    if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
        out.push((
            "xdg".to_owned(),
            PathBuf::from(config_home)
                .join("fathom")
                .join("config.toml"),
        ));
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        out.push((
            "user".to_owned(),
            home.join(".config").join("fathom").join("config.toml"),
        ));
        out.push((
            "legacy".to_owned(),
            home.join(".fathom").join("config.toml"),
        ));
    }
    out.push((
        "store".to_owned(),
        redactor
            .root
            .join(".fathom")
            .join("config")
            .join("effective.toml"),
    ));
    out
}

fn file_fact(layer: &str, path: &Path, redactor: &Redactor) -> Value {
    let stat = std::fs::metadata(path);
    json!({
        "layer": layer,
        "path": redactor.path(path),
        "present": stat.is_ok(),
        "size_bytes": stat.as_ref().map(|meta| meta.len()).unwrap_or(0),
        "modified_utc": stat
            .as_ref()
            .ok()
            .and_then(|meta| meta.modified().ok())
            .map(|time| {
                time.duration_since(std::time::UNIX_EPOCH)
                    .map(|since| since.as_millis() as i64)
                    .unwrap_or(0)
            }),
    })
}

/// The `config` section as lines: one row per layer that exists, one for the `FATHOM_*` variables set.
///
/// What the file *means* is not repeated here — an ignored key is a finding, printed with its remedy at the
/// foot of this report, and a sentence printed twice is a sentence that drifts.
fn config_lines(config: &Value) -> Vec<String> {
    let mut out = vec!["config".to_owned()];
    for layer in config["layers"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        if layer["present"].as_bool() != Some(true) {
            continue;
        }
        out.push(format!(
            "  {:<8} {} ({} B, {})",
            layer["layer"].as_str().unwrap_or("?"),
            layer["path"].as_str().unwrap_or("?"),
            layer["size_bytes"].as_u64().unwrap_or(0),
            layer["modified_utc"]
                .as_i64()
                .map_or_else(|| "?".to_owned(), crate::query::format_utc),
        ));
    }
    let names = config["environment"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if !names.is_empty() {
        let list = names
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        out.push(format!(
            "  env       set: {list} (names only; values are never printed)"
        ));
    }
    out
}

/// What lives under `.fathom/`, which is the directory a bug report is usually about.
fn fathom_layout(redactor: &Redactor) -> Value {
    let dir = redactor.root.join(".fathom");
    let mut entries = Vec::new();
    let mut total = 0_u64;
    if let Ok(read) = std::fs::read_dir(&dir) {
        for entry in read.flatten() {
            let path = entry.path();
            let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
            if path.is_file() {
                total += size;
            }
            entries.push((redactor.path(&path), size, path.is_dir()));
        }
    }
    entries.sort();
    json!({
        "path": redactor.path(&dir),
        "present": dir.exists(),
        "bytes": total,
        "entries": entries
            .iter()
            .map(|(path, size, is_dir)| json!({"path": path, "size_bytes": size, "dir": is_dir}))
            .collect::<Vec<_>>(),
    })
}

fn layout_lines(layout: &Value) -> Vec<String> {
    if layout["present"].as_bool() != Some(true) {
        return vec![format!(
            ".fathom   {} — nothing there yet",
            layout["path"].as_str().unwrap_or(".fathom")
        )];
    }
    vec![format!(
        ".fathom   {} ({} file(s), {} B)",
        layout["path"].as_str().unwrap_or(".fathom"),
        layout["entries"].as_array().map(Vec::len).unwrap_or(0),
        layout["bytes"].as_u64().unwrap_or(0)
    )]
}

/// The watcher section, delegated to `e1.9`'s own reader so the heartbeat is interpreted in exactly one
/// place, and to `verbs` for the case where the watcher was not compiled in at all.
fn watch_section(root: &Path) -> Value {
    crate::verbs::watch_report_json(root)
}

/// The consent table as data — the same rows as `--privacy`, because a machine reading `doctor --json` has
/// to be able to reach the same conclusion a human reaches from the table.
fn consent_section() -> Vec<Value> {
    let consent = features::consent_states();
    features::privacy_table()
        .iter()
        .map(|row| {
            let state = consent
                .iter()
                .find(|candidate| candidate.key == row.consent_key);
            json!({
                "feature": row.feature,
                "owner": row.owner,
                "compiled_in": row.compiled_in,
                "consent_key": row.consent_key,
                // Both halves of ADR-002 §6.3, because `compiled_in: true, consent: false` is the state a
                // user who built with `--features net-llm` is in and needs named.
                "consent": state.is_some_and(|state| state.granted),
                "consent_layer": state.map_or("default", |state| state.layer),
                "ticket": row.ticket,
            })
        })
        .collect()
}

/// The posture as data: what a reader needs to conclude "nothing left this machine" without reading the
/// page (`e6.8`). The verdict is derived, not asserted — it is the list of files consent could have come
/// from, the keys a repository tried to grant, and what is actually on disk.
fn privacy_posture(redactor: &Redactor) -> Value {
    json!({
        "outbound_compiled_in": features::privacy_table()
            .iter()
            .filter(|row| row.compiled_in)
            .map(|row| row.feature)
            .collect::<Vec<_>>(),
        "consent_scope": "user",
        "consent_files": features::consent_files()
            .iter()
            .map(|(layer, path)| json!({"layer": layer, "path": redactor.path(path),
                                        "present": path.exists()}))
            .collect::<Vec<_>>(),
        "consent_ignored_from_repository": features::consent_the_repository_tried(&redactor.root),
        "telemetry": "none",
        "local_only": features::LOCAL_ONLY_SENTENCE,
        "proven_by": "tools/check_privacy.py (offline run of this binary, with the egress trapped)",
    })
}

fn yes_no(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "yes",
        Some(false) => "no",
        None => "unknown",
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::ignored_config_keys;

    #[test]
    fn a_weight_named_for_a_feature_this_vector_does_not_have_is_refused() {
        // The report this came from: a key named for a feature this vector never had, still sitting in the
        // file that "sets" it. The notice has to name the key, say it is not a feature, and print the list
        // the writer has to choose from, or the sentence cannot be acted on.
        let notices = ignored_config_keys("[risk.weights]\nnew_dependency = 0.2\n");
        assert_eq!(notices.len(), 1, "{notices:?}");
        assert!(
            notices[0].starts_with("risk.weights.new_dependency"),
            "{}",
            notices[0]
        );
        assert!(notices[0].contains("not a feature"), "{}", notices[0]);
        for feature in ["findings", "dependency", "authorship"] {
            assert!(notices[0].contains(feature), "{}", notices[0]);
        }
    }

    #[test]
    fn a_file_that_configures_only_real_keys_says_nothing() {
        // The quiet case, and the one that has to stay quiet: a repository that moves three weights and
        // the threshold is not a defect, and a report that cried at it would be trained away in a week.
        assert!(
            ignored_config_keys("[risk]\ntau = 55\n[risk.weights]\nsize = 0.3\ntest_ratio = 0.3\n")
                .is_empty()
        );
        assert!(ignored_config_keys("").is_empty());
        assert!(ignored_config_keys("[noise]\nglobs = [\"**/dist/**\"]\n").is_empty());
    }

    #[test]
    fn a_value_that_is_not_a_number_is_refused_with_its_key() {
        let notices = ignored_config_keys("[risk.weights]\nsize = \"wide\"\n");
        assert_eq!(notices.len(), 1, "{notices:?}");
        assert!(notices[0].contains("risk.weights.size"), "{}", notices[0]);
        assert!(notices[0].contains("not a number"), "{}", notices[0]);
    }

    #[test]
    fn this_repositorys_own_fathom_toml_configures_nothing_that_does_not_exist() {
        // The checker that did not exist when W43-05 was filed. This file is the product's own config and
        // the corpus `fathom why` is run over, and it carried a weight key no vector of this tool has ever
        // had, because the key spec lets any name under `[risk.weights]` through and nothing else ever
        // looked. The same reader `fathom doctor` runs is what refuses it here, so the day a rename orphans
        // a key again the red is in `cargo test --workspace`, not in the last three lines of a score.
        let text =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fathom.toml"))
                .expect("this repository's own fathom.toml");
        let notices = ignored_config_keys(&text);
        assert!(notices.is_empty(), "{notices:?}");
    }
}
