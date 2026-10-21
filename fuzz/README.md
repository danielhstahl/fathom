# Fuzzing (`e7.8`, `reco-41a`)

Three cargo-fuzz targets over the readers that take **repository content**: lockfiles, hook events, and
path names. The threat model — what each target is a stand-in for, and what is already covered elsewhere —
is [docs/security/threat-model.md](../docs/security/threat-model.md).

```bash
cargo +nightly install cargo-fuzz
cd fuzz
mkdir -p fuzz/corpus && cp -R corpus/. fuzz/corpus/    # seed the run (see "Seeding a run" below)
cargo +nightly fuzz run lockfile          # fuzz until it crashes or you stop it
cargo +nightly fuzz run lockfile -- -max_total_time=1800 -rss_limit_mb=4096 -timeout=25
cargo +nightly fuzz run hooks
cargo +nightly fuzz run paths
```

## Why this is a separate workspace

`fuzz/Cargo.toml` carries its own `[workspace]` table and the root excludes it. libFuzzer is
`-Zsanitizer=address`, which stable cannot pass; the repository's `rust-toolchain.toml` pins stable so that
`cargo build --workspace` stays a promise a clean clone can keep. Nothing under `fuzz/` is built by
`cargo build --workspace`, `cargo test --workspace`, `cargo deny`, or `cargo dist`, and nothing under `fuzz/`
links into the shipped binary.

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
cd fuzz
cargo +nightly fuzz run lockfile corpus/lockfile/Cargo.lock__deep-array
```

## The corpus

`corpus/` is **generated**: `python3 tools/gen_fuzz_corpus.py write`, and
`python3 tools/gen_fuzz_corpus.py check` (run in CI) fails if the committed corpus and the generator
disagree. Seeds are *shapes* — nesting, repetition, one enormous token, separators where a name belongs —
because plausible lockfiles are already what `crates/fathom-analyze/tests/golden/deps/` holds.

### Seeding a run

`cargo fuzz` keeps *its* working corpus under `fuzz/fuzz/corpus/<target>` — a different directory from the
committed seeds, on purpose: the fuzzer appends whatever it discovers, and a discovered byte sequence is not
something this repository reviews. So a run has to be pointed at the seeds, either by copying them into place
(which is what CI does, so the committed corpus is never written to):

```bash
mkdir -p fuzz/fuzz/corpus && cp -R fuzz/corpus/. fuzz/fuzz/corpus/
cargo +nightly fuzz run lockfile
```

or by naming one seed or one directory on the command line:

```bash
cd fuzz && cargo +nightly fuzz run lockfile corpus/lockfile
```

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
