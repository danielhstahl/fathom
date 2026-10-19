# Episode risk ranking: the order the queue reads in, and the ceiling printed beside it

- **Status**: Accepted, and implemented — the spine-only half. `reco-884` (`e4.4`) has now replaced its
  *inputs* with a documented feature vector and a calibration set, in
  [risk-scoring.md](risk-scoring.md) (`features-1`), and `reco-wsj` (`e4.5`) replaces its *ordering
  function* with risk × staleness × blast radius; neither changes the shape described here, which is the
  shape the queue, the rail and `w` print. This document stays the contract for **that** ranking —
  `spine-1` is what the shipped queue computes until `e4.5` rewires it.
- **Ticket / plan key**: `reco-wsj` (`e4.5`) owns the queue. This ranking was pulled forward out of
  `e4.5` because `reco-glv`'s (`e1.12`) walkthrough could not be completed at all without some
  defensible answer to "which three episodes are the riskiest" — see
  [demo/README.md](../demo/README.md).
- **Implements**: ADR-006 §5.1's binding note — *"the strip must name the ranking function, and `w`
  must print the same feature vector `fathom why` prints"* — and ADR-001 §7.9's rule that a number a
  reviewer cannot check is a recommendation rather than a fact. Its inputs are the spine's:
  [paths](paths.md) (`e0.9`), [noise](noise-suppression.md) (`e2.2`),
  [episodes](episode-segmentation.md) (`e1.3`) and
  [the test surface](test-surface.md) (`e2.9`). It is *not* `e4.4`'s feature vector and does not
  claim to be.
- **Executes as**: `fathom-analyze::risk` (`Surface`, `classify`, `Signals`, `rank`, `COMPONENTS`,
  `BLIND_SPOTS`), read through `fathom-store::episode::Store::episode_files`, composed in
  `fathom-cli::episodes`, and drawn by `fathom-tui::worklist` plus the card pane's `w`. Measured by
  `crates/fathom-analyze/tests/risk_spine.rs`, `crates/fathom-tui/tests/risk_rank.rs` and
  `crates/fathom-cli/tests/risk_rank.rs`.

---

## 1. What this decides, and what it refuses to

One question, per episode:

> **Of everything in this index, which should a human read first, and what sentence justifies that
> order?**

The answer is a number 0–100 with five named parts, a badge naming the kinds of file that earned it,
and a list of what the inputs cannot see. Everything it needs comes from rows the spine already
writes — paths, added and removed lines, file counts, author kind — so it exists in a build with no
analysis layer, which is the build Epic 1 ships.

Three refusals, all of them load-bearing:

* **It is not a finding detector.** No cycle detection, no secret scanning, no "this migration has no
  down file". Those are `e4.4`'s findings and `e2.8`'s non-code rules; where the ranking cannot see
  one, it prints that it cannot see one ([§8](#8-what-a-spine-cannot-see)) instead of guessing.
* **It is not a recommendation.** It orders reading; it never marks an episode acceptable, never
  hides one, and is never the reason a row is absent. `x`/`f`/`-`/`z` dispositioning is
  `e4.6`'s, and the strip says so.
* **It is not computed in the shell.** `fathom-tui` sorts values it is handed; `fathom-cli` computes
  them once per page, in the composition root, because the shell may not run analysis any more than
  it may open a store (ADR-002 §5.2, ADR-006 §2.1).

## 2. The scale, and the arithmetic that *is* the explanation

`spine-1`, named in the model string so a screenshot says where its number came from:

```
spine-1 62 = surfaces 50 + author 8 + size 4
```

The vector prints the components that fired, largest first, and it is the same string on the card, in
the `w` toast and in the `--plain` frame a pipe or a screen reader gets. `score` is their sum, clamped
at 100; no component may exceed its own cap, and the caps are one table (`COMPONENTS`) that both the
arithmetic and the printed table read, so a document cannot drift from the code without a test
failing.

The reasons are printed in the reviewer's language, not in the model's:

```
surfaces 50/50 — db/migrations/0038_drop_email_backup.sql — a schema migration whose reversal
                 is not in the spine · an operator must act on it · also touched: env, source
```

Every reason names a path or a count the reviewer can go and look at — and `fathom show <commit>`,
which prints the per-file rows these numbers were summed from, is where they go and look. That is the
whole content of "explainable" here: not a score with a label attached, but a sentence whose nouns are
in the diff.

## 3. The five components

| Component | Cap | Input, in one sentence | Row it comes from |
|---|---|---|---|
| `surfaces` | 50 | the most consequential *kind* of file the episode touched, +2 per further operator-action surface (max +4) | `git_commit_file.path`, classified ([§4](#4-the-surface-table)) |
| `size` | 16 | churn (`n_add + n_del`) on a log scale, discounted three-quarters for noise | `episode.n_add`, `n_del`, per-file churn |
| `deletions` | 14 | how much of the churn is removal, and whether the test surface shrank while source grew | per-file `n_added_lines`, `n_removed_lines` |
| `breadth` | 12 | distinct surfaces touched, plus files past 12 and past 30 — scope creep | `episode.n_files`, surfaces seen |
| `author` | 8 | an agent wrote it (8), nobody is credited (6), a human did (0) | `episode.author_kind` |

Caps sum to 100, so a component's share is meaningful beside another's, and the printed `8/50` or
`12/16` says both the contribution and the ceiling. `author` is first-class because the thesis is
that unreviewed agent work deserves more attention than unreviewed human work (ADR-001 §7.3); a
ranking that ignored authorship would be silent about the reason the tool exists.

## 4. The surface table

`surfaces` is the axis that lets a three-line change outrank a four-hundred-line one, and it is the
reason this ranking is worth writing at all. Weight is "what a reviewer would stop for", not size:

| Surface | Weight | Path evidence | Why it is where it is |
|---|---|---|---|
| `migration` | 50 | `db/migrations/**`, `*.sql`, alembic, flyway | the data may not come back, and the reversal is often not in the diff |
| `ci` | 46 | `.github/workflows/**`, `.gitlab-ci.yml`, `Jenkinsfile`, … | permissions and secrets travel with the artefact it builds |
| `infra` | 46 | `*.tf`, cloudformation, helm, kustomize, ansible | the blast radius is the platform, not the repository |
| `container` | 40 | `Dockerfile`, `Containerfile`, compose files | what ships, and with what inside it |
| `dependency` | 36 | `uv.lock`, `Cargo.lock`, `pyproject.toml`, `package.json`, … | the supply chain is part of the diff even when nobody typed it |
| `schema` | 30 | OpenAPI, JSON Schema, protobuf, graphql, avro, thrift | an interface contract breaks callers the diff cannot see |
| `env` | 30 | `.env*`, `.tool-versions`, flag files | half the change lives outside the repository |
| `source` | 10 | anything that reads as logic | most risk does land here — but *most* is not the same as *most alarming* |
| `test` | 8 | suites, specs, benches, stories | the cover, not the thing covered |
| `other` | 8 | nothing matched | an unknown is stated, not guessed ([paths.md](paths.md) §1) |
| `generated` | 6 | `*_pb2.py`, `dist/`, `target/`, snapshots | derived elsewhere; discounted here and in `size` |
| `vendored` | 6 | `vendor/`, `node_modules/` | somebody else's code, in-tree |
| `asset` | 4 | media and data | looked at, not read |
| `binary` | 4 | not text | same |
| `docs` | 3 | `*.md`, `*.rst`, `*.adoc` | read, not executed |

Two rules about this table. First, the classification prefers the stored taxonomy
(`git_commit_file.path_class`, `e0.9`) and falls back to the path table only when the index says
`unknown` — which is every index this build's indexer writes until the `e0.9` table is wired into the
Rust ingest. Second, where the two disagree they disagree on purpose: the taxonomy answers what a path
*is*, and this answers what a *reviewer* should think, which needs CI split from other configuration
and lockfiles split from other generated files. A stored `config` class is therefore re-read as
`ci`, `container` or `env` from the path, and the reason prints which.

## 5. The size axis

`16 · ln(1 + churn) / ln(1 + 20 000)`, clamped, where `20 000` (`SIZE_CEILING`) is roughly where a
single episode stops being reviewable at all — so "16/16" means "as big as an episode gets", and two
repositories are measuring against the same ruler.

It is a log because the acceptance fixture's largest turn is around 2 500 lines of churn: on a linear
axis every ordinary commit would sit at zero and the component would be a one-episode flag.

Noise is discounted, never removed: churn on generated, lockfile, vendored and binary paths is
counted at a quarter. A lockfile storm is still a decision somebody made, and `e2.2`'s rule is that
noise is a *view* rather than a deletion. The reason says how much was discounted and why, so the
reviewer can disagree with the discount rather than merely being subject to it.

## 6. Deletions, and the test surface

Two things the aggregate `n_del` on a rail row cannot tell apart, and the per-file rows can:

* **a change that is mostly removal** — up to 6 of the 14 points once the removal is three-fifths of
  the churn and the churn is at least 40 lines;
* **a test surface that shrank while source grew** (`n_del ≥ 20` on test paths and test removals
  exceed test additions) — the other 8 points, and their own sentence.

That second case is "520 lines of rewrite, 300 lines of the suite deleted behind it", which is the
fixture's own planting and the single most common way an agent turn quietly removes coverage. It is
*not* the same claim as `e2.9`'s "new public logic, no new test", and does not pretend to be: this is
a shape in a diffstat, not a coverage argument.

## 7. Breadth, and ties

`breadth` is scope creep: 2 points per surface past the first (to 10) plus 2 more past 12 files and
4 past 30. Authorship is worth up to 8, as above.

Order, including the ties, is a contract, because `e4.5`'s acceptance criterion is that queue
ordering is reproducible: **score descending, then churn descending, then recency descending, then
episode id ascending.** Unranked episodes sort *after* every ranked one, in the rail's own order. A
row that was never ranked is never folded into the ranking as a zero — "nothing ranked this" is a
statement about the tool, and it prints as `risk ?` with a line saying which ticket will fill it
(ADR-001 §7.7).

## 8. What a spine cannot see

Printed with the score, on the card and under `w`, because a list that looks complete is the
dangerous failure mode:

1. whether a migration can be rolled back;
2. an environment variable declared and never read;
3. a new import cycle;
4. a removed public API;
5. what depends on the touched code (blast radius);
6. a committed secret whose value is real;
7. whether a deleted test was the only cover for the behaviour.

Consequences worth stating plainly, from the acceptance fixture: sixteen of its 212 episodes touch
`db/migrations`, and this ranking puts all sixteen in front of nearly everything else, biggest first,
because a spine sees a new SQL file and stops there. The destructive one is among them and the
ranking cannot tell which. That is blind spot 1, and closing it is `e2.8`'s reversibility rule and
`e4.4`'s features, not a weight tweak here. The same goes for the declared-and-never-read variable
(blind spot 2) and the new cycle (blind spot 3): both are invisible here by construction, and both are
named where a reviewer will read them rather than in a footnote.

## 9. Where the inputs come from, and what caps them

One episode's ranking needs its own per-file rows, which the rail never needed:
`Store::episode_files` sums `episode_member ⋈ git_commit_file` into one row per `(episode, path)`,
reads `path_class`/`noise_class` back rather than re-deriving them, and reports whether the
`EPISODE_FILE_CAP` cut the read short. A truncated read is a *caveat printed under the reasons*, not a
silent truncation — a ranking that saw 50 000 of 90 000 file rows has to say so. The read is part of
the one-page, fixed-query-count budget ADR-002 §5.3 sets for a first frame: four queries, never one
per row.

## 10. What each ticket replaces

| Input here | Replaced by | When |
|---|---|---|
| `surfaces` from paths | `e4.4`'s features (a finding, not a path class) | `e4.4` closes |
| `size`, discounted noise | `e2.2`'s measured noise share instead of a path guess | when the Rust ingest stamps `noise_class` |
| `deletions`' test heuristic | `e2.9`'s named cases and `e4.4`'s deleted-test feature | `e2.9`'s Rust seam |
| ordering: score only | risk × staleness × blast radius | `e4.5`, which needs `blast_radius` from `e4.4` |
| the blind-spot list | shorter, by closing items 1–7 one at a time | per finding, printed from the findings table |

Nothing in that list changes what the panes print: a number, its vector, its reasons, and the ceiling.

## 11. The measurements

* `fathom-analyze/tests/risk_spine.rs` — the ten candidates `tools/agent_fixture.py` plants, fed in
  as the rows the spine writes. Asserts the components add up, every reason quotes something, the kind
  of file beats the amount of file, the test shrink is its own reason, generated churn is discounted
  rather than dropped, and the two invisible candidates are *stated* rather than implied.
* `crates/fathom-cli/tests/risk_rank.rs` — the shipped binary on a twenty-one-commit repository: the
  queue's top three rows are the migration, the three-line workflow change and the turn that deleted
  its own suite; the 400-line documentation edit is last; the strip names `spine-1`, the count and the
  ceiling.
* `fathom-tui/tests/risk_rank.rs` — the shell's obligations, through the real renderer: order the
  rows, keep each row's own reason with its row, hand `w` the whole ceiling, and keep the frame exact
  at 80×24, 100×30 and 200×60.
* `tests/test_risk_ranking_contract.py` — the walkthrough's own question, asked of the shipped binary
  through a pty against the 500-commit fixture: the queue is ranked and not chronological, the
  migration and the CI permission change are on the first screen with their reasons, `w` prints the
  vector and the ceiling, and the rows nothing ranked print `?` at the bottom of the list instead of
  a `0` that would read as "safe".
* The acceptance harness measures the pane now that the pane answers. `tools/mvp_acceptance.py`
  records `cold.queue_visible.ms.small` (`q` until the screen printed `order: ` and a `risk` number),
  `query.queue.ms.small` and `query.card.ms.small` as measured numbers; all three were blocked on
  exactly this empty worklist, and the blocked reason said so.
