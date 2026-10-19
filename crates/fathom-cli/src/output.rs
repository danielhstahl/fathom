//! Writing to a terminal the process does not own (ADR-002 §7.2, §7.5).
//!
//! The workspace denies `clippy::print_stdout`, and for a binary that is not a prohibition on printing —
//! it is a requirement that printing happens in one place, where the two things every CLI gets wrong are
//! already handled: a closed pipe must not become a panic (`fathom --help | head` is a normal thing to
//! type), and everything the user is meant to act on goes to stderr, so `fathom --json | jq` stays
//! parseable.

use std::fmt::Arguments;
use std::io::{self, Write};

/// One line on stdout: the answer.
pub fn line(args: Arguments<'_>) {
    let _ = write_line(io::stdout(), args);
}

/// One line on stderr: the diagnosis.
pub fn problem(args: Arguments<'_>) {
    let _ = write_line(io::stderr(), args);
}

/// One row on stderr with no newline: the progress row that overwrites itself (`e5.9`).
///
/// Only a terminal should be handed one — see [`crate::guide`], which decides that. The write is flushed
/// because a progress row that sits in a buffer is a progress row nobody sees until the run ends.
pub fn row(args: Arguments<'_>) {
    let mut sink = io::stderr();
    let _ = sink.write_fmt(args);
    let _ = sink.flush();
}

fn write_line(mut sink: impl Write, args: Arguments<'_>) -> io::Result<()> {
    sink.write_fmt(args)?;
    sink.write_all(b"\n")?;
    sink.flush()
}
