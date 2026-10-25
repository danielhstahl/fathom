# The five-minute demo, scripted

- **Status**: the tour that runs itself (`m2`, **reco-by2**, on the miniature `e5.9` (**reco-2uo**) built);
  the published repository, the answer key and the recording (`e7.6`, **reco-3qr**).
- **Needs**: a built binary and nothing else — no clone of yours, no network, no configuration.
- **The claim**: eight moves over the miniature repository, from `fathom --version` to an exported review
  artefact, inside **300 seconds**, with every step asserting what it has to be true afterwards. The
  [quickstart](../quickstart.md) is that tour for a person; this page is the version that cannot go stale,
  because a machine runs it and fails loudly when a line stops being true.

```bash
cargo build -p fathom-cli                          # or --bin / $FATHOM_BIN to point at one
python3 tools/five_minute_demo.py run              # run it, print each step's verdict
python3 tools/five_minute_demo.py check            # re-run, assert, and fail if this page drifted
python3 tools/five_minute_demo.py doc              # refresh the measured table below
python3 tools/five_minute_demo.py self-test        # prove the checker can fail at all
```

By default it unpacks the miniature that ships **inside the binary** — the same pack `fathom --demo`
unpacks — into a temporary directory, so the run is reproducible on a clean machine and touches nothing of
yours. `--root DIR` points it at any repository instead; the numeric claims then fall away, because a
stranger's history is not ours to assert on.

## The tour, with what each step owes

<!-- BEGIN GENERATED: five-minute -->
| # | The move | Command | Measured | It has to |
| --- | --- | --- | --- | --- |
| 1 | **what is in the binary** | `fathom --version` |    0.0 s | the binary says which features it was built with, so a bug report starts complete |
| 2 | **index it** | `fathom index --no-progress --json` |    0.3 s | 21 commits, in milliseconds, with the cost split by phase |
| 3 | **the rail, in a terminal that is not the TUI** | `fathom episodes --plain` |    0.0 s | the same rows the rail draws, written to a log instead of painted |
| 4 | **why is this one in front of me** | `fathom why 4513a9880166 --plain` |    0.1 s | the score is arithmetic a reviewer can check, not a gauge |
| 5 | **the gate, blocking** | `fathom check --base 4513a9880166 --head HEAD --fail-on warning --no-summary --plain` |    0.1 s | four new direct dependencies are a finding, and in enforce mode the run exits 7 |
| 6 | **say a human read it** | `fathom review sign ep-4a42de295c13b5229318 --note "read on the demo; the store is fine" --plain` |    0.0 s | the sign-off is committed markdown, so it outlives the index and the rebase |
| 7 | **is anything unsigned** | `fathom review status --plain` |    0.0 s | one sign-off does not cover twenty-one episodes, and the gate says so instead of nodding |
| 8 | **hand it to the pull request** | `fathom export --episodes ep-4a42de295c13b5229318 --format markdown --out $ROOT/review` |    0.1 s | the same model, written out for the people who review on github.com |
| | **total** | | **0.7 s** | budget 300 s, from a clean clone with one binary |
<!-- END GENERATED: five-minute -->

Two of those steps are supposed to fail, and that is the load-bearing part of the design. Step 5 is
`fathom check` in `enforce` mode over the range where the miniature takes on four new direct
dependencies; it exits **7**, which is the exit code that means *a human has work to do*
([ADR-002 §7.3](../adr/0002-workspace.md)). Step 7 is `fathom review status`, still red, because one
sign-off out of twenty-one episodes is not a reviewed repository and a demo that went green on the strength
of a single `fathom review sign` would be teaching the opposite of what the ledger means. A demo whose
gate never turns red demonstrates a dashboard, not a gate.

The budget is the acceptance criterion's own five minutes, as a number in
`tools/five_minute_demo.py` (`BUDGET_S`) rather than as an adjective in a page. The miniature finishes in
well under a second on a laptop, which is the margin the criterion wants: a scripted demo that needs four
minutes is a scripted demo that gets skipped, and one that needs four minutes *on the reviewer's slow
afternoon laptop* is a scripted demo that never gets run at all.

## The demo repository

The miniature is also published as an ordinary clone, so the tour can be run against something you
`git clone` rather than something a binary unpacks for you:

```bash
python3 tools/publish_demo_repo.py build --out /tmp/signal
git -C /tmp/signal log --oneline | head     # 21 miniature + 5 exhibit + the demo's own commit
```

