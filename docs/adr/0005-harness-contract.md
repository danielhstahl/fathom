# ADR-005: harness integration contract — hook wire formats (v1), the canonical event schema, the MCP tool surface, and versioning

- **Status**: Accepted
- **Date**: 2026-02-11
- **Ticket / plan key**: `reco-3pu` / `e0.5`
- **Implements**: [ADR-001](0001-product-thesis.md) — specifically the product amendment of 2026-02-10
  in [docs/concepts/thesis.md §11](../concepts/thesis.md)
- **Constrains**: `reco-afh` (`e6.1` hooks v1 ingestion: JSONL reader, validation, replay),
  `reco-7go` (`e6.2` reference adapters), `reco-p3i` (`e6.4` MCP server),
  `reco-552` (`e6.5` shared review memory), `reco-ngk` (`e6.6` `fathom check` CI gate),
  `reco-h7p` (`e6.8` privacy posture: no network by default), `reco-raw` (`e6.7` event bus and plugins),
  `reco-nts` (`e1.3` episode segmentation), `reco-f28` (`e1.4` incremental, mmap-backed index),
  `reco-884` (`e4.4` explainable risk score), `reco-rlf` (`e4.6` review ledger),
  `reco-2rx` (`e4.7` deterministic explanation, i.e. `reason()`), `reco-wsj` (`e4.5` attention queue),
  `reco-c54` (`e4.2` intent capture), `reco-w7q` (`e4.1` attribution),
  `reco-1tn` (`e5.7` export review artefact), `reco-ld8` (`e1.10` CLI verbs incl. `doctor`),
  `reco-kvk` (`e6.3` `fathom watch` daemon)
- **Extends**: the runtime/portability claims of `e1.4` and `e1.10` from prose into executable code —
  `src/recommender/harness/elf_probe.py` (§10)

**Applies to: Fathom only.** The two decisions recorded here — a per-vendor hook *wire format* plus a
canonical *domain event* schema, and a read-only-first MCP tool surface — apply to Fathom's ingest
boundary. **reco is unaffected:** it has no harness ingest and no MCP server. Where this document says
*we*, it means Fathom.

---

# Part zero: why this document exists

