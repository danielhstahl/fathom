# Recipe: let an agent ask

- **You get**: a coding agent reading the same reviewed facts a human is looking at, through a door that has a
  schema on it — and the rules for not mistaking that for trust.
- **Time**: ten minutes.
- **Needs**: an agent that can run a shell command, and this build's `--json` outputs.
- **Plan keys**: `e2.11` (the delta model), `e6.4` (the MCP server, **not in this build**).

There is no MCP server in this binary yet. `fathom mcp serve` parses, is feature-gated on `mcp`, and answers
with a refusal naming `e6.4` — which is the designed behaviour, not a broken install
([CLI reference](../reference/cli.md#fathom-mcp)). The reason it is worth writing this recipe anyway is that
the *document* the MCP surface will serve already exists, is versioned, and is readable from a shell today:

```bash
fathom analyze <episode> --json
```

That one command is the whole agent-facing contract. MCP will be a transport and a tool list in front of it,
not a second model of the episode — ADR-002's "one insight API" rule exists precisely so that the TUI, the CLI,
the export and an agent cannot each get a slightly different answer.

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
  `no module graph in this build (e3.2)` will ask, or will say it does not know.

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

## 4 · When MCP lands, and what changes

`e6.4`'s server is specified as read-only tools over stdio, with the stdio transport deliberately chosen because
it will not need the network consent that HTTP would
([ADR-005](../adr/0005-harness-contract.md), [features](../reference/features.md)). The migration from this
recipe to that one is mechanical: the same `EpisodeDelta` documents, addressed by a tool call instead of a
shell command. Nothing in what you write today becomes wrong — which is the practical meaning of "one
versioned delta model" and the reason it was made a gate rather than a convention.

## 5 · Privacy, while we are here

Asking an agent to read a local JSON file is not a network operation, and this build cannot make one anyway:
the default binary links no network-capable crate, and the three `net-*` features are each gated on both a
compile flag and a consent key ([features](../reference/features.md)). `fathom doctor --privacy` prints the
matrix and then says it in one line, which is the line to paste into a privacy review:

```text
outbound features compiled in: none — no HTTP client and no listener in this binary (ADR-002 R6)
```

**Next**: [connect a harness](connect-a-harness.md) so the documents contain intent rather than its absence ·
[the delta model](../concepts/delta-model.md) for the version policy an agent's parser has to respect.