[`tools/publish_demo_repo.py`](../../tools/publish_demo_repo.py) assembles it from
`publish/demo-repo/` — the tour (`DEMO.md`), the answer key (`PATHOLOGIES.md`), the gate's policy
(`fathom.toml`), and the CI job that calls Fathom's reusable
[`fathom-check`](../../.github/workflows/fathom-check.yml) workflow — on top of the pack's history and
the five exhibit commits, and commits the template last. It is deliberately assembled rather than
stored: every commit up to the miniature's own tip is byte-identical to what the binary unpacks, and
`python3 tools/publish_demo_repo.py check` counts the commits above that head to prove nothing underneath
moved. The assembly also tags the result `demo-v0.1` — see [Versioning](#versioning-and-re-recording).

Three things it does that a plain copy would not:

* **the exhibits are appended, never inserted.** A churn loop needs a run of episodes on one region and a
  reformat has to be provably whitespace-only, and neither belongs in the miniature's twenty-one commits,
  which are balanced to put a row in every view rather than to be a pathology catalogue. Appending keeps
  `fathom --demo` — the first five minutes, the miniature alone — untouched.
* **`.gitignore`**, which in the miniature ignores `.fathom/` wholesale, is rewritten to ignore
  `.fathom/*` except `.fathom/reviews/`. That is [ADR-004 §12](../adr/0004-storage.md)'s sentence — the
  store is disposable, the ledger is not — made a fact in a repository somebody else clones, so
  `fathom review sign` produces something they can commit.
* **the index** is rebuilt from `HEAD` after unpacking. The pack carries no index on purpose (it would
  carry the stat data of the machine that built it), and a clone without one reports every tracked file
  as a staged deletion, which is not the first impression a demo wants to give.

Publishing it is three commands the tool prints and does not run — a repository is a remote, and a remote is
a credential this repository does not have
([the same honesty as the installer](../concepts/release.md)). Until it exists, the tour above is the
proof, and `publish/demo-repo/` is what gets published.

## The five seeded pathologies

The ticket's sentence is "each pathology deliberately seeded (churn loop, unplanned dependency, new
cycle, deleted test, reformat-only commit)". [`tools/demo_exhibits.py`](../../tools/demo_exhibits.py)
is that sentence as data: one row each, with the commits that seed it, the command that shows it, and the
text the command has to print. The same file writes the clone's answer key
([`publish/demo-repo/PATHOLOGIES.md`](../../publish/demo-repo/PATHOLOGIES.md)) and this table, and
`python3 tools/demo_acceptance.py check` runs every row against a real binary:

<!-- BEGIN GENERATED: exhibits -->
| Pathology | Seeded in | Fathom names it today? | The row a newcomer has to find |
|---|---|---|---|
| `deleted test` | the miniature (ships in the binary) | **yes** — default-on detector, `high`, and the deleted `#[test]` is quoted as evidence | the rail's `tests` column, then `fathom why` on the episode and the exported record's *Findings with evidence* |
| `unplanned dependency` | the miniature (ships in the binary) | **yes** — the gate's `dependencies` check raises one per dependency, and the run exits 7 | `fathom check`'s `dependencies` row, then the exported record's *Dependency delta* |
| `new cycle` | the miniature (ships in the binary) | **not yet** — the anchor revision is written with nodes and no edges, so no history can make it fire | read the two files (`src/core/mod.rs`, `src/api.rs`) — and then read the sentence in the exported record that says the ranking cannot see an import cycle |
| `churn loop` | the demo repository's own commits | **not yet** — the detector ships switched off, and does not fire here even when forced on | `fathom episodes --plain` lists four `retry:` episodes in a row; the fourth's diff adds back the line the second deleted (`git show` each of the four, in order) |
| `reformat-only commit` | the demo repository's own commits | **not yet** — nothing assigns `reformat` yet, so the noise axis cannot discount it | `fathom why` on the commit: the size axis says the churn is reviewable rather than discounted, and the noise axis says it found nothing generated, locked, vendored or binary |
<!-- END GENERATED: exhibits -->

**Two of the five are `named` and three are gaps, and the gaps are on purpose.** A demo that printed a
cycle finding the product cannot produce would be a demo that stops being true the first time somebody
runs it. So a gap row says what the seed is, who owns the gap, and what would close it — and the
acceptance run fails in *both* directions: a `named` row whose finding vanished (a regression) and a gap
row whose finding arrived (this page is now wrong the other way, and someone has to come and notice how
much better the product got). The criterion "a clone plus one command reproduces every documented
finding" only survives contact with reality if the findings are generated from what the binary says.

The command the rows are checked with is the one the tour hands a newcomer, run in the clone right after
`fathom index`:

```bash
fathom export --episodes 4513a9880166..HEAD --format markdown --out review.md --plain
```

