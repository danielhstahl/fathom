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
    let snapshot = state::gather(root_path, deep);
    let build = build_section();
    let config = config_section(&redactor);
    let layout = fathom_layout(&redactor);
    let watch = watch_section(root_path);
    let consent = consent_section();
    let code = snapshot.worst_code();

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
            "network_features": consent,
            "privacy": privacy,
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
    tracing::debug!(features = ?features::compiled(), "build report");

    if privacy {
        privacy_table();
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

/// The `--privacy` table, unchanged from the version `e0.7` shipped: the consent keys are a published
/// contract (`fathom_core::privacy`) and this is where a user checks which half of the double gate is
/// holding (ADR-002 §6.3).
fn privacy_table() {
    let rows = features::privacy_table();
    output::line(format_args!(
        "{:<20} {:<16} {:<10} {:<28} {}",
        "FEATURE", "OWNER", "COMPILED", "CONSENT KEY", "OWNER TICKET"
    ));
    for row in &rows {
        output::line(format_args!(
            "{:<20} {:<16} {:<10} {:<28} {}",
            row.feature,
            row.owner,
            if row.compiled_in { "yes" } else { "no" },
            row.consent_key,
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

/// The config layers `fathom` would read, described but never read.
///
/// The point of printing a *digest-shaped* summary rather than the contents is that `fathom.toml` is where
/// a user may have put a GitHub token or an LLM key: the report needs to say "the file you think is in
/// effect is 2 148 bytes and changed at 09:12", and it never needs the bytes.
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
    features::privacy_table()
        .iter()
        .map(|row| {
            json!({
                "feature": row.feature,
                "owner": row.owner,
                "compiled_in": row.compiled_in,
                "consent_key": row.consent_key,
                "ticket": row.ticket,
            })
        })
        .collect()
}

fn yes_no(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "yes",
        Some(false) => "no",
        None => "unknown",
    }
}
