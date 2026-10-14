//! The verbs: parse, dispatch, and leave with a published exit code (ADR-002 §4.3, §7.1, §7.3).
//!
//! Each verb here is a *stub that knows its own name*, and that is the design rather than a placeholder
//! for it: the verb surface, the refusal it returns before it has an answer, and the exit code it leaves
//! with are the contract `e1.10` fills in. A stub returns a typed failure and never a panic (§7.2, §9
//! rule 2), and the failure names the ticket that owns the work, so "not yet" is a pointer and not a
//! wall.

use crate::features;
use crate::output;
use clap::{Args, Parser, Subcommand};
use fathom_core::error::Diagnosable;
use fathom_core::exit::ExitCode;

/// The whole command line.
#[derive(Parser, Debug)]
#[command(
    name = "fathom",
    about = "Review an agent-driven repository before it merges.",
    version,
    propagate_version = true
)]
pub struct Cli {
    /// The verb to run. With no verb, fathom opens the terminal UI — if it was compiled in.
    #[command(subcommand)]
    pub verb: Option<Verb>,
}

/// The verbs fathom answers to. ADR-005 §6 owns the full list and `e1.10` ships it; the ones here are
/// the ones the exit-code contract and the feature gates need in order to be demonstrable at all.
#[derive(Subcommand, Debug)]
pub enum Verb {
    /// Index the repository so every other verb has something to read
    Index(IndexArgs),
    /// Show one episode in full
    Show(ShowArgs),
    /// Report what the index holds
    Stats,
    /// Run the checks CI cares about; findings above the threshold exit 7
    Check,
    /// Describe this build, its features and its privacy posture
    Doctor(DoctorArgs),
    /// Write a review artefact for a pull request or an audit
    Export(ExportArgs),
    /// Speak MCP to a coding agent
    Mcp(McpArgs),
    /// Watch the repository and index as it changes
    Watch,
}

/// `fathom index`
#[derive(Args, Debug)]
pub struct IndexArgs {
    /// The repository to index.
    #[arg(default_value = ".")]
    pub root: String,
}

/// `fathom show`
#[derive(Args, Debug)]
pub struct ShowArgs {
    /// The episode to show.
    pub episode: String,
}

/// `fathom doctor`
#[derive(Args, Debug)]
pub struct DoctorArgs {
    /// Print the network-feature and consent table.
    #[arg(long)]
    pub privacy: bool,
    /// Print the same report as JSON, for a machine.
    #[arg(long)]
    pub json: bool,
}

/// `fathom export`
#[derive(Args, Debug)]
pub struct ExportArgs {
    /// The artefact format: markdown, html or json.
    #[arg(long, default_value = "markdown")]
    pub format: String,
    /// The file stem to write.
    #[arg(long, default_value = "fathom-review")]
    pub out: String,
}

/// `fathom mcp`
#[derive(Args, Debug)]
pub struct McpArgs {
    /// What to do with the MCP server.
    #[command(subcommand)]
    pub action: McpAction,
}

/// `fathom mcp` actions.
#[derive(Subcommand, Debug)]
pub enum McpAction {
    /// Serve the tool surface over stdio, which opens no socket.
    Serve,
}

/// Run one verb. The return type is the process contract, not a `Result` a caller forgets to check.
///
/// # Errors
///
/// [`Pending`], or anything a verb chose to raise, when the verb cannot be completed; `main` maps that to
/// an exit code at the edge, which is the only place the two are joined.
pub fn dispatch(verb: Option<&Verb>) -> eyre::Result<ExitCode> {
    match verb {
        None => open_terminal(),
        Some(Verb::Index(args)) => Err(not_implemented(
            &format!("fathom index {}", args.root),
            "e1.2",
        )),
        Some(Verb::Show(args)) => Err(not_implemented(
            &format!("fathom show {}", args.episode),
            "e1.6",
        )),
        Some(Verb::Stats) => Err(not_implemented("fathom stats", "e1.10")),
        Some(Verb::Check) => Err(not_implemented("fathom check", "e6.6")),
        Some(Verb::Doctor(args)) => doctor(args),
        Some(Verb::Export(args)) => export(args),
        Some(Verb::Mcp(args)) => mcp(args),
        Some(Verb::Watch) => watch(),
    }
}

fn open_terminal() -> eyre::Result<ExitCode> {
    if !cfg!(feature = "tui") {
        return Err(feature_absent("running with no verb", "tui"));
    }
    Err(pending(
        ExitCode::Error,
        "the terminal UI is compiled in but the shell is not implemented yet",
        Some("owned by e1.5 (reco-v6t); `fathom --help` lists what works today".to_owned()),
    ))
}

fn watch() -> eyre::Result<ExitCode> {
    if !cfg!(feature = "watch") {
        return Err(feature_absent("fathom watch", "watch"));
    }
    Err(not_implemented("fathom watch", "e1.9"))
}

