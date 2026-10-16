//! The fixture generator's command line.
//!
//! ```text
//! cargo run -p fathom-fixtures -- --scenario cycle_introduced /tmp/repo
//! ```
//!
//! Four things it can do, and each one exists because a later ticket needs it: materialise one scenario
//! (`--scenario`), materialise the whole corpus (`--all`), print what is in the corpus (`--list`), and
//! re-stamp a ground-truth file's determinism block after an intentional change to a scenario (`--bless`).
//!
//! The exit status is part of the interface: a non-zero status means the repository does not agree with its
//! ground truth, so a CI job that only runs this command is a real check and not a generator with a decorative
//! report. Everything it prints goes through [`report`], for the reasons given there.

mod report;

use std::fmt::Arguments;
use std::path::PathBuf;
use std::process::ExitCode;

use fathom_fixtures::generate::{self, Fixture};
use fathom_fixtures::scenario::{self, Scenario};
use fathom_fixtures::truth::GroundTruth;

const USAGE: &str = "\
USAGE:
    cargo run -p fathom-fixtures -- --scenario <NAME> <DEST> [FLAGS]
    cargo run -p fathom-fixtures -- --all <DEST> [FLAGS]
    cargo run -p fathom-fixtures -- --list

FLAGS:
    --bless         rewrite <NAME>'s determinism block from what was just built
    --table         print one line per commit: who, which turn, what moved
    --json          print the manifest as JSON
    --list          list the scenario names and exit

A non-zero exit status means the repository did not agree with its ground truth.
";

fn main() -> ExitCode {
    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(None) => return ExitCode::SUCCESS,
        Ok(Some(args)) => args,
        Err(message) => {
            report::problem(format_args!("fathom-fixtures: {message}"));
            report::problem(format_args!("{USAGE}"));
            return ExitCode::from(2);
        }
    };
    if args.list {
        for name in scenario::names() {
            out(format_args!("{name}"));
        }
        return ExitCode::SUCCESS;
    }
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            report::problem(format_args!("fathom-fixtures: {message}"));
            ExitCode::FAILURE
        }
    }
}

/// One line of the report, on stdout.
fn out(args: Arguments<'_>) {
    report::line(args);
}

/// What the caller asked for.
struct Args {
    /// The scenarios to build, in the order given.
    scenarios: Vec<Scenario>,
    /// Where to build them: a destination for one scenario, a parent directory for more than one.
    destination: Option<PathBuf>,
    /// Re-stamp the determinism block instead of only checking it.
    bless: bool,
    /// Print the manifest as JSON instead of the human summary.
    json: bool,
    /// Print the commit table, which is what a person writing a new ground-truth file reads first.
    table: bool,
    /// Just list the corpus.
    list: bool,
}

impl Args {
    /// `Ok(None)` means the caller asked for `--help` and the usage has already been printed.
    fn parse(input: impl Iterator<Item = String>) -> Result<Option<Args>, String> {
        let mut scenarios: Vec<Scenario> = Vec::new();
        let mut destination = None;
        let mut bless = false;
        let mut json = false;
        let mut table = false;
        let mut list = false;
        let mut all = false;
        let args: Vec<String> = input.collect();
        // `--scenario cycle_introduced /tmp/repo` is the ticket's spelling; more than one name after the flag
        // is allowed, because `--scenario a b c` reads better than repeating the flag.
        let mut positional: Vec<String> = Vec::new();
        let mut expect_name = false;
        for arg in &args {
            match arg.as_str() {
                "--scenario" | "-s" => expect_name = true,
                "--all" => all = true,
                "--bless" => bless = true,
                "--json" => json = true,
                "--table" => table = true,
                "--list" | "-l" => list = true,
                "--help" | "-h" => {
                    out(format_args!("{USAGE}"));
                    return Ok(None);
                }
                other if other.starts_with('-') && other.len() > 1 => {
                    return Err(format!("unknown flag `{other}`"));
                }
                other => {
                    if expect_name {
                        expect_name = false;
                        scenarios.push(Scenario::from_name(other).ok_or_else(|| {
                            format!(
                                "no scenario `{other}`; try one of {}",
                                scenario::names().join(", ")
                            )
                        })?);
                    } else {
                        positional.push(other.to_owned());
                    }
                }
            }
        }
        match positional.as_slice() {
            [] => {}
            [one] => destination = Some(PathBuf::from(one)),
            many => {
                return Err(format!(
                    "expected at most one destination, got {} ({})",
                    many.len(),
                    many.join(" ")
                ));
            }
        }
        if all {
            scenarios = Scenario::ALL.to_vec();
        }
        if scenarios.is_empty() && !list {
            return Err("nothing to do; pass --scenario <NAME>, --all, or --list".to_owned());
        }
        if scenarios.len() > 1 && destination.is_none() {
            return Err(
                "more than one scenario needs a parent directory to build each one in".to_owned(),
            );
        }
        Ok(Some(Args {
            scenarios,
            destination,
            bless,
            json,
            table,
            list,
        }))
    }
}

