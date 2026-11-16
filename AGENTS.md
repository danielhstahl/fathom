Rust artifacts use a lot of disk space in this project.  Run `cargo clean` before starting work.

Every commit lands with its own `[Unreleased]` entry in `CHANGELOG.md`, naming the bead it closes.  The
release notes are a CI gate here, not a later chore: `python3 tools/gen_changelog.py audit` fails on any
commit since the last release that `## [Unreleased]` does not account for — by bead id, plan key, short sha
or `Changelog:` text (`C03`).  One job reports it — `docs + workspace contracts` in
`.github/workflows/contracts.yml`, the cheap one that compiles no Rust, so a missing entry is named in
seconds and not behind a workspace build — and rule `C01` refuses a second copy anywhere else, because a
gate that runs twice files twice.  Run it in the branch, before committing: it needs nothing but the
history, and the audit counts commits rather than diffs, so a commit that defers its prose goes red there.
The full rule is [docs/concepts/release.md](docs/concepts/release.md).
