# Changelog

All notable changes to fathom are documented here. The format is
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), the versions are
[Semantic Versioning](https://semver.org/spec/v2.0.0.html), and — because a toolchain floor is a promise
to whoever packages this — a dropped MSRV is a **Minor** change recorded under its own
[`Toolchain`](docs/adr/0002-workspace.md) heading, per ADR-002 §2.

This file is also the release-notes source `cargo-dist` reads when it builds a GitHub release body, so
an unreleased section that describes nothing is a release body that says nothing. Keep it true, keep it
short.

## [Unreleased]

Everything below is scaffolding: the shape ADR-002 declares, compiled, tested and shipped through CI.
No product behaviour is claimed by any entry on this page yet.

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
