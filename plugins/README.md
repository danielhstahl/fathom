# Out-of-tree plugin crates (`e6.7`, `reco-raw`)

Every crate in this directory is **not a member of the workspace at the repository root**. Each one is its
own cargo workspace — `[workspace]` in its own `Cargo.toml`, its own `Cargo.lock`, its own `target/` — and
it depends on the analysis library the way a third party would: as a dependency, through the public API,
with no path into `crates/…/src` and no `#[path]` include of anything.

That distinction is the whole point of the directory. A crate inside the workspace can reach a `pub(crate)`
helper, read a fixture two directories up, and be compiled by `cargo build --workspace` before the release
is cut. A crate out here can do none of that, so *it fails for the reasons a stranger's crate fails*. The
[`fuzz/`](../fuzz) directory is the same trick for the same reason.

## The one example

| crate | what it is | how CI runs it |
|---|---|---|
| [`acme-detectors`](acme-detectors) | two third-party detectors over the delta document, plus a binary that reads `fathom analyze --json` on stdin | built, tested and run in the `out-of-tree plugin` job of `.github/workflows/ci.yml`, including against a document the freshly built binary printed |

Build and run it locally:

```console
$ cd plugins/acme-detectors
$ cargo test                                            # unit tests, no repository needed
$ cargo run -q -- --list                                # what the crate registers, and what it can say
$ cargo run -q -- --delta fixtures/miniature.json --text
acme.untested_agent_logic  medium  src/core/mod.rs  28 lines of source were added by an agent turn across 2 file(s) …
acme-detectors: acme.untested_agent_logic ran findings=1 hidden=0 rejected=0 suppressed=0
acme-detectors: acme.unseen_reach ran findings=0 hidden=0 rejected=0 suppressed=0
$ cargo run -q -- --delta fixtures/miniature.json --json --check fixtures/miniature.expected.jsonl
acme-detectors: 1 findings, byte-identical to fixtures/miniature.expected.jsonl
```

And from the repository root, against a repository this build has actually indexed — which is the shape a
third party uses it in, and the line the CI job runs (`--demo` is the built-in miniature repository, embedded
in the binary, so the line below works on a fresh clone with nothing to clone):

```console
$ cargo build -p fathom-cli
$ ./target/debug/fathom --demo analyze ep-7090b6693637812919bd --json \
    | (cd plugins/acme-detectors && cargo run -q -- --json)
{"schema_version":1,"finding_type":"acme.untested_agent_logic", … }
```

The document on stdin is `fathom analyze`'s published delta document and nothing more: the plugin never
touches the store, never shells out to `git`, and cannot tell whether the repository it is describing still
exists. That is what makes it safe to run one over a colleague's export.

## What a plugin here is allowed to touch

Only what `fathom_analyze::plugin` hands it. That is a deliberate shortage: a detector gets
[`Input`](../crates/fathom-analyze/src/plugin/mod.rs) — one borrowed, already-composed
[`EpisodeDelta`](../crates/fathom-analyze/src/insight/delta.rs) and, if the caller supplied them, the
episodes before it — and returns `Vec<Finding>`. There is no store handle, no repository handle, no clock
and no network in the argument list, so a plugin cannot be wrong in a way that is invisible in the document
it was given. The same refusal the built-in catalogue
([`docs/concepts/pathology-detectors.md`](../docs/concepts/pathology-detectors.md)) makes about its own
detectors, enforced by a type signature instead of by review.

The published surface, and what is promised about it, is
[docs/concepts/extensibility.md](../docs/concepts/extensibility.md), section *Stability policy*. The short
version: `fathom_analyze::plugin`, `fathom_core::events`, the finding schema
([`schemas/plugin-finding.schema.json`](../schemas/plugin-finding.schema.json)) and the config keys
`detectors.*` / `events.*` are the contract; everything else in `fathom-analyze` is an implementation
detail that happens to be `pub` because Rust needs it to be.

## Adding your own

Copy `acme-detectors`, change the namespace in `src/lib.rs` to something you own, and write `detect`. The
rules a finding has to satisfy — namespacing, severity, confidence, suppression keys — are enforced by the
registry rather than trusted, and they are listed in
[`crates/fathom-analyze/src/plugin/finding.rs`](../crates/fathom-analyze/src/plugin/finding.rs) with a
number each (R1…R8). Break one and you get a notice in the report, not a crash: the product is built to
survive a plugin that is wrong.
