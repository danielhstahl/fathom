//! The example plugin's binary: a delta document in, findings out (`e6.7`, `reco-raw`).
//!
//! ```text
//! fathom analyze ep-7090b66 --json | acme-detectors --text
//! ```
//!
//! That is the whole interface, and it is deliberately the same shape as every other Fathom artefact:
//! `fathom analyze --json` prints the versioned delta model, and anything that can read a JSON object can
//! analyse it. The binary takes its input from a file or stdin, prints one finding per line in `--json`
//! mode (the format [`acme_detectors`] documents in its finding stream), and says what it refused on stderr
//! — never in the same stream as the findings, because a stream a machine reads has one thing on it.
//!
//! Arguments are parsed by hand rather than with a derive crate: this is an example of a plugin, and an
//! example whose dependencies are the library plus `serde_json` is a stronger claim than one that needs the
//! product's whole CLI stack to say hello.

use std::io::Read as _;
use std::process::ExitCode;

use acme_detectors::{UNSEEN_REACH, UNTESTED_AGENT_LOGIC, UnseenReach, UntestedAgentLogic};
use fathom_analyze::detectors::policy::Selection;
use fathom_analyze::insight::EpisodeDelta;
use fathom_analyze::plugin::{Detector, Input, Limits, Registry};
use std::sync::Arc;

/// What `--help` prints. Kept as one string so `--help` and the README cannot disagree by drift.
const USAGE: &str = "\
acme-detectors — two third-party Fathom detectors over one episode delta document

USAGE:
    acme-detectors [OPTIONS]

OPTIONS:
    --delta <path>        read the episode delta document from <path> (default: stdin)
    --json                print one finding per line, as JSON (the default)
    --text                print one finding per line, for a person
    --list                print the detectors this crate registers and what each declares
    --detectors <spec>    move the selection: `+id`, `-id`, `all`, `none` (comma-separated)
    --min-lines <n>       source lines that count as a block (default: 20)
    --min-churn <n>       churn that makes an unseen blast radius worth a row (default: 40)
    --max-findings <n>    rows per detector per episode (default: 12)
    --check <path>        compare the findings this run prints against <path>; exit 1 on a difference
    --help, -h            print this and leave

Both detectors are off by default in their own metadata, which is what an unmeasured plugin owes; this
binary turns them on so the example has something to show. Say `--detectors -acme.unseen_reach` to put one
back off, exactly as `fathom --disable-detector` would.

EXIT STATUS:
    0   the findings were printed
    1   the input could not be read, or --check found a difference
    2   the command line was not one of the above
";

/// What `main` decided the command line meant.
struct Args {
    delta: Option<String>,
    json: bool,
    text: bool,
    list: bool,
    selection: Selection,
    min_lines: i64,
    min_churn: i64,
    max_findings: usize,
    check: Option<String>,
}

