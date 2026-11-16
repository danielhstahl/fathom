# `reco-374v.1` — the CI triage census, and how to make it again

Two files, no dependencies beyond the standard library:

* **`ci_census.py`** — fetches, classifies, counts, and prints.
  * `fetch --cache DIR --since YYYY-MM-DD` pulls the run list for the window and then the job/step payload of
    every red run. Rate-limit aware: on `X-RateLimit-Remaining: 0` it sleeps to the reset and resumes, and
    every response is cached on disk by request path, so a re-run is free and a partial cache is still usable.
    With `GH_TOKEN`/`GITHUB_TOKEN` in the environment it also stops being limited to 60 requests an hour.
  * `fetchids --cache DIR --ids FILE` is the same payload fetch over a list of run ids, which is how a pass gets
    sized to the rate limit: red runs first (they carry the classification), then the green runs that cost
    minutes.
  * `census --cache DIR [--repo .] --out benchmarks/reports/ci-triage-reco-374v.json` joins the payloads with
    the trigger/job/step shape of the workflow files and writes the machine-readable census: per workflow, per
    job and per step — runs, reds, duration mean/p95/max, minutes consumed, the rust-cache restore/save proxies,
    every red with its causes and its class, the same-commit re-run analysis, the duplicated gates and the cut
    list. `--repo` lets it ask `git show --name-only` what a red's commit actually touched, which is how
    "this gate went red on a commit that cannot move it" is shown rather than asserted.
  * `report` prints the summary; `tables` prints the four tables that live inside the recommendation document;
    `full` prints the census complete — every workflow, every job and every step that ran, with run count, red
    count, mean / p95 / max duration and minutes consumed, plus the cache proxies — and its output is committed
    here as [`tables.md`](tables.md); `splice --census X --doc Y` writes the document's tables into
    `.plan/ci-triage-reco-374v.md` between its `<!-- BEGIN generated:… -->` markers, so no number in the
    recommendation is hand-typed.

The classification is a **rule table at the top of the script** (`CLASS_RULES`), one row per distinct failing
step, each with a class, a confidence, a `needs_log` flag and the evidence that put it there — a commit that
touched no Rust, a step that passed two runs earlier, a CHANGELOG entry from the ticket that fixed it. Argue
with a classification by editing a row and re-running; the tables in the recommendation document regenerate from
it. `DUPLICATED_GATES`, `CUT_LIST`, `TRIGGERS` and `WORKFLOW_SHAPE` sit beside it for the same reason.

## Known gaps, restated from the census' own `limits`

Job logs and the `Swatinem/rust-cache` hit-rate line need a token, so 17 of the 83 reds carry at least one cause
flagged `needs_log` and the cache numbers are proxies (restore-step wall time, and whether the action's `Post`
step ran at all). The run list returns only the newest attempt of a run, so a retried red is invisible as a red.
Green runs' job payloads were fetched only as far as the rate limit allowed, so a workflow that was green every
time shows `—` in the runner-minute column and its cost is the wall-clock columns instead; every red run in the
window has its full job and step payload.

The window is two pull requests and 19 commits, all of it from the week the loop spent repairing the actions.
Any rate in the recommendation is a rate over that, and the recommendation says so where each percentage is used.

## Added at the epic's close (`reco-374v`)

* **`ci_gate_map.py`** — the other half of the census: not runs and reds, but the commands every workflow
  *executes*, at any revision. `summary` sizes the fleet and the pull-request path; `map` prints each command
  with the steps that run it; `dupes` prints the commands run in more than one step; `diff --from REF` reports
  which gate families (tool + verb, so a gate that survived with one more flag is the same gate) were dropped,
  moved or added. It reads the workflows with `tools/check_workflows.py`'s own parser, so "executed" means
  `run:` / `uses:` / `with:` and never a comment — the same definition rule `C01` uses.
  ```console
  $ python3 .plan/evidence/reco-374v/ci_gate_map.py summary --ref 6c6a838
    files      13
    pr_files   9
    jobs       32
    pr_jobs    28
    steps      255
    pr_steps   222
    gates      157
    pr_gates   142
  ```
* **`dedupe_proofs.py`** — the epic's deletions, re-wired back into a scratch copy of the fleet one at a time,
  with `C01` asked what it thinks. Eight cases, each asserting its own verdict: the untouched fleet as a control, three of
  the deleted duplicates wired back into `ci.yml`, the two survivors (`docs.yml`'s generator step,
  `privacy.yml`'s egress step) removed, a required gate existing only as a YAML comment above a step (clean),
  and a required gate existing only as a `#` line inside a `run:` block (red) — the last one the hole this
  close found and closed.
* **`epic11-exit-criteria.md`** — the epic's four constraints, each with the command that ran and what it
  printed, the duplicate table worked row by row, and what the close left owed (`reco-kwg6`).
