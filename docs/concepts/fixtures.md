# Fixtures — the corpus of repositories that did not happen

Status: built by `reco-whw` (stage **reconcile**). Code: `crates/fathom-fixtures` (the generator, the
verifier, the CLI), `crates/fathom-fixtures/corpus/*.json` (the expectations),
`tools/author_corpus.py` (the file that authors the expectations). Consumed by `e1.3` (episode
segmentation), `e2.2` (dependency delta), `e4.3` (change classification) and `e6.1` (harness ingestion).

Fathom's analysis is a set of claims about histories that are not on this machine. The only honest way to
test a claim about a history is to *manufacture* the history: a real Git repository, written byte by byte,
whose SHA-256 is recorded, next to a file that says in advance which episode starts at which commit and
which finding must and must not fire. Then the analysis is wrong in a way that is reproducible on three
operating systems, or it is right.

The corpus is fifteen scenarios, 10 105 commits, 190 expected episodes and 83 expected findings — 25 of
which are assertions that a rule **stays silent**. Measured cost, debug build, one laptop: about 44 s for
the whole corpus including five test binaries' worth of regeneration (`ten_k_commits` is 21 s,
`huge_file` 14 s, everything else under a second combined), which is the ticket's two-minute budget with
room for the CI box to be worse than the laptop.

## 1. The boundary, because a fixture can certify itself

`fathom-fixtures` is a dev-only leaf crate ([ADR-002 §4.4](../adr/0002-workspace.md)). It may depend on
`fathom-core`, `fathom-store`, `fathom-ingest`, `gix`, `serde`, `serde_json`. It must not depend on
`fathom-analyze`, and `tests/determinism.rs::the_source_never_reads_anything_it_should_not` fails the build
if the crate's own source contains any of: `SystemTime`, `Instant::now`, `Local::now`, `chrono::`,
`std::collections::HashMap`, `rand::`, `CARGO_MANIFEST_DIR` (outside `scenario.rs`), or `.canonicalize()`.

The dependency rule is the load-bearing one. **A fixture that computes its own expectations with the code
under test verifies nothing** — it turns a regression into a re-bless. So the generator can build a
repository and hash it, but it cannot segment an episode, classify a change, or decide whether a finding
should fire; those live in the ground-truth files, written by a human, and the generator's only power over
them is to say *this does not match*.

Two more things are on that list for the same reason:

* **one digest function**, `crate::digest`, which delegates to `fathom_ingest::record::sha256_hex`. A
  corpus that hashed blobs its own way would agree with itself and disagree with the product.
* **real `git`, as a verification fixture, never a dependency.** The tests shell out to whatever `git` is
  on the box (isolated with `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_SYSTEM=/dev/null`,
  `core.autocrlf=false`) and demand `git fsck --strict` clean, `git status --porcelain` empty and
  `git rev-list --all --count` equal to the manifest's commit count. The object bytes are hand-written
  with `gix`, so this is the check that says they are Git's bytes and not a close impression.

## 2. Determinism is a policy, not a hope

`src/policy.rs` is the whole policy, and it is short:

* **one instant**, `FIXED_TIMESTAMP`, and every commit, tag and event timestamp is an offset from it. Event
  timestamps are `seq / 1_000` seconds and `seq % 1_000` milliseconds after the commit's instant, so the
  stream's own position is the only clock the file has — and its `ts` column comes out monotonic, which a
  real vendor's does not ([ADR-005 §3.3](../adr/0005-harness-contract.md) orders by `seq` for exactly that
  reason).
* **nobody who exists.** `pi-agent@example.invalid`, `codex-agent@example.invalid`,
  `deps-bot@example.invalid`, plus two humans, all `@example.invalid` (§2.6.9 domains). Identity is a
  *value* the scenario names, so an episode's author is a fixture fact rather than whatever `git config`
  the build machine has.
* **byte-order collation**, explicitly: paths and ref names are sorted with `<` on bytes, never with a
  locale's collator, so a `LANG=tr_TR` machine and a `tr_TR`-less one agree.
* **no filesystem metadata** in any digest: mtimes, inode numbers, file ordering from `readdir`, owner and
  mode (except the executable bit, which is a Git mode and therefore a fact about the tree).
* **absolute paths normalised to `<ROOT>` before hashing.** The event log must carry absolute paths —
  `fathom-ingest` resolves containment against them — and an absolute path is the one thing in a fixture
  that cannot be portable. Exactly that one string is replaced, and `tests/corpus.rs` generates the same
  scenario under two different scratch directories and asserts the digests are equal.

What is left is a digest over: every object written, every ref, the work-tree bytes, and the event log.

## 3. What a scenario writes

```text
cargo run -p fathom-fixtures -- --scenario cycle_introduced /tmp/repo
```

* **a Git repository**, object-by-object, loose and zlib, plus a hand-written `DIRC` v2 index and
  `.git/info/exclude` so the sidecars below are excluded rather than merely untracked. This is the only
  way to get the histories the analyser needs and `git` will not make for you: a gitlink with no submodule
  clone, a shallow cut whose boundary commit still names a parent that is not present, a branch rewritten
  under itself, an orphan root. `shallow_clone` is byte-faithful upstream: the visible commits keep their
  upstream SHAs and `.git/shallow` names the boundary, the way `git clone --depth` writes it.
