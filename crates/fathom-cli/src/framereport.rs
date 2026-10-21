//! The shell's own frame numbers, kept (`reco-08r`).
//!
//! `fathom_tui::frame::run` measures every draw it makes against ADR-002 §6's 16 ms budget and hands
//! the numbers back in a [`Report`]. Until now the binary threw that report away, which left the budget
//! measurable only from outside: the MVP acceptance run could quote what a pty driver saw between a
//! keystroke and a changed screen — honest, and not the quantity the budget is written against — because
//! nothing in the product would say what the product's own draws cost.
//!
//! So the run's numbers are now said twice and written once:
//!
//! * **said** on the shell's way out, one line on stderr (`fathom tui --json` prints the same numbers as
//!   one object on stdout instead), because the reviewer who just closed the pane is the closest reader;
//! * **written** to `.fathom/frame-report.json` — class E, disposable, gitignored like everything else
//!   under `.fathom/` (ADR-004 §3) — so `fathom doctor` can answer "what was the p99 draw time of the
//!   session I just had" an hour later, and a bug report can quote it.
//!
//! The file holds one record: the most recent shell run of this repository, renamed into place so a
//! reader never sees half of it, which is the same contract `.fathom/watch/status.json` keeps and the
//! reason a second writer needs no lock. `doctor` names the file it reads (ADR-005 §3.1) whether or not
//! it is there, because "no shell has run here" and "the record is unreadable" are different sentences.
//!
//! The numbers are a measurement, not a finding: a frame that missed the budget on a loaded machine is
//! not a broken index, so this section never moves `fathom doctor`'s exit code, not even under
//! `--strict`. It states the count, the denominator and the budget, and leaves the judgment where it
//! belongs.

use std::path::{Path, PathBuf};

use fathom_tui::frame::Report;
use serde_json::{Value, json};

use crate::query::FRAME_CONTRACT;

/// Where the record lives: `.fathom/frame-report.json`, under the repository.
///
/// Repo-relative on purpose — the same directory the store, the watcher's heartbeat and the daemon's
/// lease are in, and the one place a run is allowed to write without asking (ADR-004 §7.7).
#[must_use]
pub fn path(root: &Path) -> PathBuf {
    root.join(".fathom").join("frame-report.json")
}

/// The same path as `doctor` prints: relative to the repository, so a bug report does not learn a home
/// directory from a filename (ADR-002 §11.1).
pub const RELATIVE_PATH: &str = ".fathom/frame-report.json";

/// Milliseconds with two decimals, as `Report::summary` says them.
///
/// The record stores milliseconds rather than a duration string because the thing a reader does with
/// the record is compare it against `benchmarks/budgets.json`, which is written in milliseconds.
fn millis(since: std::time::Duration) -> f64 {
    f64::from(since.as_micros() as f32) / 1_000.0
}

/// The record for one finished run, as data.
///
/// The timestamps are stamped here rather than inside `fathom-tui`, which is handed an instant and may
/// not go and read the clock (ADR-002 §4.4); `started_ms` is the instant the binary took the screen and
/// `ended_ms` the instant it gave it back, so a report says how long the reviewer was *in* the frame.
#[must_use]
pub fn record(report: &Report, started_ms: i64, ended_ms: i64) -> Value {
    json!({
        "contract": FRAME_CONTRACT,
        "written_ms": ended_ms,
        "started_ms": started_ms,
        "ended_ms": ended_ms,
        "pid": std::process::id(),
        "fathom": crate::features::VERSION,
        // How the loop ended, in the words `LoopEnd` gives it: a run that hit a frame limit and a run
        // the reviewer quit are different measurements of a frame budget.
        "end": report.end.to_string(),
        "frames": report.frames,
        "events": report.events,
        // The denominator of every statistic below it: the window holds the last
        // `FrameScheduler::WINDOW` draws, so a long session's p99 is about its last frames, not all of
        // them, and a reader has to be able to tell which (reco-08r).
        "window_frames": report.window,
        "budget_ms": millis(report.budget),
        "p99_ms": millis(report.p99),
        "worst_ms": millis(report.worst),
        "over_budget": report.over_budget,
        // The size the draws were made at: the same keystroke costs different work at 80x24 and 200x60.
        "cols": report.size.0,
        "rows": report.size.1,
    })
}

