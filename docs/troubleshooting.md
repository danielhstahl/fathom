# Troubleshooting

- **Status**: seeded from what the alpha's authors hit, plus the failure modes the code was written to report
  (`e7.5`, **reco-60m**). It will grow in the beta: the intake path is at the bottom, and this page is where
  the answers end up.
- **Read this if**: something did not happen, or happened in a way you cannot explain.

The rule this tool holds itself to is that **a refusal names the next action** — no empty pane shrugging, no
error that ends in a ticket number instead of a remedy ([first run](concepts/onboarding.md)). So the first
move is almost always the same: read the `remedy:` line, because it was written for the case you are in.

## The first sixty seconds

```bash
fathom --version          # version + compiled-in features: paste this into any report
fathom doctor             # the build, the environment, the repo and the index, with a remedy per finding
fathom doctor --json      # the same as one object, safe to attach to a bug report
```

`doctor` exits `0` for findings that are true but not errors, which surprises people:

```text
$ fathom --plain doctor; echo "exit=$?"
repository: . is not a git repository: . is not a git repository this build can read
  remedy: run this inside a git repository, or point --root at one
index: there is no index at .fathom/index.sqlite3 yet
  remedy: run `fathom index` in this repository, or `fathom index --rebuild` to throw the cache away
exit 12 — not an error unless --strict is passed
exit=0
```

`12` is `INDEX_DEGRADED`: the index is missing, incomplete or stale. Most "nothing happens" reports are that.
The verbs that *need* an index exit `12` rather than printing an empty answer; `doctor` reports it as a state
and leaves the judgement to you, because `doctor` is a diagnostic and a diagnostic that exits non-zero in a
setup script is a script that stops reporting. Add `--strict` when you want the finding to be fatal.