* **`fixture.json`**, the manifest: the walk-ordered commits (author, kind, session, turn, parents, changed
  paths with add/delete line counts and rename sources), refs and tips, companions, `shallow`, the four
  digests, the verification verdict, and the ground truth itself, so a consumer needs one file and cannot
  silently disagree with the corpus about which ground truth it read.
* **`.fathom/events.jsonl`** for the agent-driven scenarios, to
  [ADR-005 §2–§3](../adr/0005-harness-contract.md): two layers per line — the vendor's record verbatim in
  `payload`, and `fathom_event`, the projection name or `null`. The projection table is `project()` from
  `src/recommender/harness/harness.py`, reproduced with the same rows, because the mapping is not this
  crate's to invent. No `test_run` is written: §2.2 derives it from a recognisable test command, so the
  fixture writes the `cargo test` tool pair and lets the derivation be the thing under test. A prompt is
  stored as `prompt_sha256`/`prompt_chars`, never as text — no file contents, and no free prose, cross the
  boundary.

## 4. Ground truth: two halves, deliberately different owners

`crates/fathom-fixtures/corpus/<scenario>.json`, schema `fathom.fixture-ground-truth/1`:

| half | fields | who writes it |
| --- | --- | --- |
| determinism | `digest`, `commit_count`, `head`, `tips` | `--bless`, mechanically |
| semantics | `purpose`, `consumers`, `episodes`, `findings`, `claims`, `notes` | a human, via `tools/author_corpus.py` |

`--bless` rewrites the first half and only the first half, and it re-reads the file from disk before doing
so (an `include_str!` constant is a build-time snapshot; writing a snapshot back would quietly undo a hand
edit made after the last build). The split is the point: a command that could rewrite the semantics would
turn the corpus into a recording of whatever the code currently does, which is the failure mode this whole
crate exists to avoid.

An **episode** names its commits (or `span: [first, length]`), its author, kind, session and turn, and how
it was grounded (`trailer`, `root`, …). A **finding** names a rule, whether it must `fire`, `silent`,
`collapse` into another or `survive` a rewrite, and where: a commit, an episode, paths. A **claim** is a
fact about the bytes that `verify.rs` can check without analysis: paths changed in a commit, a path whose
change is whitespace-only, a rename pair, a substring present or absent in a blob at a commit, a size
floor, a binary path, a tip, a ref, a gitlink, shallowness, an event count.

`verify.rs` does more than compare digests. It insists the ground truth is *coherent*: episode ids unique,
every finding's episode and commit exist, referenced paths really appear in that commit's story, episodes
tile the lineage without overlapping or leaving a commit unowned, an episode's author agrees with the first
commit it claims, and every rule named in a finding is a rule someone has written down. Most of the bugs
this caught during authoring were errors in the expectations, not in the generator — which is the reason
to spend the code.

## 5. Adding a scenario: one function, one file

1. **`crates/fathom-fixtures/src/scenarios.rs`** — write `pub fn my_scenario() -> Script`, a history in the
   fluent DSL: `script.commit("…").by(&PI).turn("sess","1").trailer("Agent-Intent","…").write(path, bytes).done()`,
   with `fork`, `on`, `merge_of`, `append`, `delete`, `rename`, `copy`, `submodule`, `tag`, `at`. Give it no
   expectations yet.
2. **`crates/fathom-fixtures/src/scenario.rs`** — add the variant to `Scenario`, one arm each in `name`,
   `from_name` and `script`, and one `ground_truth_path` arm. The `include_str!` makes a missing ground-truth
   file a compile error, which is the cheapest possible guarantee that no scenario ships unverified.
3. **`tools/author_corpus.py`** — add a `scenario(...)` block: purpose, which tickets consume it, the
   expected episodes, the expected findings **including the ones that must stay silent**, and the claims.
   Run `python3 tools/author_corpus.py`.
4. **Bless**: `cargo run -p fathom-fixtures -- --all /tmp/fx --bless`, then rebuild and run
   `--all /tmp/fx` again to confirm every scenario verifies. Then run
   `cargo test -p fathom-fixtures --test corpus`, which reads only the corpus and `git`.

To see the history a scenario actually builds before writing expectations, run it with `--table`: one line
per commit (branch, kind, session:turn, author, parent count, file count, `+-` lines, subject) and one line
per path it moved. That output exists for exactly this moment; writing ground truth from the DSL source is
guessing, and a guessed claim is a re-bless waiting to happen.

## 6. What is in the corpus, and what would make it wrong

`rename_storm`, `dependency_bump_storm`, `cycle_introduced`, `test_deletion`, `reformat_only`,
`scope_creep`, `rebase_rewrite`, `churn_loop`, `huge_file`, `unicode_paths`, `many_merges`,
`ten_k_commits`, `submodule`, `shallow_clone`, `clean_baseline`.

`clean_baseline` is the one that matters most: a repository with an ordinary history, no cycles, no
deletions, no scope creep, whose ground truth is almost entirely *silences*. A corpus of only positives
proves an analyser fires; only a negative control proves it fires **because of** what it is looking at.

The corpus is wrong, in the sense of not doing its job, the first time any of these is true: a scenario's
expectations are derived from the analyser's output; a digest changes without a scenario function changing;
a finding list contains only firings; or an event log is written by hand rather than from the commits it
describes. `--bless` is the tool that can produce all four quietly, which is why it touches one half of one
file and prints the path it touched.
