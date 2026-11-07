# Changelog

All notable changes to fathom are documented here. The format is
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), the versions are
[Semantic Versioning](https://semver.org/spec/v2.0.0.html), and — because a toolchain floor is a promise
to whoever packages this — a dropped MSRV is a **Minor** change recorded under its own
[`Toolchain`](docs/adr/0002-workspace.md) heading, per ADR-002 §2.

This file is also the release-notes source `cargo-dist` reads when it builds a GitHub release body, so
an unreleased section that describes nothing is a release body that says nothing. Keep it true, keep it
short.

Rules of the road, now that there are versions: work lands under `[Unreleased]`; cutting a release renames
that heading to `## [<version>] - YYYY-MM-DD` and starts a new empty one. `python3 tools/check_release.py`
fails CI if the version in `Cargo.toml` has no dated section here, because the section *is* the release
body — see [docs/concepts/release.md](docs/concepts/release.md).

## [Unreleased]
### The search budget went red on a two-core runner, over a letter set recomputed half a million times a keystroke (`reco-sr1q`)

`build+test (linux · stable)` failed the latency test with:

```text
p95 52.98548ms is over the 50ms budget ADR-006 §5.8 sets for `query.search.ms`
```

It was both of the ticket's hypotheses at once. The runner is one of the two-vCPU ones and the box this
was written on reproduces the failure only under `taskset -c 0,1`; but the scan was also doing work it
did not need, and the margin it needed for a slow runner was not there. What the corpus of measurements
said: one keystroke out of the eighteen in the budget test — `graph render cache patch`, four words,
none of them a substring of anything in the fixture and every one of them a *subsequence* of every
episode body — costs several times the median, because all 14 339 rows are hits and none of them can be
refused by the letter masks. That is the worst keystroke the box can be asked for, which is what a p95 is
for; the fixes are about what one row costs, not about which rows are looked at.

* **A token's letter set was re-derived per row per field.** `Letters::holds(token)` starts by computing
    the token's letters, and it was asked per field, per token, per row: six fields over fourteen
    thousand rows is half a million byte tests per keystroke that all answer the same thing.
    [`score::Token`] now compiles each typed token once per search and rows are asked through
    `Letters::holds_set`. This was the largest single cost in a keystroke.
* **The field walk read the long field first.** A token was scored against name, body, then the four short
    fields — so the 230-byte episode body was walked before the hundred bytes of paths, dependencies,
    detail and author, for every token, in every row, and the only early exit was a name prefix (500).
    The walk now follows the weight ladder in `score::field_score`: the name whole, then a secondary field
    that *is* the token (a compare, not a walk), then the body as a substring, then a secondary field that
    contains it, then the subsequence rungs — and it stops as soon as the match in hand is worth at least
    what everything unread could pay. That cannot move the ranking: the body pays `160`/`120`/`20` and no
    secondary field pays any of those, so the only ties are between secondary fields, whose relative order
    the walk keeps exactly as the hit explanation prints them.
* **Two byte-at-a-time loops went to `core`.** `find` was a hand-rolled first-byte scan (the comment said
    `str::find` was too expensive to call once per field per token) and `subsequence` was an iterator chain
    over bytes. `core`'s searcher is compiled at the standard library's optimisation level, not the
    caller's, so at `-C opt-level=0` — which is where this budget is measured — it is 2.3× faster to call
    it than to write the loop here, and still 1.8× faster in release; `subsequence` is now one `memchr`
    per letter of the needle from where the last one stopped, 1.6× faster in a debug build and the same
    greedy leftmost walk as before.
* **The report sorted everything to show five rows.** A broad query scores ten thousand rows and prints
    five, and ordering the other ten thousand measured 3.5 ms of a keystroke. The best `limit` are now
    seated with `select_nth_unstable_by` and only those are sorted — legal because the tie-break rank makes
    the order total.

Measured on the same two vCPUs, debug build, the same corpus: median **19 ms → 6.1 ms**, p95
**56.7 ms → 30.0 ms** against the 50 ms budget (the runner that opened the ticket was about 7 % faster
than this one, so read the CI number as ~28 ms). The same test with `--release` measures a 0.9 ms median
and a 3.8 ms p95, which is worth saying out loud: ADR-006 §5.8 names *this test* as the measurement of
record, and a debug build is about eight times stricter than the binary anybody ships. The budget is
unchanged, and so is the golden ranking — this was a cost change, and `the_ranking_is_the_golden_one`
reproduces `tests/golden/search-ranking.txt` byte for byte.

Two more things came with the fix. The failure now **names the keystroke it tripped on** (median, p95 and
worst each carry their query, in the assertion as well as the trace line) because a budget that fails
silently is a ticket that starts with "this could be a slower runner" — the `reco-axp` lesson again. And a
new unit test, `the_rung_walk_and_the_whole_ladder_pick_the_same_field`, checks that phase one's cheap rung
walk and phase two's whole-ladder walk pick the same field at the same offset for the same score on every
row of the fixture — with the fixture itself asserted to reach every rung of the ladder, so the test cannot
quietly stop covering one.

Not fixed here, and unrelated: `fathom-cli`'s `search_verb::a_saved_query_is_named_in_the_next_process`
fails on this tree before these changes and after them.


### The workflow gate was red in CI and green on every laptop, over a file CI has never had (`reco-8myi`)

`docs + workspace contracts` was red at the step that runs the workflow gate, with a complaint about a
path in a workflow that is not wrong:

```text
P01 perf: job `budgets` step 9 (the trend, for the run summary): names `benchmarks/trend/trend.md`, which does not exist
```

Both commands the step runs — `check_workflows.py --verbose` and its `self-test` — pass on any machine that
has ever run the perf gate, and fail on a fresh clone (`git clone` the repository and try it: the whole
gate has one problem, and it is this one). The gate was asking *"does this path exist"* of the working
tree, and the tree of whoever last ran `make bench-check` is not the tree `actions/checkout` produces.

* **`benchmarks/trend/trend.md` is the run's own output.** `perf.yml` writes it twelve steps earlier in
    the same job, guards the step that reads it with `if [ -f … ]`, uploads it as a job artefact, and
    `.gitignore` excludes `benchmarks/trend/` on purpose — *"the trend is a CI job artefact, not a
    committed file"*. So `P01`'s one question had exactly one wrong answer in the file set, and it was the
    answer that only a laptop could give.
* **`P01` and `P02` now ask about the checkout, not the working tree.** A named path is fine when it is
    here, and also when `.gitignore` says it is not part of the repository: an ignored path is build or run
    output, and requiring one to be committed is a rule that flips with the weather — green where somebody
    built, red where nobody did. A typo is still red, because `tools/no_such_tool.py` is not ignored by
    anything.
* **`.gitignore` is read by the gate itself, not by `git`.** The gate's whole argument is that it runs
    offline with no toolchain, so it parses the ignore file with the standard library: anchored versus
    basename patterns, `*`/`?` that stop at `/` and `**` that does not, negation, and git's rule that an
    excluded directory settles everything beneath it. `tests/test_workflows_contract.py` checks those five
    shapes against rule lists written inline, and against git where git is available, so a rewrite of
    `.gitignore` cannot make the exemption mean something else.
* **The exemption is falsified, not assumed.** The self-test now writes a workflow whose steps name two
    paths under an ignored directory in a stand-in `.gitignore`, requires that `P01` stay quiet about both
    — and fails if the stand-in ignore file yields no rules at all, because an exemption that was never
    consulted reads exactly like a passing one. The contract suite asserts the exempted paths are absent
    from this repository, so the check cannot be satisfied by a stray build file.
* **Found next door and fixed:** four `read_text()` calls in the gate (the root `Cargo.toml`, the three
    workspace manifests and `dist-workspace.toml`, all of which contain non-ASCII) inherited the process
    locale, so a runner without a UTF-8 `LC_CTYPE` — `PYTHONCOERCECLOCALE=0 LC_ALL=C`, the shape a bare
    cron or container gives you — stopped the gate with `UnicodeDecodeError: 'ascii' codec can't decode
    byte 0xc2` before it read a single workflow. They say `encoding="utf-8"` now.

### `e2.4`'s own re-measure could only reproduce on the computer that wrote the report (`reco-2ezf`)

`docs + workspace contracts` was red on every push at the `e2.4 rename pairing … re-measures to the
committed report` step:

```text
CHECK FAILED — the committed report disagrees with this run:
  ! totals differs from the committed report
```

Every one of the 30 gates passed, and every count in `totals` was identical. Two fields moved, and both
were wall clock: `ms_total` 17.3 against 16.6, `ms_mean` 0.468 against 0.447.

* **The pooled totals carried the clock.** `pool()` sums the per-window pairing time into the same block
  it pools the counts into, and `check_report` compares that block whole — so a measurement that
  reproduced exactly was "drift" on any computer but the one that last wrote the report. Spike `s4` had
  been given this rule hours earlier (`reco-v1z`): runner facts are recorded, printed, and not compared.
  `pool()` no longer writes milliseconds into a compared block; the timing is still measured and still
  reported, one row under `rows[]` and `per_scenario[]`, which is where a millisecond belongs — next to
  the window it was taken on.
* **A gate's expectation is a claim made before the run, and `A4b`'s was filled in from the run.** The
  refused O(repo) mode's cost ratio was asserted `>= ` *the shipped mode's own measured ratio*: the
  committed report said `>= 2.69`, the next run said `>= 2.3`, and it was true on every machine by
  construction because widening always reads more. It is now a number written in advance
  (`rename.eval.DECISIVE_COST_RATIO`, 2.0), which the widened mode clears at 18.73.
* **The failure output still names the claim, not the machine.** A doctored `totals.found` fails the
  check; a `ms_total` of 900 000 does not. Both halves are pinned by a test, because a loosened
  comparison that cannot fail is the same bug wearing the fix.

### The binary stopped building on Windows, over a signal that platform does not have (`reco-wgcj`)

`cargo build --workspace --all-features` is a CI step on `windows-latest`, and it died in the daemon's signal
setup, before anything of the product was compiled:

```text
error[E0425]: cannot find value `SIGHUP` in module `signal_hook::consts`
   --> crates/fathom-cli/src/daemon.rs:620:35
```

* **`SIGHUP` is the controlling terminal going away, and a Windows console is not a controlling terminal.**
  `signal-hook` exports, on Windows, only the signals the C runtime can actually be told about — `SIGINT`,
  `SIGTERM`, `SIGABRT`, `SIGFPE`, `SIGILL`, `SIGSEGV`, plus `SIGBREAK` — so `SIGHUP` is not a handler that
  never fires there, it is a name that does not exist. Closing a console is a `CTRL_CLOSE_EVENT`, which the
  C runtime does not turn into a signal at all.
* **The stop table is built per platform now (`Signals`/`stop_signals`), not filtered at run time.** `SIGINT`
  and `SIGTERM` set the stop flag everywhere; `SIGINT` and `SIGTERM` are hooked on Windows too, so Ctrl-C and
  a supervisor's stop still land in the same settle `fathom daemon stop` uses. `SIGHUP` is added on Unix, for
  both the stop flag and the hang-up flag. The one thing Windows gives up is an exit-code claim: a daemon whose
  console is closed leaves with it instead of exiting `Cancelled`, which is the platform's own answer and the
  closer truth — there is no `SIGHUP` to name as the reason.
* **The same build failure was waiting one layer down, in the tests.** `cargo test --workspace --all-features`
  compiles every test target on every OS in the matrix, and `crates/fathom-ingest/tests/daemon.rs` and
  `crates/fathom-cli/tests/daemon.rs` both drive a daemon *over its socket*, which is the transport
  `fathom-ingest` already says it cannot bind off Unix (a named pipe needs the FFI `#![forbid(unsafe_code)]`
  rules out). They are `cfg(unix)` now, with the reason next to the gate rather than in a ticket nobody
  re-reads; the one `fathom-tui` terminal test that asserts "`SIGINT`/`SIGTERM`/`SIGHUP` are hookable here"
  went the same way, and its import with it.
* **What is *not* claimed:** that the Windows test suite passes. This ticket is the build. What was verified is
  `cargo check --workspace --all-features --all-targets --target x86_64-pc-windows-msvc` clean, the same
  invocation on the host clean, and the three affected test binaries green here (`fathom-tui --test terminal`,
  `fathom-ingest --test daemon`, `fathom-cli --test daemon`) — so the Unix behaviour the gates moved is known
  not to have moved.

### An open frame kept naming the branch it had opened on, long after the reviewer had left it (`reco-nup`)

A `fathom` shell open in the second terminal printed `myrepo · main` in the tab bar while the first terminal
was three branches away from `main`. The label is read once, before the first frame, by the composition root
— which is the only party allowed to open git (ADR-002 §5.2, lint `E01`) — and nothing read it again. Every
other fact in the frame arrives off the producer `reco-bpc` installed; the branch was the one fact it never
re-read, so the frame's own claim about *which state of this repository I am showing you* went stale in
silence, underneath a status line that said `live`.

* **The producer re-reads what `HEAD` names, and the frame prints it.** `watch::Update` gained
  `repo_label`: the same string the CLI composed at startup, composed again when the probe says the head ref
  moved, and drawn in the tab bar's right-hand column. A label the shell already has is not news and
  announces nothing.
* **The cheap probe now carries the name as well as the commit**, because they are not the same fact. Two
  branches sit on one commit as often as not — a fresh `git switch -c`, a switch back to a sibling nobody has
  committed to yet — and the probe compared oids, so the one keystroke that changes the label was the one it
  did not notice. A switch between two refs at one tip is now a `worth_reading` event, and the ask that
  follows is the one allowed to skip the store re-read: the rows, the numbers and the working tree are all
  what they were, and one line of chrome is not.
* **The switch says so** on the hint row (`now on myrepo · feature/x`) when nothing else in the update has a
  sentence to say, and stays quiet when something does — a switch that also brought new commits already has
  the rail to announce itself with.
* **What is not fixed, on purpose:** a frame that admits it is a snapshot (`--no-watch`, nothing indexed yet,
  a binary built without the indexer) keeps the label it opened with. That is what a snapshot is, and the
  status line already says which it is and what would move it.

`crates/fathom-tui/tests/watch.rs` drives the loop with a scripted producer and asserts on the drawn tab
bar; `crates/fathom-cli/src/ambient.rs`'s probe tests run `git switch` against a real temporary repository,
because a probe that only notices oids *is* the bug. Checked end to end against the built binary in a pty:
`fathom` open on an indexed repository, `git switch -c feature/same-tip` typed in another shell, and the
label moves from `… · main` to `… · feature/same-tip` inside one poll — with no keystroke in the frame.

### The perf budgets job had been red on every run it ever had, and the check page said only "exit code 11" (`reco-axp`)

The `perf` workflow fails on every pull, and its annotations explained everything except the one thing
worth knowing — *which* measurement failed:

```text
perf budgets (small class)
  Run the budgets, the baseline, and the trend (local spelling: make bench-check)
    Process completed with exit code 11.
```

The gate printed its failures to stderr as prose and gave a GitHub annotation only to the *cannot
classify* list, so the budgets that stopped the build were in a log nobody had to open. Reading them on
the same box the baseline was blessed on was the fix:

* **A red gate now names what it blew, as an annotation** (`::error title=Perf budget gate::`, one per
  failing measurement), and `tests/test_perf_budget_contract.py` fails if a red gate annotates nothing.
  The self-test and the injected regression already proved the gate could fail; this proves it can be read.
* **Five blessed rows sat at the fast tail of their own distributions.** `warm.reindex.ms.small`
  (blessed 120.1 ms) has measured 101.5-107.0 ms in every run since — 11-15 % *under*, which trips
  `stale_baseline`, because the row was blessed before `reco-dkr` took the harness's own 2 ms poll out
  of every timed process — and the three pty rows (`query.diff_open.ms.small` 2.9 ms,
  `query.diff_first_hunk.ms.small` 4.3, `query.queue.ms.visible.small` 2.8) have read 4.2-12.5,
  5.6-16.3 and 2.7-8.8 ms, so the tail of their own noise crossed the floor and was reported as a
  regression on a pane that had not moved. Six gate runs of one tree and one binary: red, green, green,
  red, green, green. Those five rows are re-blessed at the median of ten probes, divided by that
  runner's calibration factor so they stay in reference-machine units, each carrying the
  centre/scale/spread/floor `reco-dkr` asks for. Every other row is untouched.
* **A runner faster than the blessed machine was credited a speed-up it had not earned.** The
  improvement half of the ratchet is judged on raw numbers so that a box shared with eight jobs is never
  read as faster code — but the runner factor is *also* clamped at 1.0, and a box the calibration bench
  measures at 0.8× measures every wall clock two tenths low against blessed times that are
  reference-machine times. The clamp is about budget headroom, so the improvement half now divides by
  the unclamped ratio when, and only when, that ratio is below 1.0. A slow runner is still credited
  nothing; a real speed-up still has to be re-blessed on any runner.

ADR-004 §11.10 carries the detail, including the two rows left alone on purpose and the one a re-bless
cannot fix: the diff pane's first paint arrives in two clumps, so it is a 120 ms budget guarding a 12 ms
measurement, and the gate says so rather than guessing.

### The packaging action could not write an `.rpm`, because one of its paths was missing a slash (`reco-ndf`)

The release job's packaging step — the one that repacks `dist`'s archives into `.deb` and `.rpm` — wrote
the `.deb` and then refused to write the `.rpm`:

```text
Processing files: fathom-0.1.0~alpha.1-1.x86_64
error: File must begin with "/": usr/share/zsh/site-functions/_fathom
RPM build errors:
    File must begin with "/": usr/share/zsh/site-functions/_fathom
```

* **One map, two spellings.** The packager relocates the archive's files with a map written in relative
    paths — `usr/share/zsh/site-functions/_fathom` — because that is what a `.deb`'s data tar names its
    members and what `cp -a` into `$RPM_BUILD_ROOT` lays down. `rpmbuild`'s `%files` section wants the
    absolute spelling, and one entry in its template was the relative one pasted in. The four that were
    literal `/usr/…` text in the template survived; the one that came from the map did not, and rpm
    refuses the *whole package* over it: not one missing file, no `.rpm` at all.
* **The list is generated now.** `spec_files` builds the `%files` block from `SHELL_DIRS["rpm"]` — the
    same map the staging reads — with every entry routed through `rpm_file()`, so a completion directory
    appears in the package and in the spec in one edit, and it cannot be a relative one. The claim is
    therefore about the two agreeing, not about five strings having been typed the same way twice.
* **Checked before `rpmbuild` is asked.** `spec_problems` answers rpm's two refusals — a relative entry,
    and an entry naming nothing that was staged — in the packager, on any machine, and `build_rpm` refuses
    to invoke `rpmbuild` until they are clear. That matters because the tool is missing from every image
    but the one that provisions it, in the one stage of a release that gets no second chance.
* **Read back afterwards, from rpm itself.** `--verify` now compares the finished package against
    `rpm -qlp`, the package's own answer to "what does this install", and the CI step that builds the
    packages runs with `--verify` rather than three steps later under `dpkg -i`.
* **Both halves go red,** since a checker that cannot fail is a decoration: the packager's `--self-test`
    and `tests/test_packaging_contract.py` each assert every `%files` entry is absolute *and* that a
    relative entry, and a spec claiming an unstaged file, are refused.
* **Found next door and fixed:** `packaging/linux-packages.sh` installed `rpmbuild` with
    `apt-get install rpmbuild`, and no Debian or Ubuntu archive has a package by that name — `/usr/bin/rpmbuild`
    ships in `rpm`. The message telling a human what to type said the same wrong thing. It provisions
    `rpm` now, and the packager's own "rpmbuild is missing" error says so.

### Spike `s4` could only reproduce on one computer, which is not where CI runs (`reco-v1z`)

`docs + workspace contracts` was red on every push with a claim about a measurement that had not moved:

```text
spike s4 does not reproduce: the measured report differs from the committed one
  line 1972:
    committed: "cpu_count": 4,
    measured:  "cpu_count": 8,
  line 1973:
    committed: "git": "git version 2.50.1",
    measured:  "git": "git version 2.47.3"
```

`s4` re-measures sixteen authored `git` histories across 23 detector configurations and diffs the result
against `benchmarks/reports/s4-rename-detection.json`. All 51 gates passed; the four fields that
disagreed were facts about the machine that ran it.

* **The comparison included the runner.** `--check` already projects out wall clock — a busy runner is
    not a regression — but `environment`'s interpreter, platform, core count and `git` build were
    compared, and they can never match: the committed report was written on an aarch64 box under CPython
    3.14, and the step runs on `ubuntu-latest` under the workflow's own `setup-python` 3.12. A gate that
    only one computer can satisfy is a gate that is red forever, and red about nothing.
* **Recorded, printed, not compared.** The four are now excluded by path, and a differing environment is
    announced rather than quietly ignored — `note: measured on a different machine than the committed
    report (not a drift): python 3.12.7 -> 3.14.5; platform Linux x86_64 -> Linux aarch64; …` — because
    the git rows are git's and the log has to say which git produced them. Everything else in
    `environment` (the 128 MinHash permutations, the default shingle width, the prototype's stated
    profile) is a claim about the measurement and is still compared, and so is `corpus.scenarios`, which
    the old key-name-only exclusion list was discarding along with `meta.scenarios`.
* **The failure output is a list of claims now, not a line diff.** The report is about 2 600 lines of
    JSON, so the old positional diff said `line 1972 differs` and then every line below it shifted: one
    runner fact, printed as a catastrophe. A drift reads `corpus.true_pairs: 59 -> 60`.
* **The git rows no longer read the runner either.** `repo.py` builds every fixture with
    `GIT_CONFIG_GLOBAL=/dev/null`, pinned dates and pinned identities, but `detect.git_detect` — the
    measurement of git itself — ran `git diff` with the ambient environment, where a stray
    `[diff] renames = false` or `renameLimit` changes which pairs git even considers. It runs through
    the same scrubbed environment now, and the committed report reproduces byte for byte afterwards.
* **Both halves are pinned by a test,** because a loosened comparison that cannot fail is the same bug
    wearing the fix: a moved probe, a moved `true_pairs`, a moved `corpus.scenarios`, a moved permutation
    count and a moved gate expectation are each still a drift, and a different runner is not.

### One test asked the machine it ran on who it was (`reco-6wr`)

`a_removed_manifest_is_a_deletion_and_a_new_member_is_an_addition` — one test, and it was red on a clean
clone and green on the laptop that wrote it:

```text
---- a_removed_manifest_is_a_deletion_and_a_new_member_is_an_addition stdout ----
Author identity unknown
*** Please tell me who you are.
fatal: unable to auto-detect email address (got 'user@host.(none)')
assertion failed: status.success()
```

* **The fixture scrubbed everything except the one helper that needed it.** Every commit this test's
    repository is built from goes through `tests/common`'s `git` helper, which clears the environment,
    pins `user.name` and `user.email`, and pins the clock to `BASE_EPOCH` — the module says why: a
    fixture whose commit ids move between machines makes a failure impossible to reproduce. The deletion
    half of this one test shelled out to `git rm` and `git commit` directly, with the ambient environment,
    so it inherited the developer's own identity. On a box with a global `user.email` (every box that
    wrote the test) it committed under that identity and dated the commit *now*, while every other commit
    in the fixture is dated January 2025 — different commit ids on every machine, which is precisely what
    the fixture exists to prevent. On a box without one — CI, a fresh clone, a container — the commit was
    refused and the test could not get to its own assertions at all.
* **The fix is the helper the fixture should have had.** `Fixture::remove(path)` stages `git rm -- path`
    and commits it through the same scrubbed `git` as `commit`, `edit` and `rename`, so the deletion
    commit is now the same object everywhere and the test passes with or without a configured identity.
    Verified both ways: the target is 8/8 green with the ambient identity present and 8/8 green with
    `HOME` pointed at an empty directory and `GIT_CONFIG_GLOBAL=/dev/null`, which is the shape that was
    red. An audit of every `Command::new("git")` in the workspace (`grep` for a commit whose invocation
    does not `env_clear` first) found this one and no others; the rest of the fixtures already scrub.

### Two workspace tests were a coin flip, because both of them asked the clock a question (`reco-ecb`)

`cargo test --workspace --all-features` failed on a clean tree and could not be reproduced on the next
run, or the one after that:

```text
---- a_span_is_the_composition_of_its_parts stdout ----
Test failed: the generated history changed nothing at all, so the composition was never exercised
    at crates/fathom-analyze/tests/graph_diff_property.rs:321
test result: FAILED. 4 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out

---- answering_appends_the_note_that_closes_it stdout ----
assertion `left == right` failed: exactly one block was appended:   left: 2  right: 1
test result: FAILED. 8 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
```

Roughly one run in thirteen broke the first way and one in ten the second, same commit, same binary,
different verdict — and proptest even helpfully saved the first failure to a
`graph_diff_property.proptest-regressions` file, which turned a one-in-thirteen flake into a permanent
red until somebody deleted a file nobody had meant to create. Neither test was wrong about the product.
Each of them asked a question whose answer is not a fact about the code.

* **The property test treated an input that never exercised it as a counterexample.** The delta algebra
    is proved over random histories, and the history generator draws a *history*, so about one draw in
    twenty-four hundred the seed lands such that every revision of a one-module history carries the same
    exports and the same edges. Every span in such a history is empty at every cut, the composition
    equality holds vacuously, and the guard that exists to notice — "a strategy that generated identical
    revisions would prove the property about nothing" — was a `prop_assert!`, so *nothing was tested* was
    reported as *the algebra is wrong*. It is now a `prop_assume!`: the case is rejected rather than
    refuted, which is what proptest's vocabulary has for exactly this. Nothing is unguarded — a strategy
    that stopped moving **altogether** trips `max_rejects`, which fails the run and blames the generator,
    which is the thing to blame. Measured, not assumed: 12 × 5 000 draws after the change, no failure and
    no regressions file; before it, the same draw at 20 000 cases failed on the 2 376th.
* **The notes test counted blocks after two answers, and a note's id carries the second it was written
    in.** `notes::entry::note_id` hashes kind, episode, author, text **and the instant**, truncated by
    `ledger_clock` to the whole second the markdown can round-trip — the same reasoning `reco-2pw` applied
    to the ledger. `fathom note answer nt-… "no — it went in the cleanup"` run twice inside one second is
    therefore one block; run twice across a second boundary it is two, because under that identity rule
    the second one *is* a second note. The test wrote the same answer twice — once for the human line,
    once for `--json` — and then asserted exactly one block had been appended, so the assertion was
    measuring where the second hand sat. The count now happens after the one answer this test's claim is
    about, and the `--json` door is still exercised, just after the append-only claim has been checked.
    Deliberately **not** fixed by making `answer` idempotent forever: that means taking the instant out of
    note identity, which re-addresses every note in every committed review memory and orphans every
    `about:` link in it. That is a product decision with a breaking change attached, not a flake fix.
* **Both rates are measurements.** Each binary was run 40 times before and after (4 red / 40, then
    0 red / 40 for the notes verb), the property test was run at 5 000 cases per run, and the remaining
    170 test binaries of the workspace were each run 5 times to look for a third: nothing else moved.
    The wall-clock lesson is the house rule, restated in the two places it was not written: a test may
    read the clock, but a test may not *depend on where the clock was* between two of its own steps.

### The perf gate died on its first measurement, because the RSS sampler had taken a name `threading` owns (`reco-yfd`)

`perf` — the perf budgets job — failed *the budgets, the baseline, and the trend* about two seconds in,
before it had indexed anything, and printed a traceback instead of a table:

```text
  File "…/tools/perf_gate.py", line 783, in measure_end_to_end
    ms, rss = harness.fathom("index", fresh_store=True)
  File "…/tools/mvp_acceptance.py", line 142, in peak
    self.join(timeout=2.0)
  File "…/python3.12/threading.py", line 1171, in _wait_for_tstate_lock
    self._stop()
TypeError: 'Event' object is not callable
```

Every number the gate takes — wall clock and resident set together — goes through
`mvp_acceptance.run_measured`, which samples a child's `VmHWM` from a `threading.Thread` subclass and
tells that thread to stop by setting an `Event` on it. The Event was bound to `self._stop`, and
`Thread._stop` is not a free name: through CPython 3.12 it is a **method** — the one
`Thread._wait_for_tstate_lock`, the body of every `join()`, calls when the thread has finished. Binding
an Event over it turns the join that closes every measurement into `TypeError: 'Event' object is not
callable`, so the gate died in its very first `fathom index` and the trend, the baseline comparison and
the artefact never got written. It reproduced nowhere a developer looked: the interpreters people run
are 3.13 or newer, which dropped `Thread._stop`, while every workflow here pins `python-version: "3.12"`
— so CI was the only place the bug could speak, and the contract suite that would have caught it (the
next step in the same job) was skipped because the gate had already died.

* **The flag is called `_stop_requested`, and the name is the fix.** Nothing about the sampler's
    behaviour changes; what changes is that it no longer overwrites a member `threading.Thread` uses
    for its own lifecycle. The class docstring says so, because `self._stop` reads as the obvious name
    for a stop flag and will be proposed again.
* **The same shadowing went for anything that measures a verb on 3.12**, which is `tools/perf_gate.py`,
    `tools/perf_realrepo.py` and `tools/mvp_acceptance.py`'s own acceptance harness: all three take
    their RSS numbers from this sampler, and all three died at the first measurement rather than
    reporting a budget.
* **What guards it.** `tests/test_perf_budget_contract.py` now reads the assignments
    `_PeakSampler.__init__` makes and refuses any name from CPython's private `Thread` vocabulary.
    The vocabulary is spelled out in the test rather than read off `dir(threading.Thread)`, because
    the interpreter running the guard is not the interpreter that broke — a guard that can only see
    the interpreter it is standing on guards nothing. A second test takes one real measurement through
    `run_timed` and demands a number back; that one is the shape of the crash, and it fails on the
    interpreter CI pins.

* **The job reports a verdict even when the gate cannot produce one.** The contract suite is now
    `if: always()`: it is the step that names what a working gate does, and skipping it turned a
    crashed gate into a log with one traceback and nothing to compare it against.

### The config reference gate could never go green, because the fix had been typed into the generated file (`reco-c81`)

`contracts` — the docs-and-workspace job — failed on the step *the config reference tables match the key
spec*:

```text
fathom config: docs/concepts/config.md: generated block `keys` is stale — run `fathom config doc --write`
```

Two things were wrong with that line, and the second is why the first survived. The remedy names a command
that does not exist: ADR-002 §4.3 designs `fathom config <subverb>`, `src/recommender/config/` is that
design made executable, and `enum Verb` in `fathom-cli` has no `config` variant to run — so the message sent
the reader to a command the binary refuses. And the row the gate called stale was
`non_code.env.secret_name_pattern`, whose default is a regex alternation:
`(?i)(^|_)(secret|token|password|…)`. A GFM table row is split on every `|` that is not written `\|`, so that
row arrived at the reader as four cells with its header row missing — the defect `reco-x69`'s render audit
correctly found, and which was then fixed in the wrong file: the bars were escaped *by hand*, inside the
`BEGIN GENERATED` block of `docs/concepts/config.md`. A hand edit inside a generated block is drift by
definition, so from that commit the generator and the document could not agree at all: fixing the render
re-opened the gate, passing the gate broke the render, and every diff that touched a config key inherited
the argument.

* **The escape belongs to the generator.** `recommender/config/explain.py` builds every table line through
    `_row(…)`, which passes each cell through `_md()` exactly once — `|` becomes `\|`, which is the escape
    GFM defines here and which `tools/build_docs_site.py` renders back to `|`, so the reader still sees the
    regex whole and the table keeps its columns. Once that was true, `doc --write` rewrote
    `docs/concepts/config.md` to … nothing: not one byte of its generated blocks changed, which is the proof
    that the file had been right and the generator was the party that was wrong.
    Escaping once is a rule, not a coincidence — `_row` is the only way a table line is built, so a default
    with a bar in it cannot be escaped twice and prints no stray backslash.
* **A remedy is a command, or it is decoration.** `recommender/config/command.py` states the invocation once
    — `PYTHONPATH=src python3 -m recommender.config …`, prefix included because `recommender` is read out of
    `src/` and installed nowhere — and every message that used to say `fathom config doc --write`, `fathom
    config schema`, `fathom config lint`, `fathom config keys` and `fathom config explain --key K` now spells
    it that way: the stale and missing doc-block problems, the schema-drift problem (which also retypes the
    `generator` field inside `schemas/fathom.schema.json`, so the artefact names the command that
    reproduces it), and the hints for unparseable TOML, a value of the wrong type, a value outside its
    declared range and a key nobody declared. `fathom config …` stays where it was always honest — prose
    about the design, in ADR-002 and in this document — and appears in nothing a reader is told to type.
* **The manual's own recipe list types too.** §11 of `docs/concepts/config.md` now runs its ten commands
    through that prefix the way `docs/concepts/rename-pairing.md` and the ADRs already do, and says in one
    sentence why there are two spellings of one surface.
* **What guards it.** `tests/test_config_contract.py` gained six clauses — fifteen cases, counting the
    parametrized ones: every generated table row splits into as many cells as its header, which is the
    assertion that goes red the moment a bar stops being escaped; a stale block, a missing block and a stale
    schema each name their remedy; each remedy is then handed to a real shell with `--help` appended and must
    be accepted, which is `tools/check_docs_commands.py`'s argument moved from the manual to the messages;
    and four refusals must not print a backticked `fathom …`. The new module is in the package's import
    whitelist with nothing in it but `__future__`, so it cannot smuggle IO into the types half.

### The perf budgets job stopped measuring the budget it was most likely to break (`reco-cwx`)

Every `perf` run since the baseline was blessed printed the line and moved on:

```text
cold.queue_visible.ms.small                               —      3,000         —  blocked: the probe never saw a first frame
```

That row is ADR-004 §11's *time to first frame* — the one budget about what a reviewer waits for when they
open the TUI — and a `blocked` verdict does not fail the build. It just means nobody looked. The gate has
been printing this on every box that is not the one the baseline was blessed on, and the reason was the
harness mistaking its own patience for an absence: `tools/tui_session.py`'s `drain` calls a frame *settled*
after two quiet ticks of 50 ms, which is a fine way to find the end of a burst and a terrible way to wait
for the beginning of one. Applied before the first byte of the answer, it is not a settle rule at all, it is
a ~150 ms ceiling on what the probe can observe: the blessed first paint measured 138 ms, this box paints
at 152-171 ms, and everything past the window came back `None`. A 3 000 ms budget that can only be read up
to 150 ms is decoration, and the trend shows the censoring rather than the product — the published
first-frame column is 25 ms on one box, 130 ms on another, and empty on a third.

* **Quiet now means "the answer has stopped", never "nothing has arrived yet".** `drain` waits for the first
    byte for the whole deadline and starts counting quiet ticks only once something has been seen, which is
    what makes `first_paint_ms`, `visible_ms` and `until_ms` mean the same thing at 400 ms as they mean at 4.
    A key that draws nothing still comes back as nothing; a key that draws late used to come back as nothing.
* **A dead child no longer measures as a fast paint.** `spawn` passes `--binary` to the pty child *after* it
    has `chdir`ed into the repository under review, so a relative `--binary target/release/fathom` — the
    spelling the tool's own `--help` suggests — pointed at nothing, and the child's
    `exec failed: [Errno 2] …` landed on the pty as terminal output, which the reader timed as a first frame
    about 1 ms after spawn. The program is resolved in the parent now, and an unresolvable one is a named
    exit instead of a number.
* **The ratchet stopped being asked to guess.** With the probe repaired, a ~140 ms row compared at ±15 % asks
    for 21 ms out of a spread that is that wide by itself (19 ms across nine probes of one tree, and the same
    tree measured 109-150 ms across the blessed runs). `SPAWN_NOISE_MS` states that as the measurement's
    floor, so the row is now listed under *ids the ±15 % ratchet cannot resolve* and guarded by its budget —
    the honest state, said out loud, instead of a build that goes red on a slower box and green on a fast one.
    A 400 ms injection still lands sixteen floors over it.
* **What guards it.** `tests/test_perf_budget_contract.py` drives `tools/tui_session.py` against a fake TUI
    that paints at a *known* delay: a first frame at 300 ms must come back at 300 ms, not `None`, and a key
    answered at 400 ms must come back late rather than absent. A second test uses the same fake to prove a
    relative `--binary` reaches the child, and a third asserts the gate publishes a number for
    `cold.queue_visible.ms.small` rather than a blocked row.

### The docs job's CI-gate check crashed instead of reporting, because nothing was built (`reco-p40`)

`contracts` — the `docs + workspace contracts` job, the one that deliberately installs no Rust toolchain —
printed a Python traceback for its `m2 CI gate` step:

```text
Traceback (most recent call last):
  File "…/tools/check_ci_gate.py", line 359, in check_reusable
  File "…/tools/check_ci_gate.py", line 167, in binary_verbs
    proc = subprocess.run([str(binary), "--help"], …)
FileNotFoundError: [Errno 2] No such file or directory: /home/runner/work/fathom/fathom/target/debug/fathom
```

The gate is run twice on purpose: on `ci.yml`, where the leg that compiles the binary re-checks the two rules
whose source *is* a compiled program, and on `contracts.yml`, where the half that reads documents and
workflow files catches a recipe that names a deleted script without waiting four minutes for a build. Only
the flag rule knew about that second home — `check_flags` opened with `if not binary.is_file(): return`. The
verb rule beside it did not, so on a runner where `target/debug/fathom` has never been built, `binary_verbs`
handed a missing path straight to `subprocess.run` and the job died of an exception instead of printing a
verdict. `tools/check_ci_gate.py self-test` raised the same way, for the same reason, which meant the one
command that is supposed to prove the checker can fail needed a toolchain to do it.

* **An absent binary is a state, not an exception.** `ask_binary()` is the one place that touches the
    process: no file, a file that is not executable, a binary for another architecture and a non-zero exit
    all come back as `None`, which `binary_verbs` and `binary_flags` turn into "nothing to read". The two
    rules that need an answer now say so on stdout — `reusable workflow: verbs not checked (no binary at
    …; ci.yml runs this half on the leg that builds one)` and `flag check skipped: …` — and the note is
    printed without `--verbose`, because a skipped half that nobody mentions is a green job that lies about
    what it looked at. The exit code is 0, which is the honest answer: nothing here disagrees with anything,
    and two rules had no source to disagree with.
* **`self-test` needs nothing again.** Its verb clause used `binary_verbs(…target/debug/fathom) or
    {"flarble"}`, which was written to survive a missing binary and could not: the `or` never got to run past
    the raise. It now ships its own `--help` — a stub in the temp directory that prints a clap-shaped
    `Commands:` block — so the parser is exercised on every job, and separately asserts that the *real*
    `fathom --help` parses when a binary happens to exist, since the stub is a guess at clap and the built
    binary is the fact.
* **The gate's own step in `contracts.yml` now runs `self-test` first**, the way `docs.yml` opens with the
    builder's self-test: it costs nothing, it needs no toolchain, and it proves the gate underneath it is
    capable of failing rather than merely silent.
* **What guards it.** `tests/test_ci_gate_contract.py` gains the docs-job state as a promise: the gate run
    against a path that is not a binary exits 0, prints neither traceback nor silence, and names the two
    halves it could not check; a file that is not a runnable *program* is the same state; and
    `binary_verbs()` on a missing path returns a set rather than raising. Those run on a machine with nothing
    built, which is the state that used to raise. The pre-existing verb tests carry `@needs_binary` and so
    never covered it.

### Every `cargo fuzz` job refused to open the fuzz workspace (`reco-com`)

The three `smoke` jobs on any pull that touches the readers, and the three `nightly` ones, printed the same
three lines and then nothing else:

```text
Error: manifest fuzz/Cargo.toml does not look like a cargo-fuzz manifest. Add following lines to override:
[package.metadata]
cargo-fuzz = true
```

cargo-fuzz reads `package.metadata.cargo-fuzz` before it builds a target — `is_fuzz_manifest`, five lines,
no fallback, no warning — and it is the only reader of that key in this repository. `cargo build`,
`cargo clippy`, `cargo deny` and `python3 tools/gen_fuzz_corpus.py check` all read the same manifest happily
without it, so the fuzzing lane could be red for a nightly cycle at a time with no crash report, no compile
error, and no diff that looks as though it touched fuzzing. `reco-yuk` had fixed the *working directory* of
these steps, which is how a run got as far as reading the manifest at all; the marker is the other half of
the same discovery rule, and the half nothing was guarding.

* **The fix is two lines in `fuzz/Cargo.toml`**: `[package.metadata]` and `cargo-fuzz = true`. Measured with
    the tool itself rather than by reading its docs — `cargo +nightly fuzz list` from the repository root now
    prints `hooks`, `lockfile` and `paths`, and `cargo +nightly fuzz build --dev lockfile` gets as far as
    invoking `cargo build --manifest-path fuzz/Cargo.toml --bin lockfile` with the sanitizer flags, where
    before the marker it stopped before starting a build. (Here it then stops at a missing `c++`, which is
    this container having no C++ compiler for libfuzzer's build script and not a claim about the manifest.)
* **The marker is also what directory discovery keys off**, which is why the prose moved with it.
    cargo-fuzz walks up from the working directory to the first manifest that is **not** marked and takes
    `<that>/fuzz`, so marking this one is what lets a run started inside `fuzz/` walk back up to the root
    instead of looking for `fuzz/fuzz/`. `fuzz/README.md` and the header of `fuzz.yml` now give the real
    reason the root is where a run belongs — the seed copy and the `fuzz/artifacts/<target>` upload are both
    root-relative — instead of claiming the wrong directory is unrecoverable, and rule `P03`'s message says
    the same.
* **`P04` is the gate.** `tools/check_workflows.py` now refuses a workflow that runs `cargo fuzz` while
    `fuzz/Cargo.toml` is a manifest cargo-fuzz would not open, `self-test` supplies the unmarked manifest to
    prove the rule can fire, and `tests/test_workflows_contract.py` mirrors it with the error text a
    contributor actually sees. It runs in `contracts.yml` on every pull, which is the only place a marker
    invisible to every other tool can be caught.

### The Guardrail 3 harness refused its own committed report, so five more diffs got read (`reco-0zi`)

`ci`'s `g3-calibration` job went red on a corpus nobody had touched, with the one message that job exists to
print:

```text
g3: the committed report is not what the pinned corpus and the shipped detectors and weights produce now.
committed:         "blamed_on_score": 23,
re-measured:       "blamed_on_score": 20,
```

Nothing under `crates/fathom-analyze/calibration/g3/**` had moved since `reco-6co`. What had moved is the code
underneath it: `reco-0er` made `risk::feature::authorship` answer `Measurement::missing` wherever the
attribution ladder had only fallen through to a guessed `human`, so in every repository whose harness stamps
no turn evidence the weight leaves the score's denominator — eleven axes instead of twelve — and every
measured term on those episodes is renormalised upward by a few percent. Over 540 real episodes that is 1 to
4 points, which is enough to move a top-decile cut (33 / 40 / 40 → 34 / 42 / 43) and to lift five episodes
over a threshold they had been under. The report is a byte-for-byte promise, so the harness was right, and
the fix was to re-measure rather than to edit the promise.

* **Five promoted episodes, five diffs read, five dispositions written.** `cli`'s `go-gh/v2` v2.16.0 →
    v2.16.1 patch bump, `flask`'s `update dev dependencies` refresh, and in `ruff` the released-`black`
    switch, the hyperfine 2.0.0 bump and the py-fuzzer minimisation budget. Four are `low-stakes` blamed on
    the ranking — the score raised a flag no finding justified — and the fifth is a `false-positive` blaming
    `untested_new_logic` (119 lines in a directory that has no test suite and never had one) and
    `debug_leftover` (the fuzzer's own `--quiet`-guarded error line, a claim this corpus has now dismissed
    on that file three times). 67 dispositions became 72, and `every-flagged-episode-is-labelled` came back
    to `pass` the way it did after `reco-6co`: by being paid for, not by being relaxed.
* **What the numbers did.** `G3` at the published τ 0.633 → **0.660** (35 false positives on 53 labelled
    flags) and at the corpus cut 0.672 → **0.683** (43 on 63); false positives blamed on the ranking 23 →
    **24**; six more `cli` readings now sit under a cut that moved out from beside them and are printed in
    `labels_unflagged` instead of counted. The two miss-rate gates did not move at all — 0.792 and 0.750,
    byte-identical — because a few percent carried the lockfile band over the line and left every reworked
    episode under it.
* **What did not move.** §5.4's three bars, `tau_alpha` 40, `min_denominator`, `DEFAULT_FLIP_MIN_LABELLED`
    and the flag floor. One consequence has to be stated because it is a moving denominator and not an
    improvement: `test_weakening` measures 0.273 on 11 answerable flags now and is under α, because two of
    its attributed false positives were readings of `cli` episodes the raised cut stopped flagging. The
    corpus has two default-enabled detectors over α with ≥ 8 answerable flags, not three.
* **The written answer was dragged to the measurement, not the other way round.**
    `docs/concepts/false-positive-calibration.md` gains a §3c for this pass, and its gate table, τ sweep and
    per-detector table are the new report's numbers; `thresholds.json`'s `how_gates_are_set`, `tau_note` and
    every gate `why` that quoted a moved number says the new one; the corpus README's §1.1 table gains the
    fourth pass. `tests/test_g3_calibration_contract.py` is what proves none of that was remembered rather
    than read.

Verified: `python3 tools/fetch_g3_corpus.py` (three pins, oids checked),
`cargo run --locked -p fathom-analyze --features sqlite-bundled --example g3_calibration -- --check` (reproduces
byte for byte, 9 gates, 0 surprises), `pytest tests/test_g3_calibration_contract.py` (26 passed),
`tools/check_docs.py`, `tools/check_ticket_claims.py`.

### Six workflows were red, and four of them had no log to read (`reco-yuk`)

`ci`, `contracts`, `fuzz`, `perf`, `Release` and `dogfood` were all failing, and the shape of the failure is
what took the time: nothing in the YAML could show *why*. A job's `uses:` reference is resolved over the
network at run time, so a wrong one is not a syntax error — the job dies in *Set up job*, before its first
step runs, which is the one failure in the whole system with no log. `ci.yml`'s `release-skeleton` job ran
`axodotdev/cargo-dist/action@v1`, a path that repository has never contained: `cargo-dist` ships an
installer, not an action. `fuzz.yml` read a step output as `steps.minutes.value` instead of
`steps.minutes.outputs.value`, which GitHub answers with the empty string rather than an error, so the
empty string travelled four steps and died in `$(( … ))`. Both jobs looked like infrastructure flakiness
from the outside and neither had anything to say for itself in a diff.

* **The workflows are now checked against this repository**, by `tools/check_workflows.py`, offline and
    standard library only. Twelve rules in five families: every action reference is well-formed and on an
    explicit inventory (and `--network` proves each inventoried ref resolves on GitHub); every `${{ … }}`
    context path resolves to a step `id:`, a `needs:` job, or a declared `inputs:` name — including the
    `steps.<id>.<prop>` shape that is missing `.outputs.`; every repository path a `run:` step names, and
    every `working-directory:`, exists; every `cargo -p`/`--features`/`--test`/`--example` is a member,
    feature and target of the manifests it is typed next to, and every `tools/*.py <subcommand>` is a
    subcommand that tool's argparse offers; and job wiring adds up — a `needs:` that exists, `permissions:`
    somewhere rather than the organisation default, and artefact names uploaded once and downloaded by a
    pattern that matches one of those uploads. It is a step in `contracts.yml`, mirrored by
    `tests/test_workflows_contract.py`, and `python3 tools/check_workflows.py self-test` writes nine
    workflows designed to break one rule each and requires every rule to fire, because a gate that has
    never said no is a gate that cannot say yes.
* **`cargo fuzz` runs from the repository root.** `fuzz.yml` set `working-directory: fuzz`, and cargo-fuzz
    takes its fuzz directory from the package it finds in the working directory — from inside `fuzz/` that
    is `fuzz/fuzz/Cargo.toml` and `fuzz/fuzz/fuzz_targets/`, so it reported "no fuzz target by the
    name(s): 'lockfile'" about a target that is committed. The `fuzz/fuzz/…` paths that step then uploaded
    were the same mistake one level down. The corpus is now copied to `$RUNNER_TEMP/seeds` and passed
    explicitly, because the fuzzer writes into whatever directory it is pointed at and a discovered byte
    sequence is not something this repository reviews; `fuzz/README.md` said "run it from `fuzz/`" and now
    says the opposite, with the reason.
* **The release path was stopped by whitespace.** `crates/fathom-cli/wix/main.wxs` is generated, and had
    been hand-touched; `dist plan` — the first step of `release.yml`, the job that decides what a release
    contains — refuses to start when a generated file is not its generator's output, so the entire release
    pipeline was red over eight lines of re-wrapped XML. Regenerated, and `dist generate --check` now runs
    on every pull request (which is what finds the next one), with the pinned `cargo-dist` read out of
    `dist-workspace.toml` instead of copied into a URL.
* **A `--all-targets` build has to name the features its targets need.** Two structure jobs built
    `fathom-analyze --features langs-all --all-targets`, and `tests/graph_index.rs` drives the graph pass
    through `fathom-store`'s SQLite backend, which ADR-002 §6.2 gates behind a store feature: the library
    built, the test suite did not. Those steps now say `langs-all,sqlite-bundled`.
* **A generated file is only "the binary's own grammar" for the binary it was blessed from.** A man page's
    VERSION section prints `fathom --version`, which carries the feature list it was compiled with; the
    committed page carried the all-features list while `gen_shell_artifacts.py`'s contract is the default,
    shipped build (ADR-002 §6.1) — and in the job, the binary on disk at that step was the headless
    `minimal` one built earlier. The step now builds the default binary itself before diffing, so the
    answer does not depend on which step last wrote the file.
* **Two clippy lints had landed with the toolchain.** `crates/fathom-ingest/src/oid.rs` now uses
    `as_chunks::<2>()` where it hand-rolled a `chunks_exact` length check, and two tables in test files name
    their types instead of spelling them inline: the language-convention row in
    `crates/fathom-analyze/tests/api_surface.rs`, and the scripted-detector body in
    `crates/fathom-analyze/tests/plugin_api.rs`. Both are new lints firing on code that was clean under the
    previous toolchain, in a step that runs `-D warnings`.
* **The search box's hint line was smaller than the search box.** `crates/fathom-tui/tests/tables/mod.rs`
    is generated from the Python tables, and had not been regenerated since `reco-6sw` widened the token
    grammar: the pane hint — and the keymap's T3 step beside it — still advertised `author: path: risk> dep:`
    while `search.rs` accepted and explained `since:` and `kind:`. The Rust prose caught up with the
    normative tables, `tests/tables/mod.rs` was regenerated, and five layout goldens were re-baked, one line
    each: the hint, now listing the grammar it has.
* **Still red, on purpose.** Two gates fail for reasons a workflow edit must not paper over, and blessing
    either is a decision about the product rather than about CI. Guardrail 3 (`g3-calibration`) re-measures
    against three pinned real repositories and no longer matches its committed report: `blamed_on_score` 23
    → 20, and 11 flags in the current corpus have no label in the hand-authored truth files
    (`unlabelled_total` 0 → 11), which moves the published miss rate from 0.672 to 0.655 — that is
    [docs/concepts/false-positive-calibration.md](docs/concepts/false-positive-calibration.md)'s headline
    number and somebody has to label those flags and agree to move it. `perf_gate.py --check` is red on one
    line: `query.metrics.ms.small` 12.831 → 14.900 ms, +16.1 % against a 15 % ratchet, with a runner factor
    of ×1.03 and a noise floor a fifth of the change, so the regression is real by the gate's own standard
    and small in absolute terms. `make bench-bless` is the ratchet, done loudly, and it is the budget
    owner's call.
* **Tests**: `tests/test_workflows_contract.py`, nine cases — every rule green on the tree as committed,
    every action reference inventoried, no step context read without `.outputs.`, the store feature still
    present for the target that needs it, the checker pure-Python and network-free, the checker wired into
    `contracts.yml`, and the checker's own `self-test` proving it can fail.
* **Three commits in this range carried a subject and no bead id**, which the coverage rule in
    `gen_changelog.py` counts as a gap, and the honest way to close a gap is to say what they did:
    `bf983176a` retyped the two clippy steps' `run:` folding (the shape that is what made the next person
    read them as data), `e194a63d9` added `publish.yml`, the workflow that turns a version-bump commit into a
    release after `ci` goes green on the default branch, and `3875f524a` put a screenshot of the TUI in the
    README and touched two workflow step names to keep the docs honest about it.

### The manual had a renderer, and the render disagreed with the markdown in four pages (`reco-x69`)

`docs/` was published in two places and rendered in one: GitHub showed the markdown, and nothing at all
turned it into a site, so the only reader able to notice a page whose markdown is legal and whose *render*
is wrong was a person, looking at the page. Four such pages were in the tree when this landed — a table row
whose code span holds an unescaped `|` (in `concepts/config.md` and `concepts/noncode-surfaces.md`, where
the regex in `non_code.env.secret_name_pattern` cut the row and the header out of the table), two tables
pasted into the middle of sentences in `concepts/rename-pairing.md`, two stray fence lines in
`reference/cli.md` that dropped `## The verbs` and `## Global flags` out of the page, and five pages that
wrote a code span containing backticks as backslash-backtick, which prints the backslashes because a code
span does not unescape. `check_docs.py` and `check_docs_site.py` were structurally unable to see any of it.

* **`tools/build_docs_site.py`** builds the site: one page per `docs/**.md`, a generated listing for every
    section directory with no `README.md`, sidebar, per-page table of contents, breadcrumbs, a lazily loaded
    search index, `404.html`, a sitemap and robots when a host is known, and every non-markdown file copied
    verbatim so `docs/demo/*.cast` plays on Pages. `write`, `check` (builds in memory, writes nothing) and
    `--self-test`, standard library only, so the same command runs on a laptop, in CI, and on a runner with
    no network.
* **The render is audited against the source.** Each page's characters are compared with the characters its
    markdown contained, so a dropped table cell, a swallowed `<episode>` placeholder or a fence that ate the
    rest of the file is a build failure rather than a reader's confusion. That rule is what found all four
    defects, and all four are fixed rather than worked around in the renderer.
* **CI builds it.** `.github/workflows/docs.yml` builds a preview artefact on a pull request and publishes
    on the default branch; `python3 tools/check_docs_site.py` builds the same site in the contracts run; and
    `tests/test_docs_html_contract.py` holds the promises the HTML makes. The entry is here because the
    commit that did all of this did not add one, two days ago: the coverage rule in `gen_changelog.py`
    noticed, which is the argument for the rule having a number on it.

### A timeline that had been open for an hour still said its newest row was brand new (`reco-5oe`)

The rail's age column printed `0:00` for a commit that landed four seconds ago, and printed `0:00` an
hour later. Two defects were in that one screen and they are different shapes of it. `0:00` is what the
`h:mm` arithmetic says at forty seconds: a spelling whose resolution is a minute, printed as though it
measured seconds, about a row that had not had one yet. And the frame is handed exactly one wall-clock
instant at the CLI's edge — ADR-002 §4.4, because nothing below the composition root may read a clock —
which an interactive run then aged every row against for as long as it stayed open. An episode that
landed *after* the frame opened was dated ahead of it, clamped to the same `0:00`, and read "this
hour" for the rest of the session. A reviewer who believed that column was being lied to by a clock,
not by the index.

* **The bucket.** `days::relative` says `<1m` below a minute and then `0:12`, `2d4h`, `3mo` exactly as
    before, and the card says `just now`, because `<1m ago` is not English. A row the frame has not
    reached — clock skew, a fixture dated forward — says `ahead`, and the card says *dated ahead of
    this frame*, instead of the clamped zero that made a future commit look like the newest thing on
    the screen. The same call [`ambient::age_since`](docs/concepts/ambient-mode.md) already makes with
    `unknown`: an age that cannot be computed is not an age of zero.
* **The clock.** `App::age_clock` anchors the frame's one wall-clock read to the monotonic instant the
    loop already wakes on — `SIGNAL_POLL`, the same clock that ages the toast and the chord timeout —
    and derives "now" from the distance between them. Monotonic is the right instrument for an age: NTP
    cannot step it backwards and make a row younger, and a laptop that slept cannot make an episode
    younger than it was when the lid closed. `run_loop` asks it next to `age_toast`, so the fix costs a
    subtraction per wake-up and no new timer.
* **The ticklessness, kept.** A draw is for a change somebody can see (ADR-006 §0.1), so the answer is
    "draw" only when the instant crossed a minute, and only when the rail is on a rung that prints an
    age at all. Below `Columns::NoSparkline` the age column is a date read off the row and the only
    `now` on the screen is the footer's day label, so there a minute turns nothing and a day does.
    `Rail::set_now` reports the minute crossing; the caller who owns the layout decides whether it is
    visible.
* **What did not move.** A one-frame render — `--plain`, `--json`, every golden — is still the single
    instant it was handed, so a piped frame stays reproducible (ADR-006 §6.7). `fathom-tui` still reads
    no clock: the instant is derived from one the caller already had, which is what makes the minute
    turn testable at all.
* **Tests**: `crates/fathom-tui/tests/timeline_age.rs`, eleven cases with the monotonic instant
    injected so none of them sleeps — every bucket edge, the ahead row, the card's sentences, a rail
    aged forty minutes in one call, same-minute wake-ups that buy no frame, the date-only rung that
    will not draw for a minute, a frame whose instant moved on its own, and a quiet `run_loop` that
    asks its clock and still draws one frame and stops.
* **Docs**: [ADR-006 §5.2](docs/adr/0006-tui.md) states the four spellings and the resolution a redraw
    turns on; [concepts/timeline.md](docs/concepts/timeline.md) says where the instant comes from and
    what carries it forward.

### A Cargo workspace could not answer its own layering question (`reco-842`)

`use fathom_analyze::graph::model::Graph;` written in `crates/fathom-core` produced a graph node and no
dependency row. That was not a corner of the Rust extractor: it is what the extractor did with every
specifier in a workspace, and this repository is a workspace of eight crates. `resolve()` joined the
specifier's segments and asked the index for a *corpus-relative* path, so `fathom_analyze::graph::model`
was looked up as `crates/fathom-analyze/src/graph/model` — a path no corpus holds. The module a file is in
was written the same way, which is the other half of the same mistake: `crates/fathom-core/src/lib.rs` is
the root of the crate `fathom_core`, not a module named `crates::fathom-core::src::lib`. So `use crate::…`
asked for a path outside its own crate, `mod tool;` asked for one outside it too, and a sibling crate name
was out of the corpus in exactly the sense a third-party base is — item 4 of
[the graph page](docs/concepts/architecture-graph.md): a dependency that leaves the corpus writes no row,
the omission is counted, and a rule written between the layers it would have connected has nothing to fire
on. The 41 files in `crates/fathom-cli/src` that write `fathom_core::` wrote no row between `l5_composition`
and `l1_durable_state`. `docs/concepts/layering.md` §2 refuses reading a row like that as a clean bill; this
repository was doing it to itself, which is why **reco-dg7** answered the question with
`[check.severity] layering = "off"` instead of answering it.

* **`seed()` was a stub, and so no Rust file ever declared a crate.** A file is now attributed to its crate
    by the layout Cargo itself defines — the first `src` component is the crate root, the directory above it
    is the package, and the import name is that directory with `-` spelled `_` — and the crate root's file
    (`lib.rs`, or `main.rs` in a binary crate) asserts the name once, for the whole corpus. A binary crate is
    named for its package, not for `lib`, which is the name its users type and the name the extractor had
    never registered. `crate`, `self`, `super` and `mod x;` are anchored on that root, and `file_module()`
    returns a path relative to it, so `crates/fathom-core/src/graph/model.rs` is `fathom_core::graph::model`
    and every `use crate::…` in every crate resolved for the first time.
* **A sibling is resolved inside the crate that names it.** A leading workspace-member name swaps the
    resolution root, and the longest-prefix walk starts at the crate root instead of the corpus root — which
    is also what lets a façade be followed: `use beta_kit::Kit` where `beta_kit/lib.rs` re-exports `Kit` from
    `thing.rs` now lands on `thing.rs`, the file that declares it, with the hop recorded in `via`.
    `use beta_kit::tool;` names a module and not a symbol, so it is stored as a wildcard carrying the
    specifier: which files a module's declaration pulls in is not a fact the language hands you, and
    inventing `tool/mod.rs` would have been the extractor's invention rather than Cargo's.
* **A third-party base still leaves the corpus, and says so.** `serde::Serialize` is unresolved after the
    change exactly as before it; the new rule fires only on a name some corpus file asserts as its own crate
    root. A resolved Rust import names a module, and a module is one file, so the edge is file-granular — the
    same rule Go's package granularity is an instance of, not an exception to it. Nothing here claims a
    package-level edge a Cargo dependency graph would give and this extractor does not read.
* **A clean row now says which kind of clean it is.** `cross_layer_edges` counts the judged rows whose two
    ends carry different `[layers]` tags — the only rows `no_upward` or a `forbid` can disagree about — and
    when it is zero while rules are in force and rows were judged, the row says so instead of printing a
    zero. `edges_checked` was the denominator every finding-count is a claim about; this is the one that says
    whether the rules had anything to bite on, and it is the number **reco-842** could not get.

`fathom.toml` carries `[layering]` rules for the first time: `no_upward`, and one `forbid` per ADR-002 §5.2's
downhill-but-refused edge, over §3's ladder — the boundaries **reco-dg7** had declared unjudgeable, and the
`layering = "off"` line is gone. `tools/check_workspace.py lint` still refuses an edge a Cargo manifest
*declares*; this refuses an edge the source *reaches along*, and `tests/test_workspace_contract.py` now pins
the `[layering]` rules against `tools/workspace_model.py`'s forbidden-edge list so the two cannot drift.
Measured at `7be179f`: `fathom check --only layering` judges 7 222 dependency rows, 869 of them across a
layer boundary, and reports 0 findings — against the 2 462 rows and no cross-crate edge at all that the
ticket measured on this same tree. Twelve tests are in
`crates/fathom-analyze/tests/structure_workspace.rs`, on a fixture workspace of two crates and a façade;
the Rust claims already owned by `tests/structure_core.rs` and `tests/structure_tail.rs` did not move.

### The graph lane stopped paying for the commit tables (`reco-7yq`)

ADR-004 §11.6 budgets the store twice — `store.bytes_per_commit_spine = 4 KiB` for the commit tables,
`store.bytes_per_commit_graph = 32 KiB` for the graph lane — and the harness had one bucket beside the
cache and called it the spine. *not*-spine was `cache`, `cache_input`, `session_state`, `index_run`,
`schema_meta`, so the lane, the blobs and the episode rows all joined `git_commit`'s bill, and §11.6's
second budget had never been measured by anything. The number epic 7 put on the register read
8,273 B/commit against a 4,096 B ceiling with more than half of it a lane that has its own 32 KiB
ceiling, while the commit tables the budget is a claim about sat at 1,376 B — a third of theirs. A
budget measuring something its own rationale does not describe is the defect; which number goes red is
the symptom.

* **`store_split` got the ADR's denominators.** Four buckets — the commit tables (`git_commit`,
    `git_commit_file`, `ref_tip`), the `graph_*` lane, the cache side, and the residue — with every
    `dbstat` page in exactly one of them, so they sum to the file instead of one of them being "what is
    left". An index is charged to the table `sqlite_master` says it indexes rather than to whatever its own
    name starts with, which is how `commit_tree` and `commit_author_time` reach the spine: a name-prefix
    rule loses them, and they are 45 KiB of `git_commit` on this corpus.
* **The lane is measured at last.** `store.bytes_per_commit_graph.small` divides the lane's pages by the
    **revisions the lane holds**, because that is the denominator `s3`'s 22 335 B/revision and `e3.2`'s
    6 041 B use — not by the commits of history. Since `reco-r3x` the lane is a coverage window, so a
    per-commit quotient is 516 B on this corpus and half that on one twice the age: it would describe the
    repository's age and not the encoding's cost. Pages and payload are named as different quantities
    instead of being compared as one (§8.4). The fixture now measures **8,064 B/revision** against 32 KiB
    and **1,466 B/commit** of commit tables against 4 KiB, and the row that was `not measured yet` in
    `benchmarks/BUDGETS.md` is not any more.
* **The measurement point moved, and that is the volatility fix.** How deep the lane has been settled
    depends on how many index runs made the store, which is why the old row read 8,289 B/commit at three
    samples and 10,446 at five. The byte split is read off the store a cold index leaves, before the warm
    commits: a cold store is a function of the corpus and the configuration and nothing else, and the two
    store rows now repeat byte for byte at one sample and at three.
* **The register is empty.** `store.bytes_per_commit_spine.small`'s row in `perf-gate.json`'s
    `known_breaches` said *delete this row when `reco-7yq` gives `store_split` the ADR's three
    denominators*, and this is that change: the row is out of the baseline and out of ADR-004 §11.10's
    table, and both store rows are re-blessed — bytes are `scaled: false`, so no runner factor is
    involved — while `store.size.mib.small` stays where it was, its 3.402 → 3.375 MiB move sitting inside
    the 1 MiB floor §11.10 already admits for it. Emptying the register needed a fix of its own:
    `tools/epic7_acceptance.py` asserted `owners and not unknown and not unresolved`, so a ledger emptied
    by closing every defect on it read as a failure. The epic now says "no id is a breach today" and checks
    the discipline exactly as hard, because an emptied register is the outcome the ledger exists to reach
    rather than an absence of one.

### A `human` nobody credited is no longer scored as a person (`reco-0er`)

The beta pass that pointed Fathom at its own repository wrote this down
([`.plan/evidence/reco-rny/fathom-on-itself.md`](.plan/evidence/reco-rny/fathom-on-itself.md) §2.6):

> Attribution: 102 of 109 episodes credited to a `human` at confidence 0.50, in a repository where a coding
> agent wrote them and the trailer vocabulary of ADR-005 is not stamped by anything here. Consequence:
> `authorship` contributes `0.00` on the risk vector, and will keep doing so in every repository whose harness
> does not co-operate, while looking like a measured zero.

One hundred and two of 109 episodes in a repository a coding loop wrote were credited to a person, at the
confidence the attribution ladder reserves for "a name with no agent evidence behind it" (ADR-003 §8
**H4**). The ladder is not broken — rung 8 is that rung precisely so the guess is visible — but
`risk::collect` read the fall-through as an answer to the axis that asks *is the thing that wrote this a
program* (ADR-001 §7.3), printed "a person wrote this, and is named as having done so", and took the
thesis's axis to 0.00 in every repository whose harness does not stamp turn evidence or a claim trailer.
Which is most of them, and the agent-written ones among them first. One of this repository's own episodes,
before and after:

```text
$ fathom why ep-5d5c29560494ff4ba1b6      # before
  11 of 12 features measured · weight denominator 1.36 · total 49.37 of 100.00 points
authorship      0.00 authorship    0.00    0.05 0.00/3.68 a person wrote this, and is named as having done so

$ fathom why ep-5d5c29560494ff4ba1b6      # after
  10 of 12 features measured · weight denominator 1.31 · total 51.25 of 100.00 points
authorship                    —       —    0.05         — nothing on this episode says who wrote it: `human`
                                                       is what the ladder falls through to when no agent
                                                       evidence exists … owned by the harness
```

* **The axis is absent where it was guessing.** `authorship` is `Measurement::missing` on a `human` episode
    decided by the `identity` rung, so its weight leaves the denominator, `fathom why` prints the `—` row
    and names the axis under *what `features-1` cannot see*, and the sentence beside it names the four doors:
    harness turn evidence, a claim trailer naming an agent, `attribution.agents` declaring the identity, a
    stored `fathom attribution correct`. A `human` that a *correction* asserted is a statement and still
    scores `0.0`; `unknown` still scores `1.0`, because "nobody is credited" is a fact about the row.
    `attribution` is untouched — 1 − confidence was already the honest reading of a 0.50.
* **The adapters were asked whether they stamp, and the answer is written down.** None of them does, and
    none of them can: pi cannot rewrite a commit message after the fact, and a `post-commit` hook that
    amended one to add `Agent-Session:` would move the oid out from under every review already keyed to it.
    The trailer is the agent's to paste, so [`docs/harness/pi.md`](docs/harness/pi.md) says that in the
    recipe, names the cost, and points at the axis; [`docs/harness/git-hook.md`](docs/harness/git-hook.md)
    says it in the row of the table that already admitted what a hook cannot know.
* **What it costs.** In a repository with no harness the score is now computed over eleven axes instead of
    twelve, so every other term is renormalised by `1/0.95` and the ceiling list is one line longer — and
    the line is the point: a hole with a name and four doors beats a zero that reads as safe. A repository
    that genuinely wants the axis at 0.0 can still say so, per identity, with `fathom attribution correct`.
* **The numbers, on this repository.** Before: `authorship` measured on all 133 episodes in the index,
    126 of them at `0.00` by `identity` — the beta pass that filed this put it at 102 of 109, and the ratio
    has not improved since. After: 7 measured, the ones a trailer or a registry entry speaks for, at `0.70`,
    and 126 printing `—`, which moves this repository's own score: `ep-5d5c29560494ff4ba1b6` went `49 → 51`,
    still flagged, on a denominator of `1.36 → 1.31`. `crates/fathom-cli/tests/cli_why.rs` pins both halves
    of that split on a fixture repository, and the collector's own test pins the boundary between a verdict
    and a fall-through.

### Ten findings about files whose whole job is printing (`reco-pfb`)

`fathom export --episodes defc5f6~5..defc5f6` — the five commits that closed epic 6 — carried ten
`debug_leftover` findings, and nine of them were this repository's own `tools/*.py`:

```text
open [finding] debug_leftover on `tools/demo_exhibits.py`: tools/demo_exhibits.py added 7 debug output
line(s) that read as left behind: print(f"{path} is missing; the prose around this block is written, the
block is generated"), …
```

Those prints are the product of those files. `tools/**` is a directory of CLI programs — `check_*.py`,
`gen_*.py`, `*_acceptance.py` — and what they put on stdout is the thing they are for. The same sentence is
true of ruff's `python/py-fuzzer/fuzz.py`, which the published G3 corpus hand-dismisses twice, and of every
`gen_*.py` in every repository this detector will ever read. Tier B had been charging any print in any file,
which is a detector firing on the house's own style, and output that gets skimmed and then ignored costs the
true findings more than silence would have.

* **A tier-B print now needs a shape, not only a marker.** `print(` is not evidence; `print(` plus a shape
    is. The line either prints a value and says nothing about it (`print(row)`, `console.log(user)`,
    `println!("{}", n)`), or it says what it is (`dbg`, `debug`, `trace`, `dump`, `TODO`, `FIXME`, a `>>>`
    gutter, or a value in its debug form — `{x:?}`, `{x!r}`, `repr(`, `.inspect`, `JSON.stringify(`), or it
    goes past the program's output (`eprintln!`, `print(…, file=sys.stderr)`, `System.err.print`). A report
    line is made of words; a leftover usually is not.
* **A file that carries its own entry point is a program.** An `if __name__ == "__main__"` dispatch, an
    `argparse` or a `fn main(` among the added lines silences tier B for that file wherever it lives, which
    is what catches the scripts in a directory no path table has a name for. `tools/**` and `examples/**`
    joined the path table beside `cmd/**` and `bin/**`, and the Ruby marker `p ` now has to start its
    statement — it had been matching every `map {` in the corpus.
* **What was refused.** Scoping the detector to `crates/`: this repository's library happens to live there
    and most repositories' do not, so that is tuning the tool to one tree. And a new ignore list: a bare
    `detectors.suppress` entry already is one, in the same glob language as every other scope
    (`debug_leftover:src/log/**`), and a second mechanism for the same answer is a second answer.
* **What it costs.** `println!("cache hit: {}", n)` in a library is a real leftover and is now silent,
    because at line level a labelled print is indistinguishable from a logger call in a house whose logging
    idiom this file does not know. That is the right way round: one missed `low` note is cheaper than a
    reviewer who has stopped reading the tier.
* **The numbers, on this repository** ([`debug_leftover.rs`](crates/fathom-analyze/src/detectors/debug_leftover.rs),
    [`docs/detectors/index.md`](docs/detectors/index.md)). Over its own 131-episode history,
    `debug_leftover` went **38 findings → 4** and nothing new fired; in the epic-6 window above,
    **10 → 1** and **38 open actions → 31**. What survives is arguable on the line it quotes: the detector's
    own marker list, two deliberately pathological files in the spike corpus, and one fixture-writing line
    whose string literally says `debug leftover`.

### A Python function whose signature had not changed was reported as a signature removal (`reco-9k3`)

`fathom check --only api --base 40a1e3f --head HEAD` over this repository's own history reported seven
error findings, and five of them were the same sentence said about five different functions:

```text
breaking prose_units tools/check_ticket_claims.py:226 — parameter removed — position 1 (`text: str`) is
gone; return type changed — was `list[tuple[int, str]]`, now `(none)`
```

None of those five signatures changed in the range: `rust_units` and `key_pattern` are untouched anywhere,
and `prose_units`, `claims` and `main` changed in their prose and their bodies and kept every parameter and
their return type. The shape of the wrongness named the culprit: the head side carried the item's name and
its true line number and no signature at all, which is what a header read as `def prose_units(text` pairs
against — the parameter's own annotation colon taken for the colon that closes the header, and every
parameter after it falling outside the header.

* **A triple-quoted literal is one literal, not eight.** The byte scanner that reads a declaration's header
    ([`api/textscan.rs`](crates/fathom-analyze/src/api/textscan.rs)) masks the contents of every literal and
    comment so their brackets and colons go inert, and it ended a literal at its newline — right for
    `"short"`, which cannot cross one, wrong for `"""…"""`, which exists in order to. Every docstring line
    after the first went back through the scanner as source, so a quoted phrase in prose opened a literal
    that swallowed the rest of its line, and a bracket pair wrapped across two prose lines was counted on
    one side only. In `check_ticket_claims.py` that left the running bracket depth at −1 for the rest of the
    file, which is how an annotation colon started reading as top level. A literal now ends at its own
    delimiter: a lone quote inside a triple-quoted literal is content, and a newline inside one is content
    too. The same edit fixed the closing test, which accepted any quote followed by a quote two bytes on and
    treated it as three.
* **A bracket depth that has gone negative is a lie with a number on it.** The mask clamps depth into a `u8`,
    so −1 reads as 0 and a scanner reading 0 has no way to know it was told a fib. The fix is upstream of
    that, but it is why the regression tests here assert the *signature*, not the finding: the failure this
    ticket is about produced no error, no gap and no `signature_unparsed` — it produced an empty answer.
* **What is still reported, and rightly.** Two of the seven stand: `DOC_GLOBS` in `tools/` really did drop
    its `: list[str]` annotation, and `resolve` in `src/recommender/store/cache.py` really did lose
    `store_schema` while gaining an optional `row_schema`. The whole gate over the five commits before
    `feb16d8` now reports two findings — that `resolve` change and the untested-logic warning the range
    already carried — where `reco-azy`, one commit back, recorded eight.

`tests/api_surface.rs` carries the pair — a module docstring widened with a quoted list and a bracket pair
split across the wrap, and two functions whose signatures do not move — and asserts the head side still names
both parameters and the return type, and that the diff calls nothing breaking. The scanner's own three tests
are in `api/textscan.rs`: a triple-quoted literal runs to its delimiter, a lone quote inside it is content,
and an unterminated `'` still stops at its newline because that is the rule the newline branch was written
for.

### The `[check] mode` decision this repository had never made (`reco-azy`)

`fathom.toml` declared no `[check]` block, so the gate that runs over this repository ran on its defaults —
`mode = warn`, fail at warning — which are the values this project would probably have chosen and are not the
same kind of thing as choosing them. The gate reported 16 findings on its own HEAD and exited 0, and nobody
had decided whether that was the plan. It is, and it is now in the file: `[check] mode = "warn"`, with the
measurement that decided it and the order of conditions that turn it to `deny`.

* **Why not `deny`, measured at `17770d5`.** The run over the five commits before it reports 8 findings and
    exits 0. Five of them report a parameter and a return type removed from a Python function whose signature
    the diff does not touch, one reports a module constant in `tools/` that really did drop an annotation, one
    is a signature change in the Python reference implementation nobody installs, and one is `tests` pricing a
    change that arrived with its tests under a floor it calls a prompt rather than a proof. `deny` would have
    blocked on all eight, and the only way to make a wrong finding green is `check.ignore` — which is how a
    project ends up with a clean bill of health for a surface its gate never judged. The five are filed as
    `reco-9k3`; the api check's scoping question (is `tools/**` anybody's import surface?) is the second half.
* **What `warn` cannot do, so CI does it.** In `warn` nothing decides an exit code: a run in which every check
    was `blocked`, or which examined nothing, prints `fathom check failed` and exits 0 like a clean one. The
    `check-gate` job now reads the run's `--format json` and fails on a `blocked` row or a run with no `ran`
    row. Findings are that job's results; a gate that did not look is its failure.
* **The half that was invisible from the config alone.** `check-gate` accepts exit `7` whatever the file says,
    so flipping one line to `deny` would have changed the log and not the build. The mode and that tolerance
    are one edit, and `docs/concepts/ci-gate.md` now says so next to the argument for the mode.

No product behaviour changed. `fathom.toml`, `docs/concepts/ci-gate.md`, two workflow files and three tests in
`tests/test_ci_gate_contract.py` — which pin the file, the page and the job to each other so a flip has to be
deliberate in all three.

### Every migration threw the whole derived cache away (`reco-3sa`)

`fathom stats` on this repository's own index said `diff.per_file 3339 lookups, 5 hits, 3316 cold, 3334 puts
— hit rate 0.1%`, with no row ever evicted for budget, and the report that came in read it the way those numbers
invite: a cache that is written and never read. Half of that was wrong, and the store could not say so, because
it printed one rate where it needed two. The answers *were* in the table. Counted properly on the store that
filed the report, 2 686 `diff.per_file` lookups had missed on `key_changed` rather than `cold` — 36 % of every
per-file lookup that store ever made — and 2 686 per-file rows (43 %) were second copies of an input the store
already held. Something was moving the key between runs, and field 9 was the thing: it carried
`meta.schema_version`, the store's own migration counter, so a migration that added a table for the ledger, or
a column for a review note, or an index nobody reads, moved every derived key in the store and paid to derive
the answer again next to the unreachable one. The store's migration counter and the shape of a derived row are
two different facts, and the key had been reading the wrong one.

* **Field 9 is the schema of the derived row**, `cache::ROW_SCHEMA_VERSION` — the highest migration that creates
    or alters `cache`, `blob` or `blob_ref` — and not of the store. A test reads the migration list and fails if
    a migration reshapes those tables without bumping it, so the bump is not a habit someone has to remember.
    Migration 9 indexes `cache` and `blob`; migration 10 gives `episode_delta` a foreign key onto `blob`. Two
    releases' worth of migrations of a table three analyzers read, and neither changes one byte of what is
    stored or how it reads back, so neither invalidates anything (ADR-004 §5.2, §5.6, §9.1; D12 untouched).
* **`fathom stats` now says which kind of miss a miss was.** The per-analyzer line appends *N asked again under
    a moved key* when the count is non-zero, and a line above it prints the read-back share — *read back 1 094
    of 6 556 stored row(s) were asked for again (16.7 %)* — which is the difference between a cache that is
    unused and a cache that is useless. A `path_pair` scope is *asked each question about once* by an
    incremental index, so a low rate with every miss `cold` is the design; the same rate with a moved-key count
    is this defect. `--json` carries `rows_read_back` and `rows_read_pct` against `cache_rows`, the rows the
    table is holding, and not against `rows`, which is the lifetime put count.
* **A budget on the disease, not the symptom.** `cache.rows_orphaned_pct` — stored rows beyond one per
    `(analyzer, scope kind, input digest)` — is a ceiling of zero in the real-repository report, because one
    build running over one repository must never write a second key for one input. The repository that measured
    42.6 % orphaned before this change measures 0.0 % after it, with all 3 727 of its rows read back.
* **The one-time cost, stated.** The first index after this change re-derives every derived row once, because
    every key in an old store moves out from under itself — the same price every past migration was charging on
    every run, paid once now instead. A store that has been through four migrations and never evicted a row is
    exactly the store this was quietly happening in.

### A module and its own directory were counted as a cycle (`reco-wk4`)

`fathom ratchet` on this repository's own source tree reported 13 uncovered cycle violations with an empty
ledger, and 10 of them were a module reading itself: `events.rs ↔ events/schema.rs`,
`drift.rs ↔ drift/detect.rs`, `scorecard/mod.rs ↔ scorecard/report.rs`. Rust gives a module two spellings and
both put its submodules in a directory named after it, so `use super::detect::…` inside `drift.rs` arrives at
the store as a dependency of a file on a file inside itself. The store keys a row on the file because the file
is the finest grain the store can hold, not because a file is the unit of architecture — and a cycle between
`drift.rs` and `drift/detect.rs` is a violation no reviewer can go and fix, in a gate whose whole job is being
believed. The other 3 were fixture corpora: `Circle.java ↔ Shape.java` and friends under `structure/corpus`,
deliberately cyclic, tracked in every commit, and nobody's architecture.

* **A module owns the directory named after it.** `owning_module` in
    [`cycles/flow.rs`](crates/fathom-analyze/src/cycles/flow.rs) folds `d/x/` into `d/x.rs` and `d/` into
    `d/mod.rs`, so a row from a module into its own
    subtree is an internal edge — the same fold the symbol view already does, one level up, and the language's
    rather than an invented one. Three things keep it honest about its reach: it fires only when the module file
    is a module *of this revision* (the dictionary remembers every file the repository ever had, and a live
    directory must not fold under a module deleted last month), it never fires for `lib.rs` and `main.rs`
    because a crate root is not a module named `src`, and it never fires for a Python package, a Java package,
    a Go package or a TypeScript barrel, where the directory is a namespace the language does not fold and two
    siblings importing each other really are two importable units closing a loop. Go keeps its own machinery —
    package-granular claims are still `package_of`'s business.
* **The fold is re-derived, not frozen.** A maintained flow has already forgotten which file a folded edge came
    from, so `git mv src/graph/mod.rs src/graph.rs` — an ordinary refactor — cannot be answered by re-keying
    edges: the flow keeps its dependency rows at graph grain and re-projects them when a module arriving or
    leaving moves the fold for somebody else, which is one view rebuilt, and only by a revision that moves a
    module file. Every scripted episode in `tests/cycles.rs` checks the maintained partition against a cold
    rebuild, and the two refactors that move the fold are scripted there.
* **The grain the module view gives up is demoted, not lost.** A loop that stays inside one module is not an
    architecture claim, and the opt-in symbol view still names the declarations that close it — same rows,
    finer answer, priced as it always was.
* **This repository's fixture corpora are out of the graph, by name.** `graph.ignored_dirs` is the knob and
    `fathom.toml` now names `corpus`, which is what every fixture corpus here is called. That needed a fix
    first: `ignores()` tested *every* path segment, including the file's own name, so `corpus` would have
    dropped the real module `src/search/corpus.rs` on the way to scoping out a fixture — a directory knob that
    eats a file is the config surprise this knob should never be able to pull. It matches directories now, and
    `tests/graph.rs` reads this repository's own `fathom.toml` and asserts both halves: every file of the three
    corpora is out, and `src/search/corpus.rs`, `examples/s1_spike.rs` and the shipped crates are in.
* **A clean tree is a clean gate.** `fathom index --refresh && fathom ratchet` reports `0 violation(s) at
    <oid>: 0 cycle(s), 0 layering` and exits 0; before, it was 13 uncovered with nothing to allow them by.
    A ledger that names a file a fold has since folded away is a stale allowance and the automatic tightening
    deletes it, which is the right outcome but a silent one: a repository with a `ratchet.toml` should re-cut it
    after this. None had one.

[docs/concepts/cycle-detection.md](docs/concepts/cycle-detection.md) has the fold next to the projection it
belongs to, and [docs/concepts/architecture-graph.md](docs/concepts/architecture-graph.md) says what
`graph.ignored_dirs` matches.

### The working-tree walk left the crate that is not allowed to walk it (`reco-p41`)

ADR-002 §3 makes `fathom-analyze` IO-free and names its exemptions exactly — `src/ledger/`, `src/notes/`,
and the ratchet baseline in `src/cycles/debt.rs` — and `lint W1` is the machine that enforces that line on
every commit. Since `reco-6sw` the line had been red: `search/symbols.rs` reached for `std::fs::read_dir` and
`std::fs::read` to fill the search box's symbol section, so `python3 tools/check_workspace.py lint` exited 1
on a clean checkout of main — a step in `ci.yml` and `contracts.yml`, and
`tests/test_workspace_contract.py::test_lint_subcommand_is_clean_on_the_repo_root`, which is the suite ADR-002
names in its own header as the thing that keeps §5's rules honest. That is a §5 R3 break, not a lint bug: the
exemption list is a named list, and `search/` was never on it.

There were two honest ways to settle it, and the ticket named both — grow §3's exemption list in a commit that
says why, or move the walk downhill. The second is the one §3 already prescribes ("analysis reads through
`fathom-ingest` and `fathom-store`", which is exactly how `deps::scan` gets dependency files out of git
through `fathom_ingest::repo`), so the mechanical half of the walk is now `fathom_ingest::worktree`: a
depth-bounded, symlink-refusing, path-sorted [`find`](crates/fathom-ingest/src/worktree.rs) that takes a
`wants` predicate and hands back paths, sizes and last touches, and one `read` that returns an `io::Result`
rather than an empty `Vec` — because "nothing here" and "could not look" are different answers (ADR-001 §7.2).
Nothing about the analysis's decisions moved: `search::symbols` still owns which files are worth opening
(`Policy::wants`, which is `Language::detect` plus the ignore list), which order the caps spend themselves in,
how many bytes and files they bound, and every note the scan prints about what it passed over. The door
keeps one list it is the right owner of — `DEFAULT_SKIPPED_DIRS`, the build and vendor names — and
`Policy::ignores` consults it, so there is still one ignore list and not two.

Nothing a user can compare changed. The stamp is the same fold over the same `(path, size, mtime)` triples, so
a corpus built before this commit is not stale after it; the caps, the gap sentences and the ranking are
untouched, and `crates/fathom-analyze/tests/search_corpus.rs` passes unchanged, including the test whose whole
subject is that `stamp` and `walk` describe one tree. What is new is `crates/fathom-ingest/tests/worktree.rs`
— seven tests for the promises the analysis now depends on: one order, one depth rule, a skip list that is
added to rather than replaced, a symlink that is neither a file nor a directory, and a read that says which
kind of failure it had. The boundary lint is green, which is the point of the ticket: a gate that is red for
one rule teaches nobody to trust it for the other eleven.

### The prose pages joined the rule that refuses a stale ticket claim, one sentence at a time (`reco-cud`)

`reco-j1o` fixed the sentences the binary prints and left the ones the documents print, because a concepts
page carries the same claim in three different tenses and only a person can say which half is history.
Fifteen hits came back from `docs/concepts/`, `docs/recipes/` and `README.md` once the globs were pointed at
them, and they were read one at a time. Ten were provenance wearing a future-tense verb — "`e0.9` owns it",
"a routing result owned by `e4.4`'s weights", "`e3.1` will ship different code" — and now say who built the
thing instead of who is waiting for it. Two were stale in a way that mattered:
[cycle-detection.md](docs/concepts/cycle-detection.md) said no code path writes a `[[allowance]]` from a
cycle finding while naming the ticket that built the writer (`fathom ratchet --update-baseline --reason "…"`
walks the cycle lane through `ratchet::inventory` and writes the row, and it has since `e3.10`), and
[episode-narrative.md](docs/concepts/episode-narrative.md) was quoting a narrative golden that
`reco-j1o` had re-baked, so its citation row still carried the sentence the product stopped printing. One
was true and badly aimed: `fathom config explain` really is not a verb in this build — checked against the
clap grammar, not remembered — but the work `e0.8` closed shipped as
`python3 -m recommender.config explain`, which is what [the recipe](docs/recipes/silence-a-finding.md) now
tells the reader to type instead of a command that does not parse. The recipe names it as `config explain`,
without the `fathom` in front: `test_recipes_name_only_verbs_this_binary_has` refuses a recipe that names an
untypeable verb, and the two rules are now on the same sentence, so the page says what the verb answers
rather than printing a command that fails in the reader's shell.

* **The checker reads prose now.** `docs/concepts/*.md`, `docs/recipes/*.md` and `README.md` joined
    `crates/*/src/**`, and a Markdown file is cut into units rather than lines: a wrapped paragraph is one
    unit (a claim spans the wrap), a list item is its own unit (joining a bulleted caveat list put every key
    in `change-classification.md`'s refusal list in one window with every "not" in it), and a fenced block
    is one unit and is scanned — a stale copy of a printout is exactly what this rule is for.
* **A phrase is matched as a word.** `owns it` used to read "an analysis that owns its own confidence"
    (`summary-card.md`) as a promise; a substring rule turns provenance into a promise, which is the false
    positive that gets a checker switched off.
* **A hit prints the clause, not the head of the unit.** Prose units are paragraphs and whole transcripts, so
    the first 160 columns of a hit were the fence opening. The problem line now quotes the named key with
    its clause around it, which keeps the review a list rather than a hunt.
* **A verbatim transcript is signed, not silently exempt.** A page that copies a shipped printout — the
    `fathom why` ceiling table, a narrative card's citation row — quotes a sentence that is the product's,
    not the page author's, so it carries `<!-- ticket-claims: transcript · why -->` above the block. The
    word is the kind of reading done and the rest is the reason, and `--verbose` lists every attested site
    with it: two exist, both listed, both greppable.

Limits, honestly: the marker is a hole a human signed, and the list is only as good as the reviewer —
`--verbose` is the only thing between it and a rule that quietly stops applying. Two stale keys live in the
product rather than in prose and are out of this rule's reach: the delta's `blast` gap still prints
`owner e3.2`, and `fathom why` still prints `· owned by e4.4` for an axis whose input an episode did not
carry. Both are true provenance today (`e3.2` wrote the graph lane; `e4.4` owns the axis), both are printed
through a `{}` placeholder so the checker cannot see a plan key in them, and if either sentence ever starts
promising work, it will take a Rust change to notice.

### `fathom why` stopped telling you the module graph was unbuilt, because it is built (`reco-j1o`)

The blast axis printed, under *what this score cannot see*:

```
what depends on the touched code — the module graph is `e3.1`'s and the graph delta is `e3.2`'s, so `blast`
is absent from the arithmetic until they land · owned by e3.1
```

Both tickets are closed. `fathom index` writes the module graph, `fathom architecture` measures coupling,
cycles and hotspots on it, and `fathom check` runs two graph checks over it — so in every repository that
had been indexed, the product was explaining a missing number by pointing at work that was already merged,
in the index standing in front of the reader. Four of the five ceiling lines were stale the same way, and
the queue's `blast unmeasured (e3.2)` and the delta's `no module graph in this build (e3.2)` had rotted
the same way. The axes themselves were honest — absent, weighted out of the denominator, never guessed —
and it was the *reason* that was remembered rather than looked up.

* **The blast reason is now a probe of the store it is printed under.** `risk::blast::Availability` asks
    the index how many revisions it has and which of this episode's paths are modules in it, and says
    which answer came back: *no graph lane* (`fathom index` writes one), *a lane with no module for these
    paths* (naming what it did find), or *a lane that could answer and was not asked* — the true one, since
    a per-episode reach figure costs a chain walk no shipped verb pays for. `fathom why`, `fathom episodes
    --explain` and the TUI's `w` all print it, because all three resolve it in front of a store.
* **The ceiling list is composed from the score, not recited.** `risk::ceiling(score)` prints the axes
    *this* read left out, in the words that read produced, then the three limits of the model itself. An
    axis that was measured can no longer be listed as invisible: hand `Supply.blast_radius` a number and
    the line disappears from `fathom why`, its `--json`, `fathom episodes --explain` and `w` in one change.
* **The owner of an absent axis is now whoever can act on it.** The blast axis is `the caller`'s, because
    no ticket is missing; `risk::<feature>::owner()` returns `the caller` / `the graph lane` instead of a
    plan key. Provenance is untouched where it is true — a measured axis still names the analysis that
    measured it, which is what makes the number auditable.
* **A checker refuses the stale kind.** `python3 tools/check_ticket_claims.py` resolves every plan key in
    a shipped sentence through `.plan/keymap.json`, asks `bd` for its status, and fails when a closed bead
    is claimed unbuilt — `tools/check_docs_commands.py`'s rule for a documented command, applied to a
    documented cause. Prose pages carry the same claims and get a human review (**reco-cud**), not a bulk
    reword.

Limits, honestly: the axes are still absent from the arithmetic, because no caller walks the graph per
episode yet — this ticket fixed the *explanation*, not the missing number, and the reason it prints now is
the truth instead of the plan. The probe is per-`fathom why` run and costs a `MAX(revision.id)` plus one
`node_key` lookup per touched path; a repository with ten thousand paths in one episode does not exist
today, and if it does the probe is the cheap half of the walk it is standing in for. Prose pages are not
checked (**reco-cud**), and `tests/test_ticket_claims_contract.py` needs `bd` on PATH to check the board —
without it the check says so rather than passing silently.


### A `fathom.toml` weight key that configured nothing is refused now (`reco-gna`)

Every `fathom why` run in this repository ended with `config: risk.weights.new_dependency: new_dependency is
not a feature of this vector; the twelve are …`. The key sat in this repository's own `fathom.toml` at 0.2,
the axis it named is called `dependency`, and the line had configured nothing since `features-1` (`e4.4`).
The notice was honest and still did no good: it arrives at the foot of a score, on the run *after* the one on
which somebody decided their edit had worked. `[risk.weights]` is an open map in the key spec
(`risk.weights.<weight>`), so nothing in the config layer can tell a weight from a typo — and `python3 -m
recommender.config schema --validate fathom.toml`, the check CI runs on this very file, correctly said
`0 warning(s), 0 fatal`.

* **`fathom doctor` names it, once, with a remedy.** A new untidy finding, `config_ignored_key`, carries the
    sentence `fathom why` prints — from the same parser, so the two cannot drift — and the thing to type.
    Untidy, like `index_dead_rows`: every answer the binary gives is still the answer, and what the finding asks
    for is a line deleted. `fathom doctor` still exits 0; `fathom doctor --strict`, the pre-flight posture, leaves
    `5` (`CONFIG_INVALID`). [docs/concepts/cli.md](docs/concepts/cli.md) has the table,
    [docs/concepts/config.md](docs/concepts/config.md) §6 why the schema is not the layer that refuses.
* **Only refusals go in that list.** `[detectors]` is the other open map, and its reader's notices mix a
    refused id with narration about a switch that worked ("`detectors.disable` turned `x` off for this
    repository"). A list this verb turns into a `--strict` failure may hold refusals only, so those notices stay
    where they are: beside a run that had a catalogue to check them against.
* **This repository's own file no longer carries a dead key.** `new_dependency` is deleted, not rewritten as
    `dependency = 0.2` — moving the number to the live name would move this repository's own scores, which is a
    decision to make on its own merits and not one to smuggle through a key fix. `dependency` runs at its
    built-in 0.07 until someone decides otherwise, and `fathom.toml` says that where the line used to be.
* **The next one is caught in CI, not in a score.** `crates/fathom-cli/src/diagnose.rs` runs its reader over
    this repository's own `fathom.toml` in a test — which failed against the checked-in config until the key was
    gone — and `crates/fathom-cli/tests/cli.rs` drives the verb on a fixture whose config carries a dead weight:
    the sentence, the remedy, the `--json` finding with `severity: untidy` and `exit_code: 5`, the `--strict`
    exit, and the same run leaving 0 once the line is deleted.

### The gate's sign-off finding named an episode no verb could open (`reco-dvu`)

`fathom check` said `episode ep-5e23bd8e8 scores 72 of 100 …`, and the reviewer who typed that id at
`fathom why` got `cannot resolve "ep-5e23bd8e8": no branch, tag, remote branch or full ref name matches it`.
The sentence had been cut to twelve characters and the finding's own `index:episode …` evidence had not:
an episode id is `ep-` plus twenty hex characters (ADR-003 §5), and the verbs that read one — `why`,
`analyze`, `narrative`, `export`, `review sign` — read it *whole*, because the only prefix grammar they
accept is a commit-oid one. Both episode-shaped checks (`sign-off`, `tests`) now print the whole id, in the
sentence and in the evidence, from one binding per finding, so the half a human copies out of a CI log and
the half a machine reads out of the same object cannot disagree.

* **The `fix:` line names a verb that reads the id it prints.** The finding said *read the episode in
    `fathom show`*, and `fathom show` reads a revspec — an episode id has never been in its grammar. It
    names `fathom analyze <episode>` and `fathom review sign <episode>`, which is the headless reading order
    `docs/recipes/review-a-session.md` already gives a reviewer who would rather not touch a terminal.
* **The agreement is a test, not a promise.** `crates/fathom-cli/tests/check_verb.rs` collects every `ep-…`
    token a `check --format json` run prints — the whole document, not only the two fields this fix touched,
    because the bug was one field shortening and the other not — resolves each one with `fathom why` and
    `fathom analyze`, and asserts that a finding's sentence contains the id its own evidence names. A finding
    that reaches for the abbreviated form now fails a test rather than a reviewer at 1 a.m.

### The search box has symbols now, and it says which tree they came from (`reco-6sw`)

`/` indexed four of the six kinds it names. The fifth was a gap: `search::corpus` registered `Kind::Symbol`
as `not indexed`, owned by `e3.1`, and the box printed that sentence instead of pretending the repository
contained no symbol names. That was the honest answer in October; it stopped being the true one the day
`reco-zoz` shipped the extractor. The extractor is a pure function of file text — no filesystem, no clock —
which means somebody still has to decide *which files to read*, and that producer is what this ticket is.

* **`search::symbols` is the producer.** It walks a working tree to depth 6, skips the directories a build
    leaves behind (`node_modules`, `target`, `venv`, every dot-directory, plus whatever `graph.ignored_dirs`
    names, because two ignore lists drift and one is a fact), reads the files whose language a grammar
    speaks, and hands them to `structure::extract`. What comes back is one row per declaration with the
    file, the one-based line, the language and the file's last touch on it — a symbol is a function of a
    **file**, so the row carries no episode and its date is somebody's Sunday afternoon of editing, not a
    turn of work.
* **The deep link became real.** A symbol hit means *go to the file and select it*: `Hit::file` is the file
    the extractor read the name out of, `Destination::Diff { path, episode: None }` carries it, and the shell
    selects that file in the diff pane. The two only sometimes overlap — a worktree declaration and one
    episode's patch are different bodies of work — and where the file is not in the patch on screen the toast
    says so and names `s` instead of parking the reviewer on a file that merely looks right.
* **Every corpus now says what it was read from.** `search::Identity` is a part per source: the store's epoch
    for the episode rows, a digest over `(path, size, mtime)` for the tree the symbols came from. A surface
    that can only ask about one part asks about one — `Index::is_current` and `Index::staleness` answer about
    the parts they are given — so an open shell whose index moved while it was open prints which epoch its
    rows came from rather than answering from them, and the watcher hands `/` a fresh corpus on the same
    update that refreshes the rail. `fathom search` prints the stamp it built from (`corpus 5758b0010f66 ·
    index epoch 4`, and `identity`/`epoch` in `--json`) and re-stamps the tree once per query — a `stat` of
    every file, no parse — so a corpus that is no longer the tree it claims to be says so instead of
    reporting what it did not find.
* **The cost was measured before it went in**, because symbol count is one to two orders of magnitude above
    episode count and `Index::build` is O(corpus) with no streaming. On this repository — 882 first-party
    source files, 17.5 MiB — the default 6 MiB ceiling reads 333 files in 1.89 s of `--release` wall, yields
    8 191 declarations, and costs 15 ms to build; an uncapped scan reads all 882 in 5.27 s and yields 21 697
    declarations, which is past `SYMBOL_MAX` entirely. `Index::build` over 2 000 episodes plus the cap's worth
    of symbols — 22 868 rows, a debug build — costs 133–162 ms and 3 KiB a row, and the keystroke budget
    holds: p95 36 ms against ADR-004's 50 ms. The numbers are printed by an ignored test
    (`measure_the_symbol_scan`) and restated beside the constants they bound.
* **Every cap is a sentence, never a silence.** A scan that passes files over says how many and why; a corpus
    that narrows to `SYMBOL_MAX` reports a `partial` gap with the counts and the order it kept (exported
    names first, because a corpus that must drop 30 000 names needs a rule for which 10 000 they are); and a
    build with no grammar linked, a tree with no file in a language the extractor speaks, and a scan whose
    caps were spent before it reached the source are three different remedies in three different sentences.
    The empty section that used to be a gap is now a section, and the gaps that remain are about the
    repository, not about this build.
* **The ordering did not move.** A symbol-only query returns symbols and nothing else (`kind:symbol`, and
    `sym`), an unfiltered query interleaves by score, and an exact path still beats a fuzzy symbol fragment —
    asserted at the size the section reaches, where the rows are 100:1 in the declarations' favour and their
    names are built from the episodes' own vocabulary.

ADR-006 §5.8's mock now prints a symbol row that names a real thing, `docs/reference/cheatsheet.md` and the
Python keymap follow its `kind:` token list, and `docs/reference/cli.md` says where a symbol hit's file
comes from.

### A shallow clone was told to `git fsck` (`reco-b7o`)

`git clone --depth 140 --single-branch https://github.com/pallets/flask`, then `fathom index` in it. Flask at
`d086db85` walks 429 commits and cannot read 9 of them, because the six grafts the fetch left behind name
parents the clone never received. The first line of the report was honest:

```text
  could not read f39c71609e93: git object 3c85ec953f15 is not available locally (behind the shallow graft
  at 0ec7f713d679)
  9 commit(s) are missing from the index: the objects are the problem, not the index — `git fsck --full` in
  the repository (fathom never fetches or repairs), then `fathom index`, and `fathom doctor` repeats this
  until it stops
```

and then the second line overwrote the cause with a repair order. A graft is not damage: nothing in that
object store contradicts anything else in it, the objects were simply never fetched. A reviewer who follows
the instruction runs `git fsck --full` on a healthy repository, finds nothing, and files the index as
broken — which is `tools/fetch_g3_corpus.py`’s clones and every CI job that clones with `--depth` doing
exactly that, once per run. The medicine for a graft is a deeper fetch, and saying so is the difference
between one command and an afternoon.

* **The reason picks the remedy.** Three causes, three commands: behind a shallow graft → `git fetch
    --deepen <n>` / `git fetch --unshallow`; left out by the clone's `--filter` → `git fetch --refetch` or
    re-clone without the filter; nothing explains it → `git fsck --full`. Only the third prints a repair
    tool, which is the acceptance rule: *fsck is printed only when the store disagrees with something it
    already holds*.
* **The reason survives the trip into the store.** A reason is kept as text (`meta`'s `commit_unreadable.*`
    rows), so the read verbs have the sentence and not the enum. `fathom_ingest::error::Absence` is the one
    place that knows which sentence means which case, `Unavailable::hint()` now delegates to it, and every
    `Unavailable` variant's wording is asserted to carry its own marker — a rewording that dropped "shallow
    graft" would otherwise re-classify every CI clone as a damaged repository, quietly.
* **One finding, four surfaces.** `fathom index` had its own hand-written version of the sentence, which is
    how one degraded repository told three stories about itself. It now prints what `fathom stats`, `fathom
    doctor` and the TUI's notice list print, from one builder, minus the example commits it has already
    named one per line above. `fathom analyze`'s and `fathom show`'s single-commit remedies are read off the
    same reason, so a shallow HEAD is no longer told to fsck in one verb and deepen in another.
* **The count is the object database's, not the run's.** The footer was built from every failure the run had,
    which folds in analyses that failed for a reason of their own — in flask, 4 unreadable commits and 5
    commits whose analysis wanted one of them. It is built from the spine's failures now, which is exactly
    the set the store records and the set `fathom stats` reports five seconds later: 4, not 9. Where a store
    really does have two kinds of hole, the finding counts each and gives both commands.
* **Docs, in the same words.** `docs/concepts/cli.md` has the table row and a section on why the reason
    decides the remedy, because the finding is only useful if a reviewer can predict it.

*Asserted by*: `crates/fathom-cli/tests/resilience.rs`
(`a_graft_cut_by_a_shallow_clone_is_told_to_fetch_deeper_and_never_to_repair`, a fixture cloned with
`--depth` through a merge, which pins the wording in all three verbs and asserts the word `fsck` appears in
none of them; the filtered clone's assertions were moved to the filter's own command for the same reason),
the unit tests in `crates/fathom-cli/src/state.rs` for the graft, filter, damage and mixed tallies, and
`crates/fathom-ingest/src/error.rs` (`every_missing_object_sentence_carries_its_marker`) for the marker
contract. The flask reproduction above is the case the fixture stands in for: a local git will not cut a
shallow window that leaves a parent unnamed, so the fixture is built with a merge and cloned at `--depth 6`,
which does.

### A `g` that waits says so (`reco-wlt`)

The walkthrough measured every view key's frame arriving 0.5–2 ms after the keystroke, and `g`
arriving 250.5 ms late, because `g` is `view.graph` *and* the first half of `gg` — so the shell has
to wait for a second key before it can answer at all. ADR-006 §7.2 accepts that cost, and the cost
is fine: 250 ms is under the interval a deliberate `gg` is typed at. What was not fine is that the
wait was unmarked. A reviewer pressed `g`, got back the same screen, pressed `g` again — which is
now `gg`, a different action in a different mode, in a view nobody asked for — and concluded the
graph view was broken. The help overlay listed `g` as "Go to: Graph" and said nothing about the
timeout; the hint row was two rows up, printing `q Queue · t Timeline · d Diff` at a question that
was being asked right then.

Nothing was missing a frame. The loop already drew one for the keystroke — it was the same frame, and
the pty's `visible_ms` correctly called that "nothing changed for 250 ms". So the fix is not a faster
key; the wait still costs what it cost. It is that the frame the keystroke lands on now says what is
happening. Following the same path turned up a second one: the prefix test was done by *character*,
and the table spells its named keys `<…>`, so `<` — which is `structure.sort_cycle` and
`graph.hop_less` — was held for the full 250 ms waiting on a second keystroke nobody can type. The
ADR's claim that `g`/`gg` is the one ambiguous pair the table ships was true in intent and false in
the code; now it is measured.

* **The hint row carries the mark while the wait lasts**: `` `g` … waiting · `g` alone = Graph in 250
    ms · `gg` = First row `` — the key in the air, what it becomes if nothing else arrives and how
    long that takes, and what the second key would still turn it into. Spelled by
    [`Keymap::pending_hint`] out of the *merged* table, so an override that gives `g` a third ending
    prints it and an override that unbinds `gg` takes the wait away entirely — no mark, no 250 ms,
    `g` opens the graph on the keystroke.
* **A held chord outranks a toast, and only for the length of the wait.** A toast reports an act that
    is over; a chord in the air is the question being asked now. When the chord resolves to something
    that prints nothing, the toast that was there before is back on its own clock (`reco-2ll`), not
    cancelled by an interrupted keystroke.
* **Where there is no hint row, the status row says it.** Glance is `CH_STATUS` alone, `ui.hints =
    false` sells the row for the list, and the ladder sells `D1.hint` at a small size — three ways to
    lose the row the message wants, none of them a licence to go quiet. [`App::hint_row`] is the
    answer, and all nine views are asserted at 120×40 and at 80×24.
* **`?` says it too**, in a generated "held keys" section that prints one line per ambiguous pair,
    in the same words the frame uses ([`Keymap::ambiguous_chords`]). The table no longer promises an
    answer the shell will not give for a quarter of a second.
* **Ambiguity is measured in keystrokes, not characters.** `chord_atoms` splits a binding the way the
    chord machine receives it — each `<name>` is one keystroke, each bare character is one — so `g`
    extends `g`, `<C-w>l` extends `<C-w>`, and `<` extends nothing. That is the difference between
    the ADR's sentence and the code's: `gg` is two keystrokes with the first held, `<C-d>` is one.
* **Measured, with the pty driver, on the shipped binary** (`tests/test_pending_chord_contract.py`).
    Pressing `g` changed the screen **2.6 ms** later, worst of three samples, where the recorded
    walkthrough measured 250.5 ms; every view key — `a d g o p q s t` — came in inside one 16 ms
    frame budget, worst in the run 4.1 ms (`d`). The frame that carries the mark is asserted to be
    the frame the keystroke *produced*, and the graph view is asserted to still arrive when the wait
    ends, with the mark gone with it. The loop-level claim, with the clock inside the process rather
    than outside it, is in `crates/fathom-tui/tests/pending_chord.rs`.
* **Five files had the old silence in their prose.** The walkthrough script's `g` step, the perf
    probe's reason for including `g` at all, the acceptance report's label — which read "p95,
    excluding the `g` chord" and never excluded anything, and now says so plainly — the P3 list in
    `docs/demo/README.md`, and ADR-006 §7.2/§7.5 plus cheatsheet §7 and §9. The recorded run
    (`.steps.json`, the cast, the rendered step table) is left alone: it is a record of the binary
    that ran, and the next recording will read differently.

### A key that cannot apply says why (`reco-xf8`)

The MVP walkthrough did not read the keymap. It pressed a key, redrew the frame, and asked whether
anything was different — which is the only measurement a reviewer can make, and it found eleven of
its forty-one keystrokes that changed nothing at all: `y` and `Y` with no path and no hunk, `<Enter>`
with nothing under the cursor, `x` and `i` with no episode in scope, all of them over an empty rail.
`m` and `w`, a few rows up the same table, answered in a sentence — `owned by e4.4` is an answer,
and so is `marked e8f1a2 — `c` compares against it` — so the shell did have a rule about speaking.
It had simply been applied to some keys and not others, and the uneven half is the half that reads as
a broken key.

ADR-006 §10.3 says *a stated absence, never a blank*, and it says it about panes. The keymap was not
covered, so §10.3 now covers it: **every bound action either changes the frame or prints the reason
it could not**, and the reason is the absence followed by the way out, in that order — the shape the
empty-state copy already uses, because the second half is the half a reviewer can act on. The honest
count, before and after: the shipped registry is 99 actions, the shell ships nine views, and against
an empty model **208 of those 891 (view, action) pairs left the frame untouched and the hint row
silent**. Eighteen do now, and they are the two exempt ids in nine views.

* **The motion family — `j`, `k`, `gg`, `G`, `H`/`M`/`L`, `<C-d>`, `<C-u>`, `<C-f>`, `<PageUp>`,
    the two scroll keys, thirteen ids in fourteen `match` arms — went through one arm.** Not for
    tidiness: a refusal has to know whether the cursor moved, which fourteen arms each had to
    answer for themselves and none did. The arm snapshots `(cursor, window top)`, runs the
    arithmetic — unchanged, now in `App::run_motion` — and if both came out where they were it says
    which of the three it is: *the rail pane has no rows to move over*, with the pane's own remedy
    beside it, or *the cursor is already on the last row of the worklist pane · 12 row(s) · `gg` and
    `G` jump to the ends*, or *the tree pane's 3 row(s) fit the frame — there is nothing to scroll*.
    A document pane (the card) says something different again: it has no cursor for these keys, so
    the sentence names the pane that does.
* **A refusal about an empty pane reuses the pane's copy rather than inventing one.**
    `fathom_tui::empty::refusal` is the line `empty::words` ends on, one row instead of three, so
    the toast and the pane above it cannot drift into contradicting each other about why the rail is
    empty — the mistake `reco-1nu` was filed against, one surface further up.
* **`t` on the view that is already live, `<C-w>1` on the tab that already is, `<Tab>` in a one-pane
    view and `<Tab>` over a zoomed pane** all said nothing, because the frame cannot show any of
    them. They say *Timeline is already the live view*, *tab 1 is already live*, *Ambient has one
    pane, and it is already focused*, *the rail pane fills the frame — `<C-w>o` puts the others
    back*. `set_view` is measured, not assumed: [`App::view_state`] is the six things it can move,
    and a switch that moves none of them is the switch that owes a sentence.
* **The field keys refuse when there is no field.** No chord reaches them there — `prompt`'s keys are
    bound only in `prompt` mode — but the palette dispatches by action id and so does every embedder,
    and `<C-u>` used to silently `prompt.clear()` a box that was not on screen. Now: *no field is
    open — `/` searches, `,` is the palette*, and a chooser is told apart from a field, because
    popping a character off a prompt while the reviewer thinks they are moving a picker is worse
    than the nothing it replaces.
* **The diff's three ways out end in a motion.** `y` used to answer *nothing selectable is under the
    cursor to copy a path from* and stop; it now adds *`j`/`k` walk the tree, `d` opens an episode's
    patch*. Same for `Y` and `E`, which said *no file is selected* — a true sentence with nothing in
    it you can do.
* **`d` again on a patch that is already open says so.** It deliberately does not re-read the store or
    reset the tree's cursor, which is right, and a deliberate no-op is indistinguishable from a lost
    keystroke unless it is announced: *e8f1a2's patch is already open — `j` walks the tree, `[`/`]`
    jump between hunks*. The guard is the same `view_state` check, so `d` from the rail — where the
    view switch is itself the change, and the pane is already printing its emptiness — stays quiet.
* **The rule is a table, in `crates/fathom-tui/tests/action_speaks.rs`.** It walks the registry in
    all nine views against an empty model, and fails on any action that neither moved the frame
    proper — the last row is excluded, because a toast is not allowed to be its own evidence — nor
    left a sentence there. The exemption list is a table with a reason per row and it is re-measured
    every run: `app.quit` and `screen.redraw` pass today *because* they still change nothing, and an
    exempt id that learns to speak fails until its row is deleted. Taking the motion refusal out
    again turns four of the sixteen tests red, so the gate has been seen to bite.

**One golden moved.** `FATHOM_GOLDEN=write` re-baked the five `diff_*` layout screenshots, whose
last row is a toast and always was (*10 episodes from fixture index*): it now carries the refusal
that frame has no patch source, which is the sentence that state deserves. Nothing else in the matrix
moved.

### The toast took the hint row and never gave it back (`reco-2ll`)

The last line of the frame is the hint row: `q Queue · t Timeline · d Diff · / Search · , Command
palette · x Mark reviewed …`. Press `<Esc>` to close the help overlay and it becomes:

```text
help closed — the view behind it kept its selection
```

…and it stayed there. A dozen keystrokes and several seconds later it was still the last line, and a
second toast — `manifest pane fills the frame` — survived ten keys after that. ADR-006 §7.5 makes that
row the surface that makes `?` unnecessary for the common case, so a toast that never expires is not a
message, it is an eviction: a reviewer who dismisses the help once never gets the hints back at all.

There was a lifetime, and it was the wrong unit. `TOAST_FRAMES = 40`, one tick taken off per draw, and
a draw is not a unit of time. The loop is tickless (ADR-006 §0.1): a frame is drawn when something
changed and otherwise nothing is drawn, so "forty draws" meant "forty more changes, whenever the
reviewer deigns to make them" — and on a frame that has gone quiet, which is where a reviewer reads
what the toast just said, never. To finish it off, `age_toast` ran *after* the draw, so the frame that
would have given the row back had to be caused by something else.

* **A toast's lifetime is now a clock: `TOAST_LIFETIME`, six seconds of wall clock.** `App::toast`
    records a deadline (`toast_until`) instead of a countdown nobody can fast-forward, and
    `App::age_toast(now)` retires it and reports whether the hint row changed — which is the loop's
    cue, not the shell's. `App::toast_lifetime` is a field so a test can watch a lifetime pass without
    waiting for one; every real run leaves it at six seconds.
* **An expiry is a change to the frame, and the loop sleeps for it.** `run_loop` ages the toast *before*
    it decides whether to draw, marks the frame dirty when the row comes back, and takes the remaining
    lifetime into the poll timeout beside the ambiguous-chord wait. The reviewer's side of that is the
    ticket's acceptance criterion: read the toast, then do nothing, and the key hints return by
    themselves. Nothing has to be pressed, and nothing is pressed to get them.
* **A resize retires the toast drawn for the size that is gone.** The old behaviour ticked a stale
    message one frame older; the honest one is to take the row back and let the re-asked ladder say what
    the new size can hold, which is what `layout degraded: …` is for.
* **Both halves the ticket asked for are asserted, in `crates/fathom-tui/tests/toast_lifetime.rs`.**
    An unexpired toast wins the row — in `App::hint_line` and as the last line of the drawn frame — and
    one a moment short of its deadline is still there; at the deadline it is gone and the row comes back
    in the keymap's own words, and ageing an already-expired toast is not a change (the loop asks every
    poll and must not repaint forever over one toast). Then the report reproduced end to end: `?`,
    `<Esc>`, and silence on the bus, with the loop run exactly as `fathom tui` runs it — the frame that
    returns the hint row arrives, `report.events` is still three (two keystrokes and the harness's own
    way out), so the row came back with no press behind it.
* **Three files had the old unit written into their prose.** ADR-006 §7.5 now says the row is borrowed,
    for how long, and why a draw is not a unit of time; `docs/reference/cheatsheet.md` §9 says it in one
    line for whoever looks up the hint table; and `tools/tui_session.py` and
    `tests/test_review_state_contract.py` had "forty draws" as the stated reason a pty harness must keep
    the frame that *first* said the toast. The number is seconds now; the capture is still needed, for
    the same reason.

### A conventional-commit subject was printed as a trailer (`reco-2ss`)

`fathom show HEAD~3`, in this repository, on a commit whose message is one line:

```text
  message   chore: gitignore .transcripts/ — session logs land in the working tree and are not artefacts
  trailers  chore: gitignore .transcripts/ — session logs land in the working tree and are not artefacts
```

The subject, twice: once where it belongs and once under the field a reviewer reads to decide whether the
agent's self-report is present at all. The parser was not misreading a trailer paragraph — there was no
paragraph to read. `Trailers::parse` takes the *last* paragraph of a message, and in a message with one
paragraph the last paragraph is the subject; a conventional-commit subject is shaped exactly like
`Key: value`, so every `fix:`, `chore:` and `feat:` commit in the repository was handed a trailer it had
never written. The golden export caught it: three of the 500 fixture commits were filed with
`{"side": "cover the exporter"}` and `{"main": "the last straight line"}` — merge subjects, wearing
trailer clothes. It is the same map that `e1.3`'s boundary rules, `e4.1`'s attribution and the intent
ladder read, so the invention was not only cosmetic: a commit could be split into its own episode because
"this commit carries a trailer", and the trailer was the word `chore`.

`git interpret-trailers` was checked against each shape, and it is the rule the parser now follows: a
trailer block is the last paragraph **and** a blank line has to separate it from what precedes it. A bare
`chore: gitignore foo` prints nothing; so does a one-paragraph `Co-authored-by: X\nSigned-off-by: Y`, even
though both lines look like trailers — the first paragraph is the subject, always.

* **One rule, in both places that had a grammar for it.** `Trailers::parse` and `CommitHeader::trailer`
    each looked for a trailer block, each took the trailing paragraph, and each found the subject sitting
    there. Both now require the separating blank line, and both read a message as *lines*, which fixes a
    second way the old code invented a trailer: a CRLF message's separator is `\r\n\r\n`, which did not
    match the `\n\n` the paragraph split looked for, so a Windows-authored commit with a real trailer block
    reported its subject instead of its trailers.
* **The pair the ticket asked for, at both surfaces.** `crates/fathom-cli/tests/show_trailers.rs` indexes
    one repository with two commits — the report's subject-only message verbatim, and one with a prose body
    plus an `agent-session`/`agent-turn` block — and asserts the text form prints no `trailers` line for
    the first and exactly `agent-session: pi-036, agent-turn: 1` for the second, with the same claim in
    `--json`. Either half alone passes the broken parser: "nothing is printed" is what a parser that never
    finds anything does, and "the block is printed" is what the parser that invented extras also did.
* **The golden file moved, by exactly three trailers.** `tests/golden/e1-2-commits.json` is regenerated,
    not hand-edited (`FATHOM_UPDATE_GOLDEN=1`), and the whole diff is those three `{"side": …}` /
    `{"main": …}` maps becoming `{}` — which is also the check that the fix does not reach anywhere else.
* **Two divergences from git are now written down instead of implied.** A paragraph that mixes trailer
    lines with prose yields no block here where git prints the trailer lines it found, and `Key:value`
    with no space after the colon is not a trailer here. Both are stricter than git and neither can
    invent a trailer, which is the failure mode this parser is judged on; the doc comment now says so,
    because "implements git's rules" without its edges is a claim nobody can check.
* **Existing stores keep the old rows until they are rebuilt.** A warm `fathom index` normalises the
    commits it has not seen (`ADR-004 §6.1`), so the fix reaches a repository on its next new commit or on
    `fathom index --rebuild` — the standing remedy for anything the indexer has learned to read
    differently, and the reason it is a documented command rather than an apology.

*Asserted by*: `crates/fathom-cli/tests/show_trailers.rs`
(`a_subject_only_message_prints_no_trailers`,
`a_subject_with_a_trailer_block_prints_the_block_and_only_the_block`), the unit tests in
`crates/fathom-ingest/src/record.rs` (`a_subject_shaped_like_a_trailer_is_not_one`,
`the_blank_line_is_what_makes_the_block`) and `crates/fathom-ingest/tests/walk.rs`
(`a_subject_that_looks_like_a_trailer_answers_no_trailer`) for the second copy of the grammar. The first
CLI test fails against the code before this change by printing the bug report's line verbatim; the fixture
commits that carry real trailers (`agent-session: s-124`, `co-authored-by: Ada Lovelace …`) are unchanged
byte for byte, which is the half that says the parser still finds what exists.

### The perf gate was timing its own polling loop (`reco-dkr`)

`query.metrics.ms.small` — `fathom stats --json`, 250 ms of budget on a 13 ms verb — measured 14.8, 14.9
and 21.6 ms in three identical runs of one tree, and `query.card.ms.cli.small` swung the same way. A ±15 %
ratchet with a 5 ms floor was being asked to resolve 1.5 ms on a number whose own spread was 6.8 ms, so the
same tree was red or green depending on the hour, and ADR-004 §11.10 had just written that down as an
accepted cost with a re-run-once workaround in the epic harness. It was not the store being bimodal. Every
wall clock the harness took was `t0 = monotonic()`, `Popen`, then a loop of `poll(); sample VmHWM;
sleep(2 ms)` — so the number returned was the wall clock *around the polling loop*, including the interval
itself, the `/proc` read inside it and wherever the scheduler woke the poller. That is 1.7 ms of a 13 ms
verb at p50 and 8 ms of it in the tail: the gate had been measuring its own harness for three epics, and
`tools/perf_realrepo.py` had the same defect with a 5 ms interval.

- **The clock is the child's exit now.** `run_measured` starts the clock before the fork — the process toll
  is part of what the budgets price — and stops it when `os.wait4` returns, which is the instant the child
  left; nothing sleeps in between. Peak RSS moved to a sampler thread at a 0.5 ms interval, which is where
  the fixture's short verbs stop moving: `fathom stats --json` reads 13.4 MiB through a 0.5 ms sampler and
  12.5 MiB through a 2 ms one, because a verb's memory peak lands in the last milliseconds of its life,
  exactly where a 2 ms loop has stopped looking. (`ru_maxrss` is not the free answer it looks like: Linux
  does not reset a thread group's max RSS across `execve`, so a waited-for child reports
  `max(parent at fork, its own peak)` and `/bin/true` reports the interpreter's own 10 MiB.)
- **The read verbs take five samples, and a blessed value now carries the noise it was blessed with.**
  `center`, `scale`, `spread` and `noise` sit beside `value` in `benchmarks/baselines/perf-gate.json`, and a
  change has to clear the ±15 % *and* that floor — this run's own scatter times two, the blessed floor if it
  is wider, the probe's stated resolution if that is widest. The scatter is a median absolute deviation, not
  the range: a range is a maximum, one slow sample in five trebles it, and a floor built on a maximum is
  widest exactly on the runs that should be distrusted and tightest on the ones that can be judged. The ±15 %
  itself did not move and `NOISE_FLOOR` was not raised: what moved is that the per-unit constant is now the
  fallback for a value blessed before floors existed, where before it was the only floor there was. The
  ratchet compares the middle of the samples against the middle the baseline was blessed with, while the
  budget keeps being checked against the p95 — a budget is a promise about a bad run and a ratchet is a
  statement about a changed one.
- **A gate that cannot resolve a metric now names it.** Where a change is past the ±15 % and inside the
  measurement's own noise, the verdict is `ok — cannot classify`, said in the table, in the report under
  `resolution.changes_this_run_could_not_classify`, and as a GitHub warning — not a silent pass, and not a
  red that flips on the next run. And because a blind spot is a fact about the gate rather than about the
  run, the gate prints the standing list too: **IDS THE ±15 % RATCHET CANNOT RESOLVE**
  (`resolution.ids_the_ratchet_cannot_resolve`), which is now seven ids — the five pty probes (a 10 ms
  instrument on a 1–5 ms number), `warm.reindex.noop.ms.small` (the store prints `elapsed_ms` as whole
  milliseconds, so a 4 ms no-op moves in 25 % steps and three ticks of that clock are its floor), and
  `store.size.mib.small` (a 1 MiB floor on a 3.4 MiB store). What guards those ids is their budgets, and
  the ratchet says so instead of guessing.
- **What it bought**: six consecutive gate runs put `query.metrics.ms.small` within ±4 % of its blessed
  value where three samples used to choose between two modes, and five samples of one tree now scatter with a
  robust standard deviation of 0.12–0.22 ms. Three of the ticket's three candidate remedies went in together,
  because fixing the clock does not make a three-sample p95 into a statistic. Three blessed rows moved — the
  two read verbs (15.1 → 13.4 ms, 15.3 → 14.8 ms) and the store-side no-op, whose basis changed — blessed
  from the quietest of three runs and divided by that run's calibration factor as §11.8 requires; the other
  twenty are untouched.

*Asserted by*: `tests/test_perf_budget_contract.py` (the clock does not carry the harness's own sleep,
the RSS sampler catches a peak reached on the way out, the floor is measured per measurement, a 16 %
slowdown on a 15 ms verb is red, the ticket's own 14.8/14.9/21.6 run is not, and an unclassifiable change
says so), and `tools/perf_gate.py --self-test`, which now also proves a one-tick store-clock change stays
green and a change inside a run's own spread is reported rather than decided.
### The no-op re-index was spending a second on history it had been told not to want (`e7.10`, `reco-r3x`)

`fathom index` on a repository that has not changed is the cheapest thing the binary does — ADR-004 §6.6 R1
prices it at about a millisecond store-side, and `reco-7tc` exists to make it that. It was costing **about a
second**, on every repository longer than 32 commits, and the cost was the module graph. `graph.window = 32`
was read as "the newest 32 *owed* revisions", which is a batch: a cold index left the rest of the history
owed, and every later run — including the ones answering `nothing_new: true` — settled one 32-revision slice
of that debt at ~900 ms a go, ten runs in a row on the 500-commit fixture, until the lane had a row for
literally every revision the repository ever had. The report even said so and told the reader to raise the
window, which was an invitation to a lane that the next ten runs were going to fill in by themselves anyway.

- **The window is now a cutoff, which is what the sentence above it always claimed.**
  `Store::graph_pending_revisions` ranks every revision the refs can reach and takes the cutoff over all of
  them, so a pass writes the revisions that are *both* owed and inside the window and refuses everything
  older. The no-op writes nothing and costs **3.8 ms**; a warm re-index with one new commit costs **120 ms**
  against its 150 ms budget; the perf fixture's two worst numbers — the only two warm ones — came back
  inside their budgets and their `known_breaches` rows came off `benchmarks/baselines/perf-gate.json` and
  out of ADR-004 §11.10 with them.
- **Refused is not forgotten.** `fathom index --json` grew `graph.outside_window`: the revisions behind the
  window are counted, in data, on the runs that had the window to compare against. What it does *not* do is
  print a line on every quiet run: 340 revisions older than the window is a fact about a configuration, not
  a per-run event, and a warning that fires on every no-op is a warning nobody reads. A pass that did write
  something and left revisions ungraphed still says so in `graph.partial`, and still names the knob that
  would reach them — and now raising it actually works, which is the difference between a notice and a lie.
- **The lane stopped being a function of the repository's age.** The fixture store held **467
  `graph_snapshot` rows for 503 commits**; it now holds **34 for 502**, because a lane that converges on
  full coverage one window per run is a lane that reaches full coverage. That is a third of the spine
  (`store.bytes_per_commit_spine.small` 8,273 → 5,545 B/commit) and a quarter of the file (4.715 →
  3.402 MiB), on a lane whose whole purpose is the newest revisions anyway: the scorecard, the churn window
  and the hotspot window are all 30 revisions wide, and the default window is one anchor period.
- **One row per tree, while the work list was open.** `graph_snapshot` is keyed by tree and a history can
  reach one tree twice, so the list hands each tree over once — by its newest commit — and the owed total
  counts revisions rather than commit rows. Two commits on a tree were one graph and one debt, not two.
- **The budget was never the wrong number.** Both warm budgets stay exactly where epic 7 set them and the
  register is one row shorter: what moved is the run, not the bar. The window default is unchanged at 32 —
  the same number the three 30-wide read windows need — and a repository that wants the architecture of its
  whole history raises `graph.window` and pays for it in the one run where it asks.

*Asserted by*: `crates/fathom-store/tests/graph_pending.rs` (the cutoff refuses rather than queues, across
four consecutive no-op runs, and a raised window reaches what it refused), `crates/fathom-analyze/tests
/graph_index.rs` and `crates/fathom-cli/tests/index_graph.rs` (the run report, the JSON, and a quiet run
that stays quiet and silent), and `tools/perf_gate.py`'s `warm.reindex.ms.small` / `warm.reindex.noop.ms.small`
against budgets that no longer need a waiver.

### An epic's exit criteria stopped being a close reason and became a command (`e7`, `reco-2ah`)

Epic 7 closes on one sentence with four clauses in it — *published binaries for three OSes, docs a
stranger can follow in five minutes, perf budgets met or consciously relaxed with a written rationale, and
a closed beta feedback loop* — and a sentence like that is unfalsifiable in exactly the way an epic close
needs it not to be. So the close is `tools/epic7_acceptance.py`: four clauses, 48 checks, one JSON report,
`--skip-perf` for the fast pass. Each clause is measured by the seam a release actually walks rather than
by the ticket that owns it — release config → generated workflow → `.deb` → `dpkg-deb` → the binary on
`/usr/bin` → the version the changelog cut; documentation → the binary that prints it → the five minutes a
stranger has; a budget in an ADR → the harness that measures it → the disposition when it does not hold; a
report somebody filed → the bead it became → the commit that fixed it → the release note they read back.
Writing it found three defects and one thing deliberately left unfixed, all of them below.

- **The binaries clause distinguishes three levels of claim and refuses to flatten them.** *Declared*: six
  `dist` targets on three OS families, both Apple triples, a static musl build, four installers. *Built and
  read back here*: `.deb`s written from the release binary, `dpkg-deb -f Version` agreeing on
  `0.1.0~alpha.1`, the package laying down binary + man page + bash/zsh/fish completions, and the extracted
  binary answering `fathom 0.1.0-alpha.1` from the path it was laid down at, with every artefact's sha256
  recomputed. *Not done here*: the publish, and executing anything on macOS or Windows — no public remote
  exists, so the release is built and **rehearsed**, and that is a `note` in the report naming `reco-3gz`
  and `reco-lnd`, not a pass. Also found and deliberately not fixed: `packaging/man/man1/fathom.1`'s VERSION
  line prints the *compiled-in* feature set, so the committed page is reproducible only from the
  `--all-features` build CI's docs leg uses; regenerating it from the release binary would print a diff that
  is a feature list and call it a documentation defect.
- **The five-minute clause runs the five minutes.** The scripted tour (8 steps, 0.2 s against a 300 s
  budget, every step's claim asserted), the clone-and-one-command demo reproducing all 27 commits and five
  pathologies of `docs/demo/five-minute.md`, the committed recording staying current, silent and in frame,
  132 documented invocations parsed by clap, and the quickstart naming the six nouns with the page that
  defines each — including what an episode *is* ("the change of one agent turn") and what it is *not* (not a
  commit, not a branch, not a pull request, not a session). The human half, three outside users doing it
  unaided, is `reco-lnd`'s and is named as blocked rather than claimed.
- **A budget may be over its ceiling only in writing, and the writing has to be alive.** The perf clause
  fails unless every red budget has a row in ADR-004 §11.10 *and* an entry in `perf-gate.json`'s
  `known_breaches`, and it fails the other way too: a relaxation about a breach that has stopped biting has
  to be deleted, not kept. Five separate ways to rot are each a named problem — register and prose
  disagreeing, an unwritten breach, a quiet row, an expired row, an ownerless row — plus the ceiling the row
  itself sets. `tests/test_epic7_contract.py` provokes all six with inputs, because a discipline that only
  ever passes on the committed tree is a discipline nobody has tested.
- **Two of the three consciously-accepted budgets were never a warm-path problem.** `warm.reindex.ms.small`
  at ~1,560 ms against 150 ms and `warm.reindex.noop.ms.small` at ~1,050 ms against a 10 ms no-op budget are
  the same thing: a cold index leaves `graph.window`-sized debt (340 owed revisions on the fixture corpus)
  and every subsequent quiet `fathom index` settles 32 of them at about a second — including the runs that
  answer `nothing_new: true`. At rest the same two measurements are ~100 ms and 2–4 ms, and both pass.
  **`reco-r3x`** owns the backfill; §6.6 R1's no-op is intact and the budget was right all along.
- **The third was a denominator, not a size.** `store.bytes_per_commit_spine` measured 8,273 B/commit
  against 4,096 B, of which 6,865 B/commit is `graph_*` — 49.8 % of the store is the graph lane while the
  commit tables are 1,376 B/commit — and ADR-004 §11.6's own budget for the lane
  (`store.bytes_per_commit_graph`, 32 KiB/revision) had *never been measured by the harness at all*.
  **`reco-7yq`** owns giving `store_split` the ADR's three denominators.
- **The third finding is about the gate, and it is the one that will bite next.** `query.metrics.ms.small`
  measured **14.8, 14.9 and 21.6 ms** in three identical runs of one tree: bimodal, and wider than a ±15 %
  two-sided ratchet with a 5 ms floor can see — and there is no middle of the spread to bless, because every
  sample is one mode or the other. Nothing was loosened. The fixture gate is now re-run once when it comes
  back red and the verdict must agree twice (a real regression is red both times), which the tool states in
  its own notes, and **`reco-dkr`** owns the fix: more samples on the read verbs, a per-measurement noise
  floor, or finding out why `fathom stats --json` has two modes.
- **The baseline was re-blessed at `36b011f`, with its basis intact.** `--bless` was run *without*
  `--skip-benches`, which matters: a `--skip-benches` bless quietly rewrites the baseline as 17 measurements
  instead of 22 and drops the five criterion rows, and `test_the_baseline_blesses_every_measurement_the_gate_runs`
  is the test that catches it — that test was red mid-close here and is the reason the bless was redone.
  `benchmarks/BUDGETS.md` now renders the breach register with its ceilings and expiries, so the page a
  stranger reads and the file the gate reads cannot disagree, and `tools/gen_budgets.py check` is one of the
  clause's 48 checks.
- **A gate that crashes writes no report, so it no longer crashes.** Pointed at a file that is not a
  binary, `subprocess` raises `OSError` — "permission denied", "exec format error" — and the clause died
  with `the clause ran without an internal error` where its verdict belonged. Running the candidate now goes
  through one helper that turns the OS refusing into exit 127 with the reason, so every clause that needs a
  working binary goes red with a sentence, the JSON is still written, and the exit code is the clause's, not
  the interpreter's.
- **What this close does not prove is in the report, not in a footnote.** One OS executed; nothing published;
  the real-repository numbers are one machine, one corpus, one commit, with two repositories cut by a
  deadline and reported as bounds; `0 external reporters`, because this repository is its own beta
  population until there is something to install; and 111 of 114 live episodes still unsigned, which
  `fathom review status` prints with exit 7 every time anyone asks.

*Asserted by*: `tests/test_epic7_contract.py` — the four clauses end to end against the release binary, the
harness pointed at a file that is not a binary (red, exit 1, report written, no crash), and six provoked
mutations of the perf ledger. `tools/epic7_acceptance.py` prints and writes all of it;
`docs/concepts/release.md` §9 now names it as the second epic-exit gate after `tools/epic6_acceptance.py`,
ADR-004 §11.10 carries the reasoning behind every relaxed number, and `.plan/keymap.json` grew `e7.10`
(`reco-r3x`), `e7.11` (`reco-7yq`) and `e7.12` (`reco-dkr`).

### The changelog quoted a number that could only have been true for the commit that wrote it (`e7.9`, `reco-0ip`)

The suite had exactly one red and it was the beta loop's own gate:
`test_beta_loop_contract.py::test_the_generator_says_which_rung_of_the_fallback_ladder_each_bullet_came_from`
demanded that `CHANGELOG.md` contain `tools/gen_changelog.py audit`'s rung-distribution line **as of right
now**. That line counts the commits since the last cut, so it moves with every commit that lands, while the
quote sits in prose under `[Unreleased]` describing the tree that introduced the generator. Verified at
`1e51073`: the file said `header 1, bead 70, key 5, subject 1` over 77 commits, the audit printed `bead 71`
over 78. The sentence was true when it was written and false from the next commit onwards, and the only
commit that could have made it green was one that edited the quote to include *itself* — predicting its own
rung before it existed — which would have gone red again one commit later. That is not a stale file waiting
to be refreshed, it is a self-reference, and a red nobody can clear is a red that stops being read: the next
contributor learns to ignore the one failure the suite has, which is exactly the failure that was worth
reading.

- **The claim now names its tree.** The number is no longer a sentence about now. It lives in a marked block
  — `<!-- gen_changelog:rungs from <sha> to <sha> -->`, the audit's own line, a closing marker — and the
  marker carries the two endpoints of the range that was counted. The claim is then "these notes were true
  for the tree they were written against", which is checkable on every commit forever and rots into a named
  commit rather than into a failure. The block in `reco-rny`'s entry names `c05860b`, and the commit reading
  it is already one past that tree — that is the normal state, not a red, and the note says so.
- **Refreshing it is a tool act.** `python3 tools/gen_changelog.py audit --bless` re-counts the live range
  and rewrites the block in `[Unreleased]`, then audits as usual. The reason it is a command and not a hand
  edit is the same trap seen from the other side: a human updating a quoted count by hand cannot know the
  number that includes the commit they are about to make, so the hand edit is always one behind — which is
  precisely the thing that made the prose version unfixable.
- **`C06` reads the claim back, in the file that quotes it.** `audit` resolves each block's endpoints,
  counts the range again and fails on a mismatch with both numbers in the message and the remedy named; it
  also prints how many commits behind HEAD the claim has fallen, because staleness is information and
  wrongness is a defect, and the gate is only allowed to refuse the second. Marker fragments that do not
  parse are a complaint too, so a half-typed block cannot quietly become an unchecked no-op.
- **A released section's claim is frozen with its release.** `--bless` touches `[Unreleased]` alone: the
  numbers under `## [0.1.0-alpha.2]` describe the tree that release shipped, and rewriting them would be
  revising history rather than re-reading it. Frozen is not exempt — `C06` still counts those ranges on
  every run, and `self-test` proves a tampered number inside a cut release goes red.
- **What the prose says now.** `reco-rny`'s entry used to promise that "this file quotes it, so the number
  cannot silently rot". It now says what is actually true: the number rots on schedule, the block names the
  tree it rotted at, and the test is the thing left complaining — which is the whole point of having it.
- **What did not move is the ladder's actual promise.** Every commit since the cut still has to be accounted
  for (`C03`), the exemption list is still shrink-only (`C07`), and the guessing rung is still kept rare —
  `audit` still refuses a release where bare prose subjects reach a tenth of the range. Only the *quoting*
  of the distribution changed: it is a diagnostic printed from a named range, and no longer a fact pasted
  into a paragraph.
- **The plan grew a key.** `e7.9` = `reco-0ip` joins `.plan/keymap.json`, because the rule that [the release
    runbook](docs/concepts/release.md) now states names the ticket behind it, and `tools/check_docs.py`
    refuses a document that cites a bead the plan has never heard of — which is the same rule from the other
    side: a citation a reader cannot look up is a citation that will silently rot.

*Asserted by*: `tests/test_beta_loop_contract.py` —
`test_the_generator_says_which_rung_of_the_fallback_ladder_each_bullet_came_from` (the live distribution is
still rare-on-the-guess-side and still printed, and every claim in the changelog is now checked against the
range it names rather than against HEAD) and
`test_a_hand_edited_rung_distribution_in_the_changelog_is_refused` (a quoted number that its own range does
not count is a `C06`, and `audit --bless` is what fixes it — run on a copy, so the repository's own block
is never touched by its own test). `python3 tools/gen_changelog.py self-test` grew the fifth control for
the same reason: a hand-written claim with wrong numbers is refused, blessing re-records it for the range
it names, blessing twice writes the same bytes, a newer commit does not invalidate it, and a tampered claim
inside a *released* section still fails.

### The queue's contract went red because the harness read the screen before there was a screen on it (`e1.12`, `reco-2vq`)

Three tests in `tests/test_risk_ranking_contract.py` were red on a clean tree, all three saying the same
impossible thing — *`the queue drew 0 ranked rows: q`* — on the 500-commit agent fixture this file has
always used, the one whose queue answers README's question: which three episodes are the riskiest, and
why that one. The queue was not empty. Driven by hand against the same binary and the same store, it draws
**27 ranked rows** on the first screen: rank column counting 1…20, scores descending, `order: queue-1`
named in the strip, the destructive migration and the live-key workflow both inside the first pane with a
reason that cites the file. What was empty was the *screen the harness looked at*, and the cost of that is
the ticket: a suite that reports "nothing ranked" when it means "nothing drawn yet" is a suite that will
be believed, and the two are a product defect and a test defect apart.

A pty is quiet before it is settled. `tools/tui_session.py`'s `drain` reads until the terminal goes quiet
for two ticks — the right rule for a warm TUI, whose frames are budgeted in single-digit milliseconds
(`query.queue.ms`, `query.card.ms`) — and a *cold* one spends its first fraction of a second opening the
store and computing a first frame with the terminal still untouched. Types struck in that window are not
eaten by raw mode, they are echoed by the line discipline, which is why the failure carried a lone `q` on
an otherwise blank screen. Three of the file's five tests sampled that; the two that pressed a second key
saw the frame arrive and went green — which is how the known-red list at the beta gate ended up naming two
tests in this file that had nothing wrong with them while three others were red.

* **The harness waits for a frame, and for *the* frame.** Before any keystroke, `_run` waits for a screen
    at all — for *a* frame, not for the answer the test wants — and a binary that never hands over the
    terminal is now reported as *`drew nothing at all in 20s`*, which is a different claim from an empty
    queue and gets a different sentence. After each keystroke it waits for the grid to **change**, or for
    the window to run out, so a keystroke that legitimately redraws the same pane (`q` on a queue that is
    already the home view) is a settled no-change rather than a dropped key. `drain`'s own report is what
    makes this cheap: it already returns when the screen changed, and the old code threw that number away.
* **What this file does *not* claim is a latency budget.** Time to first paint is a measured, budgeted
    quantity — `cold.queue_visible.ms`, 3,000 ms at `small`, and the blessed baseline is 26.6 ms, which is
    also why this never failed in CI: the contracts job builds release, and 26 ms lands inside the quiet
    window by luck rather than by design. This suite reads whatever binary is on disk (`FATHOM_BIN`, else
    release, else debug), where a debug build on a loaded machine is the slowest thing in the house — and a
    contract that only holds on the faster of two supported builds is a contract with a coin flip in it.
    Making the test depend on how fast the binary starts would have traded a red that was easy to read for
    one that is impossible to reproduce.
* **A child that will not exit is stopped, not waited for.** The cleanup gave the TUI a beat after `<C-c>`
    and then killed and reaped it, which is what lets the test below finish in seconds instead of hanging on
    a wedged binary — and a TUI left running between tests is a TUI still holding the fixture's store.
* **The distinction is asserted, not promised.** `test_a_TUI_that_never_hands_over_the_terminal_is_reported_as_no_frame`
    runs the file's own harness against a stand-in that takes the screen and never draws in it, and requires
    the *no frame* message rather than a row count of zero. Red against the harness before this change;
    it is the test that keeps this ticket's confusion from coming back.
* **And the gate's known-red list is re-recorded**, because the ticket's real finding was the list and not
    the three tests: `.plan/beta-gate-reco-69u.md` §2 now says which of the six it recorded are green, what
    was red when the whole suite was re-run (`python3 -m pytest -q`: 2,141 tests, one red), and names the
    red that is left — `test_beta_loop_contract.py::test_the_generator_says_which_rung_of_the_fallback_ladder_each_bullet_came_from`,
    the changelog quoting a rung distribution that counts commits and so can only be true for the commit
    that wrote it, filed as **`reco-0ip`**. A stale known-red list is worse than none: the next reader
    checks the named ones, watches them pass, and calls the suite clean.
* **The register moved with it.** W43-26 gets W43-29 as `triage:fixed` and the surviving red gets W43-30 as
    `triage:ticketed`, because a changed disposition is a new row and never an edit
    ([`.plan/intake/README.md`](.plan/intake/README.md)).

*Asserted by*: `tests/test_risk_ranking_contract.py`, the whole file — the three queue-screen tests above,
which are the contract (a ranked worklist, its top three operator-action work, the two planted risks on the
first screen), and the new no-frame test. All six are green against a binary built from this tree; the
three queue tests were red before it, and they are red again if `_run` is changed back to reading the
terminal once it goes quiet.

### A test function is not a public API removal (`e3.12`, `reco-u1i`)

Pointing the shipped gate at this repository's own history — `fathom check --only api --base c025320^
--head c025320` — reported one breaking change, and it was not one:

```text
error [api/breaking] breaking test_the_gate_can_go_red_twelve_ways
  tests/test_release_contract.py:78 — public item removed — fn test_the_gate_can_go_red_twelve_ways
  was last seen at tests/test_release_contract.py:78
```

What happened in that commit was that a test was renamed to say what it now covers, which is what
maintenance in a test file looks like. Nothing outside the file imports it, the only thing that loads
it is the runner that collects it by name, and the api extractor had been reading `tests/` as an
exported package because nothing had ever been told that Python's export rule — *module-level, no
leading underscore* — is a rule about library modules and says nothing at all about a file pytest
picked up by name. The cost of leaving it is not one wrong line: it is the run in which somebody
`[api] ignore`s a quarter of the repository to make the noise stop, after which the removals in the
library go unreported too. A gate that objects to maintenance is a gate that loses its authority.

* **The decision: a file the language's test convention claims is not on the surface.** Cargo's
    `tests/`, pytest's `test_*.py` and `conftest.py`, Go's `_test.go`, Maven's `src/test/java`,
    RSpec's `spec/`, Node's `__tests__` and `*.test.ts` — each declined with the reason it is
    unimportable, in a table (`api::testpath`) that is documentation and code at once. The ticket asked
    for a decision rather than a silence, and a decision means saying which of the two it is: the test
    tree is **not** a surface.
* **Three edges do the work, and each is tested.** *The unit is the file*, so no per-symbol guess about
    what "looks like a test" is ever made. *The name convention is consulted only where the runner
    consults it*: `test_*.py` is a file pattern, so a `def test_square` in an ordinary module stays on
    the surface — which matters here, because this repository's fixture corpora are full of exactly
    that shape. And **`tests` is a directory marker where `test` is not**: `django.test` is published,
    supported API, `django.tests` is not, so Python gets the plural and the file names while Ruby,
    Java and Node get their `test/`.
* **Stated, not assumed** — the same discipline `[api] ignore` follows. Declined paths are counted and
    named with the rule that claimed them (`tests/test_release_contract.py (test_dir:tests)`) on the
    byte fast path and in the report, so the cheapest answer says *"3 of them are test files, which are
    not a surface"* instead of quietly not reading them. It is a caveat and never a *gap*: a decision
    about what a surface is does not make the answer incomplete, and it must not cost the fast path.
* **`[api] track_tests` is the way back**, because a rule the language makes still has to answer to the
    repository whose tests *are* the artefact — a compliance suite shipped as a package, a conformance
    suite other projects link. `false` is the built-in; `true` is a stated claim, and `fathom config
    explain` prints which layer won.
* **Both versions are recorded.** `surface::SCHEMA_VERSION` and `extract::RULES_VERSION` go to `2`:
    a row written before this bump carries test items this build will not write, and a stored answer
    has to be able to say which rules made it.
* **Verified on the case that failed, not only on fixtures.** The command above now reports **no api
    findings** (54 surface items compared, down from 85 — 210 paths declined as test files, named in
    the note). And because the key is the honest inverse, the *same binary* reproduces the original
    finding verbatim with `track_tests = true` written in `fathom.toml` — including
    `crates/fathom-cli/tests/cli.rs` and the contract-test files coming back onto the surface. That is
    the fix and the escape hatch demonstrated with one build, on the commit that started it.
* **The plan grew a key.** `e3.12` = `reco-u1i` joins `.plan/keymap.json` — epic 3 owns the surface — and
    the beta register appends W43-28 as `triage:fixed` citing W43-02, because the register's rule is
    that a changed disposition is a new row and never an edit.

*Asserted by*: `crates/fathom-analyze/tests/api_surface.rs`
(`renaming_a_test_function_is_not_a_breaking_removal`, `a_declined_test_tree_is_counted_and_named`,
`track_tests_puts_the_test_tree_back_on_the_surface`, `a_test_shaped_name_in_an_ordinary_module_is_still_surface`,
`a_python_module_named_test_is_published_api`, `every_language_owns_its_own_test_convention`,
`a_declined_test_file_still_counts_as_its_language_being_present`) and the unit
tests in `crates/fathom-analyze/src/api/testpath.rs` and `src/api/policy.rs`. The first three are the
fix — they cannot pass against the code before it, and the CLI run above is the same claim at the
command line. The fourth and fifth pass before *and* after on purpose: they are the boundary that must
not move, which is the difference between a rule with an edge and a glob with a shrug.

### The PR artefact refused to certify itself when two episodes shared a finding (`e5.10`, `reco-c2i`)

Pointing the shipped binary at this repository — `fathom export --episodes HEAD~5..HEAD --format markdown` —
wrote the artefact and then declined to vouch for it: *self-check: UNVERIFIED — 4 problem(s) … checklist item
`finding:untested_new_logic:crates/fathom-cli/src/verbs.rs` appears twice*. The range is five episodes, the
same detector fires on the same path in more than one of them, and the checklist id was built from
`(kind, subject)` alone — so one id named two items, and the self-check, which reads an id as the identity of
the ask it belongs to, had nothing honest left to do but refuse the document. The artefact is the one thing in
this product a reviewer is obliged to trust, and the shape that broke it was the shape a release review is
about: a week of work, five episodes, the same generated file hand-edited twice. A one-episode export was
clean, which is why nobody had seen it.

* **A finding's checklist id is `finding:<episode>:<kind>:<subject>` now** — the same tuple the findings' own
    print order uses, with the subject last because a path is allowed to contain a colon. One item per
    `(episode, kind, subject)` rather than one merged line with a count, because the ask is per episode: each
    copy is closed by *that* episode's decision, names that episode in its detail line, and jumps to that
    episode's diff. A count would have kept the symptom and thrown away the closure, and the closure is the
    half a reviewer can act on.
* **And no two checklist items can share an id, at all.** Every source builds its id from the whole key of the
    fact it came from, which is what makes a collision rare; `checklist_of` now ends with a pass that gives any
    collision left a positional suffix (`…#2`), counted in the print order the record already promises, so two
    builds of one input still write the same bytes. That covers what no source can rule out from here — a
    plugin that reports one subject twice inside an episode, a ledger that prints the same problem twice.
* **The self-check says so out loud.** `CHECKS` grows a ninth line — *no two checklist items share an id, so
    every ask is one line and one line only* — so the guarantee is printed in the artefact next to the verdict
    rather than enforced silently by code a reader cannot see.
* **Fixed on the real case, not only on a fixture.** The same command over `HEAD~20..HEAD` of this repository
    — 18 episodes, 177 findings, nine `(kind, path)` pairs that appear in more than one episode — went from
    `UNVERIFIED — 17 problem(s)` to ``verified: 18 episode(s), 177 finding(s), 2 sign-off(s), 9 check(s)``,
    with 177 checklist items and 177 distinct finding ids.
* **The beta register moved with it.** W43-01 was `triage:ticketed`; this appends W43-27 as `triage:fixed`
    citing it, because the register's rule is that a changed disposition is a new row and never an edit
    ([`.plan/intake/README.md`](.plan/intake/README.md)).
* **And the ticket got a plan key.** A discovered ticket the documents want to name takes the next key in the
    scheme of the epic that owns the thing — epic 5 owns the artefact, so `e5.10` = `reco-c2i` joins
    `.plan/keymap.json`, which is what `tools/check_docs.py` checks a citation against.

*Asserted by*: `crates/fathom-analyze/tests/export.rs`
(`two_episodes_sharing_a_finding_export_verified`,
`a_finding_two_episodes_share_is_one_checklist_item_per_episode`, `no_two_checklist_items_ever_share_an_id`)
and `crates/fathom-cli/tests/export_verb.rs`
(`a_range_whose_episodes_share_a_finding_still_certifies_itself`: a two-episode repository in which both
episodes put a `TODO` on `src/lib.rs`, exported end to end, required to certify itself and to print one ask
per episode). All four fail against the code before the fix and pass after it.

### The beta loop: one channel, a changelog that cannot forget a commit, and Fathom reviewed by Fathom (`e7.7`, `reco-rny`)

A beta is not a version number, it is a loop: somebody reports, somebody dispositions it, the fix is published
with the reason, and the tool is pointed at its own repository often enough to be embarrassed. All four parts
had owners before this ticket and none of them had a gate, which is the difference between a process and an
intention. Everything below is either a file the tracker serves, a command CI runs, or a register with a
verdict in it.

- **The changelog is generated from commits, and never regenerated.** `tools/gen_changelog.py` reads `git log`
  and produces release notes: conventional-commit headers (`feat`/`fix`/`perf`/`refactor`/`docs`/`security`,
  `build`/`ci`/`test`/`chore` excluded, `!` and `BREAKING CHANGE:` promoted into **Breaking changes**), plus
  two things this repository needs — `Changelog:` / `Changelog-Section:` trailers for commits whose subject is
  an essay, and a *fallback ladder* for this repository's actual style, which is prose sentences and not
  conventional headers. `audit` prints the rung distribution on every run, and this file quotes it — in a
  generated block that **names the range it counted**, which is the shape `reco-0ip` had to give it: a
  distribution counts commits, so a copy of it pasted into prose is true for exactly one commit, the one that
  wrote it, and false in every commit after. So read the block below as a claim about the tree it names and
  not about the moment you are reading it; `audit` counts that range again on every run (`C06`) and says how
  far behind HEAD it has fallen, and `audit --bless` is the act that re-records it, because a human cannot
  be asked to guess a number that includes the commit they are about to make. The `subject` rung — the one
  where the notes are guessing at what happened — is kept rare on purpose, the gate refusing a release where
  it reaches a tenth of the range, because a generator whose notes are mostly guesses is a generator that
  gets replaced by a human who is worse at reading `git log`. The gate is coverage,
  not style: `audit` resolves the previous release (the tag, or — this clone has no remote to push tags to — the
  commit that added the last `## [version]` heading) and fails on any commit the notes do not account for, by
  bead ref, plan key, short sha or `Changelog:` text. `cut` runs the audit before it renames a heading, so **a
  release that dropped a ticket is now a red build rather than a surprise**; exemptions live in
  `.plan/changelog-coverage.json` with a reason and an owner, and `C07` makes that list shrink-only.
  `self-test` performs two consecutive cuts — with their refusals — in a throwaway repository, because the
  criterion is that the process is *followed twice* and one release has been cut on this tree.

  <!-- gen_changelog:rungs from 0f7495f3b5f35fed82982551d9b380c9e4a392b7 to c05860b6c6ddf8d95224a624ea4882101ef453e4 -->
  which rung of the fallback ladder produced each commit's bullet: header 1, bead 72, key 5, subject 1
  <!-- /gen_changelog:rungs -->

- **Five shipped tickets had no changelog entry, and that is the audit's first finding on its own history.**
  See the next section: the release body is this file, so an unnamed ticket is a ticket nobody heard about.
- **One feedback channel, two forms, and a required diagnostic.** `.github/ISSUE_TEMPLATE/` grows
  *Something is wrong* and *Beta feedback — how it went*, blank issues are switched off, and both forms require
  `fathom doctor --json` — with the instruction to read it before sending, because it carries the reporter's
  paths and the whole difference between telemetry and a bug report is that a human looked at it
  (`docs/concepts/feedback.md` §2). `tools/check_beta_loop.py` makes that structural: `B02` fails a form whose
  doctor field is not `required: true` or never mentions redaction, `B03` fails the seven dispositions if they
  differ between code, labels and documents, `B04` fails any page that tells a reader to file something without
  linking the channel, `B05` fails a cadence with no weekday, no rota, no 14-day `needs-repro` clock or no
  publication rule, and `--self-test` breaks nine of them on purpose.
- **The cadence, written down and followed.** `docs/concepts/release.md` §9 is the runbook's cadence: a weekly
  `alpha.N` cut on Tuesday whether or not anything interesting landed, a patch when a user's defect closes, a
  minor at an epic's close, plus the two rules that make it cheap (nothing waits for a release, and a published
  release is never edited). §10 states what a release publishes about itself.
- **A report becomes a bead by command.** `tools/beta_intake.py draft --report <file>` reads a filed report in
  the shape the form renders, refuses the six ways a report is unusable (no diagnostic, an un-redacted
  `/home/maria`, no symptom, an `n/a` on a bug report where nothing was measured…), and prints the `bd create`
  that captures it — the `triage:ticketed` act, with the release-notes obligation in the bead text. The weekly
  register is `.plan/intake/<ISO-week>.md` — one row per report, one of seven dispositions, an owner where one
  is owed, a bead or decision named — and `beta_intake.py ledger` validates it and prints the counts the
  acceptance criteria are written against.
- **The first register has 26 rows, and says which criterion it does not meet.**
  `.plan/intake/2026-W43.md`: 12 findings against Fathom's own repository — 11 from running the built binary over
  this tree and one from re-running the gate's own recorded red list — 7 of this repository's open defects
  re-dispositioned, 4 calibration and taxonomy gaps the `m1` gate left open, 3 things CI had said that nobody had
  re-read: **20 `triage:ticketed`, 2 `triage:fixed`, 4 `triage:by-design`**, no row without a disposition, and a
  named owner on every row (for most of them the owner is *the rota* — a role, not a person, which is what a project
  this size can honestly write). And **zero external reporters**, because the alpha is still not installable and the
  criterion that needs five of them is a tag push and an announcement, not a commit (`m0.1`, `reco-lnd`; `m0.2`,
  `reco-3gz`).
- **Twelve defects came out of the tool's own repository**, each filed as a bead the same day it was found, and
  each one found by a command in the runbook rather than by reading code: the export artefact refusing to
  certify itself on a multi-episode range (`reco-c2i`), a Python test function reported as a breaking public
  API removal (`reco-u1i`), a sign-off finding naming an episode id no verb can resolve (`reco-dvu`), a `why`
  explanation still citing closed tickets (`reco-j1o`), a config weight that configures nothing (`reco-gna`),
  thirteen ratchet violations that are a module and its own directory or a deliberately cyclic fixture corpus
  (`reco-wk4`), no `[layering]` rules declared to a tool whose boundaries check is therefore *blocked*
  (`reco-dg7`), the `[check] mode` decision nobody made (`reco-azy`), `debug_leftover` firing on `tools/*.py`
  whose job is printing (`reco-pfb`), a per-file diff cache with a 0.1 % hit rate (`reco-3sa`), and attribution
  calling 102 agent-written episodes human (`reco-0er`) — plus a contract suite whose recorded red list names two
  tests that are now green while three *other* tests in the same file are red (`reco-2vq`), found by re-running the
  list instead of trusting it. The record, with the commands and their output:
  `.plan/evidence/reco-rny/fathom-on-itself.md`; the decisions, with the alternatives they refused,
  are in `.plan/beta-loop-reco-rny.md`.
- **The release carries its own review.** `.github/workflows/self-review.yml` runs on `release: published`,
  indexes this repository with the release-profile binary, and attaches `fathom export`'s markdown, the
  doctor object and a `self-review.json` of the numbers to the release itself — with the same exit-code
  contract `dogfood.yml` states (`0` or `7`: a finding is a result, a crash is not). `check_ci_gate.py` grew a
  rule group for it, and now validates a `fathom` flag behind a path prefix (`./target/release/fathom check`)
  as eagerly as one on `PATH`.

### Five tickets that shipped without a changelog entry, and the audit that caught them (`e3.7`, `e5.6`, `e7.1`, `e7.8`, `s3`)

The new coverage audit read every commit between the `0.1.0-alpha.1` cut and here — 74 of them — and asked
whether this file accounts for each one. Five did not appear anywhere, by id or by prose. They are listed here
in the commit's own words rather than paraphrased, because the point of the entry is that the work happened and
was not announced, not that this ticket gets to describe somebody else's:

- **`e3.7` (`reco-p5u`)** — *The public API surface: what changed, which change drove it, and what a parser
  cannot know.* The API-surface detector, its breaking-change classes, and the honesty about the cases a
  tree-sitter query cannot decide.
- **`e7.1` (`reco-7f4`, `reco-02s`, `reco-ch4`, `reco-sb0`)** — *index, stats, doctor: make a messy
  repository's holes visible instead of fatal.* The hardening family: a clone with grafts, loose-object damage
  and unreadable blobs now says which hole it is looking through instead of printing a confident zero.
- **`e5.6` (`reco-2fv`)** — *a palette that is measured, and a frame that is the same frame with colour off.*
  Contrast and colour-vision simulation as floors the theme system has to clear, and `NO_COLOR` keeping every
  state's shape.
- **`e7.8` (`reco-41a`)** — *the threat model, read as a register — what a clone may try, and what answers it.*
  Its findings are described below under privacy and hardening, but never by id, which is how the audit found
  it: a document that exists in prose and not in the index of this file is invisible to anyone reading a release
  body for what changed.
- **`s3`** — *`structure.delta` now stores a chain of deltas, which is what §6.1 R2 always said it did.* A
  storage correction with a contract consequence, from a commit with no bead ref at all.

Two more commits get no note of their own, and the audit agrees that that is right rather than being fooled:
`rustfmt: the hunks the three fixes left unformatted` and *The lints the last four tickets left behind, so
clippy is green again* are housekeeping whose honest release note is nothing, and this paragraph is where they
are accounted for. `.plan/changelog-coverage.json` is therefore **empty**, deliberately: it is the place for an
omission a human has accepted with a reason and an owner, it exists for the day the audit is wrong, and `C07`
fails the next cut if anything in it becomes covered and is not deleted.

### Epic 6 closes: its three exit criteria are a runnable gate, and the run found a leak (`e6`, `reco-6t7`)

The epic's acceptance criterion is a sentence with three clauses — two working harness adapters, an MCP
session where the agent asks "what did I just change and is it risky", and a CI job that fails only on new
violations. Close reasons fade, so the clauses are now code: **`tools/epic6_acceptance.py`** runs all three
against the built binary and prints one verdict per clause, and `tests/test_epic6_contract.py` makes CI run
it (including a check that the gate can go red). Clause 1 measures four adapters and keeps the live/replay
distinction visible: git `post-commit` on real commits, `fathom-hook.py` over the recorded live capture, and
— under `--live-pi`, because they need the vendor — a real headless `pi` session through
`fathom-pi-hooks.ts` and through `fathom-envelope-driver.ts`. Clause 2 opens one stdio session against a
repository whose last change is deliberately risky and asserts the answer: `spine-1 49 = surfaces 36 +
author 8 + size 3 + breadth 2`, tier and factors, evidence ids, `grounding` saying the ranking is heuristic,
writes refused with `no_capability`, the credential in the commit subject redacted, no file-body field in
any response. Clause 3 runs the gate five times over one repository and reports `7, 0, 0, 7, 7`: a new layering
edge fails it, the **same range** passes once `fathom ratchet` baselines that edge with an owner and an
expiry, and a second, unbaselined edge fails it again while the baselined one stays counted and silent.

- **A file body was crossing the envelope adapter, and no test could see it.** The live run's canary check
  caught `structuredContent` — the key Claude Code puts a tool result's text under — copied verbatim into
  `.fathom/events.jsonl`, because `adapters/fathom-hook.py` digested a fixed list of key spellings and nobody
  had named that one. The recognizer now tokenises the key (`structuredContent`, `structured_content` and
  `Structured-Content` are one name) and calls it content when the **last token** is a content noun, and it
  digests a content key whatever its type, so `file_body`, `outputText` and a body inside a container are the
  same promise — while `context`, which merely ends in the letters `text`, keeps its evidence. The `created`
  flag that derives a `file_edit` still survives, asserted. Pinned by
  `test_a_body_under_a_vendor_spelling_of_content_is_still_digested`.
- **Two things the gate found and did not fix, written down instead** (`.plan/evidence/reco-6t7/epic6-exit-criteria.md` §4):
  `fathom ratchet`'s default "newest graph revision" is a coin flip when two commits share a second (the gate
  spaces its fixture commit dates for exactly that reason), and a baselined layering result can outlive its
  ledger row in an index that has since grown — both are candidate tickets, not epics' business.
- The record, with transcripts: `.plan/evidence/reco-6t7/epic6-exit-criteria.md` and its
  `epic6-exit-criteria.json`.

### Extensibility: the detector API, an out-of-tree example, and the event stream (`e6.7`, `reco-raw`)

A detector is a pure function over one episode, which is the only reason any of this can be safe: code this
repository has never seen can call it, and a fixture can check what it said. This is the ticket that makes
that a published surface instead of an internal shape.

- **`fathom_analyze::plugin` is the API**: a `Detector` trait (`meta` + `detect`), an `Input` that borrows a
  delta document and nothing else — no store, no `git`, no clock, no network, by shape rather than by policy
  — and a `Registry` that runs registered detectors under eight containment rules (R1–R8). An unknown
  finding-schema version, an undeclared finding type, an empty subject: each is dropped with its reason
  printed, never fatal to the run. A hostile plugin costs the reader its own findings and nothing else.
- **Plugin findings are namespaced and versioned**: `acme.untested_agent_logic`, never a bare id, so no plugin
  can take a built-in's name, and `schema_version` is in the payload rather than only in the schema's `$id`.
  `suppression_key` is inside the plugin's own namespace, so `detectors.suppress` mutes a plugin's family and
  no plugin reaches outside its own. Published as `schemas/plugin-finding.schema.json`, generated and blessed
  like every schema here.
- **`detectors.disable`, `--disable-detector`, `--detectors all,-id,none,+id`**: the switch for a *detector*,
  as distinct from `detectors.suppress`'s switch for a *finding*. A disabled built-in keeps its row in every
  report, spelled `disabled`, so "did it look?" stays answerable. Documented in
  [docs/concepts/extensibility.md](docs/concepts/extensibility.md), whose last section is the stability policy
  an external consumer can build against.
- **`fathom_core::events` is the bus; `fathom events` is the reader.** One versioned JSON object per line —
  `run.started`, `selection.resolved`, `analysis.*`, `detector.finished`/`detector.skipped`, `finding.raised`,
  `notice.raised`, `run.finished`/`run.failed`. A panicking subscriber is counted and delivery continues; a
  line from a newer build is refused by name and the rest of the stream still reads — published as
  `schemas/fathom-analysis-events.schema.json`, generated from the types that own it. Off by default, because
  the product writes no file a repository did not ask for; `[events] enabled = true` or `--events -` (stderr,
  so `--json`'s stdout stays parseable) turns it on, and a repository's config may not point the stream outside
  the repository.
- **`plugins/acme-detectors` is a second cargo workspace**, excluded from the root one, depending on
  `fathom-analyze` by path — and CI builds *and runs* it in its own `out-of-tree-plugin` job, which is what
  makes "usable from outside" a checked fact. Its two rules, `acme.untested_agent_logic` and
  `acme.unseen_reach`, are chosen because one needs a single delta field and the other speaks precisely when
  the data is missing.
- Contracts: `tests/test_extensibility_contract.py` (every declared event kind documented and every documented
  kind declared, the R-rules present in code, tests and docs, the new knobs in `e0.8`'s schema, the plugin
  excluded from the workspace and wired into CI), `crates/fathom-core/tests/events.rs`,
  `crates/fathom-analyze/tests/plugin_api.rs`, `crates/fathom-cli/tests/extensibility_verb.rs`.

### The demo repository: five seeded pathologies, a scripted tour, a recorded five minutes (`e7.6`, `reco-3qr`)

A clone plus one command has to reproduce every documented finding, and the only version of that sentence
that survives contact is one where the document is generated from what the binary says and re-run on every
commit. The tour already existed as a script (`reco-by2`); this is the demo as a *product surface* — the
repository, the answer key, the recording, and the version tag that ties them to a release.

- **`publish/demo-repo/` grows exhibits.** The published clone is now 27 commits: the 21 the binary unpacks
  (unchanged, byte for byte — `publish_demo_repo.py check` counts the commits above the pack's head to
  prove it), five that seed what the miniature has no room for, and the tour/gate commit. The five are four
  agent episodes on one function that put back what the second one deleted, and one commit that is
  whitespace and nothing else — `git diff -w` against its parent prints nothing, which is what the
  acceptance run checks instead of trusting the word "reformat".
- **`tools/demo_exhibits.py` is the ticket's sentence as data**: five rows, each with the commits that seed
  it, the command that shows it, the strings that must (or must not) come back, and — for a gap — the owner
  and the remedy. It writes `publish/demo-repo/PATHOLOGIES.md` (the answer key, which `DEMO.md`
  deliberately is not) and the table in `docs/demo/five-minute.md`.
- **`tools/demo_acceptance.py` is criterion 1 as a program**: it `git clone`s the assembled repository,
  indexes it, runs the one command the tour hands a newcomer, and fails if a documented finding is missing
  **or if a documented gap has quietly closed** — three of the five are gaps today (the cycle, the churn
  loop, the reformat), each named as such with the ticket that would close it, because a demo that printed a
  cycle finding the product cannot produce would be printing a lie.
- **`tools/record_demo.py` records the tour** — every step run on a real pty, the terminal's own bytes
  written as asciicast v2 with the times they took — and `docs/demo/five-minute.cast` is the film: 37 s
  against the 300 s budget, captions instead of narration because it has to be watchable with the sound
  off, and `check` refusing a cast that runs long, grows a sound track, predates the demo's current tag, or
  has lost an exhibit from the frame. `gif` renders through `agg` or `asciinema`+`gifski` when one is
  installed and prints the command when neither is.
- **Versioned on the minor line.** The assembly tags the demo `demo-v<major>.<minor>`, derived from
  `[workspace.package] version`, and the cast's header carries the tag it was recorded at; a minor release
  that skipped the re-record now fails CI instead of shipping a stale film. The step is in the release
  runbook (§3, step 6).

### The CI gate as something outsiders install: one installer, one reusable workflow, a scripted five-minute demo (`reco-by2`, `m2`)

The 1.0 criterion is not "we can run `fathom check` on our own history" — `ci.yml` has proved that every push
since `e6.6` (`reco-ngk`) — it is that a repository which is not ours can put the gate in front of a merge and
get an answer a reviewer can act on. That turned out to be mostly a *packaging* problem wearing a CI hat, and
the first bug was in a document: `docs/recipes/gate-a-pipeline.md` has told readers since `e6.6` to run
`./ci/install-fathom.sh`, and no such file had ever existed. It exists now, and so does the gate that keeps
documents from pointing at programs again.

- **`ci/install-fathom.sh` — the one installer, and it refuses to install a liar.** POSIX `sh`, no
  dependencies, three paths in order: a binary already on `PATH`, the release installer at the version you
  name, then a source build of the headless CI feature set (`sqlite-bundled,langs-all,deps-all`). Then it
  **refuses** whatever it got if that binary cannot open a store — because a store-less build installs
  happily, prints a version, and reports four of the six checks as `blocked`, which is a green job that looked
  at nothing. `--check PATH` answers that question about any binary without installing it; `--mode`, `--src`,
  `--prefix`, `--offline` and `--allow-degraded` are the knobs, all in `--help`.
- **`.github/workflows/fathom-check.yml` — the gate as a reusable workflow.** `workflow_call`, so the
  adoption is five lines: deep clone (four of the six checks compare two trees, and a clone without the base
  makes them report `blocked`), install through the installer above, `fathom index`, `fathom check`, then
  SARIF and a job summary under `if: always()` because the red run is the one someone needs to read. Its
  verdict step keeps the three exit codes apart — `0` clean, `7` a human has work, anything else is the gate
  breaking — and its inputs are the knobs, each declared, `fail-on` defaulting to empty so the repository's
  own `[check]` policy decides.
- **The headless build compiles again, which is what makes the installer's source fallback true.**
  `cargo build -p fathom-cli --no-default-features --features sqlite-bundled,langs-all,deps-all` and
  `--features minimal` both built a binary for the first time in some while: the TUI, the MCP server and the
  store-backed verbs are now gated on the features that provide them instead of failing to link without them.
  A CI feature set that does not compile is a promise the installer cannot keep.
- **`.github/workflows/dogfood.yml` — the gate over repositories this project does not own.** The three
  Guardrail 3 clones at their pins, unshallowed first (a pinned `window` is a calibration slice, not the state
  of a reviewer's clone), each with `--require dependencies --require tests --require sign-off`, a per-repository
  time ceiling, and the reports uploaded. The local run of those steps is in
  `.plan/evidence/reco-by2/dogfood.md`: 12 367-commit `cli`, 5 562-commit `flask`, 17 683-commit `ruff`, all
  three answering with findings a reviewer could act on, and with the checks that legitimately had nothing to
  judge reporting `blocked` and saying why.
- **`tools/check_ci_gate.py` — the CI story checked as one object.** It runs the installer against a
  fabricated store-less binary and fails if the installer *accepts* it; it requires that the reusable workflow
  declares every input it reads and invokes only verbs and flags the built binary has (`fathom check --full` is
  plausible, unread, and a job that dies three minutes into a stranger's pipeline); it requires the dogfood and
  demo jobs to reference files that exist; and it requires that every `ci/…sh` and `tools/…py` a documentation
  page tells the reader to run is a file — 201 references, which is the rule that would have caught the recipe.
  `self-test` proves it can fail, and one of its fixtures is a job whose `mode: warn` and `fail-on: warning`
  contradict each other.
- **`tools/five_minute_demo.py` — the five-minute criterion as a run, not an adjective.** Eight steps over
  the miniature that ships inside the binary, each asserting its own exit code and its own output — two of them
  `7`, on purpose, because a demo whose gate never turns red demonstrates a dashboard — inside a budget of
  `BUDGET_S = 300` measured from an empty temp directory. `doc` writes the measured table into
  `docs/demo/five-minute.md`; `check` re-runs it and fails if the page drifted (timings with slack, claims byte
  for byte); `self-test` proves the checker can fail.
- **`publish/demo-repo/` plus `tools/publish_demo_repo.py` — the demo repository, assembled rather than
  stored.** The miniature's 21 commits, then the tour (`DEMO.md`), the gate's policy (`fathom.toml`,
  `mode = "warn"`), and a CI job that calls the reusable workflow exactly as a third party's would — 22 commits,
  identical history to the pack the binary unpacks because it is generated from it. Two fixes worth naming:
  `.gitignore` is rewritten to keep `.fathom/reviews/` committable (ADR-004 §12: the store is disposable, the
  ledger is not), and the index is rebuilt from `HEAD`, because a clone with no index reports every tracked
  file as a staged deletion and that is not a first impression.
- **One documentation trap closed, and the checker that finds that class.** `--fail-on` *promotes*
  `mode = "warn"` to enforce for the run — naming a threshold is treated as meaning it — and the CLI has been
  saying so in its notices while the documentation said the opposite (`docs/concepts/ci-gate.md`, Policy).
  The recipe no longer suggests `--features minimal`, which has no store. And `tools/check_docs.py` now resolves
  link **definitions** as well as inline links, which is how it found that `docs/concepts/ci-gate.md`'s
  `[ADR-002]` had pointed at `adr/0002-rust-analyzer.md` — a file that has never existed in this repository —
  since the page was written.

### Cross-platform packaging: an artefact set a stranger can install, and the secrets that are not wired (`reco-3q4`)

`e0.7` built the release pipeline and `m0` cut the alpha out of it; both left the word *packaging* to this
ticket. The acceptance criteria were four — installers for macOS, Linux and Windows, Linux system packages,
completions and a man page generated from the CLI definitions so they cannot drift, and contributor docs
that say which release steps need a maintainer-only secret — and three of them are a mechanism, which means
they can be built and gated here rather than promised. The fourth is a table, and it is in the changelog
before it is in a wiki.

- **Six targets, four installers, one binary.** `dist-workspace.toml` now builds `x86_64`/`aarch64` Linux
  (GNU) plus `x86_64-unknown-linux-musl` as the **static** build, both Apple triples, and `x86_64-pc-windows-msvc`,
  and produces a shell script, a PowerShell script, an **msi** and a **Homebrew formula**
  (`formula = "fathom"`, so a person types the binary's name, not the crate's). arm64 Linux builds natively
  on `ubuntu-24.04-arm`, declared in `dist.github-custom-runners` so `dist` does not reach for a
  cross-compiler it does not need; `aarch64-pc-windows-msvc` stays out, because an arm64 Windows runner does
  not exist and this document would rather not make a claim whose only evidence is a green tick.
- **`.deb` and `.rpm`, repacked from the release rather than recompiled by it.** `cargo-dist` has no system-package
  target, so `[[dist.extra-artifacts]]` runs `packaging/linux-packages.sh` in the global phase — after every
  per-target archive has been downloaded back into `target/distrib/` — and `tools/package_release.py` relocates
  the archive's own bytes into each format's FHS layout (`/usr/bin`, `/usr/share/{bash-completion,zsh,fish}`,
  gzipped `/usr/share/man/man1`, `/usr/share/doc/fathom` plus a Debian `copyright`). The `.deb` is written
  here with zero timestamps and re-read by this tool and by `dpkg-deb`; the `.rpm` is written by `rpmbuild`,
  which the script installs because the runner image lacks it. `--self-test` asserts nine claims about the
  output, including the two that are invisible until a user hits them: a control member whose declared size
  is wrong unpacks happily and fails at configure time, and a file name that disagrees with the binary's
  `--version` is a package `dpkg -i` mislabels. `0.1.0-alpha.1` is `0.1.0~alpha.1` in a file name, because
  `-` is not legal in an upstream version and `~` is how both package managers spell "sorts before".
- **`fathom completions`: the CLI prints its own grammar, so nothing can drift.** `bash`, `zsh` and `fish`
  scripts via `clap_complete`, and one roff page per verb and subverb via `clap_mangen`
  (`fathom-review-sign.1`, each page naming the version it was cut from). `tools/gen_shell_artifacts.py
  write` commits them under `packaging/`, `dist`'s `include` puts them in every archive, `package_release.py`
  relocates them into the packages, and `check` regenerates and diffs in CI — the same argument `gen_docs.py`
  makes about `--help`, one directory over. The verb is in `docs/reference/cli.md` like every other verb,
  because a shipped verb with no page is an undocumented feature.
- **The installer's promise is rehearsed, with negative controls.** `tools/rehearse_installer.py` mirrors
  `target/distrib/` over `http://127.0.0.1`, runs the README's one-liner against it with only the host
  changed (`FATHOM_CLI_DOWNLOAD_URL`, `FATHOM_CLI_INSTALL_DIR`, `FATHOM_CLI_NO_MODIFY_PATH`), and insists the
  right archive was fetched, its digest matches, and the installed binary reports the version the manifest
  carries. `--self-test` then corrupts the mirror four ways — bad digest, missing artefact, an installer that
  fetches something else, a binary from another release — and requires all four to be caught, because a
  rehearsal that cannot fail is theatre.
- **Five more rules on the release gate: `R14`–`R17`.** The installer set and the two extra targets present;
  the msi still buildable, which means its `upgrade-guid` survived (regenerating that after a published msi
  splits the product in two on the user's machine); the system packages still named for the version being
  released, with their builder a file in the tree and their output under `target/`; the completions and man
  pages present, non-empty and actually `include`d; and the secrets table still rendered with its three
  signing names in it. 22 mutation cases now, all detected, `tools/check_release.py --self-test`.
- **§8 of the release document is the answer to "which steps need a maintainer-only secret"** — generated
  from a table in the checker rather than prose, with a *what a fork sees* column: nothing in the build needs
  a credential, so every artefact here builds unsigned from a fork's own releases page; `HOMEBREW_TAP_TOKEN`
  is why `publish-jobs = ["homebrew"]` is a commented line instead of a failing job; and macOS codesigning
  and notarisation are **not** a `cargo-dist` setting in 0.33 (they run between the build and the archiving,
  then `staple`), so they need a wrapper that cannot be tested without the certificate — which is why it is
  documented rather than half-wired.
- **What this does not claim.** That the msi, the formula or the `.rpm` are *right* rather than built (only
  WiX, Homebrew and `rpm -qp` can say, and CI runs the last); that anything is signed; that `brew install`
  works from a tap that does not exist yet; and that the artefact another OS receives works — that stays the
  runbook's step 5, on three machines, with a ledger row each.

### Epic 2 closes: the episode says what it now depends on, and the fold is on the record (`reco-tx0`)

Epic 2's exit criterion is one sentence — *every episode answers where the change landed and what it now
depends on, with every number traceable to its evidence, and noise is suppressed by default and never
silently hidden* — and two of its clauses were still untrue of the product when this started. The twelfth
child, `reco-tt4`, had built the document that would carry the answer and left the dependency arm of its own
`Supplied` enum unwired, on the grounds that the shape must not change when the analysis arrives. It arrived.

- **`fathom analyze`, `fathom narrative`, `fathom export` and the MCP's episode-delta tool open the
  repository and read it.** `Supplied::for_episode_with_repo` takes the store, the policy and the open
  repository, resolves the episode's first member's parent and its last member — a multi-commit episode
  compares the tree from before the episode, not its first commit — and hands that pair to
  `deps::dependency_episode_repository`, which has done this correctly since `reco-kvs`. Four surfaces, one
  call, so the dependency answer cannot be a different number in the card than in the artefact. A read with
  no object database open still says `unavailable`, and the `why` names the command that fixes it.
- **An empty answer is a measurement, not a gap.** An episode that touched no manifest gets a `measured`
  section whose graph figure says *the graph was not read — no manifest or lockfile changed*, because
  printing `0 → 0 packages` would be a number about a thing that was never looked at. The distinction is the
  one ADR-001 §7.7 is made of: *nothing changed* and *nothing to change* are both answers; *not looked* is a
  third, and the object database being shut is a fourth.
- **A dependency figure's evidence is a command, and the command is real.** Every number in the section —
  direct movement, graph size, sources, pattern — carries
  `fathom check --only dependencies --base A --head B`, at the same pair the episode was measured across.
  That holds in a build compiled without `deps-all` too, where the figures are absent rather than zero and
  the section reverts to its `Unavailable` with the same command in the remedy.
- **The noise fold is now on the record, not only on the pane.** The `what changed` section carries a
  **noise folded** figure: files and churn excluded, broken out by class (`lockfile 1f · 0 lines`,
  `generated 1f · 0 lines`). Before this, a reviewer reading "2 files, 44 lines" in a repository where a
  generated client had been rewritten had the discounted number and nothing else — the discount was true,
  and it was not visible, which is the "silently hidden" half of the criterion. A discount of zero is
  reported too: *nothing was discounted* is the answer to a question the reviewer is entitled to ask.
- **`fathom export` could not resolve its own scope, so the artefact contradicted itself.** An
  `--episodes ep-…` artefact's range-level *Dependency delta* section said *not computed for this range*
  two lines after the per-episode section had measured it: the range resolver re-parsed the raw handle as a
  revspec, and no revspec grammar reads an episode id. It reads the scope the store already resolved.
- **One blessed digest was stale, and said so.** `risk_supplied`'s card digest in
  `tests/test_summary_card_contract.py` was still the one from before `reco-6co` retuned `risk.tau` from 70
  to 40; the threshold is printed twice on that card, nothing else in any of the sixteen fixtures moved, and
  the re-bless carries the reason in the file so the next reader does not have to rediscover it.
- **What is still not wired, and where that is tracked.** `fathom episodes`' roll-up refuses the dependency
  question on purpose — it would re-open one object pair per row to print one line each, against ADR-004
  §11's budget for a path that lists hundreds of episodes — and `fathom compare` stays structural because its
  two sides are refs with no episode to attribute a package to. `blast` remains the epic's one honest `?`,
  owned by `e3.2`. Evidence: `crates/fathom-cli/tests/analyze_deps.rs` (a manifest-moving episode measures,
  a source-only episode says the graph was not read, *every figure of every section of a real document
  carries evidence and the document verifies itself*, and the named command runs), `delta_schema.rs` (a
  measured block's evidence is what the payload carries) and the re-blessed narrative golden, which now
  cites the fold.

### `reconcile` — spike `s4`: when is a pairing a fact, and what does a wrong one cost? (`reco-gj6`, folded by `reco-tx0.1`)

`s4` measured rename and copy detection across **16 authored git repositories** — 29 review windows, 60
hand-authored true pairings (5 past a stated evidence ceiling), 5 named decoys, 23 detector configurations
graded at every bar from 0.10 to 0.90, 51 gates — and `e2.4` (`reco-a0v`) shipped on those numbers. What this
entry describes is a change to the **plan's own text**, which was still carrying a pre-measurement guess:
`e2.4`'s acceptance criteria said "at least 90 percent pair precision and recall", and `e0.12` had left a
ticket — `e2.12` (`reco-tx0.1`) — to replace that bar with the measurement. That ticket has now run:
`reco-a0v`'s description and criteria quote the numbers (its original four criteria restated, so the shipped
gates `A1`–`A4` still map one for one, plus three the measurement forced), its estimate went
**360 → 700 minutes**, and the record is §5.1 of
[.plan/reconciliation-e0.12.md](.plan/reconciliation-e0.12.md).

* **The 90 % bar is deleted rather than tuned, because the point it names does not exist.** The corpus's
  scaffolded false pair scores **R064** in git at its shipped `-M50` and **J≈0.49** in a shingle/minhash
  matcher, *above* the window's true moved-and-rewritten pair at **R015 / J≈0.06** (G28, G29, G47): git, the
  matcher and path similarity all rank the lie first, so any bar low enough to catch a real rewrite is high
  enough to ship the lie.
* **The fact tier is git's own rewriter at ≥ 0.75** — not git's 50 % — with a precision floor of 0.95 and
  zero tolerance for asserting a pairing the truth names as not-a-move. Measured: 34 emitted, precision
  **1.00**, recall **0.618** of 55 claimable, **0.0** wrong pairings per 100 windows reviewed (shipped
  engine: 40 emitted, 0 untrue, recall 0.6452 of 62 claimable). Refusing the cheaper lie-tolerant point
  costs **+17 missed pairs** — the same seventeen the guess tier then carries, labelled — and that price is
  now in the ticket instead of being discovered in a review.
* **The guess tier is labelled and priced, not thresholded**: the widest band worth showing is **+17 true
  pairings for +1 named lie** (precision 0.962, recall 0.927), and confidence is a tier label plus the
  evidence behind it rather than a percentage — the shipped corpus's calibration error is **0.2039 over 61
  pairs**, which is why a printed number is evidence and not a probability.
* **Recall is reported per representation class**, because split (9 pairs), fold (3) and copy (2) are found
  **zero** times at the fact bar by every method that emits pairs: rename 48/48, split 9/9, fold 3/3, copy
  0/2 window-local, and 0 pairs emitted under the wrong shape anywhere. A copy whose source survives is
  declared out of scope for a window-local detector — the whole-tree mode that could find it is O(repo) and
  buys a named decoy with it — and silence stays a designed third outcome: 5 pairs past the ceiling reported
  as a ceiling, 9 still windows emitting nothing.
* **Two prohibitions became acceptance criteria**: path similarity is never part of a pairing score (a 0.15
  name weight takes wrong pairings from 1 to 36 at the same bar), and no content score the engine prints is
  an approximation — the minhash sketch this ticket originally assumed is a candidate generator and is
  refused as a decider (p95 error 0.063; 29 of 545 pairs flip across a 0.3 bar; LSH 32×4 loses every split
  pair).
* **Two ADR assumptions were retired where they were written.** ADR-001 §7 principle 7 ("confidence is part
  of the answer") now says the *label* carries that duty and a score cannot, and ADR-006 §5.4's
  `renames with confidence` row is a tier plus its evidence — the bare `conf 0.92` in that view's mock is
  the form `e2.4`'s card auditor refuses. `docs/concepts/rename-detection.md` now records its own
  recommendation list as discharged, in the generator that renders it.

### `reconcile` — spike `s6`: does a human's decision survive a rewrite, and what would a wrong relink cost? (`reco-f5a`)

ADR-003 §6 ships the relink pass — when a rewrite moves an episode's id, the verdict on the dead id is
copied onto the live one — and §13.1 deferred a fuzzy fourth tier with a written condition: *revisit when
the false-relink and missed-relink rates can be reported against annotated ground truth*. `s2` measured
where turns **start**, which is a different quantity, so the condition went unmet until now. This is the
measurement and the answer, which is **no**. Corpus
`crates/fathom-fixtures/corpus/relinks/` — **21 rewrite families, 1562 rewrite events**, each a declared
operation laid on one of `s2`'s annotated histories, with the truth written as **carried pairs** (this
dead episode's decision belonged on that live one) plus the annotator's claim about which tier can honestly
catch it; harness `crates/fathom-analyze/examples/s6_spike.rs`, report
[benchmarks/reports/s6-relink-accuracy.json](benchmarks/reports/s6-relink-accuracy.json), 25 claims as data
in `crates/fathom-analyze/spike/s6/thresholds.json`, and — because a relink measured with a copy of the pass
is a rumour — the spike runs the shipped `fathom_store` relink pass over every event and fails if the two
disagree link for link. See [episode relink](docs/concepts/episode-relink.md) and ADR-003 §19.

* **The shipped ladder loses 445 of 1232 annotated carries (36 %) and only 19 of them are its own fault.**
  393 are annotated `catch: none` — no shipped digest survives the rewrite — and 33 are TB10 twin churn.
  §6.3 named one such case and put no number on it; the number is **32 % of everything owed**, and §6.3 is
  missing a case: `author` is an input to all three digests, so `--amend --reset-author` is invisible to the
  whole ladder (196 carries). Tier 3 (`member-set`) buys **1** carry across the corpus: §6.2 presents three
  tiers and two do the work. Tier 2 is the leaky rung, not tier 1: **8.4 %** of its rows are false relinks
  against tier 1's 2.0 %.
* **A relink's false rate is a property of the review queue, not only of the ladder.** Every rate is
  reported on two boards: `signed-off` (every episode dispositioned) and `sparse` (every third), because RL0
  forbids landing on an episode that already has a verdict — so a false relink can only land on an episode
  nobody has read yet. Same pass, same corpus: **5.2 % false on one board, 41 % on the other**, and 123 of
  the sparse board's 168 false relinks are one dead episode's verdict landing on more than one live one,
  which is RL4 working as written. `dropped-turn`, where the annotation says the verdict goes nowhere,
  writes 0 rows on a full queue and 19 on a sparse one.
* **The fuzzy tier was priced and does not clear the bar it was set.** A subject-similarity rung run last
  (RL1) buys back **200 of §6.3's floor carries for 4 false relinks (50:1) on a full queue** and **57 for 70
  (0.8:1) on a queue that looks like a queue**, against §6.6's price of better than 3 carries per false
  relink; add the only discipline that stops it spraying one verdict across every similar unread episode —
  unambiguous in both directions — and the price becomes acceptable and the delivery falls from 200 carries
  to 53, because a reword-and-moved episode is never locally unambiguous. The rungs that reach
  `reset-author` are the ones that ignore the author, which is §6.1's identity field. So §13.1 is closed
  **NO-GO**, and §6.3's floor gets sent to the answer the corpus itself demonstrates: `method='manual'`,
  `e1.10`'s `fathom episode link` verb, where a reviewer names the target in one keystroke and never
  mis-names it.
* **Two rule changes fell out that nobody asked for.** RL4 is a multiplier and is to be tightened — at most
  one landing per donor unless a stronger tier justifies another, with the sparse board's 41 % as the number
  to beat — and relink rates are published on two boards from now on, because a single-number relink rate is
  unreadable without knowing how full the queue was.
* Asserted by `tests/test_relink_spike_contract.py` (17, including the go/no-go re-derived from the
  report's counts rather than quoted from the gate table), corpus validated by
  `tools/author_relink_corpus.py --validate-only`, tables generated by `tools/gen_relink_tables.py`, CI has a
  `--check` step.


### The CI gate: one command that answers "may this merge?" (`reco-ngk`)

ADR-002 §7.3 asked for a gate that fails only on new violations, never on a clean checkout where an analysis
failed to run, and explains itself in one sentence. **`fathom check`** is that command: a `base..head` range
across six checks — `layering`, `cycles`, `api`, `dependencies`, `tests`, `sign-off` — at severities the
repository sets, ratcheted so existing debt is not news, and printed four ways. The gate has no opinion of its
own about what is debt: five of its six checks ask an engine that already has one, which is what keeps the
answer consistent with what `fathom impact`, `fathom cycles` and `fathom explain` say about the same change
(ADR-002 §5.3). See [the CI gate](docs/concepts/ci-gate.md).

* **The decision is a pure function of a value**, so it is testable without a repository. `Gate::build(policy,
  range, runs)` in `fathom_analyze::check` computes the verdict, the exit code, the one sentence and the whole
  JSON document from what the checks gathered; the verb gathers, the decision layer decides. `--explain` prints
  the same policy from the same code, so the explanation cannot drift from the enforcement — the failure this
  split exists to prevent.
* **Nothing that examined nothing passes.** A check that cannot judge is `blocked` with a reason and a remedy,
  never `ran` with an empty list; a blocked *required* check fails; a run where no check examined anything fails
  however the findings fell. Three separate refusals, each tested, because "no findings" and "I could not look"
  are otherwise one exit code apart and indistinguishable in a log full of green.
* **`check.mode` defaults to `warn`, and `--fail-on` escalates without rewriting the policy.** A gate that blocks
  a repository the day it is installed is a gate that gets deleted the day it blocks a release. `mode = "off"`
  is not overridden by any flag: a switch that silently re-enables a deliberate kill switch is how the kill
  switch stops being one.
* **An unknown policy key is fatal.** `[check] fail_onn = "error"` exits 5 and names the key and the legal ones,
  rather than falling back to a default nobody chose. Same for a `check.version` newer than the binary, a
  malformed `--range`, and an `ignore` family that names no check.
* **Sign-off reads the ledger, not the store's mirror.** The cached review state can lag a ledger edited
  anywhere, so a fresh accepted entry on disk counts and a *stale* sign-off — one whose file has since moved —
  does not, which is the whole reason the ledger records line anchors.
* **SARIF that uploads.** Every `ruleId` is declared, `ruleIndex` resolves to it, rule IDs name the *rule* and
  not the violation instance, fingerprints come from the evidence key rather than the line number, and a
  location whose file is gone at the scanned revision is dropped rather than uploaded broken.
  `tools/check_sarif.py` re-checks all of it and carries a `--self-test` that proves it fails.
* **The gate is now in its own CI** (`check-gate`): a shallow clone, timed against the 60 s budget, its SARIF
  uploaded, its job summary written, and its exit code asserted to be 0 or 7 — because a gate whose own runs
  go unwatched rots into a warning nobody reads.

One bug fell out of running the gate over this repository: `api::textscan` masked bytes and "repaired" the
result with `String::from_utf8_lossy`, which re-encodes one broken byte as three and silently shifts every
offset after it. `line_start` then sliced `raw` at a non-char boundary and panicked on a file containing `→`
inside a comment. The mask is now realigned one byte per input byte, `line_start`/`line_end` snap to a boundary
regardless, and the regression test reproduces the exact panic when the fix is reverted.

Two limits stated rather than hidden: `fathom index` writes no architecture graph snapshots yet, so `layering`
and `cycles` report `blocked` on an index-only repository — the remedy now says so instead of sending the
reader to a command that cannot fix it — and `tests` judges file names, not behaviour, which is why its volume
is `warning`.

### The ratchet: the committed allowance ledger, and the debt whose grace is running out (`reco-5ht`)

`e3.5` and `e3.6` could already tell a baselined violation from a new one, but nothing could *write* the
baseline: `.fathom/ratchet.toml` was a file you hand-edited and the tool only read. **`fathom ratchet`** is
the write path, and the whole design of it is one refusal — the ledger may tighten by itself and must never
widen by itself.

* **One pure plan, four inputs.** `fathom_analyze::ratchet::Plan::build` is a pure function of the committed
  file, the violations at one graph revision, which engines actually judged that revision, and the clock; it
  returns exactly what the file should become, and the verb is allowed no other opinion. A violation that
  has gone away has its row deleted by the next bare run — that is the ratchet, and it needs no flag. A
  violation that has *appeared* is left uncovered: a bare run never writes news into the file, because a
  baseline that quietly absorbed this month's mess is a baseline that has stopped meaning anything.
* **Widening is an act, with things attached to it.** `--update-baseline` requires a non-empty `--reason` and
  an owner, and `Grant::new` refuses an empty reason in the type before the flag parser refuses it;
  `--reason` without `--update-baseline` is a usage error too, in both directions, so neither flag can be
  typed as a slip of the other. Every granted row carries `owner`, `reason`, `expires`, `granted` and
  `granted_by`, the last two filled in automatically from the clock and the git identity — a hand-edited row
  missing its grant date is filled in rather than trusted. `--key` narrows a grant to what you name, and a key
  that matches nothing is *refused* with an explanation rather than writing nothing quietly.
* **An expiry is the point of the row, so an expiring allowance is chased down before it lapses.** `ratchet`
  prints every live row inside the warning window with its days left, an expired allowance whose violation
  is still there is **debt back due** — said with its date, nothing deleted, uncovered again — and re-granting
  replaces the lapsed row instead of stacking a second one under it. Expiry is day-grain like `cycles::debt`,
  so "expired on the 31st" does not depend on the hour the index ran.
* **Three surfaces, one ledger.** The verb (`fathom ratchet`, `--json` as `fathom-ratchet/1`, schema
  `schemas/ratchet.schema.json`), the architecture map (`fathom_tui::graph::hand::Debt`, filled by
  `fathom-cli` from `ratchet::Ledger` — a widget grows no ratchet of its own, per ADR-002 §5.2), and the
  review artefact (`debts`, in all three renderings, with rows already lapsed by the range's newest commit
  raised as checklist items). The map's strip carries the ledger at the *front*, because a strip is clipped at
  the end and a finding nobody sees is not a finding.
* **The reason is in the review memory as well as the file.** Granting appends a `decision` note to
  `.fathom/reviews/notes.md` carrying the same sentence and the rows it wrote, anchored to the ledger path —
  so the ledger says what is forgiven and the memory says who said so and against what, which is the half a
  committed TOML file cannot hold (`e6.5`).
* **Two refusals, both load-bearing.** No index or no graph revision exits 12 (`INDEX_DEGRADED`): with no
  graph there is no violation list to read the ledger against, and "nothing uncovered" would be reporting a
  fact that was never checked. And a kind that was not judged is never tightened — switch `[layering]` off for
  an afternoon and its rows are still there when it comes back, because what changed was the question.
  `ratchet.mode` decides whether uncovered violations fail a run (`deny` → exit 7), and `off` means off: no
  alarm, no failure, no surprise.


### Hotspots: which module attention pays off in, and what looks dead (`reco-wky`)

`e3.2` answers *what does this depend on*, `e3.3` answers *what did this turn move*, and neither answers
the question the reviewer has: *if I send an agent at one module this week, which one pays?* **Hotspots**
(`crates/fathom-analyze/src/hotspots/`) ranks that, out of six metrics a person can name — fan-in, fan-out,
instability, size, complexity (from `e3.1`'s spans, and still labelled a proxy) and churn velocity over a
configurable window — plus the three dead-code sweeps: symbols added inside the window that nobody
references, code behind a flag this build does not resolve as on, and duplicated blocks that grew in the
same episode.

* **Explainable is arithmetic, not an adjective.** Each metric goes through one saturating curve
  (`raw / (raw + knee)`, knee published as `hotspots.knee.<metric>`), the score is a weighted sum in integer
  hundredths divided by the weight sum over the metrics *actually measured*, and the printed parts add to
  the printed total. Every row prints its denominator, its weight origins and a reason string in the raw
  unit (`14 module(s) depend on it`), so a score of 57 is checkable against the file it came from. It is
  the same arithmetic `e4.4`'s risk score uses, because two scores that both claim to be explainable may
  not be explainable in two ways.
* **`—` is not `0`.** A module with no structural record has no size and no complexity: the metric becomes
  *unmeasured*, its weight moves into the denominator and it prints a sentence. A cold index prints
  `churn — · no churn window: this revision has no earlier revision to be compared against` rather than a
  zero that would rank an unread generated file as tiny and stable. Wildcard dependencies and synthetic
  scopes are caveated as lower bounds instead of being counted as complete caller lists, and an empty graph
  ranks nothing and says why.
* **Shape calls, because a score does not tell you what to do.** `god` (fan-in, size, complexity and churn
  all over the bar), `hub`, `sink`, `giant` (big and complex, nobody depends on it, quiet) and the classic
  churn × complexity quadrant, so `hot_complex` reads as "a refactor pays off here" and `cold_complex`
  reads as inventory.
* **Dead-code candidates carry their doubt.** Each row states its age against the bar (`unreferenced for 4
  episode(s), past the `dead_code_after` bar of 3`) and its verdict — `firm`, `uncertain` when a named
  mechanism could explain it away, `unverified` when the window is simply shorter than the bar — and every
  report prints the permanent list of what no static read can see: reflection, derives and `serde`, dynamic
  dispatch, callers in another repository, tests and benches. Nothing in it is a delete instruction.
* **Ten knobs, all refused loudly when wrong.** `[hotspots]` in `fathom.toml` — `window`, `min_history`,
  `top`, `dead_code_after`, `duplicate_block_lines`, `god_bar`, `flags`, `ignore`, and the two open maps
  `weights.<metric>` / `knee.<metric>`. A weight that is not a number, a weight name that is not one of the
  six, a knee on `instability` (a ratio has no curve to bend) and a window below `min_history` are all
  notices naming the value that was used instead. `hotspots.ignore` exclusions are counted and named, so a
  candidate list says how much of the repository it declined to read.
* **The lane reads no store, no filesystem, no clock** (ADR-002 §4.5): the seam is
  `hotspots::facts_of(graph, naming, files, churn)`, so `crates/fathom-analyze/tests/hotspots.rs` seeds a
  god module and asserts the entire ranking — and all three dead-code sweeps — in a build with no SQLite
  and no grammar linked (31 cases, passing `--no-default-features`). Written up in
  [docs/concepts/hotspots.md](docs/concepts/hotspots.md).

### The architecture map, with what this turn did to the shape of the system drawn on it (`reco-w3w`)

`fathom` could already answer "what does this depend on?" (`e3.2`) and "what did this turn change?"
(`e3.3`), and neither question is the one a reviewer asks at the keyboard. The one they ask is *did this
turn make the shape of the system worse?* — which is a picture, an overlay on that picture, and a citation
under each arrow. `g` is that view (ADR-006 §5.5): the module graph aggregated to whatever the canvas can
hold, the episode's architecture delta drawn as marks on the same graph, and a file-and-line citation for
whichever arrow the cursor is on.

* **Degrade by aggregation, never by clipping.** A 3 000-module graph is not drawn as 1 000 modules and a
  shrug: the canvas folds whole modules into a counted cell, prints `nodes 214 of 1 902 shown · aggregated
  at L2`, and the arithmetic (drawn + folded = handed) is asserted in
  `crates/fathom-tui/tests/graph_map.rs`. Aggregation is a knob (`[`/`]`, and auto by default so the same
  hand is legible at 40 columns and at 200), the hop radius is a knob (`<`/`>`), and `!` narrows the map to
  what the episode touched plus its blast radius.
* **Three layouts, and heat with a denominator.** Layered (longest-path depth, cycles condensed into one
  rung when `graph-layout` is compiled in), circular, and a squarified treemap of the module tree — plus
  the aggregated node table every build has, which names the feature that would buy the rest. Heat bands
  carry their basis and their window (`heat churn over the last 30 revisions`), and a series that was never
  measured prints its reason instead of a cold-looking map. `H` cycles the basis; the complexity series says
  `not measured at startup` rather than `0`.
* **The citation is the criterion, and an honest one.** An arrow prints `evidence crates/cli/src/main.rs:7`;
  where the structural record names several statements that could be the dependency it prints the count and
  refuses to guess (`3 lines could be this dependency`), and where it names none it cites the file and says
  the line is not recorded. Citations are a provider asked for one arrow at a time, because naming the line
  behind 3 000 arrows at startup is 3 000 blob reads to print one.
* **The shell still opens nothing** (ADR-002 §5.2, lint `E01`). `fathom-cli` materialises the graph lane,
  reads the episode's `delta.architecture` row, the churn window and the layer tags once, and hands the
  result over as one value (`fathom_tui::graph::Hand`) through `Options::graph`; `g` draws what it was
  given. A repository with no graph lane keeps the published placeholder, and a key pressed on it says what
  would give it data. Removed nodes come back as ghost cells, so a delta cannot deny its own removals.
* **Three registry rows** — `graph.heat_cycle` on `H`, and palette-only `graph.filter_layer` /
  `graph.filter_clear` — so `graph.layout_cycle`'s label now names three layouts. The published tables in
  ADR-006 §7.3–§7.4 and the Python keymap that owns them were updated together.

### What a review knew, kept in the repository (`reco-552`)

A review produces more than a verdict. It produces *"we deliberately allow this cycle because the port is
the boundary"*, *"does anything still call the old entry point?"*, *"follow up once the migration lands"* —
and that is the part that normally dies in a pull-request thread. The **review memory** puts it in the
repository instead: committed markdown under `.fathom/reviews/notes.md`, with a mirrored index, so it is
reviewable in the same PR as the change it describes, survives a rebase, answers
`fathom context <path-or-episode>`, is searchable from the shell and the TUI, and is answerable by the agent
over MCP. The module is `crates/fathom-analyze/src/notes/`; the store mirror is migration 10,
`decision_note` (ADR-004 Appendix A).

* **Four kinds, and none of them a sign-off.** `decision`, `question`, `followup`, `annotation` — ADR-004's
  vocabulary, not a new one. A note never clocks the north-star metric; the ledger still owns the gate. Each
  is attached to an **episode** by the same four digests a sign-off carries, or to a **path** (a path anchor
  is a state, reported as `anchored`, never as a failed link), and a note with nothing to point at is refused
  at write time.
* **Appending is the only edit.** Answering a question appends a note with `about: nt-…`; withdrawing appends
  one with `withdraws: nt-…`. Nothing is rewritten and nothing is deleted, and every block ends with a
  `digest:` line hashing its own lines, so editing a committed note is detectable — by
  `fathom note verify`, which needs no index at all and so works in a fresh clone. Unknown fields are
  reported with their file and line rather than dropped, exactly as the ledger does.
* **A note is still linked after a rebase**, because it goes through the same re-link ladder a sign-off does
  (exact → fingerprint → shape → member-set) and reports which rung placed it. What the ladder cannot place
  says `ORPHAN` and why, in the file and in every report, rather than vanishing: a note whose history was
  rewritten away is a fact a reviewer needs. Measured in `crates/fathom-analyze/tests/notes.rs` against real
  `git` rewrites.
* **`fathom context <path-or-episode>`** answers *"why did we do X, who said so, and what is still open"* —
  decisions with their reasons, open questions and follow-ups, and the ledger's dispositions over the same
  episodes, each with author, date and the evidence lines beside it. One structure feeds the terminal,
  `--json`, the MCP tool and the artefact, so they cannot tell three stories about one query.
* **`fathom note`** is the door a human uses: `list` (filterable by kind, status and target), `add`, `show`,
  `answer`, `withdraw`, `verify`. Author identity resolves the same way a sign-off's does, and a note nobody
  can be attributed to is refused. Both verbs are driven as a binary in
  `crates/fathom-cli/tests/notes_verb.rs` — the printed answer with its author and its date, the `--json`
  contract ids, the exit codes (`2` for an id that is not in the memory, `7` for a block that no longer
  hashes), the refusals that leave the file byte-identical, and `kind:note` in the search corpus the TUI's
  `/` reads.
* **The agent's half** (`e6.4`'s door, now on the real record): `review_memory` is a **read** tool, so a
  read-only session can answer *"why did we do X"* with the recorded decision and its citations — episode,
  digests, member commits, target, file anchor, tamper digest. `add_review_note` appends to the same file a
  person writes, attributed to the agent (or to the human named in `reviewer`, which the file then says),
  redacted and bounded on the way in, idempotent per `(author, kind, text, target)`. `.fathom/reviews/notes.jsonl`,
  the interim store `e6.4` wrote, is superseded: it is not read, and moving one is
  `fathom note add --kind annotation "$(cat old.jsonl)"`.
* **The mirror is disposable** (ADR-004 §12, again): the write path is markdown-then-fsync-then-mirror, so a
  crash loses a row and never a note, and deleting `.fathom/index.sqlite3` loses nothing that the next
  `fathom note list` cannot rebuild.
* **Notes appear in the [export artefact](docs/concepts/export-artefact.md)** as their own block, oldest
  note first, diff-friendly one line per field — and an unanswered question is an open checklist item, so
  `--strict` will not call a range finished while someone's question is still sitting in the memory file.

Six new verb invocations (`fathom context`, `fathom note list|add|show|answer|withdraw|verify`), one MCP read
tool, one migration (`SCHEMA_VERSION` 10, which invalidates every derived row and re-derives it, migrating
nothing), documented in [docs/concepts/review-memory.md](docs/concepts/review-memory.md).

### A declared architecture that refuses things, and the violations this turn caused (`reco-cfp`)

`[layers]` already said where things live. Now `[layering]` says which edges between them are refused —
`forbid`, `allow`, boundary `only`, `no_upward`, `no_cycles`, `no_cycle_within_layer` — and the answer a
reviewer gets is the delta, not the inventory: **what this episode introduced**, with what the repository
already owed kept in a different box. The engine is `crates/fathom-analyze/src/layering/`; the CLI verb that
prints it is **reco-ngk** and the scorecard that charts it is **reco-al1**, so this ticket ships the
judgement and the numbers, not a new screen.

* **Rules are checked when the file is read, and a rule that cannot match anything is refused.** An endpoint
  naming no declared layer, `no_upward` with no `order`, an `only` written with the wrong arrow, a duplicated
  rule, an `allow` that no `forbid` is broad enough to need — each is dropped from the set in force and
  printed with its line, its reason and the spelling to write instead. A config that silently enforces
  nothing is worse than no config, and "no `[layering]` table", `mode = "off"` and "looked, found nothing"
  are three states that are printed as three states.
* **One violation is one line you can act on**: the rule and where it was declared, the offending edge at
  `file:line` — the line the *import statement* is on, from `e3.1`'s structural records, with an explicit
  note where no span was recorded — why it is wrong, and a fix concrete enough to paste.
* **Judged at the granularity the extractor guarantees** (`e0.12`'s checkpoint): a Go edge is judged
  package-to-package and says so, and a package whose `[layers]` globs cut in half has no single layer, so
  its edges are counted indeterminate rather than guessed.
* **The baseline is [the ratchet](docs/concepts/ratchet.md), a second row kind.** `kind = "layer"` in the
  same `.fathom/ratchet.toml`, keyed `rule::src->dst`, and the cycle-rule key matches member sets the way the
  cycle ledger matches them. Four boxes — news, debt, back-due, stale — and nothing is auto-written or
  auto-deleted. The key a human types and the key the code generates agree however the arrow was spelled.
* **The export is the contract** (ADR-007 §3): `violations`, `new_violations`, `baselined_violations`,
  `resolved_violations`, `edges_checked`, each rederivable from the lists beside it.

Ten `[layering]` keys are declared and documented (`mode`, `order`, `forbid`, `allow`, `only`, `ignore`,
`no_upward`, `no_cycles`, `no_cycle_within_layer`, `max_violations`), documented in
[docs/concepts/layering.md](docs/concepts/layering.md) and in the key reference. `crates/fathom-analyze/tests/layers.rs`
(41 tests) is the configured example repository: every seeded violation found, every near miss not, the
delta separating this turn from the debt, and each refusal demanded by name.
`tests/layers_episode.rs` (6 tests) drives the same answers through the real lane, and asserts that the
graph built by applying the stored delta rows yields the same violations as a fresh materialisation.

### Memory has a ceiling now: bounded caches, back-pressure, and the numbers that say so (`reco-93w`)

The claim is ADR-004 §11.8's, restated as something a person can set: **peak resident memory is a
function of configuration, not of repository size**. A number in `benchmarks/budgets.json` is a
measurement of whatever the code happened to do; a ceiling is a container that refuses to grow past a
figure somebody chose, arriving from `fathom.toml` through the same layers as every other knob.

* **Three pieces, deliberately small** — `fathom_core::memory`: `Ceilings` (six knobs in bytes, each
  clamped to a range with the clamp *named* in the run's notices, because a silently-clamped budget is
  a support ticket a week later), `BoundedCache` (a byte-weighted LRU that counts what it evicted, by
  class), and `Queue` (a bounded, blocking hand-off — a producer whose consumer has fallen behind
  **waits** instead of allocating, which is the difference between a buffer and a heap with extra steps).
* **Applied to the four structures that hold a repository's bytes** — the analysis payload cache behind
  `Store::read_blob` (`rss.analyzer_working_set.mib`), a graph lane's materialised snapshots
  (`graph.snapshot_cache_mib`), one decode buffer per ingest worker (`ingest.size_cap_mib`), and the
  batches in flight between normalizing and writing (`write.queue_batches`). Bounding that hand-off is
  also what let the two stages of the spine overlap, which they previously were not.
* **The spill is the store, not a second format** — every byte these caches hold is class R
  (ADR-004 §10): eviction is "stop holding it", and a payload is re-read from the `blob` row while a
  snapshot is re-folded from its `graph_delta` chain. The durable `cache` table got §8.6's budget with
  a protection set (`cache.budget.mib`, `cache.protect_recent_minutes`), enforced when a run closes and
  never inside a lookup — plus migration 9, the two indexes its reachability probe needed after an
  unindexed one turned out to be a table scan per blob at 200 000 blobs.
* **Observable, or it did not happen** — `fathom stats` prints the ceilings in force and what they
  cost: bytes held, rows and bytes evicted over the store's life, the same split per analyzer, the
  in-process evictions and refusals, and `fathom index --stats` prints batches at the peak and the
  back-pressure counter beside it. And the working set is *released* when the analyses end, because the
  budget is a peak: the gate's peak used to land in the episode rebuild, with a full working set in hand
  from a pass that had already finished.
* **Measured** — `crates/fathom-analyze/tests/memory_ceiling.rs` indexes a 100 000-commit fixture and
  asserts the process's own `VmHWM` against 512 MiB, printing the peak as each phase starts so a run
  that goes over says *where*, and writing the peak, the queue high-water, the cache footprint and where
  the run's minutes went into `benchmarks/reports/memory-ceiling.json`; knobs and their trade-offs are
  in [docs/concepts/memory.md](docs/concepts/memory.md).

### The structural extractor grows a tail: Go, Ruby and Java, and the scan policy to bound them (`reco-dlp.1`)

`e3.1` shipped three of the six grammars spike `s1` measured; the other three each needed a mechanism
rather than a mapping table, so they were split out rather than bundled past the plan's two-day line.
They are in now, in the same harness, in the same report, on the same relation scoring —
`benchmarks/reports/e3-1-structure-core.json` lists six languages and one definition of "measured",
because a second harness would be a second set of rules about what that word means. P 1.000 / R 1.000
on definitions, exports, imports and supertypes in all three, 12/12 synthetic scopes, 25/25 wildcard
edges, and every verdict earned rather than defaulted (`go` `pass`, `ruby` and `java` `conditions`,
each with a named blind spot behind it).

* **Go edges say they are package-granular.** An import names a directory, not a file, so a resolved
    import (`crates/fathom-analyze/src/structure/golang.rs`) fans out to every non-test file of the
    target package and each edge carries `granularity: package` plus the import path it came from —
    the shape is stored, so a month later nobody has to guess whether `granularity` is decoration.
    `Structure::granularity_sentence()` is the wording a pane must print, `dependents_of` gained a
    third list (`package_granular`) beside *named* and *unknown contents*, and the `package` clause is
    recorded as the file's unit. `//go:build` files mark their scope `build_constraint` rather than
    claiming the package is whole; the module path is deduced from the corpus, because the extractor
    is not handed `go.mod` and will not open a file it was not given.
* **Ruby is bounded, and the bound has a price written next to it.** `extract.ruby.max_files_per_run`
    (4 000), `…max_bytes_per_run` (24 000 000) and `…priority` (`lib, app, ext, spec, test`) end a
    scan that would otherwise spend 20 minutes on a big `spec/` tree; a file past the cap comes back
    `scan_budget` with its file-level numbers, exactly as an oversized file comes back `too_large`, and
    the report shows the cap biting (4 of 6 corpus files refused at a cap of 2). The MiB/s exemption
    that buys is arithmetic — 24 MB at the measured 0.612 MiB/s is 37.4 s against ADR-004's 60 s
    cold-graph budget — and `tests/test_structure_tail_contract.py` refuses the exemption without it.
    Ruby's export surface is its own file's top-level names with the note `export_surface_is_the_file`
    (no per-symbol access control exists), and the question the extractor *can* answer is
    `structure::loads(entry)`: *loading `lib/demo.rb` pulls in 4 file(s)*.
* **The Java same-package guess has a scope, and the scope is measured at 1 200 files.** A bare
    `Builder` resolves against the file's declared package and nowhere else; a single-type import of
    the same spelling shadows it (JLS §6.5.5); a name the file binds as a type parameter is a mention,
    not a use. On a machine-authored 1 200-file corpus with 58 colliding type names, 916 shadowing
    imports and 120 type-variable baits: 2 760 edges claimed, **0.000** false-positive rate against a
    stated budget of ≤ 0.02, recall 1.000. The wide scan `s1` ran claims 108 356 edges at 0.975, and
    the report re-scans with the bound-name rule off to show *that* rule is load-bearing too — 4.17 %,
    over budget, on nothing but method type variables. `extract.java.same_package_scan` selects the
    scope; the wide one stays selectable so the cost of it stays a number.
* **The structural schema version went to 2**, and says why: `granularity` and `package` on every edge,
    `package` on every structure, `scan_budget` in the refusal vocabulary. A version-1 row is not wrong,
    it is mute about a Go edge's granularity, which is a question a stored row has to answer (`e3.2`).
* **Two kinds of throughput exemption, both paid for.** Ruby's is the scan-budget arithmetic above; Go
    (542 files/s on the replica) and Java (346) are exempt from the replica itself, because a fixture
    copied 444 times turns a package into 800 files and measures the fan-out rather than the grammar —
    they are judged on a repository-shaped corpus instead (1 240 Go files at 2 060 files/s, 1 200 Java
    files at 1 113), with the MiB/s canary scaled by that corpus's own mean file size so the two floors
    cannot contradict each other.

CI now re-measures the whole report with all six grammars and runs the tail's contract test
(`crates/fathom-analyze/tests/structure_tail.rs`, 19 tests) alongside it;
`docs/concepts/structure-extraction.md` carries the numbers and the rules in prose.


### A warm re-index with nothing new re-walked the history; now the refs answer it (`reco-7tc`)

`fathom index` on a repository whose `HEAD` already had a row cost 48–58 ms — five times
`warm.reindex.noop.ms.small` (10 ms p95), and about the same as a run that *had* a new commit to process,
which is how you can tell a walk is being repeated rather than skipped. Inside the run it was: ~6 ms
walking 500 commits the store already held, and ~48 ms re-segmenting every episode and writing the rows
back identical. The breach sat on the register in `benchmarks/baselines/perf-gate.json` with a 70 ms
ceiling and this ticket as its owner, so it could not grow quietly; this is the day the entry came off.

* **The refs are asked first, and for the no-op they are the whole answer.** `index::run`
    (`crates/fathom-analyze/src/index.rs`) now opens with `nothing_new`: every ref is compared with the snapshot the store already
    holds, and when they agree, `HEAD` has a commit row of its own, and no commit is owed a derivation,
    the walk, the analysis pass and the episode rebuild are skipped instead of repeated. The run's own
    time went 53 ms → 2 ms on the 500-commit fixture.
* **Five things refuse to be quiet**, each for its own reason: a `--refresh` or `--rebuild` pass, which
    exists to re-measure the cache; a `HEAD` with no commit row of its own, so an unborn branch still says
    so in its own words; any ref that moved, vanished or was never snapshotted — **including a branch off
    to one side**, which changes what is reachable while `HEAD` sits perfectly still; a correction to an
    attribution (`reco-w7q`), which moves no ref at all and changes only what the store should *say* about
    commits it has already indexed, so the write that stores or forgets one now drops the store's own ref
    snapshot and the next run re-derives what the label means; and an input the rebuild reads that git cannot
    see — a harness stream that is not the one the last rebuild read (`e4.2`), or a `[episode]`/`[intent]`
    block that was edited. Both appear, vanish and grow without git noticing anything, so the run that
    rebuilt leaves a *witness* of what it was handed in `meta` and the next run has to match it; that is also
    why an agent-driven repository — the one this whole project is about, and the one that always has a stream
    — still gets the no-op instead of being written off by a blanket refusal. And a run that stopped early
    unsnapshots the refs, because a store whose refs say "nothing moved" while its episodes say "an analysis
    is missing" is a store that would skip finishing its own work.
* **A quiet run still reads the worktree** — §6.4's provisional records of a dirty file are unchanged —
    still stamps a run left `running` by a dead process as crashed, and prints the intent tallies the
    *store* holds rather than zeroes for a step that did not run. It opens no `index_run` row: that table
    is the history of the runs that walked, and a pass that walked nothing has no numbers to add to it.
* **The verb says which of the two it was**: `.fathom/index.sqlite3: nothing new — HEAD 231528d is indexed
    and no ref has moved, 2 ms`; `--json` gained `nothing_new` (and `schemas/index.schema.json` with it)
    so `commits.walked: 0` reads as a quiet run or as an empty repository without guessing; `--stats`
    says `warm run, nothing new: 0 commit(s) walked`.
* **The no-op is measured store-side, and every page that prints it says so.** `warm.reindex.noop.ms`
    descends from ADR-004 §13.3's prototype row — 4.8 ms, store-side, like the `warm.reindex.scaling_ratio`
    rationale spells out — and the sentence that budgeted it is "`ctrl-r` twice must not feel like work",
    which is a re-index inside a process that is already running. Charged against the whole *process* the
    same budget is unreachable for any verb: of the 16.2 ms a no-op `fathom index` takes fork to exit, 14
    ms is fork, dyld, `Repository::discover` and `Store::open` — the toll every verb pays, and more than
    the budget before the run has decided anything. So `tools/perf_gate.py` and `tools/mvp_acceptance.py`
    gate the run's own `elapsed_ms` and quote the whole-process number beside it, in the same note, so the
    basis changed and nothing was hidden.
* **The register entry is gone and the baseline moved with it** — `warm.reindex.noop.ms.small` re-blessed
    at 2.0 ms, and that row only: the same box measures eight unrelated rows red against the bless of
    `89f7198` (store bytes per commit over its own budget among them), and blessing those would have made
    the gate agree with a regression instead of catching one.

*Asserted by*: `crates/fathom-analyze/tests/incremental_index.rs` (the no-op's counters — no lookups
anywhere, the walk never ran, the tallies still the store's; a quiet run that still reads a dirty
worktree; a moved ref that is never quiet although `HEAD` sat still; a correction that is never quiet
although nothing moved at all; a stream that is quiet when it has not moved and is not quiet when it went
away; a `--refresh` that never is; and an ignored `--release --ignored` check of the 500-commit fixture
against the published budget), `crates/fathom-analyze/tests/episodes.rs`, `tests/intent.rs` and
`crates/fathom-cli/tests/intent.rs` (the three tests that found this ticket's first wrong answers:
relabelling a commit and relabelling it back once the correction is forgotten, a stream the repository
outlived, and a `fathom.toml` that was removed between two runs), `python3 tools/perf_gate.py --check` on that same fixture (the CI
measurement, now green at 2 ms against 10 ms), and the regenerated
`benchmarks/reports/e1-12-mvp-acceptance.json` row.

### The shell threw away its own frame report, so the frame budget was not measurable from the product (`reco-08r`)

`fathom_tui::frame::run` measures every draw it makes against ADR-002 §6's 16 ms budget and returned a
`Report`; the binary dropped it on the floor with the comment "the run's own numbers are not the answer
anyone asked for … `fathom doctor` is where a slow frame gets reported". `fathom doctor --deep`
contained no frame row. So the MVP acceptance run could not answer "what was the p99 draw time of this
session" from the product, and quoted what a pty driver had measured between a keystroke and a changed
screen (p95 1.9 ms, worst 250.5 ms) — honest, available, and a different quantity wearing the same name.

* **The shell says its numbers as it gives the screen back**, one line on stderr in the same voice as
    `fathom ambient`'s — `fathom: tui: frames 41 · events 63 · p99 3.20ms · worst 12.40ms · over-budget
    0 of the last 41 draws · budget 16.00ms · 132x40 · quit` — or one JSON object on stdout under
    `fathom tui --stats`, which a bare `--json` now also means.
* **And leaves them behind**, at `.fathom/frame-report.json` — class E like the watcher's heartbeat,
    renamed into place so a reader never sees half a file, and read by `fathom doctor`, which now prints
    the last session's p99 and over-budget count with the window they were taken over, the budget they
    were measured against and the frame size they were paid at. `doctor` names the file whether or not it
    is there (ADR-005 §3.1): "no shell has run here" and "the record could not be read" are two
    sentences, and both beat a silence that looks like a health check.
* **Every statistic now travels with its denominator.** `Report` gained `budget`, `window` and `size`,
    because the percentiles are taken over the scheduler's last 128 draws while `frames` counts the whole
    run, and a frame cost without the size it was paid at is half a number. `LoopEnd` says itself in
    words now rather than being printed with `{:?}`.
* **The measurement is not a finding.** A draw that missed the budget on a loaded machine is not a broken
    index, so the section never moves `fathom doctor`'s exit code — not under `--deep`, not under
    `--strict`. It states, and leaves the judgment where it belongs.
* **`--plain` says what it did not measure**: a piped run draws one frame and exits, so there is no loop
    to time, and a `--stats` asked of it explains that instead of printing nothing at all. The modes that
    print a table and leave (`--show-keys`, `--show-theme`, `--list-themes`) conflict with the frame flags
    in clap, so a combination that could not have measured a loop is a usage error rather than a flag that
    quietly did nothing.
* **The acceptance harness cites them.** `tools/mvp_acceptance.py` reads the record the shell wrote and
    quotes it under `shell_frame_report` — the product's own numbers, beside the pty's keystroke
    latencies and not confused with them, since no published budget family is a draw time.

*Asserted by*: `crates/fathom-tui/tests/frame.rs` (5: the loop driven for real, its report checked as data
and as the line printed), `crates/fathom-cli/src/framereport.rs` (3: the record's denominators, the
write-and-read round trip through the lines `doctor` prints, and absent-vs-damaged as two sentences),
`tests/test_frame_report_contract.py` (9, over a real pty on the built binary: the exit line, the
`--stats` object, the record `fathom doctor --deep` quotes, no-record and damaged-record both stated,
frame flags aimed at a mode with no loop refused at the door, and a `--plain --stats` that says it
measured no loop).

### Nothing enumerated the episodes, so the rail could not be checked from a script or CI (`reco-11b`)

The acceptance walkthrough reaches the reviewer's main list and says *the rail shows 213 episodes*. Nothing
in the binary could put a number on that sentence: `fathom show` takes one revspec and answers about one
episode, `fathom analyze`/`narrative`/`why` are one-episode verbs too, and `fathom --plain` renders one frame
of whichever view the stored session happens to name, with no flag to choose a view and no rows to feed it.
So the claim was checked by a human looking at a screen, and CI — which is exactly what a headless contract
exists for — could not assert on the reviewer's list at all. ADR-006 §6.7 says every view has a headless
equivalent; the rail, the first thing the product draws, did not have one. Two hints in the shipped binary
already told people to run `fathom episodes` (`fathom review`'s "names no episode", the diff pane's provider
error), which is the sharpest possible statement of the gap: the promise existed and the verb did not.

* **`fathom episodes` exists**, and it prints the list as rows: id, time, author kind, `+add`/`−del`, path
    count, review state and the session-qualified turn, in the rail's own order (newest `last_utc`, then id),
    with the summary the rail prints as the last column. `--json` is `fathom.cli.episodes/1`, checked in
    [`schemas/episodes.schema.json`](schemas/episodes.schema.json) and round-trip-tested like the four
    verbs that came before it.
* **One read, two surfaces.** The verb lives in `crates/fathom-cli/src/episodes.rs`, beside the store→frame
    read `reco-wzt` wrote, and the module is no longer gated with the shell — `fathom-tui` still opens no
    store (ADR-002 §5.2, lint `E01`), so both surfaces take their rows from this one function. The filters
    are the store's `EpisodeQuery`, the shape the shell filters with, so `--author`, `--kind`, `--state`,
    `--path`, `--since` and `--until` count what the rail drew rather than a near-relative of it.
* **Three counts, because there are three questions.** `index.episodes` is everything the index holds, which
    is the rail strip's number; `page.matched` is that after the filters; `page.returned` is that after the
    page. A page shorter than the selection is announced in the notes and flagged as `page.truncated`, and a
    list longer than the shell's 500-row page says that too — the rail and the index are different sizes and
    a script that conflated them would assert on a number that moves when somebody else commits.
* **The review states are the store's**, so a row that `x` accepted reads `accepted` here, and
    `review_counts` prints all six states over the filtered selection — including the states the `--state`
    filter excluded, which is the only way the number tells you what it filtered out.
* **The refusals are separated the way the published table says**: a flag *value* that names nothing
    (`--state never`, `--kind sideways`, `--since someday`) is exit 2 with the names in the hint, `--root` is
    not a repository is exit 3, no index is exit 12 with `fathom index` as the remedy, and an empty selection
    is exit 0 with a note — "how many flagged episodes?" asked of a repository with none is a question, not a
    failure.
* **Nothing in the payload is a clock.** No `generated_at`, no run id: two runs over an unchanged index are
    byte-identical, so a CI step can diff this run against last week's instead of re-deriving the world.

*Asserted by*: `crates/fathom-cli/tests/episodes_verb.rs` (8: the columns in text and in JSON from one read,
the three counts and the truncation, paging that walks the selection without gaps or repeats, the filters the
rail owns, a reviewed episode moving state with the whole breakdown printed, the usage refusals naming what
they wanted, exit 3 and exit 12, and `--include-dead` saying honestly when there is nothing dead),
`crates/fathom-cli/tests/schemas.rs` (`episodes_json_conforms_to_its_schema`, plus the schema census), and
`crates/fathom-cli/tests/tui_data.rs` (`the_rail_and_the_headless_list_count_the_same_episodes`: `fathom
--plain` and `fathom episodes` on the same store, the strip's count and the list's count required to be the
same number).

### A rewritten history left rows behind, `fathom stats` counted them as the repository, and `fathom doctor` found nothing (`reco-218`)

The acceptance walkthrough rewrites the fixture's last five commits in another terminal — `git reset --hard`,
five `git cherry-pick`s, an expired reflog, `git gc --prune=now` — and re-indexes. Afterwards the repository
had 500 commits and `fathom stats` printed:

```text
scale     505 commit(s), 630 file row(s), 5 author(s)
```

and `fathom doctor` exited 0. The spine is append-only (ADR-004 §6.2), so the five replaced commits were
still rows; nothing unreachable them, nothing said so, and they were still in the churn window, the taxonomy
counts and every scale line the CLI prints. ADR-004 §8.1 already named the verb that should have said it —
the pruner, "the only thing that deletes" — and it did not exist, so §6.2's "rows that are not reachable
remain, and the report says so" was a sentence with no implementation: nothing could say *so*.

* **The store now counts the history and the residue separately.** `Store::inventory()` returns the scale the
    status verbs read *and* the dead set: `retained` commit rows, `reachable` — what `HEAD`, the branches and
    the remote-tracking refs reach through the stored parent pointers — `dead`, `dead_files`, and up to twelve
    dead commit ids, newest first. `fathom stats`' `scale` line is now git's number, and a `dead` line names
    the rows it left out and the verb that removes them. Reachability deliberately does **not** start from
    tags: a tag pinned at a rewritten commit would otherwise hold its ghost open forever, which is ADR-004
    §8.7's open question rather than a licence to guess here.
* **Where the store cannot tell, it says so instead of picking.** No ref tip to walk from, or a reachable
    commit whose parent row is missing from the store — a fetch-sparse repository, a ref moved mid-run — and
    the walk reports `unjudged` with its reason, every unreachable row is held out of the dead set, and the
    pruner refuses. `fathom stats` prints that as a `reach` line: rows it could not place, never rows it
    killed.
* **`fathom index --prune` exists**, and it is the only thing in the codebase that deletes spine rows (ADR-004
    §8.1). It runs *after* the index, against the ref tips that run just wrote; it takes the provably dead
    commit rows, the file rows that cascade with them, any episode that loses its last member and carries no
    decision, and the document row that would have orphaned behind it. `--dry-run` prints the same numbers
    from the same statements and removes nothing. A real prune opens an `index_run` of its own, `kind =
    'prune'`, so a prune killed halfway is a run the next opener marks `crashed` and `fathom doctor`
    complains about.
* **A prune may never lose a review** (ADR-004 §8.2), and the guard is counted rather than subtracted
    silently: a dead commit whose episode carries a review state or a mirrored ledger entry stays, and the
    report says `1 dead commit row(s) left in place: a review or a sign-off points at them`.
* **`fathom doctor` reports it, and only `--strict` pays for it.** A store holding rows for commits that no
    longer exist is not an index that is failing to answer — HEAD is indexed, the queue is segmented over the
    history that still exists — so `index_dead_rows` is a new *untidy* severity: exit 0 from `fathom stats`
    and from plain `fathom doctor`, exit 12 (`INDEX_DEGRADED`) from `fathom doctor --strict`, with the remedy
    on the line after the finding either way.
* **ADR-004 §11's byte budget got the sentence it needed**: `store.bytes_per_commit_spine` is measured on the
    spine half of the store file — everything that is not `cache`, `cache_input`, `session_state`,
    `index_run` or `schema_meta`, split with `dbstat` — over the count of commits the refs *reach*. The whole
    file on the same fixture is 4 718 B/commit against a 4 096 B spine budget, so the two figures are not
    interchangeable, and dividing by the row count would flatter a store a rewrite had left rows in.

*Asserted by*: `crates/fathom-store/tests/dead_rows.rs` (14: the reachable count against git's, a rebase's
ghosts named, a tag not holding its commit alive, a remote-tracking branch holding its own, the two
unjudgeable shapes and the refusal they earn, the prune's cascade and its idempotence, and both halves of the
review guard), `crates/fathom-cli/tests/resilience.rs` (17, including a real `git` rewrite whose ghosts are
reported at 2 and pruned to 0 with `doctor --strict` agreeing, and a rewrite of a reviewed commit whose
decision survives the prune), and `tests/test_history_rewrite_contract.py` (7 against the built binary,
replaying the acceptance script's own rebase step out of `docs/demo/mvp-500-commit-review.json`: 500
reachable, 505 retained, 5 dead, exit 0 from `stats`, exit 12 from `doctor --strict`, and a dry run that
removes nothing before the real one removes exactly five).

### The status strip printed `unknown` for four facts the CLI had already read from the store (`reco-zv9`)

The bottom strip read:

```text
⏺ epoch unknown · unknown eps · queue unknown open · idx unknown
```

on a repository where `fathom stats` — the same binary, the same store, seconds earlier — printed
`epoch 1 ready`, 500 commits, integrity ok and 0 commits owed an analysis. ADR-006 is right that
`unknown` is a value; it is a value for what *cannot* be known, and all four of these could be known
by the process that had already opened the store to read the session. ADR-006 §5.2's own mock line has
been `⏺ epoch 41 · 3 412 eps · queue 7 open · idx warm` since it was written, and no build could print
it: `idx warm` had no source at all, and an indexed repository with an empty queue was told
`queue unknown open`.

* **The hand-over grew the two facts the strip was missing.** `index::Facts` — the batch `fathom-cli`
    reads before the first frame, and the same one the empty states are written from — carries
    `queue_open` and `owed_analysis` now, read from the store the composition root already had open.
    `fathom-tui` still opens nothing (ADR-002 §5.2, lint `E01`), and it still may not guess: a fact
    nobody read is a `None`, and a `None` is the only thing that earns an `unknown`.
* **The verdict is said.** `idx warm` when the index owes no analysis, `idx cold` when it owes one or
    the epoch is still `building` — `fathom stats`' `state` line, in the four characters the strip has
    room for, off the same `pending_derivation` read that verb makes.
* **The queue is the index's, not the page's.** The shell still counts its own rows when those rows are
    the whole queue, so the number moves as a reviewer reads (`reco-c05` is not reverted); where the
    rail holds only a page of a longer index, or a filter has hidden rows, or the rail drew nothing at
    all, the page is not the queue and the store's count is printed instead. Over an index of 600 open
    episodes the strip says `queue 600 open` while the rail holds the 500 a page may hold and says
    `index holds 600` doing it — two sentences about two different sets, which is the honest version of
    one number. And `0 open` over an indexed repository with nothing in it is the store's answer, not
    the shell's guess from an empty rail.
* **`unknown` kept its job.** A repository with no index, a store that would not open, a binary with no
    SQLite backend and an embedder that hands the shell nothing all still print what they mean, in
    words the fix did not soften: no zero where the truth is that nothing was read.

Because the watcher re-reads `index_facts` after every run it lands, a re-index from the frame moves the
epoch, the count, the queue and the verdict on the same tick the rows arrive — the verdict is not a
startup photograph.

*Asserted by*: `crates/fathom-tui/tests/status_strip.rs` (8: ADR-006's line character for character,
the cold verdict, the empty queue, one fact missing taking only its own `unknown` down, the page that
is not the queue, and a golden pinning the known and the unknown strip together),
`crates/fathom-tui/tests/risk_rank.rs` and `tests/layout_matrix.rs` (the live count and the pinned
frames it draws, which did not move), and `crates/fathom-cli/tests/tui_data.rs` (3 against the built
binary: `unknown` four times before `fathom index` and none of them after, three reviewed episodes
three fewer open, a `building` epoch said as `idx cold`, and a 600-episode queue printed as 600 over a
500-row page).

### `<C-r>` narrated a re-index instead of doing one, and named a ticket that had closed (`reco-j7t`)

The key is bound to `index.reindex`, labelled *Re-index this repository*, offered by the palette, and listed
in ADR-006's table. Pressing it wrote one string into the frame and stopped:

```text
indexing 0 % · unknown left (no store handle in the shell — e1.4 owns the worker)
```

Every part of that was false. The shell *does* have a producer — `reco-bpc` gave it one two tickets ago, the
composition root's own `fathom-cli/src/feed.rs`, which has been indexing on the frame's clock ever since.
`e1.4` closed weeks ago and had already shipped the worker it was named as owing. And a progress row written
once, at the moment of the keystroke, is a placeholder wearing a percentage, which is the one thing
ADR-006 §10.3 refuses outright: `warm.reindex.noop.ms` is even budgeted against `ctrl-r` twice not feeling
like work.

* **A forced ask goes to the indexer**, whether or not `HEAD` moved since the last look. "Nothing moved since
    I looked" is not the answer to "is this index up to date": an index left behind by an interrupted run, a
    `git reset`, or a `fathom index` that was never re-run moves for no `.git` event at all. A *poll* still
    runs only on movement — an idle frame may not walk its repository four times a second.
* **The run reports its own numbers** through `Update::work`, in the shell's one permitted shape: the phase
    word and unit word the indexer prints for `fathom index`, the commits derived and the commits that owed
    derivation, and the analyses computed. The shell turns them into the estimate row —
    `indexing 40 % · ~2 s left · 200 / 500 commits · 1 204 analyses computed` — because the estimate is a
    ratio of two time samples and only the loop owns a clock (ADR-002 §4.4). Same counters, same sentence as
    the CLI; neither is invented.
* **And it is on the status strip**, which is the half that made the row useless: the progress line lived only
    in a pane that had nothing else to print, so the reviewer who pressed `<C-r>` over a rail of 500 episodes
    — everyone who has ever wanted a re-index — saw no progress at all.
* **A slice that stops for its budget keeps being asked**, on the wake-ups the loop already takes: no timer,
    no thread, no new wake-up budget (ADR-006 §0.1). The denominator belongs to the run and not to the slice
    — each slice asks the store what still owes derivation and restarts its own count there — so the row
    counts up to 500 rather than restarting at `0 %` every quarter second. When the run lands, the row goes
    and the strip's epoch and episode count move with it.
* **A budget-stopped slice used to lose the rest of its work.** `oids_pending_derivation` asked only about
    runs that never closed cleanly, and a slice that ran out of budget *closes `ok`* — it did not fail — so
    the next slice walked past everything the last one had not reached. A quarter-second bite of a 130-commit
    backlog derived 79 commits, the next bite derived nothing, and the index said it was finished at 79. The
    query now honours `index_run.interrupted`, the column that exists to say exactly that, under the same
    one-run's-worth-of-re-lookups bound as the crash case. `fathom watch` and `fathom ambient` were losing
    the same work.
* **The two answers that are not "it ran"** name something the reviewer can do: `--no-watch` names the flag
    that would move the frame, an unindexed repository names `fathom index`, and a frame its caller handed
    no producer at all — an embedder, a test frame; the shipped shell always has one — says so and names the
    command. No toast names a closed ticket as if it were pending work; there is now a scanner in the test
    file pointed at the old sentence, so it cannot come back.

*Asserted by*: `crates/fathom-tui/tests/watch.rs` (18: the row is built from a producer's counters, it lands
on the strip, a run that finishes inside one ask never shows one, a finished run takes its row with it, and
none of the three answers names a ticket), `crates/fathom-cli`'s `feed` unit tests (the slice arithmetic
behind the numbers, including the frozen denominator), `fathom-store`'s `spine` tests (a budget-stopped run
leaves its commits on the derivation list, and one clean run pays the debt), and
`tests/test_tui_reindex_contract.py` (4 against the built binary over a pty, including the case the poll
cannot fix — a repository that moved before the frame opened, so the re-index can only be the keystroke).

A re-index that spans more than one slice still has one step left to join up: the episodes the last slice
writes are not on the rail until a later index run publishes them, so a 130-commit backlog shows its
progress row, says the run landed, and leaves the count where it was until the next `fathom index`. The
derivation now finishes; the rail's rows lag by one run, in the shell and in `fathom watch` alike.

### An open TUI was a photograph of a moving repository: nothing fed the frame (`reco-bpc`)

The product premise is physical — fathom sits in a second terminal in ambient mode while an agent works in
the first. The frame in that second terminal had no producer: `frame::Options` documented a worker that
would push rows, and nothing anywhere constructed one, so a walkthrough that rewrote five commits while the
TUI was open watched a strip that never moved. Detection was never the missing part — `fathom watch` notices
a commit made elsewhere in ~160 ms — the frame just had nobody to tell. A frozen rail reads as a quiet
repository, which is the worst possible misreading of a repository that is not quiet.

* **`fathom_tui::watch` is the seam that was missing**: a `Watcher` the frame asks *has anything moved?*
    and an `Update` in the only shapes the shell may draw — a batch of episodes and the index's numbers.
    Like `FrameSink`, `diff::Provider` and `compare::Provider` it is a trait the composition root
    implements, because the shell still opens no store, reads no git and runs no indexer (ADR-002 §5.2,
    lint `E01`); `fathom-cli` probes, indexes, re-reads, and hands rows over.
* **The loop stays tickless** (ADR-006 §0.1). The watcher is asked on the wake-ups the loop already takes
    for `<C-c>` — no timer, no second thread, no new wake-up budget — and a watcher that answers `None`
    draws nothing, which is what keeps an idle frame an idle process. The probe is the same one
    `fathom ambient` runs (four `stat`s and one ref read, throttled to a 400 ms look), and `.git` moving
    for a reason that changes nothing a frame draws — `git status` refreshing the index — is deliberately
    not a reason to re-read.
* **One writer per repository, the same rule the glance pane keeps.** When a `fathom watch` heartbeat is
    beating or a daemon is serving, the frame reads the index and does not write it, and says which it is
    doing; otherwise it runs the very same `IndexJob` `fathom watch` runs, in one bounded slice per look.
    The re-read is `index_facts` + `episodes::load` together, so the strip's numbers and the rail's rows
    cannot disagree about the epoch they came from, and the reviewer's cursor stays where it was while the
    rows move under it.
* **A frame that cannot watch now says so**, in the frame: `snapshot · nothing is indexed yet` in the
    status line, and the whole sentence — limitation *and* remedy — in the notice list under `?`. That is
    the other half of the acceptance criterion, and `--no-watch` makes it reachable on purpose: a still
    picture that admits it is still is a limitation, and one that does not is a lie about the repository.
* **`<C-r>` answers**, with a watcher installed: it re-reads the repository now instead of printing a
    placeholder progress row that promised someone else would get to it eventually.
* **And the number is gated on the frame, not just on the watcher.** `tools/mvp_acceptance.py` now commits
    from the harness and times until an open TUI's grid changes with the reviewer pressing nothing, against
    `watch.detect.ms.small` (1 000 ms): **210–330 ms** measured through the pty on the machine this was
    written on, debug profile, at the shipped 400 ms poll.

Asserted by `crates/fathom-tui/tests/watch.rs` (11 tests: the frame changing on a scripted producer, the
cursor surviving rows that moved, `None` drawing nothing, `snapshot` and its remedy, `live` saying nothing
more, `<C-r>` on both branches), `tests/test_tui_watch_contract.py` (5 tests that drive the shipped binary
over a pty, commit from this process, and assert the grid moved inside the budget — plus the `--no-watch`
and never-indexed branches), and the pty measurement above.

### The space bar went to the keymap instead of the field, so no documented query could be typed (`reco-q45`)

`e1.12`'s own acceptance script types `DROP COLUMN` into `/`, and the box filled with `DROPCOLUMN` —
which the session then remembered as the question the reviewer asked. ADR-006 §7.1 says that in
`prompt` and `pick` the field owns the keyboard and a printable key not bound there is *text*; the
dispatch path wrote that rule as "one character that is not `<…>`", and the terminal does not send
the space bar as a character. It sends the chord `<Space>` (`keymap::chord_of`), so the space
quietly fell out of the field as an unbound key while every letter stayed. Everything §5.2
documents is multi-token — free text plus `author:` `path:` `risk>` `dep:` — so a field that eats
the space cannot express one filter token, let alone the text that composes with it: `/`, the `,`
palette filter (whose matcher has always split the query on whitespace) and the `F` filter chooser
were all typeable only as one mashed word.

* **`keymap::text_of` is now the one answer to "is this chord text?"** — `<Space>` is a space, `j` is
  a `j`, and a named chord (`<Enter>`, `<C-a>`) is still a chord, so an unbound control typed inside a
  search box still toasts instead of vanishing. `App::apply_steps` asks that question instead of
  counting characters.
* **The rule is pinned from both ends.** Nothing in the published table binds bare `<Space>` in
  `prompt` or `pick`, which `tests/keymap.rs` asserts next to the text rule; `tests/shell.rs` types
  `author:mira path:src/harbour` through real key events into `/`, `,` and `F` and requires the field
  to hold exactly that, spaces included; and `tests/search_box.rs` checks the report got two filters,
  that free text and a token compose, and that the session restores the query with its spaces.
  `tests/test_search_prompt_contract.py` presses the same query on the built binary over a real pty —
  the byte the terminal sends is the seam that broke — and reads the query line back off the screen.
  ADR-006 §7.1 states the rule out loud now, including why the chord spelling is the trap.

### The structural extractor core: Rust, Python, TypeScript, and the harness that keeps them honest (`reco-zoz`)

Spike `s1` measured what six grammars can do and told the next ticket to copy the harness shape, not the
extractors. This is the first shipped extractor, and the durable half is the harness: a corpus with
hand-authored truth, thresholds that name the excuse categories, a committed report CI re-measures, and four
mechanism checks that refuse a 100 % score built on silence.

* **`crates/fathom-analyze/src/structure/`** extracts, per file: definitions with span and complexity proxy,
  exported visibility, imports with resolved targets, implemented traits and inherited bases, and the two
  states the spike argued for. `synthetic_eligible` is a *state* — a scope, its reasons (`macro_definition`,
  `decorated`, `monkeypatch`, `synthesised_accessors`, `wildcard_reexport`, `ambient_merge`), and the only
  sentence a surface may print about it: *may have shrunk, not shrunk*. `EdgeKind::Wildcard` is its own edge
  kind, never a named symbol, and `dependents_of` answers "who depends on this?" as two lists — the ones that
  named something and the ones whose contents are unknown.
* **A file is never lost.** Every input returns a record; one with no structure returns a reason
  (`language_unknown`, `grammar_not_compiled`, `extractor_not_shipped`, `too_large`, `not_utf8`,
  `parse_failed`, `parser`) plus its file-level measurement. A build compiled without a grammar still answers
  per file, because `--no-default-features --features minimal` is a binary CI ships (ADR-002 §6.5).
* **Resolution is a bounded iteration that reports itself**: `extract.resolution.max_passes` and
  `…max_hops` are registered config keys, every file reports `passes_used`, `converged` and
  `truncated_chains`, and the report contains the corpus re-extracted with a bound of one so the bound is
  shown biting rather than asserted. Per-file convergence, not corpus-wide — one file still working used to
  make every file look unconverged.
* **Measured, on a corpus with truth files**
  ([`benchmarks/reports/e3-1-structure-core.json`](benchmarks/reports/e3-1-structure-core.json)): precision
  and recall 1.000 on definitions, exports, imports and supertypes in all three languages, 12/12 synthetic
  scopes, 8/8 wildcard edges, throughput 1 572–3 744 files/s/core against a 1 000 floor. Every language's
  verdict is `conditions`, and the condition has a name per language: macro-written items, `@dataclass` and
  monkeypatched methods, bundler-written modules. A truth row excused as irreducible must live in a file that
  also demands a synthetic scope — `tests/test_structure_core_contract.py` refuses the excuse otherwise.
* **`structure::canonical_json` is byte-identical** across runs and input order, pinned by a golden that the
  harness's own `--snapshot` reproduces byte for byte, so `e3.2` can store this shape and diff it later.

Reproduce: `cargo run --release -p fathom-analyze --features lang-rust,lang-python,lang-typescript --example
structure_harness -- --repo . --report benchmarks/reports/e3-1-structure-core.json`, then `--check`, then
`cargo test -p fathom-analyze --features lang-rust,lang-python,lang-typescript --test structure_core` and
`PYTHONPATH=src python3 -m pytest -q tests/test_structure_core_contract.py`.


### One writer per repository: `fathom daemon`, `fathom status`, and the lease that decides who writes (`reco-kvk`)

Three TUIs and a `fathom index` used to mean four processes ready to index the same repository four times.
Now one process per repository takes the writer lease, watches, tails the harness stream and indexes on a
budget, and the other verbs are clients of it — which is ADR-004 §7.1's rule ("who writes is a fact about a
lock, not a fact about which binary you ran") turned into code, a socket protocol and a status page.

* **The lease is a storage primitive, so it lives in `fathom-store`** (`crates/fathom-store/src/lease.rs`):
  `flock(2)` on `.fathom/daemon.lock` plus a JSON record `{pid, boot_id, pid_start, started_utc_ms, socket,
  writer_epoch, build, root}`. Staleness is decided by four probes rather than by a timer where a probe exists
  — the flock, the pid, the boot id, the pid's start time — with the heartbeat age (`daemon.lease_stale_s`,
  default 120 s) as the least-trusted last resort for a process alive but wedged. Every stale lease is broken
  **with a sentence**: the notice says whose claim it was, which probe said it was dead, and what the new epoch
  is. A second `fathom daemon start` refuses and names the holder instead of starting a second writer.
* **`fathom daemon start|stop`, `--foreground` included**, so a supervisor can supervise it
  (`crates/fathom-cli/src/daemon.rs`). Without `--foreground` the parent re-executes detached, waits for the
  socket to answer, and leaves a log at `.fathom/logs/daemon.log` for the failure a detached process would
  otherwise swallow. `stop`, Ctrl-C, SIGTERM, `--deadline` and `--max-cycles` all take the one exit path:
  finish the pass, checkpoint, blank the lease, remove the socket.
* **`fathom.daemon/1`**, one request per connection over a unix socket (`hello`, `status`, `index`, `stop`),
  with the endpoint resolved by one function every client and the daemon share — repository-local
  `.fathom/daemon.sock`, then a digest-named `$XDG_RUNTIME_DIR` path when the repository path will not fit in
  `sockaddr_un`, then `$TMPDIR`. A socket file whose listener died is replaced, and staleness is decided by
  asking the path (`listen(2)`), not by a heuristic. **Windows named pipes resolve but do not bind**: binding
  needs FFI, this workspace forbids `unsafe_code`, so a Windows daemon runs with the lease and the heartbeat
  and says in `daemon.socket.reason` that it has no endpoint.
* **Coalescing, counted.** The queue keys on the *asker*, so three TUIs asking for the same thing are one
  request, a TUI that polls does not grow the queue, and `fathom status` can say who has been asking. The
  counters are the acceptance criterion: `pushed`, `coalesced`, `shed`, `served`, `depth`, `high_water`.
  A queue at its bound refuses new askers *with a reason* rather than growing.
* **The CPU budget is measured and paid, not configured and hoped for.** User+system CPU from `/proc/self/stat`
  over a rolling window; over the share the loop owes time and sleeps before taking more work, so
  `cpu.slept_ms` and `cpu.over_windows` are the visible price. Where there is no process CPU clock the number
  is still printed, from busy wall time, labelled `cpu.source: "estimate"` as the upper bound it is. A client
  heard recently shrinks the work slice to the yield slice, because the criterion is that the TUI stays
  responsive and not merely that the daemon stays under budget.
* **Back-pressure on the harness stream instead of loss**: at most `hook-bytes` per pass, stopped at a whole
  line boundary because a torn tail is not a readable event, with the unread remainder reported as
  `hooks.lag_bytes` — lag you can see rather than events quietly dropped (ADR-005 §6.5).
* **`fathom index`, `fathom watch` and `fathom ambient` ask first.** A deferred `fathom index --json` is the
  distinct contract `fathom.cli.index.deferred/1` carrying the daemon's own words, because the timings, cache
  table and segmentation of a run report belong to a run this process did not perform. `--rebuild`,
  `--refresh`, `--stats` and `--watch` stay local, each for its own stated reason. `fathom watch --json` gained
  an `indexer` field so a machine can tell "this process indexed" from "the daemon did".
* **`fathom status --json`** (`schemas/status.schema.json`, `fathom.cli.status/1`) reports pid, uptime, the
  hook offset, queue depth and cache stats — and, when nothing is running, says so with a stated
  `daemon.source` of `daemon` / `lease` / `heartbeat` / `none`, each demotion a finding with a remedy. A stale
  lease is exit `12`; a repository that is merely idle is `0`, because one code cannot mean both "fine" and
  "nobody is home". `daemon.running` is the field to branch on.

Proof: `crates/fathom-ingest/tests/daemon.rs` (21 tests) drives the real loop, socket, protocol and
coalescing with a scripted worker — including *three TUIs and a CLI make one indexing pass*, asserted with
the counters; and `crates/fathom-cli/tests/daemon.rs` (7 tests) drives the real binary against a real
repository, so the clean stop, the SIGTERM and the restarted epoch are checked where a process boundary can
still see them. The design, the three places it departs from ADR-004 §7's letter, and what it deliberately
does not do are in [docs/concepts/watch-daemon.md](docs/concepts/watch-daemon.md).

### Privacy posture, measured instead of promised (`reco-h7p`)

The promise is old (`ADR-001 §7.5`); what is new is that it is now *checked* rather than asserted, and
written down in one page. Nothing about what the binary does changed; what changed is that three of its
sentences now have a machine behind them.

* **One page.** [docs/concepts/privacy.md](docs/concepts/privacy.md), linked from the README and the docs
  front door: the two gates, what is written locally, how to read and delete it, how each claim is
  verified, and the four things the posture deliberately does **not** claim.
* **`fathom doctor --privacy` answers the question a user actually asks.** The table grew a consent
  column and a verdict next to the compile column — `off` (the code is absent), `waiting` (compiled in,
  consent withheld, here is the key) or `ON` — plus the layer that answered (`user`, `legacy`, `default`),
  the files consent may live in, the local records with their size, and the line
  `usage metrics: nothing leaves this machine`. Consent a **repository** tries to grant is read,
  discarded, and named: `ignored: this repository's fathom.toml sets privacy.advisories`. `--json` says
  the same through `privacy_posture`, schema-covered (`schemas/doctor.schema.json`).
* **A CI job runs the shipped binary with no network.**
  [`.github/workflows/privacy.yml`](.github/workflows/privacy.yml) proves its egress trap actually traps
  (`tools/check_privacy.py selftest`), then runs ten commands — including a cold `index` — with egress
  refused and requires byte-identical output and zero connection attempts. The trap is a network
  namespace where the runner allows one, otherwise an `LD_PRELOAD` tripwire
  (`tools/privacy_netprobe.c`) that logs *and* refuses `connect`/`bind`/`sendto`/`getaddrinfo`; macOS
  gets `sandbox-exec`. ADR-002 §8.3 had left this as "a requirement with a named owner rather than as
  something measured"; that sentence is closed.
* **The static half, `V01`–`V07`** (`python3 tools/check_privacy.py check`, stdlib-only, no cargo): the
  default closure links nothing network-capable; the consent table is one table across Rust, the model
  and the JSON schema; every `privacy.*` key is user-scoped and defaults to false; no `std::net` or HTTP
  client in default-shipped Rust; no network import in a Python analysis module; the page still names
  every gate; and the release artefacts of the next bullet exist and are wired.
* **SBOM and licence report per release** (`python3 tools/gen_sbom.py`): a deterministic CycloneDX 1.5
  document of everything a default build links — pinned versions, `pkg:cargo/` URLs, the SHA-256 from
  `Cargo.lock`, declared licences — and the same inventory as obligations, with a verdict computed
  against `deny.toml` rather than restated. Both are committed under `sbom/`, re-checked on every PR, and
  attached to the GitHub release by [`.github/workflows/sbom.yml`](.github/workflows/sbom.yml).

`tests/test_privacy_contract.py` (13 tests) runs the checker, the offline proof and the generator from
the repo side, and fails if the page, the binary and the SBOM stop agreeing.

### One box, one comparison, and a session that never leaves the keyboard (`reco-mkc`, `reco-ifo`, `reco-mdb`, `reco-xke`)

Epic 5's criterion was a whole review session — notification to signed-off artefact — without leaving the
keyboard. What shipped is less a list of features than the removal of four reasons to reach for the mouse:

* **One search box.** `/` in the TUI and `fathom search` in the shell read the same corpus — episodes with
  their intent text, the paths they touched, the dependencies the manifests name, the findings raised — and
  score it with three rungs per token (word-start, anywhere, subsequence, so `fathstor` finds
  `fathom-store`), deterministic down to the tie-break and pinned by a golden file. The filter grammar
  (`author: since: until: risk> risk< kind: path: dep: finding:`) is printed in the box itself, and a token
  the build cannot honour is kept as text and *said* rather than dropped. Symbols are a stated gap, not a
  silent one; `reco-6sw` wires them when the extractor of `e3.1` exists.
* **`fathom cmp`, and `m` then `c`.** A comparison is the composed delta over the span between two endpoints,
  not two diffs: `HEAD~9..HEAD`, `main@1a2b3c..feat/x@9f8e7d`, or a name somebody saved with
  `fathom cmp --save since-the-refactor`. `c` opens it as a panel over the view you were in, and `fathom cmp`
  prints the same lines, so the shell and the script cannot disagree.
* **One widget, two shapes.** `,` is the palette and `/` is search; they share the field, the history, the
  scorer and — the part that mattered — the motion. Every list, both boxes and the comparison panel now move
  with one implementation, so `M` means "middle of what is on screen" in all of them.
* **A refused keybinding says where to fix it.** All four keymap conflicts (unknown mode, unknown action, an
  action in a mode that cannot take it, a shadowed reserved key) now print `file:line` — at startup, in
  `--plain` notices, and from `fathom tui conflicts`.

Measured by two arc tests that press the session in order: `crates/fathom-tui/tests/session_arc.rs` and
`crates/fathom-cli/tests/session_arc.rs`. `h`/`l` drill out and in in list and body modes while they were there.

### The exits out of the reader: `fathom open`, `fathom difftool`, and links that only appear when asked for (`reco-dmw`)

Fathom is a reader, and a refusal to be an editor is only worth having if going *out* is good. Two verbs and
one display decision, all of them planned before anything was run and printed with the reason:

* **`fathom open [EPISODE]`** jumps to one file of one episode at its line in `$EDITOR` — `HEAD`'s episode
  with no argument, the same handle grammar `show`/`analyze`/`why` accept. The line is the file's first `@@`
  header, so `]` in the diff pane and `fathom open` agree by construction. With no `--path` the choice is the
  biggest change on a surface a review reads, ranked by the same `risk::classify` chain `fathom why` ranks
  with, and the surface is printed beside the path (`dependency`, `generated`, `vendored` are discounted by
  name as well as by the stored taxonomy, which today says `unknown` for everything). `--list` shows every jump
  with its line; `--print` prints the plan and runs nothing; the editor's own exit code is the verb's, because
  `vim :cq` is a reviewer who cancelled.
* **`fathom difftool [EPISODE]`** writes a file's two sides out of the object store into
  `$TMPDIR/fathom-difftool-…/` — real extension kept, because difftastic chooses its grammar by suffix — and
  asks the tool in the shape it documents: `difft`, `meld`, `kdiff3`, `nvim -d`, `code --diff`, WinMerge with
  `/e`, or any program named with its own arguments (`--tool "mydiff --wait"`), with the two files last.
* **No editor and no difftool are ordinary paths.** The refusal names what was looked for, the keys that change
  it (`$FATHOM_EDITOR`, `$VISUAL`, `$EDITOR`, `$FATHOM_DIFFTOOL`, `[ui] editor`, `[diff] difftool`), and where
  the staged pair is, so a machine with nothing installed still gets the diff. A *requested* tool that is
  missing is announced as a substitution rather than silently swapped.
* **A clone cannot name a program this process runs.** `[ui] editor` and `[diff] difftool` are read from the
  user config and the environment only; a repository's `fathom.toml` that tries is quoted back with the reason
  (ADR-001 §4.6 — an editor *executes*, which is a stronger reason than the theme rule already rests on).
* **OSC-8 links for paths and commits, on two conditions.** The terminal has to advertise (`TERM_PROGRAM` with
  per-terminal floors — iTerm2 3.0, VS Code 1.57, Konsole 22.12 — `VTE_VERSION` ≥ 0.36 parsed as a version, or
  a vendor marker), and the stream has to be a terminal: `fathom show | fzf` sits inside a Ghostty that
  advertises and would otherwise receive introducers as payload. tmux below 3.3 and GNU screen veto the route
  and say which one it was. `FATHOM_HYPERLINKS=always|never|auto` overrules both, `fathom doctor --json` prints
  the verdict as `emit`/`advertised`/`why`, and the visible text is byte-identical either way — which is what
  makes a link safe to add to a line a script is parsing.

The plans live in `fathom-core` (`handoff`, `links`) and read no environment, open no file and spawn nothing,
so the whole editor matrix — vim, neovim, helix, code, zed, sublime, emacs, idea, nano across Linux, macOS and
Windows — is one `cargo test` on one machine rather than three CI runners, and the diff view's `E` produces the
same `Command` the verb does. Documented in [the exits out of the reader](docs/concepts/handoff.md).

### The second terminal got its own pane: `fathom ambient` (`reco-8bx`)

The glance view existed inside the shell; what was missing was the thing to leave open in the pane beside the
agent. `fathom ambient` is that pane — latest episode and its risk, how much is waiting, whether the index is
current, and the one reason to look now — and the four things the ticket demanded are four numbers rather than
four adjectives.

* **It fits the sizes it promises.** 80×12 is the box, 40×3 is the floor (ADR-001 §4.1's tmux claim), and the
  ladder drops the keys line, then the notes, then the gap and the index line in that order. Below the floor it
  prints `AMBIENT REFUSED — TOO SMALL`, the size it needs and the verb that works at any size, and exits **0**:
  a pane clipped to 30 columns would be a pane saying something false.
* **It costs what a quiet repository costs.** The loop blocks for one poll (400 ms), starts each poll with a
  `stat` of the store, the watcher's heartbeat and `HEAD`, opens the index only when one of those moved, and
  draws **only if the text it would print changed** — ages are bucketed (`now`, `<15s`, `15s`, `1m`) so a silent
  repository does not redraw to repeat itself. `fathom ambient --self-check` prints polls, redraws and the
  run's own measured CPU share and exits **11** over 1 % of one core, so the budget is gated rather than
  claimed; the gated run measures **0.24 %** of one core with the repository quiet.
* **A new episode is on the pane inside the second.** The end-to-end test times it: commit returned → pane
  redrawn with the new episode, under the ticket's **1 s** at the shipping 400 ms poll (0.42 s measured on the
  machine this was written on), and `<Enter>` hands the terminal to the full
  view with that episode selected — the session's own resume door (`tui.view`, `timeline.selection`), so a
  `--root` other than the working directory keeps its identity across the keypress.
* **"Caught up" is one sentence in one place.** The queue line only counts (`0 open · 12 dealt with`), the index
  line only reports epoch and age, and the attention line is the only place allowed to say `caught up` — and it
  says it only when nothing outranks it, where what outranks it is, in order: the index cannot be read, `HEAD`
  is not in the index, nothing has indexed for ten minutes, a high-risk episode is open, work is unranked, work
  is waiting. A stale pane is a loud pane; a resting pane is dim all over, faces included, and the test asserts
  no span on it is printed loud.
* **And it is pipe-shaped like everything else** (ADR-006 §6.7): `--once` renders once as text, `--json` streams
  one `fathom.cli.ambient/1` object per change, `--plain` means what it means everywhere, and `--no-index` says
  on stderr that it is reading whatever the last run left. With no watcher alive it indexes new commits itself,
  because a glance pane that is quietly behind is worse than none.

Asserted by `crates/fathom-tui/tests/ambient.rs` (17 tests: the two sizes, the refusal, the ladder, the ranking
of reasons, the caught-up faces, the redraw-only-on-change loop, `q`/`<Enter>`/`r`/`p`/`?`) and
`crates/fathom-cli/tests/ambient_verb.rs` (7 end-to-end against the shipped binary, including the CPU budget and
the one-second latency).

### `e4.9` follow-up — the flag arm fired for the first time, τ moved 70 → 40, and a bot's compiled lock stopped earning the CI claim (`reco-6co`)

`reco-dyt` left two of Guardrail 3's nine gates failing on the same sentence: the change had to be to *what gets
flagged*, not to the bar. This is that change, measured before and after on the same pinned corpus.

* **The τ arm of `F(τ,W)` was inert and now is not.** `risk.tau` shipped at `e2.10`'s **70**; `features-1`'s
  ceiling across 540 episodes of real OSS history is **63**, so at the published default six episodes in 540
  were flagged and every one of them by a high-severity finding, and all 24 mature reworked episodes sat below
  the line (miss rate 1.000). ADR-001 §5.4 always said τ was to be *confirmed against three real
  repositories*, so the harness now prints the derivation: a new `aggregate.tau_sweep` scores the grid
  70…25 and reports, per threshold, the flag population, its share, how much of it a human has read,
  `G3` and the miss rate. τ is **40** — the highest of the three repositories' top-decile cuts (33/40/40)
  and the lowest threshold whose entire flag population carries a hand-authored disposition. Five copies of
  that one number moved in the same commit: `Weights::defaults()`, `fathom.toml`, the key spec (and the
  generated `schemas/fathom.schema.json`), the reference store's `DEFAULT_CONFIG`, §5.4 and
  `thresholds.json`; the harness already refuses to run when the corpus's τ disagrees with the shipped one.
* **A locked or generated file under a CI path is ranked as what it is.** `OPERATOR_FIRST` is a *directory*
  table, so `.github/workflows/` matched `issue-triage.lock.yml` — the `gh-aw` agentic-workflow lock a
  compiler re-emitted beside a Dependabot `go_modules` bump — and charged it the 46-point CI weight whose
  printed sentence is "an operator must act on it". `risk::classify` consults a `MACHINE_WRITTEN` rung
  first now (name-rule, plus the taxonomy's `generated`/`lockfile` answers): a lock is a lock wherever it
  sits. The named episode — three Dependabot bumps, +12 −12 over seven files — went **33 → 32** and fell out
  of `cli`'s top decile at 33. Deliberately *not* `Generated`: `generated_edit` measured `G3` 1.000 on 4
  flags, all of them a release bump's regenerated README, so classifying a resolver's lock as a hand-edit of
  a generated file would have bought three more bot bumps and a finding that was wrong every time.
* **A resolver bot's bump makes the supply-chain claim and not the operator claim.** `e4.1`'s registry can
  name *which* program wrote an episode, and `dependabot`/`renovate`/`github-actions` (`RESOLVER_AGENTS`) are
  bumping services rather than authoring agents. For their episodes `Surface::resolver_capped` caps
  migration/CI/IaC/container at `dependency`: the pins moved, nobody has to deploy or rotate anything, and the
  printed reason names who wrote it and which claim was withdrawn (`ep-2447c9b8…-2` 43 → 39). An authored
  agent turn is untouched — the `authorship` axis still charges a bot's bump, because that claim was never
  the false one.
* **What it bought, and what it cost, in the report's own numbers.** Flag population at τ 6 → **49** of 540
  (9.1 %, all labelled); `G3` at τ **0.633** against §5.4's 0.30 (was 0.333 on 6 flags, which was not a rate
  over anything a reviewer was shown); corpus-cut `G3` 0.697 → **0.672**; false positives blamed on the
  ranking 26 → **23**; miss rate at τ 1.000 → **0.792**. And a cost said out loud: dimming a class dims every
  member of it, so the miss rate at the decile cut went **0.708 → 0.750**. Five of nine gates fail, and no
  bar, floor or bound in `thresholds.json` moved — the only §5.4 number that moved is the one §5.4 says to
  confirm.
* **What it did not buy.** `flask`'s `update dev dependencies` episodes — a maintenance refresh that
  re-emits five CI workflow files from an out-of-tree template, 11 of its 20 flags — are hand-committed edits
  to real workflow files, and a path rule cannot see that. Naming them is content work (`e2.6`'s dependency
  delta saying *which* pin moved, a catalogue that tells a template re-emission from a permissions edit) and
  it is the standing distance to 0.30, not something a threshold edit gets to decide. Three dispositions the
  re-score demoted below the cut stay in `truth.json` and are printed as `labels_unflagged`, excluded from
  every denominator: a reading of a diff is not retracted because a score moved.

`cargo run -p fathom-analyze --features sqlite-bundled --example g3_calibration -- --check` reproduces the
re-blessed report byte for byte with all nine gates as declared; `tests/test_g3_calibration_contract.py` now
also refuses a τ where the report has unlabelled flags, and allows a label that no cut flags. New tests name
the shapes: `risk_spine.rs` ranks the compiled lock under the CI directory and asserts the resolver cap is a
demotion, and `fathom-ingest` separates the pin movers from the writing bots.

### `e4.9` follow-up — the Guardrail 3 corpus got three times the window, and the miss-rate gate stopped being an anecdote (`reco-dyt`)

`e4.9` measured Guardrail 3 on three pinned repositories and one of its own nine gates said the honest thing:
`miss-rate-denominator-is-big-enough` **failed**, because across 180 episodes only 7 were both mature and later
reworked, and a `miss_rate <= 0.20` gate on 7 samples cannot tell 0.20 from 0.60. That was a finding about the
corpus, and the fix it named was more window rather than a relaxed bar. This pass pays it.

* **Each pin now carries 180 episodes instead of 60**, fetched by `tools/fetch_g3_corpus.py` at the current
  tips — `cli` and `flask` had not moved since `e4.9`, `ruff` moved 11 commits forward (`0a977a8d18d7` →
  `3e3f22dcc47a`). The fetcher's depth became `window * 4 + 20` to match `WALK_COMMITS_PER_EPISODE`, because a
  clone that runs out does not fail: it quietly measures fewer episodes than the pin claims. Corpus: 540
  episodes, 525 mature, 66 flagged, and **24 mature reworked** — the denominator went 7 → 24 and its gate went
  `fail` → **pass** without a single bar in `thresholds.json` moving.
* **All 66 flagged episodes have a hand-authored disposition.** 44 new rows in `corpus/*/truth.json`, each
  written after reading that episode's diff (`git show`, every member) against the receipt's findings — a
  settling decision, a reason code from `e4.6`'s closed list for a dismissal, a note, an evidence line naming
  a file, line or finding text, and `blame` where the false alarm was one part of the flag. Nothing was
  inferred, templated or generated, and nothing in the tree writes a `truth.json`.
* **What the bigger corpus says.** `G3` is now measurable at §5.4's published τ and it fails narrowly —
  **0.333** on 6 labelled flags, where 60-episode pins gave a denominator of 3 and had to refuse — so
  `g3-alpha-at-published-tau` moved `not-measurable` → `fail`, the only `expected` besides the denominator
  gate to move, and neither move made a gate more comfortable. The corpus cut is **0.697** (46 of 66 labelled
  flags cost without value, 26 of the 46 blamed on the ranking rather than a detector), the miss rate is
  **1.000** at the published τ (nothing in 540 episodes scored above 63) and **0.708** at each repository's
  own top-decile cut, now a rate over 24 samples instead of an anecdote over 7.
* **Two limits the wider window made visible, both written down rather than smoothed over:** three
  default-enabled detectors (`lint_escape` 0.667/9, `untested_new_logic` 0.500/8, `test_weakening` 0.385/13)
  are over α with enough answerable flags to raise `e0.12`'s "ship it disabled" question, and
  `defaults.breaches` still prints 0 because the harness's pooling step never fills a pooled per-detector
  `g3` — so that gate cannot fail today, which is stated in `thresholds.json`, in the corpus README and in
  [the written answer](docs/concepts/false-positive-calibration.md) rather than left for the next reader to
  find. And `untested_new_logic` counted ty's mdtest resources as "no test file at all" twice, which is a
  catalogue blind spot with two labelled examples now in the corpus.

`cargo run -p fathom-analyze --features sqlite-bundled --example g3_calibration -- --check` reproduces the
re-blessed `benchmarks/reports/e4-3a-g3-calibration.json` byte for byte with all nine gates as declared, and
`python3 -m pytest -q tests/test_g3_calibration_contract.py` holds the corpus, the labels and the documents to
it — including two new guards: a pin's window may not drop below 180 episodes, and a window must deliver the
episodes it claims.

### `m1` — the beta gate, assessed by running it, and the four things running it found (`reco-69u`)

The beta milestone asks for a thing that only a real pass can answer: can a reviewer work the queue, read the
dependencies and non-code surfaces, and export a sign-off that lands on a PR. The assessment is
[`.plan/beta-gate-reco-69u.md`](.plan/beta-gate-reco-69u.md) — criterion by criterion, with the command that
re-proves each line, the external review that stands as the evidence
([`.plan/evidence/reco-69u/flask-pr6168-review.md`](.plan/evidence/reco-69u/flask-pr6168-review.md), a real
`pallets/flask` PR indexed, reviewed, signed and exported by the shipped binary), and the Guardrail 3
criterion recorded as **not met**, with its bars unmoved and the shortfall carried by `reco-dyt` (the corpus
needs five times the window before a 0.20 miss-rate gate can discriminate at n = 7) and `reco-6co` (the flag
arm is inert at τ 70 while routine bot churn leads every decile). `g3_calibration --check` reproduces the
blessed report byte for byte, before this commit and after it.

What dogfooding turned up, three of four fixed here:

- **One episode, two risk numbers.** `fathom why` and `fathom export` read a single episode through
  `detectors::collect::Reader::one`, and that read never folded in the index's per-path rollup — which is
  where a file's `add`/`del` come from. Every axis that reads per-path churn was therefore measured against
  zeros: on a real `pallets/flask` episode the verb printed **52** where the queue, the rail and the Guardrail 3
  harness all said **63**, `test_ratio` claimed "neither logic nor tests arrived" about a diff that deleted
  22 test lines, and `noise` said "nothing in this change is generated, locked, vendored or binary" about an
  episode whose `uv.lock` regeneration is 429 of its 498 churn lines. The artefact is the same read, and
  `untested.rs` gates its flagship claim on `file.add`, so the door that travels to a PR could not see an
  untested addition at all: this repository's own newest episode went from **6 findings** in the artefact to
  **10**, the two new mediums being `untested_new_logic` on `tools/check_docs_*.py`. `one()` now applies the
  same rollup `collect()` does, and
  `cli_why.rs::an_episode_read_alone_is_measured_the_way_a_page_read_measures_it` fails without it.
- **`fathom export --out review.md` wrote `review.md.md`.** The destination checked the extension of the
  name *after* appending the format's, so the documented "a stem that already ends in the extension is used
  as typed" branch could never fire on the stem. A CI step that writes `--out "$name.md"` and posts
  `"$name.md"` posts a file that does not exist. Fixed, the report now names the file it wrote, and all three
  formats are pinned by a test in `export_verb.rs`.
- **A contract test deleted the repository's own `.fathom/`** — index *and* `.fathom/reviews/ledger.md`, the
  reviewer's sign-offs — because `test_gitignore_keeps_the_ledger_visible` probed the ignore rules inside the
  real working tree and then `rmtree`'d the directory. ADR-004 §12 says the store is disposable and the
  ledger is not; the suite was making both disposable, and it ate a ledger mid-pass. It now creates only
  what is missing and removes exactly those paths.
- **A half-finished corpus clone stopped the Guardrail 3 fetcher.** `tools/fetch_g3_corpus.py --check` asked
  an interrupted clone for its `HEAD` and died with a git traceback instead of saying `NOT AT PIN`; the probe
  is now tolerant and a plain run re-clones. Separately filed: `reco-b7o`, for telling a shallow clone to run
  `git fsck` when the objects are behind a graft and the remedy is a deeper fetch.

This repository's `.fathom/reviews/ledger.md` is committed, so `fathom review status` can be satisfied
without a `git clean` undoing it, and `docs/concepts/risk-scoring.md` §8 now names the equality that §2.1
restored: one episode read alone is measured the way a page read measures it.

### `e7.5` — the documentation site: quickstart, the six concept pages, harness recipes, the detector catalogue, and generators that cannot lie (`reco-60m`)

The thesis promised "a five-minute quickstart, a concept glossary, a cookbook of harness recipes, and a
troubleshooting page", and had two of them. Now it is a site with a front door and an entry point for every
kind of reader, and the parts that could rot are generated from the program instead of written down.

- **`docs/index.md`** is the front door: who it is for, what to read first, the six nouns in one line each,
  the task index, the reading order, and the four things no page here will promise. Every page under `docs/`
  is reachable from it, which `tools/check_docs_site.py` enforces rather than hopes.
- **`docs/quickstart.md`** is five minutes against the bundled miniature (`fathom --demo`), with every
  printed line taken from the shipped binary and every `--json` claim checked against what the binary
  actually answers. It ends by making you name what an episode is — including the four things an episode is
  **not** — and then points at the concept pages.
- **The six concept pages**: [`episode`](docs/concepts/episode.md), [`delta`](docs/concepts/delta.md),
  [`risk score`](docs/concepts/risk-score.md), [`review ledger`](docs/concepts/review-ledger.md),
  [`ratchet`](docs/concepts/ratchet.md), [`ambient mode`](docs/concepts/ambient-mode.md). Each states the
  claim, what it is *not*, where the numbers come from, and what would be lost by loosening the definition —
  with the worked example of a risk score going from 15 to 82 by adding one stub, and the ledger's four
  gates on a sign-off replay.
- **`docs/recipes/`**, six harness recipes: review a session, run beside the agent, connect a harness
  (exact `fathom hooks ingest` payload, what the fields mean, what is deliberately unread), gate a pipeline,
  export for a pull request, and ask an agent about this repository. Each one runs; the ones that need a
  future build say so and name the ticket, and the CI build gate is a `fathom check --stdin` invocation a
  reader can copy.
- **`docs/detectors/index.md`**, the catalogue: every built-in detector with what it detects, why it is a
  problem specifically in code an agent wrote, a worked example against a real repository, and how to tune
  or silence it. The summary table, the knob table, the disabled-detector list and every worked example are
  **generated** — each example is the detector run on the fixture it must fire on, so a detector that stops
  firing cannot quietly keep its documentation.
- **`docs/reference/cli.md`** (generated from `fathom --help`, verb by verb and sub-verb by sub-verb, with
  the exit-code contract and its `ExitCode` source), **`docs/reference/cheatsheet.md`** (generated from the
  keymap: the three sequences, the keys a harness may bind, every view/mode/action, the reserved chords and
  the ones Fathom leaves alone), **`docs/reference/features.md`** (every declared build feature and consent
  gate, with the claim each one makes and the test that pays for it, including the three net consent keys
  and why `sqlite-bundled` may not default to off), and **`docs/troubleshooting.md`** (the exit codes you
  will meet, the three ways an index can lie, hooks delivery, terminal capabilities, and the `doctor`
  message you cannot act on).
- `tools/gen_docs.py` writes those four pages between explicit markers — `check` exits 1 on drift, `write`
  refuses to overwrite a page that lost a marker — and `tools/check_docs_site.py` fails an orphan page, a
  dangling anchor, a `TODO`/`TBD`/placeholder left in prose, or a shipped `enum Verb` case, declared build
  feature, TUI view or catalogued detector with no documentation entry. `tools/check_docs_commands.py` hands
  every command a page tells the reader to type back to clap with `--help` appended, so a recipe that types a
  subcommand nobody shipped or a flag nobody implemented fails the build instead of the reader. `contracts.yml`
  runs both; `ci.yml` re-derives the CLI page against the binary it just built and re-runs the site checks with
  that binary present, which is where those two clap-backed checks actually have a source. `tests/test_docs_site_contract.py`
  asserts the same rules so a local `pytest` catches them first.
- Also fixed as found: ADR-006's generated keymap table linked every action to an anchor its own headings
  never produced (50 dead links, repaired by removing the link from the generator and regenerating the
  document), four cross-page anchors in `docs/concepts/` pointed at renumbered headings, `docs/reference/`-
  relative links in the new pages, and `docs/adr/0005`'s `[episode]` link to a glossary heading that does
  not exist.

### `e5.7` — the review artefact: one record, three renderings, and the same bytes twice (`reco-1tn`)

- **`fathom export` writes the file a pull request or an audit keeps.** `--episodes` takes an episode id,
  any revspec, `A..B` or `A..=B` (a bare handle means *the episode that handle belongs to*, which is what the
  single-commit verbs already mean and what makes the default `HEAD` a one-episode artefact instead of the
  whole repository); `--format md|html|json` chooses the rendering, `--out` the stem, `--stdout` puts the
  artefact on stdout with the report on stderr, and `--strict` exits `7` while the checklist still has an open
  item — and still writes the artefact, because the operator has to read something to close the gate.
  A refusal writes nothing: no index is `12` with `fathom index` as the remedy, a rev that names nothing is
  `1`, and a fourth format is `2`, because the three formats are the façade's list and not a flag.
- **One structure, three renderings, so the three surfaces cannot disagree.** `export::build` composes a
  single `Record` from the delta documents, the ledger and the detector report — summary card, `e4.7`'s
  narrative verbatim, the range's dependency delta, the architecture delta, findings with their evidence,
  the operator-action checklist, the sign-off block, the commit links, and the gap list — and markdown, HTML
  and JSON are layouts over it. The JSON artefact *is* the record (`fathom-review-record/1`), so the
  cross-format test can assert that a finding in the JSON is in the markdown and the HTML too: a fact hidden
  in one format because it is awkward to lay out is the second reality ADR-001 §7.10 exists to forbid.
- **Byte-identical is structural, not hoped for.** The builder reads no clock, no locale and no terminal; every
  instant it prints is one the repository or the ledger already held. Episodes sort by first member commit,
  commits by instant, findings by severity then kind, sign-offs by stamp; maps are `BTreeMap`s; the `--out`
  and `--root` the operator typed are stripped from the echoed command line, because an artefact whose text
  depends on where it was saved is not the same artefact twice. The test rebuilds a record after a wall-clock
  second, and re-renders one whose inputs arrived in the opposite order.
- **Committed text is treated as a hostile input in two different ways, because markdown and HTML are two
  different dangers.** A commit subject carrying `<script>alert("x")</script>`, a `javascript:` link, a pipe
  and a backtick is the fixture. HTML escapes everything unconditionally and ships as one file: inline CSS,
  no subresource of any kind, a CSP of `default-src 'none'; script-src 'none'`, `@page` margins,
  break-inside rules, repeated table headers, and `print-color-adjust: exact` because colour carries meaning
  here and every colour-coded mark also says what it means in words. Markdown has an inert span and a live
  everything-else, so fields are escaped on the way in (a code span widens its own fence until the text cannot
  close it early), and the narrative — which the self-check requires be the composer's own bytes — goes
  through one span-aware pass that neutralises a `<` that opens a tag and a `[` that opens a dangerous scheme,
  *outside* code spans only, because escaping inside one would print `&lt;` where the reader came to read `<`.
  The store keeps the subject verbatim: escaping is a rendering decision, and losing what a commit said is not
  a safety improvement.
- **The sign-off block prints the claim and the claim's date, not a green tick.** Each entry carries reviewer
  and whether they were human or an agent, the ISO-8601 instant from the ledger, the scope it covered
  (commits, files, churn, paths, the members it named and how many it did not print), the dismiss/snooze code
  with the shipped vocabulary's words beside it, the digests it was taken against and its own tamper digest,
  and whether the content still matches — `unchanged`, `changed` with the ledger's reason, or nothing at all
  when the comparison could not be made, because `None` is not `unchanged`. A dismissal is not counted as
  reviewed: the verdict says `0 of 2 … · 1 undecided · 1 refused or deferred`.
- **The artefact prints what it cannot answer, and who owes it.** Each of the seven sections reports its
  `measured / partial / unavailable` counts across the range; every gap prints once (not once per episode that
  hit it) with its owner and, where there is one, the command to run; a dependency delta that could not be
  computed prints *Not computed for this range* rather than a zero. `Record::verify` runs at build time and
  its verdict ships in the file, and the tamper tests plant each fault — a finding for an episode outside the
  scope, a summary that disagrees with its episodes, a sign-off for nothing, a host path in `provenance.store`
  — and require the check to name it.
- Asserted by `crates/fathom-analyze/tests/export.rs` (25: byte-identity, escaping in both markup languages,
  anchor resolution, the sign-off and checklist content, the self-check failing when it should) and
  `crates/fathom-cli/tests/export_verb.rs` (11 end to end in real repositories, including two exports of one
  range compared byte for byte in all three formats), with the `--json` payload validated against
  [`schemas/export.schema.json`](schemas/export.schema.json) — which now describes a written artefact instead
  of a skeleton — and documented in [docs/concepts/export-artefact.md](docs/concepts/export-artefact.md).

### `e4.7` — the episode narrative: prose with a citation on every claim, and no model in the path (`reco-2rx`)

- **`fathom narrative <episode>` composes the delta document into paragraphs a human can skim.** Text,
  `--markdown` (the artefact body `e5.7` will carry, via `export::narrative_markdown`) and `--json`
  (`fathom-narrative/1`) are the same call, byte for byte, and the composition is a pure function of the
  document — no store, no clock, no config, so an identical episode prints identical bytes and the golden
  file `crates/fathom-analyze/tests/golden/narrative/measured.md` is a real check.
- **The three defects spike `s5` measured at 2.23/5 readability are fixed in the template that produced
  them.** Every claim is priced by one published function and paragraphs are ordered by price, with the
  expensive ones lifted into **Read this first** (document order was the bug, not the prose); a figure is
  claimed by exactly one sentence, so `lines` and `churn` are no longer printed twice; and every blind spot
  became **You cannot tell …** with the owning plan key and the remedy, because `path-rule discount only —
  not answered here, it is e2.2's` is an internal sentence wearing an external hat. Two demotions keep the
  ranking honest: an unanswered figure prices at 10, and a figure starting with a zero (`+0 −0`) prices at
  12, so a quiet episode cannot be dressed up as a loud one.
- **"Every claim cites its evidence" is checked as content, not as shape.** Six self-audit checks travel with
  the payload: every citation id resolves and none sits unused, every `#/…` pointer resolves inside the
  document the narrative was composed from, every locator is a place a reader can go, and **every number a
  sentence prints appears in the text of the citations that sentence carries** (digit grouping folded, so
  `9 356` is fair and an invented figure is not). The tamper test runs all four tamperings — invented number,
  dangling pointer, unattached sentence, orphan citation — because an audit that has never been seen to fail
  is decoration.
- **`reco-nv0`'s decision is a queryable fact instead of a paragraph.** `narrative::llm::SHIPPED` is `false`,
  the five gates that would reopen the polish path are in `llm::GATES` with their current state, and
  `llm::find_wired_paths` walks this crate's own source in a test and fails the build on a `net-llm` consumer
  or an HTTP call — the only way "no model in the default path" (ADR-001 §8 non-goal 6) stays true without
  someone re-reading the docs each quarter. `fathom narrative --attribution` prints the answer, the gates and
  the watermark (`fathom-template/1 … verified: 29 claims, 31 citations, 6 checks`) and needs no index.
- Citation ids are `c1..cN`, not `e1..`: this repository's own docs are full of plan keys spelled `e4.7`, and
  `tools/check_docs.py` reads every `e\d+` as a plan reference, so the collision was a failing build rather
  than a hypothesis. Asserted by `crates/fathom-analyze/tests/narrative.rs` (14) and
  `crates/fathom-cli/tests/cli_narrative.rs` (7 end to end); documented in
  [docs/concepts/episode-narrative.md](docs/concepts/episode-narrative.md).

### `e4.8` — plan versus execution: the six contradictions, with both sides linked (`reco-cpr`)

- **`fathom drift` reads a session, not an episode.** Drift is a session question: an item deferred to the
  next turn is not drift, a dependency added two turns after the plan that named it is not drift, and a
  "refactor the parser" that touched only the parser's tests is. The report compares the recorded intent
  (`e4.2`'s intent column, through the JSON round trip the CLI tests exercise against the real store) against
  what the episodes touched, in the order the harness recorded them.
- **Six kinds, each one rule and one threshold**: `plan-unimplemented` (an open item with nothing resembling
  it inside the window), `test-promised-not-written`, `refactor-claimed-not-performed` (the named target in no
  touched path and no recorded move), `out-of-scope-edit` (against `e4.2`'s declared globs, unless the plan
  itself named the path), `unplanned-dependency` (a manifest nobody announced) and `contradicts-signoff` —
  the last one is `e4.6`'s ledger as the durable side of an argument: a plan item that re-opens work a human
  signed off as done, citing the entry.
- **Every finding is a pair, and the pair is printed.** `claim` (the plan text, quoted, with the address that
  holds it) and `evidence` (the path, the glob, the ledger row) on adjacent lines, each with a `fathom` verb a
  reviewer can run; `--json` carries `fathom-drift/1` with the same two sides, and `Report::to_insight()`
  hands them to the delta document through `insight::Finding`, so the narrative and the card cite drift
  findings through the door every other finding already uses.
- **Confidence is the intent rung's, not a guess.** Each rule brings a base (0.92 for the arithmetic ones,
  0.58 for "nothing here resembles that") and the rung prices it, so the same finding reads 0.92 on a hook
  prompt and 0.83 on an `Intent:` trailer, and the finding prints which rung decided. Never 1.0, floored at
  0.30 so a trailer-only repository stays reportable at a price that admits it.
- **Bounded, and it says so.** A finite look-ahead window (default 4), a per-kind cap and a report cap that
  set `truncated` and end the verdict with *the caps cut some*; `cross_session` off by default, because
  Monday's run is a different conversation; a switched-off kind is printed as off rather than silently
  missing. A dismissal is stored against a key built from the rule and the plan text — never the episode id,
  which an amend changes — so a dismissed contradiction is counted in `suppressed` and does not come back.
- **What cannot be judged is named, not judged.** No intent, an intent with no declared scope, globs that do
  not compile, a test surface the index could not read: each is `unchecked` with its owner (`e4.2`, `e2.9`),
  and "I cannot tell whether a test was touched" never becomes "no test was written". A test walks the
  module's own source and fails it on `rusqlite`, `std::fs` or a clock read: the analysis half is a pure
  function (ADR-002 §4.5), which is what makes all 23 fixtures in `crates/fathom-analyze/tests/drift.rs`
  possible at all. `crates/fathom-cli/tests/cli_drift.rs` (7) drives the binary, including carrying a
  dismissal from one run's output into the next run's flag; documented in
  [docs/concepts/plan-drift.md](docs/concepts/plan-drift.md).

### `e4.9` — Guardrail 3 got a corpus, and the false-positive rate was measured (`reco-mkl.1`)

- **Three real repositories, pinned by commit, and the labels to score them with.** `cli/cli`,
  `pallets/flask` and `astral-sh/ruff`, 60 newest episodes each (180 total, 165 mature), pinned in
  `crates/fathom-analyze/calibration/g3/corpus/*/pin.json` and re-fetched by
  `python3 tools/fetch_g3_corpus.py`, which fetches a pinned oid and refuses a remote that hands back
  anything else. Every episode the corpus flagged got a hand-authored disposition — 22 flags, 22 labels,
  each with the file or line it was decided on, the actor, and the `e4.6` reason code — and nothing in the
  build is allowed to write a truth file (`tests/test_g3_calibration_contract.py` reads every source line
  that names `truth.json` and refuses a write primitive in it, because a harness that authors its own truth
  measures the harness).
- **`G3 = 0.682` against a bar of `0.30`, and a miss rate of `1.000`.** At §5.4's published `τ = 70`, three
  episodes in 180 reached the threshold, so `G3` is **not measurable** there (a denominator of 3 under a
  floor of 5 prints `not-measurable` rather than the `pass` that 1-of-3 would have given it) while every
  one of the 7 mature episodes that later needed rework sat *below* it — the two halves of §5.4 failing in
  opposite directions, which is exactly why §5.4 publishes both. Read at each repository's own top-decile
  cut, 22 flags go in front of a reviewer and 15 of them are cost without value: 11 `low-stakes` (Dependabot
  and release-bot bumps, maintenance `update dev dependencies` episodes that re-emit CI templates and a
  1 000-line lockfile), 3 `false-positive` (three regenerated gh-aw workflow locks charged as hand edits, a
  test rewrite charged as a test deletion, a fuzzer's own bug report charged as leftover `print`s), and 1
  `intentional` (dropping Python 3.10, with the `# noqa: F821` the subject line announced). Seven of the
  fifteen were blamed on the ranking rather than on any detector.
- **Per-detector attribution, and no shipped default flipped.** Dispositions carry `blame` — `score`, or the
  ids of the detectors whose claims did not hold — and an unattributed false positive charges every detector
  that spoke, which is §5.4's rule and stays visible in the receipts rather than being hidden by a tidier
  number. `lint_escape` 0.800 on 5 flags, `shotgun_edit` (shipped disabled) 0.500 on 4, `generated_edit`
  1.000 on 2, and `churn_loop`, `rework_loop`, `copy_paste_growth`, `scope_creep` and `secret_string` raised
  nothing at all — no evidence, not a clean bill of health. Changing a default for everybody needs 8 labelled
  flags; nothing reached it, so the four `e4.3` shipped disabled stay disabled and `defaults.breaches` reads
  `0` meaning *nothing is disqualified on this evidence*, never *everything passed* (ADR-001 §7.7).
- **`e4.3`'s and `m1`'s deferred criteria now point at a command that exists.**
  `cargo run -p fathom-analyze --features sqlite-bundled --example g3_calibration -- --check` re-measures the
  blessed `benchmarks/reports/e4-3a-g3-calibration.json` byte for byte — score from
  `risk::collect::vector` + `score` under `Weights::defaults()`, findings from `detectors::scan` with all
  twelve detectors enabled, vocabulary from `queue::REASONS`, τ checked against the shipped `risk.tau` — and
  every gate must come out as its declared `expected`, in either direction. The written answer is
  [docs/concepts/false-positive-calibration.md](docs/concepts/false-positive-calibration.md); ADR-001 §5.4
  gained the command and kept its numbers, because the alternative to a measured failure is an unmeasured
  pass.

### `e4.5` — the attention queue: the ranked worklist, its four acts, and a ledger of both (`reco-wsj`)

- **`queue-1`: risk × staleness × blast, and the arithmetic is printed.** `fathom_analyze::queue`
  orders the worklist by `risk × staleness × blast` — staleness `+5%` per unread day capped at `+50%`,
  blast `+3%` per direct dependent capped at `+30%`, severity bands at `70`/`35` — ordering on the exact
  product and printing the rounded one, with a tie-break chain that ends at the episode id so two runs
  over the same index give the same list. `w` prints the ordering the way `fathom why` prints a score:
  formula name, version, every factor and what it was measured from (`queue-1 90 = risk 60 (spine-1) ×
  staleness 1.50 (10d unread) × blast 1.00 (not measured · e3.2 owns it · displayed by fathom-tui)`).
  A stale medium now sits above a fresh high, which is the whole point and is a test rather than a claim.
- **Blast radius is `1.00`, and the queue says so instead of pretending.** No module graph exists yet, so
  the third factor is shipped idle and attributed out loud — `blast unmeasured (e3.2) · holding this
  list at 1.00` above the rows, `e3.2` named in every explain sentence. It is never guessed from path
  prefixes, and an episode nothing has scored stays `?` at the bottom of the list rather than becoming a `0`.
- **Four acts — `x`, `f`, `-`, `z` — none of them taken until it is written.** The ledger write happens
  before the shell changes, so a refusal leaves the row exactly where it was and prints `NOT written:
  <why>`; a run with no ledger attached is a supported configuration and says `in memory only: no ledger
  attached to this run` instead of implying durability. A flag with no note is refused, a dismissal with
  no reason is refused, a snooze with no condition is refused. `fathom-store` migration 6 adds
  `triage_event`: one append-only row per act — who, when, why, the condition, and the risk at the time —
  kept apart from `review_state` so a second keystroke cannot erase the first one's record, and with no
  foreign key so a history rewrite cannot delete an act.
- **A dismiss vocabulary closed to this build, extensible by the repository.** Eight shipped codes
  (`not-my-area`, `already-known`, `intentional`, `false-positive`, `low-stakes`, `superseded`,
  `will-revert`, `other`), `other` refused without a note, `[queue].dismiss_reasons` in `fathom.toml`
  adding more, and a picker ordered by what this repository actually uses. An unknown code still reads
  back truthfully instead of vanishing. Snoozes are conditions with a stamped baseline
  (`next-index:41`, `new-in-session:7`, `1d`/`7d`/`30d`), so a year later the ledger still says what the
  reviewer was waiting for and the shell can answer whether it has happened.
- **`what you skipped, and why`, on `r`.** Dismissals roll up per reason with count, share, who did it,
  when last and the riskiest thing in the bucket, over the whole ledger or a window; a systematic pattern
  (≥5 dismissals on one code and ≥40% of all dismissals) is called `a pattern, not a stack`, an empty
  ledger says nothing was skipped rather than nothing was found, and an unreadable ledger gets its complaint
  printed instead of a clean sheet. Clearing a 100-episode queue costs the shell 32 ms of the ticket's
  five minutes (`clearing_a_hundred_episode_queue_costs_the_shell_seconds_not_minutes`).


### `e4.4` — the risk score became a feature vector with its arithmetic printed (`reco-884`)

- **`features-1`: twelve documented axes, one sum.** Findings and severities, surface, migration
  exposure, blast radius, test ratio, size, dependency exposure, deletions, noise share, breadth,
  attribution confidence and authorship - each with a named unit, a stated ruler that turns the unit into
  `0..=1`, a source row, and the ticket that owns it. `fathom why <episode>` prints all twelve whether or
  not they measured: raw value, weight, contribution, the sentence behind it, and - for what could not be
  measured - the ticket that owns the missing input. An absent axis leaves the **denominator** rather than
  posing as a zero, because a `0` on a review queue reads as "safe".
- **Weights are a knob with a range, a provenance and a refusal.** `[risk.weights]` and `risk.tau` in
  `fathom.toml`, clamped to `0.0..=1.0` with the clamp printed, unknown keys refused in a notice rather
  than defaulted, every weight carrying whether it came from the build, the file, the environment or
  `--weight`. Monotone in the precise sense, and tested that way with `proptest`: a heavier weight never
  shrinks its own contribution, never shrinks the gap it favours, and never lets a dominating episode
  fall behind - and the test says out loud which stronger claim is *not* true.
- **A calibration set, so a weight change is evaluated rather than felt.** 40 hand-labelled cases,
  precision/recall at the top decile, and a deterministic coordinate-ascent retune:
  **0.750/0.500 (F1 0.600) as shipped, 1.000/0.667 (F1 0.800) retuned**, mean labelled rank 5.3 to 3.5.
  The report also caught the shipped threshold: nothing in `features-1`'s range reaches `risk.tau = 70`,
  so `--calibrate` now proposes its own decile floor beside the `[risk.weights]` block.
  `fathom why --calibrate <file>` measures a repository's own dispositions instead.
- Reproducible by construction: no clock, no path, no network in the arithmetic; two runs of the same
  episode print identical bytes, asserted against the built binary, and the printed terms add to the
  printed total in integer hundredths. [`docs/concepts/risk-scoring.md`](docs/concepts/risk-scoring.md)
  is the document; `crates/fathom-analyze/tests/risk_score.rs` and `crates/fathom-cli/tests/cli_why.rs`
  are the proof. The TUI's `w` still prints `spine-1` until `e4.5` rewires the queue around this vector.


### `s5` — the LLM narrative polish question is measured and closed: ship the template, ship no LLM path (`reco-6p8`, deciding `e4.7`)

- **Three arms, one evidence set, twenty real episodes.** Every arm composed a narrative from the
  same `fathom-delta/1` document with an address on every fact, so "supported by the evidence" became
  a checkable predicate instead of an opinion: `recommender.narrative.audit` pulls each sentence back
  out and asks whether every figure, path, counted noun and motive it asserts is in the evidence it
  cites — in a strict tier and a tolerant one, and the control arm audits at **zero in both** (265
  claims), which is what makes the same zero on the LLM arms mean anything.
- **The result: usefulness rose, trust fell, so the seam stays unwired.** The blind panel (three rubric
  passes, labels sealed until the scorecards existed) scored usefulness **2.23 → 4.40** and
  trustworthiness **4.667 → 4.000** for polish against the template, while the polished narratives
  cited **0.32** of the evidence index against the template's **0.61** — the readability was bought by
  dropping figures. The template reproduced byte-identically 20/20 and across processes; the polish arm
  matched itself **0 of 6** resamples, drifting in *which facts got said*. Decision: `reco-nv0`, with
  five re-open gates (including an arm prompted for helpfulness rather than citation, and three human
  reviewers). Written answer: [docs/concepts/narrative-polish.md](docs/concepts/narrative-polish.md);
  numbers: `benchmarks/reports/s5-narrative-polish.json`, re-derivable with
  `python3 -m recommender.narrative score … --check`.
- **What is honest about the numbers, and what is not.** Cost and latency for the LLM arms are
  **modelled** from recorded character counts at a reference price band — no API key or provider
  endpoint was reachable from the spike sandbox — and the report says so in every row that carries
  them. The panel is three passes of one model, not three humans, and the arms were written by the same
  model that wrote the audit, which the concept doc's threats section states as the reason the zero is
  an upper bound on a disciplined arm and not a rate anyone else would reproduce.
- **The finding nobody asked for:** arm A's badness is a template bug, not an LLM-shaped hole. It
  prints figures in document order, repeats the attention block as a risk block, and quotes the delta
  document's house style at the reader — three fixes that are available without buying determinism,
  byte-stable exports and the offline guarantee second-hand. Five of the panel's twelve hand-flagged
  "suspect" claims were complaints about the **delta document** (the `keeps 25%` clause printing when
  nothing was discounted), which is a follow-up for `e4.7`'s inputs, not a policy about models.

### `e5.9` — the first five minutes: a miniature repository, a converging index estimate, and no shrugs (`reco-2uo`)

- **`fathom --demo`: a repository that ships inside the binary.** From a clean machine — empty
  `$HOME`, no `fathom.toml`, nothing in the environment — the flag unpacks a 21-commit miniature
  (two people and one agent; a rename, a dependency bump in two ecosystems, a deleted test, a CI
  job, a cycle, a side branch and a merge), indexes it with the same `fathom index` a reviewer will
  run on their own history, and opens the shell on it. It is not a mock: there is no demo mode below
  `crates/fathom-cli/src/demo.rs`, because the flag changes the process's working directory and gets
  out of the way, which is why `fathom --demo stats` and `fathom --demo show <episode>` work without a
  special case anywhere. Nothing outside `$XDG_CACHE_HOME/fathom/<digest>` is written, the directory is
  marked `.fathom-demo`, and deleting it is the complete undo — asserted, not promised, by
  `crates/fathom-cli/tests/demo.rs`, which also times the cold run (under ten seconds against the
  ticket's minute) and fails if the working directory changes by so much as one file. The miniature is
  `crates/fathom-cli/assets/fathom-demo.pack` (~30 KiB of loose-object bytes, no decompressor needed),
  generated by `tools/gen_demo_repo.py` with pinned dates and identities; `check` fails when the pack
  and its manifest drift from the generator or from each other.
- **The index now reports progress with an estimate that converges**, in ADR-006 §10.3's own row:
  `indexing 62 % · ~14 s left · 4 102 / 6 552 commits`. The percentage was the easy half. The estimate
  is `fathom_core::progress` — the **median** rate of a twelve-sample window, in a crate rather than the
  shell because `fathom index` is a headless verb and the `minimal` binary has no shell to borrow one
  from — and the alternatives are rejected on measurement: a whole-run mean cannot follow the
  spine-walk-to-analyses phase change (it undershoots the rest of the run, the classic "5 s remaining"
  at 95 %), exponential smoothing needs a constant no repository agrees to, and a short-window mean lets
  one 2 s merge commit among twelve 4 ms ones predict 170 ms a unit. `crates/fathom-tui/tests/progress.rs`
  asserts all of it against synthetic histories whose true cost is known: convergence within 2 % on a
  steady run, convergence *after a step change*, immunity to one absurd unit, and the edge cases where a
  countdown would be a lie — `estimating…` before a rate exists, `unknown %` where no denominator does
  (the walk cannot count what it has not walked), `stalled — fathom doctor says what it is waiting on`
  after ten still seconds, `done` rather than an expired `~0 s left`. Rows go to stderr, overwrite
  themselves on a terminal, become one line per update on a pipe, and never appear with `--json`;
  `--progress` and `--no-progress` are the script-facing overrides.
- **Every pane's empty state is written in one module and audited as a set.** `crates/fathom-tui/src/empty.rs`
  holds all of them, because nine views' worth of copy written one pane at a time is copy nobody re-read as
  a whole, and `` `manifest` — nothing to show yet`` is the shape that results. Reason first, then the action,
  and the index axis wins whenever the index is the problem (absent, unreadable, still building, finished with
  nothing in it). `crates/fathom-tui/tests/empty_views.rs` walks the shipped view registry and fails on a
  pane that falls through to the generic fallback, on a next action that is not a keystroke the shipped
  keymap really binds in that pane's mode or a verb the CLI has, and on two panes sharing a name (`tree`,
  `detail`) saying the same nothing in different views. The search pane quotes the reviewer's own query back;
  the stale third line of the timeline card — a claim that `e2.10` and `e2.11` still owed it delta rows, both
  of which shipped — is gone.
- **No sentence in the frame may name a command this build does not have**, and that is now enforced rather
  than remembered: `crates/fathom-cli/tests/copy.rs` reads the verb and flag lists off `fathom --help` on
  the built binary and fails if the frame or a remedy names anything else, so the promise and the binary
  cannot drift apart the way `--demo` once did. The same file drives every failure mode it can construct and
  requires each to end in a next action — which fixed two that ended in nothing: `fathom analyze` with no
  episode (now prints the handle grammar and the verbs that find one) and every unbuilt analysis behind a
  verb, whose hint used to be a bare ticket number.
- New document: [docs/concepts/onboarding.md](docs/concepts/onboarding.md) — the first run in order, what
  each state's row says and why it may not say anything else, the empty-state inventory, and what proves
  each claim. ADR-006 §10.3 carries a note for the same.


### `e5.5` — every terminal, every width, and the text render that survives both (`reco-2a3`)

- **The Rust layout ladder reproduces the published matrix.** `fathom_tui::frame::plan()` walks
  ADR-006 §6.1's sale order — hint, tab bar, footer strips, box frame, secondary-to-strip, hard
  minimums, compact rows, single pane, refuse — and returns what the frame did and why, which the
  status line prints as `LAYOUT degraded · N dropped · <C-w>o, ?` instead of quietly shrinking.
  `crates/fathom-tui/tests/layout_matrix.rs` fails when the Rust answer for any view at
  40×10, 80×24, 100×30, 120×40, 160×50 or 240×60 disagrees with §6.5's two tables, in either
  direction, and writes the screenshot goldens under `tests/golden/layout/` that show each step's
  effect rather than its name. Below a view's refuse floor the panel states the size needed, the
  size it got, and `fathom --plain` for text; the panel's width budget is derived from its own
  widest line, so the explanation can never be the reason it does not fit.
- **Text is measured in terminal cells, not Unicode scalars.** New `fathom_tui::text`: display
  width, grapheme-cluster-safe clipping, and the invisible-character substitution. CJK, combining
  stacks, ZWJ emoji sequences, flags and skin-tone modifiers occupy the cells they will actually
  paint, so a path or a commit subject clipped at a pane edge loses its last grapheme instead of
  splitting one in half; control characters, C1, bidi overrides and isolates, and zero-width
  formatting become visible stand-ins in every string this tool did not author — a harness-authored
  `Agent-Summary:` carrying an RTL override reads left-to-right like everything else, because text
  that can reorder the reviewer's frame is an attack surface (`tests/unicode.rs`, plus `proptest`
  as a dev-only dependency asserting no cut lands inside a cluster and no frame panics on random
  text).
- **The pointer is opt-in, and additive to the letter.** `fathom tui --mouse` takes the pointer —
  click selects, a second click in the same cell drills, the wheel scrolls the pane under it — and
  is off by default because capture takes the terminal's own text selection away, which is how a
  reviewer gets a path out of `fathom` and into a comment. Every gesture is translated into the
  `Msg` the equivalent key sends, and `tests/pointer.rs` applies the gesture and the key to two
  identical shells and asserts the resulting state is identical, for every gesture; a gesture that
  lands on chrome answers in words instead of doing nothing. Finding the pane under a click exposed
  a real bug in the keyboard path too: the queue draws two lines per episode and the row maths divided
  by the page size, so `j` and `k` counted three rows per episode.
- **Terminal facts are given, never read.** `fathom_tui::terminal::Probe` carries `TERM`,
  `COLORTERM`, `NO_COLOR`, the `LC_ALL`→`LC_CTYPE`→`LANG` chain, tmux, screen, ssh and whether
  stdout is a terminal at all; the CLI builds it at the process edge, because a TUI crate that
  reads the environment cannot be driven from a test, a daemon or the MCP server (ADR-002 §5).
  Every degraded answer is announced with the remedy in it — `TERM=dumb`, a missing `TERM`, a
  non-UTF-8 locale pointing at the exact `export`, `NO_COLOR`, tmux needing `set -g mouse on`, a
  redirected stdout — and a healthy terminal is told nothing, because a wall of caveats trains a
  reviewer to skip the one that matters. `NO_COLOR` removes colour at the sink and nowhere else
  (`style::strip_colour`, the mirror of `Face::style`), which §10.2 makes possible: the frame is
  laid out identically and every glyph, rule and marker stays.
- **`--plain --lines` prints the frame in reading order.** Two panes side by side is the right shape
  for eyes and the wrong shape for a log, a pipe or a screen reader, so the line mode gives one block
  per pane, each headed with the pane's name and position, with no column rules and no clipping to a
  column width. Same facts, same order, one per line; `crates/fathom-cli/tests/tui_text.rs` pins the
  three externally checkable claims — no `\x1b` on stdout, no column rule surviving, every grid row
  present — and that a flag which cannot apply here (`--mouse` on a pipe) says so rather than silently
  doing nothing.

### `e2.7` — dependency risk: who asked for this, and should I be worried (`reco-yxi`)

- **New module: [`recommender.deprisk`](src/recommender/deprisk/).** Two questions per episode, and
  the second is worthless without the first. `e2.6` answers what moved in a dependency set; this
  answers what a reviewer actually asks looking at `+ fastparse 2.2.0` — who asked for it, and
  whether to be worried — and treats *silence* as the failure mode in both halves: a blank
  provenance line reads as "nobody asked", and "no advisories found" about a seven-week-old cache
  is the most dangerous sentence this tool could print.
- **Provenance is a ladder that refuses to invent a rung.** This episode's own additions, else a
  recorded earlier introduction (matched by name, and saying so when the record's version differs),
  else `not recorded` plus the two things that would fill it. The intent half is a separate verdict,
  because knowing the episode is not knowing the reason: an `Agent-Intent:` trailer is the answer,
  a harness-turn *digest* is `not captured — prompt digest only` (hooks v1 stores `prompt_sha256`
  and `prompt_chars` and no text, ADR-005 §2.2), evidence from a source outside
  `deps.provenance.intent_sources` is printed as refused with the key named, a low-confidence intent
  is printed as a labelled guess rather than dropped, and attribution is never intent by default
  (`deps.provenance.agent_counts_as_intent = false`) because who executed a change is not who asked
  for it.
- **Zero network calls by default, and no key that changes that.** Advisory lookup needs all three
  of `deps.advisories.enabled`, `deps.advisories.mode = "refresh"` and **user-scoped**
  `privacy.advisories` — a cloned `fathom.toml` cannot grant consent (ADR-001 §7.5) — to the OSV
  endpoint fixed in code, because a `deps.advisories.endpoint` key would be a redirect a clone could
  install. `fathom deprisk consent` prints the three gates and `0 network call(s)`;
  `advisories --refresh` refuses at the gate with exit 8, not at the socket; `advisories --plan`
  prints every request a refresh would make without making one. The claim is asserted by tests that
  replace `socket.socket` with a trap and then run every verb, and the analysis half is checked by
  AST for no `socket`, `urllib`, `os`, `pathlib`, `time` or `datetime` (ADR-002 §4.5).
- **Freshness is an answer, not a footnote.** Five cache states (`fresh`, `stale`, `expired`,
  `missing`, `offline`), each with what it may claim: past `stale_limit_hours` the tense changes to
  *not checked recently* and the card stops saying `no advisories found`; a package with no cached
  entry, an ecosystem not asked about, a cap overrun or a **failed query** is listed as unchecked and
  never counted clean; an advisory with no upstream severity prints as `unseverity` instead of
  nothing. The cache at `deps.advisories.cache` round-trips through its own serialisation, a corrupt
  file is a stated problem rather than a crash, and `cache --import-from` fills it offline from
  recorded OSV responses — which is how CI and this document's examples get their data.
- **A licence matrix in config, with the edge path attached.** Four class tables (`permissive`,
  `weak_copyleft`, `strong_copyleft`, `network_copyleft`) plus `deps.license.deny` as a veto, and six
  matrix cells saying what each class may do in the library layer versus anywhere else — strong
  copyleft denied in a library and warned about in a composition layer, network copyleft denied
  everywhere, an unrecognised id never allowed. `AND` is cumulative, `OR` is resolved to the least
  restrictive option and the options *not* taken are printed. Drift is caught when the version did
  not move, a removed package is never a policy question, an undeclared licence is a stated partial
  and not a pass, and every violation names the offending edge path (`fathom-analyze → jwt 2.1.0 →
  ring 0.16.8`) because "who asked" is the half a reviewer can act on. This repository dogfoods
  `deps.license.library_layers`: `[layers]` already names which crates are library code.
- **49 config keys, regenerated not hand-edited.** `deps.advisories.*`, `deps.license.*`,
  `deps.provenance.*`, `deps.card.*` and one switch per finding kind under `deps.findings.*` are
  declared in `e0.8`'s schema, so `schemas/fathom.schema.json`, `docs/concepts/config.md` and
  `fathom deprisk policy` all agree with the policy object — a cross-key impossibility
  (`stale_limit_hours` below `max_age_hours`) is refused with a sentence instead of rendered as a
  self-contradicting card, and switching a finding off counts it as suppressed rather than deleting
  it.
- **Fixture set:** 28 hand-authored cases in `src/recommender/deprisk/corpus.py` with recorded OSV
  payloads (no network, no fake server), scored in CI with provenance, intent, licence precision,
  licence recall, advisory recall, freshness, finding-table and reconciliation accuracy all barred at
  1.0 — precision included so a checker that fires on every copyleft row, including the
  dual-licensed and development-only traps, cannot pass. 200 generated episodes reconcile, every card
  passes `render.audit`, and `tests/test_deprisk_contract.py` maps the ticket's four acceptance
  criteria one section each.

### `e4.1` — attribution: who wrote this commit, with the evidence that says so (`reco-w7q`)

- **New module: [`fathom_ingest::attribute`](crates/fathom-ingest/src/attribute.rs).** Authorship is
  decided by a ladder in the same shape as the boundary cascade — a stored correction, then a harness
  turn, then a claim trailer (`Co-authored-by`, `Generated-by`, `Generated-with`, `Assisted-by`,
  `Agent`, …), then the registry of known agents, then an agent-shaped author, then the committer, then
  a message that says so, then "a real identity and nothing else" — and the first rung with an opinion
  decides. Nothing is blended. ADR-003 §8 **H4**'s published confidences are used as published
  (0.95 stated, 0.85 shaped, 0.50 nothing-but-an-identity, 0.00 no identity at all).
- **`unknown` is a verdict with a reason.** Four cases refuse to guess — no identity anywhere, two
  stored corrections that disagree, a marker that matched only inside an address's *domain*
  (`jane@assistant.fr` is a person at a hosting name), and a repo that raised
  `attribution.ambiguous_below` above the strongest evidence — and each prints which reading it could
  not choose between. Two agents both claiming one commit is deliberately *not* ambiguous: the kind is
  not in doubt, only which agent, so the kind stands and the agent is left unset with a note naming both.
- **The evidence is stored, not recomputed.** Migration 4 adds `committer_name`/`committer_email` to
  `git_commit`, `attribution_source`/`attribution_agent`/`attribution_evidence`/`attribution_reason` to
  `episode`, and the `attribution_correction` table. `CommitFacts` gained the committer identity, and
  `segment::Rules::agent_markers` became `Rules::attribution: attribute::Policy`, so the marker table,
  the registry and the corrections all arrive as values (`ADR-002 §4.5`). No episode id moves: ADR-003
  owns the *author boundary rung*, and nothing here touches `CommitFacts::author_identity`.
- **The card and the CLI show it.** The delta document gained `episode.attribution` (kind, source rung,
  agent, confidence, evidence list, reason), the rendered card prints `attribution agent · claude ·
  0.95 via trailer` plus the evidence lines, and `fathom show --json` carries the same object.
  The episode schema and `schemas/fathom.schema.json` were regenerated, not hand-edited.
- **A correction is learned, not retyped.** `fathom attribution correct <rev> --kind human`,
  `list` and `forget` write `attribution_correction`, keyed on the *identity* (`identity:<address>`)
  rather than on an episode id that a rebase would move; the next `fathom index` loads them into the
  policy, so every later commit with that signature reads as corrected — at confidence 1.0, because a
  human said so — until the correction is forgotten. `[attribution]` in `fathom.toml` (agents, markers,
  `trust_committer`, `ambiguous_below`) is the registry the ticket asked for, and the config schema is
  its source of truth.
- **Fixture set:** 30+ real-world trailer conventions (`crates/fathom-ingest/tests/attribution_corpus.rs`)
  classified at 100% per convention, the ambiguity rules tested as verdicts, and the ladder's prices
  pinned to ADR-003 §8 H4 so a re-price is a diff and not a drift.

### `e2.11` — one versioned delta model, and the version policy that keeps it one (`reco-tt4`)

- **`EpisodeDelta` is now the only analysis model.** Seven sections (`what`, `where`, `dependencies`,
  `surface`, `tests`, `blast`, `attention`), always all seven, each `measured`, `partial` or
  `unavailable`; every figure carries evidence (a JSON pointer into the document, a command to run, or
  the plan key that owns the answer), every gap carries a one-line short form and names the ticket that
  will fill it, and the document **checks itself** — `verify` resolves every evidence pointer,
  reconciles the derived totals against the file rows, refuses a section that claims a measurement it
  cannot show, and prints the list of what it checked. Nothing about a delta is inferred by whoever
  displays it.
- **New verb: `fathom analyze <episode> [--json]`.** The headless form of the model — one JSON line on
  stdout and nothing else there, or the model's own rendered sections for a human. It is the input to
  the export and to an MCP tool result, and the same `render`/`brief` the TUI card calls.
  `--schema` prints the published JSON Schema, `--schema --check` diffs the checked-in copy,
  `--schema --write` regenerates it.
- **Published schema, generated not written.**
  [`schemas/episode-delta.schema.json`](schemas/episode-delta.schema.json) is the output of a generator
  inside the crate that owns the types; a test fails if the checked-in file is not byte-identical to it.
  `schemas/episode-delta.baseline.json` archives the shape projection CI compares against.
- **The compatibility policy is a gate.** Additive changes keep the version; breaking changes bump it; a
  breaking change without a bump fails CI — `python3 tools/check_delta_compat.py check`, with
  `self-test` beside it in `.github/workflows/contracts.yml` mutating a copy of the schema (required
  key added, property renamed, type narrowed, enum shrunk) to prove the gate is awake. Both run in the
  docs/contracts job, which needs no Rust toolchain.
- **The TUI computes nothing.** The card renders `insight::brief` and the model's own sentences —
  including the ranking's ceiling, which the model words (`RiskView::blind_spot_note`) so no surface can
  print a bare score and call it an all-clear. `fathom-tui`'s duplicated `Risk` type is gone; risk
  travels inside the delta. Beyond `M02`'s façade rule, new lint rule **`M03`** fails the build if a
  surface calls `compose`/`rank`/`classify` or the store bridges through the façade.
- **Two schemas and one validator, not two validators.** The CLI's `--json` round-trip validator moved to
  `crates/fathom-cli/tests/validator.rs` and is shared by the new
  `crates/fathom-cli/tests/delta_schema.rs`, which validates every shape the model can emit — and the
  real binary's output — against the published schema.

Nothing since the alpha besides the above. Everything below this line and above the next heading belongs
to whoever cuts `0.1.0-alpha.2`; the alpha itself is cut in the section below.

## [0.1.0-alpha.1] - 2026-10-18

The first build anyone outside this repository can install: one `fathom` binary, four targets (Linux
x86_64, macOS x86_64 and aarch64, Windows MSVC), a `curl … | sh` installer, a PowerShell one, a source
tarball and a `sha256.sum`. **An alpha, not a beta**: the git spine is real and reviewable — index a
500-commit agent-driven repository, fly the timeline, read the diffs, rank the queue, keep a session
across restarts — and everything downstream of the spine is still Epic 2 and later: no ranked
delta-carrying review frame, no architecture graph, no attribution, no export. Milestone `m0`
(`reco-n1j`) records which of its four gates this satisfies and which two are a public remote and three
human beings rather than code.

Most of what is below is scaffolding: the shape ADR-002 declares, compiled, tested and shipped through
CI. The first entry is not — it changes bytes an index already stored — and the line below it used to
claim no product behaviour appeared on this page at all, which stopped being true at `e1.1`.

### `m0` — the alpha cut, the release gate, and the two things a commit cannot do

- **The version became a prerelease.** `0.1.0` → `0.1.0-alpha.1`, in one place (`[workspace.package]
  version`) and therefore in every crate, in `fathom --version`, and — because `cargo-dist` bakes the
  release tag into the installer scripts it generates and refuses a tag whose version the package does
  not carry — in the URL a user will `curl`. Prerelease on purpose: `cargo-dist` then marks the GitHub
  release a prerelease, so an alpha cannot become "latest" and a stranger's default install, and an
  alpha that takes feedback can be re-cut as `alpha.2` instead of pretending to be a final. The store
  does not care — ADR-004's cache keys come from the derived analyzer versions, not from this string —
  so no index on any machine is invalidated by a version bump.
- **`tools/check_release.py` is the release path's gate, and it needs no network, no `dist` and no
  credentials.** `e0.7`'s `dist generate --check` proves the generated workflow and its config agree;
  twelve rules cover the things that check cannot see: a target set that quietly loses an OS (`R02`), a
  missing installer for one OS family (`R03`), `pr-run-mode` dropped from `upload` so PRs stop building
  the artefacts (`R04`), a half-done version bump that resolves on nobody's machine but the author's
  (`R05`), a second crate becoming releasable (`R06`), a workflow pinning a different `dist` than the
  config (`R07`), a version with no dated changelog section, which announces nothing (`R08`), a README
  quoting a tag the installers do not bake (`R09`), captured evidence gone stale against the config
  (`R10`), a document dropping a generated block (`R11`), and a prerelease set to publish itself as
  latest (`R12`). `--self-test` lands with it: twelve ways to be red, every one of them detected, on
  throwaway copies rather than the real tree.
- **The release plan is now measured evidence in the tree.**
  `benchmarks/reports/dist-release-plan.json` is a projection of what `cargo dist plan` actually said
  about this workspace — the four triples, the four runner labels GitHub gave them (`ubuntu-22.04`,
  `windows-2022`, `macos-14`, `macos-15-intel`, which is what `dist-workspace.toml`'s comment had been
  *claiming* without anything checking it), the artefact names, the installer environment variables.
  `docs/concepts/release.md` renders its tables from that file and CI diffs them, so the documentation
  cannot keep describing a pipeline that stopped existing the day someone edited a target list.
- **The install path was exercised end to end, on the one machine available here.** `dist build` in the
  release profile, archive plus checksum, installer run against a local mirror through
  `FATHOM_CLI_DOWNLOAD_URL`, the installed `fathom` reporting `fathom 0.1.0 features: tui mcp watch
  sqlite-bundled langs-all deps-all` and a working `fathom doctor --json`. That is the mechanics proven
  — the release profile compiles, the archive layout is what the installer expects, the checksum
  verifies, the installed binary runs — and it is not the Windows or macOS artefact, and says so. The
  recipe is §4 of the release document, including the caveat.
- **Two gates stay open, and are not code.** Publishing needs a public remote (the `package.repository`
  placeholder of ADR-002 §10.3 is still the value in the manifest: a wrong one is a broken install, not
  a cosmetic bug, which is why `R09` exists) and someone to push `v0.1.0-alpha.1`. Adoption needs
  people, and the only counting method this project is willing to use is an opt-in `fathom doctor
  --json` they choose to send and a ledger row written by whoever reads it — telemetry is out, per
  ADR-001 §8 and `e6.8`'s posture, so the count under-counts on purpose. Both halves have tickets; the
  milestone's close reason says exactly what was verified and what is outstanding.

### `reco-quk.1` — the analyzer version absorbs the compiled-in feature set (`e1.15`), and Epic 1 closes

- **A cargo feature was a fact the cache did not know.** `lang-rust`, `lang-python` and the rest gate the
  tree-sitter extractors the graph and structure analyzers are built from, so `graph.build`'s answer in a
  build compiled without `lang-rust` is a *different answer* — and a row written by the `langs-all` build
  would happily answer a lookup made by the language-free binary the release matrix ships. ADR-002 §10.2
  put the feature set **into the analyzer version** rather than into a tenth `ck1` key field, because a
  framing change makes every stored row in every store unreachable and a feature flag is not a framing
  change: `graph.*` and `structure.*` now version themselves `1+<short digest of the enabled lang-* set>`,
  which reaches key field 3 and the §5.4 dependency digest alike, so one derived bump cascades the way a
  hand-typed one always did. The git-shaped analyzers deliberately do not carry the tag — a rebuild is not
  an invalidation event for normalisation. `fathom_analyze::build` computes the digest, because features are
  facts about the crate that declares them and `fathom-store` cannot see them; a build that never says what
  it was compiled with is stamped `build-features-unspecified` rather than guessed for.
- **Epic 1 is closed with it.** `reco-quk.2`, `reco-quk.3`, `reco-dot` and this were its last four children;
  `bd list -l defect` is empty. The caveat `reco-glv` recorded travels with the close: the rail carries 500
  real episodes, and the ranked, delta-carrying frame the walkthrough asked for is `e2.11`'s and `e4.4`'s
  work in Epic 2. Epic 1 closes as *the spine is real and reviewable*, not as *the frame is finished*.

### `reco-dot` — TB7's clock guard is derived now, and §5.2's worked example is one episode again

- **`gap ÷ 9` was the worst point on the whole curve.** ADR-003 §4.4 published a 3-minute-20-second guard on
  `message-shape`'s split direction, which is shorter than the "the review came back" interval the guard
  exists to tolerate — so §5.2's own example (`feat:` … `fix: review feedback` three minutes later … `test:`
  four minutes after that) was being cut in two. The guard is now **gap ÷ 5** (6 minutes), and it is
  derived: spike `s2` grew a `tb7_guard_sweep`, the corpus has five within-turn conventional-type
  adjacencies and four of them are 3 or 4 minutes apart, so the guard has to clear 4 minutes. The pooled-F1
  argmax is a 1-minute guard worth +0.010 and it was **declined** — it cuts all five of those adjacencies,
  which is the failure the guard is for — and the cost of that choice is written down rather than hidden.
- **The quarantined test is a passing test again.** `segment::tests::a_run_of_agent_commits_inside_the_gap_is_one_episode`
  was `#[ignore]`d with the defect named, not deleted; it came back in the same change that re-blessed the
  spike's report, in the order ADR-003 §18.2 **D6** demanded. The quarantine list is now empty, and the
  contract test that guards it says why being empty is the design.
- **Cascade numbers moved, all in the good direction**: pooled boundary F1 **0.935 → 0.940**, message-shape's
  false fires **5 → 3**, annotated turns split open **6 → 4**. §4.3's "weakest splitting signal" claim is
  re-argued with the curve attached: the rule is worth about nothing whichever way its split direction is
  set, and the guard was worth 0.010 of pooled F1.

### `reco-quk.2` — a `subject-claim` rung: two commits claiming one ticket are one episode (`e1.13`)

- **The strongest signal spike `s2` found is now a shipped rule.** Two neighbouring commits that both lead
  with a tracker token (`reco-q6r: …`, `[JIRA-123] …`, `#412 …`) are one episode when the token matches and
  two when it differs; a token on one side only is no opinion at all (TB6). ADR-003 §18.2 **D1** decided it
  from 204 hand-annotated turns; `e1.13` landed it as §4.2 rung 6 at confidence 0.7, above every clock and
  below every declared rung, because a claim the author made about the commit outranks a proxy for "a new
  decision".
- **The shipped cascade is what `s2` recommended.** Pooled boundary F1 **0.849 → 0.935**, real-stratum
  **0.897 → 0.931**, annotated turns swallowed **57 → 27**, annotated pathologies hidden **8 → 5**, false
  boundaries **0** — and the spike's `D-shipped` strategy became `F-hook-qualified` to the last decimal,
  which `test_the_recommendation_is_now_the_shipped_cascade_and_scores_like_it` holds in place. The spike's
  own token parser was deleted in favour of calling production's, so nothing scores a copy of the rule.
- **Nine rungs, eight stored names.** Rung 6 stores `message-shape`, so ADR-003 Appendix B, both schema
  copies, `schemas/fathom.schema.json` and `fathom.toml` all stayed where they were; the confidence is what
  tells a 0.70 claim from a 0.40 guess. `test_the_cascade_has_nine_rungs_over_eight_stored_names` asserts
  the fold itself, so a second folded rung cannot slip in unnoticed.
- **Four spike gates re-declared in the same change, in both directions.** Two failing claims started
  passing (`shipped-pooled-boundary-f1`, `shipped-beats-all-split-pooled`) and two passing ones started
  failing (`recommendation-beats-shipped-*`, now measuring a gap of zero because the recommendation landed).
  A gate that changes verdict quietly is how a measurement becomes a story, so each was re-declared with the
  reason written into the claim.

### `reco-quk.3` — a turn id is `<session_id>:<turn_index>`, on both transports (`e1.14`)

- **Two sessions' turn 3 are two turns.** `fathom_ingest::segment::qualify_turn` qualifies whatever a
  transport handed over before the `hook` or `trailer` rung compares it, so a trailer written without the
  session — which is what the shipped trailer transport wrote — can no longer read two agent turns as one
  episode. ADR-003 §18.2 **D4** is the defect; the corpus pair `s1:3` beside `s2:3` is the proof it is
  real, and `two_sessions_that_reuse_a_turn_index_are_two_episodes` is the test.
- **`fathom show --json` prints `turn_index` beside `turn_id`**, derived from the id rather than stored
  beside it, and `null` when there is no turn or the transport carried something that is not a number.
- **The measurement moved with the rule.** Spike `s2`'s `D-shipped` strategy is production, not an idea of
  it, so the harness measures the qualified comparison now: pooled boundary F1 **0.832 → 0.849**, annotated
  turns absorbed **66 → 57**, and the report, ADR-003 §18's tables and the concept document's tables were
  re-blessed in the same change.

### `reco-5fn` — folder structure delta: where did this episode land? (`e2.3`)

- **`e2.3` shipped, and "the agent reorganised the tree" is now a number a reviewer can argue
  with.** `src/recommender/structure/` answers *where did this land* per episode: a directory
  table of added / removed / **moved** lines and files created / deleted / moved, the structural
  stories the pairings add up to (`package-rename`, `subtree-move`, `module-split`,
  `directory-flatten`), tree metrics before and after (depth, fanout, files per directory, share
  of code in the top-3 directories), a bounded structure-churn score with its five components
  printed beside it, and one deterministic sentence per episode with evidence behind every
  clause. The two fixtures AC 3 names score **0.000 (local)** and **0.900 (structural)** — the
  same twenty files, one changed in place, one reshuffled across four folders — a gap the CI
  gate requires to stay at or above 0.3.
- **The table sums, and says so either way.** Totals come from the file facts and row sums from
  the directory rows, two independent paths, checked column by column including both moved
  columns (a rename is credited to the directory it left *and* the one it entered). The property
  holds on every fixture and on **416** reconciliation cases, 400 of them generated episodes built
  from the same operation vocabulary the shapes are written in.
- **A shape is a rule, not a vibe, and a guess is labelled a guess.** The four shapes are defined
  by 26 knobs — `structure.move.min_files` (3), `structure.move.min_share` (0.6),
  `structure.move.require_emptied`, `structure.split.min_files_per_destination` (2) — and every
  refusal names the knob that refused it. Pairings come from the change list, from `e2.4`, or from
  a labelled guess; a caller's pairing is a fact only if it clears `structure.move.fact_bar` (0.75,
  deliberately `rename.fact_bar` from s4's measurement), and what this package infers is a guess at
  similarity 1.00, because identical bytes are not git's word. Eight trap fixtures hold the line:
  a rename inside one directory, a two-file move, a 16-file move with one straggler, four
  deletions beside four unrelated creations, and a caller with no bytes at all each keep the
  structural findings silent.
- **Scored in CI, asserted in 287 tests.** `fathom structure score` bars shape accuracy,
  summary accuracy (the sentence, byte for byte), precision over traps, episode accuracy and
  reconciliation all at **1.0** over 7 shape / 8 trap / 12 episode fixtures, requires all five
  shapes to have a fixture, and prices the local-versus-reshuffle gap; the card audits its own
  rendered bytes and the CLI exits non-zero when they lose a fact. 37 keys declared in `e0.8`'s
  schema, digest taken over all of them per ADR-004 §5.5, and ADR-002 §4.5 purity asserted by an
  AST walk: `cli.py` is the only file in the package that reads the world.

### `reco-a0v` — rename and copy pairing: what the card asserts, and what it only guesses (`e2.4`)

- **`e2.4` shipped, and the fact tier means what the spike said it had to mean.** `src/recommender/rename/`
  pairs one review window's deleted and added bytes into **pairings** — a rename, a split, a fold, a copy
  or a tangle, with the shape kept as a field — and a pairing is a **fact** only when git's own rewriter
  reports it at or above `rename.fact_bar`, which ships at the measured **0.75** instead of git's default
  50 %. A reviewer who distrusts a fact runs `git log --follow` on it; that is the whole test of the word.
  Measured on 17 fixtures built by real `git` (37 windows, 67 hand-authored pairings, 62 of them
  claimable): the fact tier emits **40 pairings, 0 untrue**, precision **1.0**, recall **0.6452** of
  claimable; pooled, **precision 0.9836, recall 0.9677**, **2.7 untrue pairings per 100 windows**.
  The two fixtures `e2.4` names are paired in full — `rename_storm` **7/7**, `rename_rewrite` **3/3**,
  **0 untrue** in either — and the nine windows in which nothing moved emitted **nothing**.
- **Splits, folds and copies are groupings, not renames with a label.** A 1→3 split is one pairing with
  three destinations — three rows would let a reviewer accept two thirds of a split — and its confidence
  is its **weakest** edge. Because git asserts pairs and not shapes, **no grouping is ever a fact**: a
  split whose every edge reads R100 is still our inference, and arrives labelled. Recall by class,
  because pooling hides it: rename **48/48**, split **9/9**, fold **3/3**, and a pair found under the
  wrong shape anywhere in the corpus counts as a failure (**0** such cases).
- **The copy row is a stated blindness with a price list, not an oversight.** A copy whose source the
  window never touched needs a source outside the change set, which is `O(repo files)` — so shipped mode
  reports the copies it can see (**0 of 2** of the fixture's) and prints the note saying what it cannot
  see. `rename.candidates.copies_harder` buys them back (**2/2**) at measured cost, and is off by default
  because that is what a cost criterion means.
- **A guess can be accepted or rejected, and the decision is bound to bytes.** `fathom rename accept|reject`
  writes to `.fathom/rename-review.json` with a reviewer and a note; rejecting hides a pairing and keeps
  it counted, accepting shows it as asserted *and names who said so*, and a decision whose blobs have since
  changed is **re-opened** instead of inherited — lookup by paths and shape, binding over the bytes, so a
  rebased branch is recognised as the same claim about different content. A ledger that does not parse is
  reported and never overwritten.
- **The card is audited, and the auditor is given broken cards in the tests.** `--audit` re-reads the
  rendered card and fails it for a fact badge on a guess, a suppression with no suppression line, a cap
  with no count, a group row that lost its shape, a bare percentage with no method, or an unprinted limit.
  Every row carries its receipt — `content J0.34 agrees (50 shared of 99/99 3-line shingles)`, not a bare
  number — and `fathom rename explain --path P` says why the grouping is a guess and which command checks
  it.
- **The cost bound is measured as flat against the tree, and the mode that is not is priced.** Repository
  200 → 4 000 paths with the change set held at 6 moved files: **2.69× shipped** against a gate of 3.0,
  **15.56× widened**, with blob counts beside the milliseconds (6 read versus 4 000). Change set 1 → 48
  pairs costs **2.77×** — quadratic in what changed, which is the honest statement, pruned by
  `top_k`/`budget` and saying so when the budget bites. Nine quiet windows emit nothing at the shipped
  floor; the same window at floor 0.04 is where `s4` counted 84 fabrications, and gate `S2`/`A2f` keep
  that trade visible.
- **Two prohibitions became gates instead of opinions.** Path and file-name similarity is never part of a
  pairing score (`P4`: same basename, disjoint content, nothing emitted) though it is still printed and
  used to order rows, because `s4` measured a 0.15 name weight taking wrong pairings from **1 to 36**.
  And no content score is an approximation: exact Jaccard over hashed line shingles, with `P3` recomputing
  every printed number exactly. 13 config keys declared in `e0.8`'s schema, `diff.rename_detect`
  bumped to version 2 so a change to any of them invalidates cached pairings, the analysis half IO-free
  per ADR-002 §4.5, and `benchmarks/reports/e2-4-rename-pairing.json` re-measured in CI with **30 gates**
  and **60 contract tests** in `tests/test_rename_pairing_contract.py`.

### `reco-gj6` — spike `s4`: when a rename pairing is a fact, and when it is a guess (`s4`)

- **`e2.4`'s guessed bar now has a measurement under it.** `reco-a0v` promised "≥ 90 % pair precision and
  recall" for rename and copy detection, and `s4` was the P1 gate in front of it. Sixteen repositories
  authored by real `git` (29 review windows, 49 commits, 4 merges, a squash rebase, and an evil merge built
  with `commit-tree` because no checkout produces one), 60 hand-authored true pairings, **5 named decoys**
  — pairs recorded as *not* a move so a detector's temptation is measurable — and 5 pairs past a stated
  evidence ceiling. 23 detector configurations swept from 0.10 to 0.90, 51 gates, re-measured end to end
  in CI.
- **A similarity score cannot be the fact boundary, because the lies score higher than the truths.** The
  designed trap — two scaffolded handlers sharing a template, not copies of each other — scores **R064** in
  git at its shipped default and **J≈0.49** in a MinHash matcher, *above* the window's real moved-and-rewritten
  file at **R015** / **J≈0.06**. Three independent methods (git, content, paths) all rank the false pairing
  first, and all three agree on it, so no threshold is the boundary; the fact tier has to be drawn where the
  score happens to be safe and the guess tier has to be *labelled* because its lies are not removable.
- **The fact tier, measured:** git's own rewriter at **0.75** rather than the shipped 50 % — precision
  **1.00** on 34 shipped pairings, recall **0.618** of the 55 claimable pairs, **0** wrong and
  **0.0** wrong pairings per 100 windows reviewed. At the shipped 50 % git asserts one named decoy and
  still misses every rewrite below R050. Refusing the cheaper point that lets a lie through costs
  **17 more missed pairs**, stated rather than discovered later.
- **The three change classes `e2.4` names are representation problems, not tuning problems.** A 1-to-3
  split, a fold into an existing module and a copy whose source stays put are found **0** times at the fact
  bar by every method: a fold's destination is never in the additions, a copy's source is never in the
  deletions, and git's diff format pairs each deletion with at most one addition. Recall is therefore
  reported per representation class, which is the number `reco-tx0.1` should put in `e2.4`.
- **Two measured "don't"s for the implementation.** Do not blend path similarity into the score: two
  configurations differing only by a 0.15 name weight emit **36** and **1** wrong pairings at the same bar,
  and on a reformat-only window the blended one manufactures four pairings between files that were only
  edited in place (their content similarity is 0.00). Do not widen candidates to the whole tree either —
  it is the only way to see a copy at all, and it is `O(repo)`, which `e2.4`'s own cost criterion forbids.
- **`MinHash`'s own price, since `e2.4` names it.** 545 sketch-vs-exact comparisons: mean absolute error
  0.013, p95 0.063 against a textbook standard error of 0.044 at J = 0.5, and **29 pairs** where the sketch
  and the exact Jaccard land on opposite sides of a 0.3 bar. The LSH shortcut looks the best calibrated in
  the table (ECE 0.007) for the worst reason: it finds none of the split pairs.
- `src/recommender/rename/` (fixtures, truth, detectors, scorer, driver),
  [benchmarks/reports/s4-rename-detection.json](benchmarks/reports/s4-rename-detection.json),
  [docs/concepts/rename-detection.md](docs/concepts/rename-detection.md) with every table generated by
  `tools/gen_rename_tables.py`, `tests/test_rename_spike_contract.py` (15 tests, including a re-run of the
  report's own selection rule), and three CI steps: the report reproduces from scratch, the tables are
  generated, and the contract suite holds.


- **Four red builds at HEAD, all four fixed.** Checking the epic's "CI is green" criterion rather than
  trusting it turned up: `cargo fmt --all --check` failing on eight files; `cargo clippy --workspace
  --all-targets -- -D warnings` failing because the workspace denies `print_stdout` and fifteen test files
  print the measurements that are their entire output (the exemption now lives in that invocation —
  `-A clippy::print_stdout`, libs and bins still fail on it — and in ADR-002 §8.3, instead of being paid
  for file by file); two `fathom-ingest` tests failing against published expectations; and
  `tools/check_docs.py` citing tickets the registry had never heard of, plus an ADR-004 link pointing at
  `docs/BUDGETS.md`, a file that does not exist.
- **A one-byte identity bug, found by the golden vector and not by a user.** `fathom_ingest::identity()`
  hashed `author\0subject` for the **fallback** digest where ADR-003 Appendix A and the reference
  implementation in `src/recommender/segment.py` hash `author\0subject\0`. Every other field was
  NUL-terminated; the last one was not — so the strong digest matched the published vector, `episode_id`
  looked fine, and relink tier 1 was keying sign-offs off bytes no document had ever published. The
  published vector is the contract, so the Rust copy was the wrong one; `identity_matches_the_published_golden_vector`
  passes again and no §2.4 namespace bump is owed. Relink tier-1 keys change as a result: they are now the
  published ones.
- **ADR-003 §18.2 D6 is quarantined, not deleted.**
  `a_run_of_agent_commits_inside_the_gap_is_one_episode` — §5.2's own worked example, still cut in two by
  `message-shape`'s `gap ÷ 9` guard — is `#[ignore]`d with `reco-dot` and the un-ignore condition in its
  reason, because the fix is a shipped-rule change and a shipped-rule change re-blesses this spike's report
  in the same move. Three tests in `tests/test_segmentation_spike_contract.py` pin the quarantine to
  exactly that one entry, every entry to a ticket, and every `#[ignore]` in the workspace to a reason.
- **A feature-less build of `fathom-analyze`'s tests was not a build at all.** `episodes` and
  `incremental_index` import `fathom_analyze::index` and `fathom_store::{spine, cache}` — all gated behind
  a SQLite backend — so the spike `s1` CI gate compiled only by accident of workspace feature unification.
  Both targets now declare `required-features = ["sqlite-bundled"]`, the spelling `fathom-store`'s bench
  already uses.
- **`e0.12` ran.** The sequencing checkpoint read the three landed spikes and edited the plan: `e3.1`
  split into a core and the conditional-language tail `s1` described, `s2`'s decided cascade changes
  became `e1.13`/`e1.14` and a re-scoped `reco-dot`, the Guardrail 3 numbers three tickets had deferred to
  `e0.12` were set from ADR-001 §5.4 and given a calibration harness (`e4.3a`), the trailer-vocabulary
  question §18.2 D5 deferred was decided, and the MVP cut line lost the one label that contradicted
  milestone `m2`. The record — every recommendation, where it went, and what was deliberately not changed
  — is [.plan/reconciliation-e0.12.md](.plan/reconciliation-e0.12.md).

### `reco-pef` — ADR-004 §11's budgets became a red build (`e0.11`)

- **172 numbers became a gate.** `benchmarks/budgets.json` carried the budgets and a `gates` block of
  rules (15 % regression, a named calibration bench, RSS never scaled, skips must be reported) with
  nothing reading either. Now `tools/perf_gate.py` reads them, `make bench` reproduces every number in
  under 30 s locally, and `.github/workflows/perf.yml` fails the build on a breach. Every budget is
  checked **twice**: against its budget (a wall clock gets the runner factor as headroom, a resident set
  and a store byte count get nothing because a slow machine does not get to be wider) and against the
  blessed baseline in `benchmarks/baselines/perf-gate.json` — in either direction, because
  `improvement_baseline_commit_required` is true and a baseline nobody re-blesses is a budget nobody
  tightened. `make bench-bless` is the ratchet, and it refuses to bless a `--quick` or injected run.
- **The measurement is a process, so RSS is a number and not an adjective.** The scripted run drives the
  shipped binary over the deterministic 500-commit fixture: cold index into a deleted store, warm
  single-commit re-index, no-op re-index, `fathom stats --json` and `fathom show --json` for the query
  families that have verbs, and the keystroke path (`t`, `j`, `q`, `d`, first `@@`) through the same pty
  `e1.12`'s walkthrough drives. Peak RSS comes from `VmHWM` sampled *while the child runs*, because a
  peak read from a corpse is zero and zero passes every budget. `crates/fathom-store/benches/budgets.rs`
  adds the criterion half for the microsecond-scale store reads — a rail page (679 µs against an 8 ms
  budget), a card read, the two reads behind `query.frame_store_budget.ms`, and one 5 000-row
  transaction at §7.3's cap — and criterion is a `[dev-dependencies]` entry, so nothing that ships links
  it. The gate parses criterion's own `sample.json` for a real p95 rather than reading its mean.
- **A runner is not a laptop, and said so.** §11.8's named calibration bench (100 k-row batched insert
  plus 10 k FTS queries, same WAL + `synchronous=NORMAL` pragmas the product uses) runs at gate time;
  its ratio to the blessed reference, clamped to `runner_calibration_factor_range`, scales every wall
  clock, and a machine outside the window says so in the output instead of handing out a pass. Store
  bytes and resident sets are marked unscaled at the measurement, not filtered by unit at the gate,
  so "is this machine-dependent?" is stated where the number is made.
- **A gate that has never been seen fail is decoration, so failing is now testable.**
  `python3 tools/perf_gate.py --check --inject-ms 400` puts a real `sleep` in front of every measured
  process and the build goes red with exit 11 (`BUDGET_EXCEEDED`, the same number `fathom-core` prints)
  naming each blown budget; `--self-test` proves the same arithmetic — budget breach, regression,
  un-blessed improvement, RSS unscaled, wall clocks scaled, a throughput *drop* read as a regression —
  with no build, no fixture and no terminal, in milliseconds, on every runner. Both are asserted by
  `tests/test_perf_budget_contract.py` (21 checks), so a future neutering of the gate is a failed test
  rather than a quiet pass. `tests/test_perf_budget_contract.py` also checks that
  `benchmarks/BUDGETS.md` carries all 172 thresholds, since the acceptance criterion is that the page
  exists *and* that it is complete.
- **One budget currently fails, and it is written down rather than waived.** A no-op re-index costs
  46–60 ms against `warm.reindex.noop.ms.small = 10` — `e1.12` measured the same thing, filed
  `reco-7tc`, and the gate now carries it on a **breach register**: owner, rationale, and a 70 ms ceiling
  it may not grow past (with a stated noise tolerance), rendered into `benchmarks/BUDGETS.md` and
  deleted by hand the day the defect closes. CI stays green for a reason a reviewer can read, and the
  known failure cannot get worse. The same honesty is in the coverage table: 28 budget families have no
  measurement yet — the graph lane, search, retention, the daemon — and each names the ticket that owns
  the missing measurement instead of pretending the budget is asserted.
- **The trend is an artefact, not a file in the diff.** `benchmarks/trend/` (`trend.jsonl`,
  `trend.md`, `latest.json`) is gitignored and uploaded by the perf job even on a red run, and
  `trend.md` goes into the job summary: growth is visible before it is accidental without every PR
  carrying a merge conflict over a timing file. The committed number is the baseline, and one command
  rewrites it.
- Also: `benchmarks/BUDGETS.md` is now generated alongside `benchmarks/budgets.json` (one source,
  `tools/gen_budgets.py write`, `check` mode diffs both); the two stale "156 numbers" claims in ADR-004
  became a pointer to the count the generator prints; `warm.reindex.git.share_pct`,
  `store.wal_cap.mib` and the rest of the unmeasurable-for-now list carry the ticket that will make them
  measurable; and this ticket's fixture corpus, sample counts and every threshold come from the same
  generator, so no number in this entry is a hand-copied one.

### `reco-wzt` — the TUI got a data path: the rail now lists the episodes the index holds

- **No pane ever received an episode.** `fathom-cli` built every frame with `episodes: None`
  (`crates/fathom-cli/src/tui.rs:85` before this change), and the one read anyone had written for the
  purpose — `fathom_analyze::insight::episodes()` — answers `Error::NotImplemented { ticket: "e2.11" }`.
  So the timeline rail, the queue, the card, the diff and the deps panes each printed their honest "not
  in this build" over an index that held 500 episodes, and Epic 1's exit criterion ("a human reviews a
  500-commit agent repo entirely inside the TUI") was unsatisfiable by construction, because no Epic 1
  ticket owned the read. A rail row needs no delta model: identity, author kind, scale, paths,
  session/turn and review state are all rows `e1.3` writes.
- **Epic 1 owns that read now.** New `crates/fathom-cli/src/episodes.rs` opens the store the composition
  root may open and the shell may not (ADR-002 §5.2, lint `E01`), reads the live episodes newest-first
  with their review markers and their paths, maps them into `timeline::Episode`, and hands
  `frame::Options::episodes` a `Batch` whose `origin` and `total` come from the same
  `fathom_tui::index::Facts` the empty states print — so the rows, the strip's `⏺ epoch` and the empty
  sentence cannot disagree about the epoch. The page size is the façade's (`insight::PageWindow`,
  `PAGE_LIMIT_MAX`), which is what lets a rail holding part of a big index say `index holds N` instead
  of letting the reviewer conclude the repository was quiet. `Timeline::load` is reached from
  `fathom-cli` on every run of the shell that has an index.
- **One new store read, and it is a join.** `Store::episode_paths(&ids)` answers the card's `where` line
  with one `episode_member ⋈ git_commit_file` pass instead of one query per episode, for the same reason
  `episode_inputs` reads its paths in a single pass: a first frame that costs N queries is a first frame
  whose cost grows with the index. Three queries fill a whole page of rail.
- **What is still missing is still stated.** The risk column prints `?`, the card's `delta` row is
  unchanged, `insight::status().views_ready` is still `false` and `insight::episodes` still refuses with
  `e2.11`'s ticket number: this is the spine's data reaching the frame, not a claim that the delta model
  exists. Scrolling past the first page is not wired either — the strip says so — and that pager is
  `e1.8`'s list virtualization with a source attached
  ([docs/concepts/materialization.md](docs/concepts/materialization.md)).
- **A remedy that stopped being true was rewritten.** The finished-index empty state promised *nothing in
  this build can list them*; it now says `index epoch 1 ready · 500 commits · 412 eps — \`t\` opens the
  timeline, which lists them as the index knows them; \`fathom show <episode>\` prints one in full`, and a
  finished index holding **no** episodes got its own sentence rather than a pointer at an empty timeline.
  ADR-006 §10.3 and `docs/concepts/timeline.md` §9 carry six empty states now, and the MVP walkthrough's
  P1 list says which of its four defects this closes.
- **Proved by running the binary.** New `crates/fathom-cli/tests/tui_data.rs` builds a 500-commit
  agent-driven repository with `git fast-import` (fixed clock, three agent identities with
  `Agent-Session`/`Agent-Turn` trailers, two human ones — the acceptance fixture in miniature), indexes
  it, and asks the built `fathom` what it draws: rows with author badges and sparklines, a day header
  that counts real episodes, a strip that names the same number twice (`· 500 of 500`, parsed rather
  than matched, because how many episodes 500 commits segment into is the boundary rules' business and
  not this test's), a card whose `where`, `members` and `session` lines are store data, and none of the
  three sentences that used to stand in for the missing path. The first frame's toast now says
  `500 episodes from index epoch 1 (ready)` — the frame admitting how much of the index it loaded — and
  on a 4 000-episode index it adds `· index holds 4 000`, checked by hand. A second test restores a
  selection by episode id and gets that episode's card, which is ADR-003 §2's identity claim checked
  end-to-end; a third refuses the timeline pointer when there is nothing to list. `fathom-store` joins
  `fathom-cli`'s `[dev-dependencies]` so a test can read the store the binary wrote, and one golden
  (`index_ready`) was re-baked for the new wording.

### `reco-1nu` — the empty rail stopped sending the reviewer to an index that had already finished

- **One sentence was standing in for two facts.** A repository whose index held 500 commits and
  reported `epoch 1 ready` was told *nothing loaded yet — run `fathom index`*, the same sentence an
  unindexed repository gets. The reviewer ran it, reopened the shell, and was told it again; the queue
  meanwhile offered `fathom --demo`, a flag no binary of this project has ever had. ADR-006 §10.3's note
  on that row is "a shrug is not a remedy", and a false remedy is worse than a shrug because it gets
  followed.
- **The composition root now says what the store said.** New `fathom_tui::index` holds `Facts` — whether
  a store was there, whether it opened, its epoch, its status, and its commit and episode counts — filled
  by `fathom-cli` before the first frame and passed in `frame::Options::index`, because the shell may not
  open a store (ADR-002 §5.2, lint `E01`) and must not guess. Every empty pane and the rail strip branch
  on it: no index prints `no index here yet — run \`fathom index\` here, then reopen fathom`, a finished
  one prints `index epoch 1 ready · 500 commits · 412 eps — the episode read is e2.11's, not in this
  build; nothing in this build can list them, and \`fathom show <episode>\` prints one headlessly today`,
  and a `building` epoch and an unreadable store get their own sentences with their own remedies. The
  status strip's `⏺ epoch` and episode count stopped being `unknown` in the same breath.
- **`fathom --demo` is gone** — from the queue's empty state, from the drill refusal, from ADR-006 §10.3's
  table (which now carries five empty states instead of one, and the two rules that separate them), and
  from `docs/concepts/`. Pinned three ways: `crates/fathom-tui/tests/empty_state.rs` pins both sentences
  to the character and refuses any state that names a flag the binary lacks; two goldens
  (`nothing_loaded`, the new `index_ready`) pin what a pane draws; and
  `crates/fathom-cli/tests/cli.rs::an_empty_frame_names_the_index_it_actually_found` indexes a real
  repository with the `git` binary and asks the built `fathom` what its empty frame says before and after.

### `e1.12` — MVP acceptance: a 500-commit agent repository, reviewed in the TUI (reco-glv)

- **The walkthrough ran, and the spine does not reach the frame.** A scripted reviewer — no mouse, no
  leaving the terminal — opened `fathom tui` on a 500-commit, 213-turn agent-driven repository with a
  warm index (`epoch 1 ready`) and could not read a single episode. The pane that should carry them is
  handed `episodes: None` by `fathom-cli`, and `insight::episodes()` returns `NotImplemented` under
  the ticket number `e2.11` — an Epic 2 ticket Epic 1 does not depend on. Seventeen defects filed, four
  of them P1, and by the epic's own rule Epic 1 and the alpha milestone stay open. The record is
  [docs/demo/](docs/demo/): an asciinema cast, the walkthrough as JSON, the measured per-step timings,
  and the budget table.
- **A fixture for the ordinary history, and the reviewer as a pty client.** `tools/agent_fixture.py`
  builds the 500-commit agent-driven repository deterministically — one fixed clock, `.invalid`
  identities, no PRNG, real `git fast-import`, `git fsck --strict` clean, and ten planted episodes a
  reviewer is supposed to find (a destructive migration, a live key plus `permissions: write-all` in
  CI, a bump storm, tests deleted behind a rewrite, scope creep, an API removal, a declared-but-unread
  env var, a new cycle, generated churn, one enormous turn). `tools/tui_session.py` drives the shipped
  binary through a real pty and writes an asciicast v2 plus the one measurement an outsider can trust:
  the interval between a keystroke and the screen actually changing. Asserted by
  `tests/test_mvp_acceptance_contract.py` (10), which also refuses a build twice that disagrees with
  itself.
- **Perf, measured and published rather than asserted.** `tools/mvp_acceptance.py` runs the budget
  numbers into [benchmarks/reports/e1-12-mvp-acceptance.json](benchmarks/reports/e1-12-mvp-acceptance.json):
  cold index **464 ms** p95 (budget 1 500), warm re-index after one commit **70 ms** (150), spine
  **3 358 B/commit** (4 096 — the file itself is around 4 700 B/commit once the analyse cache is
  counted, which is why the budget is measured per table with `dbstat` and not per file), index peak
  RSS **20 MiB** (300), watcher detection **163 ms** (1 000), TUI RSS **8.8 MiB** (120), keystroke to
  visible change **2.5 ms** p95 with one 250 ms outlier that is the `g`/`gg` chord timeout. Two
  numbers are **over budget**: a re-index with nothing new costs **61 ms against a 10 ms floor** — so
  every script that asks "is this fresh?" pays for the whole walk — and seven query families are
  **unmeasurable**, because nothing carries a row from the store to a pane, `cold.queue_visible.ms`
  (the budget this ticket depends on) among them.

- One incidental fix: `docs/concepts/session-state.md` cited `e1.0`, which is not a plan key, and
  `schema.rs`, which is not the file — so `tools/check_docs.py` was failing on `main` and is green again.

### `e1.11` — session state: what a repository remembers about the last reviewer (reco-6hp)

- **Quitting saves the session; reopening restores it.** Selected episode, every filter (the rail's own
  `timeline.*` encoding, which existed and had no caller), focused pane and tab strip, zoom, pane sizes,
  the jump mark, the last search pattern, and the read set — the episodes `Enter` materialised a card for,
  which is what `e1.6`'s `goto.first_unread` had been computing from nothing. It lives in `session_state`
  **per repository**, not in a global config, because "where I got to" is a fact about this clone: a
  different repository opens with a different filter and a different unread set, and neither is anybody's
  default. `fathom tui --show-session` (plus `--json`) prints what the repository holds.
- **One row per fact, and every decoder total** — which is what makes "corrupt state degrades" a shape
  rather than a handler. A blob has two states, valid and useless; rows have a state per fact, so a torn
  value costs the tab strip and not the afternoon, and `restore` classifies every row as applied,
  newer-than-this-build or damaged, warns on the latter two and draws the frame anyway. Keys this build
  does not *own* are unknown rather than corrupt — an unrelated subsystem's `session_state` rows, or a
  newer fathom's, never get quarantined out of the store.
- **Two TUIs in one repository cannot clobber each other, because the write is a SQL compare-and-set**
  (`Store::set_ui_state_if`: `UPDATE … WHERE key = ? AND value = ?`), not last-writer-wins over the
  whole state. A session commits only the delta it took since its own baseline read, so a row another
  session moved is left alone and said (`session kept another session's state for the timeline: 2 row(s)`)
  rather than silently overwritten — the naive version of this feature deleted the afternoon of whichever
  TUI quit first. Nothing is written at all when nothing changed, including under `--fresh`.
- **Damage is moved, never deleted, and never fatal.** A `.fathom/index.sqlite3` that is not SQLite (a
  truncated copy, someone's `sqlite3` experiment) has the store plus its `-wal`/`-shm` siblings moved to
  `.fathom/quarantine/` and the run starts clean with the warning on stderr and exit 0; a damaged *row*
  is copied to a JSON record in the same directory and deleted from the store, and the good rows still
  apply. A warning you can keep working under beats a refusal that costs the session.
- Asserted by `crates/fathom-tui/tests/session.rs` (13), `crates/fathom-cli`'s session tests (8, the
  only Rust unit tests in that crate with a store handle), and the store's
  `a_session_state_write_lands_only_on_the_value_the_writer_read` / `a_session_state_row_survives_a_reopen`;
  `every_key_the_shell_writes_is_one_the_writer_owns` fails the moment `fathom-tui` writes a key the
  writer would not know what with, which is the seam ADR-002 §3 creates.

### `reconcile` — spike `s2`: do episodes segment the way a reviewer would cut them? (reco-q6r)

- **The measurement ADR-003 §4.4 asked for, on a corpus that says what it is.** 204 hand-annotated turns
  over 264 commits, carrying 74 annotated pathologies: **32 turns are real** — every commit this repository
  has, cut into turns by the beads board's own claim/close windows — 81 carried from `e0.10`'s
  hand-authored fixture annotations, and 91 authored to cover shapes a 34-commit history cannot contain,
  each naming the real distribution its shape came from. The ticket asked for 200 real turns; 32 existed,
  and the report prints the strata separately instead of pooling them into a number that hides which one
  earned it. Harness `crates/fathom-analyze/examples/s2_spike.rs`, report
  [benchmarks/reports/s2-segmentation-accuracy.json](benchmarks/reports/s2-segmentation-accuracy.json),
  22 claims as data in `crates/fathom-analyze/spike/s2/thresholds.json`, each declaring before the run
  whether it expects to pass — a gate that quietly starts passing fails the suite as hard as one that fails.
- **What ships is precise and short of recall.** The cascade at its published constants: precision **0.938**,
  recall **0.747**, boundary F1 **0.832** pooled and **0.897** on real history, pair co-membership F1 0.431
  against `e1.3`'s published ≥ 0.90. The three strategies the spike named are not three candidates of
  similar merit: time+author heuristics **0.722**, hook-driven boundaries **0.855** with harness evidence and
  **0.338** with what ships today because not one real commit carries turn evidence, trailers plus
  message-shape **0.092** because the trailer rung 3 reads is written by nothing. And `every commit is its
  own episode` beats the shipped cascade on boundary F1 (0.859) while hiding **23 of the corpus's 74
  annotated pathologies**, which is why no future tuning ticket gets to cite F1 without the hide column.
- **`episode.time_gap_minutes` stays at 30, with the number written down**: pooled optimum 10 minutes, worth
  +0.022 F1 and nothing else, and once a per-commit claim carries the boundary the knob is flat within
  ±0.02 from 1 to 240 minutes. What §18 decides instead is a precedence change — a **`subject-claim`** rung
  above the clock, keyed on the ticket token the loop already writes in the subject — worth **0.832 → 0.919**
  pooled and **0.897 → 0.931** real, pathologies hidden 8 → 5, annotated turns absorbed 66 → 36, at the cost
  of zero false boundaries in the corpus; plus a defect in the shipped `turn_id` transport, which is not
  session-qualified and so reads two same-indexed turns in different sessions as one. Both are recorded for
  the sequencing ticket to schedule; **the shipped cascade is unchanged by this ticket on purpose**, because a
  spike that rewrites the rule set it is measuring ends up scoring its own edit.
- Asserted by `tests/test_segmentation_spike_contract.py`; the tables in
  [docs/concepts/episode-segmentation.md](docs/concepts/episode-segmentation.md) and ADR-003 §18 are generated
  by `tools/gen_segment_tables.py`, and the corpus is validated by `tools/author_turn_corpus.py`.

### `e2.8` — non-code surface changes: config, env, flags, migrations and CI, rolled into an operator-action checklist (reco-bjd)

- **The question is "what changed that is not code, and who has to act on it", and the artifact is a checklist.** Six surfaces — env, flag, migration, CI, infrastructure-as-code, container — produce 27 finding kinds, and every finding carries a verb, an owner, a severity and a `path:line`. `checklist.build()` merges two findings that need the same thing into one row with `from_findings = 2` (two workflows reading `secrets.SLACK_WEBHOOK` is one task), orders rows by what burns first rather than by which file was edited first, prints `… 30 more: fathom noncode checklist ep-demo --all` instead of truncating silently, and puts `requires operator action: yes (N actions)` at the top where the release decision is actually made. A `Finding` with no verb, no path, no name or no confidence raises `UnanchoredFinding` at construction, because a row nobody can act on is a row that gets skimmed.
- **Environment variables are cross-checked, not listed.** Declarations (dotenv, Dockerfile `ENV`/`ARG`, Terraform, compose, Kubernetes `env:`) are joined against references parsed in the spellings eight languages actually use, which is the only way to catch both directions the ticket names: a variable added in `.env.example` with no reader anywhere (**confirm the reader exists before the operator sets it**), and a variable whose declaration vanished while `src/settings.py:4` still reads it — located at the reference, because the deleted line is gone and the boot-time KeyError is not. A rename is one row that consumes both halves, `non_code.env.rename_similarity` decides in both directions, and a committed secret-shaped value prints as `sk-live… (48 chars)`: the card ends up in terminals, exports and issue comments.
- **A migration-only episode says the words.** Reversibility is answered per dialect — no `` `def down` ``, a `def downgrade()` that only says `pass`, a missing `-- +goose Down` — destructiveness is scanned on the **forward path only** (a `DROP` inside the Down section is the rollback, not the risk) across SQL, Rails, Django and Alembic spellings, and an episode containing migrations **and nothing else** by `e2.1`'s classification emits `deploy-coordination`: **needs deploy coordination**, with the evidence that proves it (`no source or test change in this episode`, `counts: destructive=3, forward_only=1, migrations=1, reversible=0`). The mirror case, `migration-with-model`, asks for sequencing (expand, migrate, contract) instead.
- **CI, infrastructure and container changes are reported the way an infra team asks for them**: a first-time `${{ secrets.X }}` with the scope it expects (`jobs bound to environments: production`), a `permissions:` scope that gained `write` named as the scope (`id-token`, `contents`, `write-all`), a new external identity aggregated into one row per workflow (role ARN plus environment binding in one review), `ubuntu-latest → self-hosted`, a new `push` trigger on the default branch, Terraform/Kubernetes resources by **address** (`aws_s3_bucket.audit_logs`, `Deployment/worker`) with a destroy called out, exposures widened on added lines only (`0.0.0.0/0`, `"Action": "*"`), and a base image whose **major moved** (`python:3.11 → python:3.13`) plus `curl … | sh` and `USER root`. Nothing here executes or connects to anything: no `terraform plan`, no provider API call, no Docker build — the plan is a checklist row, not a command.
- **"Could not read" is never printed as "nothing", and 45 keys make the definitions somebody's opinion.** A Jenkinsfile, an undocumented flag-file shape or a confusing workflow YAML yields **one partial line per file** and drops the stated confidence from `1.0` toward `0.5`, so a clean sweep and a blind one look different on the card. Every threshold — rename similarity, destructive statements, escalating permission scopes, widened-exposure patterns, digest-pin requirement, checklist row cap, and the reference scope that decides whether "nothing reads this" may be claimed about the repository or only about the episode — is an `e0.8` key that layers, validates, appears in the generated schema and reference, and is printed as sentences by `fathom noncode definition`. `render.audit` runs on the CLI's own output and fails a card that lost a checkbox, a location, an owner or an explanation for a suppressed count. Scored in CI on 37 rule fixtures (one per kind, derived from the rule table so a kind without a fixture fails), 12 traps barred at precision **1.000**, 5 whole episodes and all 6 surfaces required, at ~5 ms per episode and ~6 ms per card.

### `e2.9` — test surface delta and coverage proxy: the testing contract per episode, in test names and locations (reco-6pl)

- **No coverage percentage anywhere.** What an episode gets is four claims computed from the same pair of revisions as the rest of the card: the test-vs-production line split (ratio **and** both absolutes, with the denominator named — `other` is out of it, so regenerating a 4 000-line lockfile cannot look like skipping tests), the named cases that appeared, vanished, started or stopped being skipped, got focused or lost their assertions, the new public logic nothing in the episode covers, and a stated confidence in how much of that was measured rather than counted. A ratio is never printed alone: `0.02` from `+4 test / +180 prod` and `0.02` from `+400 / +19 600` are different episodes, and `render.audit` fails a render that drops the absolutes.
- **Attribution is per line, not per file, because Rust tests live inside the file they test.** A path taxonomy — however good — calls `crates/store/src/snapshot.rs` a source file, and a whole `#[cfg(test)] mod` inside it would count as production. `parsers.py` finds the test regions and they outrank the path label, the card prints *which* lines it moved between columns (`note: 7 added line(s) attributed to test code inside the file (25–47)`), and the ranges survive into the JSON for `e2.10`'s diff viewer to point at.
- **A deleted or skipped test is a first-class finding, with its name and its location.** Cases are parsed by framework evidence, not by filename, for Rust (`#[test]`, `#[tokio::test]`, `test_case`), Python (`pytest`, `unittest`), TypeScript/JavaScript (vitest, jest incl. `it.each`/`todo`), Go (`testing`, testify, subtests, benchmarks skipped) and Java (JUnit 5) — deletion, skip/xfail/`t.Skip`/`@Disabled`, un-skip, `it.only`, moved-but-not-deleted and weakened assertions (`assert_eq!(x, 42)` → `assert!(x.is_ok())`, `expect(x).toBe(3)` → `.toBeDefined()`) each get a row with the case name, the `path:line` and both spellings. Weakening is **subject-matched**: a shape change that keeps its exactness (`assert_eq!` → `assert_eq!` on a new value) is not a weakening, and the trap cases are labelled in the corpus. `Finding.__post_init__` raises `UnanchoredFinding` if a finding has no path — an unanchored finding does not exist.
- **"New public logic, no new test" is 20 config keys, not a hunch, and the card prints the ones in force.** What counts as public (`test.untested.visibility`, because `pub(crate)` is a different question in Rust than in Python), what counts as logic (`require_logic`, `min_body_lines`), what counts as covering (path overlap, new-case-name overlap, a mention in the lines the episode added to a test file), and whether coverage may be satisfied from the whole repo or only from this episode (`scope`) are `e0.8` keys that layer, validate and `config explain` like every other key — and `fathom testsurface definition` reads the running values back as sentences, so the documentation and the numbers are one artifact. Every coverage decision prints its receipts, **including the silences**: `write_snapshot is covered: new test case name shares snapshot, write` is on the demo card next to the function that was *not* covered.
- **No framework is never zero tests, and a cap never drops a row quietly.** An episode in Ruby, Kotlin or CMake gets `lines-only` at a stated 0.3 confidence ("a line count correlates with test effort and does not measure it") instead of a `0` that reads like a fact; a partly-parseable test surface is `partial` at 0.6 and names the files its case counts are a lower bound over. Findings past `test.card.max_findings` print as "… 4 more: fathom testsurface card --all", and `render.audit` — run by the CLI on its own output and re-run by the contract suite against three tampered renders — fails a card that hid one. The docs' `demo` lifts the cap on purpose, and says so on the card: it exists to show one finding of each kind, and the two least severe kinds are exactly what a 12-row cap hides.
- **Scored on a hand-labelled corpus, with the two bars that matter set to 1.0.** `fathom testsurface score` gates 12 parse cases, 10 file pairs, 20 labelled coverage symbols and 6 whole episodes: parse 1.000 (bar 0.95 — one construct is genuinely ambiguous), pair 1.000, coverage **precision 1.000** and **recall 1.000** absolute, because a false "you deleted a test" and a missed one are the same card lying, and episode 1.000 because counts, ratio and confidence are arithmetic. ~0.4 ms per parse, ~2.6 ms per episode, ~5.7 ms per card (`bench`), so this runs per file per repaint. CI: `contracts.yml` runs the gate and every verb, and diffs the 20 keys against `e0.8`'s schema generator.

### `e2.2` — noise suppression: reformat, generated churn and lockfile-only change, folded with the reason printed (reco-zqf)

- **Reformat detection is a token comparison, not a whitespace flag.** `src/recommender/noise/norm.py` tokenises 24 languages (a brace scanner for the C family, Go, JS/TS and CSS; Python's own `tokenize`; a per-line word scanner for TOML/YAML/INI/Markdown; verbatim mode for multi-line strings, fenced blocks and Jest snapshots) and claims a reformat when the two revisions produce **identical token streams**. That catches what a line-oriented whitespace check cannot: a call re-wrapped one argument per line, where text crosses line boundaries and the program is unchanged. It erases only what a formatter is entitled to write — indentation, line breaks, trailing commas, `use`/`import` block order — and never statement order. Traps in the labelled corpus (a `1 + 2` → `1 + 3` hidden in re-indentation, a reflowed command inside a code fence, one character of a snapshot's expected value) stay visible.
- **Three detectors, each with a refusal built in.** Generated churn splits into `stamp` / `resort` / `regen` with bulk floors (≥60 % of the file **and** ≥40 changed lines) so a hand patch inside a generated file is `hand-edit-suspected` and stays on screen; a lockfile folds only when its declaring manifest did **not** move, and is counted from parsed entries in twelve formats over thirteen filenames (`Cargo.lock`, npm v1/v2/v3 plus `npm-shrinkwrap.json`, pnpm, yarn, poetry, Pipenv, `go.sum`, `Gemfile.lock`, `mix.lock`, composer, NuGet, pinned `requirements.txt`) rather than diffed; an unparseable lockfile, an unterminated literal and an unknown extension are `undetermined` — printed as a thing the tool cannot say, never folded.
- **A fold cannot render without its key, its policy, its reason and its evidence.** `Suppression.__post_init__` raises `InvisibleSuppression` when any of the four is missing; `render.audit` re-checks the produced text (and that expanding loses no file); the CLI runs the same audit on its own output. `tests/test_noise_suppression_contract.py` blesses both render states byte for byte and hands the auditor three tampered renders — reason removed, key removed, policy removed — so the invariant has a test that can fail. `⊘ [press b to show it] · policy: repo fathom.toml · episode folded (nothing but noise)` is the whole line, with the key and the policy placed before the description so a 40-column pane keeps the affordance.
- **Eight config keys, layered and explained like every other key.** `noise.suppress.{reformat,generated_churn,lockfile,vendored,user_glob}`, `noise.collapse.noise_only`, `noise.keep` and `noise.expand` join `e0.8`'s schema (64 keys now, generated schema and reference both CI-diffed); each boolean merges `replace` so a repository can actually say *no*, and every fold prints the layer that won, file and line. The session override is one keystroke (`b`, the key `e1.7` already binds), writes nothing — a test diffs the tree's bytes across the press — and cannot unblock a rule a repository deliberately turned off.
- **Scored, benched, and honest about the score.** `fathom noise score` gates 51 labelled pairs, 5 generated cases, 14 lockfiles and 10 episodes: pair accuracy **1.000**, fold recall **1.000**, episode collapse **1.000** against bars of 0.95 / 0.90 / 1.0 — and fold **precision** with an absolute bar of 1.0, because a false fold is one real change hidden, with each false fold also named individually. ~0.14 ms per pair, ~1.5 ms per episode (`fathom noise bench`), so suppression runs per file per repaint rather than per review.

### `e0.10` — the fixture corpus: deterministic repositories with hand-authored ground truth (reco-whw)

- **15 scenarios, 10 105 commits, 190 expected episodes, 83 expected findings — 25 of which say a rule must
  stay silent.** `fathom-fixtures` writes real Git repositories from hand-assembled object bytes
  (`git.rs`: loose objects, tree serialisation, a `DIRC` v2 index, `.git/shallow`, gitlinks) in named
  scenarios, and the whole corpus verifies in **42.5 s** in a debug build — `ten_k_commits` is 22.9 s of
  that, `huge_file` 12.6 s, the other thirteen under two seconds together. Every fixture is then handed to
  the real `git` binary: `fsck --strict` clean, `status --porcelain` empty, and
  `rev-list --all --count` equal to the manifest's commit count. `clean_baseline` exists because a corpus of
  only positives proves an analyser fires, not that it fires *because of* what it is looking at.
- **Two halves of a ground-truth file, with different owners.** `corpus/<scenario>.json` carries a machine
  block — digest, commit count, head, tips — that `--bless` alone rewrites, and a semantic block (episodes,
  findings, claims, purpose, consumers) authored by `tools/author_corpus.py` and reviewed by a human. That
  split is what stops the corpus being re-recorded into agreement with a regression, and `verify.rs` refuses
  incoherent expectations: overlapping episodes, findings naming commits that do not exist, paths no commit
  touched.
- **Determinism is a policy with a lint behind it.** One fixed instant, five committers at `example.invalid`,
  byte-order collation, no `SystemTime`, no `HashMap` order, no filesystem metadata in any digest, and
  absolute paths normalised to `<ROOT>` before hashing — asserted by a test that greps the crate for the
  banned constructs and by one that builds a scenario twice under two different paths and compares digests.
- **Event logs the contract, not a paraphrase of it.** `.fathom/events.jsonl` follows ADR-005's two layers
  (`docs/concepts/fixtures.md`), and its projection table mirrors `harness.py`'s `project()` row for row; the
  audit re-derives all 780 generated records through that reference function rather than restating the rule.
  No `test_run` is written, because §2.2 derives it — the fixture supplies the `cargo test` tool pair instead.
- **Amends ADR-002**: `fathom-fixtures` publishes no features (see §4.4), and adding a scenario is one
  function plus one ground-truth file, documented in `docs/concepts/fixtures.md`.

### `e1.8` — background materialisation: decode off the UI thread, bounded, cancellable (reco-myd)

- **The frame on screen is never the frame waiting for a read.** Hunk text is decoded by worker threads
  that serve the visible window first, the next eight hunks second and a guess third
  (`materialize.rs`: three priority tiers, one hard cap, generations, cancellation). A window whose
  hunks have not landed draws a stated hole at the row the hunk starts at — `⋯ hunk 11 (25 row(s)) is
  being decoded — this window is not complete`, `2 hunk(s) in this window still decoding` — and fills
  it in without moving anything else. Opening a **5 000-file** episode now paints in **209 ms**
  (1 557 680 patch bytes, debug profile: 209 ms of scan plus 0.29 ms of frame) against the 300 ms the
  card asks for, and the same test suite checks the shape as well as the clock: 50 tree windows cost
  14.4 ms at 400 files and 19.4 ms at 4 000, so a frame buys its height, not its index.
- **Two walls, both asserted by the workload that would breach them.** The decode cache is a byte-bounded
  LRU (8 MiB default): 60 hunks of 58 912 B against a 16 384 B cap leave **12 138 B held, 2 entries,
  30 evictions**, and a single hunk larger than the whole cap is refused with its own sentence —
  `one piece costs 24 576 B, over the 4 096 B decode cache` — rather than squeezed in. The queue caps
  *queued plus running* jobs (32 default): 1 000 wide requests produce **peak 16 in flight against a
  cap of 16**, 4 774 counted refusals, and 1 000 scrolls supersede **7 962** queued jobs, because a cap
  alone still finishes 32 decodes of work the reviewer has already scrolled past.
- **Scrolling cancels rather than outranks.** Each `(patch origin, path, shape)` lane has a generation,
  so a new window removes the old window's queued jobs and tells the running ones to stop; a cancelled
  decode writes nothing to the cache and wakes nothing. `tests/diff_materialize.rs` presses `]` three
  times with only the last window served and checks every intermediate frame twice over: nothing from
  a hunk outside its own window, and byte-identical to the inline render once its own answers arrive.
  A shape toggle is a cache miss for the same reason — the key carries the shape — so `S`/`=`/`W` can
  never draw last shape's rows.
- **The next episode is ready before `n` is pressed.** `DiffView::prewarm("next" | "previous", source)`
  scans the neighbour's patch and decodes its first 24 hunks on a worker; arriving there adopts the
  index and reads **0 lines from the patch** for the first frame (measured: 18 hunks warm, 0 pending,
  0 cache misses). The CLI's side of the seam is still empty and says so — `fathom-tui` is IO-free and
  `insight::episodes` is `e2.11`'s to answer.
- **The file tree stopped paying for files it does not show.** `TreeShape` (grouping, sorting, totals)
  is built once per index-and-noise-rule pair and shared; `Tree` draws the rows it is asked for, so the
  tree pane no longer rebuilds and renders all 5 000 rows to display 22 of them. Every golden frame in
  `tests/diff_golden.rs` is byte-identical across the change, and the inline decode path — `--plain`,
  a caller that wants the answer in the same call — keeps its window limit and behaves exactly as
  before. 28 new tests: `tests/diff_materialize.rs` (13, the four criteria) and `tests/materialize.rs`
  (15, the queue and the cache on their own, including a worker that panics, reports
  `WorkerMsg::Died`, and then does the next job).

### `e1.6` — the timeline view: the episode rail, its filters, and a cursor that follows the episode (reco-bjo)

- **The rail is the home screen, and it draws a window, not the index.** One row per episode —
  id, relative time, author badge, change size, a log-scaled sparkline, risk, and the review-state
  glyph — grouped by UTC day or agent session, and `Rail::lines` builds exactly the rows on screen:
  a 10 000-episode batch scrolls at **p99 1.8 ms** (worst 2.1 ms, 240 drawn frames, debug profile)
  while allocating **51 KB per frame** — the same 51 KB the same keystrokes cost at 500 episodes,
  measured by a counting global allocator (`tests/timeline_bench.rs`), so "constant memory" is a
  number and not an adjective. Rows arrive as a `Batch` from `fathom-cli`
  (`frame::Options::episodes`), never as a store handle: `fathom-tui` still opens nothing (ADR-002 §3).
- **Selection follows the episode, not the row.** The cursor's anchor carries id, the author+subject
  digest, the author+paths digest and the member SHAs, and a re-index or a rewrite re-attaches it
  through ADR-003 §6's tiers in order — so a rebase moves nothing, a reword rides tier 2, a squash
  rides tier 3 *and says it landed on a different episode*, and an episode that left is announced
  rather than replaced by a cursor on somebody else's row.
- **Filters compose the way the store composes them** — OR inside a dimension, AND across author, kind,
  review state, path area and a typed date range — and they are a value with a key
  (`timeline.filter`, `timeline.selection`, `timeline.members`), so `e1.11` can restore a run to the
  rows and the cursor it had. `F` opens the chooser as a prompt, which is why it added no keys: the
  surface spends the palette's existing chords and nothing for `keys.overrides` to get wrong.
- **The column ladder is the ADR's order, and the state glyph is the last cell standing** (§5.2, §6.3),
  with a fit rule underneath it that drops the risk number and then the change size rather than clip
  the glyph — `tests/golden/timeline/ladder.txt` prints every rung. Grouping counts what it groups and
  a collapsed group leaves a counted `··· 17 more` behind it, because silent suppression is the one
  failure a review tool is not allowed (ADR-001 §7.2).
- **Nothing is blank and nothing is invented.** An unranked episode prints `?`, an unknown author prints
  `?`, a dead episode prints `gone`, an empty rail names the verb (`run \`fathom index\``), a filter
  that selected nothing says so and names the key, and the card prints whose delta fields it is missing
  and which command will fill them. 37 behaviour tests, 14 goldens and 2 perf tests
  (`cargo test -p fathom-tui --test 'timeline_*'`), all from one deterministic fixture corpus.

### `e1.10` — the headless verbs: `index`, `show`, `stats`, `doctor`, `export` (reco-ld8)

- **`fathom index` grew the two flags ADR-004 §9.3 promises**: `--rebuild` (advance the epoch and re-walk;
  the epoch is settled on success and stays `building` if the run dies, which `fathom stats` then reports)
  and `--watch` (index once, then hand to `e1.9`'s watcher — `FEATURE_ABSENT` in a binary built without it).
  `--json` now carries `contract`, `rebuild`, `epoch` and per-analyzer `computes`, and names the store by a
  repository-relative path instead of an absolute one.
- **`fathom show <episode|rev>`** reads one episode out of the index — identity, authorship, message head
  and hash, trailers, the file rows with their `path_class`/`noise_class` decisions — and separates the two
  failure modes a user can tell apart: a rev that names nothing is `ERROR (1)`, a commit the index has
  never seen is `INDEX_DEGRADED (12)` with `fathom index` as the remedy. The handle is anything
  `revspec` resolves; episode identity of its own is `e1.6`'s.
- **`fathom stats`** is repo scale (`git_commit` aggregates: commits, files, authors, churn, date range,
  noise and change-kind histograms, top authors), index state (path, size, WAL, schema stamps, epoch and
  epoch status, `quick_check`/`integrity_check`, ref tips, the last run and how many never closed) and cache
  hit rate over the store's whole life — the same counters `fathom index --stats` prints, read through the
  same function, so the three cannot tell different stories (ADR-004 §11).
- **`fathom doctor` became the report ADR-002 §6.4 and `e1.10` ask for**: build and platform, terminal, the
  SQLite version with ADR-004 §4.0's capability probe (probed by *using* `STRICT`/JSON1/FTS5), the
  repository health report with its remotes, the index and cache sections shared with `fathom stats`, the
  config layers as path/size/mtime (never values), the `.fathom/` inventory, and the watcher's heartbeat.
  **Every finding carries a remedy, and `doctor` exits 0 unless `--strict` is passed**: a diagnostic that
  exits non-zero makes the diagnostic the failure. `--deep` adds the whole-database `integrity_check`.
- **No secrets, no absolute home paths** (`sanitize.rs`): paths are repository-relative or `~`-relative,
  URL userinfo is replaced with `***`, and config files are reported as digests-of-shape rather than
  contents. The test plants a token in a config under a fake `$HOME` and a password in a remote URL, and
  asserts neither appears in any of the four `doctor` renderings.
- **A new published exit code, `12 INDEX_DEGRADED`** — the index is missing, stale, incomplete, has a run
  that never closed, is still mid-rebuild, or holds nothing while HEAD does not. Added through the sanctioned
  door (ADR-002 §7.3, `tools/workspace_model.py`, regenerated in the ADR) rather than invented in a verb.
  The ticket's four classes (ok / user error / environment / degraded index) are mapped onto the published
  table in [`docs/concepts/cli.md`](docs/concepts/cli.md); `2` stays `USAGE` and `3` stays
  `NOT_A_REPOSITORY`, because renumbering a published contract is a worse bug than the ambiguity it fixes.
- **`fathom export` is a real skeleton now**: the format list is the façade's, `--out` and the new
  `--episodes RANGE` are parsed and *resolved* against the repository, and `--json` states the format,
  destination, scope and `writes_file: false` before `e5.7` renders anything.
- **Schemas and their round trip**: `schemas/{common,index,show,stats,doctor,export}.schema.json`
  (draft 2020-12, `$id`s under `https://fathom.dev/schemas`, shared `$defs` so `stats` and `doctor` print
  one definition of "the index"), each documenting its `x-fathom-exit-codes`.
  `crates/fathom-cli/tests/schemas.rs` runs each verb, validates the payload, re-serialises and validates
  again, with a keyword-subset validator in-tree — a dependency this would need is a heavier dependency than
  the subset is worth. `tests/test_cli_contract.py` checks the schemas' codes are the published ones and
  that every finding the code can produce is documented.
- **`--help` is snapshotted** (`crates/fathom-cli/tests/snapshots/help.txt`, `SNAPSHOT=1` to regenerate),
  and a test asserts the flags the ADRs tell people to type (`--rebuild`, `--watch`, `--json`, `--episodes`)
  are the flags `--help` documents.
- **`fathom-store` gained `inventory.rs`**: `integrity_check(deep)`, `scale()`, `run_summary()`,
  `cache_inventory()`, `pending_derivation()`, `ref_tips()`, `indexed_oids()` and `sqlite_provenance()`, so
  the aggregates behind two verbs are one read each and no SQL leaks into the composition root (ADR-002
  §4.3). `clock.rs` is now unconditional: reporting the world needs the instant the report describes.

### `e0.3` — ADR-003: the Episode — identity, segmentation, boundary tie-breakers, stability across rewrites (reco-ubw)

- **The identity rule, decided and published** ([docs/adr/0003-episodes.md](docs/adr/0003-episodes.md)):
  an episode is addressed by `sha256("fathom-episode/1" 0 author 0 normalised-subject 0 sorted-path-set)`
  — the three facts a rewrite preserves — and **never** by the member SHAs, the parent boundary, the tree
  or the dates, which are what a rewrite changes. That is invariant **I1** (identity is stable across
  `git rebase -i` of the same content) and it is why `--amend`, `rebase -i` and a force-push no longer
  orphan a sign-off. The ticket's "member SHAs plus parent boundary" is kept as the episode's **extent**
  (§3), which is what the diff endpoint and the rework detector need, and ADR-001's glossary was amended
  in the same change rather than redefined locally.
- **Three exact digests instead of one fuzzy fingerprint**: `fingerprint` (author+subject+paths, the id),
  `fallback_fingerprint` (author+subject — survives an amend that adds files, a squash that keeps the
  subject) and `shape_fingerprint` (author+paths — survives a reword, including mid-rebase). §13.1 declines
  similarity-based re-linking for v1 and names the measurement that would reopen it: a guessed link
  attaches a human sign-off to the wrong change, silently.
- **The closed boundary cascade**, in the order that was already in `decide()` (a human `force_split`
  outranks harness evidence; turn evidence outranks every git heuristic; the graph outranks the author;
  the clock outranks the subject line), with **TB1–TB10** tie-breakers — one rule recorded per boundary,
  no blending, same-store-same-episodes, and twin ids (`ep-x`, `ep-x-2`) for the same change landing twice.
- **Re-link tiers RL0–RL7**: dead→live only, never onto an episode that already has a verdict, one claim
  per live episode, closest-in-time inside a tier, every carry written to `episode_rewrite`. Worked
  examples for amend, amend `--reset-author`, rebase, reword, `fixup!`+squash, PR squash-merge, merge
  trains, cherry-pick twins, backports, dropped commits and splits (§5). Where all three tiers fail — a
  rebase that reworded *and* moved files — the rule is **fail-visible**: the episode comes back `unread`
  rather than inheriting a decision it cannot prove.
- **Six places the code moved instead of the prose approving it** (§14): the `segment.rs` module doc listed
  a different cascade from the code (the code's won); `created_source='index'` was stamped at 0.7 where
  ADR-004 §4.3 publishes 0.3 (now 0.3); the compiled-in gap was 45 minutes where three published places
  said 30 (now 30); `store/git.py` addressed episodes by member oids and fingerprinted them by tree oids —
  the scheme §2 rejects (now the ADR's digests, same framing, same prefix); the config enum spelled two
  rules `time`/`merge` where the store stores `time-gap`/`branch` (one vocabulary now, App. B); and the
  published `episode_rewrite.method` CHECK lacked `shape`, which the relink writes.
- **A second implementation on purpose**: `src/recommender/segment.py` re-derives the digests and holds the
  closed vocabularies, and ADR-003 Appendix A publishes the golden vectors both must reproduce
  (`identity_matches_the_published_golden_vector`, `test_the_published_vectors_in_appendix_a_are_reproducible`).
  `tests/test_episode_identity_contract.py` (19 tests) parses the ADR's own tables and diffs them against
  both implementations and both schema copies — cascade order and prices, relink tiers, `created_source`
  prices, the gap default in four places, the CHECK vocabularies, the new glossary nouns and the docs'
  status lines.
- **Also decided here**: a partial tree is a provisional episode and nothing durable may be keyed to it —
  no sign-off, no finding, no metric denominator (§7); a human commit between agent commits is its own
  episode and splits the agent's work in two, with the turn-evidence and `force_merge` remedies named (§8);
  the boundary order is **not** configurable, and `episode.boundaries` now restates it rather than
  pretending to control it (§4.5); `lineage_seq`, not `created_utc`, is the canonical successor order the
  metrics read (L2).
- **Fixed in passing**: the CHANGELOG's opening paragraph had swallowed a later ticket's section as its own
  link text (the `e1.10` heading is a `###` under `[Unreleased]` now, where it belongs).

### Added

- **The cargo workspace** (`e0.7`/`reco-e77`): the eight crates ADR-002 §3 declares — `fathom-core`,
  `fathom-store`, `fathom-ingest`, `fathom-analyze`, `fathom-tui`, `fathom-mcp`, `fathom-fixtures` and
  the `fathom` binary (`fathom-cli`) — as compiling stubs with the published feature list, the pinned
  dependency set and a committed `Cargo.lock`.
- **`fathom --version`, `fathom --help` and `fathom doctor`**: the binary prints its resolved feature
  set (ADR-002 §6.4), refuses unimplemented verbs by naming the ticket that owns them, and leaves with a
  code from the published exit-code table (§7.3). `fathom doctor --privacy` and `--json` report which
  `net-*` features are compiled in and which consent key would unlock each.
- **`fathom_core`'s published contracts**: `exit::ExitCode` (the twelve-row table), `error::ErrorCode`
  plus the `Diagnosable` trait, `time::Timestamp`/`Clock`, and the `privacy::NET_FEATURES` consent table.
  All four are checked against `tools/workspace_model.py` from both directions: Rust tests, and
  `tests/test_workspace_contract.py` for the Rust source against the model.
- **CI**: `.github/workflows/ci.yml` — `cargo fmt` and `cargo clippy -D warnings` as hard failures,
  `cargo deny` over the lockfile, build and test on ubuntu/macos/windows on `stable` and on the MSRV,
  the headless `--no-default-features --features minimal` binary on every leg, a `beta` leg allowed to
  fail, and a `cargo dist generate --check` gate. `.github/workflows/release.yml` is
  `cargo-dist`-generated; `pr-run-mode = "upload"` builds every target for every PR.
- **The façade's paging rule** (`fathom_analyze::insight::PageWindow`) and the runtime network-consent
  gate (`fathom_mcp::transport::check_consent`), both before their owning tickets, because both are the
  answer to a question three crates would otherwise answer three ways.
- **`fathom_ingest`'s git read layer** (`e1.1`/`reco-cey`): repositories are found and read without
  running `git` at all. Discovery from a path upward — worktree, bare, linked worktree, submodule,
  detached and unborn `HEAD`, unicode paths, ceilings and trust — the ref store, `HEAD`'s three states,
  and revision expressions (`main~3`, `HEAD^2`, `v1^{}`, `v1^{tree}`, short ids; anything outside that
  grammar is a `RevSpecRejected` that names the grammar instead of guessing). Commit-graph walks take
  first-parent, include/exclude, date-window and limit/skip options and are checked against
  `git rev-list` commit-for-commit, and the working tree's staged/unstaged/conflict state comes from
  `.git/index` plus the filesystem, with the stat shortcut and its one documented blind spot stated in
  the API. A shallow or filtered clone's absent objects are a first-class state — `ObjectUnavailable`
  and `HistoryTruncated` naming the object and the reason, with the remedy "fathom never fetches" — never
  a panic and never an empty answer presented as a whole one. `backend::Reader` is the seam a later
  ticket puts a `git`-binary fallback behind, and `fallback_eligible` is the whole policy: two failures
  may reach a second backend, and a missing object never does. Measured: a 100k-commit first-parent walk
  streams in 441 ms with a 30 MiB peak-RSS delta
  (`benchmarks/reports/e1-1-git-walk.json`); `tests/no_git_subprocess.rs` re-executes the test binary
  behind a fake `git` on `PATH` and fails the build if the default path ever reaches for it.
- **Commit ingestion and normalization** (`e1.2`/`reco-76g`): a commit becomes a `CommitRecord` — oid, tree,
  parents, author and committer with their offsets, message head/hash/byte-length, parsed trailers, and one
  row per path touched: change kind, old and new path as git stores them (bytes, git-quoted in JSON), modes,
  blob ids, binary-or-not-examined-and-why, added/removed line counts, git-style `@@` hunk headers, and a
  rename candidate carrying git's own similarity score. The record is the contract: `export::to_json`
  produces canonical bytes — commits sorted by id, header naming the caps and merge policy that produced
  them — and `tests/golden/e1-2-commits.json` is 500 of those commits, byte for byte, over a fixture that
  rotates through add, modify, delete, rename, binary, mode-only, multi-file, trailer blocks, a non-UTF-8
  path, an over-cap blob and two merges. The caps are values rather than prose: a blob's size comes from its
  object header, so a file over `ingest.file_size_cap.mib` (8 MiB) is turned away *before* it is inflated and
  a 32 MiB blob costs a header read and a few KiB instead of 32 MiB of RSS (`tests/memory_cap.rs` measures
  the difference rather than asserting it); an over-cap file is recorded as unexamined with its size, never
  counted as zero; hunk lists stop at 256 entries with the true total kept beside them; and a commit that
  touches more paths than `files_per_commit` sets `files_truncated` instead of quietly looking small.
  Per-commit work runs on rayon with one repository handle per worker and results scattered back by index,
  so one worker and sixteen emit identical bytes — which a test asserts, because "deterministic" is not a
  property you can eyeball. A run is idempotent and resumable: the sink names the oids it already has, each
  batch is one transaction, parents are written with `ON CONFLICT DO NOTHING` (ADR-004 §4.0), a crashed run's
  `index_run` row is stamped on the next open, and a second run of the same repository writes zero rows.
  Measured: 500 mixed commits walk, normalize and store in 30 ms against the 1 500 ms `cold.index.ms.small`
  budget — 1 220 B/commit against the 4 096 B byte model, a 7 MiB peak-RSS delta, 3.6× from rayon at 10k
  commits (`benchmarks/reports/e1-2-ingest.json`).
- **`fathom_store`'s spine** (`e1.2`/`reco-76g`): `spine::Store` carries ADR-004 Appendix A's DDL verbatim —
  `meta`, `index_run`, `ref_tip` and the STRICT `git_commit`/`git_commit_file` tables with their indexes —
  with §7's pragmas applied on every open (WAL, `synchronous=NORMAL`, `foreign_keys=ON`, `busy_timeout`,
  an 8 MiB page cache) and a behavioural probe that fails at open, not later, if the linked SQLite cannot do
  STRICT tables or JSON1, because a store that silently stores less is worse than one that refuses to start.
  Run bookkeeping (`begin_run`, `finish_run`, `recover_runs`), `known_oids` for the resume path, and
  `check`/`counts` for `doctor` live there too. `fathom-ingest` still never names SQLite: the sink that
  connects them is in tests today and moves to the composition root at `e1.10` (ADR-002 §4.2).
- **The change classifier** (`e2.1`/`reco-dsh`): every changed path in an episode gets one of the twelve
  taxonomy classes, a confidence, a band and one sentence saying why — `src/recommender/change/`
  (`markers`, `evidence`, `policy`, `engine`, `episode`, `cache`, `corpus`, `eval`, `explain` pure; `cli`
  the only half that touches a file, ADR-002 §4.5). The `e0.9` path table is consulted first and never
  rewritten: with content evidence off, the answer is byte-for-byte `e0.9`'s on all 207 fixture rows and
  on all 235 of the taxonomy's own, which is a test rather than an intention. Content evidence is layered
  *above* the table in a fixed order — an assertion in a repository's own `fathom.toml` (0.98) outranks
  everything, then the path conventions no header can refute (`vendor/`, lockfiles, 0.97), then byte facts
  (NUL/undecodable, container magic, 0.94), then 37 decisive markers and parsed shapes (generated-code
  banners, a resolver's lock header, `-- +goose Up`, OpenAPI, test attributes), then the table by
  specificity, then suggestive hints — and two claims within 0.06 with neither decisive is answered
  `other`, because guessing `source` inflates the reviewable line count and *lowers* the noise share,
  which is the failure mode that makes a risk score lie. `fathom change explain <path>` prints the
  receipts (the marker that won with the line it matched, the rule that agreed, the loser and its score)
  and `--json` prints them structurally because a pane must not parse prose; `fathom change episode` adds
  the per-change-set aggregates. Cost is bounded by construction — markers read only the first 4 KiB/40
  lines, so a committed 400 MB model costs what a 2 KB source file costs: 70 µs/file cold, 38 µs cached,
  1 000 files in ~70 ms. The cache is `change.classify`, `blob`-scoped and keyed by path **and** blob
  **and** the taxonomy digest **and** the marker digest **and** the classifier version, because a stale
  classification is invisible; payloads are canonical JSON with no clock in them, so two processes emit
  identical bytes (a test runs the corpus twice and diffs). CI gates on the number: `fathom change score
  --min 0.95` scores 207 labelled paths over 16 repo shapes (100% today) and also reports the same set
  scored from paths alone (91.8%, 17 rows content-decided), because a fixture set a content-blind
  classifier could pass is not testing this ticket. No new store columns: `cls`/`noise`/`taxonomy_version`
  map onto ADR-004 §4.2's three, and the confidence and the reason live in the analyzer row instead.
- **The dependency surface** (`e2.5`/`reco-j83`): `fathom_analyze::deps` parses the whole dependency
  surface of a revision and of a diff across eight ecosystems and twenty-five formats — `package.json`
  with `package-lock.json` v1–v3, `yarn.lock` v1 and Berry, `pnpm-lock.yaml`; `Cargo.toml` and
  `Cargo.lock` v1–v4; `pyproject.toml` (PEP 621, Poetry, PEP 735 groups, uv sources) with
  `requirements*.txt`, `poetry.lock`, `uv.lock`, `Pipfile.lock`; `go.mod`/`go.sum`/`go.work`;
  `Gemfile`/`Gemfile.lock`; `mix.exs`/`mix.lock`; `pom.xml`, Gradle build files and `gradle.lockfile`;
  `*.csproj`, `Directory.Packages.props` and `packages.lock.json` — plus the two workspace config files
  that declare membership without declaring dependencies.
  A scan answers per *member*, not per repository: `apps/web` and `apps/admin` declaring their own
  `react` is two requirements, each lockfile is bound to the package it locks, and `claimed` says
  whether any workspace glob covers a member — the half-migrated monorepo where nobody's
  `workspaces` list mentions `experimental/sandbox` is a note, not an absence.
  **A parse failure is a value.** A row whose status is `unparsed` carries one of eight typed reasons
  (`malformed_syntax`, `truncated`, `too_large`, `not_text`, `feature_absent`, `empty`,
  `unknown_shape`, `content_unavailable`), the parser's own words with a line number when it has one,
  and a preview of the raw text — the "here is the raw diff" half of the degradation rule, which is
  why a binary file still gets a `·`-sanitised caption rather than nothing. Nothing in the module can
  drop an episode: one corrupt lockfile in a ten-file surface yields nine parsed rows, one unparsed
  row and a total that still adds up. Notes come in two kinds — a *deficiency* makes the file
  `partial`, an *observation* (a yarn entry answering two specs, a package locked under a marker) does
  not, so a healthy `yarn.lock` is not `partial` for being normal.
  Nothing resolves, no build DSL is executed and no network is touched: Gradle's `${spring.version}`
  and a `Gemfile`'s Ruby stay verbatim, because running a repository's build code inside a review tool
  is how the reviewer becomes the attack surface.
  Each ecosystem is its own cargo feature (`deps-node` … `deps-dotnet`, aggregated by `deps-all`, which
  the shipped binary enables); a build with any subset — including none — compiles warning-free and a
  gated-out ecosystem answers `feature_absent` instead of failing the scan.
  Correctness is pinned by 28 hand-written golden fixtures (one per format, compared through a
  projection rather than a struct dump, with a test that fails if any claimed format lacks its fixture),
  a tolerance matrix that runs every fixture through 12 degradations (348 scans: no panic, one row
  each, typed reason, prefix-of-the-file previews), monorepo and real-`git` revision/diff tests, and a
  bench that scans an 8 MiB synthetic monorepo — ~90 ms in release against the ticket's 500 ms budget,
  with a second assertion that 8× the lockfile does not cost 8× the time squared.
  See [docs/concepts/dependency-surface.md](docs/concepts/dependency-surface.md).
- **The dependency delta** (`e2.6`/`reco-kvs`): the delta half of `fathom_analyze::deps` diffs two *resolved graphs*,
  not two lockfile texts, and every row carries the evidence for its own claim. A `State` is one revision
  as a graph plus the declarations that requested it, built from either a cold surface or the changed files
  of an episode; a `DependencyDelta` is the change between two of them — new and removed **direct**
  dependencies, upgrades, **downgrades**, version changes that cannot be ordered, transitive-only moves,
  resolution-graph size per ecosystem, new or vanished registries and git/vendored/path sources,
  integrity changes for an unchanged version, licence-set delta, and newly-contradicted peer ranges.
  Direct and transitive are properties of the *change*, each with the evidence it came from — lockfile
  statement (`DEPENDENCIES`, NuGet's `"type"`, npm's importer block), manifest declaration, or a resolved
  edge — and the three rules that make that honest are enforced: a lockfile that contradicts its manifest
  wins and the contradiction is a note, a Go `// indirect` requirement is not direct, and where the
  lockfile records no edges (a `go.sum`, a Gradle lockfile) nothing is called transitive, it is `unknown`
  with a note naming the file and the reason.
  **A bump storm is a pattern, not a line count.** Twelve refreshed transitive packages whose root the
  episode also changed are *explained*; twelve with no direct cause are a mass refresh, and one new direct
  dependency alongside an unexplained refresh reports as both (`mixed (bump storm)`) rather than collapsing
  into "13 packages changed". Source *identity* ignores git refs, so moving a commit is not a new source
  while naming a new URL, registry or path is, and a package that switched kind (`registry → git`) is a
  supply-chain event with its own row.
  Every printed row points at the lockfile hunk that proves it: a line-level unified diff per changed file
  (`deps::locate_hunk`), located by name *and* version and excerpted round the proving line, because in a mass
  refresh those are a hundred lines apart. `deps::render_table` prints the per-episode table with its
  drill-down, or the same thing as JSON.
  Pinned by 10 two-revision golden fixtures across eight ecosystems (each compared through a projection
  *and*, for two of them, the rendered table), named tests for each acceptance claim — downgrades, source
  switches, integrity changes, peer conflicts, "who asked for this", the bump-storm distinction — plus a
  real-`git` episode test, a version-grammar suite covering the six range grammars the fixtures actually
  use (including the `~1.2`/`~> 1.2` difference, which is a different rule and not a typo), and a check
  that a proof which claims a hunk really mentions its package.
  See [docs/concepts/dependency-delta.md](docs/concepts/dependency-delta.md).
- **Incremental indexing with a tree-hash analysis cache** (`e1.4`/`reco-f28`): `fathom index` indexes.
  `fathom_analyze::index::run` is three passes — the spine through `ingest::run`, the analyses, the
  working tree — and there is no invalidation code in it, which is ADR-004 §2 rule 3 rather than an
  omission: every derived row is keyed by the nine inputs that produced it (§5.3 — analyzer id, input kind
  and input hash, analyzer version, dependency digest, config digest, schema version, store schema, epoch),
  so editing one file changes one blob, which changes one `path_pair` digest, which misses while every
  other key hits. `structure.delta` is at `tree` scope and keyed by the tree OID itself, so two commits
  that reach the same tree are analysed once and a revert is a hit; its payload is the file inventory,
  derived by merging the parent tree's cached inventory with this commit's changed paths (`derived:
  "merged"`) and falling back to a full tree enumeration (`derived: "full"`) when the parent's payload is
  absent or unreadable — a bad read degrades to re-derivation, never to a wrong answer (§7.6).
  The warm path drives from the spine's `git_commit_file` rows rather than from a second two-tree walk:
  normalize has already walked both trees, and it is the only place rename detection exists, so a
  tree-comparison-driven warm path would score every `git mv` as a delete plus an add — a test asserts a
  move is one analysis with `change_kind = rename`. `fathom_ingest::diff` publishes the two-tree walk,
  blob measurement under the size caps and working-file measurement anyway, for consumers with no spine
  to read from and as the independent cross-check (against `git diff --raw`) that those rows say what git
  says.
  Migration 2 adds `analyzer`, `cache`, `blob`, `blob_ref` and `counter`; the analyzer manifest is
  compiled-in and re-registered on every open, so a store can never answer a lookup with yesterday's
  version. One `put` is one immediate transaction over blob, blob-ref and cache row, so a `kill -9` can
  strand an unreachable object file (content-addressed, prunable) but never a stranded row; the index's
  `index_run` row is closed only after the analyses, so §9.2's `running → crashed` stamp stays the truth
  and the next run's derivation list — this run's commits plus a crashed run's, bounded to exactly one
  run's worth by the first later clean run — finishes what was interrupted. Counters are summed in memory
  and flushed once per run (§7.3), `fathom index --stats` prints them per analyzer as the delta the store
  itself holds, `--json` prints the same on stdout and nothing else there, and `--refresh` looks up every
  stored unit so §6's invalidation matrix is measured rather than trusted (a version bump moves that
  analyzer and everything downstream of it, a config key moves only the analyzer that declared it, a
  schema bump makes every row unreachable without migrating any of them — each of those is a test).
  The clock moved to `fathom_cli::clock`, which is now the only `SystemTime` read in the workspace.
  Measured on the 500-commit fixture: cold index 1.4 s debug, warm re-index after one new commit **8 ms in
  release against a 150 ms p95 budget** (`warm.reindex.ms.small`; 47 ms in a debug build), one commit
  looked up, one `path_pair` computed, one tree computed, and a run with nothing new performing zero
  lookups. The budget assertion is `#[ignore]`d and runs under `--release --ignored`, the house rule for
  wall-clock tests (`fathom-ingest`'s bench says why: a debug build is three to ten times slower than the
  release build the budget was written against). New features `fathom-analyze/{sqlite-bundled,sqlite-system}`
  forward to `fathom-store`'s, because the index names both crates and ADR-002 §6 forbids forcing SQLite
  into the `minimal` binary; `fathom index` in a binary built without them exits 9 naming the feature.
- **Watch mode v1** (`e1.9`/`reco-949`): `fathom watch` notices a commit made in another terminal, names
  it, and says what it is doing about it. `fathom_ingest::watch` keeps the three jobs apart because they
  fail differently — *be told* (`source`: `notify`'s per-OS backend, a debounced poll, and the fallback
  that swaps one for the other when the OS refuses a watch or quietly never fires one), *work out what
  moved* (`observe`: a `stat` fold over `HEAD`, `packed-refs`, the loose refs, the reflogs, the index and
  the guard markers, which buys the expensive ref snapshot only when it moved), and *decide whether to act
  now* (the guards: a rebase/merge/cherry-pick/bisect/`am` in progress, a `gc`/repack/fetch storm, ref spam
  from a background process, a repository whose inode changed under the watcher). Indexing is not in the
  crate — `fathom-ingest` sits below `fathom-analyze`, so a caller implements `Job` and the watcher calls
  it, which is also what lets the whole machine be tested with a scripted job and no store at all.
  The visible states are `idle → detecting → indexing (with progress) → error → stale`, and every arrow out
  of a busy one is bounded by a deadline that names itself in a notice (`settle_max`, `throttle_max`,
  `stale_after`): a rebase waiting on a human, a `git gc` and a corrupt store are three different
  sentences, and none of them is a spinner. Failures back off and then go stale, and the run that succeeds
  after them says so rather than quietly correcting the record.
  The CPU budget is enforced rather than hoped for: the poll stretches itself toward `poll_interval_max`
  when the measured busy share exceeds `watch.budget.cpu_pct`, and the expensive side runs in
  `--chunk-units` slices with a progress line between them, so a long backlog is a series of
  short runs and not one long block. Measured on a quiet repository: **0.12% of one core in release
  (0.15% debug) against `watch.idle_cpu.pct`'s 1%**, read off the watcher's own meter rather than a
  stopwatch, and the meter charges the render and the heartbeat write too — the part that runs every
  single second. `fathom doctor` gained a `watch` section (`--root`, and `watch` in `--json`): the backend
  this filesystem would get and the `/proc/mounts` type that decided, plus the heartbeat of a running
  watcher — its phase, its *actual* backend and the OS's reason, which is how a fallback that already
  happened gets reported rather than inferred. `.fathom/watch/status.json` is that heartbeat: written by
  rename so no reader sees half a file, rewritten on a one-second beat as well as on a change so a reader
  can tell "idle" from "this process died", and read back by `doctor`, which calls a heartbeat whose pid
  is gone *stopped* — the `kill -9` case — rather than alive. `--once`, `--max-cycles` and `--deadline`
  are the script seams; `--json` is one object per state change and one closing object naming why the run
  stopped.
  Acceptance, as tests: a real commit made by a second process reaches the state machine and the store, and
  the `fathom index` run after it finds nothing new to do; the 1 s detect budget is asserted release-only
  from `benchmarks/budgets.json`, the house rule for wall-clock assertions; rebase, `gc`, repo replacement,
  ref spam, a silent notifier and a failing index are each produced the way git produces them
  (`fathom-ingest/tests/watch.rs`, 21 tests — 20 in a normal run plus the release-only budget — and
  `fathom-cli/tests/cli.rs` for the process boundary).
  Deliberately not v1: indexing the uncommitted working tree is opt-in (`--worktree`), because staging churn
  is constant while an agent works and this ticket's promise is about commits and refs; and ctrl-c keeps its
  default disposition — `fathom-cli` forbids `unsafe`, so the signal handler belongs to the daemon process
  that will own the terminal (`reco-kvk`), and until then a killed watcher's heartbeat is *reported* as
  stopped instead of pretending.

### Toolchain

- Declared MSRV **1.97.0**, edition 2024, `resolver = "3"` (ADR-002 §2), pinned for local builds in
  `rust-toolchain.toml`. Observed while landing `e0.7`: `rustc` 1.99.0 stable, and the whole workspace
  — including `rusqlite`'s bundled SQLite and the six tree-sitter grammars under `--all-features` —
  resolves and builds on 1.97.0 with no entries in
  `[workspace.metadata.fathom.msrv-exceptions]`.

### Fixed

- **A repository root of `.` is a repository** (`fathom_ingest::repo`). The lexical path collapse that
  makes `common_dir` printable was also eating the whole path when it *was* a `.`: `Repository::discover(".")`
  reported a working tree at the empty path, and the working-tree reader answered `fathom index .` with
  "the worktree is missing: nothing at " — a failure with no path in it and no remedy a user could act on,
  on the exact command line they get by typing the verb with no arguments. A relative "here" now stays a
  relative "here"; `fathom index .` indexes and `fathom watch` (whose root defaults to `.`) watches.
- **The ADR-005 FFI probe no longer reads cargo's build output as a source-tree fact**
  (`src/recommender/harness/elf_probe.py`). The probe asserts the Python contract has no compile step by
  globbing the repository for `.so` files; `e0.7` put a cargo workspace in the same tree, so the first
  `cargo build` wrote proc-macro `.so` files into `target/debug/deps/` and `python3 -m pytest -q` — the
  suite that gates ADR-005 §10 — began reporting "compiled extensions present in tree" about a tree with
  no compile step at all. The scan now skips ignored build-output directories (`target`, `dist`,
  `build`, `.venv`, `node_modules`, `.git`); a *committed* extension anywhere else is still a failure.
  `tests/test_harness_contract.py::test_probe_is_a_startup_gate` now passes in a built tree, which it
  could not before the workspace existed.

### Notes for whoever builds on this

- `fathom-store`'s `rusqlite` is gated behind `sqlite-bundled`/`sqlite-system` rather than
  non-optional as ADR-002 §3's table implies; the reason and the consequence are in
  [ADR-002 §10.3](docs/adr/0002-workspace.md) and in `crates/fathom-store/Cargo.toml`.
- `fathom-mcp`'s `net-http-transport` is a marker feature with no server crate behind it yet — every
  candidate links `hyper`, which `deny.toml` bans unconditionally. `e6.4` owns that choice.
- `package.repository` for `fathom-cli` is a **placeholder** (`https://github.com/danielhstahl/fathom`);
  `cargo-dist` refuses to generate without one. Replace it when the public remote exists — the installer
  scripts derive their download URLs from it.
- cargo-deny 0.20 removed `[licenses] deny`, so copyleft refusal now lives in the allow-list plus rule
  `D03`; see ADR-002 §10.3.
- `benchmarks/budgets.json` has no `cold.walk.ms` or `rss.walk.peak_mib` family, so the e1.1 walk budget
  (2 s at 100k commits) lives in the ticket and in
  [`benchmarks/reports/e1-1-git-walk.json`](benchmarks/reports/e1-1-git-walk.json) rather than in the
  table. The next budget round should promote it: `e1.2`'s indexer is the consumer that needs a number to
  regress against, and a walk is the first thing every `fathom` command does.
  `e1.2` did not need a new family — its budget is the existing `cold.index.ms.small`, and the caps it has to
  respect are `ingest.file_size_cap.mib` — but `rss.ingest.peak_mib` is still missing, and the ingest memory
  bound is currently asserted in a test (`tests/memory_cap.rs`) rather than in the budget table.
- The per-file taxonomy in a `FileRecord` is a real column set with a placeholder answer: `Taxonomy` is a
  trait, `Unclassified` is what the pipeline uses, and `taxonomy_version` travels in the export header so a
  golden file says which classifier produced it. `reco-dsh` fills the trait in; it must not add columns.
  Likewise copy detection (`git log -C`): `Options` enables rename pairing but not copy tracking, because
  gix's copy tracking *consumes* the source modification row that git's `-C` still prints — a copy is a row
  `e2.x`/`reco-a0v` should add deliberately, not one that silently replaces a modify.
