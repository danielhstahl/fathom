//! The one place this binary writes, so that printing is a decision rather than a side effect.
//!
//! The workspace warns on `clippy::print_stdout`, and for a binary that is not a prohibition on printing: it
//! is a requirement that printing happens in one place, where the two things every command line gets wrong are
//! already handled. A closed pipe must not become a panic — `fathom-fixtures --all /tmp/x --table | head` is a
//! normal thing to type, and the table is long — and the diagnosis goes to stderr so that `--json` stays
//! pipeable into `jq`.
//!
//! Same shape as `fathom-cli`'s `output` module, for the same reasons; it is small enough that keeping the two
//! honest is cheaper than a shared crate no shipping binary would depend on.

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

fn write_line(mut sink: impl Write, args: Arguments<'_>) -> io::Result<()> {
    sink.write_fmt(args)?;
    sink.write_all(b"\n")?;
    sink.flush()
}
