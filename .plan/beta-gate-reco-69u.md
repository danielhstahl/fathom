# The beta gate, assessed against the board (`reco-69u`, `m1`)

- **What this is**: the milestone's four acceptance criteria, each with a verdict, the command that re-proves
  it, and — where the verdict is "not met" — the ticket that carries it. Written from a real pass: the
  repository indexed with the binary built from this tree, one external repository indexed and reviewed the
  same way, and the Guardrail 3 harness run against its three pinned clones.
- **Milestone text, restated**: a reviewer can work the attention queue, understand dependencies, structure
  and non-code surfaces, and export a sign-off artefact that lands on a PR, with harness intent flowing in.
  Open the beta loop and start dogfooding Fathom on Fathom.
- **Headline**: three of the four criteria are met or met-with-a-stated-hole; **the fourth — the Guardrail 3
  false-positive gate — is not met, and its bars have not been moved.** `e4.9` measured it; this document
  points at that measurement rather than paraphrasing it into a pass.

---

## 1. The four criteria, one by one

### 1.1 "Epic 2 complete; the core tickets of epics 4, 5 and 6 listed as dependencies are closed"

**Met, with one open P2 that is not this milestone's to close.**

All six listed dependencies are closed: `reco-tt4` (the one delta model), `reco-1tn` (the export artefact),
`reco-wsj` (the attention queue), `reco-mkl.1` (the G3 corpus), `reco-7go` (the reference adapters),
`reco-rlf` (the review ledger). Epic 4 (`reco-mkl`) is closed whole. Epic 5 (`reco-xke`) and epic 6
(`reco-6t7`) remain open as epics, which is what the criterion expects of them — it names their *core
tickets*, and those are the ones above.

Epic 2 (`reco-tx0`) is 11 of 12: its open child is `reco-tx0.1` (P2, "fold spike s4's findings into e2.4"),
a follow-up the spike itself filed, not a missing capability. Epic 2's exit criterion — every episode
answers WHERE the change landed and WHAT it now depends on, every number clickable to its evidence, noise
suppressed and never silently hidden — is met by the shipped code; the `fathom analyze` document and the
export artefact are where a reviewer reads that, and both are contract-tested. Closing the epic is
`reco-tx0.1`'s own business, and this milestone does not close another ticket's parent to make a gate read
greener.

### 1.2 "Export artefact used in at least one real PR review outside this project"

**Met, with the one half this environment cannot do named out loud.**

A real change in a real repository outside this one was reviewed with Fathom's own verbs and exported:

- the change: `pallets/flask` PR 6168 — "drop support for Python 3.10" (`7689f98`, 12 files, +67 −433,
  a `uv.lock` regeneration inside it), in a clone pinned at `d086db856be1` by `tools/fetch_g3_corpus.py`;
- the review: `fathom index`, then `fathom why 7689f98` (risk receipt: `features-1 63`, with `test_ratio`
  measuring a test surface that shrank and `noise` measuring 443 of 498 churn lines as lockfile), then
  `fathom analyze` for the per-path rows, then `fathom review sign` with the reviewer's note;
- the artefact: `.plan/evidence/reco-69u/flask-pr6168-review.md`, written by
  `fathom export --root target/g3-corpus/flask --episodes 7689f98 --format markdown`, whose sign-off block
  carries the verdict, the actor, the instant and the note;
- what the review actually found, rather than rubber-stamping: the subject line says "drop support for
  Python 3.10" and the `pyproject.toml` hunk also deletes `[tool.pyright]` and drops `pyright` from the tox
  `typing` env, so a static checker goes with the Python version. That is what the flag was for, and it is
  in the ledger entry.

The half that cannot be done from here: **posting the artefact onto github.com.** There is no credential in
this environment (`gh` is absent, no token), so the artefact lands in this repository as the record of a real
external review instead of as a comment on `pallets/flask#6168`. Anyone with a token can paste
`.plan/evidence/reco-69u/flask-pr6168-review.md` onto that PR; nothing about it is environment-specific
except the `fathom 0.1.0-alpha.1` version line.

Dogfooding on Fathom itself, which the milestone also asks for: the same three verbs against this
repository's own newest episode, signed off and exported to
`.plan/evidence/reco-69u/fathom-on-fathom-review.md`, and this repository's `.fathom/reviews/ledger.md` is
**committed**, which is what `fathom review status` demands of a sign-off and what makes ADR-004 §12's
"the store is disposable, the ledger is not" a fact rather than a slogan.

### 1.3 "No open P1 defects against the beta scope"

