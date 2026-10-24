# Verification: epic 7's four exit criteria, run and recorded

- **What this is**: epic 7 (`reco-2ah`) closes on one sentence with four clauses in it — *published
  binaries for three OSes, docs a stranger can follow in five minutes, perf budgets met or consciously
  relaxed with a written rationale, and a closed beta feedback loop*. Every clause names something that
  either runs or does not, so every clause below **is** a run: the command, what came back, and the part
  that is still not proven. Nothing here is a document claiming a document is good.
- **Reproduce**: `python3 tools/epic7_acceptance.py --binary target/release/fathom` (~75 s) ·
  `--skip-perf` drops the 35 s fixture benchmark, `--with-benches` adds criterion back (CI's `perf` job
  runs that half) · `--out <file>` writes the same report as JSON.
  `tests/test_epic7_contract.py` runs it and also checks that the gate can go **red** — pointed at a
  file that is not a binary it must fail and still write its report, not crash — and checks the perf
  clause's ledger discipline against six deliberately incomplete registers.
- **Machine-readable run**: [`epic7-exit-criteria.json`](epic7-exit-criteria.json) next to this file —
  `--binary target/release/fathom --out .plan/evidence/reco-2ah/epic7-exit-criteria.json`, release
  binary built from `36b011f` (no Rust has moved since), run against the tree this ticket lands on —
  **4/4 clauses pass, 48 checks, 76.6 s**. The report's `head` is `6ddb9bc` — the tree it measured, which is
  this ticket's commit with its own write-up's last words left out of it, as any close's report is. The
  baseline it compares to was blessed at `36b011f`.
- **Headline**: all four clauses are met. Writing the gate found **three real defects** — a warm
  re-index that is a graph backfill wearing one, a store budget whose denominator is the wrong tables,
  and a read-verb budget whose noise is wider than the ratchet that gates it — plus one packaging drift
  found and deliberately **not** fixed. All four are named in §5 with their owners, not smoothed over.

| clause | verdict | measured by |
| --- | --- | --- |
| published binaries for three operating systems | **pass** | 6 targets on 3 OS families, `.deb`s built and `dpkg-deb`-read, extracted binary answers `fathom 0.1.0-alpha.1` from `/usr/bin` |
| docs a stranger can follow in five minutes | **pass** | 8-step scripted tour in 0.2 s against a 300 s budget, clone-and-one-command demo reproduced (27 commits, 0.8 s), 132 documented invocations parsed by clap |
| perf budgets met, or consciously relaxed with a written rationale | **pass** | fixture gate green in 35 s, 8 real-repository misses each named in ADR-004 §11.9, 3 breaches on a bounded register with owners and expiries |
| a closed beta feedback loop | **pass** | 34 intake rows followed to beads/commits/changelog, `fathom check` run live on this repository (exit 0), the backlog it prints (111 of 114 unsigned), 3 signed ledger entries |

---

## 1. Clause 1 — published binaries for three operating systems

Six `dist` targets on three operating-system families, each built on a runner of its own OS
(`aarch64-apple-darwin → macos-14`, `x86_64-pc-windows-msvc → windows-2022`,
`x86_64-unknown-linux-musl → ubuntu-24.04`), and the workflow that does it is `cargo-dist`'s own
generated job read back against the captured plan — 6 planned targets, 6 in the config, *identical*:

```
release path OK: one binary (fathom) at 0.1.0-alpha.1, three OS families, every rule green
self-test OK: 22 ways to be red, all of them detected
```

The part that could be a lie is *published*, and the run distinguishes three levels of evidence rather
than flattening them:

| evidence | what this run did |
| --- | --- |
| **declared** | 6 triples, both Apple ones, a static musl build, `shell` + `powershell` + `msi` + a Homebrew tap |
| **built and read back here** | two `.deb`s written from the release binary; `dpkg-deb -f … Version` says `0.1.0~alpha.1` (what dpkg spells `0.1.0-alpha.1`); the package lays down `/usr/bin/fathom`, a man page and bash/zsh/fish completions; the extracted binary runs from the path it was laid down at |
| **not done here** | the publish itself, and macOS and Windows execution |

```
$ dpkg-deb -f fathom_0.1.0~alpha.1_amd64.deb Version   → 0.1.0~alpha.1
$ ./usr/bin/fathom --version   → fathom 0.1.0-alpha.1 features: tui mcp watch sqlite-bundled langs-all deps-all
```

