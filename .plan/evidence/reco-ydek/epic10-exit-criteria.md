# Verification: epic 10's constraints, run and recorded

- **What this is**: epic 10 (`reco-ydek`) retires two things — the risk-ranked attention queue as the home
  view, and the printed `human`/`agent` badge — and its description carries five constraints rather than a
  sentence to bless. Every line below is a command that ran on the tree this note lands in, or a diff read
  back from the five epic commits. Unlike epic 6/7/8 this close ships no new gate program: the epic's
  subject is a removal, and what keeps a removal honest already exists — `tools/check_docs.py`'s
  retired-token table (10 tokens, page-exempt in both directions, shipped by `e10.3`), the four
  disposition acts' own tests, and the goldens. What was missing was that the *prose* and two **other**
  epics' gates had not been told, and that the two style gates every PR must pass had been red since the
  first of the five (§6).
- **The five commits**: `81eba3b` (`e10.1`, decision) · `dd29158` (`e10.2`, the acts onto the rail) ·
  `a40a8c4` (`e10.4`, the label out) · `22148e6` (`e10.3`, the deletion) · `59224cb` (`e10.5`, the vector).
  History order, not ticket order: `e10.4` landed before epic 8's `e8.6`, which is why §5.2's rail sentence
  could be re-written back into a stale shape after the badge left.
- **Reproduce**: `cargo test --workspace --all-features` (189 test targets, green) and both of `ci.yml`'s
  clippy invocations (§6 below), then the §14 block of
  ADR-006 — `cargo test -p fathom-tui --test timeline_acts` (14), `--test triage_vocabulary` (5),
  `--test session_arc` (3), `cargo test -p fathom-store --test triage_ledger` (8),
  `cargo test -p fathom-cli --test review_verb` (12), `cargo test -p fathom-tui --test status_strip` (8),
  `python3 tools/check_docs.py`, `PYTHONPATH=src python3 -m recommender.tui doc --check`. Note for anyone
  repeating this in a container: this checkout's volume makes `rustc` die with `SIGBUS` while writing
  archives, and the final link of the `fathom` binary is `ld` getting `SIGKILL`ed at the default job count,
  so every build here ran with `CARGO_TARGET_DIR=/tmp/ftarget cargo … -j 2`; `env/` is a macOS virtualenv
  that does not run here, so the pytest suite needs a system interpreter with `pytest` importable (on this
  one: `pip install --target=/tmp/pylibs pytest`, then `PYTHONPATH=/tmp/pylibs`), and the two acceptance
  gates that shell out to the binary need `FATHOM_BIN=/tmp/ftarget/debug/fathom`, since they look for
  `target/{release,debug}/fathom` inside the repository.