**Met.** `bd list --status open --priority 1` names `reco-3gz` (alpha announcement, task), `reco-ngk`
(`fathom check`, task — the verb prints "not implemented yet" and names its own ticket), `reco-tx0` (epic),
and the two follow-ups this pass filed (`reco-dyt`, `reco-6co`, a task and a feature). Every open `[bug]` is
P2 or P3. The defects this pass found are fixed in this commit, except the one it could not fix in scope
(`reco-b7o`, P3).

### 1.4 "G3 <= 0.30 at alpha on the three pinned repositories, miss_rate <= 0.20, measured by the e4.3a harness"

**NOT MET. The bars are unchanged, the measurement is unchanged, and the shortfall is `e4.9`'s number, still
standing:**

```text
python3 tools/fetch_g3_corpus.py
cargo run -p fathom-analyze --features sqlite-bundled --example g3_calibration -- --check
```

| Gate | Bar | Measured | Declared |
| --- | --- | --- | --- |
| `g3-alpha-at-published-tau` | `G3 ≤ 0.30` at τ 70 | denominator 3 of a floor of 5 | not-measurable |
| `g3-alpha-at-corpus-cut` | `G3 ≤ 0.30` | **0.682** (15 / 22) | fail |
| `g3-1-0-at-corpus-cut` | `G3 ≤ 0.15` | 0.682 | fail |
| `miss-rate-at-published-tau` | `miss_rate ≤ 0.20` | **1.000** (7 of 7 reworked below τ) | fail |
| `miss-rate-at-decile-cut` | `miss_rate ≤ 0.20` | 0.571 (4 of 7) | fail |
| `miss-rate-denominator-is-big-enough` | ≥ 20 mature reworked | 7 | fail (the corpus, not the product) |

Nothing in this commit moves a bar, a `expected`, or a blessed byte: `--check` reproduces
`benchmarks/reports/e4-3a-g3-calibration.json` exactly, before and after. What this pass did instead is
close the distance between the *number the reviewer reads* and the number that gate is measured on, which
was a real defect (§2.1), and put the two things that have to happen next on the board as tickets rather
than as prose:

- **`reco-dyt`** — the miss-rate denominator is 7 mature reworked episodes in 180, and a 0.20 gate cannot
  discriminate at n = 7. The fix is the corpus: roughly five times the pinned window, with a hand-written
  disposition for every episode the wider cuts flag. It is labelled as a corpus task with the labelling
  rule in its acceptance criteria, because the corpus is allowed to grow and not to be relaxed.
- **`reco-6co`** — the flag arm is inert and the top of the ranking is bot churn: `features-1` reaches ~60
  on real history against a τ of 70, while 11 of the 15 false positives were routine bumps and 7 of 15 were
  blamed on the ranking rather than a detector. Dogfooding named the mechanism precisely (a compiled
  `.github/workflows/*.lock.yml` earns the surface axis's "an operator must act on it" weight because
  `OPERATOR_FIRST` matches the CI directory before the lockfile tables are consulted). This is the
  "change what gets flagged, not the bar" work `e4.9` pointed at, and it is measured on `reco-dyt`'s corpus.

So the beta gate's fourth criterion is **carried, not met**, and the honest sentence about `m1` is still
`docs/concepts/false-positive-calibration.md` §5's: the bars stay where they are, because the alternative to
a measured failure is an unmeasured pass.

**Addendum (2026-10-20, `reco-dyt`).** The corpus follow-up landed: each pin now carries 180 episodes instead
of 60 (540 episodes, 525 mature), every one of the 66 flagged episodes has a hand-written disposition, and
`mature_rework` went 7 → 24. Criterion 1.4 is **still not met** — and now none of its gates is a refusal:
`g3-alpha-at-published-tau` measures **0.333** on 6 labelled flags instead of declining to read 3
(`not-measurable` → `fail`), the corpus cut is **0.697** (46 of 66), the miss rate is **1.000** at the
published τ and **0.708** at the decile cut, and the denominator gate is the one `expected` that moved to
**pass**. No bar moved: 0.30 / 0.15 / 0.20 and the floors are the bytes above. `reco-6co`'s finding is
unchanged and now measured three times better — nothing in 540 episodes reached τ 70 on score at all, the
highest was 63 — so the flag arm, not the corpus, is what stands between `m1` and its fourth criterion.

