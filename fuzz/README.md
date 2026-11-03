# Fuzzing (`e7.8`, `reco-41a`)

Three cargo-fuzz targets over the readers that take **repository content**: lockfiles, hook events, and
path names. The threat model — what each target is a stand-in for, and what is already covered elsewhere —
is [docs/security/threat-model.md](../docs/security/threat-model.md).

```bash
# from the repository root
cargo +nightly install cargo-fuzz
mkdir -p /tmp/fathom-seeds && cp -R fuzz/corpus/. /tmp/fathom-seeds/   # seed the run; see "Seeding a run"
cargo +nightly fuzz run lockfile /tmp/fathom-seeds/lockfile   # fuzz until it crashes or you stop it
cargo +nightly fuzz run lockfile /tmp/fathom-seeds/lockfile -- -max_total_time=1800 -rss_limit_mb=4096 -timeout=25
cargo +nightly fuzz run hooks /tmp/fathom-seeds/hooks
cargo +nightly fuzz run paths /tmp/fathom-seeds/paths
```

## Why this is a separate workspace

`fuzz/Cargo.toml` carries its own `[workspace]` table and the root excludes it. libFuzzer is
`-Zsanitizer=address`, which stable cannot pass; the repository's `rust-toolchain.toml` pins stable so that
`cargo build --workspace` stays a promise a clean clone can keep. Nothing under `fuzz/` is built by
`cargo build --workspace`, `cargo test --workspace`, `cargo deny`, or `cargo dist`, and nothing under `fuzz/`
links into the shipped binary.

## The marker cargo-fuzz reads before it does anything

`Cargo.toml` here carries:

```toml
[package.metadata]
cargo-fuzz = true
```

That is not decoration. cargo-fuzz reads it as the first thing it does — before it compiles a target, before
it resolves a corpus — and refuses to go on without it:

```text
Error: manifest fuzz/Cargo.toml does not look like a cargo-fuzz manifest. Add following lines to override:
[package.metadata]
cargo-fuzz = true
```

which is the entire report a red fuzz job leaves: no target, no crash, no compile error, three nightly jobs
agreeing with each other (`reco-com`).

The marker also drives the directory discovery in the section "Seeding a run" below: cargo-fuzz walks up
from the working directory to the first manifest that is **not** marked, and takes `<that>/fuzz` as the fuzz
directory. Nothing else in this repository can see the marker — `cargo build`, `cargo clippy`, `cargo deny`
and `tools/gen_fuzz_corpus.py check` all read this manifest happily without it, so it can go missing in a
refactor that leaves nothing else red. `python3 tools/check_workflows.py` rule `P04` reads it the way
cargo-fuzz does, on every pull.

## The one rule: a target and a test must call the same function

Each target calls a `pub fn` in the crate that owns the reader — not a copy of its logic:

| target | driver | per-commit test over the same driver |
|---|---|---|
| `lockfile` | [`fathom_analyze::hardening::lockfile`](../crates/fathom-analyze/src/hardening.rs) | `crates/fathom-analyze/tests/hardening.rs` |
| `hooks` | [`fathom_ingest::hardening::hook_event`](../crates/fathom-ingest/src/hardening.rs) | `crates/fathom-ingest/tests/hooks_hardening.rs` |
| `paths` | [`fathom_analyze::hardening::paths`](../crates/fathom-analyze/src/hardening.rs) | `crates/fathom-analyze/tests/hardening.rs` |

That is the whole reason the drivers live in `src/` of their crates rather than in this directory: a nightly
fuzzer that fuzzes a different call from the one the merge gate runs tells you nothing about what the gate
tested.

## Seed format

| target | one seed is |
|---|---|
| `lockfile` | `<file-name>\0<content>` — a seed has to name the file it pretends to be, or `deps::discover::recognize` waves it away and no parser is ever reached. No NUL means "all of it is the content of `Cargo.lock`". |
| `hooks` | the raw bytes of one or more lines of `.fathom/events.jsonl` |
| `paths` | `path\0path\0…` |