fn mcp(args: &McpArgs) -> eyre::Result<ExitCode> {
    if !cfg!(feature = "mcp") {
        return Err(feature_absent("fathom mcp serve", "mcp"));
    }
    match args.action {
        McpAction::Serve => Err(pending(
            ExitCode::Error,
            "the MCP server is compiled in but not implemented yet",
            Some(
                "owned by e6.4 (reco-p3i); stdio is the transport that will not need consent"
                    .to_owned(),
            ),
        )),
    }
}

fn export(args: &ExportArgs) -> eyre::Result<ExitCode> {
    // The format list is the facade's, so `--format` in the CLI, the TUI's export key and the MCP tool
    // cannot drift apart (§5.3), and its error reaches the right exit code through `Diagnosable`.
    let format = fathom_analyze::export::Format::parse(&args.format).map_err(failed)?;
    let rendered = fathom_analyze::export::render(format, &args.out);
    Err(match rendered {
        Ok(text) => pending(
            ExitCode::Ok,
            format!("wrote {} to {}", args.out, format.file_name(&args.out)),
            Some(format!("{} bytes", text.len())),
        ),
        Err(err) => failed(err),
    })
}

/// The one verb with a real answer at the scaffold: it reports *this build*, which is the thing a scaffold
/// is able to know (ADR-002 §6.4: a bug report should say what was compiled).
fn doctor(args: &DoctorArgs) -> eyre::Result<ExitCode> {
    let status = fathom_analyze::insight::status();
    let rows = features::privacy_table();
    let net_compiled: Vec<&str> = rows
        .iter()
        .filter(|row| row.compiled_in)
        .map(|row| row.feature)
        .collect();

    if args.json {
        let report = serde_json::json!({
            "version": features::VERSION,
            "features": features::compiled(),
            "schema_version": status.schema_version,
            "views_ready": status.views_ready,
            "sqlite": sqlite_provenance(),
            "network_features": rows
                .iter()
                .map(|row| {
                    serde_json::json!({
                        "feature": row.feature,
                        "owner": row.owner,
                        "compiled_in": row.compiled_in,
                        "consent_key": row.consent_key,
                        "ticket": row.ticket,
                    })
                })
                .collect::<Vec<_>>(),
        });
        output::line(format_args!("{report}"));
        return Ok(ExitCode::Ok);
    }

    output::line(format_args!("fathom {}", features::version_line()));
    output::line(format_args!(
        "review model: schema {}, views ready: {}",
        status.schema_version, status.views_ready
    ));
    output::line(format_args!("sqlite: {}", sqlite_provenance()));
    tracing::debug!(features = ?features::compiled(), "build report");

    if args.privacy {
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
        output::line(format_args!(
            "outbound features compiled in: {}",
            if net_compiled.is_empty() {
                "none — no HTTP client and no listener in this binary (ADR-002 R6)".to_owned()
            } else {
                net_compiled.join(", ")
            }
        ));
    } else {
        output::line(format_args!(
            "pass --privacy for the network-feature table, or --json for the same report as data"
        ));
    }
    Ok(ExitCode::Ok)
}

fn sqlite_provenance() -> &'static str {
    if cfg!(feature = "sqlite-bundled") {
        "bundled, compiled in (ADR-004 D2: STRICT, `->>`, FTS5)"
    } else {
        "not compiled into this build (no SQLite feature was enabled)"
    }
}

/// A failure that knows the exit code the process should leave with.
///
/// `main` downcasts to this at the edge (ADR-002 §7.1): the message the human reads, the hint under it
/// and the number CI branches on come from one value, so they cannot disagree.
#[derive(Debug)]
pub struct Pending {
    message: String,
    hint: Option<String>,
    code: ExitCode,
}

impl Pending {
    fn new(code: ExitCode, message: impl Into<String>, hint: Option<String>) -> Self {
        Pending {
            message: message.into(),
            hint,
            code,
        }
    }

    /// The code to leave with.
    #[must_use]
    pub fn code(&self) -> ExitCode {
        self.code
    }

    /// The line to print under the message, if there is one.
    #[must_use]
    pub fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }

    /// The message itself, for `main`'s error path.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for Pending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Pending {}

/// Turn any crate error into a failure carrying its published exit code (§7.1).
fn failed(err: impl Diagnosable + std::fmt::Display) -> eyre::Report {
    Pending::new(
        err.code().exit(),
        err.to_string(),
        err.hint().map(str::to_owned),
    )
    .into()
}

fn pending(code: ExitCode, message: impl Into<String>, hint: Option<String>) -> eyre::Report {
    Pending::new(code, message, hint).into()
}

fn not_implemented(verb: &str, ticket: &str) -> eyre::Report {
    Pending::new(
        ExitCode::Error,
        format!("`{verb}` is not implemented yet"),
        Some(format!(
            "owned by {ticket}; the ticket map is docs/adr/0002-workspace.md §11"
        )),
    )
    .into()
}

fn feature_absent(verb: &str, feature: &str) -> eyre::Report {
    Pending::new(
        ExitCode::FeatureAbsent,
        format!("`{verb}` needs the `{feature}` feature, which this binary was not built with"),
        Some(
            "rebuild with `--features <name>`; `fathom doctor` lists what is compiled in"
                .to_owned(),
        ),
    )
    .into()
}