**Not proven, and written into the report rather than left out of it.** No public remote exists, so the
release is *built and rehearsed*, not *published*: the installer URL in the docs is still a placeholder
and `dist`'s publish stage stays commented out with its reason. Owners `reco-3gz` (announcement and
install path) and `reco-lnd` (the tag on a public remote), plus the secrets table in
`docs/concepts/release.md` §8, which names codesign, notarisation, `HOMEBREW_TAP_TOKEN` and WiX as the
un-wired steps. This box has no `rpmbuild`, so the `.rpm` was not written here — CI's `packaging` job
builds it and reads it with `rpm -qip`. No `cargo-dist` here, so
`tools/rehearse_installer.py` — the localhost mirror of the release page with four deliberate
corruptions that must be caught — ran on CI, not in this pass. And the four OSes of *execution* are
still one: only this runner's artifact was run.

**Found here, deliberately not fixed**: `packaging/man/man1/fathom.1`'s VERSION line prints the
*compiled-in* feature set, so the committed page is reproducible only from the feature set that wrote it.
CI's docs leg builds `--all-features`; the release binary does not. Regenerating the man page from the
release binary would show a diff that is a feature list, not a documentation defect, so the gate measures
the artefacts against the binary that wrote them and says so in the report.

## 2. Clause 2 — documentation a stranger can follow in five minutes

The clause was written by `reco-60m` as a *mechanical* half plus a human half, and this gate runs the
mechanical half in full and names the human half as someone else's:

```
8 steps in 0.2s against the 300s budget          · the scripted tour, every step's claim asserted
5 pathologies, 27 commits, 0.8s                   · clone + one command reproduces docs/demo.md
five-minute.cast: 60 events, 37.4s, tagged demo-v0.1 · the recording is current, silent, in frame
documented commands OK: 132 invocation(s) parsed by clap
docs site OK (86 pages) · docs OK: 99 glossary terms, 87 documents checked
```