A seed is self-describing so that a crash artefact is replayable with no other context:

```bash
# from the repository root — `cargo fuzz` finds its fuzz directory as ./fuzz from wherever you stand
cargo +nightly fuzz run lockfile fuzz/corpus/lockfile/Cargo.lock__deep-array
```

## The corpus

`corpus/` is **generated**: `python3 tools/gen_fuzz_corpus.py write`, and
`python3 tools/gen_fuzz_corpus.py check` (run in CI) fails if the committed corpus and the generator
disagree. Seeds are *shapes* — nesting, repetition, one enormous token, separators where a name belongs —
because plausible lockfiles are already what `crates/fathom-analyze/tests/golden/deps/` holds.

### Seeding a run
`cargo fuzz` is run from the **repository root**. Its fuzz directory is `./fuzz` relative to the package it
finds from the working directory, and the marker above is what makes that land on `fuzz/` from either
directory: from the root directly, from inside `fuzz/` by walking up past this manifest to the root — without
it, standing in `fuzz/` sends the run looking for `fuzz/fuzz/`, and the failure reads as a missing target
rather than a missing level of the path. The root is still where a run belongs, because everything around
`cargo fuzz` in `.github/workflows/fuzz.yml` — the seed copy, the `fuzz/artifacts/<target>` upload — is
written from there.

Run from the root, its working corpus is `fuzz/corpus/<target>` — **the same directory the generator
writes**, which is the one thing about this layout worth knowing before you press enter: the fuzzer appends
everything it discovers to the directory it is pointed at, and a discovered byte sequence is not something
this repository reviews. So point a real run at a copy:

```bash
mkdir -p /tmp/fathom-seeds && cp -R fuzz/corpus/. /tmp/fathom-seeds/
cargo +nightly fuzz run lockfile /tmp/fathom-seeds/lockfile
```

That is what CI does (`.github/workflows/fuzz.yml`, into `$RUNNER_TEMP/seeds`), and it is why
`gen_fuzz_corpus.py check` can keep treating an unexplained file in `corpus/` as a failure. If you do let a
run write into `fuzz/corpus/` anyway, `git checkout fuzz/corpus` (or re-running the generator's `write`)
is the undo.

**A crash artefact is never committed here.** It becomes a regression test in the crate's `tests/`
(`hardening.rs` / `hooks_hardening.rs`) and, if it generalises, a new shape in
`tools/gen_fuzz_corpus.py`. That keeps `check` meaningful: an unexplained file in `corpus/` is a corpus
nobody can reproduce.

## What the CI cadence is

| when | what | where |
|---|---|---|
| every pull that touches the readers | 60 s per target | `.github/workflows/fuzz.yml`, `smoke` |
| nightly, and on demand with a chosen duration | 30 min per target | `.github/workflows/fuzz.yml`, `nightly` |
| every pull, on stable, no nightly | every corpus seed + 400 proptest cases per driver | `cargo test --workspace --all-features` |

The last row is the one that runs in a fork with no network: the corpus and proptest halves are ordinary
tests. libFuzzer adds coverage-guided search, which an ordinary test cannot; the ordinary tests add the
guarantee that a nightly outage does not mean nothing was checked.

## Flags, and why these

| flag | why |
|---|---|
| `-rss_limit_mb=4096` | "this seed made the reader allocate 40 GB" is a bug, and without the limit it is an OOM-killed runner with no report |
| `-timeout=25`/`-timeout=10` | a quadratic-in-the-input parser is a denial of service in a tool that indexes repositories; slowness is reported as a failure, not tolerated |
| `-error_exitcode=86` | the exit code `fathom`'s own contract uses for a timeout-ish failure, so a wrapper script cannot confuse "the fuzzer crashed" with "the job was cancelled" |
| `-max_total_time` | the wall clock the CI cadence is built around |

AddressSanitizer is on by default in `cargo fuzz`; it is what turns a one-byte overread in a hand-written
reader into a report instead of a wrong answer.