| constraint (the epic's own words) | verdict | measured by |
| --- | --- | --- |
| ADR-006 §5.1 and ADR-001's attribution stance are amended *before* anything is deleted | **pass** | `81eba3b` changed no code: it adds ADR-006 §5.1.2 and thesis Amendment 3, and the four execution tickets cite them. Both pages now read as landed, not prospective — see "prose" below. |
| §5 layouts, `docs/reference/cheatsheet.md` and the generated keymap move with the code | **pass, after this commit** | `python3 -m recommender.tui doc --check` and `check_docs_site.py` were already green; what was not true was the hand-written prose around the generated blocks (§14's commands, six "nine views" counts, §5.2's row). All corrected here. |
| the review ledger is not retired; write-before-state-change keeps its honour wherever the acts live | **pass** | `git log -S` and `--name-only` over the five commits: **`crates/fathom-store` is touched by none of them**, `spine::SCHEMA_VERSION` is still 12, and `triage_event` / `review_state` / `ledger_entry` keep their names. The rule now lives in `crates/fathom-tui/src/review.rs` (`Ledger::record` before any shell mutation, `Landed::{Recorded,InMemory,Refused}`) and its three proofs walk it from the rail: `accept_writes_before_it_marks_and_the_row_reads_accepted_in_the_same_frame`, `a_ledger_that_refuses_keeps_the_row_exactly_where_it_was`, `with_no_ledger_an_act_is_honest_about_living_only_in_memory`. |
| the timeline row keeps everything it has today except the badge; the `d` diff view does not change | **pass** | `git log -- crates/fathom-tui/src/diff` over the epic range names **no epic-10 commit** (the only commit in range that touched it is `e8.6`). The one diff golden that moved (`frame_mixed.txt`) changed one line, the shared status strip: `queue unknown open` → `unread unknown`. The rail row diff: `▸ e8f1a2 0:12 agent +412 −90 ●●●●●● ? ⚑` → `▸ e8f1a2 0:12 +412 −90 ●●●●●● ? ⚑` — state, sha, age, churn, sparkline, complexity and risk all intact, six freed cells spent on `Columns::for_width`'s thresholds (37/34/28/26/22) and on nothing else. |
| every child registers its plan key and ships its own `[Unreleased]` entry | **pass** | `.plan/keymap.json` maps `e10`→`reco-ydek` and `e10.1`–`e10.5`; `python3 tools/gen_changelog.py audit` is green over all 170 commits since the last release, and each of the five has its own heading. |

---

## 1. What no child owned: the page that lists the acceptance commands

ADR-006 §14 is titled *"Verification — the commands that **are** the acceptance criteria"*. Three of its
commands targeted the deleted pane and one named a test file that no longer exists:

| §14 told the reviewer to run | what happened | now |
| --- | --- | --- |
| `recommender.tui layout queue` | `unknown view 'queue' (known: timeline, diff, …)` | `layout timeline` (§5.2 is the layout) |
| `recommender.tui fit queue 60x12` | same refusal, exit code 0 doing nothing | `fit timeline 60x12`, which reports `state degraded` and its fired steps |
| `recommender.tui press "q <Enter> x"` | `q … -> (unbound)` — the letter is the user's since `e10.3` | `press "t j <Enter> d x"`, the published T1 sequence |
| `cargo test -p fathom-tui --test queue_triage` | no such file; `e10.3` split it — the acts went to `timeline_acts.rs` in `e10.2`, the vocabulary stayed | `--test triage_vocabulary`, the half that outlived the pane |

Six more places still counted **nine views** after the inventory went to eight (§6's opening, §6.2's intro,
§10.3 twice, §12's rejected alternative, §14's AC table "§5.1–§5.9: nine generated layouts", plus
`docs/concepts/onboarding.md` §1). §5.1's header note still said everything §5.1.2 decides "is prospective
until `e10.2`–`e10.5` land". And §5.2 — the *protected* rail row — still described the width ladder as
dropping `author`, a rung `Columns::for_width` has not had since `e10.4`; the same sentence had complexity Δ
and the sparkline inverted, a drift from `e8.6` (`870dd48`), which added the complexity rung and wrote the
sentence one commit after the badge had already left the row. The shipped order is complexity Δ → sparkline
→ date-only → risk number → identity, and `carries_complexity` says why complexity surrenders first.

Prose that described the *plan* rather than the build was rewritten as landed, in the same style the
children used for their own `*As landed (…)*` notes: ADR-006 §5.1.2(c) ("until then `fathom why` prints
`author 8`"), §9's T1 note ("`q` still standing for a pane that is on its way out"), thesis Amendment 3
item 4 and the **Risk score** glossary entry (which still listed *attribution confidence* as a vector
input — a glossary is normative, not a record), and `docs/concepts/timeline.md` §14 ("`e10.5` replaces what
the risk number is computed from … which §8's line still spends points on").

## 2. Code documentation the facade rename walked past

`e10.3` renamed `fathom_analyze::queue` → `fathom_analyze::review` and left two intra-doc links pointing at
the deleted module — `ledger/mod.rs`'s "the four acts … are [`crate::queue`]'s" and
`risk::weights`'s `DEFAULT_TAU` doc, which named the severity floors as `super::super::queue`'s. Neither
is a compile error and nothing in CI runs `cargo doc`, which is exactly why they survived; `cargo doc -p
fathom-analyze --no-deps` prints **252 unresolved-link warnings (202 distinct targets)** in this crate
alone, a backlog that predates this epic and is not this ticket's to bless — the point of fixing these two
is that they name a module this epic deleted, not that the count goes to zero. `cargo doc` output after
this commit names no `queue` link.

The plugin-facing finding schema went further: `Jump.view`'s doc advertised `queue` as one of `view`'s
legal names, while `view::ALL` is `[timeline, diff, structure, graph]` — a plugin naming `queue` gets
`None` from `resolved()`, which is rule R7 refusing to guess rather than a dead pane's redirect. The doc now
says which names exist and that `queue` is one this build does not have.

## 3. Two gates that were red *because of* this epic's arithmetic

Both were found by running the whole pytest suite, and both reproduced with this commit's diff stashed —
they are epic 10's fallout, not this commit's.

- **`tests/test_epic6_contract.py` → `tools/epic6_acceptance.py`**, clause *"an MCP session where the agent
  queries 'what did I just change and is it risky'"*. Its first check was
  `` `risk_report` ranked this change first ``. The fixture is three human commits rolled into one episode
  plus one agent turn; on `features-2`/`spine-2` the agent turn collected the author term (8 of 100 on the
  spine, up to 0.04 of 1.00 on the vector) and that is what held it top of the report. `e10.5` deleted the
  term, and the report's own numbers say what is left: on a reproduction of the fixture the agent's turn
  scores 41 (`spine-3 41 = surfaces 36 + breadth 2 + size 2 + complexity 1`) and the human roll-up 40 — a
  point apart, decided by the change rather than by who signed it. The clause was re-stated to what a
  clause about *is it risky* is about: the report answers about the change that was asked about (row found
  by the id `explain_episode` returned, not by position), it is ordered by the scores it prints, and that
  row carries a score, factors, evidence and a named model. Gate: **3/3 clauses**. ADR-001's decision is
  the reason this is a re-statement and not a regression: a label no reviewer is shown may not steer what
  they see, and a score is a printed claim.
- **`tests/test_epic8_contract.py` → `tools/epic8_acceptance.py`**, check *"every change this epic made to
  a protected golden names itself"*. `golden_audit()` ran `git show --format= -- <golden dirs>` **with no
  commit argument**, so `git show` diffed `HEAD` and every one of epic 8's commits was audited against the
  same wrong diff. It passed at epic 8's close for the reason vacuous checks pass: the close commit touched
  no golden. Epic 10 re-blessed 24 protected goldens, and the audit began reading those as epic 8's
  unexplained changes — 216 lines attributed to `e8.7`. Passed each commit's own sha, the audit reads
  **6 changed goldens, 0 unexplained**. Gate: **6/6 clauses, 50 checks**.

Neither fix is a blessing of a new number by hand: epic 6's expectation is re-derived from the report the
binary returns now, and epic 8's is a `git` invocation corrected to match the function's own docstring.

## 4. Named as owed, and now held as tickets

Written down by `e10.3` in ADR-006 §5.1.2 §7 and in `docs/concepts/review-ledger.md`, re-stated here, and
now carried by two beads so they cannot rot in prose — **`reco-o884`** and **`reco-15wk`**:

1. **The byte-frozen pre-deletion store fixture.** The check ADR-006 asks for is a store written *by the
   build that had the queue*, read back by this one: every act with its disposition, actor, reason, note,
   snooze condition and stamped risk model, `triage_event_count` unchanged, and `fathom review sync` /
   `verify` / `status` showing a reviewer with forty sign-offs still holding forty. What ships instead is
   the structural argument plus every check that needs no frozen file — no table moved, `SCHEMA_VERSION`
   did not move, `triage_ledger.rs` round-trips each field, `review_verb.rs` drives sync/verify/status over
   a store that carries acts. The residual risk the fixture would cover is *future*: a later migration
   silently breaking old stores. It is owed because the repository commits no `.sqlite3` and a file claimed
   to predate the deletion has to be produced by the build that predates it — a fixtures decision
   (`docs/concepts/fixtures.md`), not a line in a docs commit. **→ `reco-o884`.**
2. **The perf budget keys that name the dead pane.** `cold.queue_visible.ms` and `query.queue.ms` (and
   `tools/gen_budgets.py`'s description of the latter as "the attention queue's ordering query
   (`v_queue`)") outlived both the pane and the view: `v_queue` exists nowhere in `crates/` any more, and
   the probes press `t` and `j`. Renaming them is a re-bless of every number on the perf baseline and needs
   a release build on a representative runner — `tools/perf_gate.py`'s blessed values are not transferable
   between machines, as `reco-3h82` and `reco-0zot` both learned. **→ `reco-15wk`.**

## 5. The state of the suite around this commit

`python3 -m pytest -q` on this container: **8 failed, 2295 passed, 10 skipped**. Two of the eight were
recovered by this commit (epic 6 and epic 8, §3); the other seven each reproduce red with this commit's diff
`git stash`ed and re-run in that stashed tree, so none of them is this commit's. The eighth,
`test_epic7_contract`, is that suite's own packaging clause — the same man-page/man-page-vs-binary check the
`test_packaging_contract` row below reproduces with the diff stashed, plus a `target/release/fathom` this
container has no release build of. In brief, with owners:

| red test | why | owner |
| --- | --- | --- |
| `test_workspace_contract::test_ci_builds_on_all_three_operating_systems_and_both_channels` | demands `macos-latest` in `ci.yml`; `59a0079` removed the macOS/windows legs | `reco-374v` |
| `test_workflows_contract::test_the_ignore_matcher_agrees_with_git` | `.gitignore` read two ways on `crates/fathom-cli/target/x` | `reco-374v` |
| `test_beta_loop_contract::test_the_changelog_has_an_entry…` | the reader splits `[Unreleased]` on a literal that `c801092` quoted inside a changelog body | `reco-s6ge`'s successor |
| `test_packaging_contract::test_the_committed_shell_artefacts…` | the man page's VERSION line carries the *compiled* feature set, so it is only reproducible from the build that wrote it | `e7.4`, named by epic 7's own gate |
| `test_epic7_contract::test_the_four_exit_criteria_hold_end_to_end` | needs `target/release/fathom`, plus the man-page item above | environment + `e7.4` |
| `test_perf_budget_contract::test_the_gate_reports_the_changes_it_could_not_classify` | a debug build measured against blessed release baselines (`cold.index.ms` 6 642 vs 1 500) | environment |
| `test_pending_chord_contract::test_every_view_key_paints_inside_one_frame` | `d`, `g`, `o` at 16.6–19.6 ms against a 16 ms frame budget, on a debug build in a container | environment |
| `test_mcp_contract::test_with_writes_on_each_call_is_audited_and_a_token_is_bound` | its fixture's `search_history {"query": "helper"}` returns no second result group here; unattributed, and it is red with this diff stashed | needs a ticket of its own |

## 6. The two style gates every PR must pass, which nothing on this branch ran

ADR-002 §2 makes `cargo fmt --check` and clippy-strict hard failures and `ci.yml` runs both as gating jobs,
but nothing local enforces them: `check_workspace.py` only matches the §2 *table row* that claims the gate
exists, the repository ships no git hook, and `tools/release.py` runs plain `cargo fmt` as release step 12,
which would have folded the entire reflow into a release commit attributable to nothing. Measured commit by
commit over the epic range, the tree was fmt-clean at `e10.1`'s parent (`0762b23`) and is clean again on this
commit; the debt is mostly this epic's own — `c25642e` and `957730a` (epic 8's complexity work, landing
between `e10.2` and `e10.3`) took it to **4** sites, `e10.3`'s rename of `queue` → `review` and its split of
the queue's tests took it to **36**, `e10.5` left **28**, and `cargo fmt --all` here clears all 28 across 20
files, every one an import re-flow or a re-wrap and none of them behaviour (`fathom-tui/src/lib.rs` lists
`pub mod render` after `pub mod review` only because `review` is what `queue` was renamed to, and rustfmt
sorts module lists).

Clippy needed the two invocations `ci.yml` actually runs, verbatim, and nothing else:

```bash
cargo clippy --workspace --all-features -- -D warnings -D clippy::unwrap_used -D clippy::expect_used
cargo clippy --workspace --all-targets           -- -D warnings -A clippy::print_stdout
```

Both were red, on **six** sites, and `git blame` puts five of them in code `e10.3` wrote or split out of
`queue_triage.rs`:

| site | lint | fixed as |
| --- | --- | --- |
| `fathom-tui/src/timeline/rail.rs`, the strip under the rail | `collapsible_if` — `if self.risked > 0 { if let Some(model) = … }` | a let-chain, `if self.risked > 0 && let Some(model) = …`, the form the rest of the crate uses |
| `fathom-cli/src/ambient.rs`, the open-work row | `useless_conversion` — `.into()` on a value that was already a `Disposition` | the `.into()` deleted; the field and the accessor are the same type since `e10.4` took the label out |
| `fathom-tui/tests/triage_vocabulary.rs` | `iter_nth` — `.iter().nth(1)` on a `Vec` | `.get(1)`, which is also the fallible shape the assertion wants |
| `fathom-tui/tests/triage_vocabulary.rs`, module doc | `doc_overindented_list_items` — a continuation line at 5 spaces in the list that says what survived the split | 3 spaces, matching its siblings |
| `fathom-tui/tests/risk_rank.rs` | `op_ref` — `word == &score.to_string()` | the stray `&` dropped; the reflow neither created nor hid it |
| `fathom-analyze/src/risk/collect.rs` | `cloned_ref_to_slice_refs` — `&[row.id.clone()]`, written by `957730a` and re-flowed by `e10.5` | `std::slice::from_ref(&row.id)`, which is what every other single-element slice call in the workspace already writes |

Both invocations are green on this tree, and `cargo test --workspace --all-features` is green after them
(189 targets) — the two edits that touch behaviour at all (`.get(1)`, dropping `.into()`) are type-preserving
and the goldens did not move.

`cargo test --workspace --all-features` is green (189 targets), as are `check_docs.py` (90 documents, 99
glossary terms, 10 retired tokens held to their record pages), `check_docs_site.py` (89 pages),
`check_workspace.py check`/`lint`, `check_delta_compat.py check` (delta contract v2), `cargo fmt --all
--check`, and `gen_changelog.py audit`/`lint`.