/// Write the record, by rename, so a reader never observes half a file (`watch::write_status`'s reason,
/// applied here for the same shape of reader).
///
/// The failure is a string rather than an `eyre::Report` because nothing may fail over it: a shell that
/// could not leave its numbers behind has already answered the question it was asked.
pub fn write(root: &Path, record: &Value) -> Result<PathBuf, String> {
    let path = path(root);
    let rendered = serde_json::to_string(record).map_err(|err| err.to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| format!("{}: {err}", parent.display()))?;
    }
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, &rendered).map_err(|err| format!("{}: {err}", temp.display()))?;
    std::fs::rename(&temp, &path).map_err(|err| format!("{}: {err}", path.display()))?;
    Ok(path)
}

/// Read the record.
///
/// The three answers are three sentences, and collapsing them into `Option` is how a diagnostic ends up
/// telling somebody their repository has never been opened when the truth is that a disk was full
/// halfway through a write: "never written", "there is a file and it is not readable", and "here it
/// is" are each said in their own words by [`doctor_lines`].
pub fn read(root: &Path) -> Found {
    let path = path(root);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(_) => return Found::Absent,
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(value) => Found::Recorded(value),
        Err(err) => Found::Damaged(format!("{}: {err}", path.display())),
    }
}

/// What was found where the record should be.
#[must_use]
pub enum Found {
    /// No file: no shell has run in this repository since the file was invented, or `.fathom/` was
    /// removed, which it may be at any time (ADR-004 §10.2).
    Absent,
    /// A file that is not the record: a write that died halfway, or a file that is somebody else's.
    Damaged(String),
    /// The record.
    Recorded(Value),
}

/// The line(s) `fathom doctor` prints about the last shell run.
///
/// The p99 and the over-budget count are the two the acceptance run could not get from the product, so
/// they come first and are stated with the denominator and the budget they were measured against.
#[must_use]
pub fn doctor_lines(root: &Path) -> Vec<String> {
    match read(root) {
        Found::Absent => vec![format!(
            "frames: no shell run recorded yet — `fathom tui` writes {RELATIVE_PATH} as it exits, and \
             prints the same line to stderr when it does"
        )],
        Found::Damaged(why) => vec![format!(
            "frames: the last run's record could not be read ({why}); delete the file and the next \
             shell run writes a whole one"
        )],
        Found::Recorded(record) => {
            let cols = record["cols"].as_u64().unwrap_or(0);
            let rows = record["rows"].as_u64().unwrap_or(0);
            let size = if cols == 0 {
                String::new()
            } else {
                format!(", at {cols}x{rows}")
            };
            vec![
                format!(
                    "frames: last shell run ({RELATIVE_PATH}) — pid {}, fathom {}, ended {}, {}",
                    record["pid"].as_u64().unwrap_or(0),
                    record["fathom"].as_str().unwrap_or("?"),
                    record["ended_ms"]
                        .as_i64()
                        .map_or_else(|| "?".to_owned(), crate::query::format_utc),
                    record["end"].as_str().unwrap_or("unknown"),
                ),
                format!(
                    "        {} draws ({} events) · p99 {:.2} ms · worst {:.2} ms · {} of the last {} \
                     draws over the {:.0} ms budget{size}",
                    record["frames"].as_u64().unwrap_or(0),
                    record["events"].as_u64().unwrap_or(0),
                    record["p99_ms"].as_f64().unwrap_or(0.0),
                    record["worst_ms"].as_f64().unwrap_or(0.0),
                    record["over_budget"].as_u64().unwrap_or(0),
                    record["window_frames"].as_u64().unwrap_or(0),
                    record["budget_ms"].as_f64().unwrap_or(0.0),
                ),
            ]
        }
    }
}