fn run(args: &Args) -> Result<(), String> {
    let root = args
        .destination
        .clone()
        .unwrap_or_else(|| PathBuf::from("fixture"));
    let mut failures = Vec::new();
    for scenario in &args.scenarios {
        let dest = if args.scenarios.len() == 1 && args.destination.is_some() {
            root.clone()
        } else {
            root.join(scenario.name())
        };
        let fixture = generate::generate(*scenario, &dest).map_err(|error| error.to_string())?;
        report(&fixture, args);
        if args.bless {
            bless(&fixture).map_err(|error| error.to_string())?;
        }
        for failure in &fixture.failures {
            failures.push(format!("{}: {failure}", scenario.name()));
        }
    }
    if !failures.is_empty() {
        return Err(format!(
            "{} verification failure(s):\n  {}",
            failures.len(),
            failures.join("\n  ")
        ));
    }
    Ok(())
}

/// Write the determinism block back into the ground-truth file.
///
/// Only that block: the semantics are what a human asserted, and a command that could rewrite them would turn
/// the corpus into a recording of whatever the code currently does. The file is re-read from disk rather than
/// taken from the copy this binary was compiled with, because an `include_str!` constant is a build-time
/// snapshot and writing a snapshot back would quietly undo a hand edit made after the last build.
fn bless(fixture: &Fixture) -> Result<(), Box<dyn std::error::Error>> {
    let path = fixture.scenario.ground_truth_path();
    let mut truth = match std::fs::read_to_string(&path) {
        Ok(text) => GroundTruth::parse(&text, &path.display().to_string())?,
        Err(_) => fixture.ground_truth.clone(),
    };
    truth.determinism = generate::determinism_of(fixture);
    let body = format!(
        "{}\n",
        serde_json::to_string_pretty(&truth).map_err(|error| error.to_string())?
    );
    std::fs::write(&path, body.as_bytes())?;
    report::problem(format_args!("blessed {}", path.display()));
    Ok(())
}

fn report(fixture: &Fixture, args: &Args) {
    let manifest = &fixture.manifest;
    if args.json {
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        let _ = serde_json::to_writer_pretty(&mut handle, manifest);
        let _ = std::io::Write::write_all(&mut handle, b"\n");
        return;
    }
    out(format_args!(
        "{} — {} commits, head {}",
        manifest.scenario, manifest.commit_count, manifest.head
    ));
    out(format_args!(
        "  digest {} (objects {} @ {}, work tree {} @ {}, events {} @ {})",
        manifest.digest,
        manifest.objects.count,
        manifest.objects.digest,
        manifest.worktree.count,
        manifest.worktree.digest,
        manifest.events.count,
        manifest.events.digest
    ));
    for branch in &manifest.branches {
        out(format_args!("  branch {} -> {}", branch.r#ref, branch.sha));
    }
    for tag in &manifest.tags {
        out(format_args!("  tag    {} -> {}", tag.r#ref, tag.sha));
    }
    for companion in &manifest.companions {
        out(format_args!(
            "  companion {} at {} (head {})",
            companion.name, companion.git_dir, companion.head
        ));
    }
    if !manifest.shallow.is_empty() {
        out(format_args!("  shallow: {}", manifest.shallow.join(", ")));
    }
    if args.table {
        table(manifest);
    }
    if manifest.verification.ok {
        out(format_args!(
            "  verification: ok ({} episodes, {} findings, {} claims)",
            manifest.ground_truth.episodes.len(),
            manifest.ground_truth.findings.len(),
            manifest.ground_truth.claims.len()
        ));
        return;
    }
    out(format_args!(
        "  verification: {} failure(s)",
        manifest.verification.failures.len()
    ));
    for failure in &manifest.verification.failures {
        out(format_args!("    - {failure}"));
    }
}

/// One line per commit, and one per path each commit moved.
///
/// This is what a person writing a new ground-truth file looks at first: the walk order, who wrote each commit,
/// which turn named it, and what moved. Without it the first draft of a ground-truth file is written by
/// guessing, and the guessing shows up later as a claim that names a path the scenario never had.
fn table(manifest: &fathom_fixtures::Manifest) {
    out(format_args!(
        "  {:<4} {:<12} {:<6} {:<18} {:<14} {:<2} {:<3} {:>4} {}",
        "#", "branch", "kind", "turn", "author", "p", "f", "+-", "subject"
    ));
    for commit in &manifest.commits {
        let turn = commit
            .session
            .as_ref()
            .map(|session| format!("{session}:{}", commit.turn.clone().unwrap_or_default()))
            .unwrap_or_else(|| "-".to_owned());
        out(format_args!(
            "  {:<4} {:<12} {:<6} {:<18} {:<14} {:<2} {:<3} {:>4} {}{}",
            commit.index,
            commit.branch,
            commit.author_kind,
            turn,
            commit.author,
            commit.parents.len(),
            commit.n_files,
            format!("{}-{}", commit.n_add, commit.n_del),
            if commit.empty { "(empty) " } else { "" },
            commit.subject
        ));
        for change in &commit.changes {
            let from = change
                .from
                .as_ref()
                .map(|from| format!(" (from {from})"))
                .unwrap_or_default();
            out(format_args!(
                "        {:<9}{}{}",
                change.change, change.path, from
            ));
        }
    }
}
