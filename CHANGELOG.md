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
- `package.repository` for `fathom-cli` is a **placeholder** (`https://github.com/danielstahl/fathom`);
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
