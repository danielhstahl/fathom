# Recipes

- **Status**: task-shaped instructions that work on the shipped build (`e7.5`, **reco-60m**).
- **House rule for this directory**: a recipe names only commands this binary has. If the right verb does not
  exist yet, the recipe says so and gives the version that does, with the plan item that owns the better one.
  An untypeable instruction is worse than none, because it gets typed.

A concept page explains a noun. A recipe is the shortest path through several of them to something you
needed to happen. Every recipe below was run against the miniature that ships in the binary
(`fathom --demo`) or against this repository's own history, and the output blocks are what came back.

| Recipe | You get | Roughly |
|---|---|---|
| [Review a session](review-a-session.md) | 43 commits from an agent turned into signed-off reviews, with the queue explaining its own order | 10 min |
| [Run Fathom beside the agent](run-beside-the-agent.md) | a second terminal that keeps itself current while the harness drives | 5 min |
| [Connect a harness](connect-a-harness.md) | hook events arriving, so *intent* stops being a guess, and how to prove they arrived | 10 min |
| [Gate a pipeline](gate-a-pipeline.md) | the exit codes a CI job can branch on today, and the one verb they are waiting for | 15 min |
| [Export a review for a pull request](export-for-a-pr.md) | markdown for the PR, HTML for the audit, JSON for a program, one record underneath | 5 min |
| [Silence a finding](silence-a-finding.md) | a detector that stops crying wolf, by the narrowest rule that works | 5 min |
| [Let an agent ask](ask-an-agent.md) | the delta document as an agent's input, and why that is not the same as trusting it | 10 min |

If you have not done the [quickstart](../quickstart.md), start there: every recipe assumes you know what an
episode is, and the quickstart is five minutes.

**Not here on purpose**: installing Fathom ([quickstart](../quickstart.md) §0), the keymap
([cheatsheet](../reference/cheatsheet.md)), the flags ([CLI reference](../reference/cli.md)), and what to do when it is broken
([troubleshooting](../troubleshooting.md)).