**Addendum (2026-10-20, `reco-6co`).** The flag-arm follow-up landed, and §1.4 is **still not met** — now with
every gate reading as a rate instead of a refusal. τ was confirmed against the three pins rather than kept:
§5.4's published default is **40** (it moved in §5.4, `risk.tau`, `thresholds.json`, the key spec and
`fathom.toml` together, which is the trio the harness checks), because the corpus's own top-decile cuts are
33/40/40 and 40 is the lowest threshold whose whole flag population a human has read. At it the flag arm fires
on **49 of 540** episodes instead of six-by-finding, and the numbers are: `G3` **0.633** at τ (bar 0.30,
**fail**), **0.672** at the corpus cut (bars 0.30 and 0.15, **fail**), miss rate **0.792** at τ and **0.750**
at the decile cut (bar 0.20, **fail**), denominator and labelling gates **pass**. No bar moved; two
`expected`s moved with their measurements, as before. What the ranking change bought is visible in the same
report: false positives blamed on the ranking 26 → **23**, corpus `G3` 0.697 → **0.672**, and the Dependabot
episode that led `cli`'s decile on a compiled workflow lock's CI claim now scores 32 and is not flagged at
all. What it did not buy is the remaining distance, and it is content rather than routing: a maintenance
refresh that re-emits five real workflow files from an out-of-tree template is a hand commit that no path,
line count or author string distinguishes from a permissions edit, and `untested_new_logic` still cannot see
ty's mdtest suites. One divergence this ticket deliberately left open, stated rather than papered: the queue
buckets the delta's `spine-1` number at 70 and `fathom why` flags `features-1` at 40, so `high` and `flagged`
are no longer the same statement — setting the queue's floor to 40 would be an unmeasured change to a model
this corpus does not score, and `queue::order::SEVERITY_HIGH`'s doc says exactly that.

---

## 2. What the dogfood found, and what got fixed here

Four defects, each found by running the shipped binary on a real repository rather than by reading the code.
Three are fixed in this commit; the fourth is `reco-b7o`.

### 2.1 The risk number a reviewer read was not the number the queue ranked by (`fixed`)

`fathom why` and `fathom export` read **one** episode; the rail, the queue and the Guardrail 3 harness read
**a page**. Both go through `risk::collect::vector`, but the single-episode path
(`detectors::collect::Reader::one`) never folded in the index's per-path rollup — which is where a file's
`add`/`del` come from, because the rollup is what the indexer reconciled against the diff. So every axis
that reads per-path churn was measured against zeros: `size`, `noise`, `test_ratio`, and half of
`deletions`.

Measured on the pinned `pallets/flask` clone, episode `ep-bba0572e320ea2f459b0` (PR 6168):

| | the harness (page read) | `fathom why`, before | `fathom why`, after |
| --- | --- | --- | --- |
| score | 63 | **52** | **63** |
| `test_ratio` | measured — "the test surface shrank: −22 against +16 on tests while source moved +3 −7" | *not measured* — "neither logic nor tests arrived" | measured, same sentence |
| `noise` | 0.89 | **0.00** — "nothing in this change is generated, locked, vendored or binary" | 0.89 — "443 of 498 churn lines sit on generated, lockfile, vendored or binary paths" |
| `size` | 166 reviewable of 498 | 498, "all of it hand-written" — about a change whose `uv.lock` alone is 429 lines | 166 reviewable, with the discount named |

That is one episode carrying two numbers depending on which door asked. It is not only the score: the export
artefact's findings come from the same single read, and `untested.rs` gates its flagship claim on
`file.add >= untested_min_added_lines` — an addition with a zero line count cannot be untested. This
repository's own newest episode went from **6 findings (4 low, 2 medium)** in the artefact before the fix to
**10 (6 low, 4 medium)** after it, the two new mediums being `untested_new_logic` on `tools/check_docs_*.py`;
the committed artefact now carries them, and the door that travels to a PR was the one that could not see an
untested addition at all. The fix is the three-line rollup in `one()`, applied exactly as `collect()` applies
it, plus `cli_why.rs::an_episode_read_alone_is_measured_the_way_a_page_read_measures_it`
(which fails without the fix, so it is a test and not a comment). The harness's own numbers are untouched:
`--check` reproduces the blessed report byte for byte, which is the difference between fixing the CLI and
quietly re-tuning the guardrail.

### 2.2 `fathom export --out review.md` wrote `review.md.md` (`fixed`)

`docs/concepts/export-artefact.md` line 170 documents the rule: *"`--out` is a stem; the format's extension
is appended, and a stem that already ends in it is used as typed."* The code checked the extension of the
name **after** appending it, so the "used as typed" branch was always taken on the doubled name and never on
the stem: `--out review.md --format markdown` wrote `review.md.md` while the report named `review.md`. A CI
step that writes `--out "$name.md"` and then posts `"$name.md"` posts a file that does not exist — the
artefact failing to land for the dumbest possible reason. Fixed, with the report now naming the file it
actually wrote, and tested for all three formats in `export_verb.rs`.