/// The same, as data, for `fathom doctor --json`.
///
/// The record is copied rather than re-derived, so the two renderings cannot disagree about a number
/// the shell already measured; `recorded` and `path` are the parts only this reader can add.
#[must_use]
pub fn doctor_json(root: &Path) -> Value {
    match read(root) {
        Found::Absent => json!({ "recorded": false, "path": RELATIVE_PATH }),
        Found::Damaged(why) => json!({
            "recorded": false, "path": RELATIVE_PATH, "error": why,
        }),
        Found::Recorded(mut record) => {
            if let Some(object) = record.as_object_mut() {
                object.insert("recorded".to_owned(), json!(true));
                object.insert("path".to_owned(), json!(RELATIVE_PATH));
            }
            record
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use std::time::Duration;

    /// A run with every statistic in a different relation to the others: a window smaller than the run,
    /// a p99 below the worst, and a non-zero over-budget count, so an assertion cannot pass by reading
    /// the same field twice.
    fn report() -> Report {
        Report {
            frames: 500,
            events: 63,
            p99: std::time::Duration::from_micros(3_200),
            worst: Duration::from_micros(12_400),
            over_budget: 2,
            end: fathom_tui::frame::LoopEnd::Quit,
            session: fathom_tui::session::Delta::default(),
            budget: Duration::from_millis(16),
            window: 128,
            size: (132, 40),
        }
    }

    fn sandbox(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("fathom-framereport-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the sandbox directory");
        dir
    }

    #[test]
    fn the_record_states_every_number_with_what_it_was_measured_against() {
        let record = record(&report(), 1_700_000_000_000, 1_700_000_060_000);
        assert_eq!(record["contract"], FRAME_CONTRACT);
        assert_eq!(
            record["end"], "quit",
            "the sentence the loop's ending is said in"
        );
        assert_eq!(record["frames"], 500);
        assert_eq!(
            record["window_frames"], 128,
            "the denominator of the percentiles"
        );
        assert_eq!(
            record["budget_ms"], 16.0,
            "ADR-002 §6, stated beside the number"
        );
        assert_eq!(record["p99_ms"], 3.2);
        assert_eq!(record["worst_ms"], 12.4);
        assert_eq!(record["over_budget"], 2);
        assert_eq!(record["cols"], 132);
        assert_eq!(record["rows"], 40);
        assert_eq!(record["ended_ms"], 1_700_000_060_000_i64);
        assert_ne!(record["started_ms"], record["ended_ms"]);
    }

    #[test]
    fn a_written_record_comes_back_and_says_the_two_numbers_the_acceptance_run_could_not_get() {
        let root = sandbox("round-trip");
        let written = write(&root, &record(&report(), 1, 2)).expect("the record is written");
        assert_eq!(written, path(&root));
        assert!(
            !path(&root).with_extension("json.tmp").exists(),
            "the write is a rename, so no temporary is left behind"
        );
        let Found::Recorded(read) = read(&root) else {
            panic!("the record just written reads back as something else");
        };
        assert_eq!(read["p99_ms"], 3.2);

        let lines = doctor_lines(&root).join("\n");
        assert!(lines.contains("p99 3.20 ms"), "{lines}");
        assert!(lines.contains("2 of the last 128 draws"), "{lines}");
        assert!(lines.contains("16 ms budget"), "{lines}");
        assert!(lines.contains("at 132x40"), "{lines}");
        assert!(lines.contains("500 draws"), "{lines}");

        let json = doctor_json(&root);
        assert_eq!(json["recorded"], true);
        assert_eq!(json["p99_ms"], 3.2);
        assert_eq!(json["over_budget"], 2);
        assert_eq!(json["window_frames"], 128);
        assert_eq!(json["path"], RELATIVE_PATH);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn no_record_and_an_unreadable_record_are_two_different_sentences() {
        let root = sandbox("absent");
        let lines = doctor_lines(&root).join("\n");
        assert!(lines.contains("no shell run recorded yet"), "{lines}");
        assert_eq!(doctor_json(&root)["recorded"], false);

        std::fs::create_dir_all(root.join(".fathom")).unwrap();
        std::fs::write(path(&root), "{ half a write").unwrap();
        let lines = doctor_lines(&root).join("\n");
        assert!(lines.contains("could not be read"), "{lines}");
        let json = doctor_json(&root);
        assert_eq!(json["recorded"], false);
        assert!(json["error"].as_str().is_some_and(|why| !why.is_empty()));
        std::fs::remove_dir_all(&root).ok();
    }
}