[ADR-001's product amendment](0001-product-thesis.md) states the problem in one line: *reaching the
product's state requires a harness ingest layer that has no code and no contract.* There is a second
line underneath it that only becomes visible when you try to write the ingest: **the §10 libc FFI
contract is prose, and prose cannot be dlopened.**

Both halves of this ADR are the same act. §1–§9 turn an aspirational interface into a contract that a
program can check. §10 turns the ABI claim into a program.

Three facts established by measurement, not memory, shape everything below. They are recorded because
the *absence* of each is a design error someone would otherwise make:

1. **Neither the Codex CLI nor OpenCode is installed here**, and their documentation is not reachable
   from this container: `curl` and `nslookup` are both absent, so a network check cannot even confirm
   DNS. Only Claude Code 2.1.x is present, and its hook surface is observable directly
   (`/usr/local/lib/node_modules/@earendil-works/pi-coding-agent/dist/core/hooks.js`).
2. **Four clauses of the brief are not implementable in this container.** They are recorded as
   *deferred with a named reason* (§8) rather than implemented as code that lies: **MAP_SHARED** (no
   mmap at all exists yet), **`reason()` timing SLA** (`reason()` is not implemented), **risk model
   training** (no model; the risk report is a deterministic heuristic and says so), **cross-tool
   consistency** (2 of 3 adapters are untested because their products are not installed).
3. **The container is not the platform the code claims to run on.** See §10: the `libc.so.6` under
   `/usr/lib/x86_64-linux-gnu/` is an ELF object of a different machine class than the host, so a
   naive ABI check reports success on a host that cannot run it. The ABI contract *must* fail loudly.

---

# Part one: events

## 1. Context

### 1.1 What "harness" names here

**Harness** is the adapter layer that receives events from the coding agent (Claude Code, Codex CLI,
OpenCode) and turns them into [episode](../concepts/thesis.md#episode)-shaped records. It is **not an
agent** and runs no agent loop — ADR-001 amendment 1. It is one of the two pieces "the entire
remaining product depends on".

### 1.2 The observed ground truth

Everything in §2 that describes a *vendor* shape is marked with how it was established:

- **[observed]** — read out of the installed Claude Code hooks module, or run against it.
- **[external]** — taken from vendor documentation the author could not reach from this container.
  Marked as such *on purpose and without apology*: an unmarked guess about a vendor protocol is how a
  94-issue plan accumulates a load-bearing fiction. §9 names what must be done to promote each
  **[external]** claim to **[observed]**.
- **[invented]** — ours, and freely changeable within a major version.

### 1.3 Constraints that are not negotiable

1. Ingest is **append-only**; a later event may supersede an earlier one but nothing is rewritten.
2. **The harness is untrusted input** (ADR-001 §5, amendment 1: "the harness is untrusted input and
   every write path must defend against that"). No ingestion may execute, import, or path-resolve
   what it receives without validation.
3. Ingest must survive **crash-mid-write** and **two writers at once** without an operator.
4. The event schema must be **stable across vendor updates**, because the vendors ship updates on
   their own schedule and do not consult us.
5. **No file contents cross the boundary** (§6.3).

## 2. Decision — §1: two layers, not one

**Wire layer (v1).** Each adapter defines the vendor's native event shape and stamps it verbatim:

```json
{"seq": 7, "ts": "2026-02-11T10:00:00.000Z", "harness": "claude_code", "source": "SessionStart",
 "session_id": "01JQ...", "cwd": "/srv/app", "git_branch": "main",
 "payload": {"model": "claude-opus-4-8", "permission_mode": "acceptEdits"}}
```

`payload` is the vendor's JSON **verbatim**, never reshaped. The Claude Code *flavour*
(`startup` | `resume` | `clear` | `compact`) is a payload field, **not** an event type: a flavour is a
detail of one vendor, and the file is shared between adapters.

**Canonical layer (v1).** A projection onto Fathom's own event union, `{seq, ts, fathom_event, ...}`.
Adapters project, and the projection is *total*: an event type that has no meaning in Fathom is stored
with `fathom_event: null` and is **counted, not dropped**. Every unprojectable event increments
`harness.status.unprojectable.by_type`. A silently-dropped event is invisible; a counted one is a
number on the status page that somebody eventually asks about, which is the entire difference between
a coverage gap you know about and one you discover in a customer's incident.

### 2.1 Canonical event types

| `fathom_event` | Wire origins | Carries |
| --- | --- | --- |
| `session_start` | Claude `SessionStart`; Codex `session-start`/`session_start`; OpenCode `session.created` | `session_id`, `cwd`, `git_branch`, `model`, `permission_mode`, `harness_version` |
| `session_end` | Claude `SessionEnd` (`reason`); Codex `session-end` | `reason` |
| `turn_start` | Claude `UserPromptSubmit`; Codex `prompt.submit`; OpenCode `message.part.updated` (role=user) | `prompt_sha256`, `prompt_chars`, `turn_id` |
| `turn_end` | Claude `Stop`, `StopFailure`; Codex `turn.stop` | `last_assistant_message_sha256`, `stop_reason` |
| `tool_call` | Claude `PreToolUse`; Codex `tool.start`; OpenCode `tool.execute.before` | `tool`, `tool_use_id`, `tool_input_sha256`, `tool_input` (redacted, truncated), `mcp_context_injected` |
| `tool_result` | Claude `PostToolUse`, `PostToolUseFailure`; Codex `tool.end` | `ok`, `is_error`, `exit_code`, `duration_ms`, `output_sha256`, `cwd_after` |
| `file_edit` | Claude `PostToolUse` on a path-bearing tool, or `PermissionRequest` on `Edit`/`Write`; Codex `file.change`; OpenCode `file.edited` | `path`, `change` (`add`/`delete`/`update`), `lines_added`, `lines_removed`, `content_sha256_before`, `content_sha256_after`, `diff_sha256`, `diff_available`, `patch_sha256` |
| `test_run` | **Derived**, §2.2 | `command`, `exit_code`, `duration_ms`, `cwd` |
| `approval` | Claude `PermissionRequest`, `PermissionDenied`; Codex `permission.request` | `decision` (`allow`/`deny`), `tool`, `pattern`, `rule_source`, `user_overrode_recommendation`, `denial_reason` |
| `error` | Claude `StopFailure`, `PostToolUseFailure`, `PreCompact`; Codex `error` | `kind`, `detail_sha256`, `retryable` |
| `subagent_start` / `subagent_end` | Claude `SubagentStart`/`SubagentStop` | `agent_id`, `agent_type` |
| `null` | everything else (WorktreeCreate, Task*, ConfigChangePre, …) | — counted in `unprojectable` |

### 2.2 Derived events

`test_run` has no vendor equivalent. It is **derived** from `tool_call`+`tool_result` where a test
runner is recognisable, because an unrecognised test command produces no `test_run` at all. A
detection that silently missed half the test runs would make the feedback loop look better than it
is: an unrecognised command must surface as `test_run` with `detected_by: "none"` rather than vanish.

## 3. Decision — §2: the file

### 3.1 Path

`.fathom/events.jsonl` by default; `config.harness.events_path` may point anywhere. **Resolution
order:** explicit config → `$RECO_EVENTS_FILE` → `$RECO_EVENTS_DIR/events.jsonl` → `<root>/.fathom/events.jsonl`
(`$RECO_ROOT` standing in for `<root>`). Two variables is a smell, so the config-file path is the
normal one and the two env vars exist for tests and for the backfill exporter.

### 3.2 One JSON object per line, keys sorted

Records are written with `sort_keys=True`: byte-identical re-reads are what makes a byte-offset index
safe to build twice and compare.

### 3.3 Monotonic sequence numbers

- `seq` is a **strictly increasing integer per events file**, starting at 1. It is the index key
  (`reco-f28`) and the `event_id` other objects refer to; `event_id = "e" + seq`.
- **The number is allocated by the file, not by the writer.** A writer that trusted its own clock or
  its own counter produced the bug this rule removes: two Claude Code subagents running concurrently
  emit independent `seq` counters that collide, and a monotonic-index assumption then fails *later*,
  in code far away from here. Allocation is: append while holding an exclusive `flock`, read the last
  `seq`, add 1, write. The lock is held for the whole read-modify-append, so allocation is atomic with
  respect to every other writer using the same convention.
- A torn final line **does not consume a seq**. The index build's `bad_lines` counter counts it; a
  gap in the sequence is a hard error in `harness doctor`.
- `ts` is the *vendor's* timestamp when it has one, else our wall clock at ingestion. **Ordering is by
  `seq`, never by `ts`**, because vendor clocks are not monotonic and we have seen reordered writes
  from parallel subagents.
- `turn_id` is `"<session_id>:<turn_index>"`, `turn_index` counted per session at ingest. A `turn` is
  the span `turn_start` → `turn_end`; a turn that never ends is closed by the *next* `turn_start` with
  `closed_by: "next-turn"`, so a crash cannot orphan one.

### 3.4 The index is a *sidecar*, not a line

The brief asked for "a byte-offset index line". **Rejected**: an index record inside the events file
would mean ingest writes two records per event and the backfill exporter must know to ignore one of
them, and it makes the file's meaning depend on record-type dispatch forever. The index is
`.fathom/events.idx.json` — a *derived* artifact, rebuilt by `fathom reindex`, never authoritative,
never written by the ingest path. Same argument as `docs/adr/README.md`: a derived artifact is fine
when it is rebuildable and clearly not the source.

```json
{"version": 1, "events_path": "...", "built_at": 1770000000.0,
 "last_seq": 42, "size_bytes": 12345,
 "spans": [{"seq_from": 1, "seq_to": 42, "offset": 0, "length": 12345}],
 "by_event": {"file_edit": [[0, 180], [900, 60]]},
 "bad_lines": 1}
```

### 3.5 Concurrent-append contract

Append with `O_APPEND` under `flock(2)` (`LOCK_EX`), which together guarantee a single line is not
interleaved with another writer's. The belt is `O_APPEND` (kernel-side seek-to-end); the braces are
the lock, which additionally serialises *seq allocation*. `flock` is advisory, so this holds only
among processes that take it — documented, not pretended to be universal.

### 3.6 Tolerance rules

A line that is blank, `json.JSONDecodeError`, or missing `seq` **and** `type` is skipped: **skipped,
counted in `harness.status.bad_lines`, never fatal, never deleted.** The file is never rewritten to
remove damage; the index records `bad_lines` and moves on. A torn trailing line from a crash is the
common case and the process must not require operator repair.

An **oversized** line is a different failure: lines above `MAX_LINE_BYTES` (4 MiB) are rejected at
write time — the write is refused, nothing lands on disk, and `record_rejections.oversized` counts it.
Truncating content silently would corrupt an evidence trail that the product's whole premise depends
on being trustworthy.

## 4. Decision — §3: MCP tool surface

### 4.1 The read tools the agent may call

| Tool | Required args | Optional | Returns | Cost bound |
| --- | --- | --- | --- | --- |
| `explain_episode` | `episode_id` | `include_events` (bool, default false) | episode record + ranked `reason()` output (rule, score, factors, citations) + confidence + `unknowns` | ≤ `max_items` citations (default 8) |
| `changes_between` | `ref_a`, `ref_b` | `path_filter` | `changes[]` = `{change_id, kind, risk_tier, paths[], evidence[]}`, **capped at 200** | `max_changes` |
| `architecture_delta` | — | `from`, `to` | packages added/removed, cycles added, boundary crossings, layer reassignments | cycles ≤ 50, crossings ≤ 200 |
| `dependency_delta` | — | `package` | prod/dev split, cycles, removed-while-still-imported | entries ≤ 200 |
| `risk_report` | — | `ref_a`, `ref_b`, `min_tier` | ranked changes with `factors[]`, `confidence`, `model_version`, `model_artifact_id`, `training_corpus_version`, `grounding` | ≤ 100 |
| `search_history` | `query` | `k` (default 8, max 32), `since`, `author` | **pointer-shaped**: `episode_id`, `ts`, `title`, `score`, `matched_on`. **Never content.** | `k` |
| `search` | `query` | `limit` | `episode_id`, `rule`, `score`, `snippet` (≤ 200 chars, redacted, only when the requester holds `snippets` capability) | `limit` |

Three cross-cutting rules:

- **Every response carries `unknowns`** (which signals were missing, whether the change was ever
  reviewed, whether the corpus is old). "Confidence 0.9 with empty `unknowns`" must be impossible.
- **Every response carries `cost_ms`, `bytes_returned` and `degraded`.** An MCP call runs inside the
  agent's turn budget; an unbounded result is a latency outage wearing a feature's clothes.
- **Nothing throws at the caller.** Errors are structured (`{error: {code, message}}` with codes from
  `unknown_tool`, `bad_args`, `out_of_bounds`, `no_model`, `not_reviewable`, `internal`).
  A tool that raises ends the agent's turn; the boundary returns a value.

### 4.2 The gated write tools

| Tool | Args | Gate |
| --- | --- | --- |
| `mark_reviewed` | `change_id`, `reviewer`, `note` (opt) | requires `write` capability **and** an approval token bound to `change_id`; idempotent per `(change_id, reviewer)` |
| `add_review_note` | `target`, `body`, `kind` | requires `write` capability; `body` ≤ 4096 B; `target` must resolve to a known id |

`mark_reviewed` marks **a human looked**, which is a claim about a human, not about code. The
"already reviewed" state it writes lands in the change ledger (`reco-rlf`), which does not exist yet
(`reco-552`), and `reviewed` has no canonical event in §2.1 either — so *this* tool is a typed
request plus the ledger entry shape, and the honest half is §8's deferral. The agent may not call it
without `write`; an auto-approval path that could mark things reviewed without a human is refused at
schema level, not by a prompt asking nicely.

### 4.3 What is never returned

**Never, in any tool response, in any field:**

- **Secrets** — anything the `reco-h7p` scanner matches (API keys, tokens, PEM blocks, high-entropy
  strings) is redacted as `[REDACTED:<rule>]` **before** serialization, in the server, not by the
  model. Redaction runs on outbound content because an allowlist of fields fails the first time a
  vendor adds one.
- **File contents outside the model** — no tool returns a file body. Pointer-shaped results only
  (`path`, `sha256`, `line_span`, `episode_id`, `rule`). "Show me the file" is the agent's problem,
  through its own permission system, where the user can see it happen.
- **Anything the caller is not entitled to** — an MCP client may present a `capabilities` list; a tool
  the session has not been granted returns `{error: {code: "no_capability"}}` and is *absent from
  `tools/list`*. A tool you can see but cannot call is a trap; the honest surface is the callable one.

## 5. Decision — §4: versioning and capability negotiation

### 5.1 Versioning rule

- **Wire format** is versioned per adapter: `claude_code/1`, `codex_cli/1`, `opencode/1`. Bump the
  major on any *removal*, *rename*, or *type change*; additive fields do not bump it. Unknown fields
  are **preserved verbatim**.
- **Canonical events** are versioned once, as `fathom_event_schema = 1`. A change to an existing
  field's meaning is a major bump. There is no v2 and there should not be a v2 until something is
  actually wrong.
- **MCP tool contracts** carry `contract_version` on the response envelope, and the server publishes
  `contract_range: [1, 1]`.
- **Additive evolution:** a tool may add response fields; a client may ignore unknown fields. A tool
  may **not** change a field's meaning, remove a field, or tighten an argument without a major bump.
  Breaking changes are additive-first: publish `field_v2` beside `field`, mark `field` deprecated with
  `deprecations[]` in `tools/list`, and keep both for one minor release.
- **A session may not mix schema majors.** Mismatch is refused at `initialize` with an explicit
  `incompatible_version`, rather than half-working and producing subtly wrong records.
- **An unknown event type is additive-safe**: stored, projected as `null`, counted. The cost is
  bounded (a `by_type` histogram entry); the alternative — an ingest that dies on a schema the vendor
  added last week — is an outage.

### 5.2 Capability negotiation

At `initialize`, the client sends `protocol_version` and `capabilities`. The server replies with the
**intersection**, never an overclaim, plus `contract_range`, `deprecations`, and `server_version`.
Negotiated state is attached to every response so a mismatch is visible in the artifact, not only in
logs. Declared-but-unimplemented capabilities are reported `"status": "unimplemented"` rather than
omitted or faked; `tools/list` never advertises them.

## 6. Decision — §5: trust model

The harness is **untrusted input, hostile by default**: a user may hand-edit `~/.claude/settings.json`,
a plugin may emit crafted hook payloads, and a repo may contain `.claude/settings.json` that does the
same. The threat is prompt injection *through* event content and resource exhaustion *of* the ingest
path.

Six validations, each with the failure it prevents:

1. **Path containment.** `file_edit.path` must resolve inside `cwd`; absolute paths, `..`, symlink
   escape, NUL bytes and control characters are refused (`path_escape`). An ingestion that reads the
   paths it is told about reads whatever an attacker names.
2. **No execution.** Nothing in an event is `eval`'d, imported, shell-parsed, or used to build a
   command. `test_run.command` is stored as a string and only ever *displayed*.
3. **Size and rate limits** at three levels: per line (§3.6), per session, and total (`retention_days`,
   default 90). **Rotation is deferred** (§8.4). `max_bytes_per_session` (64 MiB default) is enforced
   *and over-quota sessions are quarantined* (`sessions.<id>.quarantined`, ingest skipped): an ingest
   that accepts without bound is a disk-full outage with an attacker's help.
4. **Redaction at ingest** for the tool-input/output fields (§4.3), *and* again at MCP egress (§7).
5. **The payload is data, never instructions.** Tool output and assistant text are embedded in
   citations and diffs as inert, length-bounded strings with a marker that they are untrusted;
   ingest adds no directive that the model is asked to obey.
6. **`schema_version` is checked before dispatch** and never defaults to the newest version. Defaulting
   to "whatever we support now" is how a hostile file gets to pick the most permissive parser.

**`tool_input_sha256` is a real digest over the redacted payload**, not a placeholder: a stored hash
lets us prove later which input produced a claim — the evidence chain the whole thesis rests on —
without keeping the input itself.

## 7. Decision — §6: what lands where

| Artifact | Written by | § |
| --- | --- | --- |
| `.fathom/events.jsonl` | adapter ingest (`reco-afh`) | §3 |
| `.fathom/events.idx.json` | `fathom reindex` | §3.4 |
| `.fathom/sessions.json` | adapter ingest (quarantine + per-session accounting) | §3.5, §6.3 |
| MCP responses | `reco mcp` (`reco-p3i`) | §4 |
| `.fathom/review_ledger.jsonl` | `mark_reviewed` | §4.2 (gated) |
| `.fathom/export/harness-events.jsonl` | `fathom export` (`reco-1tn`) | §4.3 |
| `harness.status` | `harness status` (`reco-afh`) | §11 |
| `probe:` machine records | `harness verify` / `harness build` | §10 |

## 8. Consequences — the four clauses that are *not* implementable here

Recorded with named reasons, because a checkbox that passes on fiction is worse than an open issue.
**§8.1–§8.3 are blockers on other tickets and this document does not have authority to resolve them.**

1. **Backpressure — deferred.** `reco-kvk` (bounded queue, shed-with-count) needs an ingest *queue*.
   §3.5 writes synchronously under `flock`: there is no queue to bound, and a fake bounded queue with
   nothing behind it would be theatre.
2. **Risk model training — deferred.** No corpus ≥ threshold, no GPU, no trained artifact exists.
   `risk_report` runs a deterministic heuristic whose `model_version` is literally
   `heuristic:<sha256-of-function-source>` and whose `model_artifact_id` is `null`, so a consumer can
   tell provenance from the artifact. It is **clearly labelled** `grounding: "heuristic — no trained
   model (reco-884)"` and `trained: false`, because presenting it as a trained model is precisely
   ADR-001's "plausible fiction". `reco-884` (the ≥80% calibration target) stays open and is not
   marked done here. `training_data_policy: "not-implemented"` states the corpus consent state
   honestly rather than leaving the key out.
3. **Semantic drift — detection only.** `reco-cpr` *detects* drift, so the honest implementation
   detects and reports it, with an auto-update path left explicitly unimplemented (a daemon that
   rewrites a model is its own reviewable change).
4. **`reason()` timing SLA — deferred.** `reco-2rx` (`e4.7`, the deterministic explanation that
   `reason()` will be) is not implemented;
   there is nothing to measure against `reason_p95_ms`. Recorded as `blocking: reco-2rx`.
5. **MAP_SHARED — done.** *Closed by this ticket* (§10): `elf_probe.py` performs the
   `mmap(PROT_READ|PROT_WRITE, MAP_SHARED)` + `mprotect` + touch + `munmap` probe and reports
   `map_shared_ok`/`mmap_len_ok`/`mprotect_exec_ok`. `reco-f28` may depend on it.
6. **Cross-tool consistency — deferred.** **Only Claude Code is installed.** `codex_cli.py` and
   `opencode.py` pass fixture round-trips against *our own* recorded fixtures, which proves our
   decoder decodes our fixtures and nothing more. Their mappings are **[external]** (§9).
   `reco-7go` (`e6.2`, the reference adapters) stays open until both capture steps above have
   actually been run against installed vendors.
7. **Rotation — deferred**, as stated above.

## 9. Consequences — what must happen to promote [external] → [observed]

Two tools are **not installed** and the network is **not reachable** (no `curl`, no `nslookup`). Until
each step below has been run, the adapter named stays **[external]**:

```bash
# 1. install, then discover the real schema rather than trusting this file
codex --version && codex hooks --help            # or the vendor's equivalent discovery command
opencode --version
# 2. capture a real session and diff it against our fixtures
codex run --hook-file /tmp/codex.jsonl "make the tests pass"
diff <(jq -s 'map(keys)' /tmp/codex.jsonl) <(jq -s 'map(keys)' tests/fixtures/codex_cli.jsonl)
# 3. only then: flip the adapter's PROVENANCE, bump major on any change, update fixtures
```

Re-verify on **every vendor update** — pin a version in `harness.status.adapters[].installed_version`,
and re-run the capture when it changes. Vendor schemas are reverse-engineered here, and
`docs/adr/README.md`'s hard rule is that prose and code may not disagree.

## 10. Decision — §7: the runtime / libc ABI probe, as executable code

`e1.4` (mmap-backed incremental index) and `e1.10` (`fathom doctor`) both rest on runtime claims
that prose cannot check. **`src/recommender/harness/elf_probe.py` is that check**, and it is a hard gate
on `harness run`, `harness verify` and `harness build`. Six things it establishes:

1. **No compiler, and none required.** `gcc`, `cc` and `make` are **absent** from this container, so any
   contract that expects to build a C shim is fiction. Every call is ctypes against the system libc;
   the probe *asserts the absence of compiler dependence* (`no_compiler_required=true`, and it fails
   if a `.so` appears in the tree). A contributor who reintroduces a compile step breaks
   `harness verify` here rather than silently working on one machine only.
2. **An explicit symbol contract** with `argtypes`/`restype` set on purpose — `gnu_get_libc_version`,
   `gnu_get_libc_release`, `sysconf(_SC_PAGESIZE)`, `mmap`, `munmap`, `mprotect`, `getpid`. Without
   `argtypes` ctypes assumes `int` and truncates a 64-bit pointer before the call ever happens.
3. **Machine-greppable ABI fields** — `harness verify --machine` / `harness build --machine` print
   `libc=<path-or-not-found>`, `libc_version=`, `libc_version_major=`, `libc_version_minor=`,
   `libc_flavour=`, `libc_machine=`, `page_size=`, `pagesize=`, `pointer_size=`, `wordsize=`,
   `float_abi=`, `pointer_significance=`, `map_shared_ok=`, `mmap_len_ok=`, `mprotect_exec_ok=`,
   `abi_version=` and `symbol.<name>=present|missing`.
4. **`probe: libc=not-found`** is an emitted state, not an exception: libc resolution is by
   **verified SONAME** (parsed out of the ELF dynamic section in pure Python — no `ldd`, no binutils),
   never by "the first thing under `/lib`", because `CDLL` will happily open an object that is not the
   libc the caller meant.
5. **ABI-mismatch detection is explicit, because a naive check reports success.** Observed on an
   earlier container state that carried an x86-64 `libc.so.6` on an aarch64 host: a plain `ctypes`
   `dlopen` of it **succeeded** and reported `ok`. Two mechanisms
   catch it, and the probe uses both: an ELF `e_machine` comparison against the host (`float_abi`,
   `libc_machine`), and `ld.so --verify`, the platform's own answer to "does this object belong
   here" (rc 0 = yes; rc 1/4 = no). `ctypes` exposes neither `RTLD_UNSAFE_ABI` nor
   `RTLD_DONT_LOAD_DEPENDENTS` in a standard CPython build, so a dlopen-mode-based check is not
   available at all.
6. **Numeric ABI facts are derived, never remembered.** Two traps this closed, both real:
   `_SC_PAGESIZE` is **not** portable as a literal — hardcoded to 39, this glibc returned **1000**, a
   plausible-looking wrong page size; the constant now comes from `os.sysconf_names['SC_PAGESIZE']`
   (30 here) and the probe *cross-checks libc against Python*. And `mmap` **rounds a
   non-page-multiple length up** rather than failing, while a **zero-length** request is the one that
   must return `EINVAL` — so `mmap_len_ok` asserts the zero-length rejection, which is the property
   an index-file writer actually depends on.
   `probe_summary()` also feeds `harness status --json`'s `ffi` key.

## 11. Consequences

### 11.1 `harness status` payload (contract v1)

```json
{"events_file": "/srv/app/.fathom/events.jsonl", "protocol": "reco-harness-hooks/1",
 "exists": true, "lines_total": 42, "bad_lines": 0, "last_seq": 42,
 "ingest_ok": 40, "record_rejections": {"path_escape": 1, "oversized": 0, "schema_version": 0},
 "unprojectable": {"total": 1, "by_type": {"WorktreeCreate": 1}},
 "sessions_quarantined": 0,
 "adapters": [{"name": "claude_code", "provenance": "observed", "installed_version": "2.1.4",
               "wire_protocol": "claude-code/1"}],
 "mcp": {"read_only": true, "contract_range": [1, 1], "tools_total": 9,
         "tools_by_capability": {"read": 7, "write": 2}},
 "abi": {"abi_version": "harness-ffi-1", "abi_verdict": "compatible",
         "abi_verdict_detail": "/lib/ld-linux-aarch64.so.1 --verify .../libc.so.6: rc=0",
         "libc_machine": "aarch64", "host_machine": "aarch64"},
 "unknowns": ["codex_cli and opencode are not installed: their mappings are [external] (§9)",
              "no mmap-backed index exists yet: reco-f28 consumes the probe's contract only",
              "reason() SLA and backpressure deferred (§8.1, §8.4)"],
 "blocking": ["reco-2rx (reason SLA)", "reco-552 (shared review memory)"],
 "ok": true}
```

Two deliberate properties. `harness status` **exits 0 with `ok:false`** rather than raising: it is a
diagnostic surface, and a non-zero exit buried in a diagnostic is what `fathom doctor`
(`reco-ld8`) exists to interpret. And `ok` is false here *because the ABI probe correctly refuses
the mismatched libc* (§10) — a green status on a host that cannot run what it names would be the same
category of bug as a passing test for an absent vendor protocol.

### 11.2 `harness verify --machine` contract

`probe: harness=ok`, `probe: compiler=none-required`, `probe: libc=<path>|libc=not-found`,
`probe: mmap_len_ok=`, `probe: mprotect_exec_ok=`, plus the field names §10 lists. **Exit 0 only if
the probe completed.** The `probe:` lines are emitted for **every** subcommand startup: `harness run`
calls the FFI gate before any engine or plugin operation, so the contract cannot be skipped.

### 11.3 Cost of the two-layer events design

Every adapter carries a projection function and every projection mistake shows up as a `by_type`
histogram entry rather than as a crash. That is the trade: a little adapter code now in exchange for
coverage visibility forever.

### 11.4 What this makes safe to build next

`reco-afh` (the harness contract), `reco-p3i` (the MCP server), `reco-nts` (episode segmentation),
and `reco-f28` (mmap-backed index: it may now depend on the MAP_SHARED contract in §10) become
buildable without inventing a wire format, an event union, a tool surface, or an ABI claim as you go.
`reco-7go` (`e6.2`) **stays open** until §9's captures are run.

### 11.5 Reference implementation

`src/recommender/harness/` — `harness.py` (ingest, index, status, `verify`, `build`, **`mcp`**),
`elf_probe.py` (§10), `src/recommender/hooks/` (adapters + MCP server + `reco-hooks` installer),
`tests/test_harness_contract.py` (machine-greppable contract checks). It is a contract fixture: it
exists to make the contract checkable. CI runs the suite as an **`ffi`**-marked job.

## 12. Alternatives considered

- **One event format, the vendor's.** Rejected: every consumer would branch per vendor, and a vendor
  update becomes a product outage.
- **One event format, ours, discarding vendor detail.** Rejected: it destroys the evidence trail that
  ADR-001's differentiator requires, and it makes §9's re-verification impossible — you cannot diff a
  vendor protocol you never stored.
- **An index line inside `events.jsonl`.** Rejected (§3.4): dual-purpose records in an append-only file
  rebuild neither cleanly nor safely.
- **Version the canonical events per adapter.** Rejected: the projection boundary *is* the version
  boundary; two version axes on one file guarantees they drift apart.
- **Gate MCP writes by prompt ("please don't mark things reviewed").** Rejected: the gate is
  capability + approval-token + idempotency, enforced in the server.
- **Expose the file-reading tool over MCP for convenience.** Rejected: §4.3. `search_history` returns
  pointers; contents stay the agent's problem, under the user's permission system.
- **A hand-written ADR-006 index in `docs/adr/README.md`.** Rejected: `tools/check_docs.py` regenerates
  the table from the ADRs and fails CI on drift, so there is one source of truth.
- **Annotate the §10 claim with "untested".** Rejected: the claim is checkable, so it is checked. prose
  is what produced the mismatched-libc situation in the first place.

## 13. Verification — the commands that *are* the acceptance criteria

Run from the repository root. All four must pass; none needs a compiler, a network,
or a vendor install.

```bash
PYTHONPATH=src python3 -m recommender.harness.elf_probe --machine   # §10 ABI fields, greppable
PYTHONPATH=src python3 -m recommender.harness.harness run verify    # §10 startup gate
PYTHONPATH=src python3 -m recommender.harness.harness status --json # §11.1 payload
PYTHONPATH=src python3 -m pytest -q                                 # every clause above
python3 tools/check_docs.py                                         # docs/plan/id consistency
```

The pytest suite is the executable form of this document: `test_probe_prints_every_machine_field`
fails if a §10 field disappears, `test_concurrent_writers_do_not_collide` fails on a duplicate `seq`,
`test_torn_and_bad_lines_are_counted_not_fatal` fails if ingest becomes fatal on damage,
`test_write_tools_require_capability_and_token` fails if a gated write loses its gate,
`test_adapter_provenance_is_honest` fails if an uncaptured vendor adapter ever claims `observed`, and
`test_no_file_contents_cross_the_boundary` fails if a tool ever returns a body. A clause that has no
test is a clause that will drift.

### 13.1 Measured environment facts (2026-02-11)

Recorded because three of them contradict what the code would assume if it were written from memory:

| Fact | Value | Consequence |
| --- | --- | --- |
| Python | 3.14 on the system interpreter; the venv from earlier in the ticket was lost to a sandbox reset | the fixtures stay stdlib-only so any interpreter can run them; `pytest` is the only third-party dep (`pip install pytest`, which does reach the network even though `curl`/`nslookup` are absent) |
| C toolchain | **absent** (`gcc`, `cc`, `make` all missing) | any compile-step contract is fiction here (§10.1) |
| libc | glibc **2.41**, `aarch64`, `/usr/lib/aarch64-linux-gnu/libc.so.6` | `libc_version` is measured, never hardcoded |
| `_SC_PAGESIZE` | **30** here; hardcoded 39 made glibc answer **1000** | constants come from `os.sysconf_names` (§10.6) |
| page size | 4096, confirmed by libc `sysconf` and `os.sysconf` | `pagesize_confirmed=true` is a cross-check, not a tautology |
| `mmap` length semantics | non-page-multiple length **rounds up**; length 0 → `EINVAL` | `mmap_len_ok` asserts the zero-length rejection (§10.5) |
| vendor installs | **none** of `claude`, `codex`, `opencode`; pi 1.1.0 present | only `pi_json` may claim `observed` (§9) |
| outbound network | no `curl`, no `nslookup` | §9's capture procedure is a prerequisite, not a nicety |
