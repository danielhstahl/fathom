# ADR-001 — Product thesis, glossary, north-star metric and non-goals

| | |
|---|---|
| **Status** | Accepted (draft 1). One open item: harness-owner ACK on the non-goal boundary (§8), requested in a comment on `reco-982`. |
| **Date** | 2026-10-14 |
| **Ticket** | `reco-982` (plan key `e0.1`) |
| **Deciders** | danielstahl (product + coding-harness owner) |
| **Supersedes** | nothing — this is the first document |
| **Referenced by** | every later ticket; ADR-002…ADR-006 in particular |

> **How to use this document.** This is the document later tickets cite. Three rules:
>
> 1. If a ticket uses a noun defined here, it must use it with exactly this meaning. If it needs a
>    different meaning, it is a different noun and gets a new entry here — never overload an existing one.
> 2. New nouns enter the vocabulary of this project by being added to the glossary (§4) first, in the same
>    change that uses them. One term, exactly one entry, one meaning.
> 3. A later ADR may narrow or amend this one, but it must say so explicitly and update the affected
>    section here. Forking the vocabulary into a second document is the failure mode this rule exists to
>    prevent.

**Contents.** [1 The thesis](#1-the-thesis) · [2 Who the user is](#2-who-the-user-is) ·
[3 The model in one picture](#3-the-model-in-one-picture) · [4 Glossary](#4-glossary) ·
[5 Metrics](#5-metrics) · [6 What would falsify this](#6-what-would-falsify-this) ·
[7 Principles](#7-principles-the-thesis-commits-us-to) · [8 Non-goals for v1](#8-non-goals-for-v1) ·
[9 Open questions](#9-open-questions) · [10 Sign-off](#10-review-and-sign-off)

---

## 1. The thesis

**The bottleneck in agentic software delivery is no longer writing code. It is the human rebuilding a
correct mental model of what the agent did.**

Everything else in this plan follows from that sentence, so it is worth unpacking.

Generation got cheap. An agent produces a hundred commits in the time it used to take a person to type
them, and it produces them in parallel, overnight, across branches nobody is watching. What did not get
cheaper is the part still on the critical path: one human forming a trustworthy understanding of what
the repository now *is* — where the change landed, what it now depends on, which contracts it broke,
what was claimed and quietly not done — and being willing to put their name on it.

That model is hard to rebuild for three structural reasons, and they are why a diff viewer does not
solve the problem:

- **A diff is text between two blobs; it is not a claim about the world.** `+1,200 −90` says nothing
  about whether the change is a rename, a new dependency, a deleted test, or a reformat. Rebuilding the
  model means reading, and reading does not scale with generation.
- **Agent history is not a narrative.** One intent arrives as five commits or as zero commits and a
  dirty working tree; commits interleave with a human's own; a session ends in `rebase -i` and a
  force-push, and the story you had in your head no longer matches the graph.
- **A wrong model is paid for late.** The cost of not understanding a change is not zero until you look;
  it materialises weeks later as rework, and rework is the most expensive unit in software.

So the product is not "a better diff viewer". The product is **turning agent output into something a
specific human can gain confidence in fast, and keeping the record that they did.** Fathom is the
confidence half of a two-part system: the coding harness produces the change, Fathom produces the
understanding. Together they are how a person delivers agentic software with confidence.

**The falsifiable form.** The thesis is not a slogan; it can be wrong. It claims a causal link:

> If we shorten **time-to-confidence** (§5) for a fixed body of agent output, then a team can accept a
> larger share of agent output per reviewer-hour *without* an increase in rework rate.

If shortening time-to-confidence does not raise accepted throughput, or raises it only by pushing rework
downstream, the thesis is false and this product is a nicer git viewer rather than infrastructure. §6
lists the observable conditions we would treat as evidence against it, and §5 defines the numbers that
make the argument checkable.

---

## 2. Who the user is

**The primary user is the *accountable human*: the person on the hook for software they did not type.**
Call them the reviewer.

They may be a solo builder running two agents in tmux, a tech lead who owns a component an agent is now
editing, or a founder whose entire backend is agent-written and who still gets paged at 03:00. The
common factor is not seniority — it is **accountability without authorship**. They did not write this
code, they will be blamed for it, and they are smart enough to know that "the tests are green" is not an
understanding of a system.

**Their job, in one sentence:** convert "the agent produced 43 commits in the last 40 minutes" into "I
understand change set *K* and I accept it, and here is the evidence" — repeatedly, at a rate that keeps
up with the agent, without becoming the bottleneck themselves.

**Their physical situation.** One terminal running the coding harness; a second terminal running Fathom,
open all day. Keyboard only. 80×24 is a real target, not a joke. They glance rather than study, and
switch to studying only when something warrants it. Interrupting them is expensive; irrelevant output is
worse, because it teaches them to ignore the window.

**What they need, derived from the persona** (each is a constraint on later tickets, not a wish):

| Need | Consequence |
|---|---|
| Trust | Every number, label and sentence links to the evidence that produced it. No claim without a citation (§7.1). |
| Speed | Answers in seconds, not builds. "Did that turn make the architecture worse?" must be a glance, not a research project. |
| Rank | An ordered list of what deserves attention beats a complete list. The queue, not the diff, is the home screen. |
| Durability | Sign-off must survive `rebase -i`, amend, squash and force-push, or the reviewer must re-review — and re-review is what they bought the tool to avoid. |
| Honesty about ignorance | "Unknown, because no hook events" is a correct and shippable answer. A confident guess is a defect. |
| No new dependency | Works offline, on their machine, on their repo, with no account and no network. If Fathom needs a server, it has failed the persona it serves. |

**Secondary users**, in priority order. They are real, and they are why the analysis model is an API
rather than a screen (§4, *insight API*):

1. **The coding agent.** It reads the same review model over MCP (`e6.4`) to answer "what did I just
   change and is it risky" before it asks a human. The agent is a consumer of the model, never its
   author.
2. **CI.** `fathom check` (`e6.6`) consumes the same model as a gate that fails only on new violations.
3. **The downstream reader.** A staff engineer, an auditor, or the reviewer's future self, who never
   opens the TUI and only ever sees the exported review artefact (`e5.7`).

**Explicitly not the user for v1:** people reviewing in a web UI; live pair-programming or real-time
collaboration on the same diff; people who want an agent runner with a nicer front end; code-learning
"explain this codebase to me" tourists. Where these appear later, they appear as amendments to this ADR,
not as scope creep inside a feature ticket.

---

## 3. The model in one picture

The five core nouns are stages of one pipeline. Later tickets should cite this picture when they need to
say where they live.

```
      coding harness                      Fathom                                  human
  ┌───────────────────────┐   ┌──────────────────────────────────────────┐   ┌──────────────┐
  │ agent turn            │   │                                          │   │              │
  │  prompt · tools ·     │──▶│ EPISODE   one turn = one reviewable unit │   │  reads,      │
  │  edits · tests        │   │    │                                     │   │  ranks,      │
  │  (hooks v1, e6.1)     │   │    ▼                                     │   │  dismisses,  │
  └───────────────────────┘   │ DELTA     evidence-backed what-changed   │──▶│  signs off   │
                              │    │                                     │   │    │         │
                              │    ▼                                     │   │    ▼         │
                              │ ATTENTION QUEUE  ranked worklist ────────┼──▶│ REVIEW       │
                              │    │                                     │   │ LEDGER       │
                              │    ▼                                     │   │ (durable,    │
                              │ AMBIENT MODE  glanceable second terminal │   │  committable)│
                              └──────────────────────────────────────────┘   └──────┬───────┘
                                                                                    │
                                            rework ≤ N later episodes ◀─────────────┘
                                            (the penalty signal: a wrong model, found late)
```

Reading it: the harness produces turns; Fathom turns turns into **episodes**, describes each episode with
a **delta**, ranks unreviewed episodes into an **attention queue**, and sits in **ambient mode** while
the human does something else. The human's dispositions land in the **review ledger**, which is both the
output people trust and the input to every metric in §5. The rework arrow is the one that matters: work
that comes back is proof the mental model was wrong.

---

## 4. Glossary

**Rule:** one term, one entry, one meaning. Entries name the ticket or ADR that *owns* the detail; this
document defines *what the word means*, the owner defines *how it works*. Terms are grouped, then
alphabetical within a group. A term not listed here is not project vocabulary yet — add it here first.

### 4.1 The five core nouns

**Episode** — the atomic reviewable unit: the change produced by **one agent turn**, materialised as the
ordered set of commits that turn produced (plus, optionally, its uncommitted working-tree state as a
*provisional episode*). Identified content-addressedly so it stays the same object across history
rewrites. *Not* a commit (an episode has one or more), *not* a branch, *not* a PR, *not* a session (a
session contains many). Everything reviewable, review state, finding and metric in the product is keyed
to an episode. — identity and segmentation: ADR-003 (`e0.3`), implementation `e1.3`.

**Delta** — the versioned, evidence-backed description of what changed for one episode, or between any
two endpoints: structure, dependencies, architecture, non-code surfaces, test surface, and what was
suppressed as noise. A delta is a **claim with citations**, not a text diff: each assertion carries the
evidence ids that support it and a confidence. The machine-readable form is `EpisodeDelta` (§4.4).
*Diff vs delta:* a diff is byte-level text between two blobs and explains nothing; a delta is a
state-level claim and cites its sources. — `e2.11`, schema `schemas/episode-delta.schema.json`.

**Attention queue** — the ranked, keyboard-driven worklist of episodes a human has not yet
dispositioned, ordered by risk weighted by staleness and blast radius. This is the product's main
workflow and its home screen; the timeline is a view, the queue is the job. Every entry leaves the queue
by exactly one of four dispositions: *accepted*, *flagged*, *dismissed with a reason*, *snoozed*. The
queue must explain its own ordering. — `e4.5`, ranking formula `e4.4`.

**Review ledger** — the durable, human-readable, committable record of human dispositions: sign-offs,
notes, decisions and dismissals, keyed to episode identity plus a content fingerprint, stored as
markdown under `.fathom/reviews/` with a mirrored index so it can travel in a PR. Re-linking
reconnects dispositions after rebase, amend and squash; a ledger entry whose content has since changed
is reported **stale** rather than silently trusted. The ledger is the audit trail and the source of the
`review.*` fields every metric is computed from. — `e4.6`, identity coupling ADR-003.

**Ambient mode** — the low-attention display mode Fathom runs in while an agent works: a compact pane
(fits 80×12) showing the latest episode and its risk, how many episodes are waiting, index state, and
the single most important reason to look now; quiet and dim when caught up, no animation loops, no colour
strobing. One keypress drills into the full view. Ambient mode is a *display mode*, not a separate data
path — it renders the same delta model. — `e5.1`, layout ADR-006.

### 4.2 People and roles

**Accountable human / reviewer** — the primary user (§2): the person accountable for software an agent
wrote. Canonical term is *reviewer*; *accountable human* is the expansion used when the accountability
matters. — §2.

**Agent** — the coding harness and the model driving it: the thing that writes code. Fathom never runs,
steers or retries an agent (§8). — boundary contract ADR-005.

**Coding harness** — the external system that runs agent turns and emits *hook events*. Fathom is
designed to be its review half, not its part. — ADR-005 (`e0.5`).

**Auditor / downstream reader** — a secondary user who sees only the exported artefact: a staff
engineer, lead, compliance function, or the reviewer's future self.

**Author kind** — the attribution label on a commit or episode: `agent`, `human` or `unknown`, plus
confidence and evidence. `unknown` is a legitimate terminal value, never silently bucketed. — `e4.1`.

**Identity** — the reviewer's name as recorded on a ledger entry, taken from git config or explicit
config. A sign-off without an identity is not a sign-off. — `e4.6`.

### 4.3 History and identity

**Commit** — a git commit. Input, not unit of review; commits are *members* of episodes.

**Episode identity** — the stable address of an episode: a content digest of the facts a rewrite
*preserves* — who authored it, what its normalised subject claims, which paths it touches — used to
re-link the same logical episode after the graph is rewritten. The member SHAs and the parent boundary
are its *extent* (§4.3), never its identity. Amended by ADR-003 (`e0.3`), which rejected member-SHA
identity because a rewrite changes exactly those.

**Episode extent** — which commits an episode covers and what its diff is compared against: the ordered
member commit SHAs plus the parent of the first member. It changes on every rewrite; the identity must
not. — ADR-003 §3.

**Content fingerprint** — the content-derived digest of an episode's change that survives a rewrite when
the SHAs do not; the mechanism by which sign-offs and notes re-attach after rebase, amend or squash.
ADR-003 §2.2 publishes **three** exact digests (author+subject+paths, author+subject, author+paths) plus
one shared-commit test, in place of the single fuzzy fingerprint the ticket proposed — see ADR-003 §13.1
for why similarity is deferred. — ADR-003, verified by `e4.6`.

**Digest tier** — one of the ordered re-link tests ADR-003 §6 runs when an episode's id moved: strongest
tier that matches wins, the match is always written down, and a weaker tier never overwrites a stronger
one.

**Episode twin** — a second live episode with the same identity digest as another live episode: the same
change landing twice (a cherry-pick, a duplicated "bump version"). Twins are separate episodes with
separate review state, disambiguated by an ordered suffix. — ADR-003 §2.3, §5.9.

**Fail-visible** — the tie-break of last resort for episode identity: when Fathom cannot *prove* that a
rewritten change is the one a human already reviewed, the change returns as unread rather than inheriting
the old decision. Re-reviewing is annoying and visible; trusting the wrong change is invisible. —
ADR-003 §6.3.

**Lineage** — the ordering context an episode is measured within: the ref/branch sequence of episodes
after it, used to define windows such as "the next N episodes". — `e1.3`, used by §5.

**Provisional episode** — an episode built from uncommitted (staged or unstaged) working-tree state so
the reviewer can see work in progress. Provisional state is always visible as such; nothing is
permanently keyed to a provisional identity, and no provisional episode may be signed off. — `e1.4`
(the cache half, which exists), ADR-003 §7 (the rules), `e1.6`/`e5.1` (the row writer, which does not yet).

**Rewrite** — any history mutation that changes SHAs without changing intent: amend, `rebase -i`, squash,
merge train, cherry-pick, force-push. A rewrite must not orphan review state; that is a bug class, not a
limitation. — ADR-003, `e4.6`.

**Session / turn** — harness vocabulary: a *session* is one agent conversation, a *turn* is one
prompt→work cycle within it. A turn is the default source of an *episode* boundary. When hook events are
absent, boundaries fall back to trailers and heuristics, and the fallback reason is recorded. — ADR-005,
ADR-003.

### 4.4 Analysis model

**Delta model / `EpisodeDelta`** — the single versioned, typed structure of per-episode analysis,
exposed by the insight API and consumed identically by the TUI, CLI, exporter, CI gate and MCP server.
No surface computes its own version of reality. Additive changes allowed; breaking changes bump the
version and fail CI. — `e2.11`. §5.5 states which of its fields the metrics require.

**Insight API** — the read interface to the delta model: `fathom analyze <episode> --json`, the
in-process query API, and the MCP tools that expose the same thing to the agent. — `e2.11`, `e6.4`.

**Evidence (evidence id)** — the addressable proof behind a claim: a file-and-line range, a hunk, a
lockfile line, a graph edge, a deleted test, a config key. Every finding, card figure and narrative
sentence cites evidence ids that resolve to a view. "Every number is a button" is the user-visible form
of this rule (§7.1). — `e2.10`, `e2.11`.

**Finding** — a named, evidence-backed observation with a detector id, severity, confidence, jump target
and a suppression key. Findings are the atoms of risk; a finding without evidence is not a finding. —
`e4.3`, catalogue `e7.5`.

**Detector** — a pure function over the delta model that returns findings. Deterministic, testable,
enable-able per repo, and documented with a worked example. Out-of-tree detectors are the plugin story.
— `e4.3`, `e6.7`.

**Path taxonomy** — the single place that decides what a path *is* (source, test, docs, config, schema,
migration, generated, vendored, lockfile, asset, binary) and how noisy it is, explainable per path.
Nothing downstream encodes its own glob list. — `e0.9`, used by `e2.1`.

**Noise / noise suppression** — change that carries no reviewable information: reformat-only,
generated-file churn, lockfile-only refresh. Suppression is a **view, never a deletion**: the raw content
is always one keypress away and the rule that hid it is printed inline. *Noise share* is the fraction of
an episode's change so classified, and it is a risk-score input. — `e2.2`, policy `e0.9`.

**Structure delta** — where a change landed: directories added/removed/touched, subtree moves, files
created/deleted/renamed, and the structure-churn score that makes "the agent reorganised the tree"
obvious. — `e2.3`.

**Dependency delta** — the resolved dependency *state* before and after an episode, diffed as a graph
rather than as lockfile text: new/removed direct deps, upgrades and downgrades, transitive-only changes,
source switches, integrity changes, licence-set delta. — `e2.5`, `e2.6`.

**Non-code surface** — the change classes that break production and rarely appear in source files:
environment variables, feature flags, database migrations, CI workflow and permissions, IaC,
Dockerfile/base image. — `e2.8`.

**Test surface** — the testing contract as a diffable quantity: test lines vs production lines, tests
added/renamed/deleted/skipped, weakened assertions, and production logic added with no matching test
surface. Not a coverage percentage. — `e2.9`.

**Architecture delta** — the diff of the per-revision module/symbol graph: added, removed and changed
nodes and edges, symbol moves (distinct from delete+add), new or resolved cycles, layering-violation
delta, public API surface delta with a semver recommendation, and hotspots. — `e3.3`…`e3.8`.

**Blast radius** — an episode's reach: the set (and count) of modules, files or symbols that directly
depend on what changed, plus a one-hop estimate; path heuristics when the graph is unavailable, with the
reduced confidence stated. — `e2.10`, graph `e3.2`.

**Module graph / graph snapshot** — nodes are modules and exported symbols, edges are imports, uses,
implementations and inheritance, tagged with layers; stored one snapshot per tree hash with structural
sharing between adjacent revisions; built from the tree-sitter extractor (`e3.1`). — `e3.2`, cost basis
ADR-004, spike `s3`.

**Cycle debt** — existing import cycles, baselined with owner and expiry, so only *new* cycles alarm; the
count of nodes inside cycles is a trendable health metric. — `e3.5`.

**Layering rule** — a declared, per-repo intended architecture in `fathom.toml`: named layers, allowed
and forbidden edges, no-cycle and no-upward-reference rules. Evaluated as a per-episode **delta** so the
reviewer sees what this turn did, not what the codebase already owed. — `e3.6`.

**Ratchet / baseline** — the committed file (`.fathom/ratchet.toml`) recording existing violations with
owner and expiry so CI fails only on new ones; it only tightens automatically, and loosening it is a
deliberate act with a written reason that lands in the file and in the artefact. — `e3.10`.

**Hotspot** — a module ranked by churn × complexity, fan-in/fan-out, instability and size, with every
metric's contribution shown. — `e3.8`.

**Scorecard** — the five-dimension architecture health summary (boundaries, coupling, acyclicity, API
hygiene, hotspot pressure) with a hand-reproducible formula and a per-window trend, including a "since my
last review" delta. — `e3.9`.

### 4.5 Risk and attention

**Attribution** — who wrote it: author kind, agent identity, confidence, evidence list. User-correctable,
and corrections are learned for matching future signatures. — `e4.1`.

**Intent** — the recorded *why* behind an episode: turn prompt, plan items, declared scope (as path
globs), session id, or a trailer fallback. Missing intent is a normal state that the UI states plainly,
not an error. — `e4.2`.

**Pathology** — a characteristic agent failure mode that detectors look for: churn loops, scope creep,
untested new logic, shotgun edits, rework loops, test weakening, copy-paste growth, debug leftovers,
hand-edited generated files, secret-shaped strings, disabled lint or type checks. — `e4.3`.

**Risk score** — the explainable 0–100 per-episode score: a documented feature vector (findings and
severities, size and blast radius, dependency and migration exposure, test ratio, attribution confidence,
noise share) times transparent, clamped, per-repo-tunable weights. No opaque model in v1: if we cannot
say why an episode is 82, it must not be 82. — `e4.4`, calibration `e4.4`.

**`fathom why`** — the command that prints an episode's risk feature vector, weights and each
contribution. The user-facing form of "explainable". — `e4.4`.

**Flag (high-risk flag)** — the boolean `risk score ≥ τ` (or a `high` severity finding). Flagging is a
routing decision, never a judgement; flags are what the false-positive guardrail (§5.4) measures.

**Severity** — a finding's local badness (`low`/`medium`/`high`) assigned by its detector. **Risk** is the
*episode-level* aggregate, weighted and contextual. The two are different objects and must not be
conflated in UI or code.

**Disposition** — the terminal outcome a reviewer assigns to an episode: `accepted`, `flagged`,
`dismissed` (with a reason code from a fixed, learnable vocabulary), `snoozed` (until a condition). Every
disposition records who, when, why. — `e4.5`, persisted by `e4.6`.

**Sign-off** — the disposition that says "I understand this and I accept it", with reviewer identity,
timestamp and scope, stored in the *review ledger*. Sign-off is the event the north-star metric clocks
to. A sign-off whose content later changed is *stale*. — `e4.6`.

**Review state** — the per-episode lifecycle Fathom tracks: `unread`, `seen`, `in review`, `accepted`,
`flagged`, `dismissed`, `snoozed`, `stale`. Derived from the ledger, never stored twice. — `e4.6`,
`e1.11`.

**Dismiss vocabulary** — the closed, learnable set of reasons a reviewer may dismiss with, e.g.
`expected`, `not-applicable`, `low-stakes`, `false-positive`, `handled-elsewhere`, `pre-existing`.
Dismissing systematically must be visible: dismissals roll up in a retrospective view, and the set
doubles as the false-positive labelling source (§5.4). — `e4.5`.

**Caught up** — the unambiguous state in which the queue is empty and no episode is `unread` or `stale`.
Ambient mode goes quiet and dim here on purpose; "all clear" must be a state, not a mood. — `e5.1`,
`e4.5`.

**Snooze** — deferring an episode until a stated condition (a later episode, a date, a re-index) rather
than dismissing it. Snoozed work is still open work and appears in the backlog count. — `e4.5`.

**Episode card (summary card)** — the one-screen answer for an episode: what changed semantically, where
it landed, what it now depends on, which non-code surfaces it touched, what is tested, and a
blast-radius estimate — every figure linking to its evidence, byte-deterministic for a given episode. —
`e2.10`.

**Narrative** — the deterministic, template-composed prose summary of an episode in which every claim
carries a resolvable evidence id. Optional LLM polish is opt-in, watermarked as generated,
citation-preserving, and degrades to the template when unavailable. — `e4.7`, spike `s5`.

**Operator-action checklist** — the per-episode list of things a human must now *do* outside the repo:
deploy a migration, set an env var, rotate a secret, coordinate a rollout. — `e2.8`, exported by `e5.7`.

### 4.6 Interfaces and surfaces

**Hooks v1** — the harness→Fathom event contract: append-only JSONL at `.fathom/events.jsonl`
(configurable) with event types `session_start`, `turn_start`, `turn_end`, `tool_call`, `file_edit`,
`test_run`, `approval`, `error`; monotonic sequence numbers; tolerant of concurrent appends and partial
lines; validated against a checked-in JSON Schema, with malformed events quarantined rather than fatal.
Degrades, never blocks the agent. — ADR-005 (`e0.5`), reader `e6.1`.

**Harness output is untrusted input** — hook events, commit messages, paths, lockfiles and
repo-provided config are hostile-by-default. A clone must never be able to make Fathom execute code or
write outside `.fathom/`. — ADR-005, `e7.8`.

**MCP surface** — the read-only (plus explicitly gated write) tools Fathom exposes back to the agent:
`explain_episode`, `changes_between`, `architecture_delta`, `dependency_delta`, `risk_report`,
`search_history`; gated: `mark_reviewed`, `add_review_note`. Never returns secrets or arbitrary file
bodies; returns references and summaries; bounded and paginated. Runs locally. — ADR-005, `e6.4`.

**Review artefact** — the exported, byte-stable record of a review range (markdown, HTML, JSON): summary
cards, dependency and architecture deltas, findings with evidence, operator actions, dismissals with
reasons, and the sign-off block with reviewer identity and time. Posted on a PR by the user; Fathom does
not post it (§8, non-goal 4). — `e5.7`.

**Watch mode / daemon** — live detection of new commits and refs, and the single per-repository process
that does watching, hook tailing and budgeted background indexing while TUIs and CLI commands attach over
a local socket. — `e1.9`, `e6.3`.

**Store / index / cache** — the local SQLite (WAL) database and analysis cache under `.fathom/`, keyed by
`(tree hash, analyzer version)` so changing an analyzer invalidates exactly the right rows.
Distinguishing **disposable** (anything rebuildable from git) from **must-survive** (reviews, decisions,
baseline) is an explicit ADR-004 requirement. — ADR-004 (`e0.4`), `e1.4`.

**Tree hash / analyzer version (cache key)** — the pair that keys every cached analysis result; bumping
an analyzer version invalidates its outputs and nothing else. — ADR-004, `e1.4`.

**Durability class** — the three-way label ADR-004 puts on every stored artifact: **M must-survive**
(a human act that cannot be recomputed: sign-offs, notes, decisions, dismissals, ratchet
allowances), **R rebuildable** (derivable from git plus harness events; losing it costs time, never
information), **E discardable** (presentation and coordination state; loss costs a keystroke). The
load-bearing corollary: *nothing inside `.fathom/` is M by virtue of being there* — M is M because it
is committed. — ADR-004 §10, `e4.6`.

**Index epoch** — the generation stamp on every derived row. A row is live iff its cache key matches
*and* its epoch is the current `index_epoch`; a rebuild fills a staging store at the new epoch and
swaps the epoch atomically, so no reader ever sees a half-rebuilt index. Invalidation is therefore a
lookup miss, never a delete. — ADR-004 §6.3, §9.3, `e1.4`.

**Analyzer manifest** — the registration every analyzer carries: id, version, result schema version,
cache scope kind, the `fathom.toml` keys it reads, its dependency DAG, and the source globs that
implement it. Touching an analyzer's sources without bumping its version fails CI. — ADR-004 §5.1,
§5.7, `e0.11`.

**Invalidation matrix** — the map from cause (new commit, dirty file, analyzer bump, config change,
schema migration, rewrite, epoch rebuild) to the cache-key field that moves and the rows that become
unreachable. It is generated from the analyzer manifest, and its cascades are measured, not asserted.
— ADR-004 §6, §13.2.

**Store lease** — the single-writer contract: the daemon holds `flock` on `.fathom/daemon.lock` with
a `writer_epoch` and a heartbeat; a CLI that cannot take the lease sends its mutation over the daemon
socket instead of writing. Who writes is a fact about a lock, not about which binary ran. — ADR-004
§7.1, `e6.3`.

**Timeline / episode rail** — the chronologically ordered list of episodes (the home *view*; distinct
from the *attention queue*, which is ordered by risk). Grouped by day or agent session, filterable, with
the unread marker. — `e1.6`, layout ADR-006.

**Delta overlay** — the per-episode rendering layer on top of the structure and module maps: added edges
solid, removed edges ghosted, changed nodes outlined, filtered to what this episode touched, with counts
when the delta exceeds what can be drawn. A presentation of the *architecture delta*, never a second
analysis. — `e3.4`, layout ADR-006.

**Command palette** — the searchable action list (`,`) in which every action is an addressable command
with a stable id and its current keybinding shown. Completeness is enforced by a test that enumerates
actions against palette entries. — `e5.4`.

**Keymap** — the global key binding table plus per-mode user overrides from `fathom.toml`, with
load-time conflict detection; the source of both the in-app help overlay and the generated cheatsheet, so
neither can lie. Everything is reachable from it; the mouse is additive. — ADR-006, `e5.4`, `e7.5`.

**Network consent** — the explicit, per-feature user decision that gates any outbound call (advisory
lookup, optional narrative polish). Default off; `fathom doctor --privacy` prints what is on, what is off
and which key turns each on. — `e6.8`, `e0.8`.

**Privacy posture** — the documented and mechanically verified claim about what leaves the machine:
no egress by default, no telemetry transmission ever, local-only counters the user can read, verified by a
CI job that runs the binary with no network. — `e6.8`, `e7.8`.

**`fathom.toml`** — the per-repo config file, layered as flags > env > repo file > user file > defaults,
with every effective value explainable via `fathom config explain`. Unknown keys warn with a nearest-name
suggestion. — `e0.8`.

**`fathom doctor`** — the self-diagnostic command whose output is safe to attach to a bug report (no
secrets, no absolute home paths) and states a remedy for every problem it finds. — `e1.10`.

**Perf budget** — the numeric performance contract per repo size class (cold index, warm re-index, diff
open, query p95, RSS ceiling) that ADR-004 sets and CI asserts, with a hard failure on a >15 %
regression. Budgets may be consciously rewritten with a written rationale, never quietly missed. —
ADR-004, `e0.11`, `e7.2`.

**Fixture corpus** — the deterministic synthetic repositories (fixed timestamps, committers,
byte-identical SHAs) with ground-truth expectations that analysis tickets assert against instead of
ad-hoc strings. — `e0.10`.

**Frame** — the whole terminal: chrome plus exactly one view. A frame has one scope and one focused
pane, and "what is on screen" is fully described by (view, scope, focus) — which is what `e1.11`
persists between sessions and what `e5.7` writes into an artefact's header. — ADR-006 (`e0.6`).

**View** — one of the nine frames a reviewer can put on the screen (Queue, Timeline, Diff, Structure,
Graph, Deps, Ambient, Artefact, Search), each answering exactly one question and each reachable by one
global key. A view is not a page and not a route: it is a pane layout plus a focus ring plus the drill
edges out of it. — ADR-006 (`e0.6`).

**Pane** — a bordered region inside a view that owns one kind of content and declares the fields of the
delta model it renders. Two panes and chrome per view at 80 columns; a third region is always a strip
inside a pane, never a third column. — ADR-006.

**Chrome** — the frame furniture around a view's panes: tab bar, pane footer strips, hint line, status
line, box frame. Chrome is what the degradation ladder sells first, in a fixed order, and the status
line is never for sale because it carries the notice that says the rest was sold. — ADR-006.

**Focus ring** — the ordered loop of panes `<Tab>`/`<C-w>l` walk inside a view, pane 1 first. Focus is
per pane, not per view: where it is decides which keymap mode is live. — ADR-006.

**Drill** — the movement from a pane to the more specific thing it is pointing at (`<Enter>`), and back
(`<Esc>`), with the selection preserved. A drill is an edge in the view graph, not a new view; the
drill stack is the sequence of panes you came through, popped one `<Esc>` at a time. — ADR-006.

**Overlay** — a view that borrows the frame instead of owning it, currently only Search (`/`): it covers
the frame, takes `prompt` mode, and `<Esc>` restores exactly what was there. Distinguished from a view
because an overlay may not steal global keys while it is open. — ADR-006.

**Mode (keymap)** — the named layer of the keymap that is live for the focused pane (`prompt`, `pick`,
`list`, `body`, per-view, `global`), with a fixed precedence and first-binder-wins resolution. Modes are
why `j` can mean five things without a single binding being ambiguous. — ADR-006.

**Tab (view tab)** — a pinned view in the tab bar, numbered `<C-w>1..9`, with `T` pinning and unpinning
the current one. The tab bar is a label, not the navigation: the view keys work with the bar degraded
away. — ADR-006.

**Scope** — the thing a view is currently about: an episode, a range of two episodes, a file, a cycle.
Every view reads its scope and every view key acts on it; `d s g p` change the question asked about the
scope, never the scope itself. One scope per frame is what makes the same keystroke mean the same thing
in eight views. — ADR-006.

**Degradation ladder** — the one ordered list of things a view gives up as the terminal shrinks: hint
line, tab bar, footer strips, frame, secondary-pane strips, hard widths, compact rows, trimmable body
lines — then the single-pane fallback, then an honest refusal. Applied in that order for every view and
announced in the status line whenever it fires, because a review tool that silently drops information
has committed the one failure it is not allowed. — ADR-006, §7.2.

**Canonical task** — one of the three key sequences ADR-006 publishes verbatim (triage an episode,
compare against the previous one, find a thing) so that "what keys do I press" has a single answer in
the docs, the help overlay and the cheatsheet. The sequences are resolved through the real keymap in a
test, so a rename cannot leave the docs describing keystrokes that do not exist. — ADR-006.

### 4.7 Metric vocabulary

**Time-to-confidence (TTC)** — the north-star metric: wall-clock from an episode's creation to a human
sign-off that was not followed by rework. Formal definition §5.1.

**Guardrail metric** — a metric reported beside the north star specifically to catch the ways the north
star can be improved by pretending: *in-session review share* (§5.2), *rework rate* (§5.3), *risk-flag
false-positive rate* (§5.4). A number that moves the north star but breaks a guardrail is a failure, not
a win.

**Rework** — the operational signal that a mental model was wrong: an accepted episode edited again,
partially reverted, or fully reverted within *N* later episodes on its *lineage*. Detectors R1–R3 in
§5.3.

**Maturity lag** — the waiting period before an episode may enter a rework-denominated ratio: it needs
*N* successor episodes on its lineage or 7 days elapsed, whichever comes first. Without the lag, recent
work silently counts as good and the metric flatters itself.

**In-session** — a sign-off that happened inside the same agent *session* as the episode (preferred
rule), or within 30 minutes of episode creation when no session id exists. Formal definition §5.2.

**Cohort** — a set of episodes over a window, reported with its size `n` and its censoring; medians are
never reported without `n`.

**Calibration set** — the labelled fixtures and real-repo annotations used to evaluate risk-score weight
changes rather than feel them, and the source of the false-positive rate floor. — `e4.4`, `e4.3`.

**Milestone gates** — `alpha` (`m0`, spine usable for daily review), `beta` (`m1`, change intelligence,
queue and export), `1.0` (`m2`, architecture awareness, CI gate, packaging). **v1** in this document means
the 1.0 gate; metric targets are stated against these gates so they can be checked at a known date rather
than "someday". — `m0`, `m1`, `m2`.

### 4.8 Terms deliberately not defined here

Words that come up in conversations about this product and are **not** project vocabulary for v1, kept
out on purpose so they cannot smuggle scope in: *workspace* (an editor/IDE concept; we say *repository*
or *repo*), *project* (we say *repository*), *agent team / swarm* (Fathom reviews output, it does not
orchestrate agents), *chat pane* (Fathom is not a chat client; the harness owns the conversation), *cloud
sync / account / telemetry pipeline* (§8). If a ticket needs one of these words, it must first amend this
section.

---

## 5. Metrics

### 5.0 What this section is for

One north star, three guardrails, all four computable **offline, on one repository, from the delta model
plus the review ledger** — because Fathom makes no network calls by default and runs no telemetry service
(§8, non-goals 5–6). That constraint is deliberate and it shapes the metrics: they are per-repo claims a
user can verify on their own machine with `fathom stats --json`, not fleet-wide product-analytics
numbers. A metric that could only be computed by uploading usage data is rejected on principle, not on
effort.

**Notation.** `W` = reporting window: the trailing 14 days by episode creation date, or the last 200
dispositioned episodes in repos with little history — the smaller of the two, so `n` is always stated.
`A(W)` = episodes in `W` carrying a human *sign-off* (`accepted`). `E(W)` = all *agent*-attributed
episodes in `W`; episodes with `author kind = unknown` are reported as a separate line and never silently
merged into either population. `now` = time of computation.

**Reporting discipline (all four metrics).** Report the median, not the mean (TTC has a long right tail
and a mean is a joke waiting to happen); report `n` and censoring with every number; report per repository
and never turn it into a cross-repo ranking; report matured cohorts and recent cohorts separately; and
never show the north star without its three guardrails in the same view.

### 5.1 North star — time-to-confidence

For an episode `e`:

```
created(e)   = e.episode.created_at
               derivation priority: harness turn_end timestamp
                                    > committer time of the episode's last member commit
                                    > index time of the provisional episode   (recorded, low confidence)
signed(e)    = ledger entry with decision == accepted and actor kind == human
               → review.signed_at
ttc(e)       = max(0, signed(e) − created(e))
rework(e)    = R1(e) ∨ R2(e) ∨ R3(e)                     (see §5.3)
counted(e)   = signed(e) exists ∧ ¬rework(e)
```

> **TTC(W) = median { ttc(e) : e ∈ A(W) ∧ counted(e) }**

Reported as **median (headline), p90, `n`**, plus the two denominators that give the median meaning —
*sign-off coverage* `|A(W)| / |E(W)|` and *backlog* (episodes older than 24 h with no disposition,
including `snoozed`). A median over signed-off episodes alone can be improved by signing off only the
easy ones; coverage and backlog are what stop that.

**Cohort maturity.** An episode enters the TTC cohort only once `mature(e)` holds (§5.3), so the median is
computed on settled outcomes rather than on work that has not yet had a chance to come back. Coverage and
backlog count every episode immediately, including immature ones, so the two views together cannot hide
recent trouble.

**What moves it.** Each lever has a cheap local proxy already in the plan, so a ticket can claim progress
on something measurable:

| Lever | Proxy already in the plan |
|---|---|
| Orient faster (where am I, what is new) | time-to-first-orientation < 60 s (`e1.12`) |
| Understand an episode faster | episode card renders < 50 ms warm (`e2.10`); diff open latency (ADR-004) |
| Triage faster | 100-episode queue cleared < 5 min (`e4.5`) |
| Trust the tool enough to sign | evidence-backed findings, zero silent suppression (`e2.2`, §7.1) |
| Re-review nothing after a rewrite | ≥ 95 % of sign-offs re-linked after rebase/amend/squash (`e4.6`) |
| The physical layer: nothing feels slow | warm re-index < 150 ms, query p95, RSS ceiling (ADR-004, `e0.11`) |

### 5.2 Guardrail 1 — share of episodes reviewed in-session

Reviews done *while the agent was still working* are the cheap ones: context is warm, the agent can still
act on the feedback, and the reviewer is not reconstructing history. This is the guardrail against a
north star improved by batch-signing a weekend backlog.

```
in_session(e) =
    TRUE    if both sides carry a harness session id and sign-off session id == e.turn.session_id
    TRUE    else if signed(e) − created(e) ≤ 30 min                     (fallback, no hook events)
    FALSE   otherwise
    UNKNOWN if neither is computable — counted in the denominator, never in the numerator

G1(W) = |{ e ∈ A(W) : in_session(e) = TRUE }| / |A(W)|
```

To keep this from being gameable by reflexive one-keypress acceptance from ambient mode, `G1` counts an
episode only when the ledger records that the reviewer **engaged**: opened the episode card, or opened at
least one evidence link, before signing (`review.engaged = TRUE`, derived from local UI interaction
counters, never transmitted — `e6.8`). Provisional targets: `G1 ≥ 0.50` at alpha (`m0`), `≥ 0.70` at 1.0
(`m2`), recalibrated at `e0.12`.

### 5.3 Guardrail 2 — rework rate

Rework is the financial instrument of this thesis: it is the observable trace of a wrong mental model,
discovered after the money was already spent. It is also the penalty term inside the north star
(`counted(e)` requires `¬rework(e)`), so it has to be defined concretely enough to implement.

Let `L_N(e)` = the first `N` (default **N = 5**) later episodes on `e`'s *lineage* — those with
`created(e′) > created(e)`, in creation order — and let `added(e)` be the lines and symbols `e`
introduced.

```
R1 (revert)         ∃ e′ ∈ L_N(e) containing a commit whose diff reverts a member commit of e,
                    or whose message/trailer declares a revert of e's content.
R2 (undo-of-work)   ∃ e′ ∈ L_N(e) that removes or re-modifies ≥ 20 % of added(e),
                    or re-modifies a symbol e touched back toward its pre-e state.
R3 (pathology)      ∃ e′ ∈ L_N(e) whose findings include a rework-loop, churn-loop or test-weakening
                    detector hit whose evidence cites e's episode identity.

rework(e)  = R1(e) ∨ R2(e) ∨ R3(e)
mature(e)  = |{ lineage successors of e }| ≥ N  ∨  (now − created(e)) ≥ 7 days
Mature(W)  = { e ∈ A(W) : mature(e) }

G2(W) = |{ e ∈ Mature(W) : rework(e) }| / |Mature(W)|
```

Report `G2` split by detector (`R1`/`R2`/`R3`) — a revert spike and a hunk-repeat pattern mean very
different things — and always with the matured denominator. Provisional target: `G2 ≤ 0.15` at alpha,
`≤ 0.08` at 1.0. R2's 20 % threshold and `N` are the two constants most worth re-fitting at `e0.12`
against the `s2` ground truth, because re-editing code is normal and only *rework of reviewed content*
is a signal.

### 5.4 Guardrail 3 — risk-flag false-positive rate

The attention queue only earns trust if "high risk" means something. This guardrail measures the cost of
crying wolf, because a reviewer who stops believing the flags stops using the product.

```
τ             = high-risk threshold, default risk score ≥ 40 (per-repo tunable)
F(τ,W)        = { e ∈ E(W) : risk(e) ≥ τ ∨ ∃ finding with severity = high }
labelled(e)   = e has a disposition (the reviewer looked)
FP(e)         = disposition.reason_code ∈ { expected, not-applicable, low-stakes, false-positive }
TP(e)         = disposition == accepted-with-required-change ∨ rework(e)

G3(τ,W) = |{ e ∈ F(τ,W) : labelled(e) ∧ FP(e) }| / |{ e ∈ F(τ,W) : labelled(e) }|
```

τ was 70 — `e2.10`'s key-spec default, carried in until something measured it — and this section always said
the number had to be "confirmed against three real OSS repositories". `reco-6co` did the confirming, and 70
came out inert: across 540 real episodes the shipped score topped out at 63, six episodes reached the
threshold and all six got in through a high-severity finding, so the `risk(e) ≥ τ` half of `F(τ,W)` fired on
nothing and every one of the 24 mature episodes that later needed rework sat below it. The confirmed default
is the corpus's own **top-decile floor, 40** — the highest of the three repositories' top-decile cuts, which
is also the lowest threshold whose whole flag population (49 episodes, 9.1 % of the corpus) the
hand-authored dispositions can answer for. It is one number, held in `risk.tau`, in
`crates/fathom-analyze/calibration/g3/thresholds.json` and in this section, and the harness below refuses to
run when the copies disagree.

`G3` resolves immediately from the *dismiss vocabulary*, which is why that vocabulary must stay closed and
small: it doubles as the labelling mechanism. The slow confirmation is `rework(e)` — a flagged episode
that later needs rework was correctly flagged — so `G3` is reported with a fast label and a matured
follow-up, never as a single number.

**Report it with its complement, always.** `G3` alone is trivially satisfied by flagging nothing. The
required companion is the **miss rate**:

```
miss_rate(W) = |{ e ∈ Mature(W) : rework(e) ∧ risk(e) < τ }| / |{ e ∈ Mature(W) : rework(e) }|
```

Provisional gates: `G3 ≤ 0.30` at alpha, `≤ 0.15` at 1.0; `miss_rate ≤ 0.20`. Thresholds are confirmed
against the `e4.4` calibration fixture and three real OSS repositories, and the number earlier tickets
defer to is written here: `e4.3` and `m1` both say "under the threshold agreed in `e0.12`" — `e0.12`
should set them from this section rather than invent new ones.

**These are the numbers, and this is the thing that measures them** (`e4.9`, `reco-mkl.1`; re-measured by `reco-6co`, which is also when τ moved):

```text
python3 tools/fetch_g3_corpus.py
cargo run -p fathom-analyze --features sqlite-bundled --example g3_calibration -- --check
```

Three OSS repositories pinned by commit, dispositions labelled by hand from the closed vocabulary `e4.6`
ships, per-detector attribution, and a blessed report that `--check` re-measures byte for byte:
[`benchmarks/reports/e4-3a-g3-calibration.json`](../../benchmarks/reports/e4-3a-g3-calibration.json),
with its gates in `crates/fathom-analyze/calibration/g3/thresholds.json` and the written answer in
[Guardrail 3, measured](false-positive-calibration.md). The measurement on those repositories — 540
episodes, 64 flags, every one of them labelled against a diff — does **not** clear these bars: `G3` 0.672
across everything the corpus flagged, `G3` 0.633 at the published τ 40 on the 49 episodes it flags, and a
miss rate of 0.792 (19 of the 24 mature reworked episodes still scored under τ). Before `reco-6co` the same
corpus read `G3` 0.697 and a miss rate of 1.000 at τ 70 — on real history the flag arm did not fire at all,
which is the failure this section's own confirmation clause existed to catch, and the window tripling in
`reco-dyt` measured it three times better rather than fixing it. The bars stay where they are, because the
alternative to a measured failure is an unmeasured pass; what moved is what gets flagged, and the answer
page states what that bought and what is still outstanding. The `not-applicable` reason code in `FP(e)`
above has no separate shipped code; the mapping `e4.9` uses is written out in `thresholds.json` rather than
left implied.

### 5.5 Contract on the delta model

The metrics above are only real if the fields exist. This is the measurement contract: **`e2.11`'s
`EpisodeDelta` v1 must expose these fields, or the metric is unmeasurable, which is a blocking defect
against `e2.11` rather than a note in a comment.** Renaming or dropping one requires amending this section
in the same change.

| Field | Consumed by | Source ticket |
|---|---|---|
| `episode.identity.id`, `episode.identity.fingerprint` | join key for reviews and all metrics | `e1.3`, ADR-003 |
| `episode.created_at`, `episode.created_confidence` | TTC, G1, G2 | `e1.3`, `e6.1` |
| `episode.members[].sha`, `episode.lineage.ref` | lineage, G2 | `e1.2`, `e1.3` |
| `episode.turn.session_id`, `episode.turn.turn_index` | `in_session` preferred rule, intent | `e4.2`, `e6.1` |
| `episode.attribution.kind`, `.confidence` | population split `E(W)` vs human | `e4.1` |
| `episode.noise.share` | risk vector | `e2.2` |
| `delta.structure.*`, `delta.dependencies.*`, `delta.architecture.*`, `delta.non_code.*`, `delta.tests.*` | risk vector, episode card, `fathom check` | `e2.3`, `e2.6`, `e3.3`, `e2.8`, `e2.9` |
| `findings[].{detector_id, severity, confidence, evidence_ids[]}` | G3, risk vector, episode card | `e4.3` |
| `risk.{score, reasons[], weights_version}` | flags, G3, `fathom why` | `e4.4` |
| `blast_radius.{direct_count, hop1_count}` | risk vector, queue ordering | `e2.10` |
| `evidence[].{id, kind, locator}` | "every number is a button" | `e2.10`, `e2.11` |
| `review.{state, decision, reason_code, actor, signed_at, engaged}` — joined from the ledger | TTC, G1, G3 labels, backlog | `e4.6`, `e4.5` |
| `rework.{flag, detectors[], checked_successors}` — computed with the maturity lag | TTC `counted`, G2, miss rate | `e4.3`, `e4.6` |

The **single** computation site for `review.*` and `rework.*` is the store query layer, so the TUI,
`fathom stats --json`, the export artefact and the MCP `risk_report` cannot disagree. Where the
measurement needs a home command: propose `fathom stats --json` (`e1.10`) grows a `metrics` block
implementing §5 verbatim, fed by the local-only counters of `e6.8`. That is the one scope item this ADR
adds, and it is a follow-up ticket, not a hidden requirement.

---

## 6. What would falsify this

Stated in advance, so that we notice rather than rationalise. Any of these is evidence against §1:

1. **TTC improves, accepted throughput does not.** Reviewers get fast and confident and still choose not
   to accept more agent output — the bottleneck was elsewhere (trust in the agent, or in the
   requirements), not in review.
2. **TTC improves and rework rate rises.** Confidence got *cheaper* rather than *better*: we optimised a
   guardrail away, which is the signature of a tool that manufactures false confidence.
3. **Sign-offs are rubber-stamped.** `G1` high but `review.engaged` thin, or `G2` for signed-off episodes
   no different from unsigned ones. The ledger then records approval, not understanding.
4. **Nobody keeps the second terminal open.** Ambient-mode adoption collapses within a week — the
   glanceable premise is wrong and this is a batch tool, which changes the whole design.
5. **Segmentation is not good enough** (`s2`). If true episode boundaries cannot be recovered at usable
   precision, "one turn = one reviewable unit" is unimplementable and the unit of review has to be
   rethought — the largest single risk in the plan, which is why `s2` gates ADR-003 and the MVP.
6. **Graph-per-revision is unaffordable** (`s3`). If architecture deltas cannot be computed incrementally
   within budget, the answer to "did this make the architecture worse?" stays slow, and the differentiator
   becomes a report rather than a glance.

---

## 7. Principles the thesis commits us to

These are consequences, not sentiments: any of them can be cited to settle an argument on a later ticket.
Amending one requires amending this section.

1. **Evidence before prose.** No number, label or sentence without a resolvable evidence reference. If the
   evidence cannot be shown, the claim does not appear.
2. **Never hide silently.** Suppression, filtering, baselining and collapsing are *views* with a printed
   reason and a one-keypress undo. Silently hidden content is the one failure mode a review tool is not
   allowed.
3. **The reviewer's attention is the budget.** Defaults follow rank-first: the queue, not the diff; the
   summary, then the drill-down. Every new feature spends attention and must say what it gives back.
4. **Deterministic by default.** Identical input yields identical bytes, in analyses, scores, narratives
   and exports. Byte-stability is what makes golden tests, reproducible sign-offs and trustworthy artefacts
   possible at all.
5. **Local by default.** Reads the repository; writes only under `.fathom/`; makes no network call unless
   the user turned a named feature on. Opt-in features degrade offline.
6. **Degrade, never block.** Fathom sits beside work in progress. A missing analyzer, malformed lockfile,
   unreadable object or absent hook event means a stated, partial answer — never a stalled index and never
   an obstacle in front of the agent or a commit.
7. **Confidence is part of the answer.** `unknown` is a first-class value. A guessed rename, a
   low-confidence attribution or a heuristic boundary is labelled as a guess in the UI, in the JSON and in
   the export.
8. **Stability across rewrites is a feature, not an implementation detail.** If a history rewrite orphans
   review state, that is a defect with a severity, not a limitation to document.
9. **Explainable before clever.** Transparent, tunable, hand-reproducible scoring and narratives beat a
   smarter opaque model in v1 — including, and especially, where an LLM is tempting (see `s5`).
10. **One reality.** Analysis lives in the delta model, once. TUI, CLI, CI, export and MCP all read the
    same versioned structure; a surface computing its own truth is a bug.

---

## 8. Non-goals for v1

Scope for **v1** = the 1.0 gate (`m2`). Each non-goal states the boundary, the reason, where the need goes
instead, and what would change our mind. Non-goals are not "later, maybe"; they are the reason the rest of
this is buildable.

| # | v1 is not… | Boundary in practice | Why | Revisit trigger |
|---|---|---|---|---|
| 1 | **a git client** | Fathom reads git and writes only under `.fathom/`. No commit, checkout, branch, merge, rebase, stash, push, force-push, conflict resolution or ref rewriting. Handing work *to* git is a documented handoff (`fathom open`, difftool). | The reviewer already has git and the harness has a git path. Ref mutation would put Fathom in the business of destroying history it cannot model, and every review tool that grew a "quick fix" button died there. | A read-only gap that makes an episode unreviewable *and* cannot be filled by a handoff. |
| 2 | **an IDE** | No editing, no buffer, no formatter, no lint that rewrites files, no build, no test runner. Jump-to-`$EDITOR` at file:line is the boundary (`e5.8`). | Editing is a different product with different muscle memory; Fathom's value is comprehension at a distance from the code. Writing into the tree also breaks §7.5. | Review genuinely requires a mutation to be verifiable — answered by an external handoff, not an embedded editor. |
| 3 | **an agent runner** | Fathom never starts, steers, retries, approves or kills an agent; it has no prompt box and no "fix this" button. It reads the harness one way (hooks v1) and answers read-only questions the other way (MCP). | The reviewer's trust in the review must not depend on the reviewer also being the driver. The observer has to stay independent of the observed, or the review is theatre. | A *review* action genuinely needs a re-run (e.g. "re-index after the test you asked for"). Then it becomes a gated MCP write tool with an audit line — never a button that launches a model. |
| 4 | **a PR host** | No PR or review-comment hosting, no hosted discussion, no merge, no ownership of someone else's pipeline. Fathom *emits* an artefact (`e5.7`) and `fathom check` runs inside your pipeline (`e6.6`); posting is your `gh`/API call. | The organisation's review record already lives in the PR host; the honest product is the thing that produces a *better record* for that host. Duplicating the host is years of work and a fight with the incumbent's API quota. | A team standard where the review record must be created in Fathom and only Fathom. (Not on the horizon.) |
| 5 | **a cloud service** | No accounts, no hosted backend, no sync, no telemetry upload, no server-side indexing. Everything works on a laptop on a plane. Advisory lookups (`e2.7`) and optional narrative polish (`e4.7`) are separately opted-in, feature-gated and degrade offline; local counters stay local and readable (`e6.8`). | The thesis is about *personal* model-building, and the data is the whole history of a company's source — exactly the data nobody's security review approves for a third-party index. This is also what forces §5's metrics to be honest per-repo numbers. | Cross-repo fleet analytics or team-wide review memory — needs an explicit self-hosted design and an amendment here first. |
| 6 | **LLM-dependent** | The default build makes no mandatory LLM call and no network call. Classification, deltas, risk scoring, narrative and export are deterministic templates over the delta model. **`s5` decided (`reco-nv0`): `e4.7` ships the deterministic template and no LLM path** — polish measured usefulness 2.23 → 4.40 and trustworthiness 4.667 → 4.000 against the same evidence set, so the seam gets no consumer, no `privacy.llm` surface and no code path. | A review artefact you cannot reproduce is not evidence. Determinism is also what makes byte-stable exports, golden tests and cheap CI possible, and keeps the whole product usable offline (§5.5). | ~~Only if `s5` shows a measured, non-trivial win on trust and usefulness with hallucination counts at zero~~ — `s5` ran and did not clear it; reopening needs all five gates in `reco-nv0`, measured against a real provider and a human panel. [The measurement](../concepts/narrative-polish.md). |

**In scope though it resembles a non-goal** (to head off predictable confusion): the CI gate `fathom check`
(`e6.6`) is not the cloud service of non-goals 4/5 — it is a binary that runs where you run it and needs
neither network nor a full history fetch; the MCP server (`e6.4`) is a local process, not a hosted API;
SARIF and step annotations are *output formats*, and uploading them is your action.

**Honest gaps these non-goals leave.** v1 will not help a reviewer who wants to fix a small thing while
reading (they leave the tool: non-goal 2), who wants to tell the agent to fix it from the same window
(non-goal 3), or who works in a team of five reviewing the same episodes concurrently and wants shared
live state (`e6.5` gives durable shared memory, not concurrent live collaboration). Those are accepted
costs, chosen so that the core loop — understand, trust, sign off — is genuinely finished rather than five
surfaces half-finished.

---

## 9. Open questions

Tracked, not hidden. Each is owned by the named ticket; none blocks ADR-001 acceptance.

| # | Question | Owner |
|---|---|---|
| 1 | Metric thresholds above 0/∞: `Δ_session`, `N`, R2's line threshold, `τ`, and the `G1`/`G2`/`G3`/miss-rate gates to be confirmed from spike data and three real repos. | `e0.12` |
| 2 | Does hooks v1 carry enough session semantics for `in_session` to use the preferred rule rather than the 30-minute fallback in every harness we support? | `e0.5`, `e6.1` |
| 3 | Does the repo-level `metrics` block live on `fathom stats --json` (`e1.10`) or does §5 need its own ticket? | `e1.10` |
| 4 | `author kind = unknown` episodes: exclude from `E(W)` (cleaner population) or include with a flag (more honest coverage)? Resolve with `e4.1` accuracy data. | `e4.1` |
| 5 | Confirm *Fathom* as the product name and *episode* as the term that survives contact with users (three outside users must be able to name what an episode is — `e7.5`). | `e7.5` |

---

## 10. Review and sign-off

| Item | Reviewer | Status | Date |
|---|---|---|---|
| Thesis (§1), falsifiability list (§6), principles (§7) | danielstahl (product owner) | **accepted** | 2026-10-14 |
| Persona (§2), core nouns (§4.1), metric contract (§5) | danielstahl (product owner) | **accepted** | 2026-10-14 |
| Non-goal boundary vs the coding harness (§8) | danielstahl (coding-harness owner) | **proposed — explicit ACK requested on `reco-982`** | — |

The non-goal boundary is derived from the harness owner's own framing of this project: *Fathom lives
alongside a coding harness, and this project plus the harness is together sufficient for a human to
deliver agentic software with confidence.* That sentence only holds if the two halves stay separate and
neither absorbs the other, which is what non-goals 1–6 encode. Because that boundary constrains the harness
as much as it constrains Fathom — non-goals 3 and 4 say what the harness must keep owning — the owner's
explicit acknowledgement is recorded here rather than assumed; `e0.5` and `e0.3` both depend on it.

---

## 11. Product amendments

An amendment changes a position recorded here without reversing a decision an ADR already made. It is
appended, never edited in.

### Amendment 1 — 2026-02-10 — the harness is a product dependency, and the gap inventory is the deliverable

**Amends**: §5.1 (evidence for *tied_to_code* / *landed*), §7 principle 5, §8 non-goals 3 and 4, §9 open
question 2.
**Owned by**: `reco-3pu` (`e0.5`), with the contract in [ADR-006](../adr/0005-harness-contract.md).

1. **Non-goals 3 and 4 do not mean Fathom may ignore the harness.** They say Fathom must not *become* a
   harness. Reaching the product's state still *requires* a harness ingest layer, and §5's metrics are
   undefined without it: §5.1 counts episodes tied to a change as *tied_to_code* / *landed* evidence
   and nothing produces that mapping today; there is no ingest, no event schema, no MCP surface
   (`reco-p3i`), no privacy/write-side exposure policy (`reco-h7p`, `e6.8`), no shared review memory or ledger (`reco-552`, `e6.5`; `reco-rlf`, `e4.6`), no deterministic explanation contract (`reco-2rx`, `e4.7`), no risk model beyond heuristics (`reco-884`, `e4.4`), and no
   `evidence_grade` or `unknowns` field anywhere in the schema. **This amendment records that gap
   rather than closing `e6.1`–`e6.4`** — §5's metrics stay honest-by-narrowness until `e0.5` lands — because a
   glossary that defines the evidence grades is not evidence.
2. **§5.1 *tied_to_code* / *landed* are explicitly unavailable in v1.** Recorded rather than quietly
   computed from a proxy. `evidence_grade ∈ {observed, inferred, asserted, absent}` and
   `unknowns: [str]` are *specified* by this amendment as required fields on every explanation and
   every metric response; **the gap does not block `e0.5`**, which is the ticket whose whole job is
   closing `e6.1`–`e6.4`. When it does, the *landed* population grows and T2C moves: that movement is exactly the
   effect size this metric is meant to reveal, not noise to be smoothed away.
3. **The trust model extends to the harness (§5 principle 5).** Principle 5 was written about the change
   fingerprint, and the same failure mode is available one layer out: *Fathom's model of the codebase is
   built from what the harness emits, so a harness that emits a flattering story makes Fathom's model
   flattering.* Therefore **harness output is untrusted input**: every event carries a provenance stamp
   (`harness`, `schema_version`, `ts`); `file_edit` evidence is verified against git when git is
   available and downgrades to *asserted* when it is not; and any `file_edit` whose stated path is
   outside the session's working directory is quarantined and counted (§5 principle 5 applies to the
   ingest path exactly as it applies to the fingerprint).
4. **§9 open question 2 is answered: no.** Hooks v1 as specified does **not** carry enough session
   semantics for `in_session` to use the preferred rule everywhere. The fallback stays, and §5.2's
   `in_session` definition is amended: `session_id` is **required** on `file_edit` and `approval`
   events, and where it is missing the fallback is used **and counted** (§5.2's denominator reports the
   fallback share alongside the rate). A metric whose fallback path is invisible is a metric that will
   quietly change meaning when the fallback stops firing.

### Amendment 2 — 2026-10-16 — an episode is addressed by what a rewrite preserves, and its commits are its extent

**Amends**: §4.3 (*Episode identity*, *Content fingerprint*, *Provisional episode*), and adds the four
nouns *episode extent*, *digest tier*, *episode twin*, *fail-visible*.
**Owned by**: `reco-ubw` (`e0.3`), with the contract in [ADR-003](../adr/0003-episodes.md).

1. ***Episode identity* is given the scheme §4.3 left open.** The address is
   `sha256("fathom-episode/1" ‖ author ‖ normalised subject ‖ sorted path set of the first member)`, stored
   as `ep-` plus the first twenty hex characters, and the *content fingerprint* is the same digest without
   the framing — with two weaker digests over the same three facts (author+subject, author+paths) so a
   rewrite that changes one of them can still be recognised. The words in §4.3 did not change their meaning:
   a rewrite preserves author, subject and paths, and rewriting them was always what "the work itself
   reappears under a new address" meant. What changed is that the address is no longer left to be invented
   per surface.
2. **The ordered member commit SHAs and the parent boundary — the scheme the ticket proposed — are the
   episode's *extent*, not its identity.** They are what the diff endpoint and the rework detector need, and
   they are precisely what `git rebase -i` and `--amend` rewrite, so keying a sign-off to them loses it on
   the operations users perform most (§7.8 with a severity attached). Any place that did key to them is a
   defect, not a competing definition; one such place existed in the reference store and is corrected in the
   same change.
3. ***Provisional episode* becomes operational.** "Nothing may be permanently keyed to a provisional
   identity" now means: no review row, no ledger entry, no finding of record, no risk score of record, no
   metric numerator or denominator — and **sign-off is refused**, because a sign-off must be about content
   that can be reproduced. The boundary cascade, its confidence *prices* (an ordinal, never a probability)
   and the re-link tiers are in ADR-003 §4 and §6, and are the only ones v1 has.