The codes you will actually meet, and the sentence each one is trying to tell you
(the whole contract is in the [CLI reference](reference/cli.md#exit-codes)):

| Exit | Name | You meet it when |
|---|---|---|
| `0` | `OK` | the work was done. For `doctor`, "done", not "nothing found" |
| `3` | `NOT_A_REPOSITORY` | wrong directory, a bare repo, or an unreadable `.git` |
| `4` | `STORE_CORRUPT` | the index failed its integrity check; `doctor` names the rebuild |
| `5` | `CONFIG_INVALID` | a `fathom.toml` that does not fit the schema — the message names the key |
| `7` | `CHECK_VIOLATIONS` | a gate objected: `fathom review status`, `fathom export --strict`, `fathom drift --fail-on-findings` |
| `9` | `FEATURE_ABSENT` | you asked this binary for something it was not compiled with, e.g. the TUI from a `minimal` build. The message names the feature and the rebuild |
| `12` | `INDEX_DEGRADED` | no index, or one that is behind. `fathom index`, and `fathom stats` to confirm |

## Symptoms

| Symptom | Cause, usually | Do |
|---|---|---|
| `is not a git repository this build can read` (exit `3`) | wrong directory, a bare repo, or a `.git` file pointing somewhere unreadable | `cd` into the work tree, or `--root /path/to/repo` |
| `no index yet` / exit `12` | nobody has indexed here, or the epoch was thrown away | `fathom index`, then `fathom stats` |
| the queue is empty but there are commits | a stored filter, or everything is signed off, or the index is behind | `fathom --fresh`; `fathom tui --show-session`; `fathom index` |
| the frame will not draw, and says `LAYOUT REFUSED` | the terminal is below the view's floor | widen it, or use `--plain`; ambient needs 40×3 ([ambient](concepts/ambient-mode.md)) |
| no progress row during `fathom index` | stderr is not a terminal — a pipe, a CI step, a `tee` | `fathom index --progress` asks for the row anyway |
| `--json` refuses to run with `--progress` | deliberate: one document on stdout, no decoration | drop `--progress`; the row is for humans |
| a key does nothing | you are in `prompt`/`pick` mode, where letters are text; or an override took it | `?` for the merged table; `fathom tui --show-keys` |
| your key override did nothing | unknown mode, unknown action, a collision, or a shadowed reserved key | it is a **startup report**, not a silent fallback — read it; then `fathom tui --show-keys` |
| colours are wrong, or absent | `NO_COLOR`, a theme that failed a floor, or a terminal without colour | `fathom tui --show-theme`, `--list-themes`, `--show-theme --audit` ([theming](concepts/theming.md)) |
| sign-offs keep going **stale** | the history moved after review (amend, rebase) | expected; re-review. Why it is reported instead of trusted: [the ledger](concepts/review-ledger.md) |
| `review sign` refuses with "the index holds no diff for this episode" | the episode genuinely has no diff — a merge commit is the usual case | sign a member commit instead. A sign-off that could never be checked for staleness is not written |
| `fathom review status` says the ledger is ignored by git | `.fathom/*` is gitignored and the negation line is missing | add `!.fathom/reviews/` to `.gitignore` (ADR-004 §12's stanza) |
| every figure says `unknown` | the analyses did not run, or their feature is not compiled in | `fathom doctor`, `fathom stats`; check the feature line from `--version` ([features](reference/features.md)) |
| a dependency ecosystem is unread | the ecosystem is not in the 25 formats, or the file failed to parse | `--json` names which, with a raw-text preview per unparsed file ([dependency surface](concepts/dependency-surface.md)) |
| advisory lookup refuses (exit `8`) | a `net-*` feature is compiled in and not consented to | the message names the key. **The default build cannot reach a network at all** ([features](reference/features.md)) |
| `fathom check` says a check is `blocked` | the input that check needs is missing — no index, no module graph, a base outside a shallow clone, or a binary built without the feature | the row prints the remedy; [the CI gate](concepts/ci-gate.md#no-silent-passes) explains why a blocked check is never a pass |
| a finding you already suppressed is back | the episode id moved and the key was derived wrong, or the config layer changed | the key is `detector:subject` and contains no episode id — so check which `fathom.toml` won |
| the watcher fell back to polling | inotify is unavailable (network mount, container) | expected and reported: `fathom doctor` prints the backend and the OS's reason |
| the frame feels slow | a wide rail, a big diff, a slow terminal, a loaded machine — or a real regression | `fathom tui` prints its own frame report as it exits (draws, events, p99, worst, over-budget against the 16 ms budget), and `fathom doctor` repeats the last session's from `.fathom/frame-report.json` |

## The three things that are not bugs

* **`unknown` where you expected a number.** An unmeasured axis leaves the denominator rather than becoming a
  `0`, and a delta section says which plan key owns it
  ([the delta](concepts/delta.md)). A zero would be a lie with better manners.
* **a four-column-wide "no" from a view.** Every pane's empty state names the cause and the verb that fills it,
  and the whole set is reviewed as one module. If you get a generic shrug, that *is* a bug — file it.
* **`fathom --demo` writing to your cache directory.** It is the only thing the demo touches, and deleting
  `$XDG_CACHE_HOME/fathom` is a complete undo. The miniature is a real repository indexed by the real
  `fathom index`, which is why `--demo` can be trusted to look like your own data later.

## Two indexes you can get into

**A parked epoch.** `fathom index --rebuild` advances the epoch and leaves the old rows on disk marked dead, so
an interrupted rebuild is recoverable by definition. `fathom stats` reports a parked epoch as `building`, with
the remedy. The staging file and the atomic swap exist for a concurrent reader, and there is no concurrent
reader until the daemon (`e6.3`) lands — that is a documented absence, not a half-finished mechanism.

**A store that will not open.** Exit `4` (`STORE_CORRUPT`) means the integrity check failed. `doctor` names the
rebuild; the store is class **R** — disposable — with one exception that matters: class **M** files (the
[ledger](concepts/review-ledger.md), `.fathom/ratchet.toml`) are must-survive *because they are committed*, and
`doctor` refuses to call a store with untracked ledger files healthy. Rebuilding the index is always legal;
losing those files is not, which is why the distinction is on the door of the recovery path.

## Filing something worth reading

There is **no telemetry**, so the bug report is the intake, and the quality of the report is the quality of the
fix. Four things, in order of usefulness:

```bash
fathom --version                 # the exact build, including the compiled-in feature set
fathom doctor --json             # one object: build, environment, repository, index, config layers
fathom hooks --check --json      # if harness events are anywhere near the problem
```

plus the command you typed, what it printed, and what you expected — and, when it is safe, the repository or a
redacted reduction of it. Two honest notes about the redaction:

* **the delta documents and `doctor --json` are already sanitised** — home paths render as `~`, absolute paths
  and control bytes are refused by the export's own verifier — but a `fathom analyze --json` contains your file
  paths and commit subjects. Look at it before you attach it.
* **`fathom --demo` reproduces a surprising amount**: if the miniature shows the same wrong figure, you have a
  minimal reproduction and a fix that can be tested against it in CI.

Where to send it: **the repository's issue tracker**, through one of its two forms — and
[the feedback channel](concepts/feedback.md) is the page that owns that sentence: what each form asks for,
why `fathom doctor --json` is required, the Tuesday triage cadence, and the seven dispositions a report can
end with (`e7.7`, **reco-rny**). This page is the answers side of the same machine: when a beta issue gets
fixed, its symptom line stays here, because the next person will hit it too, and a symptom that is written
twice in two places is a symptom that gets fixed once and contradicted once.

## If the tool is lying

The worst failure this product can have is not a crash, it is a confident wrong answer: a finding that is not
true, a score that ranked by accident, a document that says `measured` about something it did not measure.
If you find one, that is the most valuable report you can file, and say the word "wrong" rather than "odd" in
the title. The gates that exist to catch this class are
[the detector fixtures](detectors/index.md), [the calibration corpus](concepts/false-positive-calibration.md)
and the delta self-check, and a real-world counterexample beats all three.

---

**Next**: [the CLI reference](reference/cli.md) for the exit-code table ·
[the recipes](recipes/) for the workflows those symptoms interrupted ·
[the docs home](index.md).