fn main() -> ExitCode {
    match parse(std::env::args().skip(1)) {
        Ok(Args {
            delta,
            json,
            text,
            list,
            selection,
            min_lines,
            min_churn,
            max_findings,
            check,
        }) => {
            if list {
                print_manifest();
                return ExitCode::SUCCESS;
            }
            run(
                delta.as_deref(),
                json || !text,
                &selection,
                min_lines,
                min_churn,
                max_findings,
                check.as_deref(),
            )
        }
        Err(reason) => {
            eprintln!("acme-detectors: {reason}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn parse<I: Iterator<Item = String>>(arguments: I) -> Result<Args, String> {
    let mut args = Args {
        delta: None,
        json: false,
        text: false,
        list: false,
        selection: Selection::default(),
        min_lines: acme_detectors::DEFAULT_MIN_SOURCE_LINES,
        min_churn: acme_detectors::DEFAULT_MIN_CHURN,
        max_findings: 12,
        check: None,
    };
    let mut arguments = arguments.peekable();
    while let Some(argument) = arguments.next() {
        // Every flag accepts `--flag value` and `--flag=value`, because the second form is what a shell
        // loop writes and the first is what a person types.
        let (flag, inline) = match argument.split_once('=') {
            Some((flag, value)) => (flag.to_owned(), Some(value.to_owned())),
            None => (argument.clone(), None),
        };
        let mut value = |flag: &str| -> Result<String, String> {
            if let Some(inline) = inline.clone() {
                return Ok(inline);
            }
            arguments
                .next()
                .ok_or_else(|| format!("`{flag}` needs a value"))
        };
        match flag.as_str() {
            "--help" | "-h" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            "--delta" => args.delta = Some(value("--delta")?),
            "--json" => args.json = true,
            "--text" => args.text = true,
            "--list" => args.list = true,
            "--detectors" => {
                let spec = value("--detectors")?;
                args.selection.merge(&Selection::parse(&spec));
            }
            "--min-lines" => args.min_lines = number("--min-lines", value("--min-lines")?)?,
            "--min-churn" => args.min_churn = number("--min-churn", value("--min-churn")?)?,
            "--max-findings" => {
                args.max_findings =
                    number("--max-findings", value("--max-findings")?)?.max(1) as usize
            }
            "--check" => args.check = Some(value("--check")?),
            other => {
                return Err(format!("`{other}` is not a flag this program has"));
            }
        }
    }
    Ok(args)
}

fn number(flag: &str, text: String) -> Result<i64, String> {
    text.trim()
        .parse::<i64>()
        .map_err(|_| format!("`{flag}` wants a whole number and got `{text}`"))
}

/// Both detectors, on, unless the caller said otherwise.
///
/// The metadata says off-by-default because a plugin nobody has measured owes the repository that choice;
/// the binary says on because the point of running it is to see what it says. `--detectors` arbitrates
/// between the two, exactly as it does for a built-in detector.
fn registry(min_lines: i64, min_churn: i64) -> (Registry, Selection) {
    let mut registry = Registry::new();
    let detectors: Vec<Arc<dyn Detector>> = vec![
        Arc::new(UntestedAgentLogic::new(min_lines)),
        Arc::new(UnseenReach::new(min_churn)),
    ];
    registry.register_all(detectors);
    // Both of this crate's detectors, on: their own metadata says off-by-default, and the binary is the
    // host that decides otherwise. `--detectors` still gets the last word.
    let selection = Selection::parse(&format!("+{UNTESTED_AGENT_LOGIC},+{UNSEEN_REACH}"));
    (registry, selection)
}

fn read_delta(path: Option<&str>) -> Result<EpisodeDelta, String> {
    let text = match path {
        Some(path) => {
            std::fs::read_to_string(path).map_err(|err| format!("reading `{path}`: {err}"))?
        }
        None => {
            let mut buffer = String::new();
            std::io::stdin()
                .read_to_string(&mut buffer)
                .map_err(|err| format!("reading stdin: {err}"))?;
            buffer
        }
    };
    if text.trim().is_empty() {
        return Err("no delta document was given; pipe `fathom analyze <episode> --json` in, or name a file with \
             --delta".to_string());
    }
    serde_json::from_str::<EpisodeDelta>(&text).map_err(|err| {
        format!("that is not an episode delta document (`fathom analyze --json` prints one): {err}")
    })
}

fn run(
    delta: Option<&str>,
    json: bool,
    flags: &Selection,
    min_lines: i64,
    min_churn: i64,
    max_findings: usize,
    check: Option<&str>,
) -> ExitCode {
    let document = match read_delta(delta) {
        Ok(document) => document,
        Err(reason) => {
            eprintln!("acme-detectors: {reason}");
            return ExitCode::FAILURE;
        }
    };
    let (registry, mut selection) = registry(min_lines, min_churn);
    selection.merge(flags);
    let report = registry.run(
        &Input::new(&document),
        &selection,
        &Limits {
            max_findings,
            ..Default::default()
        },
    );

    for notice in &report.notices {
        eprintln!("acme-detectors: notice: {notice}");
    }

    let lines: Vec<String> = if json {
        report
            .findings
            .iter()
            .filter_map(|finding| finding.to_line().ok())
            .collect()
    } else {
        report
            .findings
            .iter()
            .map(|finding| {
                format!(
                    "{}  {}  {}  {}",
                    finding.finding_type, finding.severity, finding.subject, finding.message
                )
            })
            .collect()
    };

    if let Some(path) = check {
        return check_against(path, &lines, &report);
    }

    for line in &lines {
        println!("{line}");
    }
    if !json {
        for run in &report.runs {
            eprintln!(
                "acme-detectors: {} {} findings={} hidden={} rejected={} suppressed={}",
                run.detector,
                run.outcome.as_str().to_owned(),
                run.findings,
                run.hidden,
                run.rejected,
                run.suppressed
            );
        }
        if lines.is_empty() {
            eprintln!("acme-detectors: nothing raised for {}", document.episode.id);
        }
    }
    ExitCode::SUCCESS
}

/// Compare this run's findings with a blessed file, bless it when asked, and say what differed.
fn check_against(
    path: &str,
    lines: &[String],
    report: &fathom_analyze::plugin::Report,
) -> ExitCode {
    let written = format!("{}\n", lines.join("\n"));
    if std::env::var("FATHOM_BLESS").is_ok_and(|value| value == "1") {
        if let Err(err) = std::fs::write(path, &written) {
            eprintln!("acme-detectors: could not bless `{path}`: {err}");
            return ExitCode::FAILURE;
        }
        eprintln!("acme-detectors: blessed {path} ({} findings)", lines.len());
        return ExitCode::SUCCESS;
    }
    match std::fs::read_to_string(path) {
        Ok(expected) => {
            if expected == written {
                eprintln!(
                    "acme-detectors: {} findings, byte-identical to {path}",
                    lines.len()
                );
                ExitCode::SUCCESS
            } else {
                eprintln!(
                    "acme-detectors: the findings differ from {path}: {} printed against {} expected \
                     ({} notices said why anything was refused)",
                    lines.len(),
                    expected.lines().count(),
                    report.notices.len()
                );
                for (index, (printed, expected)) in lines
                    .iter()
                    .map(String::as_str)
                    .zip(expected.lines())
                    .enumerate()
                {
                    if printed != expected {
                        eprintln!("  line {}: printed {printed}", index + 1);
                        eprintln!("  line {}: expected {expected}", index + 1);
                    }
                }
                ExitCode::FAILURE
            }
        }
        Err(err) => {
            eprintln!("acme-detectors: reading the blessed file `{path}`: {err}");
            ExitCode::FAILURE
        }
    }
}

/// What this crate registers, in the shape `fathom plugins` would print for it.
fn print_manifest() {
    let (registry, _) = registry(
        acme_detectors::DEFAULT_MIN_SOURCE_LINES,
        acme_detectors::DEFAULT_MIN_CHURN,
    );
    for meta in registry.manifest() {
        let row: Vec<String> = meta
            .row()
            .into_iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect();
        println!("{}", row.join(" "));
    }
}