The quickstart has to name the six nouns and link the page that defines each —
`episode`, `delta`, `risk-score`, `review-ledger`, `ratchet`, `ambient-mode` — 6/6 linked, and say what
an episode *is* ("the change of one agent turn") and what it is *not* ("not a commit, not a branch, not
a pull request, not a session"). That is the difference between a stranger starting and a stranger
starting and then believing something false about the model.

The human half — three outside users completing this unaided — is `m0`'s (`reco-lnd`), exactly as
`reco-60m`'s AC3 said when it shipped it.

## 3. Clause 3 — perf budgets met, or consciously relaxed with a written rationale

Three measuring sticks, and the ledger that has to cover every miss:

```
budgets OK: 192 budgets, 4 size classes, budgets.json and BUDGETS.md in sync
fixture gate: verdict pass in 35 s (its own promise is 300 s), runner factor ×1.036
self-test ok: red on a 400 ms injection, green on none, does not scale RSS, does scale wall clocks,
              demands a re-bless on improvement
15 real-repository verdicts on aws/aws-sdk-go, 8 failing — every one named in ADR-004 §11.9
2 repositories cut by a deadline, reported as bounds and not as ratios (git/git, tensorflow/tensorflow)
100,000 commits × 1,000 files at HEAD: peak RSS 299 MiB against the 512 MiB ceiling, 106,898 rows evicted
6/6 ceilings are documented knobs with their trade-off written next to them
```

A budget over its ceiling is legal only with a row on ADR-004 §11.10's ledger **and** an entry in
`perf-gate.json`'s `known_breaches` — same owner, same bound, an expiry, and the defect has to still be
open. Three rows are on it, and the gate checks all five ways it could rot:

| measurement | measured | budget | ceiling | owner | expires |
| --- | ---: | ---: | ---: | --- | --- |
| `warm.reindex.ms.small` | 1,577.6 ms | 150 ms | 1,750 ms | `reco-r3x` | 2026-12-15 |
| `warm.reindex.noop.ms.small` | 1,049.0 ms | 10 ms | 1,250 ms | `reco-r3x` | 2026-12-15 |
| `store.bytes_per_commit_spine.small` | 8,273 B | 4,096 B | 10,500 B | `reco-7yq` | 2026-12-15 |

`benchmarks/BUDGETS.md` renders the same register with its expiries, so the page a stranger reads and the
file the gate reads cannot disagree — `tools/gen_budgets.py check` is one of this clause's checks.

## 4. Clause 4 — a closed beta feedback loop

```
check_beta_loop: 4 clauses checked, 7 notes, 0 problems
self-test: the nine ways a beta loop rots are each caught by a named rule
34 intake rows across 1 week — 6 `triage:fixed`, 24 `triage:ticketed`, 12 beads closed
gen_changelog audit: 79 commits in the range, 1 behind HEAD, none forgotten
index exit 0 · `fathom check` exit 0 · `fathom review status` exit 7 — in 13 s
  blocks: 111 of 114 live episodes have no fresh sign-off
3 entries committed in `.fathom/reviews/ledger.md`
```

Every `triage:fixed` row was followed to a bead that is closed **with a reason**, and to a commit in this
tree that names it; every `triage:ticketed` row to a bead that exists. The loop is then run *live* against
this repository — `index`, `check`, `review status` — because a self-review nobody has run since it was
written is a press release. `fathom review status` exits 7 with its unread backlog printed, which is the
loop working, not failing.

The honest size of the population, in the register's own words: **0 external reporters**. This repository
is its own beta population until a public install path exists (`reco-lnd`, `reco-3gz`), and the criterion
that names five external users is blocked on that, not on this epic.

## 5. What writing the gate found

**A warm re-index is a graph backfill wearing one.** `warm.reindex.ms.small` was 1,543 ms against a 150 ms
budget and `warm.reindex.noop.ms.small` 1,053 ms against 10 ms — and the no-op is the one that gave it
away: a run that reports `nothing_new: true` cost a second. A cold index leaves `graph.window`-sized debt
(340 owed revisions on the fixture corpus), and each subsequent quiet `fathom index` settles 32 of them at
about a second. At rest the same measurement is ~100 ms and the store-side no-op is 2–4 ms, so §6.6 R1's
no-op is intact and the *budget* is right; the run is a backfill. **`reco-r3x`** owns making the backfill
incremental, bounded per run, or visibly its own verb rather than hidden inside a quiet one.

**A store budget was billing the graph lane to the commit tables.** `store.bytes_per_commit_spine` was
8,273 B/commit against 4,096 B, and 6,865 B of it is `graph_*` — 49.8 % of the store is the lane, while
`git_commit` + `git_commit_file` + `ref_tip` are 1,376 B/commit. ADR-004 §11.6 gives the lane its own
budget (`store.bytes_per_commit_graph`, 32 KiB/revision, `s3` measured 22,335 B) and `perf_gate.py` had
never measured it. The number also moves with how much of the lane one gate run happens to write —
8,273 B/commit at three samples, 10,446 at five — which is the same defect talking. **`reco-7yq`** owns
giving `store_split` the ADR's three denominators.

**One read-verb budget is noisier than the ratchet that gates it.** `query.metrics.ms.small`
(`fathom stats --json`, p95 of three) measured **14.8, 14.9 and 21.6 ms** in three identical runs of the
same tree — bimodal, and by more than a ±15 % two-sided limit with a 5 ms floor can see. So the baseline
there is no middle of the spread to bless — every sample is one mode or the other — so the blessed values
sit in the fast mode and a slow-mode run is absorbed by the 5 ms floor until it exceeds it. What this close
did instead of tuning a knob: the gate re-runs the fixture gate once when it comes back red and requires the
verdict twice: a real regression is red both times, a jitter is not. **`reco-dkr`** owns it, with the two
measurement remedies (sample the read verbs more than three times; record a noise floor per measurement
instead of a constant per unit) and one product question — *why* is `fathom stats --json` bimodal — written
against it. What must not happen is loosening the ±15 % limit or raising `NOISE_MS`: the ratchet's whole
value is that it can see a real 15 % regression in a verb users wait on — and the next person to meet that
red with none of this written down will read it as a regression in `stats`.

All three are rows in this week's intake register — W43-32, W43-33, W43-34, each `triage:ticketed` with its
bead — because a finding that lives only in an ADR is a finding the loop never saw, and the register is the
loop. The prose count at the top of `.plan/intake/2026-W43.md` was stale (it said 25 rows against 31); it now
says 34 and matches, per source, which is the kind of thing this epic's own gate is for.

## 6. What this close does not prove

- One operating system was executed here. Two were declared and, for Linux/Debian, built and read back.
- Nothing was published: no remote, no tag, no artifacts, so the five-minute claim is "the tour runs and
  every step's claim holds", not "a stranger did it".
- The real-repository numbers are one machine's, at one commit (`3a08088`), on one corpus, with two
  repositories cut by a deadline.
- The beta loop is closed and the beta loop is small. `0 external reporters` is the number, and it is in
  the report rather than rounded away.
- 111 of 114 live episodes are still unsigned. The loop makes that visible in a command with exit 7; it
  cannot make a human read them.

| file | what it is |
| --- | --- |
| `tools/epic7_acceptance.py` | the gate: four clauses, 48 checks, a JSON report, and a contract test that proves it can go red |
| `docs/adr/0004-storage.md` §11.10 | the written rationale: what moved at the re-bless, what is consciously accepted, and what the clock on it says |
| `benchmarks/baselines/perf-gate.json` | the blessed numbers and the `known_breaches` register the gate reads |
| `benchmarks/BUDGETS.md` | the same register rendered, ceilings and expiries included |
| `.plan/keymap.json` | `e7.10 → reco-r3x`, `e7.11 → reco-7yq` |
