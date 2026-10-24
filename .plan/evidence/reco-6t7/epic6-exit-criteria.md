# Verification: epic 6's three exit criteria, run and recorded

- **What this is**: epic 6 (`reco-6t7`) closes on one sentence with three clauses in it — *a working
  adapter for at least two real harnesses, an MCP session where the agent queries "what did I just
  change and is it risky", and a CI job that fails only on new violations*. Every clause names a run,
  so every clause below **is** a run: the command, what came back, and the part that is still not
  proven. Nothing here is a fixture presented as a run.
- **Reproduce**: `python3 tools/epic6_acceptance.py` (offline, ~1 s) ·
  `python3 tools/epic6_acceptance.py --live-pi` (adds the two live vendor sessions, ~15 s) ·
  `--out <file>` writes the same report as JSON. CI runs it through
  `tests/test_epic6_contract.py`, which also checks that the gate can go **red** (pointed at
  something that is not a binary it must fail, not crash).
- **Machine-readable run**: [`epic6-exit-criteria.json`](epic6-exit-criteria.json) next to this file —
  produced by `--live-pi --out .plan/evidence/reco-6t7/epic6-exit-criteria.json` on the release
  binary built from this tree; 3/3 clauses pass, 29 checks, 13.3 s.
- **Headline**: all three clauses are met. Writing the gate found one **real defect** (a file body
  crossed the envelope adapter's boundary under a key name the adapter did not know), fixed here, and
  two **recorded oddities** that are named in §4 rather than smoothed over.

| clause | verdict | measured by |
| --- | --- | --- |
| two working harness adapters | **pass** | 4 adapters produced a clean stream, 3 of them from a live session |
| the agent asks "what did I just change, and is it risky" | **pass** | one stdio MCP session, two tool calls, one ranked risk vector with evidence |
| a CI job that fails only on new violations | **pass** | 5 gate runs: `7`, `0`, `0`, `7`, `7` — the baselined violation never came back as news |

---

## 1. Clause 1 — a working adapter for at least two real harnesses

Four adapters, four kinds of evidence, and the difference between them is kept visible rather than
flattened into "adapters: ✓":

| adapter | live? | what came back |
| --- | --- | --- |
| `adapters/fathom-post-commit` (git fallback) | **yes** — real commits, real git | 3 events for 3 changed paths over 2 commits, 0 refusals, canary absent |
| `adapters/fathom-hook.py` (stdin envelope family) | replay of a live capture | 14 events, 4 turns rebuilt, 0 refusals (`tests/traces/envelope-live.jsonl`) |
| `adapters/fathom-pi-hooks.ts` (pi, native events) | **yes** — `pi -e …`, a real headless session | 13 events, 3 turns, 0 refusals, canary absent, `fathom index` saw the stream |
| `adapters/fathom-envelope-driver.ts` → `fathom-hook.py` | **yes** — a real session, the vendor stood in for itself | 10 events, 2 turns, 0 refusals, canary absent |

The live pi runs are one prompt each in a scratch repository with a canary file:

```bash
pi -e /workspace/adapters/fathom-pi-hooks.ts -p "Read canary.txt, then write notes.md containing the single word done. Reply ok."
```

and what the repository looked like afterwards is the part that matters — the stream is healthy *and
the index now knows a harness was here*:

```
$ fathom hooks --check --plain
.fathom/events.jsonl — offset 0 of 5402 bytes (5402 bytes, 13 lines behind)
  events read: 0   last seq 0 (file at 13)   types: none yet
  turns: 0 opened, 0 open, 0 inferred, 0 closed by the next turn — all session-qualified
  rejections: none
exit 0 — the stream is healthy

$ fathom index --json | jq .intent.harness
{"events": 13, "events_file": ".fathom/events.jsonl", "exists": true, "notices": [], "refused": 0, "turns": 3}
```

Two clauses of that are load-bearing and asserted per adapter: **0 refusals** (the reader accepted
every line the adapter wrote, so the writer's contract and the reader's agree) and **the canary absent**
(a file body must cross as a digest, never as text — ADR-005 §4.3).

### 1.1 What the first live run found, and what was fixed

The first time this gate ran with `--live-pi`, the canary check **failed** on the envelope driver:

```
FAIL adapters :: pi through the envelope driver: no file body crossed
```

The body had crossed. `adapters/fathom-envelope-driver.ts` re-spells a live tool result the way
Claude Code does, which puts the text under `structuredContent`:

```json
{"tool_response": {"content": [{"type": "text", "text_sha256": "0e7a…"}],
                   "structuredContent": "FATHOM-E6-CANARY-… is inside"}}
```

`adapters/fathom-hook.py` digests content under a **list of known key spellings** — `content`, `text`,
`body`, `diff`, `stdout`, … — and `structuredContent` was not on the list, so it was copied verbatim
into `.fathom/events.jsonl`. That is exactly the promise the adapter's own docstring makes
("Bodies, diffs and command output are digested, not copied"), broken by a key name nobody had
thought of. Two changes in `adapters/fathom-hook.py`:

* the recognizer now **tokenises** the key — `structuredContent`, `structured_content` and
  `Structured-Content` are one name — and calls it content when the **last token** is a content noun,
  so `file_body`, `outputText` and `commandOutput` are the same promise. A bare suffix match would be
  wrong the other way: `context` ends in the letters `text`, and digesting it throws evidence away; and
* a content key is digested **whatever its type** — recursing into a container and trusting the inner
  key names is the same hole one level down.

Pinned by `tests/test_adapter_contract.py::test_a_body_under_a_vendor_spelling_of_content_is_still_digested`,
which feeds one envelope with five spellings plus a flag (`created`) that must **survive** — a flag is
not content and the `file_edit` derivation needs it.

### 1.2 What clause 1 still does not prove

Unchanged from [`docs/harness/verification.md`](../../docs/harness/verification.md) §4, and stated
again because it is the epic's boundary, not a ticket's:

- **pi is still the only vendor installed.** The envelope family is exercised live only with pi
  standing in for the vendor, so the vendor field *spellings* stay [external] until ADR-005 §9's
  capture runs against a real Claude Code / Gemini CLI / OpenCode.
- **prompt text** still does not arrive from a real vendor: the streams carry a digest, so the
  intent ladder is exercised end to end on the trailer and digest rungs, not the prompt rung.
- **"recipes followed by someone other than their author"** remains unsatisfied — no second person
  was available, and the epic does not get to claim it on its children's behalf.

---

## 2. Clause 2 — the MCP session where the agent asks "what did I just change, and is it risky"

One session with `fathom mcp serve`, over the stdio the shipped config describes, against a
repository whose last change is deliberately worth asking about: three human commits, then **one
agent turn** that bumps a dependency, adds untested logic and puts a credential in its own commit
subject. The question is two calls, because the model is two questions:

```python
session.call("explain_episode", {"episode_id": "HEAD"})   # what did I just change
session.call("risk_report", {})                           # and is it risky
```

`tools/list` offers the seven read tools and neither write tool:

```
['explain_episode', 'changes_between', 'architecture_delta', 'dependency_delta',
 'risk_report', 'search_history', 'review_memory']
```

The answer, as the envelope returned it:

```
episode      ep-37c84499239302ccec63 · 1 commit · author_kind=agent
what moved   src/app.py (source) · requirements.txt (dependency surface, operator action attached)
risk         spine-1 49 = surfaces 36 + author 8 + size 3 + breadth 2 · tier medium
factors      surfaces 36/50 · author 8/8 · size 3/16 · breadth 2/12
evidence     episode:ep-37c84499239302ccec63 · model:spine-1
grounding    heuristic — no trained model exists (reco-884's calibration bar is unmet)
cost         risk_report 2 782 bytes in 2 ms · explain_episode 14 028 bytes in 8 ms
```

Every line of that is checkable by the caller: the score is a vector with a cap per factor, the
factors name the file, the evidence ids are addresses in the index, and `grounding` says out loud
that the ranking is heuristic rather than trained (`degraded: true`, and `unknowns` lists what the
model could not see). "Is it risky" is answered, and the answer admits its own limits in the same
document.

The same session also had to refuse, and did:

| asked | got |
| --- | --- |
| `mark_reviewed` (a write, config off) | `error.code = no_capability`, and the tool was never in `tools/list` |
| the credential in the commit subject | `[REDACTED:aws-access-key]` in the answer, the key absent |
| a file body | no `body`/`content`/`diff`/`patch`/`hunk`/`text`-shaped field in any response, and the canary absent |
| an oversized answer | bounded and cursorable (`bytes_returned ≤ mcp.max_response_bytes`, `truncated`, `next_cursor`) — asserted in `tests/test_mcp_contract.py` |

---

## 3. Clause 3 — the CI job that fails **only** on new violations

One scratch repository, two declared layers (`l1_low = src/low`, `l2_high = src/high`,
`no_upward = true`), `[check] mode = "deny"`, five runs in the order a branch would produce them:

| run | range | what the range contains | exit | news |
| --- | --- | --- | --- | --- |
| 1 | `clean → violation` | a new upward edge `src/low/base.py → src/high/up.py` | **7** | `no_upward::src/low/base.py->src/high/up.py` |
| 2 | **the same range**, after `fathom ratchet` baselined that key | the same edge, now debt | **0** | none — "not news to this range" |
| 3 | `baseline commit → a comment-only change` | nothing architectural | **0** | none |
| 4 | `… → a second upward edge` | a *new* edge `src/low/extra.py → src/high/up.py` | **7** | only `no_upward::src/low/extra.py->src/high/up.py` |
| 5 | the whole range | both edges, three commits | **7** | only the new key |

Run 1, verbatim:

```
$ fathom check --base HEAD~1 --head HEAD --plain; echo $?
fathom check · 6c4cbf29dcd7..35d0c98df101 · base from --base · policy v1 · mode deny · fail at warning

  check         volume   status    examined                     findings
  layering      error    ran       3 dependency edges judged    1 (1 listed)
…
fathom check failed: 1 finding(s) at or above `warning` in 6c4cbf29dcd7..35d0c98df101 — worst is error
[layering/no_upward::src/low/base.py->src/high/up.py] at src/low/base.py (graph:layering (imports))
```

Then the ledger gets one row, with an owner, an expiry and a reason — the way `reco-5ht` says an
allowance is granted, by a commit rather than by a flag:

```toml
# .fathom/ratchet.toml, written by `fathom ratchet --update-baseline --key … --owner team-core --expires 2027-01-31 --reason …`
[[allowance]]
kind = "layer"
key = "no_upward::src/low/base.py->src/high/up.py"
owner = "team-core"
expires = "2027-01-31"
reason = "carried in from the migration; the split lands with the layer rewrite"
```

and run 2 — **the identical range** — says why it is quiet:

```
  layering      error    ran       3 dependency edges judged    —
    note: 1 violation(s) are baselined in .fathom/ratchet.toml, so they were not news to this range
fathom check passed dbf9b6556e6a..e9860bb92d7b: 0 finding(s) below the `warning` threshold, 6 check(s) ran
```

Run 4 is the criterion in one line: the reported rule set is `[no_upward::src/low/extra.py->src/high/up.py]`
— the baselined edge is still in the tree, still counted, and still not news. The gate's "why did CI
fail" line is one sentence and names a path (`… at src/low/base.py (graph:layering (imports))`), and
the slowest of the five runs took 0.019 s against `reco-ngk`'s 60 s budget. SARIF, the reusable
workflow and the external corpora are `reco-ngk`'s and `reco-by2`'s own evidence
([`docs/concepts/ci-gate.md`](../../docs/concepts/ci-gate.md),
[`.plan/evidence/reco-by2/`](../reco-by2/)); this section proves the ratchet half, which is the half
the epic's wording turns on.

---

## 4. What is *not* proven, and what the gate turned up

Recorded because the alternative is a checkbox that passes on a fiction.

1. **Two commits in the same second make "newest revision" a coin flip.** The gate's first version
   failed at random: `fathom ratchet` with no `--revision` judges "the newest graph revision the
   index holds", and ordering by commit date is how it finds it — two fixture commits sharing a
   second resolved to either of them, so the allowance was sometimes granted for the clean tree. The
   gate now stamps each fixture commit with its own minute (the same discipline
   `crates/fathom-cli/tests/check_verb.rs` already follows), which makes the run deterministic and
   leaves the underlying tie **un-fixed and open**: a repository whose commits land within one second
   (an agent that commits twice in a breath) can get `fathom ratchet`'s default revision wrong. The
   remedy is a ticket of its own: order the tie by revision, not by wall clock.
2. **A layering lane result can outlive the ledger row that produced it.** Observed while iterating:
   granting an allowance, re-running the check (forgiven), then deleting `.fathom/ratchet.toml` and
   re-running the *same* range in an index that had since grown more revisions — the violation stayed
   quiet, with no "baselined" note. Deleting the row in a repository with no later commits does bring
   the violation back, so the suspicion is the analyse cache not being invalidated when the ledger
   changes. Not proven either way, and deliberately not chased: the gate never un-baselines, so it
   cannot be bitten. Worth a defect ticket with this recipe.
3. **`architecture_delta` still refuses honestly rather than answering** when an index has no graph
   for the range; the MCP envelope returns `no_model` with a remedy and `degraded: true` rather than an
   empty list. This is the design (`reco-p3i`'s "a refusal is a code with a remedy"), not a pass, so it
   is named here rather than scored as one.
4. **The MCP risk number is heuristic.** `grounding` and `degraded` both say so; a trained model is
   `reco-884`'s calibration bar, unmet, and no clause of this epic pretends otherwise.
5. **No live second vendor**, as §1.2 says: the envelope family's field spellings are still [external].