### 2.3 A contract test deleted the repository's own `.fathom/`, index and review ledger included (`fixed`)

`tests/test_storage_contract.py::test_gitignore_keeps_the_ledger_visible` probes git's ignore rules by
writing two files into `$ROOT/.fathom/` — and then, in its `finally`, `shutil.rmtree(ROOT / ".fathom")`. So
running the Python contract suite in a repository that has been indexed deleted the index *and*
`.fathom/reviews/ledger.md`: the reviewer's sign-offs. ADR-004 §12's sentence is "the store is disposable,
the ledger is not", and the suite was about to make both disposable — an uncommitted ledger is gone for
good. It bit during this very pass: a ledger written an hour earlier was gone with the index.

The test now creates only what is missing and removes exactly those paths, and leaves a `reviews/` that holds
a ledger alone. This is the kind of bug only dogfooding finds: no unit test of the ledger can notice that a
*test* eats it.

### 2.4 A half-finished corpus clone stopped the fetcher with a traceback (`fixed`)

`tools/fetch_g3_corpus.py --check` ran `git rev-parse HEAD` inside every directory that has a `.git`, which
aborts with "ambiguous argument 'HEAD'" on a clone interrupted between `git init` and `git fetch` — the
state a dropped connection actually leaves behind, and the state `reco-dyt` is going to visit repeatedly.
`--check` now answers `NOT AT PIN — want ed40e856ffec, have nothing` and exits 1, and a plain run re-clones
it.

### 2.5 Noticed, not called a defect

- **A shallow clone is told to repair itself.** `fathom index` in a `--depth 140` clone prints the graft
  correctly and then prescribes `git fsck --full`; the remedy for a graft is a deeper fetch. → `reco-b7o`.