That is one `fathom` command, and it is the one that *answers*: the dependency delta, the findings with
evidence, the sign-off state and the operator checklist are all in the artefact it writes. `fathom index`
is the line before it because nothing in the product indexes on its own behalf except `fathom --demo`
— `fathom export --reindex` is the flag that would make the criterion literal, and it is not built. What
the gate adds on top — the exit 7, the six checks, the CI annotations — is the tour's step 5, and it is a
second command because it is a different question: *may this merge*, not *what happened*.

## The recording

[`five-minute.cast`](five-minute.cast) is the tour, recorded: asciicast v2, 100×28, **38 s against a
300 s budget**, and with no audio track to miss — every step prints its own caption line, so it reads with
the sound off or with the terminal muted on a plane.

```bash
asciinema play docs/demo/five-minute.cast          # or: asciinema upload, or drag it onto asciinema.org
python3 tools/record_demo.py record                # re-record it: runs the real tour, writes the real bytes
python3 tools/record_demo.py check                 # the gate: length, no audio, every exhibit still in frame
python3 tools/record_demo.py gif                   # render to GIF, if a renderer is installed here
```

The cast is *made*, not typed: `tools/record_demo.py` builds the demo repository into a temporary
directory, runs each step as the real binary, and writes the frames with the elapsed times it measured,
plus a caption and a pause per step so a person can read it. It refuses to write a cast over the budget
or under the exhibits — `check` re-reads the committed file, strips the escape sequences, and fails if
any documented finding string is missing from what the terminal actually printed. It also re-derives the
length and refuses this page and the README if they quote a different number, because "under five minutes"
is the criterion and a remembered number is not evidence of it. A GIF is the same cast
through `agg`, or `asciinema cat | gifski`; neither is installed on this repository's runners, so the GIF
is derived on the machine that wants one and the cast is the checked-in artefact.

There is a second recording in this directory, [`README.md`](README.md)'s TUI walkthrough; it is Epic 1's
acceptance run over a 500-commit fixture and it stays what it was. This one is the five-minute tour.

## Versioning and re-recording

The acceptance criterion is "demo repo version tagged and re-recorded at each minor release", so both
halves are a command rather than an intention:

| Half | The command | The gate |
|---|---|---|
| tagged | `python3 tools/publish_demo_repo.py build --out DIR` writes and pushes nothing but tags `demo-v0.1`, derived from `[workspace.package] version` | `publish_demo_repo.py check` fails if the assembled repository carries any tag but that one |
| re-recorded | `python3 tools/record_demo.py record` after the tag moves | `record_demo.py check` fails if the committed cast predates the current tag, is over budget, or has lost an exhibit |

Minor, not patch: a patch release that changes nothing the tour shows must not re-record a demo, because a
timestamp on a recording that did not change is worse than no timestamp. The step is in
[the release checklist](../concepts/release.md) beside the other things that only happen at a minor
release, and the cast's header carries the tag it was recorded at, which is what `check` compares against
— a recording that predates the current tag is a stale recording, and saying so is cheaper than remembering
to.

## What the tools are for

Five scripts, one per claim, because "the demo is honest" is not a thing a person can keep true by hand:

| Tool | The question it answers | CI |
|---|---|---|
| `tools/five_minute_demo.py` | does the scripted tour still run, on the miniature the binary ships, inside 300 s? | `run`, `check`, `doc`, `self-test` |
| `tools/publish_demo_repo.py` | does the publishable clone still assemble — miniature untouched underneath, exhibits on top, one tag? | `check` |
| `tools/demo_exhibits.py` | is the answer key still what the catalogue renders, with an owner and a remedy on every gap? | `write`, `check`, `show` |
| `tools/demo_acceptance.py` | does a clone plus one command still reproduce every documented finding, and is every documented gap still a gap? | `run`, `check`, `self-test` |
| `tools/record_demo.py` | is the film still under budget, silent, current with the tag, and does it show the exhibits? | `record`, `check`, `gif`, `show` |

The first two are `reco-by2`'s; the last three are this ticket's, and they are deliberately separate because
they fail for different reasons: `demo_acceptance` fails when the *product* changes what it says about a
pathology, `demo_exhibits` fails when a document drifts from the catalogue it is generated from, and
`record_demo` fails when nobody re-ran the thing that shows it.

## What this is not

Not the TUI walkthrough — that is [`README.md`](README.md) in this directory, a recorded asciicast with
per-step frame timings. Not a benchmark: the timings in the table above are wall clock on whatever machine ran the
check, which is why `check` compares this page's numbers with slack (a step more than five times slower
than documented is a complaint, one that is a little slower is a Tuesday) and compares the *claims*
byte for byte.
