Rust artifacts use a lot of disk space in this project.  Run `cargo clean` before starting work.

Every commit lands with its own `[Unreleased]` entry in `CHANGELOG.md`, naming the bead it closes.  The
release notes are a CI gate here, not a later chore: `python3 tools/gen_changelog.py audit` fails on any
commit since the last release that `## [Unreleased]` does not account for — by bead id, plan key, short sha
or `Changelog:` text (`C03`).  Two jobs run it: `docs + workspace contracts`, and the `e7.7 beta loop` step
of `build+test (ubuntu-latest · stable)`.  Run it in the branch, before committing: it needs nothing but the
history, and the audit counts commits rather than diffs, so a commit that defers its prose goes red wherever
the gate runs — and one red commit buys two tickets, one per job.  The full rule is
[docs/concepts/release.md](docs/concepts/release.md).