- **Every path row says `unknown`.** `stats` prints `noise unknown 8 74` because ingest stamps the taxonomy
  as `unknown` by design until a Rust policy engine is wired in (`fathom_ingest::record::Taxonomy`'s own doc
  says so, and ADR-004 §4.7's `v_unknowns` makes it visible). Consequence for the beta: the `noise` and
  `surface` axes rank on the path-name fallback, not on e2.2's measurement. Stated, not news — but the beta
  report should say that the corpus the guardrail was measured on was classified by path name.
- **The queue's row and the guardrail's threshold are different models.** The queue prints `spine-1`, the
  guardrail and `fathom why` use `features-1`, and `queue.rs` explains why (computing `features-1` per row
  is outside a four-query frame budget). Deliberate and documented; it is also why §2.1 mattered, and why
  `reco-6co`'s acceptance criterion insists the two agree on the *flag* even while they differ on the
  *order*.
- **Nothing enumerates episodes headlessly** (`reco-11b`), so the artefact for "everything in this push that
  nobody has signed off" still needs a shell loop over revs. That is the missing piece for the CI half of the
  beta loop, and it already has a ticket.
- **Six contract tests were red before this commit started** (verified by stashing every change in it and
  re-running): `test_cli_contract.py::test_the_cli_document_names_every_degraded_state_the_code_produces`,
  `test_config_contract.py::test_starter_file_is_toml_and_names_only_known_keys`,
  `test_risk_ranking_contract.py::test_w_prints_the_arithmetic_and_the_ceiling` and
  `::test_an_unranked_episode_is_answered_rather_than_fobbed_off`,
  `test_segmentation_spike_contract.py::test_no_ignored_test_anywhere_in_the_workspace_is_unreasoned`,
  `test_workspace_contract.py::test_lint_subcommand_is_clean_on_the_repo_root`. None touch what this commit
  changes; all of them are the suite telling somebody something, which is the intake the beta loop exists to
  keep. They are listed here rather than silently passed over.
- **That list went stale, and it is re-recorded here (`reco-2vq`, 2026-10-24), because a known-red list that
  is not kept is worse than no list.** All six above are green (that re-run is intake **W43-23**). What the
  re-run found was the red had *moved*: three other tests in
  `test_risk_ranking_contract.py` were red, and nothing on the page named them — the next reader would check
  the six, watch them pass, and call the suite clean. Whole suite, binary built from this tree,
  `python3 -m pytest -q`: **2,141 tests, one red.**

  * The three queue-screen tests (`::test_the_queue_is_a_ranked_worklist`,
    `::test_the_three_riskiest_are_operator_action_work`,
    `::test_the_two_changes_that_must_not_be_missed_are_on_the_first_screen`) were red, and the queue was not
    the problem: on the 500-commit fixture it draws **27 ranked rows** on the first screen — rank column
    1…20 counted, scores descending, `order: queue-1` named in the strip, both planted must-not-miss
    changes inside the first pane. What was broken is the harness. `tui_session.drain` returns on the second
    quiet tick, and a cold TUI is *quiet* for the fraction of a second it spends opening the store with the
    terminal still untouched, so the harness typed `q` into a terminal the TUI had not taken yet and read back
    an empty screen — which parses exactly like "the queue drew 0 ranked rows". The file now waits for a
    frame before it types anything and, if none ever arrives, says *that* instead, so the two hypotheses the
    ticket named cannot be confused again; the wait for a keystroke's frame is the same idea (a keystroke
    that legitimately redraws the same pane is a no-change, not a dropped key). Time to first paint is a
    budgeted quantity — `cold.queue_visible.ms`, 3,000 ms at `small`, blessed at 26.6 ms on a release build,
    which is why the contracts job never saw this red and a local debug run could not miss it: 26 ms falls
    inside the quiet window, the ~0.6 s this box measured for a debug build's first byte does not, and nothing
    in the contract ever said which one it assumed.
    This suite reads whatever binary exists (`FATHOM_BIN`, release, else debug), and latency is the perf
    gate's contract; this file's is what the pane says once it has been drawn.
  * The one red left was
    `test_beta_loop_contract.py::test_the_generator_says_which_rung_of_the_fallback_ladder_each_bullet_came_from`,
    and it is `e7.7`'s own machinery: the changelog quotes `tools/gen_changelog.py audit`'s rung distribution
    line, and that line counts commits, so it moves on every commit that lands — the quote can only be true
    for the commit that wrote it. Filed as **`reco-0ip`** with the three ways out. Nothing in `reco-2vq`
    touches it.

    **Closed by `reco-0ip`, which took the second way out and kept the first one's honesty.** The quoted
    distribution is a generated block now, and the block *names the range it counted*: `audit --bless` writes
    it into `[Unreleased]`, `audit` counts that range again on every run and calls a mismatch `C06`, a block
    inside a released section is frozen with that release (checked, never rewritten), and the note prints how
    far behind HEAD the claim has fallen — staleness is information, wrongness is a defect, and only the
    second is allowed to be red. So the claim became "these notes were true for the tree they name", which
    is checkable on every commit forever, instead of "true as of now", which was true for exactly one commit
    and false for every commit after it. Re-run the same way after the fix (`python3 -m pytest -q`, binaries
    built from this tree): **2,140 passed, 2 skipped, none failed** — the two skips are the unchanged pair
    that need a live pi session and an installer rehearsal, and they have always skipped. The known-red list
    is empty, which is a state worth doubting: it is empty because somebody re-ran it, not because nothing
    was ever on it.

    One thing that looks like a red and is not, recorded so the next reader does not file it:
    `test_packaging_contract.py::test_the_committed_shell_artefacts_are_what_the_binary_prints` fails when the
    local binary is built with default features, because `packaging/man/man1/fathom.1` carries the
    `--all-features` version line and CI runs that check on the leg that built `--all-features` (the
    build-test job). `cargo build -p fathom-cli --all-features` makes it green; nothing in `reco-0ip` touches
    the CLI, and it failed identically with this ticket's changes stashed.

  The quiet-before-the-first-frame pattern — an initial drain that returns on the first quiet tick, then keys
  — is also in `test_diff_open_contract.py`, `test_frame_report_contract.py`, `test_review_state_contract.py`
  and `test_search_prompt_contract.py`. None of them is red today, and only this file was changed, because
  this is the file that was.

---

## 3. The loop, opened

The dogfood pass is four commands, and it is the whole loop for a repository:

```bash
fathom index                                  # or: fathom index <path-to-a-clone>
fathom why <episode>                          # why is this in front of me
fathom review sign <episode> --note "…"       # the act, with the reason committed with it
fathom export --episodes <range> --format markdown --out review.md
fathom review status                          # exit 7 until every live episode is covered
```

plus the two that make it a loop rather than a demo: commit `.fathom/reviews/ledger.md`, and file what the
pass made noise about (`bd create`, labelled from the area the noise came from). Two of the four defects in
§2 came from running exactly these commands against a real clone of an external repository.

Until `reco-ngk` lands, `fathom check` prints which ticket owns it and exits 1, so the CI half of the loop is
`fathom review status` (exit 7 while live episodes are unsigned) and nothing more; until `reco-11b` lands,
the range has to be named by a person or a script rather than by "whatever needs review".
