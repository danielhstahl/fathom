# Recipe: let an agent ask

- **You get**: a coding agent reading the same reviewed facts a human is looking at, through a door that has a
  schema on it — and the rules for not mistaking that for trust.
- **Time**: ten minutes.
- **Needs**: an agent that can run a shell command, and this build's `fathom mcp serve`.
- **Plan keys**: `e2.11` (the delta model), `e6.4` (the MCP server).

The server is in this binary. `fathom mcp serve` speaks MCP over stdio — no socket, so no consent key — and
serves the same versioned documents the rail and the CLI print, addressed by a tool call instead of a shell
command ([CLI reference](../reference/cli.md#fathom-mcp)). The reason that matters is ADR-002's "one insight
API" rule: the TUI, the CLI, the export and an agent must not each get a slightly different answer, so there
is one delta model behind all four doors and the MCP layer is allowed to *point at* it, never to re-derive it.

Start it, and ask it what it offers:

```bash
fathom mcp serve            # stdio; a client speaks, this build never opens a port
fathom mcp tools --json     # the same surface without a session: tools, bounds, what config did
```

The client config an agent understands is the ordinary one — a command, no URL:

```json
{ "mcpServers": { "fathom": { "command": "fathom", "args": ["mcp", "serve"] } } }
```

Six read tools are offered: `explain_episode`, `changes_between`, `architecture_delta`, `dependency_delta`,
`risk_report`, `search_history`. The two write tools are off until an operator says otherwise, and stay out
of `tools/list` while off, so an agent never reaches for a door that will not open
([ADR-005 §4.3](../adr/0005-harness-contract.md)).

And the shell door is still there, unchanged:

```bash
fathom analyze <episode> --json
```

## 1 · Give the agent the document, not a prose summary

```bash
fathom analyze <episode> --json > /tmp/delta.json
fathom analyze --schema > /tmp/delta-schema.json     # the contract, from the types that produce it
```

Two properties make this safe to hand to something that will act on it:

* **it self-checks.** `fathom analyze` re-resolves every evidence pointer in the document it just composed and
  prints the verdict, so a payload that disagrees with its own evidence says so in the payload;
* **its gaps name their owner.** A section that could not be measured is `unavailable` with a `short` form and
  the plan key that owns the answer. An agent reading `0` will build on the zero; an agent reading
  `no supplied reach (e3.2)` will ask, or will say it does not know.

Tell the agent the second property explicitly. "A section may be `unavailable`; if it is, say so and do not
infer the value" is one sentence and it removes the most likely way this goes wrong.

## 2 · The other three documents an agent may want

| Question | Command | Contract |
|---|---|---|
| what changed, with evidence | `fathom analyze <ep> --json` | `schemas/episode-delta.schema.json` |
| what a human already decided about it | `fathom review list --json` | `fathom.cli.review/1` |
| is this plan item contradicted by what shipped | `fathom drift --json` | `fathom-drift/1` |
| why it ranked where it did | `fathom why <ep> --json` | `fathom.cli.why/1` |
| the prose a human skims | `fathom narrative <ep> --json` | `fathom-narrative/1` |

Every verb prints **one JSON document on stdout and nothing else** — which is why `--json` refuses
`--progress`, and why `2>/dev/null` is never needed.

## 3 · A worked example, in a harness

The pattern that works with a command-running agent — a small, read-only instruction, and the document
fetched with an explicit revision so the agent cannot ask for "everything":

```bash
# In the agent's instructions, or as a tool it may call:
fathom index --no-progress            # keep the index honest
fathom analyze --json HEAD            # the change under discussion
fathom review status                  # has a human signed this range off yet?
```

What to forbid in the same instruction, and why:

* **writing verbs.** `fathom review sign` is a human act with an identity attached; an agent signing off its
  own work with `--agent` recorded is a different thing, and the tool renders it differently for a reason
  ([the ledger](../concepts/review-ledger.md#who-signed));
* **`--all` sweeps** (`fathom drift --all`) as a context source. They are the input to a report, not an
  answer to a question, and an agent handed 500 findings will summarise them into mush;
* **treating the delta as ground truth about intent.** It reports what the record says about *why*, including
  `not recorded`, and the record is written by a harness whose output is untrusted input like everything else
  the tool reads ([the thesis](../concepts/thesis.md) is blunt about this).

## 4 · What the session adds, and what it refuses

The MCP tools return the same documents as the shell verbs, plus four things a shell command cannot promise:

* **bounds.** A page is at most `mcp.max_response_bytes`; a longer one is cut and handed back with
  `next_cursor`, so an agent that asked for everything gets a fact about its asking rather than a truncated
  paste it mistakes for the whole.
* **cost on the answer.** `cost_ms` and `bytes_returned` are in every response, including refusals, because a
  tool call runs inside the agent's turn budget and an unbounded result is a latency outage in costume.
* **one egress rule.** No file body and no secret leaves a response. The redaction table that runs at ingest
  runs again on the way out, and a body-shaped field (`body`, `snippet`, `patch`, `text`) is refused in place
  and said in `unknowns`. An agent that wants a file reads the file; the model is a map, not the territory.
* **a gate on the write half.** `mcp.allow_writes = true` turns `mark_reviewed` and `add_review_note` on and
  starts an audit line per call — tool, argument digest, outcome. `mark_reviewed` additionally needs a token
  from `fathom mcp approval-token`, and that token is bound to one change, so an agent cannot walk a queue
  signing things off with a single credential it obtained once.

Nothing in the shell recipe above becomes wrong: same documents, addressed differently — which is the
practical meaning of "one versioned delta model" and the reason it was made a gate rather than a convention.

## 5 · Privacy, while we are here

Asking an agent to read a local JSON file, or running a server over a pipe it does not own, is not a network
operation, and this build cannot make one anyway: the default binary links no network-capable crate, and the
three `net-*` features are each gated on both a compile flag and a consent key
([features](../reference/features.md)). The MCP server is stdio for exactly that reason; asking it for the
HTTP transport refuses until `privacy.mcp_http_transport` is set *and* the feature is compiled in.
`fathom doctor --privacy` prints the matrix and then says it in one line, which is the line to paste into a
privacy review:

```text
outbound features compiled in: none — no HTTP client and no listener in this binary (ADR-002 R6)
```

**Next**: [connect a harness](connect-a-harness.md) so the documents contain intent rather than its absence ·
[the delta model](../concepts/delta-model.md) for the version policy an agent's parser has to respect.
